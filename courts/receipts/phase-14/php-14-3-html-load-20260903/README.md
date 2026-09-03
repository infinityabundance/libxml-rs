# Phase 14.3 — dom S1: html-load family (XML_DOC_HTML, AS_XML save, htmlCreateFileParserCtxt)

Date: 2026-09-03
Suite movement: **232 failed → 208 failed**, zero regressions (name-level
diff vs the 232 baseline: 0 new; 6 phpt flipped). Log:
`phpbuild-c:/out/xpe-six5.log`.

## Members closed
- `DOMDocument_loadHTMLfile.phpt`, `DOMDocument_loadHTMLfile_variation2.phpt`
  — previously produced NOTHING (the load failed silently).
- `bug79451`, `gh15670`, `gh16535`, `gh17397` — saveXML/saveHtml of
  html-loaded documents now carries the `<?xml version="1.0"
  standalone="yes"?>` declaration and correct content.

## Root causes (three engine gaps in the loadHTML family)
1. **html-parsed doc properties** (`src/xml/html/mod.rs html_parse_buffer`):
   the html-parsed document carried `properties = XML_DOC_WELLFORMED` only.
   Upstream `xmlSAX2StartDocument` for HTML parsers sets
   `ctxt->myDoc->properties = XML_DOC_HTML` (SAX2.c 2.15) on the
   htmlNewDocNoDtD document (which also defaults `standalone = 1`). PHP's
   serializer/save logic keys html-ness on that flag.
2. **AS_XML dispatch of HTML documents** (`src/xml/tree/mod.rs
   node_dump_internal`): the `XML_HTML_DOCUMENT_NODE` arm always used the
   HTML serializer. Upstream `xmlSaveDocInternal` (xmlsave.c 2.15) only
   takes the HTML branch when `type == HTML && !(AS_XML) && !(XHTML)` or
   `AS_HTML` — under `XML_SAVE_AS_XML` (php `DOMDocument::saveXML`) the
   document is dumped by the XML serializer, emitting the XML declaration
   with `doc->standalone`. The arm now keys on `DumpState.as_html`.
3. **`htmlCreateFileParserCtxt` was a NULL stub** (`exports_xml2.rs`) —
   `DOMDocument::loadHTMLFile` obtained NULL and returned FALSE before
   parsing anything. Implemented in `exports_html.rs` (next to the memory
   variant): the file bytes are read through the registered
   `xmlParserInputBufferCreateFilenameDefault` (php streams loader; built-in
   read otherwise), copied into the html-ctxt host input, and
   `htmlParseDocument` then runs the normal html parse.

## Evidence
- `/out/xpe-six5.log`: 1291 tests / **208 failed** / 40 skipped; zero new.
- C probes (kept; candidate == oracle):
  - `consumers/htmlflags-probe.c` — htmlReadMemory doc: standalone=1,
    properties=0x80 (XML_DOC_HTML) on both engines.
  - `consumers/htmlsave-probe.c` — xmlSaveToIO(XML_SAVE_AS_XML)+xmlSaveDoc
    of an html doc: both emit `<?xml version="1.0" standalone="yes"?>` +
    doctype + content (byte-identical, same return status 182).
  - `consumers/nbsp-save-probe.c` — shows the still-open residual: the oracle
    XML-save escapes an html-origin 0xA0 as `&#xA0;`, the candidate writes it
    raw (tracked residual; dom005 keeps failing on this + the head-whitespace
    policy).
- Guards:
  - `src/xml/save.rs test_save_html_doc_as_xml_includes_declaration`
    (AS_XML save of an html doc starts with the standalone declaration).
  - `src/xml/html/mod.rs test_parsed_html_doc_flags` (XML_DOC_HTML bit set +
    standalone=1 after html parse).

## Validation
- `cargo test --lib`: 1236 passed / 1 ignored (1237 total, +2 guards).
- `cargo clippy --lib`: no new warnings (4 pre-existing, untouched).
- `cargo fmt --check`: clean.
- Six-extension php suite at 208 (this log), zero regressions.

## Residuals in this family (tracked in CURRENT-STATE.md / plan-dom)
- html-parser <head>-region whitespace policy (whitespace-only text children
  of head/head-body boundary must not exist; body whitespace kept).
- html-origin character escaping under the XML serializer (`&#xA0;` vs raw
  byte) and the HTML serializer (`&nbsp;`).
  `dom005` now differs ONLY in those two details; `gh19612` is the
  declared-entity attribute retention family (`x="&foo;"` must not become
  `&amp;foo;`).

## Commit
`0b8917e7 (pushed to origin/main)`
