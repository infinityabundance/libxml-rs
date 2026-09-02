# Phase 14.3 — SP-14.3.1-8 (gh20439/bug27908) closure analysis (NOT sealed)

Authoritative state: working tree reverted to f190faeb (SP-14.3.1-7). The
in-progress SP-14.3.1-8 engine edits (`src/xml/parser/state.rs`,
`src/xml/parser/tokenizer.rs`) + a proposed `tests.rs` guard were **not**
committed: though they close the three ext/xml targets, they regress two
currently-green `ext/dom` XML-fragment tests. Root cause below.

## What SP-14.3.1-8 targets
`ext/xml` push/SAX per-character crash cluster:

- `bug27908.phpt` — default handler must receive the RAW markup `<root>` /
  `</root>` (SAX1 expat-compat single-shot).
- `gh20439_1.phpt` — namespace parser, PER-BYTE `xml_parse(..,false)` feed,
  default handler must receive the raw comment + start tag + end tag, byte
  identical to source (entity refs left literal, no decode), no crash.
- `gh20439_2.phpt` — `xml_parser_create_ns` self-closing tag: default handler
  gets the raw `<ns:test xmlns:ns='urn:x'   >` AND a synthesised `</ns:test>`.

## The crash/cluster root cause (author-identified, verified)
PHP expat-compat's default-handler raw markup is produced by seeking back from
`xmlParserInput->cur` to the tag's opening `<` and passing `base..cur`.
Two candidate defects made that seek dereference a dangling 0x1 pointer and/or
emit nothing:

1. Push context (`xmlCreatePushParserCtxt`) wired the C `ctxt->input` to the
   empty constructor buffer; the accumulated buffer is rebuilt on every
   `xmlParseChunk`, so `base` became stale. `sync_input_position()` now
   repoints `_xmlParserInput` base/cur/end/line/col from the tokenizer's
   LIVE buffer at every event (`consumed` zeroed so the compat byte-index
   `consumed + (cur - base)` stays = `cur - base`; keeps bug26614 = 96).
2. EOF-in-construct must be a PAUSE, not text: a chunk ending right after
   `<`, `<!`, or inside a comment was previously returned as
   `Characters("<")`/`Characters("<!")` and delivered as data, corrupting the
   per-byte feed. The tokenizer now (a) routes a trailing `<` through
   `scan_start_tag` (records NAME_REQUIRED, marks unterminated), (b) returns
   `Eof` for a trailing `<!` and for undecided/unfinished markup decls, and
   (c) `Comment` tokens carry an `unterminated` flag so state.rs can pause
   (`truncated_abort`) during silent probes / partial delivery instead of
   firing a partial comment or its "Comment not terminated" error.

## Measured: the three targets PASS, ext/xml 5 -> 1, zero ext/xml sibling regress
With the edits, candidate `ext/xml` = 67 run / 55 pass / 1 fail / 11 skip; the
only failure is `xml_error_string_basic_libxml` = SP-14.3.1-9 (next atom).
bug27908 + gh20439_1 + gh20439_2 PASS; bug81351 / XML_OPTION_PARSE_HUGE /
gh12254 / bug46699 / xml_set_object_multiple_times{,_errors} stay green.
New Rust guard `test_push_incremental_eof_prefixes_not_text` FAILS on HEAD and
PASSES with the edits (genuine regression guard). `cargo test --lib` 1217 pass;
clippy no new warnings; fmt clean.

## Why it was NOT sealed: two green `ext/dom` XML-fragment tests regress
Full six-extension candidate remeasure: dom 169 -> 168 total, but the dom FAIL
SET shifts: 3 dom tests flip PASS (innerHTML_cache_invalidation,
Element_innerHTML_prefixed_writing, Element_outerHTML_writing) while 2 flip FAIL
(Element_innerHTML_writing_errors, Element_outerHTML_writing_errors). Net -1
masks a 2-test regression. Oracle passes all five; neither pre nor post is
oracle-clean on the family.

