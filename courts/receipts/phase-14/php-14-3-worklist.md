# Phase 14.3 — atomized execution worklist (status tracked here)

Authoritative split (full-suite run at /out/total-check.log, DSO @ 8f42257f):
Total 305 failed.  dom 170 | xsl 58 | xmlreader 29 | xmlwriter 19 | simplexml 9 | xml 20
Oracle skips 40 (all extensions agree). Oracle baseline all-extension = 0 fails.

Legend: [ ] not started | [~] in progress | [x] closed+committed (ext subset PASSed)
Method per atom: mirror upstream at the Rust engine layer; commit; run the owning
extension subset to confirm the targeted test(s) flip PASS with no sibling regress.

## SP-14.3.1  ext/xml  (20)
- [x] SP-14.3.1-1  SAX notationDecl + NDATA unparsedEntityDecl dispatch
       (xml_set_notation_decl_handler_basic)  -> commit 96381efc (ext/xml 21->20)
- [x] SP-14.3.1-2  xml004/xml_closures_001: elem4 + tail dropped after external/
       parameter-entity include in SAX push (xml004-probe.php). Root:
       undeclared-entity severity now mirrors xmlHandleUndeclaredEntity
       (fatal only standalone/no-ext-subset/no-PE-refs; else 27 warn/err and
       continue) + hasExternalSubset/hasPErefs tracking + xmlCtxtUseOptions
       member updates. ext/xml 20 -> 18 failed.
- [x] SP-14.3.1-3  xml_parse_into_struct namespace tag/attr encoding:
       bug50576/bug25666/xml009/xml010/bug72714 all PASS. Roots closed:
       (a) xmlParseChunk eager delivery — non-final calls probe silently and
       one completing parse delivers exactly once (php xml_parse defaults
       isFinal=false); (b) SAX1-vs-SAX2 dispatch by sax->initialized
       (non-ns expat-compat gets raw-QName SAX1 events with xmlns attrs);
       (c) parser-scoped ns scope stack for pure-SAX ancestor resolution.
       Also closed bug73135 + xml_set_object_multiple_times{,_errors};
       bug81351 kept green. ext/xml 18 -> 10 failed.
- [x] SP-14.3.1-4  xml_parse_into_struct empty/False result: bug35447 (char entity
       inside attr recode), bug71592. ext/xml 10 -> 8 failed (also closed
       bug30875 + gh14834, which the ABI fix exposed as double-delivery).
- [x] SP-14.3.1-5  bug26614_libxml_gte2_11 end-element locator col/byte.
       ext/xml 8 -> 7 failed.
- [ ] SP-14.3.1-6  XML_OPTION_PARSE_HUGE depth/name limit semantics
       (`xml_parse` + `xml_parse_into_struct` must fail without HUGE).
- [ ] SP-14.3.1-7  gh12254 recursion-on-callback guard (Parser must not be called
       recursively).
- [ ] SP-14.3.1-8  crash cluster gh20439_1/gh20439_2/bug27908 (php compat SAX2
       default-emit deref).
- [ ] SP-14.3.1-9  xml_error_string_basic_libxml (error code/string table rows 47,
       64 for PI-not-finished and reserved-name).
- [ ] SP-14.3.1-10 xml_set_object_multiple_times{,_errors}, xml46699 + residuals.

## SP-14.3.2  ext/simplexml (9)

## SP-14.3.3  ext/dom engine (subset of the 170; rest to SP-14.3.4)

## SP-14.3.4  ext/dom validation

## SP-14.3.5  ext/xmlreader (29)

## SP-14.3.6  ext/xmlwriter (19)

## SP-14.3.7  ext/xsl (58)

## SP-14.3.8  closure sweep -> 14.3-Q binary-sub / S ZTS / T full gate

Keep this file updated the moment an atom closes; re-run the owning extension
subset after each; re-run the full suite at each subphase gate.
