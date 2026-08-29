#!/usr/bin/env python3
"""Generate the CLI differential court casefiles (§43).

Phase 9/10 differential evidence promotion (11.1-A): the 44-case xmllint +
xmlcatalog suite (formerly target/difftest.sh) and the 12-case xsltproc corpus
become committed, schema-valid casefiles under courts/suites/cli/. Deleting
target/ must never delete this evidence.

Regenerate with:  tools/evidence/gen_cli_casefiles.py
Runner:           courts/cli/court-runner.sh
Expected captures:courts/expected/cli/<tool>/<case>.{out,err,exit}
Aggregates:       courts/manifests/phase-09.json, phase-10.json
"""
import json
import os
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SUITES = os.path.join(ROOT, "courts", "suites", "cli")

XML2_ORACLE = "libxml2-2.15.3"
XSLT_ORACLE = "libxslt-1.1.45"


def case(suite, case_id, purpose, argv, oracle, provenance, document=None,
         stdin=None, steps=None, created_file=None, surface=None, tags=None):
    inputs = {"argv": argv}
    if document:
        inputs["document"] = document
    if stdin:
        inputs["stdin_script"] = stdin
    if steps:
        inputs["steps"] = steps
    if created_file:
        inputs["created_file"] = created_file
    doc = {
        "case_id": case_id,
        "surface": surface or {"xmllint": "cli-xmllint", "xmlcatalog": "cli-xmlcatalog",
                               "xsltproc": "cli-xsltproc"}[suite],
        "purpose": purpose,
        "oracle_profile": oracle,
        "inputs": inputs,
        "observables": ["exit_status", "stdout_hash", "stderr_hash"],
        "normalization": [],
        "provenance": provenance,
        "tags": tags or ["differential", "cli"],
    }
    outdir = os.path.join(SUITES, suite)
    os.makedirs(outdir, exist_ok=True)
    path = os.path.join(outdir, f"{case_id}.json")
    with open(path, "w", encoding="utf-8") as f:
        json.dump(doc, f, indent=2, ensure_ascii=False)
        f.write("\n")
    return path


