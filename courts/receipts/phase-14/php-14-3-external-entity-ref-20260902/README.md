# SP-14.3.1-4 receipt — bug35447 + bug71592 (ext/xml 10 -> 8)

Closed by the next commit after 5b215015 (SP-14.3.1-3). ext/xml subset:
67 run -> 48 pass / 8 fail / 11 skip (was 46 pass / 10 fail after -3), zero
sibling regressions. Also closed bug30875 + gh14834 (exposed by the instate
ABI fix as double entity delivery).

## Evidence in this directory

- xml-subset.log        — ext/xml 67-test run on the candidate (post-fix)
- ext71592-php-probe-candidate.txt / -oracle.txt — byte-identical php-visible
  output for the bug71592 flow (handler fires, err=21, line=6, parse stops)
- (host) dom-now.txt / dom-then.txt — dom 169 failures = the -3 state; the 4
  fragment/adoptNode failures verified pre-existing by stash-rebuild at HEAD

## Probes kept in courts/suites/phase14/consumers/

- ext71592-probe.c      — engine-level getEntity/external-entity differential
- ext71592-probe.php    — php-visible bug71592 differential (runs both sides)
- ctxoffset-probe.c     — offsetof probe pinning the ABI layout (real headers
                         vs candidate headers: identical; the Rust-side guard
                         in src/abi/structs.rs asserts the same numbers)
- bom-probe.c           — 4-case real-BOM/literal-BOM push differential
- chunkrc-probe.c       — non-final/final xmlParseChunk return-value contract
- intent-probe2.c       — wf0-vs-wf1 entity delivery (compat branch clone)
- extget-probe.c        — residual: declared external entity + getEntity(NULL)

## Root causes (engine layer)

1. R-14.3-BOM-PUSH       — input.rs push_bytes re-detects BOM when the first
   real bytes arrive on an initially-empty push buffer.
2. R-14.3-INSTATE-ABI     — ctxt->instate values: the engine, the installed
   header and abi::types all disagreed with the real 2.15 enum (engine
   CONTENT=3, header 4, real 7). All now mirror the real header verbatim;
   PHP compat's `instate == XML_PARSER_CONTENT` gate only opens at 7.
3. R-14.3-EXT-ENTITY-REF  — external general PARSED entities register into the
   expat-compat registry only with replaceEntities (upstream gate); a
   getEntity-triggered stop honours `if (!wellFormed) return;` and the
   starting wellFormed survives probe/delivery re-parses.
4. R-14.3-CHUNK-RC        — parse_chunk reports errNo when wellFormed==0 and
   refuses chunks after xmlStopParser (upstream xmlParseChunk).
5. Candidate php rebuilt against the corrected installed headers (its compat.c
   had compiled XML_PARSER_CONTENT=4 from the old header).

## Guards added (src/xml/parser/tests.rs)

- test_push_utf8_bom_and_dtd_attr_defaults (+ no-BOM control)
- test_push_external_entity_ref_stop_keeps_handler_error
- test_push_wf0_compat_context_single_entity_delivery
- test_push_wf1_context_substitutes_entity_content (control)
- test_push_error_at_end_still_delivers updated to oracle rc (76)
- src/abi/structs.rs parser_ctxt/entity/sax_handler ABI-layout guards

cargo test --lib 1212 pass; clippy no new warnings; fmt clean; dom/simplexml/
xmlreader/xmlwriter/xsl remeasured: no new failures vs the -3 state.
