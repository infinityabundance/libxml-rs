# php-14.3 xmlreader NR/NX/AT/EV closure (2026-09-03)

Full suite **201 → 186 failed**, ZERO regressions (name-level diff vs the
201 baseline `phpbuild-c:/out/xpe-six9.log`: 0 new). Log:
`phpbuild-c:/out/xpe-six10.log`. xmlreader 30 → 15; dom 118 → 117
(`bug47530` — empty attributes — flipped as a cross-family win).

Flipped to PASS (15): xmlreader `001, 002, 003, 003-mb, 006, 009, 011`,
`cache_slot, expand, gh16292, readString_basic, static, var_dump,
fromUri_custom_constructor` + dom `bug47530`.

## Engine root causes closed

### NR — memory/IO reader constructors emitted ZERO events (~13 members)
php `XMLReader::XML()` / `fromString()` call
`xmlParserInputBufferCreateMem(source, ...)` + `xmlNewTextReader` +
`xmlTextReaderSetup(reader, NULL, ...)`. Two candidate bugs:
- `xmlParserInputBufferCreateMem` allocated an empty struct and THREW THE
  BYTES AWAY (upstream copies them into the buffer's internal xmlBuf and
  leaves readcallback NULL). The candidate now stashes a copy in an
  input-buffer content side table (helpers.rs) and `reader_from_input` /
  `xmlTextReaderSetup` consume it when no read callback is set (an I/O
  buffer still slurps through its callback).
- `xmlTextReaderSetup(reader, NULL, ...)` freed the parser context
  `xmlNewTextReader` had just wired and never rebuilt one. Upstream
  promotes the input the reader was created with; the candidate now keeps
  the context on a NULL input (and resets gracefully when the reader
  already parsed and its context was consumed).
- `xmlReaderForIO` required BOTH callbacks (upstream allows a NULL close) —
  no-op close fallback added.
C probe `consumers/reader-nr-probe.c` (new): all four constructor shapes
(file/memory/IO/NewTextReader+Setup) now emit oracle-identical event walks.

### NX — END_ELEMENT of explicitly-closed empty elements was dropped
`walk_tree` suppressed the END event for every childless element, but the
oracle emits END_ELEMENT for `<a></a>` and not for `<a/>`. The tree alone
cannot tell the forms apart, so during reader parses
(`ctxt->parseMode = XML_PARSE_READER`, set in parse_and_build_events —
already the marker upstream xmlreader + the candidate validation add_ref
use) the parser records self-closed element nodes (state.rs parse_element,
helpers.rs side table keyed by DOC — thread-safe across parallel parses);
`walk_tree` consumes the markers and emits the END event for every element
that was not self-closed.

### AT — attribute value accessors vs the attribute cursor
Upstream xmlreader.c `xmlTextReaderGetAttribute/GetAttributeNo/
GetAttributeNs` return NULL while the attribute cursor is active
(`curnode != NULL`); they read from the ELEMENT position only. The
candidate tracked the cursor with `cur_attribute` but the accessors ignored
it — they now return NULL when `cur_attribute >= 0`.

### EV — empty attribute value `""` collapsed to nothing
`<foo bar=""/>` lost the attribute entirely:
- `vec_to_cstr_null` returns NULL for an empty value, so the SAX2 atts
  array carried a NULL value pointer and the tree builder (default.rs)
  skipped the attribute; the value pointer is now a real `""`
  (`c""`), matching xmlParseStartTag2's in-place empty value.
- `parser_set_prop` rejected `value_len <= 0` (upstream creates the
  attribute; xmlGetProp reports a childless attribute as "").
- reader attribute-value emission (cursor value, GetAttribute*) returns ""
  for a childless attribute.
Also flipped dom `bug47530` (fragment import with empty attributes).

## Probes kept (consumers/, candidate == oracle)
- `reader-nr-probe.c` — constructor-shape event walks (file/memory/IO/
  NewTextReader+Setup).

## Tracked residuals (next atoms, xmlreader 15)
- `003-get-errors` / `003-move-errors` / `015-get-errors` — php invalid-
  argument ValueError surface (attribute move/get arg handling).
- `010` / `next_basic` — next()/read() cursor semantics after namespace
  elements.
- `007` / `008` / `013` — reader schema (RelaxNG/XSD) + DTD attach — VA/VD
  families; overlap ext/dom schemaValidate*/validate_external_dtd
  (SP-14.3.4).
- `bug42139` (option constants via XML() — DOC_TYPE node name empty),
  `bug64230` (error suppression), `fromStream_*` /
  `fromString_custom_constructor` (baseURI: oracle reports "" for
  memory-string readers where the candidate reports the php CWD — php
  passes xmlCanonicPath(getcwd()) as the reader URL), `gh19098` (expand /
  php_libxml_node_free lifetime).

Validation: cargo test --lib 1239 pass (self-closed markers keyed by doc —
parallel-test safe), clippy at the 4 pre-existing warnings, fmt clean.
Full six-gate `phpbuild-c:/out/xpe-six10.log`: 1291 / **186 failed** /
40 skipped.
