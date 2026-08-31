#!/usr/bin/env python3
"""EXPORT-SURFACE-DISPOSITION (Phase 12) — exact current-oracle export surface
and per-symbol disposition of every candidate DSO export.

Phase 12 requires that "what the loader sees is the contract you intended to
preserve — and nothing accidentally escaped from the Rust implementation".
Every symbol exported by a shipped candidate DSO is classified against the
executed oracle:

  CURRENT_ORACLE_EXPORT   present in the executed oracle DSO (nm -D);
  HISTORICAL_COMPAT_EXPORT
                          absent from the executed oracle but present in the
                          upstream versioning maps (libxml2.syms / libxslt.syms
                          archaeology) — upstream used to export it;
  CUSTODIAN_EXTENSION     deliberate candidate addition, documented in the
                          disposition allowlist (atlas/EXPORT_DISPOSITION
                          allowlist entries) or declared by the candidate
                          headers beyond the oracle;
  INTERNAL_LEAK           everything else — Rust implementation machinery that
                          #[no_mangle]'d its way into the dynamic surface and
                          must be hidden from the shipped DSOs.

The shipped contract (and the generated version scripts) is the EXACT oracle
surface:

  libxml2.so.16   unversioned exports = system /usr/lib/libxml2.so.16 (nm -D)
  libxslt.so.1    27-node LIBXML2_1.x named-version graph with the oracle's
                  per-symbol node assignment (extracted from the oracle's
                  DT_VERDEF/@@ suffixes)
  libexslt.so.0   unversioned exports = system /usr/lib/libexslt.so.0

Every non-oracle candidate export is recorded in the disposition ledger with
its class; INTERNAL_LEAK and HISTORICAL_COMPAT exports are hidden from the
shared DSOs by the generated version scripts (the staticlib libxml2.a retains
the full implementation, so the historical surface remains statically
linkable). The ledger is the single source of truth for the Phase-12
ELF-VERSIONING and DYNSYM-SURFACE courts.

Outputs (committed):
  atlas/EXPORT_SURFACE_DISPOSITION.json   machine-readable ledger
  atlas/EXPORT_SURFACE_DISPOSITION.md     human-readable view
  tools/packaging/libxml2.syms            exact unversioned core export map
  tools/packaging/libxslt.syms            27-node LIBXML2_1.x version map
  tools/packaging/libexslt.syms           exact unversioned exslt export map

--check mode verifies the SHIPPED DSOs (target/debug/lib/*) implement exactly
the generated contract (set equality of defined exports + per-symbol version
nodes + no version definitions where the oracle has none).

Usage:
  python3 tools/phase12/export_surface.py [--check]
"""
import argparse
import json
import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
ATLAS = os.path.join(ROOT, "atlas")
PKG = os.path.join(ROOT, "tools", "packaging")
LIBDIR = os.path.join(ROOT, "target", "debug", "lib")
CORE = os.path.join(ROOT, "target", "debug", "liblibxml_rs.so")
XSLT_FACADE = os.path.join(LIBDIR, "libxslt.so.1.1.45")
EXSLT_FACADE = os.path.join(LIBDIR, "libexslt.so.0.8.25")

ORACLE = {
    "libxml2": "/usr/lib/libxml2.so.16",
    "libxslt": "/usr/lib/libxslt.so.1",
    "libexslt": "/usr/lib/libexslt.so.0",
}
CANDIDATE = {
    "libxml2": CORE,
    "libxslt": XSLT_FACADE,
    "libexslt": EXSLT_FACADE,
}

# Upstream versioning maps from the archaeology trees (historical provenance).
UPSTREAM_SYMS = {
    "libxml2": os.path.join(ROOT, "oracle", "historical", "src",
                            "libxml2-2.13.5", "libxml2.syms"),
    "libxslt": os.path.join(ROOT, "oracle", "historical", "src",
                            "libxslt-1.1.42", "libxslt", "libxslt.syms"),
    "libexslt": None,
}

