# Standards Atlas — 11.1-P three-way reconciliation

For every standards-backed subsystem: **SPECIFICATION** (what the standard requires) / **UPSTREAM ORACLE** (what libxml2/libxslt does) / **LIBXML-RS** (what the candidate does).

Policy: for drop-in parity the upstream observable behavior takes precedence unless a documented safety constraint requires a safe divergence. Upstream-vs-spec differences are recorded, not silently corrected.

Classifications: `STANDARD_CONFORMING` · `UPSTREAM_EXTENSION` · `UPSTREAM_QUIRK` · `HISTORICAL_BUG` · `VERSION_SPECIFIC` · `PLATFORM_SPECIFIC` · `UNRESOLVED` · `SAFE_DIVERGENCE` · `CANDIDATE_DIVERGENCE`.

## XML 1.0 (Fifth Edition) — `xml-1.0`

Specification: **W3C-XML-1.0**

- **SPECIFICATION:** Well-formedness constraints: character production, element type match, comments (no '--' inside), no '<' in attribute values, proper nesting, entity declaration rules.
- **UPSTREAM ORACLE:** Substantially conforming; the recovery mode (XML_PARSE_RECOVER) is a deliberate UPSTREAM_EXTENSION that continues parsing after well-formedness violations and returns a partial tree. In non-recovery mode violations are fatal with exact WFC messages ('Double hyphen within comment', 'Unescaped '<' not allowed in attributes values', 'PCDATA invalid Char value').
- **LIBXML-RS:** Non-recovery mode matches the oracle for the diagnostics in the ERROR-001 court (48-case corpus, byte-identical). Recovery mode matches for the TREE-001 recovery cases. However, several WFC checks the oracle raises are silently accepted by the candidate outside the court corpus: '<!-- a -- b -->', '<a b="x < y"/>', and attribute-value character checks.
- **Classification:** `STANDARD_CONFORMING`
- **Divergence records:**
  - `oracle_vs_spec` — xmlParseComment raises 'Double hyphen within comment' (XML_ERR_COMMENT_NOT_FINISHED) — matches XML 1.0 [15] Comments.
  - `candidate_vs_oracle` — xmllint '<a><!-- a -- b --></a>' -> oracle: 'parser error : Double hyphen within comment'; candidate: no diagnostic, tree built.
    - impact: drop-in consumers relying on the WFC diagnostic see different parse results (wellFormed flag, error stream, partial-tree semantics).
    - residual: to be closed in 11.1-X with a PARSER-002 court (WFC cluster)
- **Courts:** ERROR-001, TREE-001, PARSER

## Namespaces in XML 1.0 / 1.1 — `xml-namespaces`

Specification: **W3C-XML-NS-1.0 / W3C-XML-NS-1.1**

- **SPECIFICATION:** xmlns="uri" declares the default namespace; xmlns:p="uri" declares a prefix; Namespaces 1.0 forbids undeclaring prefixes (1.1 allows xmlns:p=""), forbids binding xml: to a wrong URI, and forbids the XML namespace URI as the default namespace.
- **UPSTREAM ORACLE:** xmlParseStartTag2 enforces: 'xmlns:%s: Empty XML namespace is not allowed' (XML_NS_ERR_XML_NAMESPACE), 'xml namespace prefix mapped to wrong URI', 'xml namespace URI cannot be the default namespace', 'reuse of the xmlns namespace name is forbidden', 'redefinition of the xmlns prefix is forbidden'. Namespace nodes have no parent pointer (XPath data model, QUIRK-0002).
- **LIBXML-RS:** The default-ns relative-URI warning and xmlns="" href semantics now match (TREE-001). However the oracle's namespace-declaration error checks are not yet raised: xmlns:p="", xmlns:xml="urn:x", and the XML namespace as default namespace are all accepted silently.
- **Classification:** `STANDARD_CONFORMING`
- **Divergence records:**
  - `candidate_vs_oracle` — xmllint '<a xmlns:p=""><p:b/></a>' -> oracle: 'namespace error : xmlns:p: Empty XML namespace is not allowed'; candidate: accepted.
    - impact: namespace error stream and xmlGetLastError differ for drop-in consumers.
    - residual: to be closed in 11.1-X with a NS-001 court
- **Courts:** TREE-001, XPATH-001

## XPath 1.0 — `xpath-1.0`

Specification: **W3C-XPATH-1.0**

- **SPECIFICATION:** Data model (document order node-sets), number->string conversion (NaN/'NaN', +/-Inf/'Infinity'/' -Infinity', shortest decimal), string->number, substring() rounding via round(), the '|' union operator in document order.
- **UPSTREAM ORACLE:** Observed 2.15.3: string(1 div 0)='Infinity', string(-1 div 0)='-Infinity', string(0 div 0)='NaN', string(-0)='0', substring('12345',1.5,2.6)='234'. Union operator yields document order in current releases; historical versions returned reversed order (VERSION_SPECIFIC, see SEMANTIC_EPOCHS).
- **LIBXML-RS:** The four number->string probes and substring rounding match the oracle byte-for-byte. XSLT-level number formatting diverges (see xslt-1.0: value-of 1234567.891).
- **Classification:** `STANDARD_CONFORMING`
- **Divergence records:**
  - `candidate_vs_oracle` — xsltproc '<xsl:value-of select="1234567.891"/>' -> oracle '1234567.891'; candidate '1234567.891000000061467' (Rust float Display instead of XPath shortest form).
    - impact: XSLT output text differs; also affects EXSLT string() results.
    - residual: to be closed in 11.1-X (XPath number formatting, XPATH-002 court)
