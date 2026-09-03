# Phase 14.3 — simplexml S7: node/document copies no longer carry `_private`

Date: 2026-09-03
Suite movement: **233 failed → 232 failed**, zero regressions (the only phpt
flipped is `ext/simplexml/tests/bug63575.phpt`). Log:
`phpbuild-c:/out/xpe-six4.log`.

## Member closed
- `ext/simplexml/tests/bug63575.phpt` — `clone $o1; $o2->xpath('/a')` +
  `addChild('c')` previously mutated the ORIGINAL `$o1`; the mutation now
  lands on the clone `$o2` exactly like the oracle.

## Root cause
SimpleXML's root-element clone deep-copies the whole DOCUMENT:

```c
/* php sxe_object_clone() */
if (is_root_element) {
    docp = xmlCopyDoc(sxe->document->ptr, 1);
    ...
    nodep = xmlDocGetRootElement(docp);
}
```

PHP keys its wrapper registrations on the C nodes' `_private`
(`php_libxml_node_ptr`, stored in `node->_private`; `php_libxml_increment_node_ptr`
reuses a registration found there). The candidate's copy machinery in
`src/xml/tree/mod.rs` COPIED `_private`:

- `copy_node` line 1153 (`(*new_node)._private = n._private;`)
- `copy_ns_list` lines 1279/1295

so every node of the copied document still carried the ORIGINAL document's
php wrapper pointers. PHP therefore bound the clone's SimpleXML object to the
ORIGINAL registrations: the clone's `xpath('/a')` context anchored in the
original document and `addChild('c')` mutated `$o1` while `$o2` stayed
unchanged.

Upstream never does this: `xmlStaticCopyNode` (tree.c 2.15) `memset(ret, 0,
...)` and fills only the structural fields; `xmlCopyNamespaceList` copies
href/prefix only. A copied subtree must look UNREGISTERED to consumers that
key state on `_private` (PHP, and lxml's similar registrations).

## Fix
`copy_node` and `copy_ns_list` no longer propagate `_private` — copies keep
the zeroed NULL. Upstream-faithful for every caller of these helpers:
`xmlCopyDoc` / `xmlDocCopyNode` (tree exports), the XInclude loader, the XSLT
variable value copies, the XPointer range copies and the XPath node-copy
helpers.

## Evidence
- `/out/xpe-six4.log`: 1291 tests / **232 failed** / 40 skipped. Name-level
  diff vs the 233 baseline: **0 new**, exactly bug63575 fixed.
- Probe `consumers/clone-xpath-probe.php` (kept): candidate == oracle —
  clone → xpath('/a') resolves to the CLONE's root (mutating it changes only
  `$o2`); xpath on the original mutates only `$o1`; a second clone taken
  before the mutation stays independent.
- Isolation probes (both engines identical, kept for reference):
  `consumers/copydoc-probe.c`, `consumers/clonedoc-xpath-probe.c`,
  `consumers/detached-xpath-probe.c` — confirmed `xmlCopyDoc`/`xmlDocCopyNode`
  ownership and absolute-path anchoring are already oracle-equal; the
  divergence was solely the `_private` inheritance into PHP's binding.
- Guard: `exports_xml2::tests::test_copies_do_not_carry_private` — after
  `xmlCopyDoc` and `xmlDocCopyNode` the copies' `_private` is NULL while the
  source marker is untouched.

## Validation
- `cargo test --lib`: 1234 passed / 1 ignored (1235 total, +1 guard).
- `cargo clippy --lib`: no new warnings (4 pre-existing, untouched).
- `cargo fmt --check`: clean.
- Six-extension php suite at 232 (this log), zero regressions.

## Commit
`<filled at commit time>`
