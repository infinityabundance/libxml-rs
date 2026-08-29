#!/usr/bin/env python3
"""Exhaustive C ABI census probe generator + runner (11.1-G).

Generates one C probe per project covering every public struct (sizeof/alignof/
offsetof per field), union, and enum (every enumerator value), compiles it
against the ORACLE headers (system /usr/include) and against the CANDIDATE
headers (repository include/), executes both, and diffs the values into
atlas/ABI_PARITY_LEDGER.json.

sizeof/offsetof are compile-time constants, so the candidate probe needs no
link against the Rust library — a header-only compile proves the layout claim.

Usage:
  abi_probe_gen.py
"""
import json
import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
DOX = os.path.join(ROOT, "oracle", "historical", "doxygen")
ATLAS = os.path.join(ROOT, "atlas")

PROJECTS = {
    "libxml2": {
        "inv": os.path.join(DOX, "libxml2-2.15.0", "inventory-full.json"),
        "public_inv": os.path.join(DOX, "libxml2-2.15.0", "inventory-public.json"),
        "oracle_inc": ["/usr/include/libxml2", "/usr/include"],
        "cand_inc": [os.path.join(ROOT, "include")],
        "include_guard": (
            "#include <libxml/tree.h>\n#include <libxml/parser.h>\n"
            "#include <libxml/xpath.h>\n#include <libxml/xmlreader.h>\n"
            "#include <libxml/xmlwriter.h>\n#include <libxml/xmlschemas.h>\n"
            "#include <libxml/schemasInternals.h>\n#include <libxml/xmlschemastypes.h>\n#include <libxml/relaxng.h>\n"
            "#include <libxml/xinclude.h>\n#include <libxml/xpointer.h>\n"
            "#include <libxml/catalog.h>\n#include <libxml/encoding.h>\n"
            "#include <libxml/entities.h>\n#include <libxml/HTMLparser.h>\n"
            "#include <libxml/HTMLtree.h>\n#include <libxml/SAX2.h>\n"
            "#include <libxml/uri.h>\n#include <libxml/valid.h>\n"
            "#include <libxml/xmlautomata.h>\n#include <libxml/xmlregexp.h>\n"
            "#include <libxml/xmlsave.h>\n#include <libxml/xmlstring.h>\n"
            "#include <libxml/xmlunicode.h>\n#include <libxml/xmlversion.h>\n"
            "#include <libxml/xpathInternals.h>\n#include <libxml/dict.h>\n"
            "#include <libxml/hash.h>\n#include <libxml/list.h>\n"
            "#include <libxml/nanohttp.h>\n#include <libxml/parserInternals.h>\n"
            "#include <libxml/pattern.h>\n#include <libxml/schematron.h>\n"
            "#include <libxml/threads.h>\n#include <libxml/xmlIO.h>\n"
            "#include <libxml/xmlmemory.h>\n#include <libxml/xmlmodule.h>\n"),
        "version": "2.15.3",
    },
    "libxslt": {
        "inv": os.path.join(DOX, "libxslt-1.1.42", "inventory-full.json"),
        "public_inv": os.path.join(DOX, "libxslt-1.1.42", "inventory-public.json"),
        "oracle_inc": ["/usr/include/libxml2", "/usr/include"],
        "cand_inc": [os.path.join(ROOT, "include")],
        "include_guard": (
            "#include <libxslt/xslt.h>\n#include <libxslt/xsltInternals.h>\n"
            "#include <libxslt/transform.h>\n#include <libxslt/xsltutils.h>\n"
            "#include <libxslt/security.h>\n#include <libxslt/namespaces.h>\n"
            "#include <libxslt/variables.h>\n#include <libxslt/keys.h>\n"
            "#include <libxslt/numbersInternals.h>\n#include <libxslt/extensions.h>\n"
            "#include <libxslt/extra.h>\n#include <libxslt/functions.h>\n"
            "#include <libxslt/attributes.h>\n#include <libxslt/imports.h>\n"
            "#include <libxslt/documents.h>\n#include <libxslt/preproc.h>\n"
            "#include <libxslt/templates.h>\n#include <libexslt/exslt.h>\n"),
        "version": "1.1.45",
    },
}

SKIP_STRUCTS = {"__anon"}


