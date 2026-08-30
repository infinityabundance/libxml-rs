#!/usr/bin/env python3
"""11.1-P — Standards vs Upstream Semantics reconciliation.

For every standards-backed subsystem the plan calls out (session3.md L234591)
this produces a three-way record:

    SPECIFICATION       — what the standard requires (W3C/ISO reference +
                          clause)
    UPSTREAM ORACLE     — what libxml2/libxslt actually does (observed
                          behavior + archaeology citation)
    LIBXML-RS           — what the candidate does (observed behavior)

plus a classification and, where the candidate deviates from the oracle or
the oracle deviates from the spec, an explicit divergence record. For
drop-in parity the upstream observable behavior takes precedence unless a
documented safety constraint requires a safe divergence.

Observable facts below were verified with the system oracle (libxml2 2.15.3 /
libxslt 1.1.45 / xmllint / xsltproc) and the candidate (target/debug/
xmllint / xsltproc / liblibxml_rs.so) on 2026-08-30; probe inputs are
recorded inline in each entry's evidence.

Outputs:
    atlas/standards/STANDARDS_RECONCILIATION.json  (canonical)
    atlas/standards/STANDARDS.md                   (generated view)

Usage:
    python3 tools/evidence/standards_reconciliation.py
"""

import datetime
import json
import os

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
OUT_JSON = os.path.join(ROOT, "atlas", "standards", "STANDARDS_RECONCILIATION.json")
OUT_MD = os.path.join(ROOT, "atlas", "standards", "STANDARDS.md")

# Classification vocabulary (§12 STANDARDS.md)
# STANDARD_CONFORMING | UPSTREAM_EXTENSION | UPSTREAM_QUIRK | HISTORICAL_BUG
# | VERSION_SPECIFIC | PLATFORM_SPECIFIC | UNRESOLVED | SAFE_DIVERGENCE
# | CANDIDATE_DIVERGENCE

