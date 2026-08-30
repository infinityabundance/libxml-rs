#!/usr/bin/env python3
"""Surface reconciliation engine (11.1-F).

Reconciles the Doxygen-derived surface inventory against every independent
surface oracle for the current versions:

  Doxygen XML  (oracle/historical/doxygen/<proj>-system/inventory-public.json)
  Clang AST    (atlas/api/<proj>/<ver>.json, from apiatlas.py)
  Raw DSO      (readelf dynamic symbols of the system + candidate libraries)
  Installed    (installed public header file list)

Any entity that appears in only one source is classified (Doxygen-only,
AST-only, DSO-only, header-only) and recorded — never silently dropped.
Disagreements between extractors are residuals, not overrides.

Output: atlas/SURFACE_RECONCILIATION.json (committed evidence).

Usage:
  surface_reconcile.py
"""
import json
import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
DOX = os.path.join(ROOT, "oracle", "historical", "doxygen")
ATLAS = os.path.join(ROOT, "atlas")

PROFILES = {
    "libxml2": {"version": "2.15.3", "system_dso": "/usr/lib/libxml2.so.16",
                "cand_dso": os.path.join(ROOT, "target", "debug", "libxml2.so.16.1.3"),
                "headers_dir": "/usr/include/libxml2/libxml"},
    "libxslt": {"version": "1.1.45", "system_dso": "/usr/lib/libxslt.so.1",
                "cand_dso": os.path.join(ROOT, "target", "debug", "libxslt.so.1.1.45"),
                "headers_dir": "/usr/include/libxslt"},
}

# names that legitimately differ between sources (compiler-generated, tooling)
SKIP = {"_fini", "_init", "_start", "_end", "_edata", "__bss_start", "main"}


def dso_symbols(path):
    """Defined dynamic-symbol names (FUNC + OBJECT), GLOBAL bind, DEFAULT vis.

    UND (undefined/import) entries and version suffixes are excluded: only
    symbols the library itself defines are part of its surface."""
    if not os.path.exists(path):
        return None
    funcs, objs = set(), set()
    r = subprocess.run(["readelf", "-s", "--wide", path], capture_output=True, text=True)
    for line in r.stdout.splitlines():
        parts = line.split()
        if len(parts) < 8 or parts[0] == "Symbol" or parts[0].startswith("["):
            continue
        # [Num] Value Size Type Bind Vis Ndx Name
        typ, bind, vis, ndx = parts[3], parts[4], parts[5], parts[6]
        if bind != "GLOBAL" or vis != "DEFAULT" or ndx == "UND":
            continue
        name = parts[-1].split("@")[0]
        if typ == "FUNC":
            funcs.add(name)
        elif typ == "OBJECT":
            objs.add(name)
    return funcs, objs


def load_dox(project):
    p = os.path.join(DOX, f"{project}-system", "inventory-public.json")
    if not os.path.exists(p):
        return None
    with open(p) as f:
        return json.load(f)


def load_ast(project, version):
    p = os.path.join(ATLAS, "api", project, f"{version}.json")
    if not os.path.exists(p):
        return None
    with open(p) as f:
        return json.load(f)


def installed_headers(project):
    d = PROFILES[project]["headers_dir"]
    if not os.path.isdir(d):
        return set()
    return {f for f in os.listdir(d) if f.endswith(".h")}


