# KEY-1 — BOM-less declared encoding (ISO-8859-1) transcoded to UTF-8

Closed 2026-09-02. ext/xsl **58 -> 52 failed** (xslt001 +
xsltprocessor_{get,remove}Parameter{[,-invalidparam]} + setparameter-nostring
flip PASS; zero regressions: dom 169 / simplexml 9 / xml / xmlreader 29 /
xmlwriter 19 unchanged). Full suite 289 -> 283.

## Root cause

A BOM-less byte stream whose XML declaration names a *native* non-UTF-8
encoding was never transcoded. `InputBuffer::detect_bom_and_encoding` parsed
`encoding="iso-8859-1"` and recorded `Encoding::Iso8859_1`, but only the UTF-16
BOM paths converted the buffered bytes to UTF-8. The tokenizer then read the
raw Latin-1 byte (e.g. `0xE4` = 'ä') as UTF-8 and raised XML_ERR_INVALID_ENCODING
(81) "Invalid bytes in character encoding" — a FATAL the oracle never emits
(upstream installs the converter via `xmlSwitchEncoding` right after
`xmlParseXMLDecl`).

`ext/xsl/tests/xslt.xml` declares `encoding="iso-8859-1"` and contains one
`0xE4` byte on line 20. 27 of the 57 failing xsl diffs carried the spurious
warning preamble from its shared `prepare.inc` load; removing the warning alone
explains the 6 that now PASS outright (the other 21 have an independent real
engine divergence and remain — see plan-xsl families).

## Fix (src/xml/parser/input.rs, engine layer)

- `convert_declared_native_encoding()`: after the no-BOM XML-declaration
  detection, transcode the whole buffered stream to UTF-8 when the declared
  encoding is ISO-8859-1 (byte-wise mapping — the ASCII declaration itself is
  unaffected); latch US-ASCII (a UTF-8 subset) without transcoding. Unknown /
  iconv-only encodings (R-000157) are left untouched so the existing
  unsupported-encoding path applies unchanged.
- `converted_to_utf8` + `decl_pending` latches on `InputBuffer`, threaded
  through all constructors and `duplicate_for_reparse`.
- `push_bytes`: incremental pushes re-run detection while a `<?xml` declaration
  is pending (it may only complete once the stream accumulates) and, once
  latched for ISO-8859-1, convert just the new raw tail (byte-wise) so
  multi-call `xmlParseChunk` feeds of a declared-Latin-1 doc stay consistent.

## Guards (regression pins)

- input.rs: `test_declared_latin1_bytes_transcoded_to_utf8`,
  `test_declared_latin1_incremental_push_transcodes`,
  `test_duplicate_of_converted_latin1_stays_utf8`.
- parser/tests.rs: `test_parse_declared_latin1_bomless_memory_doc` (engine
  tree: `<r>ä</r>` decodes, no encoding error).
- cargo test --lib 1220 pass / 1 ignored; clippy no new warnings (4 pre-existing
  at HEAD); fmt clean.

## Oracle-pinned expectations (verified live both sides)

BOM-less `<?xml encoding="iso-8859-1"?><r>ä</r>` (byte `E4`):
- oracle (2.15.3) & candidate now: load OK, no warning; `saveXML()` emits
  `<r>` `E4` `</r>` byte-identical; simplexml_load_file returns the element.

## Receipts / evidence

- pre: courts/receipts/phase-14/plan-xsl-head.txt (57) ; post:
  plan-xsl-after-key1.txt (52).
- logs (phpbuild-c): /out/key1-xsl5.log, /out/key1-xsl-full.log,
  /out/key1-other5.log.
