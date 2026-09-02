# SP-14.3.1-3 — push delivery + SAX1/SAX2 dispatch + parser ns scope

Closed 2026-09-02. ext/xml: **18 -> 10 failed** (xml009, xml010, bug25666,
bug50576, bug72714 flip PASS; bug73135 + xml_set_object_multiple_times{,_errors}
also flipped via the same roots; zero sibling regressions; full-suite log
`/out/sp1-xml7.log` = 67 run / 46 pass / 10 fail / 11 skip).

## Root causes (three engine layers, all mirroring upstream parser.c/SAX2.c)

1. **Incremental push delivery** — PHP `xml_parse()` defaults `isFinal =
   false`, and upstream `xmlParseChunk` parses every chunk EAGERLY. A single
   non-final call carrying a complete document therefore delivers all events
   on the oracle; the candidate only parsed on the terminating call, so
   xml009/xml010/bug25666/bug72714 (single-shot `xml_parse($p, $doc)`) never
   fired an event. Related: a doc whose final token is an ERROR (bug25666's
   `</foo>` closing `<foo:a ...>`) still delivered everything up to the error
   on the oracle.
2. **SAX1 vs SAX2 dispatch** — upstream `xmlCtxtInitializeLate` selects the
   SAX1 parse path (`xmlParseStartTag`) unless
   `!(options & XML_PARSE_SAX1) && sax->initialized == XML_SAX2_MAGIC &&
   (startElementNs || endElementNs || no SAX1 handlers)`. PHP's expat-compat
   layer resets `sax->initialized = 1` for `xml_parser_create()` (non-NS):
   those parsers must receive SAX1 events — the RAW QName and the full
   SAX1 attribute list with `xmlns` declarations as ordinary attributes and
   no namespace processing — but the candidate dispatched every element
   through the SAX2 `startElementNs` slot, producing URI-joined garbage names
   (bug50576) and dropped xmlns attrs.
3. **Parser-scoped namespace stack** — upstream keeps in-scope namespaces on
   the parser context (`ctxt->nsTab`, `xmlParserNsPush` in
   `xmlParseStartTag2`). The candidate resolved ancestor prefixes only through
   the tree (`ctxt->node`), which is NULL for pure-SAX parses, so
   `<foo:a xmlns:bar="u"><bar:b/></foo:a>` could not resolve `bar` for the
   child and raised a bogus "Namespace prefix ... is not defined"
   (bug25666/bug81351 observed errNo 201).

## Fixes

1. `helpers::parse_chunk` + `XmlParser` probe modes (`state.rs`):
   - per-context push state (`completed`) freed with the context;
   - every non-final call runs a SILENT completeness probe over the
     accumulated input: SAX delivery deferred (`probe` flag), diagnostics
     deferred (bookkeeping kept), EOF inside an open construct pauses
     (`paused`), EOF inside a truncated construct marks `truncated_abort`;
   - the probe delivers when the input parsed through to its end: `rc == 0`
     (clean document end) OR `rc != 0 && !paused && !truncated_abort` (a
     failure on a complete token at the input end — bug25666);
   - delivery = one completing parse (`XmlParser::new`), then later calls
     only buffer (epilog) and the terminating call re-validates silently,
     with a sax-suppressed diagnostics pass (`sax_suppressed`) when the
     accumulated input no longer parses cleanly (trailing junk surfaces its
     errors exactly once);
   - probe-time entity resolution is side-effect-free (predefined + doc
     registry), because PHP's compat getEntity feeds the default handler.
2. `parse_element`/`sax_start_element`/`sax_end_element` select SAX1 vs SAX2
   via `sax2_mode()` (mirror of `xmlCtxtInitializeLate`); SAX1 dispatch goes
   through the context's OWN `sax` struct (`userData` is the consumer's
   opaque object — PHP's XML_Parser struct — never a parser context).
3. `XmlParser.ns_scope` stack pushed/popped per element in SAX2 mode when
   `ctxt->node == NULL`; consulted by the element/attribute URI resolution
   and the `prefix_declared` validation (upstream `ctxt->nsTab` semantics).

## Guards

- `src/xml/parser/tests.rs`: `test_push_single_shot_without_terminate_delivers`,
  `test_push_chunked_delivers_once`, `test_push_error_at_end_still_delivers`,
  `test_push_sax1_raw_qname_and_xmlns_attributes` (counters + captured
  events, thread-local for the parallel runner). `cargo test --lib` 1206
  pass / 0 fail / 1 ignored.
- C probes kept under `courts/suites/phase14/consumers/`: `nspush-probe.c`
  (candidate dispatch trace), `nsoracle-probe.c` (system-2.15.3 oracle pin),
  `undecl-entity-probe.c` (atom -2 severity pin), `/tmp/pushprobe.c` pattern
  re-verified byte-identical (single-shot 2/2/2, chunked 2/2/2, final no
  re-delivery).
- Phase-13 HOSTILE-ABI / HOSTILE-ALLOCATOR / HOSTILE-CALLBACKS /
  HOSTILE-FAILURE / HOSTILE-OWNERSHIP courts re-run PASS (xmlParseChunk
  behavior change verified against the oracle).
- clippy (no new warnings beyond pre-existing toolchain drift) and
  `cargo fmt --check` clean.