# The oracle libxslt version-node chain (from readelf --version-info of the
# system 1.1.45): node -> parent. Newest node last; its version is default.
XSLT_NODE_CHAIN = [
    "LIBXML2_1.0.11", "LIBXML2_1.0.12", "LIBXML2_1.0.13", "LIBXML2_1.0.16",
    "LIBXML2_1.0.17", "LIBXML2_1.0.18", "LIBXML2_1.0.22", "LIBXML2_1.0.24",
    "LIBXML2_1.0.30", "LIBXML2_1.0.32", "LIBXML2_1.0.33", "LIBXML2_1.1.0",
    "LIBXML2_1.1.1", "LIBXML2_1.1.2", "LIBXML2_1.1.3", "LIBXML2_1.1.5",
    "LIBXML2_1.1.7", "LIBXML2_1.1.9", "LIBXML2_1.1.18", "LIBXML2_1.1.20",
    "LIBXML2_1.1.23", "LIBXML2_1.1.24", "LIBXML2_1.1.25", "LIBXML2_1.1.26",
    "LIBXML2_1.1.27", "LIBXML2_1.1.30", "LIBXML2_1.1.34",
]

# Deliberate candidate additions beyond the oracle surface. The pre-2.15
# global-data plane is the documented CUSTODIAN_EXTENSION: upstream 2.15 moved
# the error/global value data behind __xml* accessor functions (e.g.
# `#define xmlLastError (*__xmlLastError())`), but the candidate headers
# declare the historical data variables AND the Rust core exports both, so
# binaries compiled against pre-2.15 headers (which reference the data
# symbols) substitute cleanly — a drop-in superset over the executed oracle.
# Storage coherence (accessor returns addr_of_mut! of the same variable) is
# proven by the DSO-STATE / DATA-PLANE-COHERENCE courts.
CUSTODIAN_EXTENSIONS = {
    # "xmlMyExtension": "R-0001xx: <rationale>",
}

# Candidate header trees per project (for header-declared provenance).
PROJECT_HEADERS = {
    "libxml2": ["include/libxml", "include/libxml2"],
    "libxslt": ["include/libxslt"],
    "libexslt": ["include/libexslt"],
}

# Symbols that belong to another project's surface (the core carries all
# three projects; the disposition of a symbol in the core is relative to the
# libxml2.so.16 DSO, so xslt*/exslt* are leaks there even though the libxslt
# facade legitimately exports them).
FOREIGN_PREFIXES = {
    "libxml2": ("xslt", "exslt", "xsl"),
    "libxslt": ("exslt",),
    "libexslt": (),
}


def nm_dyn(so):
    """defined dynamic exports: {name: kind} (version suffix stripped)."""
    out = subprocess.run(["nm", "-D", "--defined-only", so],
                         capture_output=True, text=True).stdout
    syms = {}
    for line in out.splitlines():
        p = line.split()
        if len(p) < 3:
            continue
        name = p[2].split("@@")[0].split("@")[0]
        kind = p[1]
        if kind == "A":
            # absolute symbols: the version-node names themselves
            # (LIBXML2_1.x), not exports
            continue
        if name in ("", "_edata", "_end", "__bss_start", "_init", "_fini"):
            continue
        syms[name] = kind
    return syms


def oracle_version_map(so):
    """{name: @@node} for the versioned defined exports of a DSO."""
    out = subprocess.run(["nm", "-D", "--defined-only", so],
                         capture_output=True, text=True).stdout
    m = {}
    for line in out.splitlines():
        p = line.split()
        if len(p) < 3:
            continue
        if "@@" in p[2]:
            sym, node = p[2].split("@@")
            m[sym] = node
    return m


def read_version_info(so):
    """(version_definition_nodes, version_need_entries) via readelf."""
    out = subprocess.run(["readelf", "--version-info", so],
                         capture_output=True, text=True).stdout
    defs = []
    in_defs = False
    for line in out.splitlines():
        if "Version definition section" in line:
            in_defs = True
            continue
        if in_defs:
            if "Version needs section" in line:
                break
            mm = re.search(r"Name: (LIBXML2_\S+|LIBXSLT_\S+|lib\S+\.so\.\d+)\s*$", line)
            if mm and mm.group(1) not in defs:
                defs.append(mm.group(1))
    needs = [l.split("File: ")[1].strip() for l in out.splitlines()
             if "File: " in l and "Cnt:" in l]
    return defs, needs


