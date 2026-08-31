#!/usr/bin/env python3
"""
parity_matrix.py — 11.1-W generated parity matrix.

Replaces hand-typed headline counts ("API complete") with machine-readable
ledgers whose totals are recomputed from evidence on every run:

  atlas/PARITY_MATRIX.json        — current headline parity matrix (all
                                    six plan categories per project)
  atlas/PARITY_MATRIX.md          — generated human view of the matrix
  atlas/API_PARITY_LEDGER.json    — per-project 6-way API reconciliation
  atlas/API_PARITY_LEDGER.md      — generated view
  atlas/DOXYGEN_SURFACE_ATLAS.json/.md — per-version Doxygen public surface

Evidence sources:
  - oracle/historical/doxygen/<proj>-system/inventory-public.json
    (Doxygen preprocessor configs + public inventory counts)
  - atlas/api/<proj>/<version>.json (Clang AST public API records)
  - oracle headers: /usr/include/libxml2/libxml, /usr/include/libxslt,
    /usr/include/libexslt (declaration extractor shared with the
    header-surface audit)
  - oracle DSOs: /usr/lib/libxml2.so.16, /usr/lib/libxslt.so.1,
    /usr/lib/libexslt.so.0 (nm -D --defined-only)
  - candidate headers: include/{libxml,libxslt,libexslt}
  - candidate DSO: target/debug/liblibxml_rs.so

Each total is reproducible: run this script (or the exact nm/parse commands
embedded in it) to recompute.
"""

import json
import os
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
ATLAS = ROOT / "atlas"
DEXTS = "abcdefghijklmnopqrstuvwxyz"

FUNC_MACROS = ("XMLPUBFUN", "LIBXSLT_PUBLIC", "XSLTPUBFUN", "EXSLTPUBFUN")
VAR_MACROS = ("XMLPUBVAR", "LIBXSLT_PUBLIC", "XSLTPUBVAR", "EXSLTPUBVAR")

# Public symbol-name prefixes per project. libxml2 uses xml*/html*/__xml*,
# libxslt uses xslt* (+ exslt* in the shared headers), libexslt uses exslt*.
NAME_PREFIXES = {
    "libxml2": r"(?:xml|html|__xml)",
    "libxslt": r"(?:xslt|exslt)",
    "libexslt": r"exslt",
}


def nm_exports(dso):
    """Defined dynamic exports split into (functions, data) with version
    suffixes stripped (oracle libxslt symbols carry @@LIBXML2_x suffixes)."""
    out = subprocess.run(["nm", "-D", "--defined-only", dso],
                         capture_output=True, text=True).stdout
    funcs, data = set(), set()
    for line in out.splitlines():
        parts = line.split()
        if len(parts) < 3:
            continue
        name = parts[2].split("@", 1)[0]
        if parts[1] == "T":
            funcs.add(name)
        elif parts[1] in ("D", "B", "R"):
            data.add(name)
    return funcs, data


def parse_declared(headers, proj):
    """(functions, vars) declared across `headers` (paths)."""
    prefix = NAME_PREFIXES.get(proj, r"(?:xml|html|xslt|exslt)")
    fn_re = re.compile(
        r"^\s*(?:" + "|".join(FUNC_MACROS) + r")[^;(]*\b("
        + prefix + r"[A-Za-z0-9_]+)\s*\(", re.M)
    var_re = re.compile(
        r"^\s*(?:" + "|".join(VAR_MACROS) + r")[^;]*\b("
        + prefix + r"[A-Za-z0-9_]+)\s*;", re.M)
    funcs, vars_ = set(), set()
    for h in headers:
        text = Path(h).read_text(errors="replace")
        funcs.update(fn_re.findall(text))
        vars_.update(var_re.findall(text))
    return funcs, vars_


def header_dir(paths):
    out = []
    for p in paths:
        pp = Path(p)
        if pp.is_dir():
            out.extend(str(f) for f in sorted(pp.glob("*.h")))
        elif pp.exists():
            out.append(str(pp))
    return out


def doxygen_counts(proj):
    inv = ROOT / "oracle" / "historical" / "doxygen" / f"{proj}-system" / "inventory-public.json"
    if not inv.exists():
        return None, None, None
    d = json.load(open(inv))
    return d.get("counts"), d.get("doxygen_version"), d.get("config_hash")