def reconcile(project):
    info = PROFILES[project]
    dox = load_dox(project)
    ast = load_ast(project, info["version"])
    sys_funcs, sys_objs = dso_symbols(info["system_dso"]) or (set(), set())
    cand_funcs, cand_objs = dso_symbols(info["cand_dso"]) or (set(), set())
    hdrs = installed_headers(project)
    if dox is None or ast is None:
        return {"project": project, "error": "missing input"}

    dox_funcs = {e["name"] for e in dox["entities"] if e["kind"] == "function"}
    dox_globals = {e["name"] for e in dox["entities"] if e["kind"] == "variable"}
    dox_typedefs = {e["name"] for e in dox["entities"] if e["kind"] == "typedef"}
    dox_records = {e["name"] for e in dox["entities"] if e["kind"] in ("struct", "union")}
    dox_headers = {e.get("header") for e in dox["entities"] if e.get("header")}

    ast_funcs = {f["name"] for f in ast["functions"]}
    ast_globals = {g["name"] for g in ast["globals"]}
    ast_typedefs = {t["name"] for t in ast["typedefs"]}
    ast_records = {r["name"] for r in ast["records"]}
    ast_headers = set()
    if isinstance(ast.get("headers"), list):
        ast_headers = {h if isinstance(h, str) else str(h) for h in ast["headers"]}

    report = {
        "project": project,
        "version": info["version"],
        "doxygen_inventory_hash": dox["inventory_hash"],
        "sources": {
            "doxygen_functions": len(dox_funcs),
            "clang_ast_functions": len(ast_funcs),
            "system_dso_functions": len(sys_funcs) if sys_funcs else None,
            "candidate_dso_functions": len(cand_funcs) if cand_funcs else None,
            "installed_headers": len(hdrs),
        },
        "functions": {
            "in_both_dox_ast": sorted(dox_funcs & ast_funcs),
            "doxygen_only": sorted(dox_funcs - ast_funcs),
            "ast_only": sorted(ast_funcs - dox_funcs),
        },
        "globals": {
            "in_both_dox_ast": sorted(dox_globals & ast_globals),
            "doxygen_only": sorted(dox_globals - ast_globals),
            "ast_only": sorted(ast_globals - dox_globals),
        },
        "typedefs": {
            "in_both": sorted(dox_typedefs & ast_typedefs),
            "doxygen_only": sorted(dox_typedefs - ast_typedefs),
            "ast_only": sorted(ast_typedefs - dox_typedefs),
        },
        "records": {
            "in_both": sorted(dox_records & ast_records),
            "doxygen_only": sorted(dox_records - ast_records),
            "ast_only": sorted(ast_records - dox_records),
        },
        "dsos": {
            "system_dso_functions_not_in_dox": sorted(
                {s for s in (sys_funcs or set()) if s not in dox_funcs and s not in SKIP}),
            "candidate_missing_vs_system": sorted(
                {s for s in (sys_funcs or set()) if s not in (cand_funcs or set()) and s not in SKIP}),
            "candidate_extra_vs_system": sorted(
                {s for s in (cand_funcs or set()) if s not in (sys_funcs or set()) and s not in SKIP}),
            "candidate_globals_missing_vs_system": sorted(
                {s for s in (sys_objs or set()) if s not in (cand_objs or set()) and s not in SKIP}),
            "note": "The candidate is a single cdylib serving both libxml2 and libxslt "
                    "SONAMEs, so 'candidate_extra_vs_system' for libxslt contains the "
                    "libxml2 surface; the libxml2 row is the authoritative parity gap "
                    "for the candidate. Missing functions are the parity-obligation "
                    "census input (11.1-I), not silently dropped.",
        },
        "headers": {
            "doxygen_headers": sorted(dox_headers),
            "installed_headers": sorted(hdrs),
            "dox_not_installed": sorted(dox_headers - hdrs),
            "installed_not_dox": sorted(hdrs - dox_headers),
        },
    }
    # verdict: everything classified; discrepancies are recorded residuals
    report["verdict"] = "RECONCILED (discrepancies classified below)"
    return report


def main():
    out = {"schema": "surface-reconciliation-1", "projects": {}}
    for project in ("libxml2", "libxslt"):
        r = reconcile(project)
        out["projects"][project] = r
        print(f"═══ {project} ═══")
        print(f"  functions: dox={r['sources']['doxygen_functions']} "
              f"ast={r['sources']['clang_ast_functions']} "
              f"dso={r['sources']['system_dso_functions']}")
        print(f"  dox-only functions: {len(r['functions']['doxygen_only'])} "
              f"ast-only: {len(r['functions']['ast_only'])}")
        print(f"  candidate DSO vs system: missing "
              f"{len(r['dsos']['candidate_missing_vs_system'])}, "
              f"extra {len(r['dsos']['candidate_extra_vs_system'])}")
        print(f"  headers: dox {len(r['headers']['doxygen_headers'])} "
              f"installed {len(r['headers']['installed_headers'])}")
        for name in list(r["functions"]["doxygen_only"])[:6]:
            print(f"    dox-only fn: {name}")
        for name in list(r["functions"]["ast_only"])[:6]:
            print(f"    ast-only fn: {name}")
    path = os.path.join(ATLAS, "SURFACE_RECONCILIATION.json")
    with open(path, "w") as f:
        json.dump(out, f, indent=1, ensure_ascii=False)
        f.write("\n")
    print("reconciliation ->", path)
    return 0


if __name__ == "__main__":
    sys.exit(main())
