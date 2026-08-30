#!/usr/bin/env python3
"""Canonical surface graph (11.1-E).

Synthesizes the stable, machine-readable entity graph of the libxml2/libxslt
ecosystem from the Doxygen inventories (public + full profiles), the surface
epochs (first-seen/last-seen/signature changes), the condition universe
(conditional gates by header), and the candidate's exported symbols (mapping).
Every entity gets a stable ID and a classification; nothing is discarded —
internal/static/dead entities are classified, not dropped.

Output: atlas/SURFACE_GRAPH.json (committed evidence).

Usage:
  surface_graph.py
"""
import json
import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
DOX = os.path.join(ROOT, "oracle", "historical", "doxygen")
ATLAS = os.path.join(ROOT, "atlas")

XML2_VERSIONS = ["2.7.8", "2.8.0", "2.9.4", "2.9.10", "2.9.14", "2.10.4",
                 "2.11.5", "2.12.6", "2.13.0", "2.13.5", "2.14.1", "2.15.0"]
XSLT_VERSIONS = ["1.1.26", "1.1.32", "1.1.35", "1.1.38", "1.1.42"]

PUBLIC_API = "PUBLIC_API"
PUBLIC_ABI = "PUBLIC_ABI"
EXPORTED_UNDOCUMENTED = "EXPORTED_UNDOCUMENTED"
HEADER_VISIBLE_INTERNAL = "HEADER_VISIBLE_INTERNAL"
CALLBACK_INTERFACE = "CALLBACK_INTERFACE"
DATA_LAYOUT_INTERFACE = "DATA_LAYOUT_INTERFACE"
PRIVATE_IMPLEMENTATION = "PRIVATE_IMPLEMENTATION"
BUILD_INTERFACE = "BUILD_INTERFACE"
DEAD_OR_HISTORICAL = "DEAD_OR_HISTORICAL"


def load_inv(project, version, profile):
    p = os.path.join(DOX, f"{project}-{version}", f"inventory-{profile}.json")
    if not os.path.exists(p):
        return None
    with open(p) as f:
        return json.load(f)


def load_epochs(project, profile):
    p = os.path.join(DOX, f"{project}-{profile}-surface-epochs.json")
    if not os.path.exists(p):
        return None
    with open(p) as f:
        return json.load(f)


def load_conditions():
    p = os.path.join(DOX, "conditions.json")
    if not os.path.exists(p):
        return {}
    with open(p) as f:
        return json.load(f)


def candidate_symbols():
    """Exported symbol names of the candidate DSOs."""
    out = {"libxml2": set(), "libxslt": set()}
    for proj, soname in (("libxml2", "libxml2.so.16.1.3"),
                         ("libxslt", "libxslt.so.1.1.45")):
        path = os.path.join(ROOT, "target", "debug", soname)
        if not os.path.exists(path):
            continue
        r = subprocess.run(["readelf", "-s", "--wide", path],
                           capture_output=True, text=True)
        for line in r.stdout.splitlines():
            parts = line.split()
            if len(parts) >= 8 and parts[4] == "FUNC" and parts[7] == "GLOBAL":
                name = parts[-1]
                if name.startswith("__"):
                    continue
                out[proj].add(name)
    return out


def classify(project, e, public_headers, cand_syms, current):
    kind = e["kind"]
    name = e["name"]
    header = e.get("header") or ""
    is_public = header in public_headers
    documented = e.get("documented", False)
    last = e.get("last_seen", current)
    dead = last < current

    if kind in ("function", "variable", "typedef", "struct", "union", "enum", "enumvalue", "define"):
        if kind == "function":
            if e.get("static"):
                return PRIVATE_IMPLEMENTATION
            if not is_public:
                return HEADER_VISIBLE_INTERNAL if header else PRIVATE_IMPLEMENTATION
            if dead:
                return DEAD_OR_HISTORICAL
            if not documented:
                return EXPORTED_UNDOCUMENTED
            return PUBLIC_API
        if kind == "variable":
            if e.get("static"):
                return PRIVATE_IMPLEMENTATION
            if not is_public:
                return HEADER_VISIBLE_INTERNAL
            return PUBLIC_ABI if name in cand_syms.get(project, set()) else DATA_LAYOUT_INTERFACE
        if kind == "typedef":
            if not is_public:
                return HEADER_VISIBLE_INTERNAL
            if "(*" in e.get("type", "") or "Handler" in name or "Callback" in name:
                return CALLBACK_INTERFACE
            return PUBLIC_API
        if kind in ("struct", "union"):
            if not is_public:
                return HEADER_VISIBLE_INTERNAL
            return DATA_LAYOUT_INTERFACE
        if kind in ("enum", "enumvalue"):
            return PUBLIC_API if is_public else HEADER_VISIBLE_INTERNAL
        if kind == "define":
            return BUILD_INTERFACE if re.match(r"^(LIBXML|LIBXSLT|XML_|XSLT_|EXSLT)", name) or not is_public else PUBLIC_API
    return HEADER_VISIBLE_INTERNAL