AREAS = [
 dict(
  id="xml-1.0", title="XML 1.0 (Fifth Edition)", spec="W3C-XML-1.0",
  spec_behavior="Well-formedness constraints: character production, element "
                "type match, comments (no '--' inside), no '<' in attribute "
                "values, proper nesting, entity declaration rules.",
  oracle_behavior="Substantially conforming; the recovery mode (XML_PARSE_RECOVER) "
                  "is a deliberate UPSTREAM_EXTENSION that continues parsing "
                  "after well-formedness violations and returns a partial tree. "
                  "In non-recovery mode violations are fatal with exact WFC "
                  "messages ('Double hyphen within comment', 'Unescaped '<' not "
                  "allowed in attributes values', 'PCDATA invalid Char value').",
  candidate_behavior="Non-recovery mode matches the oracle for the diagnostics "
                     "in the ERROR-001 court (48-case corpus, byte-identical). "
                     "Recovery mode matches for the TREE-001 recovery cases. "
                     "However, several WFC checks the oracle raises are silently "
                     "accepted by the candidate outside the court corpus: "
                     "'<!-- a -- b -->', '<a b=\"x < y\"/>', and attribute-value "
                     "character checks.",
  classification="STANDARD_CONFORMING",
  divergence_record=[
   {"kind": "oracle_vs_spec",
    "fact": "xmlParseComment raises 'Double hyphen within comment' (XML_ERR_COMMENT_NOT_FINISHED) — matches XML 1.0 [15] Comments.",
    "upstream": "error raised", "spec": "same", "candidate": "accepted silently"},
   {"kind": "candidate_vs_oracle",
    "fact": "xmllint '<a><!-- a -- b --></a>' -> oracle: 'parser error : Double hyphen within comment'; candidate: no diagnostic, tree built.",
    "impact": "drop-in consumers relying on the WFC diagnostic see different parse results (wellFormed flag, error stream, partial-tree semantics).",
    "residual": "to be closed in 11.1-X with a PARSER-002 court (WFC cluster)"},
  ],
  courts=["ERROR-001", "TREE-001", "PARSER"],
 ),
 dict(id="xml-namespaces", title="Namespaces in XML 1.0 / 1.1", spec="W3C-XML-NS-1.0 / W3C-XML-NS-1.1",
  spec_behavior="xmlns=\"uri\" declares the default namespace; xmlns:p=\"uri\" "
                "declares a prefix; Namespaces 1.0 forbids undeclaring prefixes "
                "(1.1 allows xmlns:p=\"\"), forbids binding xml: to a wrong URI, "
                "and forbids the XML namespace URI as the default namespace.",
  oracle_behavior="xmlParseStartTag2 enforces: 'xmlns:%s: Empty XML namespace is "
                  "not allowed' (XML_NS_ERR_XML_NAMESPACE), 'xml namespace prefix "
                  "mapped to wrong URI', 'xml namespace URI cannot be the default "
                  "namespace', 'reuse of the xmlns namespace name is forbidden', "
                  "'redefinition of the xmlns prefix is forbidden'. Namespace "
                  "nodes have no parent pointer (XPath data model, QUIRK-0002).",
  candidate_behavior="The default-ns relative-URI warning and xmlns=\"\" href "
                     "semantics now match (TREE-001). However the oracle's "
                     "namespace-declaration error checks are not yet raised: "
                     "xmlns:p=\"\", xmlns:xml=\"urn:x\", and the XML namespace "
                     "as default namespace are all accepted silently.",
  classification="STANDARD_CONFORMING",
  divergence_record=[
   {"kind": "candidate_vs_oracle",
    "fact": "xmllint '<a xmlns:p=\"\"><p:b/></a>' -> oracle: 'namespace error : xmlns:p: Empty XML namespace is not allowed'; candidate: accepted.",
    "impact": "namespace error stream and xmlGetLastError differ for drop-in consumers.",
    "residual": "to be closed in 11.1-X with a NS-001 court"},
  ],
  courts=["TREE-001", "XPATH-001"],
 ),
 dict(id="xpath-1.0", title="XPath 1.0", spec="W3C-XPATH-1.0",
  spec_behavior="Data model (document order node-sets), number->string conversion "
                "(NaN/'NaN', +/-Inf/'Infinity'/' -Infinity', shortest decimal), "
                "string->number, substring() rounding via round(), the '|' union "
                "operator in document order.",
  oracle_behavior="Observed 2.15.3: string(1 div 0)='Infinity', string(-1 div 0)="
                  "'-Infinity', string(0 div 0)='NaN', string(-0)='0', "
                  "substring('12345',1.5,2.6)='234'. Union operator yields "
                  "document order in current releases; historical versions "
                  "returned reversed order (VERSION_SPECIFIC, see SEMANTIC_EPOCHS).",
  candidate_behavior="The four number->string probes and substring rounding match "
                     "the oracle byte-for-byte. XSLT-level number formatting "
                     "diverges (see xslt-1.0: value-of 1234567.891).",
  classification="STANDARD_CONFORMING",
  divergence_record=[
   {"kind": "candidate_vs_oracle",
    "fact": "xsltproc '<xsl:value-of select=\"1234567.891\"/>' -> oracle '1234567.891'; candidate '1234567.891000000061467' (Rust float Display instead of XPath shortest form).",
    "impact": "XSLT output text differs; also affects EXSLT string() results.",
    "residual": "to be closed in 11.1-X (XPath number formatting, XPATH-002 court)"},
  ],
  courts=["XPATH-001", "XSLT-001"],
 ),
 dict(id="xpointer", title="XPointer", spec="W3C-XPTR-1.0",
  spec_behavior="xpointer() and xmlns() scheme evaluation over XPath.",
  oracle_behavior="xpointer.c implements the xpointer scheme (and legacy '#' "
                  "fragment handling in xmlXPtrEval); largely frozen upstream.",
  candidate_behavior="Exports xmlXPtrEval/xmlXPtrBuildNodeList/xmlXPtrNewLocationSet "
                     "(census: xpointer 2/2 exports); XPointer evaluation semantics "
                     "covered by the XPATH-001 family.",
  classification="STANDARD_CONFORMING",
  divergence_record=[],
  courts=["XPATH-001"],
 ),
 dict(id="xinclude", title="XInclude 1.0", spec="W3C-XINCLUDE-1.0",
  spec_behavior="xi:include with parse='xml'|'text', xpointer selection, "
                "fallback processing, base-URI fixup.",
  oracle_behavior="xinclude.c implements inclusion with the upstream fixup "
                  "semantics; xmlXIncludeSetResourceLoader hook present in 2.15.3.",
  candidate_behavior="xinclude exports 12/13 (xmlXIncludeSetResourceLoader missing, "
                     "see R-000165); inclusion behavior court-covered by "
                     "XINCLUDE-family tests.",
  classification="STANDARD_CONFORMING",
  divergence_record=[
   {"kind": "candidate_vs_oracle",
    "fact": "census R-000165: xmlXIncludeSetResourceLoader not exported.",
    "impact": "consumers installing a custom resource loader cannot link.",
    "residual": "R-000165"},
  ],
  courts=["XINCLUDE"],
 ),
 dict(id="xml-schema", title="XML Schema 1.0", spec="W3C-XSD-1.0",
  spec_behavior="Full XSD 1.0 structures + datatypes validation.",
  oracle_behavior="libxml2 ships a partial validator (xmlschemas.c) covering "
                  "common constructs; full XSD conformance is not claimed "
                  "upstream (documented partial).",
  candidate_behavior="xsd subsystem 28 oracle DSO exports, 27 candidate "
                     "(xmlSchemaSetResourceLoader missing, R-000165); "
                     "validation engine is a subset.",
  classification="UPSTREAM_EXTENSION",
  divergence_record=[],
  courts=["XSD"],
 ),
 dict(id="relaxng", title="RELAX NG", spec="OASIS RELAX NG / ISO 19757-2",
  spec_behavior="Pattern-based validation with datatype library integration.",
  oracle_behavior="relaxng.c implements the full pattern language; datatype "
                  "integration via xmlRelaxNGSetDatatypeSpecificFuncs.",
  candidate_behavior="relaxng 27 oracle exports, 24 candidate (missing "
                     "xmlRelaxNGSetResourceLoader, xmlRelaxNGValidCtxtClearErrors, "
                     "xmlRelaxParserSetIncLImit — R-000165).",
  classification="STANDARD_CONFORMING",
  divergence_record=[
   {"kind": "candidate_vs_oracle", "fact": "R-000165 missing exports",
    "impact": "link-time gaps for resource-loader / error-clearing consumers.",
    "residual": "R-000165"},
  ],
  courts=["RELAXNG"],
 ),
 dict(id="schematron", title="Schematron", spec="ISO/IEC 19757-3",
  spec_behavior="Rule/assert/report pattern language; XSLT query binding.",
  oracle_behavior="schematron.c evaluates via XSLT (query binding 'xslt').",
  candidate_behavior="schematron 10/10 exports; SchematronSchema engine is "
                     "native-Rust; the schematron NULL-context UB was fixed in "
                     "11.1-O (ASan clean).",
  classification="STANDARD_CONFORMING",
  divergence_record=[],
  courts=["SCHEMATRON"],
 ),
 dict(id="canonical-xml", title="Canonical XML 1.0", spec="W3C-C14N-1.0",
  spec_behavior="Inclusive canonicalization: absolute namespace URIs required, "
                "attributes in lexicographic order, namespace declarations "
                "propagated to every element.",
  oracle_behavior="xmlC14NExecute/xmlC14NDocDumpMemory implement C14N 1.0; the "
                  "relative-URI rule is enforced: xmllint --c14n on "
                  "'<a xmlns=\"u\">' prints 'Failed to canonicalize'.",
  candidate_behavior="Candidate c14n accepts the relative URI and produces "
                     "'<a xmlns=\"u\"><b xmlns=\"u\" x=\"1\"/></a>' where the "
                     "oracle refuses. Divergence (both the absolute-URI check "
                     "and the inclusive ns propagation need auditing).",
  classification="STANDARD_CONFORMING",
  divergence_record=[
   {"kind": "candidate_vs_oracle",
    "fact": "xmllint --c14n '<a xmlns=\"u\"><b x=\"1\"/></a>' -> oracle 'Failed to canonicalize'; candidate canonicalizes and re-declares xmlns on <b>.",
    "impact": "canonicalization outputs differ; security-sensitive consumers "
              "comparing digests see divergence.",
    "residual": "to be closed in 11.1-X with a C14N-001 court"},
  ],
  courts=["C14N"],
 ),
 dict(id="exclusive-c14n", title="Exclusive XML Canonicalization 1.0", spec="W3C-EXC-C14N-1.0",
  spec_behavior="Exclusive canonicalization: only visibly utilized namespace "
                "declarations, inclusive-ns-prefix-list handling.",
  oracle_behavior="xmlC14NExecute with inclusive-ns flag (xmlC14NModeExclusive); "
                  "xmllint --exc-c14n.",
  candidate_behavior="Candidate --exc-c14n path exists; exclusive-mode namespace "
                     "selection is part of the C14N-001 closure work.",
  classification="STANDARD_CONFORMING",
  divergence_record=[],
  courts=["C14N"],
 ),
 dict(id="html", title="HTML (upstream-targeted behavior)", spec="HTML 4.01 / legacy HTML5-targeted",
  spec_behavior="Upstream does not claim strict HTML conformance; it targets "
                "tag-soup leniency for real-world documents (auto-closing, "
                "implied elements, encoding sniffing).",
  oracle_behavior="HTMLparser.c tag-soup engine: htmlAutoClose/htmlHandleOmittedElem, "
                  "HTML4 DTD attached on parse (endDocument), E-007 epoch "
                  "(2.15.0 HTML serialization single-line change).",
  candidate_behavior="html-parser 45 oracle exports, 44 candidate + 2 missing "
                     "(htmlCtxtSetOptions, htmlUTF8ToHtml — R-000165); 18 of 18 "
                     "html STUB obligations from the pre-11.1 census remain "
                     "unimplemented bodies (htmlDefaultSAXHandlerInit, "
                     "htmlElementAllowedHere, ...).",
  classification="UPSTREAM_EXTENSION",
  divergence_record=[
   {"kind": "candidate_vs_oracle",
    "fact": "18 html* obligations are STUB (empty bodies) per PARITY_OBLIGATIONS; "
            "census html-parser PARTIAL.",
    "impact": "HTML consumers get no-op behavior for the element-table APIs.",
    "residual": "R-000163-era html stubs tracked for 11.1-X"},
  ],
  courts=["HTML"],
 ),
 dict(id="xslt-1.0", title="XSLT 1.0", spec="W3C-XSLT-1.0",
  spec_behavior="Template model, number formatting (format-number with "
                "decimal-format), number->string of xsl:value-of, RTF handling, "
                "priority/import rules.",
  oracle_behavior="libxslt 1.1.45: format-number(1234567.891,'#,##0.00') = "
                  "'1,234,567.89'; value-of 1234567.891 = '1234567.891'. "
                  "E-008: core transform output stable across 15 years.",
  candidate_behavior="Candidate xsltproc: format-number with any pattern yields "
                     "EMPTY output (rc=0); value-of numbers use full double "
                     "precision ('1234567.891000000061467'). Both diverge from "
                     "the oracle and from XSLT 1.0 §12.3/§7.6.",
  classification="STANDARD_CONFORMING",
  divergence_record=[
   {"kind": "candidate_vs_oracle",
    "fact": "format-number(1234567.891,'#,##0.00') -> oracle '1,234,567.89', "
            "candidate ''; format-number(1234567.891,'#.##') -> oracle "
            "'1234567.89', candidate ''.",
    "impact": "XSLT number formatting is a core language feature; empty output "
              "breaks transforms.",
    "residual": "to be closed in 11.1-X with an XSLT-002 court (number formatting)"},
  ],
  courts=["XSLT-001"],
 ),
 dict(id="exslt", title="EXSLT modules", spec="EXSLT 1.0 (common/math/sets/strings/dynamic/dates/functions)",
  spec_behavior="Module function semantics per EXSLT 1.0 drafts.",
  oracle_behavior="libexslt 1.1.45 ships all modules with per-module "
                  "registration (exsltMathRegister etc.) plus exsltRegisterAll.",
  candidate_behavior="Only exsltRegisterAll is exported; the 16 per-module "
                     "registration entry points are missing (R-000165); module "
                     "function semantics largely untested.",
  classification="STANDARD_CONFORMING",
  divergence_record=[
   {"kind": "candidate_vs_oracle", "fact": "R-000165: exslt*Register family absent.",
    "impact": "consumers registering single modules cannot link.",
    "residual": "R-000165"},
  ],
  courts=["EXSLT"],
 ),
 dict(id="uri", title="URI handling", spec="RFC 3986",
  spec_behavior="URI parsing, resolution, escaping per RFC 3986.",
  oracle_behavior="uri.c predates RFC 3986 (RFC 2396-era); xmlSaveUri/xmlURIEscape "
                  "carry legacy escaping quirks; xmlParseURISafe used for the "
                  "xmlns relative-URI warning (scheme==NULL check).",
  candidate_behavior="uri 18/18 exports; the scheme check for the xmlns warning "
                     "now matches (TREE-001).",
  classification="UPSTREAM_QUIRK",
  divergence_record=[],
  courts=["URI"],
 ),
]