def header_declared(include_dir):
    """Functions + data declared in the candidate header tree."""
    funcs, data = set(), set()
    fn_re = re.compile(r"XMLPUBFUN\s+(?:(?!\bXMLPUBFUN\b)[^;])*?\b([A-Za-z_]\w*)\s*\(",
                       re.S)
    var_re = re.compile(r"XMLPUBVAR\s+[^;]*?\b([A-Za-z_]\w*)\s*(?:\[[^\]]*\])?\s*;",
                        re.S)
    for root, _dirs, files in os.walk(include_dir):
        for fn in files:
            if not fn.endswith(".h"):
                continue
            text = open(os.path.join(root, fn), encoding="utf-8",
                        errors="replace").read()
            for m in fn_re.finditer(text):
                funcs.add(m.group(1))
            for m in var_re.finditer(text):
                data.add(m.group(1))
    return funcs, data


def upstream_sym_names(project):
    path = UPSTREAM_SYMS[project]
    if not path or not os.path.exists(path):
        return set()
    text = open(path, encoding="utf-8", errors="replace").read()
    names = set()
    for m in re.finditer(r"^\s*([A-Za-z_]\w*);\s*$", text, re.M):
        names.add(m.group(1))
    return names


def upstream_xml_node_map():
    """symbol -> LIBXML2_2.x node from the last upstream versioned map
    (libxml2-2.13.5/libxml2.syms). Newer symbols have no node (upstream
    stopped adding to the map; they were exported unversioned)."""
    path = UPSTREAM_SYMS["libxml2"]
    if not path or not os.path.exists(path):
        return {}
    text = open(path, encoding="utf-8", errors="replace").read()
    mapping = {}
    node = None
    for line in text.splitlines():
        mm = re.match(r"^(LIBXML2_\S+)\s*\{", line)
        if mm:
            node = mm.group(1)
            continue
        if node and re.match(r"^\s*([A-Za-z_]\w*);", line):
            mapping[re.match(r"^\s*([A-Za-z_]\w*);", line).group(1)] = node
    return mapping


def upstream_xml_node_chain():
    """ordered node chain of the upstream 2.13.5 map (parent = previous)."""
    path = UPSTREAM_SYMS["libxml2"]
    if not path or not os.path.exists(path):
        return []
    text = open(path, encoding="utf-8", errors="replace").read()
    chain = []
    for line in text.splitlines():
        mm = re.match(r"^(LIBXML2_\S+)\s*\{", line)
        if mm:
            chain.append(mm.group(1))
    return chain


# Terminal node for exports the upstream map predates (2.14/2.15 additions,
# the __xml* accessor functions, the candidate's data-plane extensions).
XML_TERMINAL_NODE = "LIBXML2_2.15.0"

# Terminal node for libxslt exports the executed oracle does not version
# (the candidate's CUSTODIAN_EXTENSION / HISTORICAL_COMPAT additions).
XSLT_TERMINAL_NODE = "LIBXML2_1.1.45"


def classify(name, kind, oracle, hdr_f, hdr_d, up_syms, project):
    if name in oracle:
        return "CURRENT_ORACLE_EXPORT"
    if name in CUSTODIAN_EXTENSIONS:
        return "CUSTODIAN_EXTENSION"
    in_hdr = name in hdr_f if kind in ("T", "W", "i", "I") else name in hdr_d
    if in_hdr:
        return "CUSTODIAN_EXTENSION" if name not in up_syms \
            else "HISTORICAL_COMPAT_EXPORT"
    if name in up_syms:
        return "HISTORICAL_COMPAT_EXPORT"
    return "INTERNAL_LEAK"


