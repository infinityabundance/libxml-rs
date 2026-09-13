# lxml court 7aw — XSLT document loading, access control, caller-parameter scope

## Root causes fixed

### 1. `xsl:include` / `xsl:import` bypassed the registered loader

`compile_top_level` loaded included/imported stylesheets with
`xsltParseStylesheetFile`, which calls `xmlReadFile` directly. Upstream
`imports.c` goes through `xsltDocDefaultLoader`, i.e. the `xsltDocLoaderFunc`
installed by `xsltSetLoaderFunc`. lxml installs `_xslt_doc_loader`
(`xslt.pxi`), which runs the Python resolvers off `style->doc->_private` and
records `XSLTParseError("Cannot resolve URI ...")` when the load fails.

The fix adds `load_style_doc`, which calls `xslt_doc_default_loader(...,
XSLT_LOAD_STYLESHEET)`, wraps the result with `xsltParseStylesheetDoc`, and on a
NULL load reports `unable to load <uri>` and bumps `style->errors` (upstream
increments at the call site, and lxml's `XSLT.__init__` only raises when
`c_style.errors` is set). `doc/resolvers.txt` is now 34/34.

### 2. `document()` ignored the transform's access control

`xsltLoadDocument` loaded unconditionally. Upstream checks
`xsltCheckRead(ctxt->sec, ctxt, URI)` first and fails the load with
`xsltLoadDocument: read rights for <uri> denied`. The check is now performed
after the document cache lookup; a denial records the diagnostic and returns
NULL, so `document()` yields an empty node-set and the transform ends in
`XSLT_STATE_ERROR` — which `xsltApplyStylesheetUser` already turns into a NULL
result, giving lxml's `XSLTApplyError`.

`document('')` resolves to the stylesheet's own URL, so that path now runs the
same check before falling back to the stylesheet document
(`test_xslt_document_parse_deny`, `_deny_all`).

### 3. Caller parameters were written onto the compiled stylesheet

`xsltApplyStylesheetUser` prepended the caller's `(name, value)` pairs to
`style->variables`. That mutated the compiled stylesheet: after one call, a
later call without parameters still saw the stale value, so a stylesheet's own
`<xsl:param>` default could never win (`test_xslt_default_parameters`).

Upstream keeps user parameters on the *transform* (`xsltEvalUserParams` ->
`xsltEvalOneUserParam` -> `xmlXPathRegisterVariableNS`). The new
`xslt_eval_user_params` registers each pair directly into the transform's
variable map under the stylesheet's declared raw name and frees the temporary
stack element, so nothing persists between applies. The stylesheet's declared
param is then skipped by the existing `skip_if_bound` rule.

### 4. `xmlEncTable` ported in full

`encoding_from_name` carried a hand-written subset of upstream's alias table.
The complete 157-entry case-insensitive `xmlEncTable` (encoding.c) is now
ported, with the `xmlCharEncoding` variants it needs (`UTF16`, `HTML`,
`8859_10/11/13/14/15/16`, `WINDOWS_1252`) added at their upstream enum values.
Candidate-only spellings that upstream resolves outside the table (the
R-000157 `utf-32*` mapping lxml's PEP-393 path relies on, `cp932`, `latin-N`,
`ebcdic`, `us`, ...) are retained explicitly so the port does not narrow the
accepted set.

`xmlFindCharEncodingHandler` also gained the canonical re-lookup already
present in `xmlFindCharEncodingHandler_owned`, so an alias such as
`iso8859-1` (a genuine table entry, and the spelling lxml's serializer passes)
resolves to the real `ISO-8859-1` handler.

## Evidence

* Full suite: **29 raw unique failing ids** (28 filtered), down from 34 (33) —
  5 fixed, **0 regressions**.
* `cargo test --lib`: 1325 passed / 0 failed / 3 ignored.
* `cargo fmt --check`: clean.
* PHP six-gate: **1250 passed / 0 failed** (rc=0).
* `failing-ids.txt` is the raw unique id set; the filtered delta against
  lxml-7av is exactly the five ids above.

GitHub still carries no status checks or workflow runs; these are local and
committed results.