def collect(project):
    inv = json.load(open(PROJECTS[project]["inv"]))
    inv_public = json.load(open(
        os.path.join(DOX, PROJECTS[project]["public_inv"])))
    public_headers = {e.get("header") for e in inv_public["entities"]
                      if e.get("header")}
    structs = {}
    for e in inv["entities"]:
        if e["kind"] == "variable" and e.get("struct") \
                and e.get("header") in public_headers:
            structs.setdefault(e["struct"], []).append((e["name"], e.get("type", "")))
    enums = {}
    for e in inv["entities"]:
        if e["kind"] == "enum" and e.get("header") in public_headers:
            enums[e["name"]] = e.get("enum_values", [])
    return structs, enums


def candidate_defined(project, structs, enums):
    """Which structs/enums the candidate headers actually define (grep-based),
    so the candidate probe compiles and the census records the rest as MISSING."""
    inc = os.path.join(ROOT, "include")
    hay = ""
    for root, _d, files in os.walk(inc):
        for fn in files:
            if fn.endswith(".h"):
                try:
                    hay += open(os.path.join(root, fn), encoding="utf-8",
                                errors="replace").read() + "\n"
                except OSError:
                    pass
    s_def = {s for s in structs if re.search(rf"struct\s+[A-Za-z_]*{re.escape(s)}\s*{{|typedef\s+struct\s+[A-Za-z_]*{re.escape(s)}", hay)}
    e_def = set()
    for ename, values in enums.items():
        if re.search(rf"\b{re.escape(ename)}\b", hay):
            e_def.add(ename)
    return s_def, e_def


def gen_probe(project, entity_filter=None):
    structs, enums = collect(project)
    s_def, e_def = candidate_defined(project, structs, enums)
    if entity_filter == "candidate":
        structs = {s: f for s, f in structs.items() if s in s_def}
        enums = {e: v for e, v in enums.items() if e in e_def}
    elif entity_filter == "oracle-only":
        structs = {s: f for s, f in structs.items() if s not in s_def}
        enums = {e: v for e, v in enums.items() if e not in e_def}
    guard = PROJECTS[project]["include_guard"]
    lines = [
        "#include <stddef.h>",
        "#include <stdio.h>",
        guard,
        "int main(void) {",
    ]
    n_struct = 0
    for sname, fields in sorted(structs.items()):
        if not sname or sname in SKIP_STRUCTS or sname.startswith("(") \
                or sname.startswith("__anon") or not re.match(r"^[A-Za-z_]\w*$", sname):
            continue
        lines.append(f'  printf("STRUCT {sname} sizeof=%zu alignof=%zu\\n", '
                     f'sizeof(struct {sname}), _Alignof(struct {sname}));')
        for fname, _ftype in fields:
            if not re.match(r"^[A-Za-z_]\w*$", fname):
                continue
            lines.append(f'  printf("  FIELD {sname}.{fname} offsetof=%zu sizeof=%zu\\n", '
                         f'offsetof(struct {sname}, {fname}), '
                         f'sizeof(((struct {sname} *)0)->{fname}));')
        n_struct += 1
    n_enum = 0
    for ename, values in sorted(enums.items()):
        if not re.match(r"^[A-Za-z_]\w*$", ename):
            continue
        for vname, _vinit in values:
            if not re.match(r"^[A-Za-z_]\w*$", vname):
                continue
            lines.append(f'  printf("ENUM {ename}.{vname}=%d\\n", (int){vname});')
        n_enum += 1
    lines.append("  return 0;")
    lines.append("}")
    src = "\n".join(lines) + "\n"
    out = os.path.join(ROOT, "target", f"abi-probe-{project}.c")
    os.makedirs(os.path.dirname(out), exist_ok=True)
    with open(out, "w") as f:
        f.write(src)
    return out, n_struct, n_enum


def build_and_run(src, inc_dirs, tag):
    """Compile+run; on errors, drop the offending entity lines and retry
    (bounded) so opaque structs / absent enum values are recorded as gaps,
    not fatal probe failures."""
    exe = src + f".{tag}"
    retries = 0
    name_re = re.compile(
        r"'(\w+)' undeclared"
        r"|incomplete type 'struct (\w+)'"
        r"|undefined type 'struct (\w+)'"
        r"|unknown type name '(\w+)'"
        r"|'struct (\w+)' has no member named '(\w+)'")
    while retries < 200:
        args = ["gcc", "-std=c11", "-o", exe, src] + \
               [a for i in inc_dirs for a in ("-I", i)]
        r = subprocess.run(args, capture_output=True, text=True)
        if r.returncode == 0:
            run = subprocess.run([exe], capture_output=True, text=True)
            return run.stdout, None
        names = set()
        err = r.stderr.replace("‘", "'").replace("’", "'")
        for m in name_re.finditer(err):
            names.update(g for g in m.groups() if g)
        if not names:
            return None, r.stderr[-2000:]
        with open(src) as f:
            lines = f.readlines()
        kept = [ln for ln in lines if not any(
            re.search(rf"\b{re.escape(n)}\b", ln) for n in names)]
        if len(kept) == len(lines):
            return None, r.stderr[-2000:]
        with open(src, "w") as f:
            f.writelines(kept)
        retries += 1
    return None, "retry limit"


