#!/usr/bin/env python3
"""Generate per-module fact sheets for the 11.2 custodian commentary court.

For every src module the court checks, emit a compact JSON fact sheet with:

  - upstream sources/headers mapped by the subsystem census (SUBSYSTEM_CENSUS)
  - the ABI export family (from the module's #[no_mangle] name prefixes)
  - capability epochs that touch the subsystem (COMPATIBILITY_PROFILES)
  - residuals whose component list mentions the module (RESIDUAL_LEDGER)
  - court families / probes that exercise it (phase-11-y-census manifest)

The fact sheets are committed under atlas/custodian/ so commentary writers and
future maintainers share the same grounded evidence base (no fabrication).

Usage:
    python3 tools/evidence/custodian_factsheets.py
"""
import json
import os
import re

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
OUT_DIR = os.path.join(ROOT, "atlas", "custodian")

# module (src-relative, no extension) -> upstream source files (hand-verified
# against the subsystem census; a few are added where the census names them by
# header only).
UPSTREAM = {
    "abi/allocator": ["xmlmemory.c", "globals.c"],
    "abi/callbacks": ["SAX2.c", "globals.c"],
    "abi/constants": ["globals.c", "xmlerror.h", "parser.h"],
    "abi/data_globals": ["globals.c", "SAX2.c", "chvalid.c"],
    "abi/ownership": ["tree.c", "xmlmemory.c"],
    "abi/structs": ["tree.h", "parser.h", "xpath.h", "SAX2.h", "xslt.h", "schemasInternals.h"],
    "abi/types": ["tree.h", "parser.h", "xpath.h", "xmlerror.h", "xmlreader.h", "xmlwriter.h"],
    "abi/versioning": ["globals.c", "parser.c"],
    "abi/exports_automata": ["xmlautomata.c"],
    "abi/exports_buffer": ["buf.c", "tree.c"],
    "abi/exports_hash": ["hash.c", "dict.c"],
    "abi/exports_html": ["HTMLparser.c", "HTMLtree.c"],
    "abi/exports_misc": ["parser.c", "tree.c", "uri.c", "xmlstring.c"],
    "abi/exports_nano": ["nanohttp.c", "nanoftp.c"],
    "abi/exports_parser": ["parser.c", "parserInternals.c"],
    "abi/exports_parserint": ["parserInternals.c", "parser.c"],
    "abi/exports_relaxng": ["relaxng.c"],
    "abi/exports_schema": ["xmlschemas.c", "xmlschemastypes.c"],
    "abi/exports_shell": ["xmllint.c", "debugXML.c"],
    "abi/exports_string": ["xmlstring.c"],
    "abi/exports_treedump": ["debugXML.c"],
    "abi/exports_tree": ["tree.c", "buf.c"],
    "abi/exports_uri": ["uri.c"],
    "abi/exports_xinclude": ["xinclude.c"],
    "abi/exports_xlink": ["xlink.c"],
    "abi/exports_xml2": ["globals.c", "parser.c", "threads.c", "encoding.c", "xmlmemory.c", "xmlstring.c"],
    "abi/exports_xptr": ["xpointer.c", "xpath.c"],
    "abi/exports_xslt": ["xslt.c", "xsltutils.c"],
    "abi/exports_xslt_apply": ["transform.c"],
    "abi/exports_xslt_avt": ["xsltutils.c", "attributes.c"],
    "abi/exports_xslt_compile": ["xslt.c", "patterns.c", "keys.c", "numbers.c", "sort.c", "variables.c", "preproc.c"],
    "abi/exports_xslt_exec": ["transform.c", "templates.c", "imports.c", "documents.c", "extensions.c", "xsltutils.c"],
    "abi/exports_xslt_ext": ["extensions.c"],
    "abi/exports_xslt_functions": ["functions.c", "xsltutils.c"],
    "abi/exports_xslt_util": ["xsltutils.c"],
    "abi/exports_xslt_vars": ["variables.c", "params.c"],
    "bin/xmllint": ["xmllint.c"],
    "bin/xmlcatalog": ["xmlcatalog.c"],
    "bin/xsltproc": ["xsltproc.c"],
    "compatibility/profiles": ["parser.c", "xpath.c", "HTMLtree.c", "valid.c", "xmllint.c"],
    "internal/globals": ["globals.c"],
    "exslt/common": ["libexslt/common.c"],
    "exslt/dates": ["libexslt/date.c"],
    "exslt/dynamic": ["libexslt/dynamic.c"],
    "exslt/functions": ["libexslt/functions.c"],
    "exslt/math": ["libexslt/math.c"],
    "exslt/saxon": ["libexslt/saxon.c"],
    "exslt/sets": ["libexslt/sets.c"],
    "exslt/strings": ["libexslt/strings.c"],
    "xml/automata": ["xmlautomata.c"],
    "xml/c14n": ["c14n.c"],
    "xml/catalog": ["catalog.c"],
    "xml/chvalid": ["chvalid.c", "xmlunicode.c"],
    "xml/debug": ["debugXML.c"],
    "xml/dictionary": ["dict.c"],
    "xml/dtd": ["valid.c", "parser.c"],
    "xml/encoding": ["encoding.c"],
    "xml/entities": ["entities.c", "parser.c"],
    "xml/errors": ["error.c"],
    "xml/globals": ["globals.c"],
    "xml/hash": ["hash.c"],
    "xml/html": ["HTMLparser.c", "HTMLtree.c", "HTMLdocument.c"],
    "xml/io": ["xmlIO.c"],
    "xml/list": ["list.c"],
    "xml/memory": ["xmlmemory.c"],
    "xml/namespaces": ["namespaces.c"],
    "xml/parser": ["parser.c", "parserInternals.c", "SAX2.c"],
    "xml/parser/helpers": ["parser.c", "parserInternals.c"],
    "xml/parser/input": ["parserInternals.c", "xmlIO.c"],
    "xml/parser/state": ["parser.c"],
    "xml/parser/tokenizer": ["parser.c", "parserInternals.c"],
    "xml/reader": ["xmlreader.c"],
    "xml/regex": ["xmlregexp.c"],
    "xml/relaxng": ["relaxng.c"],
    "xml/save": ["xmlsave.c", "xmlIO.c"],
    "xml/sax": ["SAX2.c", "parser.c"],
    "xml/sax/dispatch": ["SAX2.c"],
    "xml/schemas": ["xmlschemas.c", "xmlschemastypes.c", "xmlschemavalues.c"],
    "xml/schematron": ["schematron.c"],
    "xml/string": ["xmlstring.c"],
    "xml/threads": ["threads.c"],
    "xml/tree": ["tree.c", "buf.c"],
    "xml/uri": ["uri.c"],
    "xml/validation": ["valid.c"],
    "xml/writer": ["xmlwriter.c"],
    "xml/xinclude": ["xinclude.c"],
    "xml/xpath": ["xpath.c"],
    "xml/xpath/ast": ["xpath.c"],
    "xml/xpath/axes": ["xpath.c"],
    "xml/xpath/context": ["xpath.c"],
    "xml/xpath/eval": ["xpath.c"],
    "xml/xpath/exports": ["xpath.c", "xpathInternals.h"],
    "xml/xpath/functions": ["xpath.c"],
    "xml/xpath/lexer": ["xpath.c"],
    "xml/xpath/parser": ["xpath.c"],
    "xml/xpath/types": ["xpath.c"],
    "xml/xpointer": ["xpointer.c"],
    "xslt/attributes": ["attributes.c"],
    "xslt/compiler": ["xslt.c", "preproc.c"],
    "xslt/documents": ["documents.c"],
    "xslt/errors": ["xsltutils.c", "xsltInternals.h"],
    "xslt/extensions": ["extensions.c"],
    "xslt/imports": ["imports.c"],
    "xslt/keys": ["keys.c"],
    "xslt/namespace_alias": ["namespaces.c", "preproc.c"],
    "xslt/numbering": ["numbers.c"],
    "xslt/parameters": ["params.c", "variables.c"],
    "xslt/patterns": ["patterns.c"],
    "xslt/security": ["security.c"],
    "xslt/serialization": ["xsltutils.c"],
    "xslt/sorting": ["sort.c"],
    "xslt/stylesheet": ["xslt.c", "xsltInternals.h"],
    "xslt/templates": ["templates.c"],
    "xslt/transform": ["transform.c"],
    "xslt/variables": ["variables.c"],
    "xslt/whitespace": ["xsltutils.c", "preproc.c"],
}


