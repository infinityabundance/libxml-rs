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

SP-14.3.1-7 closed (R-14.3-COMPLETED-CTXT-REFUSES-REPARSE FIXED): gh12254 PASS;
  ext/xml 6 -> 5 failed (zero regressions).
  - root cause: a parser context that finished a complete document stayed
    usable on the candidate: parse_chunk reset instate=START at every call and
    re-parsed, so gh12254's SECOND xml_parse_into_struct on the same parser
    fired the element events again. Upstream xmlParseTryOrFinish keeps the
    completed context at XML_PARSER_EOF and every later xmlParseChunk parses
    nothing (`case XML_PARSER_EOF: goto done`). parse_chunk now mirrors that:
    instate==XML_PARSER_EOF at entry returns the recorded outcome without
    parsing. Incomplete parses never set instate=EOF, so the multi-call
    incremental flows are unaffected.
  - guards: tests.rs test_push_completed_context_refuses_reparse (second
    single-shot final parse fires no events; instate stays EOF).
    cargo test --lib 1216 pass; clippy no new warnings; fmt clean; phase-13
    HOSTILE verdicts PASS; ext/xml 67 run -> 51 pass / 5 fail / 11 skip;
    five-extension remeasure unchanged.
  - receipt dir: php-14-3-reparse-guard-20260902/

KEY-1 closed (declared-encoding-on-BOM-less transcoded to UTF-8): full suite
  289 -> 283 (ext/xsl 58 -> 52; zero regressions: dom 169 / simplexml 9 /
  xml 5 / xmlreader 29 / xmlwriter 19 unchanged).
  - root cause: BOM-less stream with `<?xml encoding="iso-8859-1"?>` was never
    transcoded (only UTF-16 BOM paths converted); the tokenizer read raw
    Latin-1 bytes (e.g. 0xE4) as UTF-8 and raised XML_ERR_INVALID_ENCODING (81)
    "Invalid bytes in character encoding", which the oracle never emits.
    ext/xsl/tests/xslt.xml declares iso-8859-1 + one 0xE4 byte (line 20), so 27
    xsl diffs carried the spurious preamble; 6 now PASS outright
    (xslt001 + get/removeParameter-family + setparameter-nostring).
  - fix: src/xml/parser/input.rs — convert_declared_native_encoding()
    transcodes the buffered stream when the declaration names ISO-8859-1
    (latch US-ASCII; iconv-only names untouched, R-000157 unchanged);
    converted_to_utf8 + decl_pending latches threaded through all
    constructors/duplicate_for_reparse; push_bytes re-detects while a `<?xml`
    decl is pending and converts just the raw tail once latched.
  - guards: input.rs test_declared_latin1_bytes_transcoded_to_utf8 /
    _incremental_push_transcodes / test_duplicate_of_converted_latin1_stays_
    utf8; tests.rs test_parse_declared_latin1_bomless_memory_doc.
    cargo test --lib 1220 pass; clippy no new warnings (4 pre-existing);
    fmt clean. Oracle-pinned: load + saveXML byte-identical on both sides.
  - receipt dir: php-14-3-key1-declared-encoding-20260902/

KEY-2 + SP-14.3.1-8 closed (R-14.3-CONTENT-MARKUP-DECL + the SP-8 push/EOF
  default-markup cluster): full suite 283 -> 276 (dom 169 -> 166, xml 5 -> 1;
  zero regressions: simplexml 9 / xmlreader 29 / xmlwriter 19 / xsl 52
  unchanged).
  - root causes: (a) SP-14.3.1-8 — the push/SAX per-character crash cluster:
    default-handler raw markup is produced by PHP expat-compat seeking back from
    input->cur to the tag's '<', but the push context's C input pointed at the
    empty constructor buffer (stale base -> dangling deref) and EOF-in-construct
    prefixes ('<', '<!') were delivered as spurious text. sync_input_position()
    now repoints base/cur/end/line/col from the tokenizer's live buffer at every
    event (consumed=0 keeps bug26614 at 96); a trailing '<'/<! or an unterminated
    comment pauses in probes instead of firing partial events; Comment tokens
    carry unterminated.
    (b) KEY-2 — the content-`<!`-markup rule: a '<!' that is not '<!--',
    '<![CDATA[', or a prolog '<!DOCTYPE' is an invalid element start in element
    content (upstream xmlParseStartTag fails the name at '!' with
    XML_ERR_NAME_REQUIRED 68 + wellFormed=0); the tokenizer pre-screened such
    constructs into scan_start_tag and DocType-in-content raises 68 (was
    silently swallowed as text / internal-error-1-with-wf=1). Without this rule
    the SP-8 push edits regressed the two dom innerHTML/outerHTML *_writing_
    _errors_ tests (they accept '<!ENTITY ...>' content); with it they stay green.
  - measured: ext/xml 5 -> 1 (bug27908 + bug46699 + gh20439_1 + gh20439_2 PASS;
    xml_error_string_basic_libxml = SP-14.3.1-9 remains); dom 169 -> 166
    (innerHTML_cache_invalidation + Element_innerHTML_prefixed_writing +
    Element_outerHTML_writing PASS; no new failures; the remaining dom F1
    members Element_innerHTML_writing / innerOuterHTML_reading /
    insertAdjacentHTML are serializer-blocked ("Could not save document") =
    dom S1-family, pre-existing). Push probe now byte-identical to oracle for
    ENTITY/DOCTYPE/ELEMENT-in-content (wf=0 errNo=68) and comment/cdata/text
    (wf=1 errNo=0).
  - guards: tests.rs test_push_incremental_eof_prefixes_not_text (fails at
    HEAD), test_content_markup_decl_clears_wellformed.
    cargo test --lib 1222 pass; clippy no new warnings; fmt clean.
  - receipt dir: php-14-3-sp8-content-markup-20260902/