def parse(text):
    out = {}
    for line in text.splitlines():
        if line.startswith("STRUCT "):
            m = re.match(r"STRUCT (\S+) sizeof=(\d+) alignof=(\d+)", line)
            if m:
                out[f"struct:{m.group(1)}"] = {"sizeof": int(m.group(2)),
                                               "alignof": int(m.group(3))}
        elif line.startswith("  FIELD "):
            m = re.match(r"  FIELD (\S+)\.(\S+) offsetof=(\d+) sizeof=(\d+)", line)
            if m:
                out[f"field:{m.group(1)}.{m.group(2)}"] = {
                    "offsetof": int(m.group(3)), "sizeof": int(m.group(4))}
        elif line.startswith("ENUM "):
            m = re.match(r"ENUM (\S+)\.(\S+)=(-?\d+)", line)
            if m:
                out[f"enum:{m.group(1)}.{m.group(2)}"] = int(m.group(3))
    return out


def main():
    ledger = {"schema": "abi-parity-ledger-1", "projects": {}}
    for project, info in PROJECTS.items():
        src, n_struct, n_enum = gen_probe(project)
        print(f"{project}: probe with {n_struct} structs, {n_enum} enums -> {src}")
        vo, err_o = build_and_run(src, info["oracle_inc"], "oracle")
        # candidate probe: only entities the candidate headers define; the rest
        # are recorded as header-surface gaps (residuals), not probe failures
        src_c, n_sc, n_ec = gen_probe(project, "candidate")
        vc, err_c = build_and_run(src_c, info["cand_inc"], "candidate")
        if err_o or err_c:
            print(f"  compile errors: oracle={bool(err_o)} candidate={bool(err_c)}")
            if err_o:
                print("  oracle:", err_o[:300])
            if err_c:
                print("  candidate:", err_c[:300])
        po, pc = parse(vo or ""), parse(vc or "")
        mismatches = []
        for k, v in po.items():
            if k not in pc:
                mismatches.append({"entity": k, "oracle": v, "candidate": "MISSING"})
            elif pc[k] != v:
                mismatches.append({"entity": k, "oracle": v, "candidate": pc[k]})
        for k, v in pc.items():
            if k not in po:
                mismatches.append({"entity": k, "oracle": "MISSING", "candidate": v})
        # oracle-only entities = candidate header-surface gaps
        src_o, n_so, n_eo = gen_probe(project, "oracle-only")
        header_gaps = []
        if n_so or n_eo:
            vo2, err_o2 = build_and_run(src_o, info["oracle_inc"], "oracle-gaps")
            if not err_o2:
                header_gaps = sorted(parse(vo2 or "").keys())
        ledger["projects"][project] = {
            "version": info["version"],
            "structs_probed": n_struct,
            "enums_probed": n_enum,
            "oracle_entities": len(po),
            "candidate_entities": len(pc),
            "mismatch_count": len(mismatches),
            "mismatches": mismatches,
            "candidate_header_gap_entities": header_gaps,
            "verdict": "PASS" if not mismatches else "FAIL",
        }
        print(f"  oracle entities={len(po)} candidate={len(pc)} "
              f"mismatches={len(mismatches)} header-gaps={len(header_gaps)} "
              f"verdict={ledger['projects'][project]['verdict']}")
        for mm in mismatches[:6]:
            print("   ", mm)
    out = os.path.join(ATLAS, "ABI_PARITY_LEDGER.json")
    with open(out, "w") as f:
        json.dump(ledger, f, indent=1, ensure_ascii=False)
        f.write("\n")
    print("ledger ->", out)
    return 0 if all(p["verdict"] == "PASS" for p in ledger["projects"].values()) else 1


if __name__ == "__main__":
    sys.exit(main())