GEN_TIME = datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def main():
    out = {
        "schema": "standards-reconciliation-1",
        "generator": "tools/evidence/standards_reconciliation.py",
        "phase": "11.1-P",
        "generated": GEN_TIME,
        "policy": "For drop-in parity the upstream observable behavior takes "
                  "precedence unless a documented safety constraint requires a "
                  "safe divergence. Upstream-vs-spec differences are recorded, "
                  "not silently corrected.",
        "areas": AREAS,
    }
    os.makedirs(os.path.dirname(OUT_JSON), exist_ok=True)
    with open(OUT_JSON, "w") as f:
        json.dump(out, f, indent=1)
        f.write("\n")
    write_md(out)
    print(f"wrote {OUT_JSON}")
    print(f"wrote {OUT_MD}")
    print("areas:", len(AREAS))


def write_md(data):
    L = []
    L.append("# Standards Atlas — 11.1-P three-way reconciliation\n")
    L.append("For every standards-backed subsystem: **SPECIFICATION** (what the "
             "standard requires) / **UPSTREAM ORACLE** (what libxml2/libxslt "
             "does) / **LIBXML-RS** (what the candidate does).\n")
    L.append("Policy: for drop-in parity the upstream observable behavior takes "
             "precedence unless a documented safety constraint requires a safe "
             "divergence. Upstream-vs-spec differences are recorded, not "
             "silently corrected.\n")
    L.append("Classifications: `STANDARD_CONFORMING` · `UPSTREAM_EXTENSION` · "
             "`UPSTREAM_QUIRK` · `HISTORICAL_BUG` · `VERSION_SPECIFIC` · "
             "`PLATFORM_SPECIFIC` · `UNRESOLVED` · `SAFE_DIVERGENCE` · "
             "`CANDIDATE_DIVERGENCE`.\n")
    for a in data["areas"]:
        L.append(f"## {a['title']} — `{a['id']}`\n")
        L.append(f"Specification: **{a['spec']}**\n")
        L.append(f"- **SPECIFICATION:** {a['spec_behavior']}")
        L.append(f"- **UPSTREAM ORACLE:** {a['oracle_behavior']}")
        L.append(f"- **LIBXML-RS:** {a['candidate_behavior']}")
        L.append(f"- **Classification:** `{a['classification']}`")
        if a["divergence_record"]:
            L.append("- **Divergence records:**")
            for d in a["divergence_record"]:
                L.append(f"  - `{d['kind']}` — {d['fact']}")
                if d.get("impact"):
                    L.append(f"    - impact: {d['impact']}")
                if d.get("residual"):
                    L.append(f"    - residual: {d['residual']}")
        L.append(f"- **Courts:** {', '.join(a['courts']) or '—'}\n")
    with open(OUT_MD, "w") as f:
        f.write("\n".join(L))


if __name__ == "__main__":
    main()