def main():
    n = 0

    # ── xmllint (34 cases: 31 from target/difftest.sh + 3 entity/residual cases)
    x = [
        ("CLI-XMLLINT-0001", "default-dump", "plain parse + dump", ["ok.xml"], None),
        ("CLI-XMLLINT-0002", "noout", "--noout suppresses output", ["ok.xml", "--noout"], None),
        ("CLI-XMLLINT-0003", "format", "--format reformats", ["ok.xml", "--format"], None),
        ("CLI-XMLLINT-0004", "format-fmt", "--format on compact input", ["fmt.xml", "--format"], None),
        ("CLI-XMLLINT-0005", "valid-ok", "--valid on valid doc", ["valid.xml", "--valid"], None),
        ("CLI-XMLLINT-0006", "valid-bad", "--valid on invalid doc (exit 3 epoch)", ["invalid.xml", "--valid"], None),
        ("CLI-XMLLINT-0007", "valid-nodtd", "--valid with no DTD (exit epoch 4/3/0)", ["ok.xml", "--valid"], None),
        ("CLI-XMLLINT-0008", "parse-error", "well-formedness error diagnostics", ["bad.xml"], None),
        ("CLI-XMLLINT-0009", "xpath-nodeset", "--xpath node-set dump", ["lib.xml", "--xpath", "//book"], None),
        ("CLI-XMLLINT-0010", "xpath-string", "--xpath string()", ["lib.xml", "--xpath", "string(//title)"], None),
        ("CLI-XMLLINT-0011", "xpath-count", "--xpath count()", ["lib.xml", "--xpath", "count(//book)"], None),
        ("CLI-XMLLINT-0012", "xpath-attr", "--xpath attribute node-set", ["lib.xml", "--xpath", "@id"], None),
        ("CLI-XMLLINT-0013", "xpath-empty", "--xpath empty node-set (exit 10/0/11 epoch)", ["lib.xml", "--xpath", "//zzz"], None),
        ("CLI-XMLLINT-0014", "xpath0", "--xpath0 (no newline separators)", ["lib.xml", "--xpath0", "//book"], None),
        ("CLI-XMLLINT-0015", "xpath-noout", "--xpath with --noout", ["lib.xml", "--xpath", "count(//book)", "--noout"], None),
        ("CLI-XMLLINT-0016", "debug", "--debug tree dump", ["ok.xml", "--debug"], None),
        ("CLI-XMLLINT-0017", "copy", "--copy deep copy dump", ["ok.xml", "--copy"], None),
        ("CLI-XMLLINT-0018", "c14n", "--c14n canonicalization", ["lib.xml", "--c14n"], None),
        ("CLI-XMLLINT-0019", "html", "--html parse + dump", ["page.html", "--html"], None),
        ("CLI-XMLLINT-0020", "html-noout", "--html --noout", ["page.html", "--html", "--noout"], None),
        ("CLI-XMLLINT-0021", "dropdtd", "--dropdtd removes DTD", ["valid.xml", "--dropdtd"], None),
        ("CLI-XMLLINT-0022", "bad-option", "unknown option handling", ["ok.xml", "--bogus"], None),
        ("CLI-XMLLINT-0023", "no-args", "no arguments usage/exit", [], None),
        ("CLI-XMLLINT-0024", "version", "--version identity output", ["--version"], None),
        ("CLI-XMLLINT-0025", "xinclude-lib", "--xinclude processing", ["lib.xml", "--xinclude"], None),
        ("CLI-XMLLINT-0026", "encode-utf8", "--encode UTF-8 round-trip", ["ok.xml", "--encode", "UTF-8"], None),
        ("CLI-XMLLINT-0027", "xmlout", "--xmlout on HTML-ish input", ["lib.xml", "--xmlout"], None),
        ("CLI-XMLLINT-0028", "pedantic", "--pedantic mode", ["ok.xml", "--pedantic"], None),
        ("CLI-XMLLINT-0029", "noent", "--noent entity substitution", ["esc.xml", "--noent"], None),
        ("CLI-XMLLINT-0030", "recover", "--recover on malformed input", ["bad.xml", "--recover"], None),
        ("CLI-XMLLINT-0031", "xpath-attr2", "--xpath attribute on namespaced doc", ["ns.xml", "--xpath", "@id"], None),
        ("CLI-XMLLINT-0032", "debug-ent", "--debug entity decl content (R-000119)", ["dclent.xml", "--debug"], ["residual-R-000119", "differential"]),
        ("CLI-XMLLINT-0033", "debug-attrent", "--debug entity-containing attribute (R-000120)", ["attrent.xml", "--debug"], ["residual-R-000120", "differential"]),
        ("CLI-XMLLINT-0034", "attr-markup-entity", "markup entity in attribute value (R-000121)", ["markattr.xml"], ["residual-R-000121", "differential"]),
    ]
    for cid, name, purpose, argv, prov in x:
        prov = prov or ["upstream-documentation", "phase-10-differential-corpus"]
        case("xmllint", cid, purpose, argv, XML2_ORACLE, prov,
             document=argv[0] if argv and not argv[0].startswith("-") else None,
             tags=["differential", "xmllint"])
        n += 1

    # ── xmlcatalog (11 cases; @TMP@ substituted per side by the runner)
    # rows: (cid, name, purpose, argv, steps, created_file, stdin)
    c = [
        ("CLI-XMLCATALOG-0001", "create-file",
         "xmlcatalog --create writes the catalog skeleton",
         None, [["--create", "@TMP@/cat.xml"]], "@TMP@/cat.xml", None),
        ("CLI-XMLCATALOG-0002", "create-stdout",
         "--create with --noout",
         None, [["--create", "@TMP@/scratch.xml", "--noout"]], None, None),
        ("CLI-XMLCATALOG-0003", "no-args",
         "no arguments usage/exit", [], None, None, None),
        ("CLI-XMLCATALOG-0004", "bad-option",
         "unknown option handling", ["--bogus"], None, None, None),
        ("CLI-XMLCATALOG-0005", "dump-empty",
         "--create dumps empty catalog to stdout", None,
         [["--create", "@TMP@/empty2.xml"]], None, None),
        ("CLI-XMLCATALOG-0006", "add-system-public",
         "--add system + public entries", None,
         [["--create", "@TMP@/cat.xml"],
          ["--noout", "--add", "system", "http://ex.org/a", "http://ex.org/a.xml", "@TMP@/cat.xml"],
          ["--noout", "--add", "public", "-//OASIS//DTD X//EN", "http://ex.org/x.dtd", "@TMP@/cat.xml"]],
         "@TMP@/cat.xml", None),
        ("CLI-XMLCATALOG-0007", "resolve-system",
         "--resolve system identifier", None,
         [["--create", "@TMP@/cat.xml"],
          ["--noout", "--add", "system", "http://ex.org/a", "http://ex.org/a.xml", "@TMP@/cat.xml"],
          ["--noout", "--resolve", "http://ex.org/a", "@TMP@/cat.xml"]],
         None, None),
        ("CLI-XMLCATALOG-0008", "resolve-public",
         "--resolve public identifier", None,
         [["--create", "@TMP@/cat.xml"],
          ["--noout", "--add", "public", "-//OASIS//DTD X//EN", "http://ex.org/x.dtd", "@TMP@/cat.xml"],
          ["--noout", "--resolve", "-//OASIS//DTD X//EN", "@TMP@/cat.xml"]],
         None, None),
        ("CLI-XMLCATALOG-0009", "dump-populated",
         "plain dump of populated catalog", None,
         [["--create", "@TMP@/cat.xml"],
          ["--noout", "--add", "system", "http://ex.org/a", "http://ex.org/a.xml", "@TMP@/cat.xml"],
          ["--noout", "--add", "public", "-//OASIS//DTD X//EN", "http://ex.org/x.dtd", "@TMP@/cat.xml"],
          ["@TMP@/cat.xml"]],
         None, None),
        ("CLI-XMLCATALOG-0010", "shell-commands",
         "interactive shell: resolve/system/public/del/dump",
         None,
         [["--create", "@TMP@/cat.xml"],
          ["--noout", "--add", "system", "http://ex.org/a", "http://ex.org/a.xml", "@TMP@/cat.xml"],
          ["--shell", "@TMP@/cat.xml"]],
         None,
         "resolve http://ex.org/a\nsystem http://ex.org/a\npublic -//OASIS//DTD X//EN\ndel http://ex.org/a\ndump\nexit\n"),
        ("CLI-XMLCATALOG-0011", "del-entry",
         "--del removes an entry", None,
         [["--create", "@TMP@/cat.xml"],
          ["--noout", "--add", "system", "http://ex.org/a", "http://ex.org/a.xml", "@TMP@/cat.xml"],
          ["--noout", "--del", "http://ex.org/a", "@TMP@/cat.xml"],
          ["@TMP@/cat.xml"]],
         "@TMP@/cat.xml", None),
    ]
    for cid, name, purpose, argv, steps, created, stdin in c:
        prov = ["upstream-documentation", "phase-10-differential-corpus"]
        if argv is None and steps:
            argv = steps[0]
        case("xmlcatalog", cid, purpose, argv or [], XML2_ORACLE, prov,
             steps=steps, created_file=created, stdin=stdin, tags=["differential", "xmlcatalog"])
        n += 1

    # ── xsltproc (12 cases from the Phase 9 corpus)
    s = [
        ("CLI-XSLTPROC-0001", "basic", "basic transform: for-each, AVT, value-of, if, count", ["basic.xsl", "doc.xml"]),
        ("CLI-XSLTPROC-0002", "avt", "XPath core functions: count, substring, arithmetic, concat", ["avt.xsl", "doc.xml"]),
        ("CLI-XSLTPROC-0003", "exsl", "exsl:node-set on RTF + math:/set:/str:", ["exsl.xsl", "doc.xml"]),
        ("CLI-XSLTPROC-0004", "pred", "predicates: position, equality, attribute", ["pred.xsl", "doc.xml"]),
        ("CLI-XSLTPROC-0005", "attr", "attribute string-values in AVTs and tests", ["attr.xsl", "doc.xml"]),
        ("CLI-XSLTPROC-0006", "if", "xsl:if/xsl:when boolean conversion (node-set/number/string)", ["if.xsl", "doc.xml"]),
        ("CLI-XSLTPROC-0007", "num", "xsl:number with format", ["num.xsl", "doc.xml"]),
        ("CLI-XSLTPROC-0008", "sort", "xsl:sort descending by attribute", ["sort.xsl", "doc.xml"]),
        ("CLI-XSLTPROC-0009", "keys", "xsl:key + key() lookup", ["keys.xsl", "doc.xml"]),
        ("CLI-XSLTPROC-0010", "ct", "call-template with with-param", ["ct.xsl", "doc.xml"]),
        ("CLI-XSLTPROC-0011", "html", "method=html output", ["html.xsl", "doc.xml"]),
        ("CLI-XSLTPROC-0012", "params", "global params via --param", ["--param", "who", "'World'", "--param", "times", "2", "params.xsl", "doc.xml"]),
    ]
    for cid, name, purpose, argv in s:
        case("xsltproc", cid, purpose, argv, XSLT_ORACLE,
             ["upstream-documentation", "phase-09-differential-corpus"],
             document="xslt/doc.xml",
             tags=["differential", "xsltproc"])
        n += 1

    print(f"generated {n} CLI casefiles under {SUITES}")


if __name__ == "__main__":
    sys.exit(main())