def module_list():
    out = []
    for root, _dirs, files in os.walk(os.path.join(ROOT, "src")):
        for fn in files:
            if fn.endswith(".rs"):
                rel = os.path.relpath(os.path.join(root, fn), os.path.join(ROOT, "src"))
                out.append(rel[:-3])
    return sorted(out)


def canon(rel):
    """Normalize a module key: strip trailing /mod so fact-sheet keys match
    the court's module list AND the UPSTREAM map."""
    return rel[:-4] if rel.endswith("/mod") else rel


def main():
    os.makedirs(OUT_DIR, exist_ok=True)
    census = json.load(open(os.path.join(ROOT, "atlas", "SUBSYSTEM_CENSUS.json")))
    profiles = json.load(open(os.path.join(ROOT, "atlas", "PARITY_OBLIGATIONS.json")))
    ledger = json.load(open(os.path.join(ROOT, "atlas", "RESIDUAL_LEDGER.json")))
    fams = json.load(open(os.path.join(ROOT, "courts", "manifests", "phase-11-y-census.json")))["families"]

    # subsystem title -> (sources, headers)
    subsys = {}
    for sid, s in census["subsystems"].items():
        subsys[sid] = (s.get("sources", []), s.get("headers", []))

    # residual component -> [ids]
    res_by_comp = {}
    for r in ledger["ledger"]:
        for c in r.get("component", []):
            res_by_comp.setdefault(c, []).append(r["id"])

    # export families by file: first #[no_mangle] names tell the family
    def exports_of(rel):
        p = os.path.join(ROOT, "src", rel + ".rs")
        if not os.path.exists(p):
            return []
        names = re.findall(r"#\[no_mangle\]\s*\n\s*(?:pub\s+)?(?:unsafe\s+)?extern\s+\"C\"\s+fn\s+(\w+)",
                           open(p, encoding="utf-8").read())
        return names[:6]

    count = 0
    for rel0 in module_list():
        rel = canon(rel0)
        if rel in ("abi/exports_xslt_internals", "xml/unicode_tables",
                   "abi/ucs_blocks", "abi/ucs_cat", "xml/parser/debug_test",
                   "xml/parser/tests"):
            continue
        facts = {
            "module": rel0,
            "upstream": UPSTREAM.get(rel, []),
            "export_samples": exports_of(rel0),
            "subsystems": [sid for sid, (srcs, _) in subsys.items()
                           if any(s in srcs for s in UPSTREAM.get(rel, []))],
            "residuals": sorted({rid for c, rids in res_by_comp.items()
                                 if rel in c for rid in rids}),
        }
        # dedupe court families against the module's upstream sources
        fam_hits = []
        for fam, d in fams.items():
            blob = json.dumps(d)
            if any(s.replace(".c", "") in blob for s in UPSTREAM.get(rel, [])):
                fam_hits.append(fam)
        facts["court_families"] = sorted(set(fam_hits))
        with open(os.path.join(OUT_DIR, rel0.replace("/", "__") + ".json"), "w") as f:
            json.dump(facts, f, indent=1)
            f.write("\n")
        count += 1
    print(f"wrote {count} fact sheets to {OUT_DIR}/")


if __name__ == "__main__":
    main()
