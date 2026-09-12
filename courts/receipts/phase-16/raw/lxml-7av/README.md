# lxml court 7av — incremental HTML push parser (SAX1 dispatch + persistent resume)

## Why

`test_htmlparser.py` was the single largest failing cluster (16 ids). It was not a
tree-shape problem: `htmlParseChunk` accumulated the whole document and only at
`terminate != 0` built a tree. Consumers that need events *while* they feed —
lxml's `iterparse`, `HTMLPullParser`, and parser *targets* — saw nothing until
`close()`, and `ctxt->myDoc` / `ctxt->node` were not live between chunks.

## What changed

### The parser now has one tree-building authority

`src/xml/html/mod.rs` gained an emission seam. The HTML grammar decisions
(implicit html/head/body, auto-close, void elements, raw text) live in the
engine and are expressed as `emit_start` / `emit_end` / `emit_text` /
`emit_comment` / `emit_pi` / `emit_doctype`. In whole-document mode these mutate
the tree directly; in push mode they dispatch the consumer's SAX1 callback, and
the **default** handler (`xmlSAX2StartElement` -> `xmlSAX2StartHtmlElement`,
`xmlSAX2EndElement`, `xmlSAX2Characters`, ...) routes straight back into the same
`html_create_element_node` / `html_node_pop` primitives. There is no second tree
builder and no divergence between a consumer-driven parse and a plain parse.

`xmlSAX2StartElement`/`xmlSAX2EndElement` previously re-invoked
`sax.startElement`/`sax.endElement` and would have recursed on an HTML context;
they now branch to the HTML SAX1 handlers.

### The push driver is genuinely persistent

`push_drive` is a faithful port of upstream `htmlParseTryOrFinish` plus
`htmlParseElementInternal` / `htmlParseEndTag` / `htmlParseCharData` /
`htmlParseComment` / `htmlParseDocTypeDecl`:

```
START -> XML_DECL -> MISC/PROLOG/CONTENT -> START_TAG / END_TAG -> ... -> EOF
```

`input_pos` only ever advances. A construct that is not yet complete rewinds the
cursor to the construct start and the call returns; the next chunk retries it.
Incomplete constructs are detected with persistent lookups that mirror upstream
`ctxt->checkIndex` / `ctxt->endCheckState` (`html_lookup_gt`,
`html_lookup_string`), including the "rescan only `strLen + extraLen - 1` bytes
of overlap" rule — so an unfinished tag/comment/doctype is never rescanned from
the beginning. `ctxt->nameTab` is not needed: the open-element chain is the
engine's own `current` pointer, and `emit_start` falls back to creating the
engine's node when a consumer (a target) does not materialise one, so the
grammar stays coherent.

Raw-text elements (`<script>`, `<style>`) park and resume via `data_tag`.

### Error reporting

Unexpected end tags now raise `XML_ERR_TAG_NAME_MISMATCH` through
`raise_error_streamed` with `parser_delivery`, so the diagnostic reaches lxml's
per-context `sax.serror` (and the generic channel otherwise). HTML errors clear
`wellFormed` but stay recoverable, which is what makes
`recover=False` paths raise while default paths continue.

Whole-document parses (`htmlCtxtReadMemory`) now pass the host context for
diagnostic delivery, and dispatch SAX only when the consumer actually replaced
`startElement` — the default `xmlSAX2*` handler keeps the exact direct-builder
path the PHP court has always exercised.

### The `xmlCtxtResetPush` prefix

lxml's `_htmlCtxtResetPush` hands the leading (encoding-sniffing) bytes to
`xmlCtxtResetPush` and the remainder to `htmlParseChunk`. The first push call
now adopts `[input->cur, input->end)` into the engine buffer, so both halves
reach the parser.

## Evidence

* `test_htmlparser.py`: **54 passed / 0 failed** (was 37/17).
* Full suite: **34 raw unique failing ids** (33 excluding the known-flaky
  group), down from 50 (48) — 16 fixed, **0 regressions**.
* `cargo test --lib`: 1325 passed / 0 failed / 3 ignored.
* `cargo fmt --check`: clean.
* PHP six-gate: **1250 passed / 0 failed** (rc=0).
* `failing-ids.txt` is the raw unique id set from the run; the filtered delta
  against lxml-7au (`ab6ec036`) is exactly the 16 htmlparser ids, with nothing
  added.

GitHub still carries no status checks or workflow runs; these are local and
committed results.
