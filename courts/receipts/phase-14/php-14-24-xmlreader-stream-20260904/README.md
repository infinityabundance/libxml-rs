# Phase 14.24 — xmlreader streaming (partial-doc prefix delivery + deferred EOF finalize): 2 -> 1

Gate: xpe-six41b.log = 1 failure (xmlwriter shiftjis = oracle-failing parity
only). cargo fmt clean, cargo test --lib 1242 pass (1241 + 1 new). Valgrind
0 errors on the new path (stream-finalize-valgrind-probe.php).

**All candidate-driven failures are now closed.** The single remaining
failure, `xmlwriter_toStream_encoding_shiftjis`, fails IDENTICALLY on the
oracle container (re-verified this phase): raw Shift_JIS bytes (82 A1) can
never equal the checked-in `.exp` empty-comment text — the `.exp` is
unsatisfiable and the candidate's SJIS emitter matches the oracle byte for
byte. It is excluded from the drop-in target as oracle-parity.

## The last candidate-driven failure: xmlreader fromStream_broken_stream

The test opens `php://memory`, writes `<root><!--my comment-->` (a
deliberately UNTERMINATED doc), builds `XMLReader::fromStream($h, ...)`,
appends `<child/></root>` + fclose after the first read, and loops
`while (@$reader->read())`. Expected: read #1 = ELEMENT root, read #2 =
COMMENT "my comment", read #3 = false (suppressed premature-EOF), then
`depth == 1`.

### Root cause

The candidate slurps the stream at construction (fine: php://memory EOF
caching means upstream sees the same first 23 bytes) but then ran a
WHOLE-DOCUMENT terminating parse on the first `Read()`: the incomplete doc
failed immediately with `Premature end of data`, read #1 returned false
(`int(0)`), no prefix events were ever delivered.

Upstream xmlreader.c is a PUSH-based streaming reader
(`xmlTextReaderPushData` + `xmlParseChunk`): it parses whatever the input
currently holds and yields events as constructs complete; an end of the
available input inside an open construct PAUSES (no premature-EOF yet). The
oracle probe (io-readlog-probe.php, run on upstream 2.15.3) confirmed:
two stream pulls (23 bytes, then 0) happen on the first read; events ELEMENT
root then COMMENT are delivered across reads #1/#2; only read #3 (past the
last event, needing more input) runs the terminating chunk and reports the
error — with the cursor FROZEN on the comment (nodeType 8, depth 1 persist
on every later read).

### Fix (root-cause, reuse of the SP-14.3.1-6 push machinery)

`src/xml/reader/mod.rs`:

1. `parse_and_build_events` now runs the reader's first parse with the
   eager-partial-delivery engine (`XmlParser::new_with_partial_resume`, the
   same pause-tolerant engine the push API uses): a complete document
   behaves exactly as before; an input that ends inside an open construct
   (paused at a construct boundary or truncated mid-construct, wellFormed
   still 1) is classified INCOMPLETE instead of failed. The completed prefix
   is kept as `self.doc` and events are built with the END_ELEMENT events of
   the still-open elements (the `ctxt->nodeTab[0..nodeNr]` chain, snapshotted
   before the context is freed) SUPPRESSED — upstream emits END only once the
   end tag parses or the document completes. A re-parseable copy of the
   accumulated input is retained (`reparse_input`).
2. New `run_eof_finalize` / `try_finalize_incomplete`: the `Read()` (and the
   exhausted `Next()` paths) that run past the last delivered event of an
   incomplete document re-parse the retained input in a fresh context with
   full diagnostics — `Premature end of data` / `Document is empty` fires
   exactly on that read, then `state = ERROR` with the cursor fields
   untouched (every later read returns -1 while nodeType/depth/value still
   report the last node). Events-empty incomplete first reads (empty input,
   construct truncated from the start) finalize within read #1, matching
   upstream's INITIAL read (-1, no node to yield).
3. Definitive parse failures (wellFormed 0, or an error on a complete token)
   keep the old fail-at-first-read semantics; deferred schema/RNG validation
   and preserve-pattern pruning still run only on complete documents.
4. `xmlTextReaderSetup` resets the new state (`doc_incomplete`, `finalized`,
   `reparse_input`).

New unit test `test_read_incomplete_document_defers_eof_error` locks the
contract: root + comment delivered, read #3 = -1 with cursor frozen.

### Evidence

- fromStream_broken_stream.phpt: PASS (was the last candidate-driven fail).
- ext/xmlreader suite: rc=0, all PASS.
- Full six-extension gate xpe-six41b.log: 1250 passed, 1 failure (shiftjis =
  oracle parity; oracle fails it identically).
- cargo test --lib: 1242 passed.
- Valgrind (phpbuild-c, USE_ZEND_ALLOC=0, stream-finalize-valgrind-probe.php
  covering broken-stream + legit fromStream + empty stream):
  0 errors from 0 contexts.

Candidate-driven failure count: 0. Next milestone: decide the disposition of
the shiftjis `.exp` (recorded oracle-parity; drop-in target excludes it).
