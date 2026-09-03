# Phase 14.3 — simplexml S8 / dom-L2: filename opens route through the registered input loader (php streams)

Date: 2026-09-03
Suite movement: **236 failed → 233 failed**, zero regressions (the only phpt
flipped is `ext/simplexml/tests/bug79971_1.phpt`; one further variant of an
already-failing phpt also healed). Logs: `phpbuild-c:/out/xpe-six3.log`
(comprehensive), `/out/xpe-six2.log` (intermediate).

## Member closed
- `ext/simplexml/tests/bug79971_1.phpt` — `simplexml_load_file("file://…%00foo")`
  now emits both oracle warnings (php `URI must not contain percent-encoded NUL
  bytes` + engine `I/O warning : failed to load …`) and returns false;
  `simplexml_load_file($uri)` (valid `file://`) loads, and
  `$sxe->asXML("…%00foo")` emits the php percent-NUL warning and returns false.

## Root cause
The engine's filename-input creation NEVER consulted the registered
`xmlParserInputBufferCreateFilenameDefault`. PHP installs its php-streams
loader (`php_libxml_input_buffer_create_filename`) at request init, and
upstream routes EVERY filename open through it:

- upstream `xmlNewInputFromUrl` (parserInternals.c 2.15): the registered
  value is called first; a NULL result is `XML_IO_ENOENT` **with no built-in
  fallback**; `xmlNewInputFromFile` then raises
  `xmlCtxtErrIO(ctxt, XML_IO_ENOENT, uri)` — `I/O warning : failed to load
  "%s": %s`.
- php's loader unescapes `file://` URIs to plain paths, enforces the
  percent-encoded-NUL guard (its own E_WARNING + NULL), honors php stream
  contexts and suppresses its own "file not found" stream notice via a QUIET
  stat.

The candidate instead read the LITERAL path with an internal file reader at
every site (`helpers::input_from_file` / `io::input_buffer_create_file`):
`file://…` URIs could not load at all, PHP's `%00` guard never fired, and
failed opens were SILENT (upstream warns). Consequences visible in the suite:
bug79971_1 (simplexml), plus the whole family of missing-file / file://
warning tests across dom/xmlreader/xsl (dom L2 rows, xmlIO loader rows) and
entities loaded through php streams.

## Fix
New shared machinery in `src/abi/exports_parser.rs`:

- `call_loader_materialize(uri)` — invoke the registered
  `xmlParserInputBufferCreateFilenameDefault` through the C ABI
  (`func(uri, XML_CHAR_ENCODING_NONE)`), read the produced buffer's bytes via
  its read callback (memory-backed buffers fall back to their content), then
  release the C buffer so the close callback (php stream close) runs exactly
  once. NULL loader result → `Err`.
- `open_filename_routed(uri) -> RoutedFileOpen::{Builtin,Failed,Loaded}` —
  no loader registered → `Builtin` (caller keeps the built-in open); loader
  NULL → `Failed`; bytes → `Loaded(InputBuffer)` (filename = original URI).
- `io_load_failure_message(uri)` — the upstream xmlCtxtErrIO text
  (`failed to load "…": No such file or directory` when errno is stale).

All 11 filename-open sites now consult the loader first (upstream
xmlNewInputFromFile semantics):

1. `xmlReadFile` (exports_xml2)
2. `xmlCtxtReadFile` (exports_parser)
3. `xmlCreateFileParserCtxt` (exports_xml2 — dom `DOMDocument::load`)
4. `xmlCreateURLParserCtxt`
5. `xmlSAXParseFile`, `xmlSAXParseFileWithData`, `xmlSAXUserParseFile`,
   `xmlSAXParseEntity`
6. `xmlParseDTD`, `xmlParseEntity`, `xmlParseCtxtExternalEntity`
7. `xmlReaderForFile` (xmlreader) — SILENT on failure: upstream builds the
   reader input via `xmlParserInputBufferCreateFilename`, which does not
   raise xmlCtxtErrIO on ENOENT; php's binding reports "Unable to open
   source data" alone (regression seen mid-session on
   `open_error.phpt` / `fromUri_custom_constructor_error.phpt` fixed by this
   distinction).
8. `default_external_entity_loader` — external DTD/entity loads (XXE family)
   now read through php streams; NO_XXE/NONET gating unchanged (the loader is
   simply not reached under NO_XXE). The loader's bytes are rebuilt as a
   memory-backed C input for the entity machinery (base/end consumption).

`Failed` raises the xmlCtxtErrIO report through the context's parser channel
for the doc-parse sites only. Engine-only contexts without a registered
loader keep the built-in file open byte-for-byte.

## Evidence
- `/out/xpe-six3.log`: 1291 tests / **233 failed** / 40 skipped. Name-level
  diff vs the 236 baseline: **0 new**, exactly bug79971_1 fixed.
- Probes kept (candidate == oracle byte-for-byte):
  - `consumers/nul-uri-probe.php`: plain-path + `file://` load, `file://`+%00
    (both warnings + false), `asXML` to a %00 URI (php warning + false).
  - `consumers/missing-file-probe.php`: simplexml_load_file missing → engine
    I/O warning + false (was silent); DOMDocument::load / XMLReader::open
    suppressed with `@` in both engines.
- Guard: `exports_parser::tests::test_main_doc_open_consults_registered_input_loader` —
  a php-shaped loader (xmlParserInputBufferCreateIO over a serving state)
  satisfies a main-doc open over a bogus `file://` URI (stream closed exactly
  once); a NULL loader result fails the open with the "failed to load" report
  on the error channel.

## Validation
- `cargo test --lib`: 1233 passed / 1 ignored (1234 total, +1 guard).
- `cargo clippy --lib`: no new warnings (4 pre-existing, untouched).
- `cargo fmt --check`: clean.
- Six-extension php suite at 233 (this log), zero regressions.

## Commit
`00f1d19a (pushed to origin/main)`
