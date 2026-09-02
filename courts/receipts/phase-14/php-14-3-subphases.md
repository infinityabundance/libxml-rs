# Phase 14.3 — subphase closure plan (to-zero)

Authoritative re-baseline (2026-09-02, candidate DSO set @ ff536c32):
**1291 tests / 306 failed / 40 skipped** (skips == oracle). Failing universe
extracted from `/out/subphase-baseline.log` FAILED TEST SUMMARY.

Method for every root cause: mirror upstream (oracle `oracle/historical/
src/libxml2-2.15.0`) at the Rust engine layer; add a regression guard / probe;
re-run the affected PHP extension subset to confirm the targeted tests flip to
PASS with zero sibling regressions; run the FULL six-extension suite at each
subphase gate to confirm net failure count drops and no regression; commit +
push each closure; keep `CURRENT-STATE.md` current.

## Subphases (ordered; gate between each = full-suite re-measure)

- **SP-14.3.1 ext/xml** (21 fails). SAX2/expat-compat layer: attribute encode/
  decode + entity options, notation-decl handler, error-string table, object
  handler multiple-sets, huge option, parser closures. Root-causes visible in
  `ext/xml/tests/*.diff`.
- **SP-14.3.2 ext/simplexml** (9 fails). Engine-backed: `LIBXML_RECOVER` /
  `LIBXML_NO_XXE`, doc/encoding parity, serialization.
- **SP-14.3.3 ext/dom XML engine** (the large dom bucket): serialize /
  save / DOCTYPE entity (mostly done in Fix-5), namespace + prefix
  reconciliation + default-ns, import/append/replace/textContent + innerHTML,
  DOMNode isEqualNode, C14N-references, XMLDocument-from-* / encoding,
  option semantics (`NO_XML_DECL`, `preserveWhiteSpace`, `PARSE_HUGE`).
- **SP-14.3.4 ext/dom validation**: schemaValidate* add-attrs/error message,
  relaxNG error parity, validate external DTD / on-parse.
- **SP-14.3.5 ext/xmlreader** (30): 001-015 stream/read APIs, custom
  constructors, expand, readString, gh16292/gh19098 + php_libxml_node_free.
- **SP-14.3.6 ext/xmlwriter** (19): OO syntax/attribute-ns, indentation, flush,
  shift_jis output, error parity.
- **SP-14.3.7 ext/xsl** (~60): php:function/registerPHPFunctions routing,
  setParameter/getParameter/removeParameter validation, transformTo* APIs +
  doc/result lifetimes, XSLT engine edges (bug26384/bug49634 crash-class),
  namespace_mapper + callables + auto-registration.
- **SP-14.3.8 closure sweep** (any residual crash-class + mix) -> then
  **14.3-Q** binary substitution, **14.3-S** ZTS, **14.3-T** full gate.

## Current atom
- Resolved down from baseline 308 -> 306 via Fix-5/5b layers + Fix-6 (NDATA
  entity metadata). Current in-family residual targeted in SP-14.3.3 (dom):
  DOMEntity_fields empty public id (PUBLIC "") ExternalID="" collapses to NULL.
