# lxml court 7ax — `xmlSchemaSAXPlug` interception + schema diagnostic parity

## Root causes fixed

### 1. `xmlSchemaSAXPlug` was a pass-through stub

lxml's parse-time schema validation (`xmlschema.pxi`
`_ParserSchemaValidationContext.connect`) installs the validator by calling

```
xmlSchemaNewValidCtxt(schema)
xmlSchemaSetValidStructuredErrors(vctxt, _receiveError, error_log)
xmlSchemaSAXPlug(vctxt, &c_ctxt.sax, &c_ctxt.userData)
```

and reads the outcome back through `xmlSchemaIsValid(vctxt)` in
`_handleParseResult`. The old stub recorded nothing and left `*sax` untouched,
so `xmlSchemaIsValid` always reported valid and an invalid instance parsed
successfully — `etree.fromstring("<a><c></c></a>", parser)` never raised.

The plug now reproduces upstream's `struct _xmlSchemaSAXPlug` design
(`xmlschemas.c`): it keeps the caller's original table/user data, installs a
forwarding table whose callbacks deliver each event to the caller's original
handler, and rewrites `*user_data` to the plug so the split callbacks can
recover it. Every callback is wrapped (not only the schema-relevant ones) because
`*user_data` changes: a verbatim-copied callback would otherwise receive the
plug instead of the caller's own data.

Because this engine validates a materialized tree rather than streaming a
vstate, the schema engine is driven over `ctxt->myDoc` from the `endDocument`
split — the first point at which the whole tree exists and, crucially, before
lxml reads `isvalid()`. `ctxt->myDoc` is NULLed by lxml in `_handleParseResult`
*before* `isvalid()` runs, so this hook is the only point that can supply the
tree. The parser context is recovered without a second API surface: `sax` is
field 0 of `_xmlParserCtxt`, so the `&ctxt->sax` address the caller passes *is*
the context address. `xmlSchemaSAXUnplug` restores the original table and runs
any pending validation (upstream `xmlSchemaPostRun`).

Notes:
* Validation is skipped when the parse itself was not well-formed; the parser
  reports that failure and running the schema engine over a partial tree would
  only add noise.
* `xmlSchemaValidateDoc` already honours `XML_SCHEMA_VAL_VC_I_CREATE`, so
  default/fixed attribute injection stays idempotent under lxml's post-parse
  `inject_default_attributes` call (`inject_default_attribute` skips an already
  present attribute).
* With `*sax == NULL` there is nothing to forward to; the historical
  pass-through shape is retained for that (unused by real consumers) case.

### 2. Schema datatype diagnostics did not match upstream

The element/attribute value failures were emitted as
`Element 'a' has invalid value 'no int' for type 'Integer'` — a Rust
`{:?}` of the internal enum. Upstream (`xmlschemas.c`
`xmlSchemaFormatNodeForError` + `xmlSchemaSimpleTypeErr`) produces

```
Element 'a': 'no int' is not a valid value of the atomic type 'xs:integer'.
Element 'a', attribute 'x': 'v' is not a valid value of the atomic type 'xs:integer'.
```

All five XSD datatype-error sites now use the existing
`datatype_kind_qname` helper and the upstream phrasing. This is what
`doc/validation.txt:97` asserts, and it removes the internal enum name from
every other XSD value diagnostic.

## Also in this commit (working-tree diagnostic parity)

This commit additionally lands already-verified diagnostics work that was
uncommitted in the tree. None of it changed the id count on its own, but it is
what makes `doc/validation.txt` fully green alongside the SAX-plug fix:

* `src/xml/validation/mod.rs`: `vctxt_error_node_code` — the DTD
  EMPTY-content failure now emits upstream's
  `Element b was declared EMPTY this one has content` with code `528`
  (`XML_DTD_NOT_EMPTY`) and the offending node's line/file.
* `src/xml/relaxng/mod.rs` + `src/abi/exports_relaxng.rs`: RELAX NG errors
  carry their `_xmlError.code` and `.line` (per-error metadata tracked through
  `errors.truncate`).
* `src/abi/exports_schema.rs` (`dispatch_valid_errors`) and
  `src/abi/exports_schema.rs` schema dispatch: `_xmlError.line` is taken from
  the offending node instead of being zeroed.
* `src/xml/parser/state.rs`: entity declarations store the raw literal as
  `orig` and the character-reference-decoded text as `content` (upstream
  `xmlParseEntityValue` -> `xmlStringLenDecodeEntities` with
  `XML_SUBSTITUTE_REF`; general entity references stay verbatim).

## Evidence

* Full suite: **25 raw unique failing ids**, down from 29 — **4 fixed,
  0 regressions**:
  * `doc/validation.txt` (91/91 doctest examples)
  * `test_xmlschema_parse`
  * `test_xmlschema_stringio`
  * `test_xmlschema_iterparse_fail`
* `cargo test --lib`: 1325 passed / 0 failed / 3 ignored.
* `cargo fmt --check`: clean.
* `failing-ids.txt` is the raw unique id set; the filtered delta against the
  pre-slice baseline (lxml-7aw) is exactly the four ids above.

GitHub still carries no status checks or workflow runs; these are local and
committed results.
