# EXT-6 — ext/xmlwriter 19 → 1 (W1–W4 writer engine + W6 filename-open routing)

Closed 2026-09-03. Full suite **275 → 255**, zero regressions (comm: no new
failures across dom/simplexml/xml/xmlreader/xmlwriter/xsl). Breakdown of the 20
flips: 18 xmlwriter + 2 dom (`modern/common/namespace_sxe_interaction`,
`modern/xml/XMLDocument_fromString_02` — file-output tests that the W6 routing
fixes through the PHP streams layer), so dom is 166 → 164. xmlwriter residual = 1
(`xmlwriter_toStream_encoding_shiftjis`, W5, encoder scope → W9/R-000157).

Logs: phpbuild-c:/out/wr7.log … wr10.log (ext/xmlwriter 51-run), wr10-six.log
(six-extension 1291-run, 255 failed / 40 skipped). Oracle = 0 failed.

## RC-1 · writer engine parity (mirrors xmlwriter.c 2.15) — 18 members

The candidate's element stack is `depth`/`elem_stack`, not upstream's node
list; the fixes restore byte parity per family (byte captures `ora-*.txt` /
`can-*.txt` in THIS receipt dir were the calibration for each hunk):

- **W1 QName end tags.** `xmlTextWriterStartElementNS` now pushes the FULL
  `prefix:name` on the element stack (was the bare local name), so every
  `EndElement`/`FullEndElement` closes `</prefix:name>`. Fixed 006, 007, 011,
  012 + OO_006/007/010/011 and the prefix loss inside bug41287, bug41326,
  `xmlwriter_write_attribute_ns_basic_001`.
- **W2 empty content + END indent + attr-ns decls.**
  - `FullEndElement` closing straight from the NAME state (empty element)
    clears `doindent`, so `<empty></empty>` sits on ONE line (012/OO_011);
  - the END-tag indent is `depth - 1` (upstream `nodes - 1` — a closing tag at
    depth 1 is at column 0);
  - `xmlTextWriterWriteString` in Element state with EMPTY caller content
    closes the tag instead of erroring (bug41287's full-form empties);
  - `xmlTextWriterStartAttributeNS`/`WriteAttributeNS` queue the attribute's
    `xmlns[:prefix]="uri"` declaration (shared `queue_attr_ns_decl`, deduped
    per element: same prefix+uri no-op, same prefix+different uri → error),
    flushed when the element's start tag closes
    (`xmlwriter_write_attribute_ns_basic_001`).
- **W3 bare DTD children.** `StartDocument` leaves `state = None` (the `XMLDecl`
  state is removed); `<!ENTITY`/`<!ELEMENT`/`<!ATTLIST` at the prolog WITHOUT a
  `<!DOCTYPE` wrapper are legal via `dtd_child_begin` + the `dtd_bare` flag
  (no ` [`, no indentation, return to None on End*). Fixed 008/OO_008.
- **W4 leaf atomicity.** `StartPI`/`StartCDATA` after a NAME parent close the
  start tag with `>` and NO newline — the PI/CDATA body stays inline on the
  open-tag line (009 leaf); only `EndPI` writes the trailing newline.
- **Flush.** `xmlTextWriterFlush` does NOT close an open start tag — a flush
  mid-start-tag returns the partial buffer (`<foo`) exactly like the oracle
  (`xmlwriter_toMemory_flush_combinations`).

## RC-2 · W6 lifecycle — filename opens honor the registered default
(mirrors xmlIO.c 2.15 `xmlOutputBufferCreateFilename`) — 2 xmlwriter + 2 dom

PHP installs `php_libxml_output_buffer_create_filename` at request init through
`xmlOutputBufferCreateFilenameDefault(...)`. Upstream funnels every filename
open through that default; the candidate opened files directly with libc, so:

- `openUri("php://memory")` returned NULL (writer never created → bogus
  "Invalid or uninitialized XMLWriter" on the next call) — bug71536;
- real-file opens never created the PHP stream, so no
  `PHP_STREAM_FLAG_NO_FCLOSE` resource existed and the phpt's manual `fclose`
  hit a TypeError instead of the friendly "cannot close the provided stream"
  warning — bug79029.

Fix: `xmlNewTextWriterFilename`, the exported `xmlOutputBufferCreateFilename`,
`xmlSaveToFilename` and `htmlSaveFileFormat` all route through the new
`io::output_buffer_create_filename_routed`, which consults the per-thread
default (same cell as `xmlOutputBufferCreateFilenameDefault` and the thrDef
variant) and falls back to the builtin file open when none is registered.
Bug71536 + bug79029 PASS; dom `namespace_sxe_interaction` +
`XMLDocument_fromString_02` (dom save-to-file via
`php_new_dom_dump_node_to_file`/`php_libxml_default_dump_doc_to_file`) also
flip PASS because their `out->context` is now a real php stream.

### Exposed latent defect: xmlURIUnescapeString `len <= 0` (mirrors uri.c 2.15)

Routing the file opens through php's callback immediately exposed a latent
candidate defect: php's stream wrapper calls
`xmlURIUnescapeString(filename, 0, NULL)` and the candidate treated `len == 0`
as a literal zero-byte decode → php_stream_open_wrapper_ex received an empty
path → `ValueError: Path must not be empty` on EVERY file open (14 xmlwriter
tests regressed). Upstream uri.c: `if (len <= 0) len = strlen(str)`. Fixed;
oracle-pinned rows in the C probe (see below).

## Oracle pins / probes

- `consumers/uri-probe.c` — xmlParseURI / xmlURIUnescapeString /
  xmlURIEscapeStr rows over {abc.xml, `-`, /tmp abs, php://memory, 004.xml}.
  Oracle (system libxml2 2.15.3): unescape returns the input unchanged for
  no-%-strings; `xmlURIEscapeStr(s, ":")` escapes `/` (php:%2F%2Fmemory) —
  candidate now byte-identical on every row.
- `consumers/openuri-mem.php` — bug71536 repro (`php://memory`).
- `consumers/sjis-phpt-body.php` — W5 byte evidence: oracle emits the comment
  content as real SHIFT_JIS (`0x82 0x41` ×3 for ぁぁぁ), candidate emits raw
  UTF-8 (`0xE3 0x81 0x81` ×3). The phpt `--EXPECT--` itself carries the raw
  SJIS bytes. W5 needs a UTF-8 → SHIFT_JIS output converter → Workstream 9.

## Guards

- io/mod.rs `test_output_buffer_create_filename_routed_honors_default`
  (registered default consulted and its result returned verbatim; the public
  export funnels identically; builtin file open still runs when unregistered).
- uri/mod.rs `test_xml_unescape_string_len_zero_means_whole` (len 0 → whole
  string incl. %-decoding).
- cargo test --lib 1225 pass / 1 ignored; clippy no new warnings (4
  pre-existing); fmt clean.
