#!/usr/bin/env python3
"""Doxygen XML -> normalized forensic surface inventory (11.1-B/11.1-C).

Deterministic pipeline: profile (pins config/source) -> doxygen XML ->
normalized inventory.json (stable entity records, hashed). The raw XML and the
normalized inventory are both hashed; the extraction config hash and Doxygen
version are recorded so any extraction drift is detectable.

Usage:
  doxygen_inventory.py run <project> <version> [public|full]   # (re)generate
  doxygen_inventory.py summarize <project> <version> [profile]  # counts only
"""
import hashlib
import json
import os
import re
import subprocess
import sys
import xml.etree.ElementTree as ET

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
DOX = os.path.join(ROOT, "oracle", "historical", "doxygen")


def raw_xml_hash(xmldir):
    h = hashlib.sha256()
    for fn in sorted(os.listdir(xmldir)):
        if fn.endswith(".xml"):
            h.update(fn.encode())
            h.update(b"\0")
            h.update(open(os.path.join(xmldir, fn), "rb").read())
            h.update(b"\0")
    return h.hexdigest()


def parse_inventory(project, version, profile):
    work = os.path.join(DOX, f"{project}-{version}")
    profile_path = os.path.join(work, f"profile-{profile}.json")
    xmldir = os.path.join(work, f"xml-{profile}")
    with open(profile_path) as f:
        pdoc = json.load(f)

    entities = []
    ns = {"d": "http://www.w3.org/1999/xhtml"}  # not used; plain XML
    for fn in sorted(os.listdir(xmldir)):
        if not fn.endswith(".xml"):
            continue
        path = os.path.join(xmldir, fn)
        try:
            tree = ET.parse(path)
        except ET.ParseError:
            continue
        root = tree.getroot()
        # each compound XML has <compounddef kind="..."> with <sectiondef><memberdef kind="...">
        for comp in root.findall(".//compounddef"):
            comp_kind = comp.get("kind")
            comp_name = (comp.findtext("compoundname") or "").strip()
            for mdef in comp.findall(".//memberdef"):
                kind = mdef.get("kind")
                name = (mdef.findtext("name") or "").strip()
                if not name:
                    continue
                ent = {"kind": kind, "name": name}
                t = mdef.findtext("type") or ""
                args = mdef.findtext("argsstring") or ""
                if kind == "function":
                    ent["args"] = args
                    ent["definition"] = mdef.findtext("definition") or ""
                    ent["static"] = mdef.get("static") == "yes"
                    ent["prot"] = mdef.get("prot")
                elif kind in ("variable", "typedef"):
                    ent["type"] = t.strip()
                elif kind == "define":
                    ent["value"] = (mdef.findtext("initializer") or "").strip()
                elif kind == "enumvalue":
                    ent["value"] = (mdef.findtext("initializer") or "").strip()
                    ent["enum"] = comp_name
                ent["documented"] = bool((mdef.findtext("briefdescription") or "").strip())
                loc = mdef.find("location")
                if loc is not None:
                    fpath = loc.get("file") or ""
                    ent["header"] = os.path.basename(fpath)
                    ent["line"] = loc.get("line")
                if comp_kind == "struct":
                    ent["struct"] = comp_name
                if comp_kind == "union":
                    ent["union"] = comp_name
                entities.append(ent)

    # deterministic ordering
    entities.sort(key=lambda e: (e["kind"], e.get("header") or "", e.get("struct") or "",
                                 e.get("enum") or "", e["name"], e.get("args") or ""))
    counts = {}
    for e in entities:
        counts[e["kind"]] = counts.get(e["kind"], 0) + 1
    counts["total"] = len(entities)

    inventory = {
        "schema": "doxygen-inventory-1",
        "project": project,
        "version": version,
        "profile": profile,
        "doxygen_version": pdoc["doxygen_version"],
        "config_hash": pdoc["config_hash"],
        "source_tree_hash": pdoc["source_tree_hash"],
        "raw_xml_hash": raw_xml_hash(xmldir),
        "counts": counts,
        "entities": entities,
    }
    inv_hash = hashlib.sha256(json.dumps(inventory, sort_keys=True).encode()).hexdigest()
    inventory["inventory_hash"] = inv_hash
    return inventory


def run(project, version, profile="public"):
    work = os.path.join(DOX, f"{project}-{version}")
    doxyfile = os.path.join(work, f"Doxyfile-{profile}")
    if not os.path.exists(doxyfile):
        print(f"profile missing; generate it first: doxygen_profile.py gen {project} {version} {profile}")
        return 1
    r = subprocess.run(["doxygen", doxyfile], capture_output=True, text=True)
    if r.returncode != 0:
        print("doxygen failed:", r.stderr[-800:])
        return 1
    inv = parse_inventory(project, version, profile)
    out = os.path.join(work, f"inventory-{profile}.json")
    with open(out, "w") as f:
        json.dump(inv, f, indent=1, ensure_ascii=False)
        f.write("\n")
    print(f"{project}-{version} [{profile}]: {inv['counts']['total']} entities, "
          f"raw_xml_hash {inv['raw_xml_hash'][:12]}, inventory_hash {inv['inventory_hash'][:12]}")
    return 0


def summarize(project, version, profile="public"):
    inv = parse_inventory(project, version, profile)
    print(f"{project}-{version} [{profile}] counts:")
    for k in sorted(inv["counts"]):
        print(f"  {k}: {inv['counts'][k]}")
    return 0


if __name__ == "__main__":
    if len(sys.argv) < 2:
        print(__doc__)
        sys.exit(1)
    cmd = sys.argv[1]
    if cmd == "run":
        sys.exit(run(sys.argv[2], sys.argv[3], sys.argv[4] if len(sys.argv) > 4 else "public"))
    if cmd == "summarize":
        sys.exit(summarize(sys.argv[2], sys.argv[3], sys.argv[4] if len(sys.argv) > 4 else "public"))
    print(__doc__)
    sys.exit(1)