def main():
    cand_syms = candidate_symbols()
    conditions = load_conditions()
    graph = {"schema": "surface-graph-1", "entities": {}, "counts": {}}

    for project, versions in (("libxml2", XML2_VERSIONS), ("libxslt", XSLT_VERSIONS)):
        current = versions[-1]
        inv_public = load_inv(project, current, "public")
        inv_full = load_inv(project, current, "full")
        if inv_public is None:
            print(f"missing current inventory for {project}")
            continue
        public_headers = set()
        for e in inv_public["entities"]:
            h = e.get("header")
            if h:
                public_headers.add(h)
        epochs = load_epochs(project, "public")
        fsl = epochs["first_seen_last_seen"] if epochs else {}

        # build full-profile map (static/private detail + source-side doc blocks:
        # libxml2 documents functions in the .c files, not the headers)
        full_map = {}
        if inv_full:
            for e in inv_full["entities"]:
                full_map.setdefault((e["kind"], e["name"], e.get("header") or ""), []).append(e)

        for e in inv_public["entities"]:
            kind, name, header = e["kind"], e["name"], e.get("header") or ""
            key = (kind, name, header, e.get("struct") or "", e.get("enum") or "")
            kstr = json.dumps(list(key), sort_keys=True)
            fs = fsl.get(kstr, {"first": current, "last": current,
                                "signature_changes": []})
            static = False
            documented = e.get("documented", False)
            fentries = full_map.get((kind, name, header), [])
            if not fentries:
                # definition lives in the .c file (parser.c vs parser.h): join by
                # (kind, name) when the header-based key misses
                fentries = [fe for k, fe in full_map.items()
                            if k[0] == kind and k[1] == name][0] if any(
                    k[0] == kind and k[1] == name for k in full_map) else []
            if fentries:
                static = any(f.get("static") for f in fentries)
                # source-side doc blocks (parser.c etc.) are the real docs
                documented = documented or any(f.get("documented") for f in fentries)
            e2 = dict(e)
            e2["static"] = static or e2.get("static", False)
            e2["documented"] = documented

            gates = []
            for cid, rec in sorted(conditions.items()):
                if rec["project"] == project and any(
                        f.endswith(header) for f in rec["files"]):
                    if header and rec["public_header"]:
                        gates.append(rec["expression"])
                    elif header and not rec["public_header"]:
                        pass
                    elif any(f == header for f in rec["files"]):
                        gates.append(rec["expression"])
            gates = sorted(set(gates))[:20]

            cls = classify(project, e2, public_headers, cand_syms, current)
            sid = f"{project}:{kind}:{name}"
            if sid in graph["entities"] and kind == "function":
                sid = f"{project}:{kind}:{name}:{header}"
            ent = {
                "stable_id": sid,
                "project": project,
                "kind": kind,
                "name": name,
                "header": header,
                "source_path": header,
                "signature": e2.get("args") or e2.get("type") or e2.get("value") or "",
                "documented": e2.get("documented", False),
                "static": static,
                "first_seen": fs.get("first", current),
                "last_seen": fs.get("last", current),
                "signature_changes": fs.get("signature_changes", []),
                "classification": cls,
                "conditional_gates": gates,
                "candidate_mapping": name if name in cand_syms.get(project, set()) else None,
            }
            graph["entities"][sid] = ent

        # full-profile-only entities (internals/statics not in public inventory)
        if inv_full:
            public_keys = {(e["kind"], e["name"], e.get("header") or "")
                           for e in inv_public["entities"]}
            for e in inv_full["entities"]:
                k = (e["kind"], e["name"], e.get("header") or "")
                if k in public_keys:
                    continue
                sid = f"{project}:{kind}:{name}" if False else \
                    f"{project}:{e['kind']}:{e['name']}:{e.get('header') or '?'}"
                if sid in graph["entities"]:
                    sid = sid + f":{len(graph['entities'])}"
                cls = PRIVATE_IMPLEMENTATION if e.get("static") else HEADER_VISIBLE_INTERNAL
                graph["entities"][sid] = {
                    "stable_id": sid, "project": project, "kind": e["kind"],
                    "name": e["name"], "header": e.get("header") or "",
                    "source_path": e.get("header") or "",
                    "signature": e.get("args") or e.get("type") or "",
                    "documented": e.get("documented", False),
                    "static": e.get("static", False),
                    "first_seen": current, "last_seen": current,
                    "signature_changes": [], "classification": cls,
                    "conditional_gates": [], "candidate_mapping": None,
                }

    counts = {}
    for e in graph["entities"].values():
        counts.setdefault(e["classification"], 0)
        counts[e["classification"]] += 1
    graph["counts"] = counts
    graph["generator"] = "tools/archaeology/surface_graph.py"
    out = os.path.join(ATLAS, "SURFACE_GRAPH.json")
    with open(out, "w") as f:
        json.dump(graph, f, indent=1, ensure_ascii=False)
        f.write("\n")
    print(f"surface graph -> {out}: {len(graph['entities'])} entities")
    for k, v in sorted(counts.items()):
        print(f"  {k}: {v}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
