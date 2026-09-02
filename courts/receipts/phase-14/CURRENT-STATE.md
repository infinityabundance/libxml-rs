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

SP-14.3.1-1 closed (96381efc, R-14.3-SAX-NOTATION-UNPARSED-DECL FIXED):
  internal-subset NOTATION -> SAX notationDecl and NDATA-unparsed ENTITY ->
  unparsedEntityDecl now dispatch (fire_sax_notation_decl /
  fire_sax_unparsed_entity_decl) when a handler is set. ext/xml
  xml_set_notation_decl_handler_basic flips PASS; ext/xml suite 21 -> 20 failed.

SP-14.3.1-2 closed (R-14.3-UNDECLARED-ENTITY-SEVERITY FIXED):
  xml004 + xml_closures_001 PASS; ext/xml 20 -> 18 failed, zero regressions.
  - root cause: parse_reference raised EVERY undeclared general entity as
    XML_ERR_UNDECLARED_ENTITY (26) FATAL, dropping the element tail after
    `&included-entity;` (xmltest.xml's `%incent;`/SYSTEM ext subset were never
    tracked). Upstream xmlHandleUndeclaredEntity makes the reference FATAL
    only when standalone==1 or (hasExternalSubset==0 && hasPErefs==0);
    otherwise XML_WAR_UNDECLARED_ENTITY (27) ERROR/WARNING and the parse
    continues.
  - fixes: parse_dtd sets ctxt->hasExternalSubset when the DOCTYPE carries a
    SYSTEM/PUBLIC id; parse_internal_subset sets ctxt->hasPErefs on a
    well-formed %Name; reference (upstream xmlParsePERefInternal); the
    undeclared branch mirrors xmlHandleUndeclaredEntity (fatal | DTDVALID
    validity error | xmlErrMsgStr 27-ERROR when loadsubset or
    replaceEntities && !NO_XXE | xmlWarningMsg 27-WARNING) + valid=0 + SAX
    reference event only when replaceEntities==0; xmlCtxtUseOptions now
    mirrors xmlCtxtSetOptionsInternal member updates (was an options-only
    stub; replaceEntities stayed 0 under PHP's NOENT compat path).
  - guards: tests.rs undeclared-entity severity trio (fatal plain/intsub 26;
    non-fatal extsub/PE-ref; NOENT -> errNo 27) + consumers/
    undecl-entity-probe.c; oracle-pinned on the system 2.15.3 and php-oracle
    containers. cargo test --lib 1202 pass; clippy/fmt clean.
  - receipt/analysis: php-14-3-undeclared-entity-20260902/ (log sp1-xml3.log
    ext/xml 67 run -> 38 pass / 18 fail / 11 skip).

SP-14.3.1-3 closed (R-14.3-PUSH-DELIVERY + R-14.3-SAX1SAX2 + R-14.3-NS-SCOPE
  FIXED): xml009/xml010/bug25666/bug50576/bug72714 PASS; ext/xml 18 -> 10
  failed (also closed bug73135 + xml_set_object_multiple_times{,_errors});
  zero regressions. Three engine root causes:
  - R-14.3-PUSH-DELIVERY: php xml_parse defaults isFinal=false and upstream
    xmlParseChunk parses each chunk eagerly, so a single non-final call with
    a complete document must deliver. helpers::parse_chunk now probes each
    non-final call with a silent parse (SAX + diagnostics deferred, EOF in an
    open construct pauses) and, when the accumulated input parsed through to
    its end (clean end OR a failure on a complete token at the input end —
    bug25666's `</foo>`-closes-`<foo:a>`), runs one completing parse that
    delivers exactly once; later calls only buffer (epilog) and the final
    call re-validates silently with a sax-suppressed diagnostics pass for
    trailing junk; push state is freed with the context (address reuse).
  - R-14.3-SAX1SAX2: element dispatch now follows upstream xmlCtxtInitializeLate
    (SAX1 when !(SAX1 flag) && initialized != XML_SAX2_MAGIC ...): the non-ns
    expat-compat parser (initialized=1) gets the SAX1 startElement with the
    RAW QName + full attribute list (xmlns declarations included) instead of
    the SAX2 NS handler; dispatch goes through the context's own sax struct
    (userData is the consumer's opaque object, not a parser context).
  - R-14.3-NS-SCOPE: parser-scoped namespace scope stack (upstream nsTab)
    for SAX2 pure-SAX parses (ctxt->node == NULL): ancestor xmlns decls now
    resolve for elements/attrs and the prefix-declared check, so
    <foo:a xmlns:bar=...><bar:b/> resolves and raises no bogus
    undefined-prefix error (bug25666/bug81351/xml009).
  - guards: tests.rs push delivery trio + SAX1 qname/xmlns + SAX2 ancestor
    URI tests; C probes consumers/nspush-probe.c (candidate) +
    nsoracle-probe.c (oracle) pin SAX1-vs-SAX2 dispatch; oracle-pinned on the
    system 2.15.3 + php-oracle containers (bug81351 kept green).
    cargo test --lib 1206 pass; phase-13 HOSTILE-ABI/ALLOCATOR/CALLBACKS/
    FAILURE/OWNERSHIP probes PASS (parse_chunk change verified); clippy (no
    new warnings) + fmt clean.
  - receipt/analysis: php-14-3-push-delivery-20260902/ (log sp1-xml7.log
    ext/xml 67 run -> 46 pass / 10 fail / 11 skip).

SP-14.3.1-4 closed (R-14.3-BOM-PUSH + R-14.3-INSTATE-ABI + R-14.3-EXT-ENTITY-REF
  + R-14.3-EXTERN-ENTITY-REG-GATE + R-14.3-CHUNK-RC FIXED): bug35447 +
  bug71592 PASS; ext/xml 10 -> 8 failed (zero regressions; also closed
  bug30875 + gh14834, which the instate fix exposed as double delivery).
  Five engine/ABI root causes:
  - R-14.3-BOM-PUSH: InputBuffer::push_bytes re-runs BOM/encoding detection when
    the first real bytes arrive on an initially-empty push buffer (the buffer
    was constructed empty, so detection ran on no bytes). Real-BOM push parses
    now byte-identical to the oracle (bom-probe.c 4-case differential).
  - R-14.3-INSTATE-ABI: ctxt->instate is an ABI-visible field; PHP
    expat-compat compat.c compares it against XML_PARSER_CONTENT while
    resolving entity references. The candidate's private constants AND its
    installed header AND abi::types::xmlParserInputState each carried a
    DIFFERENT wrong numbering (engine CONTENT=3, header 4, real 7). All three
    now derive from/mirror the real 2.15 header verbatim
    (abi::types::xmlParserInputState + include/libxml/parser.h +
    state.rs aliases + exports_parserint EOF=-1). bug71592's external-entity
    handler only fires when the engine writes instate==7 at the reference.
  - R-14.3-EXT-ENTITY-REF: external general PARSED entities declared in the
    internal subset must register into the SAX-compat registry ONLY when
    ctxt->replaceEntities != 0 (upstream xmlParseEntityDecl gate) — parameter
    and NDATA-unparsed declarations never enter the registry
    (compat_entity_must_register + parse_internal_subset gating). After a
    getEntity resolves an entity, parse_reference honors upstream's
    `if (!ctxt->wellFormed) return;` (stop mid-lookup: no substitution, no
    events) and the context's STARTING wellFormed is preserved across
    probe/delivery re-parses (PushState.start_well_formed +
    started_unwellformed) so expat-compat (wf=0-at-create) parses never
    double-substitute: compat's get_entity side effects are the only content
    delivery (bug30875/gh14834 "aentent").
  - R-14.3-CHUNK-RC: helpers::parse_chunk now mirrors upstream xmlParseChunk —
    returns ctxt->errNo when wellFormed==0 after the delivery/terminating
    parse (was always 0 on non-final calls), and a stopped context
    (disableSAX==2) refuses further chunks with errNo. xml_parse() returns
    FALSE for bug71592 exactly like the oracle (chunkrc-probe.c).
  - The candidate-side php had to be rebuilt against the corrected installed
    headers (its compat.c had compiled XML_PARSER_CONTENT==4 from the old
    header): php-court-stage.sh candidate contract re-run in phpbuild-c.
  - guards: tests.rs push BOM+ATTLIST-default (sax2 attr capture), external-
    entity-ref stop (rc/errNo 21, no events past the stop), wf0-compat
    single-entity-delivery vs wf1 substitution control; ABI-layout guard
    pinning _xmlParserCtxt/_xmlEntity/_xmlSAXHandler offsets to the oracle
    offsetof probe; test_push_error_at_end_still_delivers updated to the
    oracle-verified rc (76) incl. the terminating call.
    cargo test --lib 1212 pass; clippy (no new warnings) + fmt clean; ext/xml
    67 run -> 48 pass / 8 fail / 11 skip; dom/simplexml/xmlreader/xmlwriter/xsl
    remeasure: no new failures vs the -3 state (4 dom fragment/adoptNode
    failures pre-existing at -3, verified by stash-rebuild).
  - probes kept: consumers/{ext71592-probe.c,ext71592-probe.php,
    ctxoffset-probe.c,bom-probe.c,chunkrc-probe.c,intent-probe2.c} +
    extget-probe.c (residual).

SP-14.3.1-5 closed (R-14.3-END-LOCATOR FIXED): bug26614_libxml_gte2_11 PASS;
  ext/xml 8 -> 7 failed (zero regressions).
  - root cause: sax_end_element fired the endElement/endElementNs callback
    without syncing the C-visible input position, so
    xml_get_current_line_number/column/byte_index at the end callback
    reported the stale position of the PREVIOUS event (the last text run) —
    `</DATA> at line 9, col 1 (byte 89)` instead of upstream's one-past-the-'>'
    position (byte 96). The end tag is fully consumed by the tokenizer before
    the event fires; sax_end_element now sync_input_position()s first, exactly
    like upstream xmlParseEndTag1/2 which fire the callback after the tag.
  - guards: tests.rs test_push_end_element_locator_is_one_past_gt (SAX1 push,
    asserts line 9 / byte 96 for bug26614 case 1; oracle-pinned).
    cargo test --lib 1213 pass; fmt/clippy clean; five-extension remeasure
    unchanged (dom 169 / simplexml 9 / xmlreader 29 / xmlwriter 19 / xsl 58).
  - receipt dir: php-14-3-end-locator-20260902/

SP-14.3.1-6 closed (R-14.3-PARSE-HUGE-LIMITS + R-14.3-MULTICALL-EAGER-DELIVERY
  FIXED): XML_OPTION_PARSE_HUGE PASS; ext/xml 7 -> 6 failed (zero regressions;
  bug81351 kept green). Two engine root causes:
  - R-14.3-PARSE-HUGE-LIMITS: without XML_PARSE_HUGE an element name longer
    than XML_MAX_NAME_LENGTH (50 000 bytes; 10 000 000 = XML_MAX_TEXT_LENGTH
    with HUGE) failed the tag parse upstream with XML_ERR_NAME_REQUIRED (68)
    "StartTag: invalid element name"; the candidate accepted any name length.
    The tokenizer now enforces the limit (max_name_length set from the ctxt
    options; upstream xmlParseName fast-path parity).
  - R-14.3-MULTICALL-EAGER-DELIVERY: the -3 push model deferred ALL delivery
    until the accumulated input formed a complete document, so a non-final
    xml_parse on an INCOMPLETE doc fired nothing — but upstream parses each
    xmlParseChunk eagerly and delivers every completed construct immediately
    (the HUGE success case prints CONTAINER/A/A/SECOND during the first call
    and only closes the container on the final call). parse_chunk now runs an
    eager-partial delivery whenever the silent probe pauses at a clean
    construct boundary (SAX + diagnostics on, EOF-in-open-construct pauses),
    records delivered_bytes, and every later parse (non-final or terminating)
    resumes from that boundary: SAX events at or below it are suppressed, the
    tokenizer splits character runs there so segmentation matches, and the
    terminating parse still raises premature-end errors (bug81351). startDocument
    fires once per session. All events fire exactly once across the calls.
  - guards: tests.rs test_push_multicall_eager_delivery_then_resume +
    test_push_name_length_limit_without_huge (+huge control).
    cargo test --lib 1215 pass; clippy no new warnings; fmt clean; phase-13
    HOSTILE-ABI/ALLOCATOR/CALLBACKS/FAILURE/OWNERSHIP verdicts PASS; ext/xml
    67 run -> 50 pass / 6 fail / 11 skip; five-extension remeasure unchanged
    (dom 169 / simplexml 9 / xmlreader 29 / xmlwriter 19 / xsl 58).
  - receipt dir: php-14-3-parse-huge-20260902/
