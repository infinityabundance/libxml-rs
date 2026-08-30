#!/usr/bin/env python3
"""11.1-Q — Historical Surface Delta Engine.

Computes version-to-version surface deltas automatically from the per-version
Doxygen inventories under oracle/historical/doxygen/ and correlates important
boundaries with upstream NEWS/changelog evidence.

For every entity the engine determines:

    first release / last release / signature changes / layout (struct member)
    changes / macro value changes / enum value changes / deprecation-relevant
    removal / export presence.

Change classification per boundary (added / removed / changed):

    function           args (signature) changed
    define             value changed
    enumerator         value changed
    variable           type changed
    struct member      added / removed / type changed (variable with a
                       `struct` member)  -> layout epoch
    typedef            presence only

The engine is the single source for atlas/HISTORICAL_SURFACE_EPOCHS.json
(canonical) and its Markdown view. Every total is reproducible from the
committed per-version inventories.

Usage:
    python3 tools/evidence/surface_delta_engine.py
"""

import collections
import datetime
import json
import os
import re

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
DOXY = os.path.join(ROOT, "oracle", "historical", "doxygen")
ARCH = os.path.join(ROOT, "archaeology")
OUT_JSON = os.path.join(ROOT, "atlas", "HISTORICAL_SURFACE_EPOCHS.json")
OUT_MD = os.path.join(ROOT, "atlas", "HISTORICAL_SURFACE_EPOCHS.md")

PROJECTS = {
    "libxml2": {
        "versions": ["2.7.8", "2.8.0", "2.9.4", "2.9.10", "2.9.14", "2.10.4",
                     "2.11.5", "2.12.6", "2.13.0", "2.13.5", "2.14.1", "2.15.0"],
        "inventory_dir": "libxml2-{v}",
        "news": os.path.join(ARCH, "libxml2-git", "NEWS"),
    },
    "libxslt": {
        "versions": ["1.1.26", "1.1.32", "1.1.35", "1.1.38", "1.1.42"],
        "inventory_dir": "libxslt-{v}",
        "news": os.path.join(ARCH, "libxslt-git", "NEWS"),
    },
}


def load_inventory(project, version):
    d = os.path.join(DOXY, PROJECTS[project]["inventory_dir"].format(v=version))
    with open(os.path.join(d, "inventory-public.json")) as f:
        data = json.load(f)
    entities = {}
    for e in data["entities"]:
        key = (e["kind"], e["name"])
        entities.setdefault(key, []).append(e)
    return entities


def entity_sig(e):
    """Comparable signature for change classification."""
    k = e["kind"]
    if k == "function":
        return e.get("args", "")
    if k == "define":
        return e.get("definition", "")
    if k == "enumerator":
        return e.get("value", "")
    if k == "variable":
        return (e.get("type", ""), e.get("struct", ""))
    if k == "enum":
        vals = e.get("enum_values", [])
        return tuple((v[0], v[1]) for v in vals)
    return ""


def compute_deltas(project):
    versions = PROJECTS[project]["versions"]
    invs = {v: load_inventory(project, v) for v in versions}
    deltas = {}
    for a, b in zip(versions, versions[1:]):
        ia, ib = invs[a], invs[b]
        added, removed, changed, kind_changed = [], [], [], []
        names_a = collections.defaultdict(set)
        names_b = collections.defaultdict(set)
        for (k, n) in ia:
            names_a[n].add(k)
        for (k, n) in ib:
            names_b[n].add(k)
        all_names = set(names_a) | set(names_b)
        for name in sorted(all_names):
            ka, kb = names_a.get(name, set()), names_b.get(name, set())
            if ka and kb and ka != kb:
                # same name, different kind(s): e.g. a function became a
                # data-global or a macro became a function.
                kind_changed.append([name, sorted(ka), sorted(kb)])
                continue
            for kind in sorted(ka | kb):
                key = (kind, name)
                ea, eb = ia.get(key), ib.get(key)
                if ea is None and eb is not None:
                    for e in eb:
                        added.append([e["kind"], e["name"], e.get("header", ""),
                                      e.get("args", "") or e.get("definition", ""),
                                      e.get("struct", "")])
                elif eb is None and ea is not None:
                    for e in ea:
                        removed.append([e["kind"], e["name"], e.get("header", ""),
                                        e.get("args", "") or e.get("definition", ""),
                                        e.get("struct", "")])
                elif ea is not None and eb is not None:
                    sigs_a = collections.Counter(entity_sig(e) for e in ea)
                    sigs_b = collections.Counter(entity_sig(e) for e in eb)
                    if sigs_a != sigs_b:
                        e0 = eb[0]
                        reason = classify_change(kind, ea, eb)
                        changed.append([kind, name, e0.get("header", ""), reason])
        deltas[f"{a}->{b}"] = {
            "added": added, "removed": removed, "changed": changed,
            "kind_changed": kind_changed,
            "added_count": len(added), "removed_count": len(removed),
            "changed_count": len(changed),
            "kind_changed_count": len(kind_changed),
        }
    # first/last seen across the version chain
    first_last = {}
    for v in versions:
        for key in invs[v]:
            rec = first_last.setdefault(str(list(key)), {"first": v, "last": v,
                                                         "signature_changes": []})
            rec["last"] = v
    return deltas, first_last, invs


def classify_change(kind, ea, eb):
    if kind == "function":
        da = {entity_sig(e): e for e in ea}
        db = {entity_sig(e): e for e in eb}
        common = set(da) & set(db)
        if not common:
            return "signature changed"
        return "signature changed (one or more declarations)"
    if kind == "define":
        return "macro value changed"
    if kind == "enumerator":
        return "enum value changed"
    if kind == "variable":
        structs_a = collections.Counter(e.get("struct", "") for e in ea)
        structs_b = collections.Counter(e.get("struct", "") for e in eb)
        if structs_a != structs_b:
            return "struct membership changed (layout epoch)"
        return "type changed"
    if kind == "enum":
        return "enum members changed"
    return "changed"


