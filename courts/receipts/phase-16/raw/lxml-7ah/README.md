# lxml court 7ah — external-subset DTD parsing (xmlParseDTD)

`candidate_sha=2db724a6` · oracle libxml2 2.15.3 · lxml 6.1.2 · 2007 tests

## Result

```
previous (0e6a5c34 / lxml-7ag)   80 unique failing ids
now (2db724a6 / lxml-7ah)        79 unique failing ids
fixed                             1
new                               0
PHP six-gate                     1250 passed / 0 failed
cargo test --lib                 1325 passed / 0 failed / 3 ignored
```

`test_dtd.py` went from 8 failed / 30 passed to 7 failed / 31 passed.

## Symptom

`etree.DTD(path)` always raised `DTDParseError`, and a DTD obtained without an
error had no declarations at all (`dtd.elements()` was empty). Three
independent defects, all divergences from upstream.

## 1 — `xmlParserCheckEOF` was a fatal error

`pi_parse_external_subset` raised `XML_ERR_EXT_SUBSET_NOT_FINISHED` whenever
the top-level input was exhausted:

```rust
if (*input).cur >= (*input).end {
    if (*ctxt).inputNr <= old_input_nr {
        pi_fatal_err(ctxt, XML_ERR_EXT_SUBSET_NOT_FINISHED);   // always
        break;
    }
```

Upstream calls `xmlParserCheckEOF(ctxt, XML_ERR_EXT_SUBSET_NOT_FINISHED)`,
which raises **only when bytes remain unconsumed**:

```c
void xmlParserCheckEOF(xmlParserCtxt *ctxt, xmlParserErrors code) {
    if (ctxt->errNo != XML_ERR_OK) return;
    if (in->cur < in->end) { xmlFatalErr(ctxt, code, NULL); return; }
    /* encoder flush / truncated multi-byte check */
}
```

A completely consumed external subset is a valid EOF. Added `pi_check_eof`
mirroring the contract and used it at that call site.

## 2 — the returned DTD was not detached

`xmlParseDTD` returned `myDoc->intSubset` (or `extSubset`) and then freed the
parser context — which frees `myDoc` and its subsets — leaving the returned DTD
dangling. Upstream `xmlCtxtParseDtd` detaches first:

```c
    ret = ctxt->myDoc->extSubset;
    ctxt->myDoc->extSubset = NULL;
    ret->doc = NULL;
    for (tmp = ret->children; tmp; tmp = tmp->next) tmp->doc = NULL;
```

The candidate now does the same. (Fixing defect 1 made the success path
reachable, which is how the latent double free surfaced.)

## 3 — declarations were dispatched to a no-op SAX handler and discarded

`xmlParseDTD` did not create the document's **external** subset up front, and
the `pi_` declaration parsers only dispatched SAX callbacks. The candidate's
default SAX2 `elementDecl` / `attributeDecl` / `entityDecl` handlers are
no-ops, so nothing recorded the declarations.

Upstream's SAX2 handlers are the recorders (`xmlAddElementDecl`,
`xmlAddAttributeDecl`, `xmlAddDocEntity` / `xmlAddDtdEntity`). The parsed
declarations are now added directly as well, against the subset the SAX2
handler would have used (`ctxt->inSubset == 2` → `extSubset`), mirroring
`state.rs::parse_element_decl`, which already does both. `xmlParseDTD` now
creates the document and its external subset from the start, like upstream
`xmlCtxtParseDtd`.

Two entity-value bugs were fixed alongside:

- `pi_parse_entity_value` took the `orig` pointer **before** consuming the
  opening quote, so `orig` was `"&#42` instead of `&#42;`.
- the stored replacement text was the raw literal; upstream stores the
  expanded value. It is now decoded with `string_decode_entities`, so
  `<!ENTITY c "&#42;">` yields `orig = "&#42;"`, `content = "*"`.

## Verification

Oracle differential via a C probe (`xmlParseDTD(NULL, "/tmp/d3.dtd")`) — the
candidate's returned DTD child chain now matches the oracle exactly:

```
oracle    child type=15 name=a ; type=16 name=default atype=9 def=1 default=valueA elem=a ; type=15 name=b
candidate child type=15 name=a ; type=16 name=default atype=9 def=1 default=valueA elem=a ; type=15 name=b
```

and the lxml-level probe matches for both plain paths and `file://` URLs:
`elements()`, content models, `attributes()`, and (for the first time)
`entities()`.

## Remaining test_dtd failures

- `test_dtd_broken` — `<!ELEMENT b HONKEY>` must be a `DTDParseError`.
- `test_dtd_file`, `test_dtd_file_pathlike` — `xmlValidateDtd` does not
  substitute the passed DTD for the document's own subsets during the element
  walk (upstream `doc->extSubset = dtd; doc->intSubset = NULL;` then restore),
  so it reports "No declaration for element a". A first attempt was reverted:
  it introduced a segfault in the same suite, so it needs its own root-cause
  analysis rather than a mechanical port.
- `test_dtd_invalid`, `test_dtd_invalid_duplicate_id`, `test_dtd_api_internal`,
  `test_internal_dtds` — separate validation/iteration semantics.
