# Phase 14.29 — encoding backend completion (R-000157 FIXED): the full enumerated iconv/ICU set is native

Gates: NTS + ZTS six-extension file-list = **1290 / 1250 passed / 40 skipped /
0 failed** each (`/out/nts-cand-six-1429.log`, `/out/zts-cand-six-1429.log`).
`cargo test --lib` **1254 pass** (7 new codec tests); cargo fmt clean;
valgrind 0 errors on the new input-decode path.

## What landed

### 1. Handlers for every remaining enumerated encoding (`src/xml/encoding/mod.rs`)

- **ISO-8859-2..11 / 13..16**: encoding_rs-backed single-byte converters
  (`ISO_8859_2`…`ISO_8859_16`; ISO-8859-11 == TIS-620 == the WHATWG
  `windows-874` index; ISO-8859-9 via the WHATWG `windows-1254` index — the
  WHATWG label for it). Registered under the canonical names + `windows-874`.
- **ISO-2022-JP**: encoding_rs (escape-state handled inside each conversion
  call). Whole-document single-flush output is oracle-identical; multi-flush
  chunked output resets escape state per call (bounded remainder, see below).
- **UCS-2**: native fixed-width codec. glibc iconv "UCS-2" uses the HOST
  byte order — little-endian on the executed x86-64 oracle — so the codec is
  LE; astral code points are unmappable (the -2 input-error convention →
  decimal charref), surrogates error on input.
- **UCS-4LE / UCS-4BE**: native 4-byte-unit codecs.
- **EBCDIC (IBM037)**: a full 256-entry table derived from the oracle
  container's glibc iconv (`IBM037`), a bijection onto U+0000..U+00FF with a
  const-fn reverse lookup; registered as `IBM037`/`EBCDIC-US`/`EBCDIC`.
- `char_enc_out`'s scratch grew to 5x so the 4-bytes-per-ASCII-char UCS-4
  expansion can never exhaust it mid-buffer.

### 2. Parser INPUT side (`src/xml/parser/input.rs`)

The whole-buffer declared-encoding converter (previously a hand whitelist of
ISO-8859-1/US-ASCII) now dispatches ANY registry-served encoding named in a
BOM-less XML declaration (aliases canonicalized like
`xmlFindCharEncodingHandler_owned`), and `detect_bom_and_encoding` sniffs
upstream `xmlDetectCharEncoding`'s first-4-byte patterns for the
non-ASCII-compatible family before falling back to the UTF-8 declaration
scan: UCS-4LE (`3C 00 00 00`), UCS-4BE (`00 00 00 3C`), EBCDIC
(`4C 6F A7 94`) and BOM-less UTF-16LE/BE (`3C 00 3F 00` / `00 3C 00 3F`).
Incremental `push_bytes` tails convert per-chunk in the source encoding.

This also fixed pre-existing input gaps for encodings that were already
native on the output side (windows-1252 and Shift_JIS/EUC-JP *declared
files* now load; before, even ISO-8859-1 was the only declared non-UTF-8
file encoding that converted).

## Byte-parity evidence (candidate == oracle, `cmp`-clean)

- **Output** (`enc-remainder-probe.php`: XMLWriter comment/text under every
  enumerated encoding incl. the full decl, 1897 bytes): identical for
  UCS-4LE, UCS-4BE, UCS-2, EBCDIC-US, IBM037, ISO-8859-2/3/4/5/6/7/8/9/10/
  11/13/14/15/16 and ISO-2022-JP (the oracle's ISO-2022-JP escape dance
  `ESC $ B … ESC ( B` reproduced exactly).
- **Input** (`enc-input-probe.php`: DOMDocument::load of files physically
  encoded in UCS-4LE/UCS-4BE/UCS-2/EBCDIC-US/ISO-8859-2/ISO-8859-7/
  ISO-2022-JP plus the ISO-8859-1 control): identical.

## Bounded remainder (documented)

- Encoding names beyond the R-000157 enumeration that glibc iconv also
  serves (KOI8-R/U, IBM866, macintosh, GBK, Big5, EUC-KR, …) are not yet
  registered.
- ISO-2022-JP chunked OUTPUT (a single output buffer flushed more than once,
  i.e. documents large enough to cross the 256-byte flush threshold) resets
  the escape state per flush; whole-document output is oracle-identical.
