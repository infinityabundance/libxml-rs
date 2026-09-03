# Phase 14.6 — XPath namespace axis + namespace-node string-value (96 → 95)

Date: 2026-09-04 · Commit: `ddaa4405` · Gate log: `phpbuild-c:/out/xpe-six21.log`
Diff vs previous valid gate (`xpe-six19.log`): `OLD=96 NEW=95 FIXED=1 NEW_ONLY=0`

## Fixed
- `ext/dom/tests/DOMXPath_evaluate_node_set_to_string.phpt`
  (namespace node-set passed to `php:functionString` — candidate aborted with
  a misaligned-pointer panic inside `xml::tree::node_get_content`).

## Root causes (three coupled edits, all required)
1. **`src/xml/xpath/axes.rs` — `namespace_axis` was not upstream-faithful.**
   Upstream `xmlXPathNextNamespace` semantics:
   - empty axis unless context node is an ELEMENT;
   - the implicit `xml` namespace is emitted FIRST (hardcoded
     `xmlXPathXMLNamespace`), always present;
   - remaining nodes are the in-scope declarations served in REVERSE order
     (`tmpNsList[--tmpNsNr]`): own decls first, then ancestors, nearest
     declaration wins (dedup by prefix);
   - every emitted node is an independent `_xmlNs` copy
     (`xmlXPathNodeSetDupNs`) whose `next` = owning element; php's
     `ext/dom/xpath.c` reads `node->_private` to recover the owner when
     building `DOMNameSpaceNode` proxies, and free paths key off `next`.
   Previous code emitted the *live* tree `_xmlNs` in forward order, produced
   wrong ordering (`xmlns:bar/xmlns:xml` shuffles) and fed ns structs to
   consumers as if they were tree nodes.
   New implementation emits the xml namespace first from a throwaway source
   struct (static literal href/prefix, struct freed after `push_copy`; the
   copy owns strdup'd strings), then `in_scope.iter().rev()` copies, each
   with `next` and `_private` set to the owning element.
2. **`src/xml/xpath/types.rs` — `compare_document_order`** walked tree links on
   namespace copies. A namespace node in a node-set is an `_xmlNs` copy with
   NO parent/children; reading `children`/`next` as node fields reads ns data
   at wrong offsets. Upstream `xmlXPathCmpNodes` returns a fixed result for
   comparisons involving namespace nodes. Added an early `Ordering::Equal`
   short-circuit when either operand is `XML_NAMESPACE_DECL`, which (a) never
   dereferences the copy as a node and (b) keeps `NodeSet::push`'s sort
   stable so the axis emission order (xml first, then reverse decl order)
   survives into the result — the php-visible contract for `//namespace::*`.
3. **`src/xml/tree/mod.rs` — `node_get_content`** lacked an
   `XML_NAMESPACE_DECL` arm. XPath string-value of a namespace node is its
   URI (upstream `xmlNodeGetContent` returns `ns->href`); the code fell into
   the element arm and walked `children` (which overlays ns prefix/href data
   in the copy), panicking with a misaligned dereference
   (`0x…2f67` = reversed ASCII of `g/XLM…`).
   New arm: `href` non-null → append `xmlStrlen(href)` bytes.

## Validation
- `cargo test --lib`: **1241 passed; 0 failed; 1 ignored** (pre-existing
  xinclude `#[ignore]`).
- `cargo fmt --check`: clean.
- Targeted (fresh DSO, `PHP_TESTS_LIST` single-shot run):
  `DOMXPath_evaluate_node_set_to_string`, `xpath_domnamespacenode`,
  `xpath_domnamespacenode_advanced`, `DOMXPath_evaluate_namespace_node_set`
  all PASS.
- Full six-extension gate `xpe-six21.log`: 95 failed (was 96), zero new
  names anywhere.

## Infrastructure notes (IMPORTANT)
- `cargo test --lib` and `cargo build --bin xsltproc` do **NOT** refresh the
  top-level `target/debug/liblibxml_rs.so` (crate-as-dependency artifacts
  land in `target/debug/deps/`). The php candidate mounts
  `target/debug:/candidate`, and
  `/candidate/lib/libxml2.so.16.1.3 -> ../liblibxml_rs.so`, so only a plain
  **`cargo build`** (lib as primary target) refreshes what php dlopens.
  The first xpe-six20 gate ran with the stale DSO and showed zero movement;
  after `cargo build`, xpe-six21 confirmed the flip. Always run
  `cargo build` before a gate when src changed.
- `node_string_value` (xpath/types.rs) has no namespace arm, but
  php's string conversion goes through `node_get_content`
  (exports_xml2.rs xmlXPathCastToString path), so no change was needed there.
