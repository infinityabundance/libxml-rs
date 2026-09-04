# php-14.10 — html id registration + nameless doctype + NOIMPLIED/NODEFDTD (2026-09-04)

Full six-gate **64 → 57 failed**, ZERO regressions (name-level diff vs the 64
baseline `phpbuild-c:/out/xpe-six24.log`: **NEW_ONLY=0, FIXED=7**). Log:
`phpbuild-c:/out/xpe-six25.log`. dom 38 → 31 | xmlreader 8 | xsl 16 |
simplexml 1 | xmlwriter 1. Commit `225a7cd4`.

## Root cause 1 — html-parsed docs never registered ID attributes (~4 tests)
The html tree builder attaches attributes via tree::set_prop BEFORE the
element gains its document pointer (add_child propagates doc later), so the
per-attribute ID registration added in 14.9 could never run — doc->ids stayed
empty and getElementById returned NULL on every loadHTML'd document
(node_textcontent, bug77686, DOMElement_toggleAttribute).
FIX: tree-order post-parse pass `register_html_ids` (html/mod.rs) walks the
finished document and registers every id/IDREF attribute (is_id → add_id /
is_ref → add_ref) with first-wins semantics, matching the in-order xmlAddID
calls of a SAX2 parse.
Also: explicit `<html>`/`<head>`/`<body>` SOURCE tags dropped their
attributes — the skeleton branches returned before attaching. `attach_attrs`
now runs in the html/head/body create branches (bug77686 needs `<body
id="x">`).

## Root cause 2 — nameless `<!DOCTYPE>` became the default HTML 4.0 DTD
`parse_html_doctype_decl` returned None for `<!DOCTYPE>` (no root name), so
html_parse_buffer fell into the default-HTML4-DTD branch — doctype->name read
back "html". Upstream htmlParseDocTypeDecl fires the internalSubset callback
with a NULL name for `<!DOCTYPE>`. The parser helper now returns the
empty-name case and html_parse_buffer creates the subset with a NULL name
(bug78025, gh17500; saveHTML prints `<!DOCTYPE >` — oracle byte-identical).

## Root cause 3 — HTML_PARSE_NOIMPLIED / HTML_PARSE_NODEFDTD ignored (bug76285)
The html parser never consulted these option bits: loadHTML(…,
NOIMPLIED|NODEFDTD) still wrapped content in the implied html/body skeleton
and emitted the default HTML 4.0 doctype.
FIX:
- ensure_html/ensure_head/ensure_body no-op under NOIMPLIED;
- top-level content (ctxt.current == NULL) attaches to the DOCUMENT instead
  of the implied body (body-transition + void-element + head-only branches);
- source `<head>`/`<body>` without an `<html>` parent become document
  children;
- the default HTML 4.0 DTD is skipped under NODEFDTD;
- the post-parse skeleton ensure is skipped under NOIMPLIED.

## Flipped (7)
bug76285, bug77686, bug78025, gh17500, node_textcontent,
DOMElement_toggleAttribute, bug78221. (HTMLCollection_named_reads passes in
an isolated run but stays red in the full suite — order-dependent, tracked.)

## Guards / validation
- cargo test --lib 1241 pass / 1 ignored; fmt clean.
- Targeted phpt runs green incl. dom005, gh15670, gh17397, gh19612,
  bug79701/toggle.
- Oracle comparison: nameless-doctype saveHTML byte-identical.

## Residuals next (57)
dom 31: memory-abort family (DOMDocument_adoptNode, DOMElement_
insertAdjacentElement, DOMElement_replaceChildren,
DOMDocument_saveXML_XML_SAVE_NO_DECL, bug79968,
DOMDocument_getElementsByTagName_liveness_xinclude, gh14702,
HTMLDocument_serialize_ns_imported_05), the C14N pair, entity/notation
family (DOMEntity_fields, DOMEntityReference_predefined_free,
delayed_freeing/entity_reference + notation_declaration, DTDNamedNodeMap,
gh17145, css/token entities), textContent/html serialization
(textContent_edge_cases, bug80268_2, loadHTMLfile_error1),
getLineNo_65536, gh21544, serialize_non_default_empty_xmlns,
override_encoding pair, isEqualNode, bug43364, bug67081, gh12616_3;
xmlreader 8; xsl 16; simplexml 1; xmlwriter 1.