def clang_ast(proj, version):
    p = ATLAS / "api" / proj / f"{version}.json"
    if not p.exists():
        return None
    return json.load(open(p))


def reconciled(funcs_a, funcs_b):
    return len(funcs_a & funcs_b)


def build_project_matrix(proj, version, oracle_dso, cand_dso, oracle_hdr_paths, cand_hdr_paths, ast):
    dox, dox_ver, dox_cfg = doxygen_counts(proj)
    o_f, o_v = nm_exports(oracle_dso)
    c_f, c_v = nm_exports(cand_dso)
    # Cross-project DSO accounting (11.1-Z.1): the candidate ships three DSOs
    # (core libxml2.so.16 + libxslt/libexslt facades), each with a
    # namespace-scoped surface, so the per-project candidate DSO is used and
    # BOTH sides are filtered by the project's symbol prefix — otherwise the
    # combined core's full function set would be counted for every project
    # (the old 1980-everywhere contamination) and the oracle's few
    # non-prefixed exports would look like unresolved obligations.
    prefix = re.compile(NAME_PREFIXES.get(proj, r"(?:xml|html|xslt|exslt)"))
    o_f = {x for x in o_f if prefix.match(x)}
    o_v = {x for x in o_v if prefix.match(x)}
    c_f = {x for x in c_f if prefix.match(x)}
    c_v = {x for x in c_v if prefix.match(x)}
    oh_f, oh_v = parse_declared(header_dir(oracle_hdr_paths), proj)
    ch_f, ch_v = parse_declared(header_dir(cand_hdr_paths), proj)

    def ast_count(key):
        return len((ast or {}).get(key) or [])

    row = {
        "project": proj,
        "version": version,
        "oracle_dso": oracle_dso,
        "candidate_dso": cand_dso,
        "counts": {
            "public_functions": {
                "oracle_doxygen": (dox or {}).get("function", None),
                "oracle_clang_ast": ast_count("functions"),
                "oracle_headers": len(oh_f),
                "oracle_dso": len(o_f),
                "candidate_headers": len(ch_f),
                "candidate_dso": len(c_f),
                "fully_reconciled": reconciled(o_f, c_f),
                "unresolved_oracle_only": len(o_f - c_f),
                "candidate_extra": len(c_f - o_f),
            },
            "exported_data": {
                "oracle_doxygen": (dox or {}).get("variable", None),
                "oracle_clang_ast": ast_count("globals"),
                "oracle_headers": len(oh_v),
                "oracle_dso": len(o_v),
                "candidate_headers": len(ch_v),
                "candidate_dso": len(c_v),
                "fully_reconciled": reconciled(o_v, c_v),
                "unresolved_oracle_only": len(o_v - c_v),
                "candidate_extra": len(c_v - o_v),
            },
            "struct_fields": {
                "abi_ledger_measurable": None,  # filled from ABI_PARITY_LEDGER.json
            },
            "callbacks": {
                "oracle_clang_ast": ast_count("callbacks"),
                "oracle_doxygen_typedefs": (dox or {}).get("typedef", None),
            },
            "macros": {
                "oracle_doxygen_defines": (dox or {}).get("define", None),
                "oracle_clang_ast": None,
            },
            "types": {
                "oracle_clang_ast_typedefs": ast_count("typedefs"),
                "oracle_clang_ast_records": ast_count("records"),
                "oracle_clang_ast_enums": ast_count("enums"),
                "oracle_clang_ast_enumerators": ast_count("enumerators"),
            },
        },
        "evidence": {
            "doxygen_version": dox_ver,
            "doxygen_config_hash": dox_cfg,
            "clang_ast_generator": (ast or {}).get("generator", None),
            "clang_ast_tag": (ast or {}).get("version_tag", None),
        },
    }
    # fill the ABI measurable count from the ABI parity ledger
    abi = ATLAS / "ABI_PARITY_LEDGER.json"
    if abi.exists():
        try:
            led = json.load(open(abi))
            proj_led = (led.get("projects") or {}).get(proj, {})
            pa = proj_led.get("probe_accounting", {})
            row["counts"]["struct_fields"] = {
                "abi_ledger_measurable": pa.get("oracle", {}).get("measurable"),
                "abi_ledger_structs_probed": proj_led.get("structs_probed"),
                "abi_ledger_enums_probed": proj_led.get("enums_probed"),
                "abi_ledger_oracle_entities": proj_led.get("oracle_entities"),
                "abi_ledger_candidate_entities": proj_led.get("candidate_entities"),
                "abi_ledger_mismatch_count": proj_led.get("mismatch_count"),
                "abi_ledger_verdict": proj_led.get("verdict"),
            }
        except Exception:
            pass
    return row


