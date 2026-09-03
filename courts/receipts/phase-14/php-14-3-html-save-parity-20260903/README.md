# Phase 14.3 — dom S1: html-parser NOBLANKS + xmlsave non-ASCII escaping parity

Date: 2026-09-03
Suite movement: stays **208 failed** with zero regressions (name-level diff
vs the 208 baseline: 0 new, 0 fixed — both members this work targets,
dom005 and gh19612, are each one sub-issue from green). Log:
`phpbuild-c:/out/xpe-six7.log`.

## Why this work matters
dom005's xml-save section now differs from the oracle ONLY in the html-save
`&nbsp;` representation, and the C-level save/parse behavior for html docs
and no-encoder saves is byte-identical to the oracle (esc-probe,
nbsp-save-probe, html-tree-probe, htmlsave2-probe, htmldump-probe). These
two engine rules (parse options + save escaping) are prerequisites for the
remaining dom005/gh19612 members and every later xmlsave-parity test.

## Root causes & fixes
1. **XML_PARSE_NOBLANKS ignored by the html parser**
   (`src/xml/html/mod.rs` + `src/abi/exports_html.rs`): whitespace-only text
   runs were always built as text nodes, so `loadHTMLFile(…,
   LIBXML_NOBLANKS)` serializations kept the `<head>`-region newline text
   nodes the oracle drops (dom005's xml-save body). Fix: the parse options
   are threaded from the host `htmlParserCtxt` (stored by `htmlCtxtUseOptions`
   and friends) into the internal `HtmlParserCtxt.options`, and
   `html_parse_buffer` drops whitespace-only text nodes after the build
   (`drop_blank_text_nodes`) — mirroring xmlSAX2Characters' ignorable-
   whitespace behavior. Tree equality verified with `html-tree-probe.c`.

2. **No-encoder xmlsave escapes non-ASCII** (`src/xml/tree/mod.rs`): upstream
   `xmlSaveWriteText` (xmlsave.c 2.15) sets `XML_ESCAPE_NON_ASCII` when
   `ctxt->encoding == NULL`, and `xmlSerializeText` then writes every
   non-ASCII character as an uppercase-hex reference (`&#xA0;`, `&#xE9;`),
   with the U+FFFD fallback for invalid sequences. The candidate passed raw
   UTF-8 bytes through. `serialize_text_flags`/`serialize_attr_value_flags`
   now decode UTF-8 and write `&#x%X;`; the decision is computed by
   `save_escapes_non_ascii(save_encoding, doc)` and gated to the explicit
   save entry (`DumpState.explicit_save`, set by
   `serialize_node_opts_enc_full`), non-HTML-method output, and documents
   without a real (non-native) encoding. Bare node dumps (the xslt per-child
   output path used by `xsltSaveResultToString`) keep the raw pass-through —
   xslt001's iso-8859-1 output is protected (a mid-session regression on
   xslt001 was caused by applying the escape in `serialize_node_opts_xhtml`,
   which the bare chain shares; the flag was moved to the full-save entry).

## Evidence
- `/out/xpe-six7.log`: 1291 tests / 208 failed / 40 skipped; 0 new.
- `html-tree-probe.c`: candidate NOBLANKS tree == oracle (head-region
  whitespace-only text dropped; body mixed text kept).
- `esc-probe.c` / `nbsp-save-probe.c`: `café …` → `caf&#xE9; …`,
  `&#xA0;` byte-identical to the oracle.
- Guards: `save.rs::test_save_no_encoding_escapes_non_ascii`;
  `html/mod.rs::test_parsed_html_doc_flags`.

## Validation
- `cargo test --lib`: 1237 passed / 1 ignored (1238 total).
- `cargo clippy --lib`: no new warnings (4 pre-existing, untouched).
- `cargo fmt --check`: clean.
- Six-extension php suite at 208, zero regressions.

## Residual (tracked)
html-save (`saveHTML`/`htmlDocDumpMemoryFormat` path) prints html-origin
non-ASCII characters RAW where the oracle re-emits the named entity
(`&nbsp;`, `&eacute;`) — dom005's saveHTML section. That is a separate
html-serializer entity-representation rule (the oracle tree keeps html
entities as entity-ref nodes); no engine change made here regresses it.

## Commit
`<filled at commit time>`
