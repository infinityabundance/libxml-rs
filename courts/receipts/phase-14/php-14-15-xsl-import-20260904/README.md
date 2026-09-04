# php-14.15 — xsl stylesheet import/include resolution + simplified-root validation (2026-09-04)

Full six-gate **33 → 28 failed**, ZERO regressions (name-level diff vs the
`phpbuild-c:/out/xpe-six30.log` baseline: **NEW_ONLY=0, FIXED=5**). Log:
`phpbuild-c:/out/xpe-six31.log`. xsl 13 → 9, simplexml 1 → 0 | dom 10 |
xmlreader 8 | xmlwriter 1. Commit on main (Phase 14.15).

## Root cause 1 — xsl:include/xsl:import resolved against CWD, not the stylesheet base (~3 tests)
`compile_top_level` passed the raw `href` to `xsltParseStylesheetFile`, so an
`<xsl:include href="include.xsl"/>` inside a stylesheet loaded from
`file:///…/tests/53965/collection.xsl` resolved against the process CWD and
silently failed; the `cd` template never compiled and the built-in rules
emitted raw text (bug53965, transformToDoc_sxe_type_error — and the 53965
data-dir dependency shared by those two). FIX: resolve the href against
`style->doc->URL` with `xml::uri::resolve_uri` (upstream imports.c
`xmlNodeGetBase` + `xmlBuildURI`) in both the include and import branches,
and make the builtin `input_from_file` strip a `file://` scheme+authority
(upstream xmlIO file-open semantics).

## Root cause 2 — xsltParseStylesheetDoc double-free on failure + simplified-root validation (~1 test)
`compile()` treated any non-xsl:stylesheet root as a simplified stylesheet and
compiled it unconditionally, so `XSLTProcessor::importStylesheet()` accepted a
`<container/>` document (gh21496 returned true, then a double free: the doc
was freed by `xsltParseStylesheetDoc` on failure AND released by PHP).
Upstream `xsltParseStylesheetProcess` requires the literal-result root to
carry `xsl:version`, else emits "xsltParseStylesheetProcess : document is not
a stylesheet" and returns NULL *without* freeing the document. FIX: the
version check in `compile()`'s simplified branch, and `xsltParseStylesheetDoc`
detaches `style->doc` before freeing the stylesheet on failure so PHP's clone
release is the single owner (file/memory parse call sites that own their doc
free it themselves).

## Root cause 3 — xsltMaxDepth default 30000 instead of 3000 (~1 test)
`maxTemplateVars_modification_validation_bypass.phpt` var_dumps the untouched
`maxTemplateDepth` property as `int(30000)`; upstream `transform.c` defaults
`xsltMaxDepth = 3000`. One-line default change; `xsltMaxVars = 15000` already
matched.

## Guards / validation
- cargo test --lib 1241 pass / 1 ignored; clippy clean; fmt clean. The two
  simplified-stylesheet rust tests were made upstream-faithful (xsl:version on
  the literal root).
- simplexml/gh17153 also flipped green in the full gate (order-sensitive
  autovivification case; watch for flakiness in later gates).

## Residuals next (28)
dom 10 (html save/doctype/noimplied leftovers, DTDNamedNodeMap, isEqualNode,
override_encoding pair, bug80268_2, getLineNo_65536 …) | xsl 9 (xslt007 +
transformToURI + bug54446 pair = result-file writes + saxon output security;
bug71571_a/b = recursion depth/vars reporting; gh21357_2 = xmlns-ns attr
copy; req30622 = namespaced params; xinclude doXInclude heap crash) |
xmlreader 8 | xmlwriter 1.
