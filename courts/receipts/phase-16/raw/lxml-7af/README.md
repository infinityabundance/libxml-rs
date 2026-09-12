# lxml court 7af — XPath lexer / context propagation / id()

`candidate_sha=0e6a5c34` · oracle libxml2 2.15.3 · lxml 6.1.2 · 2007 tests

## Result

```
previous (eaffc17f / lxml-7ae)   89 unique failing ids
now (0e6a5c34 / lxml-7af)        80 unique failing ids
fixed                             9
new                               0
PHP six-gate                     1250 passed / 0 failed
cargo test --lib                 1325 passed / 0 failed / 3 ignored
```

## 1 — the lexer skipped untokenizable characters

`etree.XPath('\\fad')` compiled successfully. The lexer's catch-all was:

```rust
// Unknown character, skip
self.advance();
self.next_token()
```

Upstream `xmlXPathCompExpr` records a syntax error at that byte offset.
Verified against the oracle:

```
oracle    expr=[\fad] comp=NULL errNo=1207 dom=12 int1=0
candidate expr=[\fad] comp=OK   errNo=0    dom=0  int1=0     (before)
candidate expr=[\fad] comp=NULL errNo=1207 dom=12 int1=0     (after)
```

The lexer now records `invalid_pos` (first offset it cannot tokenize) and
`parse_xpath` fails with "Invalid expression" there. `xmlXPathCtxtCompile`
already maps a parse failure to `XPATH_EXPR_ERROR`, delivered as the
1200-based 1207 in the `XML_FROM_XPATH` domain.

Fixes `test_xpath_class_error`, `test_xpath_compile_error`, `test_xpath_error`.

## 2 — a lone `/` was `self::node()`

`parse_location_path` returned `Expr::Step(Self_, Node)` for `/` with no
relative path, so it evaluated against the context node and
`tree.xpath('/')` returned the root **element**. XPath 1.0
`AbsoluteLocationPath ::= '/' RelativeLocationPath?` selects the document
node (upstream `xmlXPathRoot` sets the context node to the document). It is
now an `AbsolutePath` over `self::node()`, which `eval_absolute_path` starts
from the document node — so lxml maps it to `[]`.

Fixes `test_xpath_document_root`.

## 3 — document and context-node propagation

### `xmlXPathEvalExpression` did not mirror `ctxt->doc`

It mirrored `ctxt->node` into the internal Rust context but not `ctxt->doc`.
lxml evaluates an `ElementTree` against a temporary "fake" document built by
`_fakeRootDoc` (non-recursive copy + `xmlDocCopyNode(node, doc, 2)` +
`xmlDocSetRootElement`, with the original children transplanted and their
parent pointers diverted). `xpctxt->doc = c_doc` pointed at that fake
document, but the evaluator kept using the document the context was created
with, so `ElementTree(c).xpath('/c')` searched the real document.

```
oracle:    tree.xpath('/c') -> [<Element c>]   tree.xpath('/*') -> [<Element c>]
candidate: tree.xpath('/c') -> []              tree.xpath('/*') -> [<Element a>]   (before)
```

Fixes `test_elementtree_getpath_partial`, `test_xpath_evaluator_tree_absolute`.

### Extension functions saw a stale context node / document

`_XPathContext.context_node` (lxml `extensions.pxi`) reads
`xpathCtxt->node` and requires `node->doc == xpathCtxt->doc`. Both bridges
now mirror the evaluator's live `context_node`/`document` into the C context
before the call:

- NS-registered functions: `c_func_bridge_closure` (this is the path lxml's
  `xmlXPathRegisterFuncNS` trampolines take).
- C func-lookup functions: `invoke_c_extension_function`.

For XSLT, upstream sets `xpctxt->doc = cur->doc` in the apply-templates and
for-each loops (transform.c 5093/5553) and restores it at the end. The
candidate never did, and `eval_xpath` additionally forced the transform's
principal document back onto the XPath context. Both are now upstream-shaped:
the loops switch the document per source node, and `eval_xpath` only sets
`xpathCtxt->node`.

Fixes `test_xpath_context_node`, `test_xpath_on_foreign_context_node`.

## 4 — `id()` was a stub

```rust
const fn fn_id(...) -> ... { Ok(XPathValue::NodeSet(NodeSet::new())) }
```

Ported `xmlXPathIdFunction` + `xmlXPathGetElementsByIds`: the argument's
blank-separated ID tokens — the per-node string values of a node-set
argument, or the string cast of any other argument — are resolved with
`xmlGetID`, and the element owning the ID attribute (or the element itself
when `xmlGetID` returns one) is added to the result.

Fixes `test_xpath_text_from_other_document` (the extension function returns
`etree.XML(...).xpath("id('k1')")`, which was empty).

## Residual

`test_variable_result_tree_fragment` (in the same XSLT extension class) still
fails: a variable holding a result tree fragment is not walked by
`xsl:apply-templates` (`<A/>` instead of `<A>bXb</A>`). It was failing before
this change too; the foreign-context fix did not alter its outcome.
