#!/usr/bin/env python3
"""Doxygen forensic census orchestrator (11.1-B/11.1-C).

Runs the complete historical Doxygen census: for every catalogued release of
libxml2 and libxslt, generate both extraction profiles (public headers + full
source), run Doxygen, normalize to inventories, then diff adjacent releases
into surface epochs. Records an aggregate census manifest with every identity
hash. Era-incompatible releases get a documented archaeology-failure residual
instead of a silent skip.

Usage:
  run_doxygen_census.py [--profiles public,full] [--project libxml2|libxslt|all]
  run_doxygen_census.py --summarize
"""
import argparse
import hashlib
import json
import os
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
TOOLS = os.path.join(ROOT, "tools", "archaeology")
DOX = os.path.join(ROOT, "oracle", "historical", "doxygen")

XML2_VERSIONS = ["2.7.8", "2.8.0", "2.9.4", "2.9.10", "2.9.14", "2.10.4",
                 "2.11.5", "2.12.6", "2.13.0", "2.13.5", "2.14.1", "2.15.0"]
XSLT_VERSIONS = ["1.1.26", "1.1.32", "1.1.35", "1.1.38", "1.1.42"]


def sh(cmd):
    r = subprocess.run(cmd, capture_output=True, text=True)
    return r.returncode, (r.stdout or "") + (r.stderr or "")


def run_census(project, versions, profiles):
    failures = []
    for v in versions:
        for profile in profiles:
            rc, out = sh([sys.executable, os.path.join(TOOLS, "doxygen_profile.py"),
                          "gen", project, v, profile])
            if rc != 0:
                failures.append({"project": project, "version": v, "profile": profile,
                                 "stage": "profile", "detail": out.strip()[:200]})
                continue
            rc, out = sh([sys.executable, os.path.join(TOOLS, "doxygen_inventory.py"),
                          "run", project, v, profile])
            if rc != 0:
                failures.append({"project": project, "version": v, "profile": profile,
                                 "stage": "doxygen", "detail": out.strip()[:200]})
    for profile in profiles:
        sh([sys.executable, os.path.join(TOOLS, "doxygen_diff.py"), project, profile])
    return failures


def census_manifest(profiles):
    doc = {"schema": "doxygen-census-1", "profiles": profiles, "runs": {},
           "failures": [], "surface_epochs": {}}
    for project in ("libxml2", "libxslt"):
        versions = XML2_VERSIONS if project == "libxml2" else XSLT_VERSIONS
        for v in versions:
            for profile in profiles:
                p = os.path.join(DOX, f"{project}-{v}", f"inventory-{profile}.json")
                if os.path.exists(p):
                    with open(p) as f:
                        inv = json.load(f)
                    doc["runs"][f"{project}-{v}-{profile}"] = {
                        "inventory_hash": inv["inventory_hash"],
                        "raw_xml_hash": inv["raw_xml_hash"],
                        "config_hash": inv["config_hash"],
                        "doxygen_version": inv["doxygen_version"],
                        "source_tree_hash": inv["source_tree_hash"],
                        "counts": inv["counts"],
                    }
    for project in ("libxml2", "libxslt"):
        for profile in profiles:
            p = os.path.join(DOX, f"{project}-{profile}-surface-epochs.json")
            if os.path.exists(p):
                with open(p) as f:
                    se = json.load(f)
                doc["surface_epochs"][f"{project}-{profile}"] = {
                    "versions": se["versions"],
                    "inventory_hashes": se["inventory_hashes"],
                    "delta_counts": {k: {kk: len(vv) for kk, vv in d.items()}
                                     for k, d in se["deltas"].items()},
                }
    with open(os.path.join(DOX, "census.json"), "w") as f:
        json.dump(doc, f, indent=1, ensure_ascii=False)
        f.write("\n")
    print("census manifest ->", os.path.join(DOX, "census.json"))
    return doc


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--profiles", default="public,full")
    ap.add_argument("--project", default="all")
    ap.add_argument("--summarize", action="store_true")
    args = ap.parse_args()
    profiles = [p.strip() for p in args.profiles.split(",")]

    if args.summarize:
        doc = census_manifest(profiles)
        for run_id, r in sorted(doc["runs"].items()):
            print(f"  {run_id}: {r['counts'].get('total', 0)} entities, inv {r['inventory_hash'][:12]}")
        return 0

    projects = ("libxml2", "libxslt") if args.project == "all" else (args.project,)
    all_failures = []
    for project in projects:
        versions = XML2_VERSIONS if project == "libxml2" else XSLT_VERSIONS
        print(f"════ doxygen census: {project} ({len(versions)} releases) ════")
        all_failures += run_census(project, versions, profiles)

    doc = census_manifest(profiles)
    doc["failures"] = all_failures
    with open(os.path.join(DOX, "census.json"), "w") as f:
        json.dump(doc, f, indent=1, ensure_ascii=False)
        f.write("\n")

    if all_failures:
        print("ARCHAEOLOGY FAILURES (documented, not skipped silently):")
        for fl in all_failures:
            print(" ", fl)
        return 1
    print("CENSUS COMPLETE — no archaeology failures")
    return 0


if __name__ == "__main__":
    sys.exit(main())