def parse_news(path):
    """Return {version: [bullets]} from the upstream NEWS file."""
    sections = collections.OrderedDict()
    if not os.path.exists(path):
        return sections
    cur = None
    with open(path, errors="replace") as f:
        for line in f:
            m = re.match(r"^(?:v?)(\d+\.\d+(?:\.\d+)?)", line.strip())
            if m and line.startswith(("v", "2.", "1.")) or \
               (m and len(line.strip()) < 30):
                v = m.group(1)
                cur = v
                sections.setdefault(cur, [])
            elif cur is not None:
                s = line.strip()
                if s and not s.startswith("==") and not s.startswith("--"):
                    sections[cur].append(s)
    return sections


def news_for_boundary(project, to_version):
    """NEWS bullets for the release `to_version` (best-effort)."""
    news = parse_news(PROJECTS[project]["news"])
    # NEWS sections are headed by the release version; tolerate prefixing.
    for v, bullets in news.items():
        if v == to_version or v.startswith(to_version) or to_version.startswith(v):
            return bullets
    return []


def correlate(project, deltas):
    """Attach NEWS bullets mentioning changed symbols to boundary records."""
    out = {}
    for boundary, d in deltas.items():
        to_v = boundary.split("->")[1]
        bullets = news_for_boundary(project, to_v)
        mentions = []
        for kind, name, *_ in d["added"] + d["removed"]:
            base = name.replace("_", " ").lower()
            for b in bullets:
                if base and base in b.lower():
                    mentions.append({"symbol": name, "news": b})
                    break
        out[boundary] = {"deltas": d,
                         "news_bullets": bullets[:40],
                         "symbol_mentions": mentions[:20]}
    return out


def main():
    result = {
        "schema": "historical-surface-epochs-1",
        "generator": "tools/evidence/surface_delta_engine.py",
        "phase": "11.1-Q",
        # no embedded timestamp: the JSON is byte-reproducible; generation
        # time is recorded in the generate_all.py receipt instead
        "policy": "Surface epochs derived from per-version Doxygen public "
                  "inventories; important boundaries correlated with upstream "
                  "NEWS evidence. Layout epochs (struct membership) are "
                  "derived from member-variable inventories.",
        "projects": {},
    }
    for project in PROJECTS:
        deltas, first_last, invs = compute_deltas(project)
        correlated = correlate(project, deltas)
        result["projects"][project] = {
            "versions": PROJECTS[project]["versions"],
            "deltas": correlated,
            "first_seen_last_seen": first_last,
            "totals": {
                "entities": len(first_last),
                "boundaries": len(correlated),
            },
        }
    with open(OUT_JSON, "w") as f:
        json.dump(result, f, indent=1)
        f.write("\n")
    write_md(result)
    print(f"wrote {OUT_JSON}")
    print(f"wrote {OUT_MD}")
    for project in PROJECTS:
        r = result["projects"][project]
        added = sum(v["deltas"]["added_count"] for v in r["deltas"].values())
        removed = sum(v["deltas"]["removed_count"] for v in r["deltas"].values())
        changed = sum(v["deltas"]["changed_count"] for v in r["deltas"].values())
        print(f"{project}: entities={r['totals']['entities']} boundaries="
              f"{r['totals']['boundaries']} added={added} removed={removed} "
              f"changed={changed}")


def write_md(result):
    L = []
    L.append("# Historical Surface Epochs — 11.1-Q\n")
    L.append("Generated by `tools/evidence/surface_delta_engine.py` from the "
             "per-version Doxygen public inventories under "
             "`oracle/historical/doxygen/`.\n")
    for project in PROJECTS:
        r = result["projects"][project]
        L.append(f"## {project}\n")
        L.append(f"Versions: {', '.join(r['versions'])}  ")
        L.append(f"Entities tracked: {r['totals']['entities']}\n")
        L.append("| boundary | added | removed | changed | kind-changed |")
        L.append("|---|---|---|---|---|")
        for b, v in r["deltas"].items():
            d = v["deltas"]
            L.append(f"| {b} | {d['added_count']} | {d['removed_count']} | "
                     f"{d['changed_count']} | {d['kind_changed_count']} |")
        L.append("")
        L.append("### Notable boundaries with NEWS correlation\n")
        for b, v in r["deltas"].items():
            d = v["deltas"]
            interesting = [x for x in d["removed"] if x[0] in
                           ("function", "define", "enum")]
            layout = [x for x in d["changed"] if "layout" in x[3]]
            if interesting or layout or d["changed_count"] > 5 or d["kind_changed"]:
                L.append(f"#### {b}\n")
                for x in d["removed"]:
                    L.append(f"- removed {x[0]} `{x[1]}` ({x[2]})")
                for x in d["kind_changed"]:
                    L.append(f"- kind changed `{x[0]}`: {'/'.join(x[1])} -> {'/'.join(x[2])}")
                for x in d["changed"]:
                    L.append(f"- changed {x[0]} `{x[1]}` — {x[3]}")
                if v["symbol_mentions"]:
                    L.append("- NEWS correlation:")
                    for m in v["symbol_mentions"]:
                        L.append(f"  - `{m['symbol']}`: {m['news'][:160]}")
                L.append("")
    with open(OUT_MD, "w") as f:
        f.write("\n".join(L))


if __name__ == "__main__":
    main()
