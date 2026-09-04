# Phase 14.27 — writer output encoder (R-000198 FIXED) + native Shift_JIS/EUC-JP converters (R-000157 slice)

Gates (candidate, same DSOs): NTS six-extension file-list = **1290 / 1250
passed / 40 skipped / 0 failed** (`/out/nts-cand-six-1427.log`); ZTS
six-extension file-list = **1290 / 1250 / 40 skipped / 0 failed**
(`/out/zts-cand-six-1427.log`). `cargo test --lib` **1247 pass** (5 new
encoding tests); cargo fmt clean; valgrind 0 errors on the new encoder path
with byte-identical output.

## What landed

### 1. Writer output-encoder install (R-000198 → FIXED)

`xmlTextWriterStartDocument` (`src/xml/writer/mod.rs`) now mirrors upstream
xmlwriter.c exactly:

- the declared encoding is resolved through the encoding registry
  (`xmlFindCharEncodingHandler`);
- a handler the registry cannot serve makes the call return -1 WITHOUT
  writing anything (upstream "unsupported encoding" semantics; php
  `XMLWriter::startDocument` returns FALSE on -1);
- a found handler is INSTALLED on the writer's output buffer —
  `out->encoder` (registry-owned pointer) + a 4000-byte `out->conv` buffer —
  before the XML declaration is written;
- every subsequent write lands in the raw UTF-8 buffer and is transcoded at
  `output_buffer_flush` (the io flush machinery converts when `ob.encoder`
  is set and `conv` is present).

Previously the writer only set an `encoder_active` byte-count flag and never
installed `out->encoder`/`conv`, so `XMLWriter::toStream()` +
`startDocument(encoding:"SHIFT_JIS")` emitted unconverted UTF-8.

### 2. Native Shift_JIS + EUC-JP converters (R-000157 slice)

`src/xml/encoding/mod.rs` registers native input+output handlers (names
`SHIFT_JIS`/`SJIS`/`CP932` and `EUC-JP`/`EUCJP`; lookup is case-insensitive)
backed by `encoding_rs` (WHATWG Shift_JIS = the CP932-compatible superset;
WHATWG EUC-JP):

- shared `enc_rs_output`: converts complete UTF-8 characters, stops with the
  house -2 input-error convention at the first character the target cannot
  represent (so `char_enc_out` substitutes the upstream DECIMAL character
  reference and retries — the encoding_rs `Unmappable(char)` payload's
  `read` includes the bad character, so `*inlen` is rewound by
  `char::len_utf8()` to point at it), and hard-errors (-1) on invalid UTF-8;
- shared `enc_rs_input`: decodes complete characters, hard-errors on
  undefined bytes / incomplete tails (iconv EILSEQ semantics, loop-free for
  the caller);
- the registry entries are stateless and process-lifetime like every other
  built-in handler (`xmlFindCharEncodingHandler`/`_owned`,
  `xmlCharEncCloseFunc` ownership, and the writer's non-owning
  `out->encoder` install are all consistent).

## Byte-parity evidence (candidate == oracle)

`sjis-euc-byte-parity-probe.php` runs the SAME script against the ZTS
candidate (`phpbuild-z`, candidate DSOs) and the ZTS oracle
(`/srcz/php-oracle`, system libxml2 2.15.3 + iconv):

- `SHIFT_JIS` comment/attribute/text of ぁ漢ｱ → `0x82 0x9F 0x8A 0xBF 0xB1`
  byte-identical;
- `EUC-JP` same content → `0xA4 0xA1 0xB4 0xC1 0x8E 0xB1` byte-identical;
- unmappable U+1F600 under SHIFT_JIS → decimal character reference
  `&#128512;` byte-identical (both sides, `cmp` clean);
- byte-identical under valgrind (0 errors).

The php suite's `xmlwriter_toStream_encoding_shiftjis` oracle-parity
exclusion REMAINS: the pristine phpt's `.exp` demands an empty comment
`<!---->`, which no correct libxml2 emits (the oracle writes the real
Shift_JIS bytes). Candidate and oracle emit identical bytes; the exclusion
records the broken test, not a candidate gap.

## Residual status after this phase

- R-000198 FIXED.
- R-000157 narrowed: Shift_JIS + EUC-JP (and windows-1252, earlier) now
  native; still iconv/ICU-only: UCS-4LE/BE, EBCDIC, UCS-2, ISO-8859-2..16,
  ISO-2022-JP (stateful — needs cross-flush escape state).
- 5 open residuals total (R-000157, R-000168, R-000177, R-000179, R-000199).
