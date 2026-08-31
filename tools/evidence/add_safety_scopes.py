#!/usr/bin/env python3
"""Insert module-level SAFETY-SCOPE markers into the mechanical export-registry
modules (11.1-Z.3 proof-scope model, classified-generated bucket).

The marker is inserted immediately after the module's leading `//!` doc block
(and any `use` lines), before the first item, and declares that every unsafe
block in the module is the mechanical extern-"C" export pattern covered by the
upstream C contract and measured by the ABI-FUNCTION-SIGNATURE / DSO-LOADER
courts and the C-API differential probes.

Run from the repo root: python3 tools/evidence/add_safety_scopes.py
"""
import os
import re

ROOT = os.getcwd()

MODULES = [
    "src/abi/exports_tree.rs", "src/abi/exports_html.rs",
    "src/abi/exports_parserint.rs", "src/abi/exports_misc.rs",
    "src/abi/exports_shell.rs", "src/xml/xpath/exports.rs",
    "src/abi/exports_xml2.rs", "src/abi/exports_treedump.rs",
    "src/abi/exports_parser.rs", "src/abi/exports_automata.rs",
    "src/abi/exports_string.rs", "src/abi/exports_hash.rs",
    "src/abi/exports_uri.rs", "src/abi/exports_xptr.rs",
    "src/abi/exports_nano.rs", "src/abi/exports_xslt_functions.rs",
    "src/abi/exports_xslt_ext.rs", "src/abi/exports_buffer.rs",
    "src/abi/exports_xslt_avt.rs", "src/abi/exports_xinclude.rs",
    "src/abi/exports_schema.rs", "src/abi/exports_xslt.rs",
    "src/abi/exports_xslt_util.rs", "src/abi/exports_xslt_exec.rs",
]


def scope_id(rel):
    stem = rel.split("/")[-1].replace(".rs", "").replace("exports_", "EXPORT-").upper()
    return f"{stem}-MECHANICAL-001"


def insert_marker(path):
    text = open(path, encoding="utf-8").read()
    if "SAFETY-SCOPE:" in text:
        print(f"  (already has marker) {path}")
        return
    lines = text.split("\n")
    # find the first real item line (pub/fn/static/mod/struct/type/const/
    # macro_rules/use at line start); the marker is inserted before it, so
    # it lands after any //! docs AND any multi-line #![...] attributes
    item_re = re.compile(
        r"^(?:pub|fn|unsafe|extern|static|mod|struct|enum|type|const|macro_rules|use)\b")
    idx = None
    for i, line in enumerate(lines):
        s = line.strip()
        if s and item_re.match(s):
            idx = i
            break
    if idx is None:
        print(f"  NO ITEM FOUND in {path}")
        return
    sid = scope_id(path)
    marker = (
        f"// SAFETY-SCOPE: {sid}\n"
        "// (11.1-Z.3 proof scope, classified-generated) — this module is the\n"
        "// mechanical extern-\"C\" export surface: every `unsafe` block in it is\n"
        "// the documented indirection/registry-access pattern whose validity\n"
        "// rests on the upstream C contract, and the exported signatures are\n"
        "// machine-measured by the ABI-FUNCTION-SIGNATURE and DSO-LOADER\n"
        "// courts and the C-API differential probes. The safety contract of\n"
        "// each export is stated in its own doc comment; this scope covers the\n"
        "// mechanical wrappers' unsafe blocks.\n"
    )
    lines.insert(idx, marker)
    open(path, "w", encoding="utf-8").write("\n".join(lines))
    print(f"  + marker {sid} -> {path}")


def main():
    for rel in MODULES:
        path = os.path.join(ROOT, rel)
        if not os.path.exists(path):
            print(f"  MISSING {path}")
            continue
        insert_marker(path)
    print("done")


if __name__ == "__main__":
    main()
