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
  - known residual within this family (recorded, next): NOENT-substituted CONTENT
    is coalesced into one characters() event (CD[ENTy]) vs the oracle's separate
    runs (CD[ENT] CD[y]); attribute-value whitespace normalization; per-extension
    trace via consumers/*probe.php. Not yet pushed to origin/main.

Open residuals remaining to closure (in phase order, one root cause each):
- ~19 remaining crash-class members across dom, simplexml, xml, xsl.
- ~290 output-mismatch failures grouped by root cause.
- then 14.3-Q binary substitution, 14.3-S ZTS, and 14.3-T full gate.
