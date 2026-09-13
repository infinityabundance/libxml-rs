# lxml court 7bd — push recover delivery and explicit input-encoding override

## Root causes fixed

### 1. Recover-mode push parsers lost the document

`test_feed_parser_recover` fed a document split across six `feed()` calls and
`close()` returned `None`; `_no_id_dict` returned a root with no children.

Two replay-engine defects:

* A non-final call left the context at `XML_PARSER_EOF`, and the replay-only
  EOF gate at the top of `parse_chunk` then swallowed the terminating call —
  the parser never ran its final recovery. The EOF→EPILOG reset was
  conditional on `wellFormed != 0`; it is now unconditional for non-final
  calls (the `disableSAX` gate, not the EOF gate, is what stops later calls for
  a non-recoverable fatality).
* Even after that, eager partial delivery re-parses the accumulated stream
  from a byte offset against live tree state, and the recovery path for a
  stray end tag (an ancestor closed while a descendant is open) cannot
  tolerate a mid-document restart: the open `<a>` was reparented to the
  document instead of staying inside `<root>`.

The second is architectural to the replay engine, so recover-mode push input is
now accumulated (like every other unresolved case in that engine) and finalized
at the terminating call, which parses from byte zero — the same shape as the
single-feed case that already produced the correct tree. Non-final recover calls
deliver nothing; a recover parse is not required to deliver events before
`close()`.

Fixes `test_feed_parser_recover` and `test_feed_parser_recover_no_id_dict`.

### 2. `xmlCtxtResetPush`'s encoding override only covered the first bytes

lxml's `iterparse(encoding=...)` passes the override to `xmlCtxtResetPush`,
which switches the input encoding. `apply_name_encoding_override` converted the
bytes buffered so far and then set the encoding identity to UTF-8, so every
later chunk was appended raw and decoded according to the document's own
declaration — a Latin-1 document declaring UTF-8 raised "Invalid bytes in
character encoding".

The override now keeps the native encoding identity (`Iso8859_1`, or
`Other("windows-1252")`), so `append_decided` transcodes every later chunk
through the same handler: the override governs the whole stream, as upstream's
pre-parse `xmlSwitchEncoding` does.

Fixes `test_iterparse_encoding_8bit_override`.

## Evidence

* Full suite: **8 raw unique failing ids**, down from 11 — **3 fixed,
  0 regressions**:
  * `test_feed_parser_recover`
  * `test_feed_parser_recover_no_id_dict`
  * `test_iterparse_encoding_8bit_override`
* `cargo test --lib`: 1325 passed / 0 failed / 3 ignored.
* `cargo fmt --check`: clean.
* PHP six-gate: 1250 passed / 0 failed.
* `failing-ids.txt` is the raw unique id set; the filtered delta against the
  pre-slice baseline is exactly the three ids above.

GitHub still carries no status checks or workflow runs; these are local and
committed results.