KEY-3 closed (PI-vs-XML-decl routing + reserved-name/not-finished codes):
  full suite 276 -> 275 — ext/xml 1 -> **0** (SP-14.3.1 fully closed;
  xml_error_string_basic_libxml PASS); zero regressions (dom 166 / simplexml 9
  / xmlreader 29 / xmlwriter 19 / xsl 52 unchanged).
  - root cause: `<?xml` was routed to the XML-declaration scanner for ANY
    case-insensitive `<?xml` at byte offset 0. Upstream xmlParseDocument only
    treats `<?xml` + BLANK (CMP5 + IS_BLANK(NXT(5))) at the LOGICAL document
    start as a declaration; every other `<?xml...` is an ordinary PI whose
    target must pass xmlParsePITarget. So `<?xml?>`/`<?xml>`/`<?XML?>`/even
    leading-space `<?xml?>` were misparsed as declarations (codes 4/57), and
    the legal `<?xml-stylesheet ...?>` PI was broken (57).
  - fixes: tokenizer routes declarations only at the base input's logical
    start (doc_start_offset — 3 after a retained UTF-8 BOM, so bug35447 stays
    green) with lowercase `xml` + blank; otherwise xmlParsePITarget semantics:
    exact lowercase "xml" / case-variant 3-char targets -> FATAL
    XML_ERR_RESERVED_XML_NAME (64), xml-prefixed non-W3C targets -> warning,
    xml-stylesheet/xml-model exempt; a PI never closed by `?>` records
    XML_ERR_PI_NOT_FINISHED (47) LAST (so `<?xml>` ends at 47 exactly like the
    oracle — the later error overwrites errNo); PI tokens carry unterminated so
    incremental probes pause. InputStack gains doc_start_offset()/at_base_input().
  - oracle-pinned (php probes, byte-identical): `<?xml?>`/` <?xml?>`/`<?XML?>`
    -> 64; `<?xml>` -> 47; `<?xml version="dummy">` -> 57;
    `<?xml-stylesheet ...?><r/>` and BOM+decl -> ok.
  - guards: tests.rs test_pi_vs_xml_decl_routing_error_codes.
    cargo test --lib 1223 pass; clippy no new warnings; fmt clean.
  - receipt dir: php-14-3-pi-decl-routing-20260902/