def doxygen_surface_atlas():
    entries = {}
    for inv in sorted((ROOT / "oracle" / "historical" / "doxygen").glob("*-system/inventory-public.json")):
        proj = inv.parent.name.replace("-system", "")
        d = json.load(open(inv))
        entries[proj] = {
            "doxygen_version": d.get("doxygen_version"),
            "config_hash": d.get("config_hash"),
            "source_tree_hash": d.get("source_tree_hash"),
            "counts": d.get("counts"),
        }
    # historical per-version inventories (non-system)
    hist = {}
    for inv in sorted((ROOT / "oracle" / "historical" / "doxygen").glob("*/inventory-public.json")):
        name = inv.parent.name
        if name.endswith("-system"):
            continue
        d = json.load(open(inv))
        proj = d.get("project")
        hist.setdefault(proj, {})[d.get("version")] = {
            "doxygen_version": d.get("doxygen_version"),
            "config_hash": d.get("config_hash"),
            "counts": d.get("counts"),
        }
    return {"current": entries, "historical": hist}


def main():
    ATLAS.mkdir(parents=True, exist_ok=True)
    projects = [
        {
            "proj": "libxml2", "version": "2.15.3",
            "oracle_dso": "/usr/lib/libxml2.so.16",
            "cand_dso": "target/debug/libxml2.so.16",
            "oracle_hdr": ["/usr/include/libxml2/libxml"],
            "cand_hdr": ["include/libxml"],
        },
        {
            "proj": "libxslt", "version": "1.1.45",
            "oracle_dso": "/usr/lib/libxslt.so.1",
            "cand_dso": "target/debug/libxslt.so.1",
            "oracle_hdr": ["/usr/include/libxslt"],
            "cand_hdr": ["include/libxslt"],
        },
        {
            "proj": "libexslt", "version": "0.8.25",
            "oracle_dso": "/usr/lib/libexslt.so.0",
            "cand_dso": "target/debug/libexslt.so.0",
            "oracle_hdr": ["/usr/include/libexslt"],
            "cand_hdr": ["include/libexslt"],
        },
    ]
    # The three candidate DSOs (core + facades) are 11.1-Z.1 artifacts; keep
    # the combined core as the fallback for tooling that predates the facades.
    cand_dso = "target/debug/liblibxml_rs.so"
    matrix = {"schema": "parity-matrix-2", "generated": "generated by tools/evidence/parity_matrix.py",
              "projects": {}}
    for p in projects:
        ast = clang_ast(p["proj"], p["version"])
        matrix["projects"][p["proj"]] = build_project_matrix(
            p["proj"], p["version"], p["oracle_dso"],
            p["cand_dso"] if os.path.exists(p["cand_dso"]) else cand_dso,
            p["oracle_hdr"], p["cand_hdr"], ast)

    (ATLAS / "PARITY_MATRIX.json").write_text(
        json.dumps(matrix, indent=1, ensure_ascii=False) + "\n")

    # API parity ledger: per-project 6-way function/global reconciliation
    api = {"schema": "api-parity-ledger-1", "generated": "generated by tools/evidence/parity_matrix.py",
           "projects": {}}
    for p in projects:
        o_f, o_v = nm_exports(p["oracle_dso"])
        c_f, c_v = nm_exports(p["cand_dso"] if os.path.exists(p["cand_dso"]) else cand_dso)
        prefix = re.compile(NAME_PREFIXES.get(p["proj"], r"(?:xml|html|xslt|exslt)"))
        o_f = {x for x in o_f if prefix.match(x)}
        o_v = {x for x in o_v if prefix.match(x)}
        c_f = {x for x in c_f if prefix.match(x)}
        c_v = {x for x in c_v if prefix.match(x)}
        oh_f, oh_v = parse_declared(header_dir(p["oracle_hdr"]), p["proj"])
        ch_f, ch_v = parse_declared(header_dir(p["cand_hdr"]), p["proj"])
        ast = clang_ast(p["proj"], p["version"])
        api["projects"][p["proj"]] = {
            "functions": {
                "oracle_headers": sorted(oh_f),
                "oracle_dso": sorted(o_f),
                "candidate_headers": sorted(ch_f),
                "candidate_dso": sorted(c_f),
                "reconciled_all": sorted(oh_f & o_f & ch_f & c_f),
                "oracle_only": sorted((oh_f | o_f) - (ch_f | c_f)),
                "candidate_only": sorted((ch_f | c_f) - (oh_f | o_f)),
            },
            "data": {
                "oracle_headers": sorted(oh_v),
                "oracle_dso": sorted(o_v),
                "candidate_headers": sorted(ch_v),
                "candidate_dso": sorted(c_v),
                "reconciled_all": sorted(oh_v & o_v & ch_v & c_v),
                "oracle_only": sorted((oh_v | o_v) - (ch_v | c_v)),
                "candidate_only": sorted((ch_v | c_v) - (oh_v | o_v)),
            },
            "clang_ast": {
                "functions": len((ast or {}).get("functions") or []),
                "typedefs": (ast or {}).get("typedefs"),
                "records": (ast or {}).get("records"),
                "enums": (ast or {}).get("enums"),
                "enumerators": (ast or {}).get("enumerators"),
                "callbacks": (ast or {}).get("callbacks"),
                "globals": (ast or {}).get("globals"),
            },
        }
    (ATLAS / "API_PARITY_LEDGER.json").write_text(
        json.dumps(api, indent=1, ensure_ascii=False) + "\n")

    # Doxygen surface atlas
    dox = doxygen_surface_atlas()
    (ATLAS / "DOXYGEN_SURFACE_ATLAS.json").write_text(
        json.dumps(dox, indent=1, ensure_ascii=False) + "\n")

    write_markdown(matrix, api, dox)
    print("wrote PARITY_MATRIX.json/.md, API_PARITY_LEDGER.json/.md, DOXYGEN_SURFACE_ATLAS.json/.md")


