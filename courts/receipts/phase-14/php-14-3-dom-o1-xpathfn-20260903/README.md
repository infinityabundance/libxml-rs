# dom O1 — XPath php-function-callback bridge (function/URI identity)

Closed 2026-09-03. Full suite **255 → 251**, zero regressions. dom 164 → 160:
`modern/xml/return_dom_node_from_xpath`, `registerPhpFunctionNS`,
`DOMXPath_constructor_registered_functions` and `gh22077` PASS. simplexml 9 /
xml 0 / xmlreader 29 / xmlwriter 1 (W5) / xsl 52 unchanged. Log:
phpbuild-c:/out/o1-six.log (1291-run, 251 failed / 40 skipped).

## Root cause (mirrors xpath.c 2.15 xmlXPathCompOpEval)

The candidate's C-extension-function bridge (`call_c_xpath_function`,
src/abi/exports_xml2.rs) invoked a registered `xmlXPathFunction` WITHOUT first
exposing the invoked function's identity on the context. Upstream:

```c
oldFunc = ctxt->context->function;
oldFuncURI = ctxt->context->functionURI;
ctxt->context->function = op->value4;       /* local name */
ctxt->context->functionURI = op->cacheURI;  /* resolved ns URI or NULL */
func(ctxt, op->value);
ctxt->context->function = oldFunc;
ctxt->context->functionURI = oldFuncURI;
```

PHP registers ONE C trampoline for every custom-namespace XPath function
(`Dom\XPath::registerPhpFunctionNs` → `xmlXPathRegisterFuncNS`; xsl
`xsltRegisterExtFunction`) and dispatches to the PHP closure by reading
`ctxt->context->functionURI` + `ctxt->context->function` back
(`dom_xpath_ext_fetch_intern` → `php_dom_xpath_callbacks_call_custom_ns`).
The candidate left both fields as whatever the context held → the namespace
hash lookup dereferenced garbage → segv inside libc (every
`return_dom_node_from_xpath`-shaped test).

## Fix

- `call_c_xpath_function` gains the invoked function's local `name` and
  optional `ns_uri`; it NUL-terminates both, sets `(*c_ctxt).function` /
  `functionURI` around the callback, and restores the previous values after
  (nested-call safe).
- `c_func_bridge_closure`/`c_func_call_bridge` parse the registration key —
  `{uri}name` Clark notation from `xmlXPathRegisterFuncNS`, bare `name`
  otherwise (`split_qualified`) — once at closure build.
- The xslt extension-function closure (`register_xslt_functions`,
  src/xslt/transform/mod.rs) passes the local name + resolved namespace href
  it already resolved at lookup time.

## Guard

exports_xml2 `test_c_xpath_function_bridge_exposes_function_and_uri`:
register ns `t` → `urn:t` and a C function via `xmlXPathRegisterFuncNS`, eval
`t:capture()`; the C callback reads `ctxt->context->function`/`functionURI`
exactly like php's trampoline and pushes `"capture@urn:t"` back; the eval
result asserts that string. Fails at HEAD with a garbage deref.

## Probes

- consumers/xpath-retnode.php — the phpt body (php pin; green now).
- consumers/bug79968-repro.php + adopt-reduce.php + savetree-probe.c —
  remaining O1 ADOPT-family residual (DOMDocument_adoptNode / bug79968 crash
  at shutdown after adoptNode(docless text) + saveXML(node)). The pure-engine
  xmlDOMWrapAdoptNode(NULL, NULL, docless-text, doc, NULL, 0) + xmlSaveTree
  probe (savetree-probe.c) is byte-identical to the oracle on both sides, so
  the defect sits in the php serializer/adopt interplay (documented as an open
  O1 residual in CURRENT-STATE.md).
