# Phase 14.3 — simplexml S3: XPath error delivered verbatim to the generic channel

Date: 2026-09-03
Suite movement: **238 failed → 236 failed** (ext/simplexml 4 → 3), zero
regressions. Receipt for the closure described at the end of
`courts/receipts/phase-14/CURRENT-STATE.md` ("simplexml S3 XPath error channel
closed").

## Member closed
- `ext/simplexml/tests/008.phpt` — `$sxe->xpath("**")` warned
  `XPath error : Invalid expression` (fragment-prefixed); oracle (and now the
  candidate) warns `Invalid expression`.

## Root cause
`raise_xpath_error` (src/abi/exports_xml2.rs, the shared compile/eval-failure
deliverer) always used `GenericDelivery::Stream`. The Stream delivery routes
the error through `xmlFormatError`'s fragment stream, which prefixes
`XPath error : ` and (with a source window) the expression/caret context
lines — matching upstream **only for the parser channels** (xmlParserError /
xmlParserWarning / xmlParserValidityError / xmlParserValidityWarning).

Upstream xpath.c `xmlXPathErrFmt` does NOT select a parser channel when no
structured handler is present:

```c
channel = xmlGenericError;
data = xmlGenericErrorContext;
channel(data, "%s", to->message);
```

`xmlGenericError` is not one of the parser channels, so error.c `xmlVRaiseError`
delivers `to->message` **verbatim** through the generic channel — no fragment
prefix. PHP installs a generic handler at request start
(`php_libxml_issue_warning`), so its `sxe->xpath(): ...` warning text is the
message alone. The Stream routing therefore doubled the prefix into the PHP
warning (`XPath error : Invalid expression`) and broke ext/simplexml 008.

## Fix
`raise_xpath_error` now selects the delivery channel the same way upstream
selects the callback:

- generic handler installed → `GenericDelivery::Custom(func, ctx)`
  → `channel(data, msg)` once, verbatim (the xmlVRaiseError behavior above);
- no generic handler → `GenericDelivery::Stream` fallback, preserving the
  console fragment stream (`XPath error : ...` + caret) that xmllint/xsltproc
  consumers rely on (HOSTILE-FAILURE F3, lxml).

Structured delivery is unaffected: with `ctxt->error` (or the global
`xmlStructuredErrorFunc`) installed the structured path fires first with the
1200-offset code (`XML_XPATH_EXPRESSION_OK + XPATH_EXPR_ERROR` = 1207) — the
kept probe proves candidate == oracle on that channel.

## Evidence
- `/out/xpe-six.log` (candidate, six-extension run after the fix):
  1291 tests / **236 failed** / 40 skipped. Breakdown: dom 151 | xsl 52 |
  xmlreader 29 | xmlwriter 1 | simplexml 3 | xml 0. ext/simplexml 008 PASS.
- Probe `courts/suites/phase14/consumers/xpeval-probe.c` (kept):
  structured handler receives `code=1207 msg=[Invalid expression]` on the
  candidate and the oracle alike.
- Engine guard added: `src/abi/exports_xml2.rs`
  `test_xpath_compile_error_verbatim_to_generic_channel` — a recording generic
  handler installed via `xmlSetGenericErrorFunc` receives exactly
  `b"Invalid expression\n"` (trailing newline, no `XPath error : ` prefix) for
  a failed `xmlXPathCtxtCompile(ctxt, "**")` with `ctxt->error == NULL`.

## Validation
- `cargo test --lib`: 1232 passed / 1 ignored (1233 total, +1 guard).
- `cargo clippy --lib`: no new warnings (4 pre-existing: unnecessary-cast x2,
  needless-option-as-deref, tree/mod.rs iter().any() — all untouched).
- `cargo fmt --check`: clean.
- Six-extension php suite re-measured at 236 (this log), zero regressions.

## Commit
`<filled at commit time>`
