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

Open residuals remaining to closure (in phase order, one root cause each):
- ~20 remaining crash-class members (double free / SIGSEGV) across dom,
  simplexml, xml, xsl.
- ~290 output-mismatch failures grouped by root cause.
- then 14.3-Q binary substitution, 14.3-S ZTS, and 14.3-T full gate.
