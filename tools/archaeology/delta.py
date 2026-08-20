#!/usr/bin/env python3
"""
delta.py — Historical delta atlas tool (§10, §9).

For a given project, compares two adjacent-version API JSON files (produced
by apiatlas.py) and produces a structured delta record of:

  ADDED_SYMBOLS        — functions/globals present in new but not old
  REMOVED_SYMBOLS      — functions/globals present in old but not new
  SIGNATURE_CHANGES    — functions present in both but with different params/type
  STRUCT_SIZE_CHANGES  — records present in both with different field layouts
  ENUM_CHANGES         — enum values added/removed/changed
  GLOBAL_CHANGES       — globals present in one but not the other

Output is written to:
  atlas/deltas/<project>/<old_version>-<new_version>.json

Usage:
  delta.py libxml2 <old-version> <new-version>
  delta.py libxslt <old-version> <new-version>
  delta.py libxml2 2.6.32 2.9.14
"""

import json
import os
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent


def load_api(project, version):
    """Load an API JSON record."""
    path = ROOT / "atlas" / "api" / project / f"{version}.json"
    if not path.exists():
        print(f"ERROR: API record not found: {path}", file=sys.stderr)
        sys.exit(1)
    with open(path) as f:
        return json.load(f)


def build_function_map(record):
    """Build a name-indexed map of functions for efficient comparison."""
    funcs = {}
    for f in record.get("functions", []):
        name = f.get("name")
        if name:
            funcs[name] = f
    return funcs


def build_typedef_map(record):
    """Build a name-indexed map of typedefs."""
    tds = {}
    for td in record.get("typedefs", []):
        name = td.get("name")
        if name:
            tds[name] = td
    return tds


def build_record_map(record):
    """Build a name-indexed map of struct/union records."""
    recs = {}
    for r in record.get("records", []):
        name = r.get("name")
        if name:
            recs[name] = r
    return recs


def build_enum_map(record):
    """Build a name-indexed map of enums with their values."""
    enums = {}
    for e in record.get("enums", []):
        name = e.get("name") or e.get("typedef_name")
        if name:
            enums[name] = e
    return enums


def build_global_map(record):
    """Build a name-indexed map of globals."""
    globs = {}
    for g in record.get("globals", []):
        name = g.get("name")
        if name:
            globs[name] = g
    return globs


def function_signature(f):
    """Return a canonical signature string for a function."""
    params = f.get("params", [])
    param_strs = []
    for p in params:
        ptype = p.get("type", "")
        pname = p.get("name", "")
        if pname:
            param_strs.append(f"{ptype} {pname}")
        else:
            param_strs.append(ptype)
    return f"{f.get('type', 'void')} ({', '.join(param_strs)})" + \
           (" ..." if f.get("isVariadic") else "")


def compare_functions(old_funcs, new_funcs):
    """Compare function sets, returning added, removed, and changed."""
    old_names = set(old_funcs.keys())
    new_names = set(new_funcs.keys())

    added = sorted(new_names - old_names)
    removed = sorted(old_names - new_names)

    changed = []
    for name in sorted(old_names & new_names):
        old_sig = function_signature(old_funcs[name])
        new_sig = function_signature(new_funcs[name])
        if old_sig != new_sig:
            changed.append({
                "name": name,
                "old_signature": old_sig,
                "new_signature": new_sig,
                "old_header": old_funcs[name].get("header"),
                "new_header": new_funcs[name].get("header"),
            })

    return added, removed, changed


def compare_records(old_recs, new_recs):
    """Compare struct/union records, reporting field layout changes."""
    old_names = set(old_recs.keys())
    new_names = set(new_recs.keys())

    added = sorted(new_names - old_names)
    removed = sorted(old_names - new_names)

    changed = []
    for name in sorted(old_names & new_names):
        old_fields = old_recs[name].get("fields", [])
        new_fields = new_recs[name].get("fields", [])
        old_field_sigs = [(f.get("name"), f.get("type")) for f in old_fields]
        new_field_sigs = [(f.get("name"), f.get("type")) for f in new_fields]
        if old_field_sigs != new_field_sigs:
            changed.append({
                "name": name,
                "old_tag": old_recs[name].get("tagUsed"),
                "new_tag": new_recs[name].get("tagUsed"),
                "old_fields": old_field_sigs,
                "new_fields": new_field_sigs,
                "old_field_count": len(old_fields),
                "new_field_count": len(new_fields),
            })

    return added, removed, changed