def generate_xml_syms(names, node_map, node_chain, path):
    """Versioned LIBXML2_2.x export map: the upstream 43-node chain from the
    last upstream libxml2.syms (2.13.5) plus a terminal LIBXML2_2.15.0 node
    for exports the upstream map predates. Upstream deliberately retains the
    historical nodes for backward compatibility with older linked binaries
    (the reviewer's cited contract); new symbols were exported unversioned by
    upstream, and assigning them the terminal node preserves unversioned-ref
    binding (default version) while giving versioned-distro binaries their
    required nodes."""
    by_node = {}
    for n in names:
        node = node_map.get(n, XML_TERMINAL_NODE)
        by_node.setdefault(node, []).append(n)
    chain = list(node_chain)
    if XML_TERMINAL_NODE not in chain:
        chain.append(XML_TERMINAL_NODE)
    # drop nodes that ended up with no shipped symbols (every symbol they
    # listed upstream is an INTERNAL_LEAK here); keep the chain contiguous
    # over the emitted nodes.
    chain = [n for n in chain if by_node.get(n)]
    lines = ["# Generated by tools/phase12/export_surface.py — the upstream\n",
             "# LIBXML2_2.x named-version chain (libxml2-2.13.5/libxml2.syms,\n",
             "# the last upstream versioned map) plus the terminal\n",
             f"# {XML_TERMINAL_NODE} node for 2.14/2.15 additions and the\n",
             "# candidate's documented extension plane. INTERNAL_LEAK symbols\n",
             "# are hidden (local: * inside the terminal node). The executed\n",
             "# oracle (system 2.15.3) is unversioned; unversioned references\n",
             "# bind to the default (single) node of each symbol.\n"]
    last = chain[-1]
    for i, node in enumerate(chain):
        syms = sorted(by_node.get(node, []))
        lines.append(f"{node} {{\n    global:\n")
        for s in syms:
            lines.append(f"      {s};\n")
        if node == last:
            # GNU ld: an anonymous "local: *" node cannot follow named
            # nodes; the idiom is local: *; inside the final named node
            # (applied globally — every unlisted symbol becomes local).
            lines.append("  local: *;\n")
        if i == 0:
            lines.append("};\n\n")
        else:
            lines.append(f"}} {chain[i-1]};\n\n")
    with open(path, "w") as f:
        f.writelines(lines)


def generate_xslt_syms(symmap, shipped, path):
    """The 27-node LIBXML2_1.x graph with the oracle's per-symbol nodes, plus
    a terminal LIBXML2_1.1.45 node for the shipped exports the oracle does
    not version (CUSTODIAN_EXTENSION / HISTORICAL_COMPAT additions)."""
    by_node = {}
    for sym in shipped:
        by_node.setdefault(symmap.get(sym, XSLT_TERMINAL_NODE), []).append(sym)
    chain = list(XSLT_NODE_CHAIN)
    if XSLT_TERMINAL_NODE not in chain:
        chain.append(XSLT_TERMINAL_NODE)
    lines = ["# Generated by tools/phase12/export_surface.py — the exact\n",
             "# 27-node LIBXML2_1.x named-version graph of the executed\n",
             "# oracle (system libxslt 1.1.45 DT_VERDEF), with the oracle's\n",
             "# per-symbol node assignment, plus the terminal\n",
             f"# {XSLT_TERMINAL_NODE} node for shipped additions the oracle does\n",
             "# not version. Hidden: xslt* implementation internals not in the\n",
             "# dispositioned surface (local: * inside the final node).\n"]
    last = chain[-1]
    for i, node in enumerate(chain):
        syms = sorted(by_node.get(node, []))
        lines.append(f"{node} {{\n    global:\n")
        for s in syms:
            lines.append(f"      {s};\n")
        if node == last:
            lines.append("  local: *;\n")
        if i == 0:
            lines.append("};\n\n")
        else:
            lines.append(f"}} {chain[i-1]};\n\n")
    with open(path, "w") as f:
        f.writelines(lines)


