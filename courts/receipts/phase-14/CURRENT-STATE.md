# Phase 14.3 — tracked state (auto-updated)

Authoritative clean-seal re-baseline (before any fix): commit 3f42436b
Receipt dir: courts/receipts/phase-14/php-14-3-baseline-20260902/

- oracle php-8.5.10 : 1291 tests / 0 failed / 40 skipped  (PASS)
- candidate (HEAD DSO set): 1291 / 321 failed / 40 skipped (FAIL baseline)
- crash-class failures in the 321: 31 (content-classified)

Fix 1 committed (07d56779, R-14.3-DOM-FRAGMENT-SERIALIZE FIXED):
- ext/dom/tests/DOMParentNode_empty_argument.phpt green on candidate; full
  suite 321 -> 320; crash-class -> 30; zero regressions.
- receipt dir: courts/receipts/phase-14/php-14-3-fragfix-20260902/

Current tracked (post-Fix-1 full candidate run, iteration tree, same config):
- candidate: 1291 / 320 failed / 40 skipped (skips == oracle)

Open residuals remaining to closure (in phase order, one root cause each):
- Bug-2 SimpleXML attribute set/unset double free (repro recorded; raw libxml
  primitives clean, defect sits in SimpleXML object/attribute path).
- ~29 more crash-class members (double free / SIGSEGV) across dom, simplexml,
  xml, xsl.
- ~289 output-mismatch failures grouped by root cause.
- then 14.3-Q binary substitution, 14.3-S ZTS, and 14.3-T full gate.
