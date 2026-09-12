# lxml court 7ak — XSLT compile-error propagation + output encoding

`candidate_sha=d809cdc1` · oracle libxml2 2.15.3 / libxslt 1.1.45 · lxml 6.1.2 · 2007 tests

## Result

```
previous (5b99dbc8 / lxml-7ai)   72 unique failing ids
now (d809cdc1 / lxml-7ak)        64 unique failing ids
fixed                             8
new                               0
PHP six-gate                     1250 passed / 0 failed
cargo test --lib                 1325 passed / 0 failed / 3 ignored
```

## 1 — xsltSaveResultToString ignored the xsl:output encoding

`bytes(res)` / `str(res)` for a stylesheet with `<xsl:output encoding="UTF-16"/>`
returned UTF-8 (no BOM), so lxml's `_xslt_setup` helper failed to decode:

```
oracle    108 bytes, starts ff fe 3c 00 ..., decodes as UTF-16
candidate  55 bytes, starts 3c 3f 78 6d ...  (UTF-8), decode fails
```

The function carried an explicit residual: only UTF-8 and ISO-8859-1 were
converted. Upstream `xsltSaveResultToString` (xsltutils.c) serializes into an
`xmlOutputBuffer` whose encoder is `style->encoding`, so the returned bytes are
in that encoding including the BOM.

The serialized UTF-8 is now routed through `convert_bytes_to_encoding()`, which
builds the same output-buffer/encoder pair used by the file and buffer save
paths (`output_buffer_create_buffer` + `output_buffer_write` +
`output_buffer_close`). The candidate output is now byte-identical to the
oracle for the UTF-16 case, and `str(res)` / `res.write(f, encoding=...)`
agree.

## 2 — stylesheet compile errors did not propagate

`<xsl:foo/>` or a nested `<xsl:stylesheet/>` at the top level compiled silently.
Upstream `xsltParseStylesheetTop` (xslt.c) raises

```
xsltParseStylesheetTop: unknown foo element
```

(increments `style->errors`), and `xsltParseStylesheetUser` then detaches
`style->doc` and returns `-1`, so `xsltParseStylesheetDoc` returns NULL and lxml
raises `XSLTParseError`. Oracle:

```
compilation error, element 'foo'          (context line)
xsltParseStylesheetTop: unknown foo element   (last error)
```

`compile_top_level` now reports unknown XSLT-namespace top-level elements with
that message and increments `style->errors`; `stylesheet::xsltParseStylesheetDoc`
detaches the document and frees the shell when `errors != 0`, returning NULL.

### Ownership note (why not "return -1 from compile")

The first attempt made `compiler::compile()` return -1 whenever
`style->errors != 0`. That produced `free(): double free detected in tcache 2`:
lxml frees its stylesheet document copy and then the stylesheet, and
`xsltFreeStylesheet` also frees `style->doc`. Upstream avoids this in
`xsltParseStylesheetUser` by detaching `style->doc` before reporting failure;
the candidate already did the same in its exported `xsltParseStylesheetUser`
(`exports_xslt_compile.rs`), but the lxml-facing `xsltParseStylesheetDoc`
called `compile()` directly. The correct layer is therefore the Doc entry
point, not the compiler.

A second discarded attempt incremented `style->errors` inside
`xsltTransformError` for every compile-time diagnostic. Upstream increments
`style->errors` at the *call sites* and uses `style->warnings++` for warnings
(invalid `xsl:output` method/value, missing `xsl:version`, most
`xsl:decimal-format` problems, undefined extension prefixes). The blanket
increment would have converted those warnings into hard failures, so the
increment is applied only to the new unknown-top-level-element reports.

## Remaining test_xslt failures

- `test_xslt_default_parameters` — stylesheet parameter override.
- `test_xslt_document_parse_deny`, `_deny_all` — `XSLTAccessControl` read
  denial must make `document()` raise `XSLTApplyError`.
- `test_variable_result_tree_fragment` — an RTF variable must be walked by
  `apply-templates` (`<A/>` expected `<A>bXb</A>`).