def compare_enums(old_enums, new_enums):
    """Compare enums, reporting value changes."""
    old_names = set(old_enums.keys())
    new_names = set(new_enums.keys())

    added = sorted(new_names - old_names)
    removed = sorted(old_names - new_names)

    changed = []
    for name in sorted(old_names & new_names):
        old_vals = {(v.get("name"), v.get("value")) for v in old_enums[name].get("values", [])}
        new_vals = {(v.get("name"), v.get("value")) for v in new_enums[name].get("values", [])}
        if old_vals != new_vals:
            old_val_map = {v.get("name"): v.get("value") for v in old_enums[name].get("values", [])}
            new_val_map = {v.get("name"): v.get("value") for v in new_enums[name].get("values", [])}
            added_vals = sorted(set(new_val_map.keys()) - set(old_val_map.keys()))
            removed_vals = sorted(set(old_val_map.keys()) - set(new_val_map.keys()))
            changed_vals = sorted(
                k for k in set(old_val_map.keys()) & set(new_val_map.keys())
                if old_val_map[k] != new_val_map[k]
            )
            if added_vals or removed_vals or changed_vals:
                changed.append({
                    "name": name,
                    "added_values": added_vals,
                    "removed_values": removed_vals,
                    "changed_values": [{
                        "name": k,
                        "old_value": old_val_map[k],
                        "new_value": new_val_map[k],
                    } for k in changed_vals],
                })

    return added, removed, changed


def compare_globals(old_globs, new_globs):
    """Compare global variable sets."""
    old_names = set(old_globs.keys())
    new_names = set(new_globs.keys())

    added = sorted(new_names - old_names)
    removed = sorted(old_names - new_names)

    changed = []
    for name in sorted(old_names & new_names):
        old_type = old_globs[name].get("type")
        new_type = new_globs[name].get("type")
        if old_type != new_type:
            changed.append({
                "name": name,
                "old_type": old_type,
                "new_type": new_type,
            })

    return added, removed, changed


def compare_typedefs(old_tds, new_tds):
    """Compare typedef sets."""
    old_names = set(old_tds.keys())
    new_names = set(new_tds.keys())

    added = sorted(new_names - old_names)
    removed = sorted(old_names - new_names)

    changed = []
    for name in sorted(old_names & new_names):
        old_type = old_tds[name].get("type")
        new_type = new_tds[name].get("type")
        if old_type != new_type:
            changed.append({
                "name": name,
                "old_type": old_type,
                "new_type": new_type,
            })

    return added, removed, changed


def compute_delta(project, old_version, new_version):
    """Compute structured delta between two API versions."""
    old_rec = load_api(project, old_version)
    new_rec = load_api(project, new_version)

    old_funcs = build_function_map(old_rec)
    new_funcs = build_function_map(new_rec)
    old_tds = build_typedef_map(old_rec)
    new_tds = build_typedef_map(new_rec)
    old_recs = build_record_map(old_rec)
    new_recs = build_record_map(new_rec)
    old_enums = build_enum_map(old_rec)
    new_enums = build_enum_map(new_rec)
    old_globs = build_global_map(old_rec)
    new_globs = build_global_map(new_rec)

    func_added, func_removed, func_changed = compare_functions(old_funcs, new_funcs)
    rec_added, rec_removed, rec_changed = compare_records(old_recs, new_recs)
    enum_added, enum_removed, enum_changed = compare_enums(old_enums, new_enums)
    glob_added, glob_removed, glob_changed = compare_globals(old_globs, new_globs)
    td_added, td_removed, td_changed = compare_typedefs(old_tds, new_tds)

    # Summary statistics
    old_func_count = len(old_funcs)
    new_func_count = len(new_funcs)
    old_rec_count = len(old_recs)
    new_rec_count = len(new_recs)
    old_enum_count = len(old_enums)
    new_enum_count = len(new_enums)
    old_glob_count = len(old_globs)
    new_glob_count = len(new_globs)
    old_td_count = len(old_tds)
    new_td_count = len(new_tds)

    delta = {
        "project": project,
        "old_version": old_version,
        "new_version": new_version,
        "old_tag": old_rec.get("version_tag"),
        "new_tag": new_rec.get("version_tag"),
        "generator": "tools/archaeology/delta.py",
        "summary": {
            "old_functions": old_func_count,
            "new_functions": new_func_count,
            "functions_added": len(func_added),
            "functions_removed": len(func_removed),
            "functions_changed": len(func_changed),
            "old_records": old_rec_count,
            "new_records": new_rec_count,
            "records_added": len(rec_added),
            "records_removed": len(rec_removed),
            "records_changed": len(rec_changed),
            "old_enums": old_enum_count,
            "new_enums": new_enum_count,
            "enums_added": len(enum_added),
            "enums_removed": len(enum_removed),
            "enums_changed": len(enum_changed),
            "old_globals": old_glob_count,
            "new_globals": new_glob_count,
            "globals_added": len(glob_added),
            "globals_removed": len(glob_removed),
            "globals_changed": len(glob_changed),
            "old_typedefs": old_td_count,
            "new_typedefs": new_td_count,
            "typedefs_added": len(td_added),
            "typedefs_removed": len(td_removed),
            "typedefs_changed": len(td_changed),
        },
        "functions_added": [{"name": n, "type": new_funcs.get(n, {}).get("type"),
                             "header": new_funcs.get(n, {}).get("header")}
                            for n in func_added],
        "functions_removed": [{"name": n, "type": old_funcs.get(n, {}).get("type"),
                               "header": old_funcs.get(n, {}).get("header")}
                              for n in func_removed],
        "functions_changed": func_changed,
        "records_added": [{"name": n, "tag": new_recs.get(n, {}).get("tagUsed"),
                           "fields": new_recs.get(n, {}).get("fields", [])}
                          for n in rec_added],
        "records_removed": [{"name": n, "tag": old_recs.get(n, {}).get("tagUsed"),
                             "fields": old_recs.get(n, {}).get("fields", [])}
                            for n in rec_removed],
        "records_changed": rec_changed,
        "enums_added": [{"name": n, "values": new_enums.get(n, {}).get("values", [])}
                        for n in enum_added],
        "enums_removed": [{"name": n, "values": old_enums.get(n, {}).get("values", [])}
                          for n in enum_removed],
        "enums_changed": enum_changed,
        "globals_added": [{"name": n, "type": new_globs.get(n, {}).get("type"),
                           "header": new_globs.get(n, {}).get("header")}
                          for n in glob_added],
        "globals_removed": [{"name": n, "type": old_globs.get(n, {}).get("type"),
                             "header": old_globs.get(n, {}).get("header")}
                            for n in glob_removed],
        "globals_changed": glob_changed,
        "typedefs_added": [{"name": n, "type": new_tds.get(n, {}).get("type")}
                           for n in td_added],
        "typedefs_removed": [{"name": n, "type": old_tds.get(n, {}).get("type")}
                             for n in td_removed],
        "typedefs_changed": td_changed,
    }

    return delta


