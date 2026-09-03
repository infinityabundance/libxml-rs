# php-14.5 validation parser-context parity (schemaValidate*/relaxNGValidate* error paths)

Date: 2026-09-03
Full six-extension gate: **110 → 102 failed** (1291 tests / 102 failed / 40
skipped, dom 73 | xsl 16 | xmlreader 16 | simplexml 1 | xmlwriter 1), **ZERO
regressions** (name-level diff vs the 107 baseline `phpbuild-c:/out/xpe-six15.log`:
0 new). Logs: `phpbuild-c:/out/xpe-six15.log` (107) and
`phpbuild-c:/out/xpe-six16.log` (102).

## Root cause 1 — validity diagnostics lacked the trailing newline PHP's handler requires (~4 tests)
php_libxml_internal_error_handler_ex (ext/libxml/libxml.c) only RAISES a message
when it strips a trailing `\n`; messages without one are accumulated silently.
The schema/relaxNG validity dispatchers
(`exports_schema::dispatch_valid_errors`, `exports_relaxng::dispatch_relaxng_valid_errors`)
and the schema parser dispatcher (`dispatch_parser_error`) now append `\n` when
missing (upstream messages always end with one).
Flipped: `DOMDocument_schemaValidate_error2`, `schemaValidateSource_error2`
("Element 'books': No matching global declaration available for the validation
root." — also added the missing-global diagnostic in `xsd_validate_doc`),
`relaxNGValidate_error1`, `relaxNGValidateSource_error1` ("Did not expect element
pear there").

## Root cause 2 — schema/relaxNG parser contexts did not reproduce upstream's parse-stage flow (~5 tests)
php registers parser error callbacks AFTER the context constructor, so a parse
that happens eagerly in the constructor can never reach them; upstream defers
all reporting to xmlSchemaParse / xmlRelaxNGParse. New behavior:
- `xmlSchemaNewParserCtxt(url)` opens the schema document through the standard
  input machinery (xmlParseFile) — an unreadable resource now raises upstream's
  "I/O warning : failed to load external entity ..." and well-formedness
  diagnostics carry the REAL resource name (was the hard-coded "schema.xsd").
  The failure stage is remembered (Resource vs Document) and reported by
  `xmlSchemaParse` through the registered parser handlers:
  "Failed to locate the main schema resource at '%s'." / "Failed to parse the
  XML resource '%s'." ('in_memory_buffer' for memory contexts), then NULL.
- `xmlSchemaNewMemParserCtxt` parses with NO resource name so diagnostics keep
  the upstream "Entity: line N: parser error : ..." shape.
- `xmlRelaxNGParse` reports "xmlRelaxNGParse: could not load %s" (file contexts
  whose resource was unreadable) and aborts with NULL on a fatal grammar error.
- `<element>` patterns without a content model are a FATAL parse error
  (`RelaxNgSchema::fatal`): "xmlRelaxNGParseElement: element has no content"
  (was silently accepted as an empty-content pattern, so the instance was
  validated instead).
Flipped: `DOMDocument_schemaValidate_error1` (not-a-schema file),
`schemaValidate_error5` (missing file), `schemaValidateSource_error1`
(not-a-schema string), `relaxNGValidate_error2` (missing rng file),
`relaxNGValidateSource_error2` (rng element without content).

## Guards
- cargo test --lib: 1240 passed + new `test_xml_schema_validate_root_without_global_decl`
  (root element matching no global declaration fails with the exact upstream
  diagnostic); clippy at the 4 pre-existing warnings; fmt clean.
- Targeted php re-runs (all 9 green): the five parse-side tests above + the
  four previously-fixed validity tests (no regression).

## Helpers added
- `consumers/cand-six-gate.sh` — full six-extension gate with an arbitrary log
  name on the phpC-volume container.
- `consumers/cand-phpt.sh` — targeted phpt runner printing PASS/FAIL + diffs.
- `consumers/php-court-failnames.py` — log FAIL-set extractor/differ (handles
  the pty `TEST n/N [path]\r<STATUS>` record shape).

## Next residuals (biggest clusters)
dom 68: validation leftovers (schemaValidate/relaxNGValidate addAttrs default-attr
creation, validate external-DTD family, xmlreader 008/013-style reader-validate
flows), XPath namespace-axis/DOMNameSpaceNode family (incl the
xpath_domnamespacenode_advanced segfault), reader cursor/props family;
xsl 16; xmlreader 16.
