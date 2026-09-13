# lxml court 7ba — XPath predicate error classification and top-level node-set sorting

## Root causes fixed

### 1. `XPATH_INVALID_PREDICATE_ERROR` was never produced

Upstream's predicate grammar is lenient about trailing content: the predicate
sub-expression parser (`xmlXPathCompileExpr`) parses one complete Expr and
returns, and the *caller* (`xmlXPathCompPredicate`) decides the error. If the
next character is not the closing `]`, it raises
`XPATH_INVALID_PREDICATE_ERROR` ("Invalid predicate"); a failure INSIDE the
sub-expression propagates its own error ("Invalid expression").

The Rust parser already stopped at the same place (its recursive-descent
`parse_or_expr` returns after `1.1` in `//*[1.1.1]`, leaving the leftover
`.1`), but the `expect(RBracket)` failure was mapped to the generic
`XPATH_EXPR_ERROR`. `ParseError` now carries an `error_code`
(`XPATH_EXPR_ERROR` by default) and both predicate loops tag the missing-`]`
failure with `XPATH_INVALID_PREDICATE_ERROR`. The delivery layers
(`xmlXPathCtxtCompile`, `xmlXPathCompile`, `evaluate_str` →
`raise_internal_xpath_error`) surface the code and its message.

Nine oracle probes now match byte-for-byte
(`//*[1.1.1]`, `//*[1.1.1`, `//*[']['` → Invalid predicate; `//*[)`,
`1.1.1`, `//a[func(]`, `//a[`, `//*[@a=]` → Invalid expression).

Fixes `doc/xpathxslt.txt`.

### 2. Node-sets were not sorted like `xmlXPathNodeSetSort`

Upstream emits an `XPATH_OP_SORT` step on the outermost expression
(`xmlXPathCompileExpr` with `sort != 0`, skipped for literals), so a node-set
result is delivered in document order. Our top-level evaluation returned
insertion order, so an extension function returning a Python list of elements
came back unsorted.

The existing `NodeSet::sort` used a total-order `sort_by`, whose cross-document
fallback compares raw pointers. Upstream's `xmlXPathNodeSetSort` is a Shell
sort that swaps **only** when `xmlXPathCmpNodes == -1`; that comparator returns
`-2` for nodes from distinct documents, so cross-document entries keep their
insertion order. The pointer fallback reordered them — the source of the
long-standing `doc/extensions.txt` flakiness.

`NodeSet::sort` is now a faithful port: `cmp_nodes` (raw `xmlXPathCmpNodes`
including the attribute ordering, the namespace-node `1`, the
ancestor/`prev`/`next` shortcuts and the `-2` distinct-document case) plus the
Shell sort. For a single-document node-set the result is identical to the
previous document-order sort; for a multi-document set it is now stable.
`sort_result` applies it at the whole-expression boundaries
(`xmlXPathEvalExpression`/`xmlXPathEval`, `xmlXPathCompiledEval`,
`xmlXPathEvalExpr`).

Fixes the `test_xpathevaluator` `xpath` doctest and makes
`doc/extensions.txt` deterministic (128/128).

## Evidence

* Full suite: **17 raw unique failing ids**, down from 19 — **2 fixed,
  0 regressions** (plus `doc/extensions.txt` no longer flaky):
  * `doc/xpathxslt.txt` (131/131 doctest examples)
  * the `xpath` doctest in `test_xpathevaluator`
* `doc/extensions.txt` 128/128; `doc/objectify.txt` 288/288.
* `cargo test --lib`: 1325 passed / 0 failed / 3 ignored.
* `cargo fmt --check`: clean.
* PHP six-gate: 1250 passed / 0 failed.
* `failing-ids.txt` is the raw unique id set; the filtered delta against the
  pre-slice baseline is exactly the two ids above.

GitHub still carries no status checks or workflow runs; these are local and
committed results.