Repro (candidate dom writes an XML fragment via a PUSH wrap
`<context xmlns...> <fragment> </context>` in `ext/dom/inner_outer_html_mixin.c
dom_xml_fragment_parsing_algorithm`, feeding many tiny non-final
`xmlParseChunk("<",1,0)` calls then a terminating `">",1,1`):

- inner case 6 `innerHTML = '<!ENTITY foo "content">'` -> no exception, doc
  MUTATED (oracle: "XML fragment is not well-formed", doc unchanged).
- outer case 2 child `outerHTML = '<!DOCTYPE html>'` -> no exception (oracle
  throws "XML fragment is not well-formed").

## Bisect result
Reverting ONLY `tokenizer.rs scan_tag_or_markup` (back to `<`-at-EOF ->
`Characters("<")`) fixes both dom regressions BUT re-breaks all three ext/xml
targets. `sync_input_position()` (state.rs) is NOT the culprit (isolated test).
So the `<`-at-EOF routing that ext/xml needs is what perturbs the dom push
fragment path.

## Underlying engine truth (why a fragile reconciliation is unacceptable)
A C probe mirroring the dom wrap feed (`<root><!ENTITY foo "content"></root>` /
`<root><!DOCTYPE html></root>` pushed) shows:

- Oracle: ENTITY and DOCTYPE both -> `wellFormed=0`, `errNo=68`
  "StartTag: invalid element name" (a `<!...` that is not `<!--` / `<![CDATA[`
  IS an invalid element start in a CONTENT position).
- Candidate (pre-SP8 AND post-SP8 identically): ENTITY -> accepted as text
  (`wellFormed=1`), DOCTYPE -> `errNo=1` ("Unexpected token") but
  `wellFormed` stays 1.

So the candidate engine ALREADY fails to reject markup decls in element
content (pre-existing SP-14.3.3-scoped defect); `xmlParseInNodeContext` returns
68/68/68 for all three on both oracle and candidate, so the *pull* fragment API
is at parity — the divergence is in the *push* fragment (inner/outerHTML) path,
where `wellFormed` must be cleared for content-position `<!ENTITY` / `<!DOCTYPE`.

## Correct closure requires (do NOT ship the raw edits alone)
Make a `<` followed by `!` that is NOT `<!--`, `<![CDATA[`, or a DOCTYPE in its
legal position raise `XML_ERR_NAME_REQUIRED` (68) AND clear `ctxt->wellFormed`
in a CONTENT position, mirroring `xmlParseStartTag` — while STILL permitting
decls in the DTD/internal-subset path and permitting `< !DOCTYPE` in the prolog.
That single engine rule reproduces `wellFormed=0 errNo=68` for the dom fragment
cases AND removes the rely-on-`Characters("<")` fragility that SP-14.3.1-5/6/8
touched. It is SP-14.3.3 (dom-fragment/content-markup) work and needs its own
validation across dom/simplexml/xml before SP-14.3.1-8 can be sealed without a
regression.

Recommended sequencing:
1. Land the content-position `<!`-markup rejection (domain: SP-14.3.3) with the
   three dom inner/outer tests as guards.
2. Then re-apply the SP-14.3.1-8 tokenizer/state edits + `tests.rs` guard and
   re-run the matrix (ext/xml targets, dom 5-family, five-extension remeasure,
   cargo / clippy / fmt / phase-13 HOSTILE). Net dom should then be a strict the
   3-improvement (0 regression) or better.

## Diagnostic probes (consumers, host tree)
- `lt-eof-probe.c` — oracle: push non-final `<` waits (rc0, wf1); per-char feed
  pure.  (author-supplied)
- `gh20439-state-probe.php` — candidate==oracle default-handler bytes; residual:
  candidate reports transient per-char `xml_get_error_code()` (68/45/73) where
  the oracle reports none — recorded here, not chased (out of the asserted
  gh20439 contract; would need errNo-suppression-on-truncated-probe work).
