#!/usr/bin/env python3
"""Split the safety-target list into per-agent assignment files."""
import json
import os

d = json.load(open("/tmp/safety_targets.json"))
t = d["fn_targets"]
undoc = d["undoc_unsafe_fns"]

# Agent -> list of modules (disjoint write scopes, balanced by fn count)
ASSIGN = {
    "A": ["src/xml/validation/mod.rs", "src/xml/io/mod.rs", "src/xml/parser/state.rs"],
    "B": ["src/xml/c14n/mod.rs", "src/xml/dtd/mod.rs", "src/xml/tree/mod.rs"],
    "C": ["src/xml/html/mod.rs", "src/xml/reader/mod.rs", "src/xml/relaxng/mod.rs"],
    "D": ["src/xml/entities/mod.rs", "src/xml/xinclude/mod.rs",
          "src/xslt/transform/mod.rs", "src/xml/xpointer/mod.rs"],
    "E": ["src/xslt/numbering/mod.rs", "src/xslt/patterns/mod.rs",
          "src/xml/encoding/mod.rs", "src/xml/writer/mod.rs"],
    "F": ["src/xml/catalog/mod.rs", "src/xml/schematron/mod.rs",
          "src/xml/debug/mod.rs", "src/abi/allocator.rs", "src/xml/automata/mod.rs"],
    "G": ["src/xml/uri/mod.rs", "src/xml/parser/tests.rs", "src/xml/regex/mod.rs",
          "src/xml/hash/mod.rs", "src/xml/list/mod.rs", "src/xml/chvalid.rs",
          "src/xml/dictionary/mod.rs", "src/xml/errors/mod.rs",
          "src/xml/globals/mod.rs", "src/xml/string.rs", "src/xml/save.rs",
          "src/xml/sax/default.rs", "src/xml/xpath/context.rs",
          "src/xml/xpath/functions.rs", "src/xml/xpath/parser_context.rs",
          "src/xml/xpath/axes.rs", "src/xml/xpath/eval.rs", "src/xml/xpath/types.rs",
          "src/xml/memory/mod.rs", "src/xml/parser/input.rs",
          "src/xml/parser/debug_test.rs", "src/bin/xmllint.rs",
          "src/bin/xmlcatalog.rs", "src/abi/data_globals.rs", "src/abi/ownership.rs",
          "src/xml/schemas/mod.rs"],
    "H": ["src/xslt/serialization/mod.rs", "src/xslt/variables/mod.rs",
          "src/xslt/compiler/mod.rs", "src/xslt/errors/mod.rs",
          "src/xslt/parameters/mod.rs", "src/xslt/security/mod.rs",
          "src/xslt/stylesheet/mod.rs", "src/xslt/extensions/mod.rs",
          "src/xslt/namespace_alias/mod.rs", "src/xslt/whitespace/mod.rs",
          "src/xslt/keys/mod.rs", "src/xslt/sorting/mod.rs",
          "src/xslt/attributes/mod.rs", "src/xslt/documents/mod.rs",
          "src/xslt/imports/mod.rs", "src/bin/xsltproc.rs",
          "src/exslt/strings/mod.rs", "src/exslt/sets/mod.rs",
          "src/exslt/common/mod.rs", "src/exslt/dates/mod.rs",
          "src/exslt/math/mod.rs", "src/exslt/dynamic/mod.rs"],
}

os.makedirs("/tmp/assign", exist_ok=True)
for agent, mods in ASSIGN.items():
    fn_t = {m: t[m] for m in mods if m in t}
    undoc_agent = [(rel, fn) for (rel, fn) in undoc if rel in mods]
    out = {"fn_targets": fn_t, "undoc_unsafe_fns": undoc_agent}
    n = sum(len(v) for v in fn_t.values())
    with open(f"/tmp/assign/{agent}.json", "w") as f:
        json.dump(out, f, indent=1)
    print(f"agent {agent}: {len(fn_t)} modules, {n} fns, {len(undoc_agent)} undoc fns")
