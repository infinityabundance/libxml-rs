#!/usr/bin/env python3
"""Doxygen surface deltas across historical releases (11.1-Q surface epochs).

Diffs adjacent version inventories: entity added/removed/changed (kind, name,
signature/type/value), and produces the first-seen/last-seen table for every
entity over the whole span. Output is deterministic JSON + a human summary.

Usage:
  doxygen_diff.py <project> [profile] [versions...]
    (versions default to the built-oracle registry from the matrix)
"""
import json
import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
DOX = os.path.join(ROOT, "oracle", "historical", "doxygen")

XML2_VERSIONS = ["2.7.8", "2.8.0", "2.9.4", "2.9.10", "2.9.14", "2.10.4",
                 "2.11.5", "2.12.6", "2.13.0", "2.13.5", "2.14.1", "2.15.0"]
XSLT_VERSIONS = ["1.1.26", "1.1.32", "1.1.35", "1.1.38", "1.1.42"]


def key(e):
    return (e["kind"], e["name"], e.get("header") or "", e.get("struct") or "",
            e.get("enum") or "")


def signature(e):
    if e["kind"] == "function":
        return e.get("args", "")
    if e["kind"] in ("variable", "typedef", "field"):
        return e.get("type", "")
    return e.get("value", "")


def load_inv(project, version, profile):
    p = os.path.join(DOX, f"{project}-{version}", f"inventory-{profile}.json")
    if not os.path.exists(p):
        return None
    with open(p) as f:
        return json.load(f)


def diff_pair(a, b):
    """Returns (added, removed, changed) entity lists a->b."""
    ka = {key(e): e for e in a["entities"]}
    kb = {key(e): e for e in b["entities"]}
    added = [e for k, e in kb.items() if k not in ka]
    removed = [e for k, e in ka.items() if k not in kb]
    changed = []
    for k, e in kb.items():
        if k in ka and signature(ka[k]) != signature(e):
            changed.append({"kind": k[0], "name": k[1], "from": signature(ka[k]),
                            "to": signature(e), "header": k[2]})
    return added, removed, changed


def run(project, profile="public", versions=None):
    if versions is None:
        versions = XML2_VERSIONS if project == "libxml2" else XSLT_VERSIONS
    invs = {}
    for v in versions:
        inv = load_inv(project, v, profile)
        if inv:
            invs[v] = inv
        else:
            print(f"missing inventory: {project}-{v} [{profile}]")

    vs = sorted(invs.keys(), key=lambda v: [int(x) for x in re.split(r"[.\-]", v) if x.isdigit()])
    deltas = {}
    for a, b in zip(vs, vs[1:]):
        added, removed, changed = diff_pair(invs[a], invs[b])
        deltas[f"{a}->{b}"] = {
            "added": sorted([key(e) for e in added]),
            "removed": sorted([key(e) for e in removed]),
            "changed": sorted((c["kind"], c["name"], c["from"], c["to"]) for c in changed),
        }

    # first-seen / last-seen over the span
    seen = {}
    for v in vs:
        for e in invs[v]["entities"]:
            k = key(e)
            rec = seen.setdefault(k, {"first": v, "last": v, "signature": signature(e)})
            if rec["signature"] != signature(e):
                rec.setdefault("signature_changes", []).append(
                    {"version": v, "to": signature(e)})
                rec["signature"] = signature(e)
            rec["last"] = v

    result = {
        "schema": "doxygen-diff-1",
        "project": project,
        "profile": profile,
        "versions": vs,
        "inventory_hashes": {v: invs[v]["inventory_hash"] for v in vs},
        "deltas": deltas,
        "first_seen_last_seen": {
            json.dumps(list(k), sort_keys=True): {"first": r["first"], "last": r["last"],
                "signature_changes": r.get("signature_changes", [])}
            for k, r in seen.items()
        },
    }
    out = os.path.join(DOX, f"{project}-{profile}-surface-epochs.json")
    with open(out, "w") as f:
        json.dump(result, f, indent=1, ensure_ascii=False)
        f.write("\n")
    print(f"surface epochs -> {out}")
    print(f"  versions: {len(vs)}  entities tracked: {len(seen)}")
    for dk, d in deltas.items():
        print(f"  {dk}: +{len(d['added'])} -{len(d['removed'])} ~{len(d['changed'])}")
    return 0


if __name__ == "__main__":
    args = sys.argv[1:]
    project = args[0] if args else "libxml2"
    profile = args[1] if len(args) > 1 else "public"
    versions = args[2:] or None
    sys.exit(run(project, profile, versions))
