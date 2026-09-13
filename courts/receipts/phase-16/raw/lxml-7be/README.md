# lxml court 7be — result tree fragments cross into C extension functions

## Root cause fixed

An XSLT variable holding inline content (a result tree fragment) was bound in
the XPath context as an internal `NodeSet` containing the fragment's document
node. When the variable was passed to a C-level extension function
(`etree.FunctionNamespace` / `xmlXPathRegisterFuncNS`, and the
`func_lookup`-based registration), the argument was converted to a plain
`XPATH_NODESET`.

lxml unwraps a node-set by taking element nodes only, and only descends into a
document node when the object type is `XPATH_XSLT_TREE`
(`_createNodeSetResult` / `_unpackNodeSetEntry`), so the fragment arrived as an
empty list: `myns:mytext($content)` saw no elements and
`test_variable_result_tree_fragment` produced `<A/>` instead of `<A>bXb</A>`.

Upstream hands a C function an `XPATH_XSLT_TREE` object whose node-set holds
the fragment's document node (`xmlXPathNewValueTree`). The argument bridge now
does the same (`xpath_arg_to_object`): a one-node node-set whose node is a
document becomes an `XPATH_XSLT_TREE` object. Both bridges use it — the
`funcLookup` path (`invoke_c_extension_function`) and the registered-function
path (`call_c_xpath_function`).

Fixes `test_variable_result_tree_fragment`.

## Evidence

* Full suite: **7 raw unique failing ids**, down from 8 — **1 fixed,
  0 regressions**:
  * `test_variable_result_tree_fragment`
* `cargo test --lib`: 1325 passed / 0 failed / 3 ignored.
* `cargo fmt --check`: clean.
* PHP six-gate: 1250 passed / 0 failed.
* `failing-ids.txt` is the raw unique id set; the filtered delta against the
  pre-slice baseline is exactly the one id above.

Remaining: the four HTTP input-loader ids and the three isoschematron ids.

GitHub still carries no status checks or workflow runs; these are local and
committed results.