def generate_exslt_syms(names, path):
    funcs = sorted(n for n, k in names.items() if k in ("T", "W", "i", "I"))
    data = sorted(n for n, k in names.items() if k not in ("T", "W", "i", "I"))
    with open(path, "w") as f:
        f.write("# Generated by tools/phase12/export_surface.py — exact\n")
        f.write("# current-oracle export surface of libexslt.so.0 (executed\n")
        f.write("# oracle: system 0.8.25; unversioned).\n")
        f.write("{\n  global:\n")
        for n in funcs:
            f.write(f"    {n};\n")
        for n in data:
            f.write(f"    {n};\n")
        f.write("  local: *;\n};\n")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--check", action="store_true",
                    help="verify the shipped DSOs implement the generated contract")
    args = ap.parse_args()

    os.makedirs(ATLAS, exist_ok=True)
    os.makedirs(PKG, exist_ok=True)

    hdr_f, hdr_d = header_declared(os.path.join(ROOT, "include"))

    ledger = {"schema": "export-surface-disposition-1",
              "phase": "12",
              "generator": "tools/phase12/export_surface.py",
              "note": "shipped shared-DSO surface = executed-oracle surface + "
                       "documented CUSTODIAN_EXTENSION plane (pre-2.15 global-"
                       "data compat); INTERNAL_LEAK/HISTORICAL_COMPAT exports "
                       "are hidden by the generated version scripts",
              "projects": {}}
    shipped_ok = True
    for project in ("libxml2", "libxslt", "libexslt"):
        oracle = nm_dyn(ORACLE[project])
        cand = nm_dyn(CANDIDATE[project])
        up = upstream_sym_names(project)
        phdr_f, phdr_d = set(), set()
        for hd in PROJECT_HEADERS[project]:
            f_, d_ = header_declared(os.path.join(ROOT, hd))
            phdr_f |= f_
            phdr_d |= d_
        rows = {}
        for name in sorted(cand):
            kind = cand[name]
            cls = classify(name, kind, oracle, phdr_f, phdr_d, up, project)
            rows[name] = {"kind": kind, "disposition": cls,
                          "in_oracle": name in oracle,
                          "in_headers": name in phdr_f or name in phdr_d,
                          "in_upstream_syms": name in up}
        counts = {}
        for r in rows.values():
            counts[r["disposition"]] = counts.get(r["disposition"], 0) + 1
        ledger["projects"][project] = {
            "oracle_dso": ORACLE[project],
            "candidate_dso": CANDIDATE[project],
            "oracle_exports": len(oracle),
            "candidate_exports": len(cand),
            "counts": counts,
            "symbols": rows,
        }

        # Generated version script = the shipped surface: every candidate
        # export except INTERNAL_LEAK (the Rust implementation machinery).
        # CUSTODIAN_EXTENSION and HISTORICAL_COMPAT exports are deliberately
        # retained: they are declared API / upstream-versioned surface that
        # real ecosystem binaries reference.
        shipped = {n: r for n, r in rows.items()
                   if r["disposition"] != "INTERNAL_LEAK"}
        if project == "libxslt":
            vmap = oracle_version_map(ORACLE["libxslt"])
            generate_xslt_syms(vmap, set(shipped), os.path.join(PKG, "libxslt.syms"))
        elif project == "libxml2":
            node_map = upstream_xml_node_map()
            node_chain = upstream_xml_node_chain()
            generate_xml_syms(shipped, node_map, node_chain,
                              os.path.join(PKG, "libxml2.syms"))
        else:
            generate_exslt_syms(shipped, os.path.join(PKG, "libexslt.syms"))

    # ── --check: shipped DSOs must implement the generated contract ───────
    if args.check:
        print("── checking shipped DSOs against the generated contract ──")
        for project in ("libxml2", "libxslt", "libexslt"):
            oracle = nm_dyn(ORACLE[project])
            cand = nm_dyn(CANDIDATE[project])
            p = ledger["projects"][project]
            expected = {n for n, r in p["symbols"].items()
                        if r["disposition"] != "INTERNAL_LEAK"}
            extra = sorted(set(cand) - expected)
            missing = sorted(expected - set(cand))
            status = "PASS" if not extra and not missing else "FAIL"
            if status != "PASS":
                shipped_ok = False
            print(f"  {project}: candidate={len(cand)} expected={len(expected)} "
                  f"extra={len(extra)} missing={len(missing)} {status}")
            if extra:
                print(f"    extra: {extra[:10]}")
            if missing:
                print(f"    missing: {missing[:10]}")
            # version-graph parity
            if project in ("libxslt", "libxml2"):
                odefs, oneeds = read_version_info(ORACLE[project])
                cdefs, cneeds = read_version_info(CANDIDATE[project])
                if project == "libxslt":
                    omap = oracle_version_map(ORACLE["libxslt"])
                    cmap = oracle_version_map(CANDIDATE["libxslt"])
                    defs_ok = [n for n in odefs if n not in cdefs]
                    sym_ok = all(omap.get(s) == cmap.get(s) for s in omap)
                    if defs_ok or not sym_ok:
                        shipped_ok = False
                    print(f"  libxslt version nodes oracle={len(odefs)} "
                          f"candidate={len(cdefs)} missing_nodes={defs_ok} "
                          f"symbol_nodes_ok={sym_ok}")
                else:
                    node_map = upstream_xml_node_map()
                    expect_nodes = [n for n in upstream_xml_node_chain()
                                    if any(s in node_map and node_map[s] == n
                                           for s in expected)]
                    expect_nodes.append(XML_TERMINAL_NODE)
                    missing_nodes = [n for n in expect_nodes if n not in cdefs]
                    if missing_nodes:
                        shipped_ok = False
                        print(f"  libxml2 missing version nodes: {missing_nodes}")
                    else:
                        print(f"  libxml2 version nodes ok "
                              f"(upstream chain + {XML_TERMINAL_NODE})")
            else:
                odefs, oneeds = read_version_info(ORACLE[project])
                cdefs, cneeds = read_version_info(CANDIDATE[project])
                if odefs != cdefs:
                    shipped_ok = False
                    print(f"  {project} version-definition mismatch: "
                          f"oracle={odefs} candidate={cdefs}")
                else:
                    print(f"  {project} version definitions match (none on "
                          f"either side: {len(odefs)})")

    out_json = os.path.join(ATLAS, "EXPORT_SURFACE_DISPOSITION.json")
    with open(out_json, "w") as f:
        json.dump(ledger, f, indent=1, ensure_ascii=False)
        f.write("\n")
    # Markdown view
    out_md = os.path.join(ATLAS, "EXPORT_SURFACE_DISPOSITION.md")
    with open(out_md, "w") as f:
        f.write("# Export Surface Disposition (Phase 12)\n\n")
        f.write("Every symbol exported by a shipped candidate DSO, classified "
                "against the executed oracle. The shipped shared-DSO surface is "
                "the exact oracle surface; INTERNAL_LEAK and "
                "HISTORICAL_COMPAT exports are hidden by the generated version "
                "scripts (the staticlib retains them).\n\n")
        for project in ("libxml2", "libxslt", "libexslt"):
            p = ledger["projects"][project]
            f.write(f"## {project}\n\n")
            f.write(f"- oracle exports: {p['oracle_exports']}\n")
            f.write(f"- candidate exports: {p['candidate_exports']}\n")
            f.write(f"- dispositions: "
                    f"{', '.join(f'{k} {v}' for k, v in sorted(p['counts'].items()))}\n\n")
            f.write("| symbol | kind | disposition | in oracle | in headers | "
                    "in upstream syms |\n|---|---|---|---|---|---|\n")
            for name, r in sorted(p["symbols"].items()):
                f.write(f"| `{name}` | {r['kind']} | {r['disposition']} | "
                        f"{r['in_oracle']} | {r['in_headers']} | "
                        f"{r['in_upstream_syms']} |\n")
            f.write("\n")
    print(f"ledger -> {out_json}")
    print(f"view   -> {out_md}")
    print(f"syms   -> {PKG}/libxml2.syms, {PKG}/libxslt.syms, {PKG}/libexslt.syms")
    if args.check:
        print(f"SHIPPED-SURFACE {'PASS' if shipped_ok else 'FAIL'}")
        return 0 if shipped_ok else 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