EXT-6 xmlwriter closed 19 -> 1 (2026-09-03): full suite 275 -> 255; zero
  regressions (dom 166 -> 164 via the same file-open routing:
  namespace_sxe_interaction + XMLDocument_fromString_02 PASS; simplexml 9 /
  xml 0 / xmlreader 29 / xsl 52 unchanged). Two root causes (receipt
  php-14-3-xmlwriter-20260903/):
  - RC-1 (writer engine, mirrors xmlwriter.c): W1 — End* emit the FULL stack
    QName `prefix:name` (StartElementNS pushes the QName; 006/007/011/012 +
    OO_* + bug41287/bug41326/write_attribute_ns prefix loss); W2 — empty-
    content elements close `></name>` on ONE line from the NAME state
    (FullEndElement clears doindent), END-tag indentation is `depth-1`
    (upstream `nodes-1`), empty WriteString in Element closes the tag
    (bug41287), attribute-ns queued `xmlns[:prefix]` decls deduped per element
    via queue_attr_ns_decl (write_attribute_ns_basic_001); W3 — bare prolog
    DTD children (`<!ENTITY/<!ELEMENT/<!ATTLIST` without `<!DOCTYPE`) via
    dtd_child_begin + dtd_bare flag, XMLDecl state removed
    (008/OO_008); W4 — StartPI/StartCDATA after a NAME parent emit `>` with NO
    newline (009 leaf); xmlTextWriterFlush does NOT close an open start tag
    (toMemory_flush_combinations).
  - RC-2 (W6 lifecycle, mirrors xmlIO.c/xmlwriter.c): the writer/save/html
    filename opens (`xmlNewTextWriterFilename`, exported
    `xmlOutputBufferCreateFilename`, `xmlSaveToFilename`, `htmlSaveFileFormat`)
    now honor the per-thread default installed by
    `xmlOutputBufferCreateFilenameDefault` (PHP installs
    php_libxml_output_buffer_create_filename at RINIT) — upstream
    `xmlOutputBufferCreateFilename` delegates to
    `xmlOutputBufferCreateFilenameValue` when set. Fixes bug71536
    (openUri "php://memory") + bug79029 (fclose of a writer/reader-owned
    NO_FCLOSE php stream warns instead of TypeError). The routing exposed a
    latent defect: `xmlURIUnescapeString` treated `len == 0` as a literal
    0-byte decode; upstream uri.c uses `if (len <= 0) len = strlen(str)`, and
    php's stream wrapper calls `xmlURIUnescapeString(path, 0, NULL)` — the
    empty path made php_stream_open_wrapper_ex throw ValueError "Path must not
    be empty" on every file open.
  - guards: io test_output_buffer_create_filename_routed_honors_default
    (registered default consulted; export funnels; builtin fallback); uri
    test_xml_unescape_string_len_zero_means_whole (len 0 -> whole string,
    %-decoding intact). cargo test --lib 1225 pass; clippy no new warnings;
    fmt clean. Probe kept: consumers/uri-probe.c (oracle-pinned unescape/
    escape/parse rows), openuri-mem.php; W5 byte evidence in the receipt
    (sjis-phpt-body.php vs oracle).
  - xmlwriter residual 1: xmlwriter_toStream_encoding_shiftjis = W5 — the
    comment content must be transcoded to real SHIFT_JIS bytes (oracle emits
    0x82 0x41 x3; candidate emits UTF-8) — encoder workstream W9/R-000157.

dom O1 xpath-php-function-callback bridge closed (2026-09-03): full suite 255
  -> 251; zero regressions (dom 164 -> 160 — return_dom_node_from_xpath +
  registerPhpFunctionNS + DOMXPath_constructor_registered_functions +
  gh22077 PASS; simplexml 9 / xml 0 / xmlreader 29 / xmlwriter 1 / xsl 52
  unchanged). Receipt: php-14-3-dom-o1-xpathfn-20260903/.
  - root cause: the C-extension-function bridge (`call_c_xpath_function`)
    invoked a registered `xmlXPathFunction` WITHOUT setting
    `ctxt->context->function` / `functionURI` to the invoked function's LOCAL
    name and namespace URI. Upstream xpath.c xmlXPathCompOpEval
    (XPATH_OP_FUNCTION) sets both before the call and restores them after;
    PHP registers ONE trampoline for every custom-namespace XPath function
    (dom `xmlXPathRegisterFuncNS`; xsl `xsltRegisterExtFunction`) and
    dispatches to the PHP closure by reading those two fields back
    (`dom_xpath_ext_fetch_intern` -> `php_dom_xpath_callbacks_call_custom_ns`
    looks the ns up in `ctxt->context->functionURI`, the name in
    `ctxt->context->function`) — garbage fields -> segv inside libc.
  - fix: the bridge now parses the registration key (`{uri}name` Clark
    notation from xmlXPathRegisterFuncNS, bare name otherwise) into local
    name + optional URI, NUL-terminates both, sets
    `function`/`functionURI` around the callback with save/restore of the
    previous values (nested calls), mirroring upstream exactly. The xslt
    extension-function closure (register_xslt_functions) now passes the
    local name + resolved href it already had at lookup time.
  - guards: exports_xml2 test_c_xpath_function_bridge_exposes_function_and_uri
    (end-to-end: register ns t->urn:t + C fn via xmlXPathRegisterFuncNS, eval
    "t:capture()", the callback reports "capture@urn:t" — fails at HEAD with
    a garbage deref). Probes kept: consumers/xpath-retnode.php (php pin).
    cargo test --lib 1226 pass; clippy no new warnings; fmt clean.
  - remaining O1 residuals in this family: DOMDocument_adoptNode / bug79968
    (docless-node adopt + saveXML teardown double-destroy — the pure-engine
    xmlDOMWrapAdoptNode + xmlSaveTree probe is byte-identical to the oracle,
    so the defect sits in the php-serializer/adopt interplay; repro probes
    consumers/adopt-reduce.php + bug79968-repro.php + savetree-probe.c) and
    DOMNode_isEqualNode / DOMElement_replaceChildren / gh22570 /
    xpath_domnamespacenode_advanced.