def write_markdown(matrix, api, dox):
    lines = ["# Parity Matrix — generated by tools/evidence/parity_matrix.py",
             "", "Every total below is recomputed from evidence on every run "
                  "(Doxygen system inventories, Clang AST api records, header "
                  "declaration extraction, `nm -D --defined-only` on the oracle "
                  "and candidate DSOs).", ""]
    for proj, row in matrix["projects"].items():
        c = row["counts"]
        lines += [f"## {proj} {row['version']}", ""]
        for cat in ("public_functions", "exported_data"):
            lines += [f"### {cat}", "", "| source | count |", "|---|---|"]
            for k, v in c[cat].items():
                lines.append(f"| {k} | {v if v is not None else '—'} |")
            lines.append("")
        lines += [f"### struct fields", "",
                  "| ABI ledger metric | value |", "|---|---|"]
        for k, v in c['struct_fields'].items():
            lines.append(f"| {k} | {v if v is not None else '—'} |")
        lines.append("")
        lines += [f"### callbacks / types / macros", "",
                  "| category | count |", "|---|---|"]
        for cat in ("callbacks", "macros", "types"):
            for k, v in c[cat].items():
                lines.append(f"| {cat}.{k} | {v if v is not None else '—'} |")
        lines.append("")
    lines += ["---", "", "# API Parity Ledger — per-project reconciliation", ""]
    for proj, rec in api["projects"].items():
        lines += [f"## {proj}", "",
                  f"- functions: oracle headers {len(rec['functions']['oracle_headers'])}, "
                  f"oracle DSO {len(rec['functions']['oracle_dso'])}, "
                  f"candidate headers {len(rec['functions']['candidate_headers'])}, "
                  f"candidate DSO {len(rec['functions']['candidate_dso'])}, "
                  f"reconciled all four {len(rec['functions']['reconciled_all'])}, "
                  f"oracle-only {len(rec['functions']['oracle_only'])}, "
                  f"candidate-only {len(rec['functions']['candidate_only'])}",
                  f"- data: oracle headers {len(rec['data']['oracle_headers'])}, "
                  f"oracle DSO {len(rec['data']['oracle_dso'])}, "
                  f"candidate headers {len(rec['data']['candidate_headers'])}, "
                  f"candidate DSO {len(rec['data']['candidate_dso'])}, "
                  f"reconciled all four {len(rec['data']['reconciled_all'])}, "
                  f"oracle-only {len(rec['data']['oracle_only'])}, "
                  f"candidate-only {len(rec['data']['candidate_only'])}", ""]
    lines += ["---", "", "# Doxygen Surface Atlas", ""]
    for proj, cur in dox.get("current", {}).items():
        lines += [f"## {proj} (current system)", "",
                  f"- doxygen {cur.get('doxygen_version')}, config "
                  f"{cur.get('config_hash')}, source {cur.get('source_tree_hash')}",
                  f"- counts: {json.dumps(cur.get('counts'))}", ""]
    for proj, versions in dox.get("historical", {}).items():
        lines += [f"## {proj} (historical)", "", "| version | doxygen | functions | defines | enums | typedefs | variables |", "|---|---|---|---|---|---|---|"]
        for ver in sorted(versions, key=lambda v: tuple(int(x) for x in v.split("."))):
            e = versions[ver]
            cnt = e.get("counts") or {}
            lines.append(f"| {ver} | {e.get('doxygen_version')} | {cnt.get('function','—')} | "
                         f"{cnt.get('define','—')} | {cnt.get('enum','—')} | "
                         f"{cnt.get('typedef','—')} | {cnt.get('variable','—')} |")
        lines.append("")
    (ATLAS / "PARITY_MATRIX.md").write_text("\n".join(lines) + "\n")
    (ATLAS / "API_PARITY_LEDGER.md").write_text(build_api_md(api))
    (ATLAS / "DOXYGEN_SURFACE_ATLAS.md").write_text(build_dox_md(dox))


