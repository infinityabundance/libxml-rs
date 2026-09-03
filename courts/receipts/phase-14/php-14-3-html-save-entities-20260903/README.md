# Phase 14.3 — dom S1: html-save pseudo-"HTML" encoding parity (saveHTML `&nbsp;`)

Date: 2026-09-03
Suite movement: full six-extension suite **208 → 207 failed** with ZERO
regressions — name-level diff vs the 208 baseline: `dom005.phpt` flipped PASS,
0 new. Log: `phpbuild-c:/out/xpe-six8.log`. Receipt dir: this folder.

## Why this work matters
`DOMDocument::saveHTML()` on the doc path (`htmlDocDumpMemoryFormat`) is the
last red sub-issue of dom005: after the NOBLANKS + AS_XML-save work the oracle
re-emitted html-origin non-ASCII chars as HTML 4 named entities
(`&nbsp;`, `&eacute;`) while the candidate printed raw UTF-8. This was the
tracked "html-serializer entity-representation" residual of
php-14-3-html-save-parity-20260903. gh19612's remaining diff is the
*declared-entity attribute* `&foo;` retention — a different (xmlsave
attr-entity-ref) family.

## Root cause
Upstream `htmlDocDumpMemoryFormat` (HTMLtree.c) creates its output buffer via
`htmlFindOutputEncoder(doc->encoding)` — a NULL `doc->encoding` falls back to
the pseudo **"HTML"** encoding, whose output converter `UTF8ToHtmlWrapper`
(encoding.c) → `htmlUTF8ToHtml` (HTMLparser.c) turns every non-ASCII UTF-8
character into an HTML 4 named entity when one exists (`&nbsp;`, `&eacute;`)
or a decimal reference (`&#9731;` for U+2603). The same fallback applies to
`htmlSaveFileFormat(..., encoding)` when the *caller's* encoding string is
NULL (php `DOMDocument::saveHTMLFile` passes `htmlGetMetaEncoding`, NULL for
meta-less docs). The candidate's dumps wrote the serializer's raw UTF-8
bytes — the encoder layer did not exist for these convenience dumps.

php's modern `Dom\HTMLDocument` is unaffected: it always sets
`doc->encoding` (default `"UTF-8"`) and serializes through the lexbor-based
html5 serializer, never through these C dumps — so the correct gate is
exactly "no declared encoding in force", matching upstream's
`htmlFindOutputEncoder` NULL/"HTML" resolution.

## Fix (src/abi/exports_html.rs)
- `html_pseudo_encoding_in_force(encoding)`: NULL or case-insensitive "HTML"
  (upstream `htmlFindOutputEncoder` fallback). Declared real encodings stay
  out of scope (output converters are Workstream 9 / R-000157; they keep the
  existing raw UTF-8 pass-through, documented).
- `html_buf_append_html_ascii(out, content, len)`: byte-exact port of
  upstream `htmlUTF8ToHtml` over a whole dump — ASCII runs copied in one
  `buf_add`, non-ASCII sequences decoded (no validation, like upstream) and
  looked up in the existing `HTML40_ENTITIES` value table (`&name;`) or
  written as a decimal ref (`&#N;` via a stack digit buffer); a truncated
  trailing sequence is dropped (upstream stream converter semantics).
- `html_buf_apply_pseudo_encoding(buf, encoding)`: swaps the serialized
  buffer for a converted copy when the pseudo encoding is in force.
- Wired into `htmlDocDumpMemoryFormat` (keyed on `(*cur).encoding`) and
  `htmlSaveFileFormat` (keyed on the caller's `encoding` argument).
  `htmlDocDump(FILE*)`/`htmlNodeDump*`/obuf paths stay raw — upstream creates
  those buffers with a NULL encoder, so raw is the oracle behavior there.

## Evidence
- `/out/xpe-six8.log`: 1291 tests / 207 failed / 40 skipped; dom005 PASS;
  0 new names vs `/out/xpe-six7.log` (the 208 baseline).
- dom005 `.diff` gone: `--- save as HTML` section now ends
  `html files with undeclared entities&nbsp;` exactly like the oracle.
- Guards: `exports_html.rs` unit tests
  `test_pseudo_html_transcode_semantics` (`&nbsp;`/`&eacute;` named, U+2603
  decimal `&#9731;`, ASCII markup untouched, truncated tail dropped) and
  `test_html_doc_dump_memory_pseudo_encoding` (NULL doc->encoding → named
  refs; declared UTF-8 → raw bytes).

## Validation
- `cargo test --lib`: 1239 passed / 1 ignored (1238 → 1239 with the 2 new
  guards).
- `cargo clippy --lib`: no new warnings (pre-existing set untouched).
- `cargo fmt --check`: clean.
- Six-extension php suite at 207, zero regressions (name-level diff).

## Residual (tracked)
1. The html parser's parse-side entity table (`src/xml/html/mod.rs`
   `HTML_ENTITIES`) is a curated subset that omits the Latin-1 accented run
   (`eacute` & co.) — `&eacute;` stays literal text where the oracle html
   tokenizer resolves it to U+00E9. Separate html-tokenizer entity-resolution
   rule (affects any loadHTML + save round trip of accented named refs).
2. gh19612: declared-entity attribute `x="&foo;"` is written as
   `x="&amp;foo;"` — xmlsave must emit the entity-ref child as `&foo;`
   (KEY-6/dom E1 family).

## Commit
`(see git log HEAD)`
