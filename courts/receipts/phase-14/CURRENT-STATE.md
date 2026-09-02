# Phase 14.3 — tracked state (auto-updated)

Authoritative clean-seal re-baseline (before any fix): commit 3f42436b
Receipt dir: courts/receipts/phase-14/php-14-3-baseline-20260902/

- oracle php-8.5.10 : 1291 tests / 0 failed / 40 skipped  (PASS)
- candidate (HEAD DSO set): 1291 / 321 failed / 40 skipped (FAIL baseline)
- crash-class failures in the 321: 31 (content-classified)

Fix 1 committed (07d56779, R-14.3-DOM-FRAGMENT-SERIALIZE FIXED): 321 -> 320.
Fix 2 committed (74b62ca2, R-14.3-XMLFREEPROP-DICT-NAME FIXED): 320 -> 315.
Fix 3 committed (R-14.3-COPYDOC-ROOT-PARENT FIXED): 315 -> 310.
  - xmlCopyDoc must keep the new doc node as parent of copied top-level
    children (upstream xmlStaticCopyNodeList(doc->children,ret,(xmlNodePtr)ret));
    NULL parent made PHP treat cloned root as ownerless -> early subtree free
    -> doc teardown double free.
  - closed DOMElement_append_hierarchy_test, DOMElement_insertAdjacentText,
    DOMElement_prepend_hierarchy_test, xsl importStylesheet_clone_retained_
    {document,node}; zero regressions.
  - receipt dir: courts/receipts/phase-14/php-14-3-copydoc-parent-20260902/

Current tracked (post-Fix-3 full candidate run, iteration tree, same config):
- candidate: 1291 / 310 failed / 40 skipped (skips == oracle)

Fix 4 committed (R-14.3-EMPTY-STRING-DANGLING-PTR FIXED): 310 -> 308.
  - b"" as *const u8 -> xml_strdup idiom handed a dangling 0x1 pointer;
    replaced with c"".as_ptr() (real NUL static) at 5 sites.
  - closed ext/simplexml 027 + 028; zero regressions.
  - receipt dir: courts/receipts/phase-14/php-14-3-empty-string-ptr-20260902/

Fix 5 committed (0972f2c4, R-14.3-SAX-NOENT-ENTITY-REGISTRY FIXED): engine-level.
  - root cause: push/expat-compat SAX parse (xml_parser/ext_xml) has NO ctxt->myDoc,
    so parse_internal_subset dropped the internal subset and never registered
    `<!ENTITY e "ENT">`; NOENT content+attr references failed ("Entity 'e' not
    defined", rc=-1). Fix keeps a lazy SAX_COMPAT_MODE + XML_DOC_INTERNAL
    registry doc/fake intSubset on ctxt->myDoc when an internal general entity is
    declared (mirrors upstream parser.c xmlParseEntityDecl) and registers entities;
    free_parser_ctxt reclaims XML_DOC_INTERNAL docs (no leak); SAX2 attr
    value_end always published so external attrs decode.
  - C regress entprobe.c candidate==oracle (attr + content entity substitution
    byte-identical; parse rc 0; xmlGetDocEntity resolves e). attrrefs-probe.php
    byte-identical. cargo test --lib 1199 pass; clippy/fmt clean.
  - measured ext/xml + ext/simplexml combined: 1291-subset 225 run -> 183 passed /
    30 failed / 12 skipped.
  - receipt/analysis: courts/receipts/phase-14/php-14-3-sax-entity-registry-rootcause.md
  - known residual within this raw-entity family (recorded; the content
    characters()-event boundary aspect closed by Fix 5b, see below):
    attribute-value whitespace normalization; markup-bearing entity content
    reported by PHP's compat layer as a single default event (php-consumer
    nuance; pure-libxml SAX parity holds).

Fix 5b committed (5a8d0af4, R-14.3-SAX-CONTENT-ENTITY-BOUNDARY FIXED): characters()
  event segmentation across a substituted general entity. tokenizer.rs
  scan_characters breaks a character run when the input-stack depth falls below
  the depth it started at (entity-content input boundary). Pure-libxml push-SAAS
  probe now byte-identical to the oracle for plain / adjacent / leading /
  trailing / markup-content entities (CD[ab] CD[ENT] CD[cd] not CD[ab] CD[ENTcd]);
  invisible to tree building (SAX2 coalesces). cargo test --lib 1199 pass;
  clippy/fmt clean; ext/xml + ext/simplexml 30 unchanged (no regression).
  Probes: cdseg-probe.php (php) + entprobe.c multi-doc push-SAAS.

Open residuals remaining to closure (in phase order, one root cause each):
- ~19 remaining crash-class members across dom, simplexml, xml, xsl.
- ~290 output-mismatch failures grouped by root cause.
- then 14.3-Q binary substitution, 14.3-S ZTS, and 14.3-T full gate.

Fix 6 committed (e12b2ed0, R-14.3-ENTITY-NDATA-METADATA FIXED): parse external
  general entities declared SYSTEM/PUBLIC with an NDATA notation as
  XML_EXTERNAL_GENERAL_UNPARSED_ENTITY, carrying the notation name on the entity
  content (nameless NDATA keeps an empty string). DOMEntity publicId/systemId/
  notationName for NDATA/PUBLIC-SYSTEM entities now byte-identical to the oracle
  (entitymeta-probe.php); DOMEntity_fields.phpt narrowed from whole-output failure
  to a single residual microcase: PUBLIC with an empty public id () reports
  NULL instead of string(0)"". cargo test --lib 1199 pass; clippy/fmt clean;
  ext/dom remeasure no new failures. NEXT residual in this atom: the
  empty-public-id ExternalID="" storage ("" collapses to NULL).

SP-14.3.1-1 closed (HEAD after this: R-14.3-SAX-NOTATION-UNPARSED-DECL FIXED):
  internal-subset NOTATION -> SAX notationDecl and NDATA-unparsed ENTITY ->
  unparsedEntityDecl now dispatch (fire_sax_notation_decl /
  fire_sax_unparsed_entity_decl) when a handler is set. ext/xml
  xml_set_notation_decl_handler_basic flips PASS; ext/xml suite 21 -> 20 failed.