- **Courts:** XPATH-001, XSLT-001

## XPointer — `xpointer`

Specification: **W3C-XPTR-1.0**

- **SPECIFICATION:** xpointer() and xmlns() scheme evaluation over XPath.
- **UPSTREAM ORACLE:** xpointer.c implements the xpointer scheme (and legacy '#' fragment handling in xmlXPtrEval); largely frozen upstream.
- **LIBXML-RS:** Exports xmlXPtrEval/xmlXPtrBuildNodeList/xmlXPtrNewLocationSet (census: xpointer 2/2 exports); XPointer evaluation semantics covered by the XPATH-001 family.
- **Classification:** `STANDARD_CONFORMING`
- **Courts:** XPATH-001

## XInclude 1.0 — `xinclude`

Specification: **W3C-XINCLUDE-1.0**

- **SPECIFICATION:** xi:include with parse='xml'|'text', xpointer selection, fallback processing, base-URI fixup.
- **UPSTREAM ORACLE:** xinclude.c implements inclusion with the upstream fixup semantics; xmlXIncludeSetResourceLoader hook present in 2.15.3.
- **LIBXML-RS:** xinclude exports 12/13 (xmlXIncludeSetResourceLoader missing, see R-000165); inclusion behavior court-covered by XINCLUDE-family tests.
- **Classification:** `STANDARD_CONFORMING`
- **Divergence records:**
  - `candidate_vs_oracle` — census R-000165: xmlXIncludeSetResourceLoader not exported.
    - impact: consumers installing a custom resource loader cannot link.
    - residual: R-000165
- **Courts:** XINCLUDE

## XML Schema 1.0 — `xml-schema`

Specification: **W3C-XSD-1.0**

- **SPECIFICATION:** Full XSD 1.0 structures + datatypes validation.
- **UPSTREAM ORACLE:** libxml2 ships a partial validator (xmlschemas.c) covering common constructs; full XSD conformance is not claimed upstream (documented partial).
- **LIBXML-RS:** xsd subsystem 28 oracle DSO exports, 27 candidate (xmlSchemaSetResourceLoader missing, R-000165); validation engine is a subset.
- **Classification:** `UPSTREAM_EXTENSION`
- **Courts:** XSD

## RELAX NG — `relaxng`

Specification: **OASIS RELAX NG / ISO 19757-2**

- **SPECIFICATION:** Pattern-based validation with datatype library integration.
- **UPSTREAM ORACLE:** relaxng.c implements the full pattern language; datatype integration via xmlRelaxNGSetDatatypeSpecificFuncs.
- **LIBXML-RS:** relaxng 27 oracle exports, 24 candidate (missing xmlRelaxNGSetResourceLoader, xmlRelaxNGValidCtxtClearErrors, xmlRelaxParserSetIncLImit — R-000165).
- **Classification:** `STANDARD_CONFORMING`
- **Divergence records:**
  - `candidate_vs_oracle` — R-000165 missing exports
    - impact: link-time gaps for resource-loader / error-clearing consumers.
    - residual: R-000165
- **Courts:** RELAXNG

## Schematron — `schematron`

Specification: **ISO/IEC 19757-3**

- **SPECIFICATION:** Rule/assert/report pattern language; XSLT query binding.
- **UPSTREAM ORACLE:** schematron.c evaluates via XSLT (query binding 'xslt').
- **LIBXML-RS:** schematron 10/10 exports; SchematronSchema engine is native-Rust; the schematron NULL-context UB was fixed in 11.1-O (ASan clean).
- **Classification:** `STANDARD_CONFORMING`
- **Courts:** SCHEMATRON

## Canonical XML 1.0 — `canonical-xml`

Specification: **W3C-C14N-1.0**

- **SPECIFICATION:** Inclusive canonicalization: absolute namespace URIs required, attributes in lexicographic order, namespace declarations propagated to every element.
- **UPSTREAM ORACLE:** xmlC14NExecute/xmlC14NDocDumpMemory implement C14N 1.0; the relative-URI rule is enforced: xmllint --c14n on '<a xmlns="u">' prints 'Failed to canonicalize'.
- **LIBXML-RS:** Candidate c14n accepts the relative URI and produces '<a xmlns="u"><b xmlns="u" x="1"/></a>' where the oracle refuses. Divergence (both the absolute-URI check and the inclusive ns propagation need auditing).
- **Classification:** `STANDARD_CONFORMING`
- **Divergence records:**
  - `candidate_vs_oracle` — xmllint --c14n '<a xmlns="u"><b x="1"/></a>' -> oracle 'Failed to canonicalize'; candidate canonicalizes and re-declares xmlns on <b>.
    - impact: canonicalization outputs differ; security-sensitive consumers comparing digests see divergence.
    - residual: to be closed in 11.1-X with a C14N-001 court