def print_report(delta):
    """Print a human-readable delta report."""
    s = delta["summary"]
    print(f"{'='*70}")
    print(f"API Delta: {delta['project']} {delta['old_version']} → {delta['new_version']}")
    print(f"{'='*70}")
    print(f"  Functions:  {s['old_functions']} → {s['new_functions']} "
          f"(+{s['functions_added']}, -{s['functions_removed']}, "
          f"~{s['functions_changed']} changed)")
    print(f"  Records:    {s['old_records']} → {s['new_records']} "
          f"(+{s['records_added']}, -{s['records_removed']}, "
          f"~{s['records_changed']} changed)")
    print(f"  Enums:      {s['old_enums']} → {s['new_enums']} "
          f"(+{s['enums_added']}, -{s['enums_removed']}, "
          f"~{s['enums_changed']} changed)")
    print(f"  Globals:    {s['old_globals']} → {s['new_globals']} "
          f"(+{s['globals_added']}, -{s['globals_removed']}, "
          f"~{s['globals_changed']} changed)")
    print(f"  Typedefs:   {s['old_typedefs']} → {s['new_typedefs']} "
          f"(+{s['typedefs_added']}, -{s['typedefs_removed']}, "
          f"~{s['typedefs_changed']} changed)")

    if delta["functions_added"]:
        print(f"\n  ADDED FUNCTIONS ({len(delta['functions_added'])}):")
        for f in delta["functions_added"][:20]:
            print(f"    {f['name']}  ({f.get('header', '?')})")
        if len(delta["functions_added"]) > 20:
            print(f"    ... and {len(delta['functions_added']) - 20} more")

    if delta["functions_removed"]:
        print(f"\n  REMOVED FUNCTIONS ({len(delta['functions_removed'])}):")
        for f in delta["functions_removed"][:20]:
            print(f"    {f['name']}  ({f.get('header', '?')})")
        if len(delta["functions_removed"]) > 20:
            print(f"    ... and {len(delta['functions_removed']) - 20} more")

    if delta["functions_changed"]:
        print(f"\n  CHANGED FUNCTIONS ({len(delta['functions_changed'])}):")
        for f in delta["functions_changed"][:10]:
            print(f"    {f['name']}:")
            print(f"      old: {f['old_signature']}")
            print(f"      new: {f['new_signature']}")

    if delta["records_changed"]:
        print(f"\n  CHANGED RECORDS ({len(delta['records_changed'])}):")
        for r in delta["records_changed"][:10]:
            print(f"    {r['name']}: {r['old_field_count']} → {r['new_field_count']} fields")

    if delta["enums_changed"]:
        print(f"\n  CHANGED ENUMS ({len(delta['enums_changed'])}):")
        for e in delta["enums_changed"][:10]:
            print(f"    {e['name']}:")
            if e.get("added_values"):
                print(f"      + {e['added_values']}")
            if e.get("removed_values"):
                print(f"      - {e['removed_values']}")
            for c in e.get("changed_values", []):
                print(f"      ~ {c['name']}: {c['old_value']} → {c['new_value']}")

    print(f"\n{'='*70}")


def main():
    if len(sys.argv) < 4:
        print(__doc__)
        sys.exit(1)

    project = sys.argv[1]
    old_version = sys.argv[2]
    new_version = sys.argv[3]

    delta = compute_delta(project, old_version, new_version)
    print_report(delta)

    # Write structured output
    out_dir = ROOT / "atlas" / "deltas" / project
    out_dir.mkdir(parents=True, exist_ok=True)
    out_path = out_dir / f"{old_version}-{new_version}.json"
    with open(out_path, "w") as f:
        json.dump(delta, f, indent=2, sort_keys=True)
    print(f"\nStructured delta written to: {out_path}")


if __name__ == "__main__":
    main()