dom O1 deep-tree free recursion closed (2026-09-03): full suite 251 -> 250;
  zero regressions (dom 160 -> 159 — modern/xml/gh22570 PASS; simplexml 9 /
  xml 0 / xmlreader 29 / xmlwriter 1 / xsl 52 unchanged). Receipt:
  php-14-3-dom-o1-deep-free-20260903/.
  - root cause: free_node_list recursed per tree level; the 100k-deep
    Dom\XMLDocument (gh22570) overflowed the C stack at php shutdown AFTER
    saveXml's "Maximum call stack size reached" Error had already fired
    (segv during teardown, not during serialize). Upstream xmlFreeNodeList
    walks the tree ITERATIVELY with an explicit depth counter (tree.c 2.15).
  - fix: free_node_list rewritten as an iterative post-order walk with an
    explicit resume stack (node + its pre-descent next sibling, since the
    struct is freed on resume); DTD-skip and entity-ref no-descend semantics
    unchanged; free order (children before parents) unchanged.
  - guards: tree/mod.rs test_free_deeply_nested_chain_is_iterative (500k-deep
    chain freed through free_doc; a recursion regression dies in the test
    thread). cargo test --lib 1227 pass; clippy no new warnings; fmt clean.

KEY-4 part 1 (RECOVER diagnostics + NO_XXE gating) closed (2026-09-03): full
  suite 250 -> 241; zero regressions (dom 159 -> 152; simplexml 9 -> 7; xml 0
  / xmlreader 29 / xmlwriter 1 / xsl 52 unchanged). Receipt:
  php-14-3-key4-parse-options-20260903/. Three engine root causes:
  - recovery silent close: the parse_element EOF-in-content arm closed the
    element silently under XML_PARSE_RECOVER — upstream raises
    XML_ERR_TAG_NOT_FINISHED (77) BEFORE the recovery close (recovery only
    decides whether parsing continues). Now the raise fires in both modes;
    ext/simplexml + ext/dom xml_parsing_LIBXML_RECOVER show the full warning
    block like the non-recover case.
  - NO_XXE external-entity gate: xmlParseReference's first-parse/substitution
    phase ran for EXTERNAL general parsed entities even under NO_XXE
    (loader invoked -> spurious "I/O warning : failed to load" for
    file:///etc/passwd). Upstream gates on
    `(ent->etype == XML_INTERNAL_GENERAL_ENTITY) || (!NO_XXE &&
    (replaceEntities || validate))`; the candidate now skips the load and the
    reference expands to nothing (ext/simplexml + ext/dom
    xml_parsing_LIBXML_NO_XXE).
  - DTD serializer model (exposed by the NO_XXE member, mirrors xmlsave.c
    xmlDtdDumpOutput): the internal-subset decls were dumped from the HASH
    tables (hash-bucket order -> reversed multi-declaration output, RESIDUAL
    R-DTD-DUMP-ORDER); upstream dumps the DTD node's CHILDREN list
    (declaration order; notations stay hash-only). dtd_dump_output now walks
    dtd->children dispatching by decl node type. ALSO the intSubset-explicit
    dump in doc_content_dump_output fired when doc->children was EMPTY —
    php's modern serializer temporarily NULLs doc->children around xmlSaveDoc
    for a declaration-only pass, so the extra dump produced a duplicated
    <!DOCTYPE>; the explicit dump now requires a non-empty children chain.
    These two closed dom modern/spec/{Document_implementation_createDocument,
    _createDocumentType, clone_document, pre_insertion_validation} +
    modern/common/gh21077 (doctype-on-clone/creation serialization) as well.
  - guards (parser/tests.rs): test_recover_raises_premature_eof_tag_not_finished
    (errNo 77 both modes; tree returned under RECOVER);
    test_no_xxe_blocks_external_entity_and_doctype_serializes_once_in_order
    (serialized bytes: single doctype, foo before xxe, &xxe; expands to
    nothing). Probes kept: recover-probe.c / savedtd-probe.c / dtd-children.c
    (engine pins), recover-sxe.php (php pin). cargo test --lib 1229 pass;
    clippy no new warnings; fmt clean.

simplexml S5/S6 content discipline closed (2026-09-03): full suite 241 -> 239;
  zero regressions (simplexml 7 -> 5 — bug44478 + bug76712 PASS; dom 152 /
  xml 0 / xmlreader 29 / xmlwriter 1 / xsl 52 unchanged). Receipt:
  php-14-3-simplexml-addchild-20260903/.
  - root cause: the exported xmlNewChild treated content as RAW text
    (new_text storage) — upstream xmlNewChild -> xmlNewDocNode -> xmlNewElem
    parses the content as an ATTRIBUTE VALUE (xmlNodeParseAttValue): empty
    content adds NO text child, `&#38;`-style character references are
    decoded, declared general entities become entity-ref children, and a bare
    '&' without ';' consumes the '&' and keeps the rest as text. The
    crate's own xmlNewDocNode already routed through the faithful
    node_parse_att_value; xmlNewChild now mirrors upstream (build via
    xmlNewDocNode + direct link at the parent's tail).
  - additionally exports_tree.rs node_parse_att_value hard-failed on a bare
    '&' (no ';'), diverging from its exports_string.rs twin and upstream
    (tree.c: `if ((remaining <= 0) || (*cur == 0)) break;`) — xmlNewDocNode
    rejected contents like "x & y" that the oracle accepts as "x  y". Fixed
    to break-and-continue-as-text.
  - guards: exports_xml2 test_xml_new_child_parses_content_as_att_value
    ("" -> no child; "a &#38; b" -> text "a & b"; "x & y" -> "x  y").
    Probes kept: consumers/addchild-probe.php (php pin: oracle-equal on all
    four content shapes). cargo test --lib 1230 pass; clippy no new warnings;
    fmt clean.

simplexml S4 PI data boundary closed (2026-09-03): full suite 239 -> 238;
  zero regressions (simplexml 5 -> 4 — gh12167 PASS; dom 152 / xml 0 /
  xmlreader 29 / xmlwriter 1 / xsl 52 unchanged). Receipt:
  php-14-3-simplexml-pi-data-20260903/.
  - root cause: the tokenizer's PI scan skipped only ONE blank after the
    target and TRIM-med the trailing whitespace of the data. Upstream
    xmlParsePI SKIP_BLANKS consumes ALL blanks between the target and the
    data and copies every character up to the '?' of the terminator — the
    SAX data of `<?foo pi contents ?>` is "pi contents " (trailing space
    kept). SimpleXML's string() of the PI lost the last character
    (gh12167: length 11 instead of 12).
  - guard: parser/tests.rs test_pi_data_keeps_trailing_space_skips_all_
    leading_blanks ("pi contents " + `<?a   b  ?>` -> "b  ").
    cargo test --lib 1231 pass; clippy no new warnings; fmt clean.

simplexml S3 XPath error channel closed (2026-09-03): full suite 238 -> 236;
  zero regressions (simplexml 4 -> 3 — ext/simplexml 008 PASS; dom 152 -> 151 /
  xml 0 / xmlreader 29 / xmlwriter 1 / xsl 52 unchanged). Receipt:
  php-14-3-simplexml-xpath-error-20260903/.
  - root cause: raise_xpath_error (xpath.c xmlXPathErrFmt channel selection)
    delivered compile/eval failures with GenericDelivery::Stream, which
    routes through xmlFormatError's fragment stream and PREFIXES "XPath
    error : ". Upstream with no structured handler sets `channel =
    xmlGenericError` — NOT one of the parser channels, so xmlVRaiseError
    calls `channel(data, "%s", to->message)` with the message text ALONE.
    PHP installs a generic handler at request start
    (php_libxml_issue_warning), so the handler must receive "Invalid
    expression\n" — ext/simplexml 008 warned "XPath error : Invalid
    expression" and the warning text leaked the prefix into SimpleXML's
    xpath() error message. The delivery is now
    GenericDelivery::Custom(generic func, ctx) when a generic handler is
    installed (Stream fallback otherwise, preserving the xmllint/xsltproc
    console fragment stream). Structured handlers (ctxt->error / the global
    xmlStructuredErrorFunc) are unaffected.
  - guard: exports_xml2
    test_xpath_compile_error_verbatim_to_generic_channel — a recording
    generic handler receives exactly b"Invalid expression\n" for a
    failed xmlXPathCtxtCompile with no structured handler on the context.
  - probe kept: consumers/xpeval-probe.c (structured code=1207,
    msg=[Invalid expression] on both engines).
    cargo test --lib 1232 pass; clippy no new warnings (4 pre-existing:
    unnecessary-cast x2 + needless-option-as-deref + tree/mod.rs
    iter().any() — untouched); fmt clean.