- **Courts:** C14N

## Exclusive XML Canonicalization 1.0 — `exclusive-c14n`

Specification: **W3C-EXC-C14N-1.0**

- **SPECIFICATION:** Exclusive canonicalization: only visibly utilized namespace declarations, inclusive-ns-prefix-list handling.
- **UPSTREAM ORACLE:** xmlC14NExecute with inclusive-ns flag (xmlC14NModeExclusive); xmllint --exc-c14n.
- **LIBXML-RS:** Candidate --exc-c14n path exists; exclusive-mode namespace selection is part of the C14N-001 closure work.
- **Classification:** `STANDARD_CONFORMING`
- **Courts:** C14N

## HTML (upstream-targeted behavior) — `html`

Specification: **HTML 4.01 / legacy HTML5-targeted**

- **SPECIFICATION:** Upstream does not claim strict HTML conformance; it targets tag-soup leniency for real-world documents (auto-closing, implied elements, encoding sniffing).
- **UPSTREAM ORACLE:** HTMLparser.c tag-soup engine: htmlAutoClose/htmlHandleOmittedElem, HTML4 DTD attached on parse (endDocument), E-007 epoch (2.15.0 HTML serialization single-line change).
- **LIBXML-RS:** html-parser 45 oracle exports, 44 candidate + 2 missing (htmlCtxtSetOptions, htmlUTF8ToHtml — R-000165); 18 of 18 html STUB obligations from the pre-11.1 census remain unimplemented bodies (htmlDefaultSAXHandlerInit, htmlElementAllowedHere, ...).
- **Classification:** `UPSTREAM_EXTENSION`
- **Divergence records:**
  - `candidate_vs_oracle` — 18 html* obligations are STUB (empty bodies) per PARITY_OBLIGATIONS; census html-parser PARTIAL.
    - impact: HTML consumers get no-op behavior for the element-table APIs.
    - residual: R-000163-era html stubs tracked for 11.1-X
- **Courts:** HTML

## XSLT 1.0 — `xslt-1.0`

Specification: **W3C-XSLT-1.0**

- **SPECIFICATION:** Template model, number formatting (format-number with decimal-format), number->string of xsl:value-of, RTF handling, priority/import rules.
- **UPSTREAM ORACLE:** libxslt 1.1.45: format-number(1234567.891,'#,##0.00') = '1,234,567.89'; value-of 1234567.891 = '1234567.891'. E-008: core transform output stable across 15 years.
- **LIBXML-RS:** Candidate xsltproc: format-number with any pattern yields EMPTY output (rc=0); value-of numbers use full double precision ('1234567.891000000061467'). Both diverge from the oracle and from XSLT 1.0 §12.3/§7.6.
- **Classification:** `STANDARD_CONFORMING`
- **Divergence records:**
  - `candidate_vs_oracle` — format-number(1234567.891,'#,##0.00') -> oracle '1,234,567.89', candidate ''; format-number(1234567.891,'#.##') -> oracle '1234567.89', candidate ''.
    - impact: XSLT number formatting is a core language feature; empty output breaks transforms.
    - residual: to be closed in 11.1-X with an XSLT-002 court (number formatting)
- **Courts:** XSLT-001

## EXSLT modules — `exslt`

Specification: **EXSLT 1.0 (common/math/sets/strings/dynamic/dates/functions)**

- **SPECIFICATION:** Module function semantics per EXSLT 1.0 drafts.
- **UPSTREAM ORACLE:** libexslt 1.1.45 ships all modules with per-module registration (exsltMathRegister etc.) plus exsltRegisterAll.
- **LIBXML-RS:** Only exsltRegisterAll is exported; the 16 per-module registration entry points are missing (R-000165); module function semantics largely untested.
- **Classification:** `STANDARD_CONFORMING`
- **Divergence records:**
  - `candidate_vs_oracle` — R-000165: exslt*Register family absent.
    - impact: consumers registering single modules cannot link.
    - residual: R-000165
- **Courts:** EXSLT

## URI handling — `uri`

Specification: **RFC 3986**

- **SPECIFICATION:** URI parsing, resolution, escaping per RFC 3986.
- **UPSTREAM ORACLE:** uri.c predates RFC 3986 (RFC 2396-era); xmlSaveUri/xmlURIEscape carry legacy escaping quirks; xmlParseURISafe used for the xmlns relative-URI warning (scheme==NULL check).
- **LIBXML-RS:** uri 18/18 exports; the scheme check for the xmlns warning now matches (TREE-001).
- **Classification:** `UPSTREAM_QUIRK`
- **Courts:** URI