def build_api_md(api):
    lines = ["# API Parity Ledger — generated by tools/evidence/parity_matrix.py", ""]
    for proj, rec in api["projects"].items():
        lines += [f"## {proj}", "",
                  f"- functions: oracle headers {len(rec['functions']['oracle_headers'])}, "
                  f"oracle DSO {len(rec['functions']['oracle_dso'])}, "
                  f"candidate headers {len(rec['functions']['candidate_headers'])}, "
                  f"candidate DSO {len(rec['functions']['candidate_dso'])}, "
                  f"reconciled all four {len(rec['functions']['reconciled_all'])}, "
                  f"oracle-only {len(rec['functions']['oracle_only'])}, "
                  f"candidate-only {len(rec['functions']['candidate_only'])}",
                  f"- data: oracle headers {len(rec['data']['oracle_headers'])}, "
                  f"oracle DSO {len(rec['data']['oracle_dso'])}, "
                  f"candidate headers {len(rec['data']['candidate_headers'])}, "
                  f"candidate DSO {len(rec['data']['candidate_dso'])}, "
                  f"reconciled all four {len(rec['data']['reconciled_all'])}, "
                  f"oracle-only {len(rec['data']['oracle_only'])}, "
                  f"candidate-only {len(rec['data']['candidate_only'])}", ""]
    return "\n".join(lines) + "\n"


def build_dox_md(dox):
    lines = ["# Doxygen Surface Atlas — generated by tools/evidence/parity_matrix.py", ""]
    for proj, cur in dox.get("current", {}).items():
        lines += [f"## {proj} (current system)", "",
                  f"- doxygen {cur.get('doxygen_version')}, config "
                  f"{cur.get('config_hash')}, source {cur.get('source_tree_hash')}",
                  f"- counts: {json.dumps(cur.get('counts'))}", ""]
    for proj, versions in dox.get("historical", {}).items():
        lines += [f"## {proj} (historical)", "",
                  "| version | doxygen | functions | defines | enums | typedefs | variables |",
                  "|---|---|---|---|---|---|---|"]
        for ver in sorted(versions, key=lambda v: tuple(int(x) for x in v.split("."))):
            e = versions[ver]
            cnt = e.get("counts") or {}
            lines.append(f"| {ver} | {e.get('doxygen_version')} | {cnt.get('function','—')} | "
                         f"{cnt.get('define','—')} | {cnt.get('enum','—')} | "
                         f"{cnt.get('typedef','—')} | {cnt.get('variable','—')} |")
        lines.append("")
    return "\n".join(lines) + "\n"


if __name__ == "__main__":
    main()
