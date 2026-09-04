# php-14.9 — ns-scope search, DTD attr decls, ID-table engine parity (2026-09-04)

Full six-gate **77 → 64 failed**, ZERO regressions (name-level diff vs the 77
baseline `phpbuild-c:/out/xpe-six23.log`: **NEW_ONLY=0, FIXED=13**). Log:
`phpbuild-c:/out/xpe-six24.log`. dom 51 → 38 | xmlreader 8 | xsl 16 |
simplexml 1 | xmlwriter 1. Commit `5663c421`.

## Root cause 1 — ns-scope search read doc->oldNs as element nsDef (~6 tests)
`xmlSearchNs`/`xmlSearchNsByHref` climbed `cur = node->parent` WITHOUT the
upstream `node->type == XML_ELEMENT_NODE` loop guard. `_xmlDoc.oldNs` sits at
the same offset as `_xmlNode.nsDef`, so once the walk hit the document node it
iterated php's parked-decl list (`php_libxml_set_old_ns` keeps retired xmlns
decls on `doc->oldNs`). php's `dom_reconcile_ns_internal` (appendChild glue)
queries `xmlSearchNsByHref(search_parent, href)` and STRIPS a child's own
nsDef when the parent side "already declares" the same href — phantom matches
stripped the `<a1 xmlns="http://example.com">` decls gh11500,
clone_attribute_namespace_01/02 and bug80927 need on output.
FIX (tree/mod.rs `search_ns` + `search_ns_by_href` rewritten to full upstream
semantics — xmlSearchNsSafe/xmlSearchNsByHrefSafe): element-only walk,
NULL-href decls never bind a prefix, ancestor `element->ns` pointers count
(`orig != node`), the `xmlNsInScope` shadow rule, and the is_attr
"no default-ns for attributes" rule; `doc->oldNs` head is the implicit xml
decl (`ensure_doc_xml_ns`), never walked as nsDef.
Flipped: gh11500, clone_attribute_namespace_01/02, bug80927, bug34276.

## Root cause 2 — no DTD default/#FIXED fallback in has/get-prop (~3 tests)
Upstream xmlHasProp/xmlHasNsProp/xmlGetProp/xmlGetNsProp (via
`xmlGetPropNodeInternal useDTD=1`) report an ATTLIST default/#FIXED
declaration (an XML_ATTRIBUTE_DECL with defaultValue) when no real attribute
matches — php's removeAttribute/toggleAttribute return false for such
decls, setAttributeNode treats them as "no existing attribute", and
hasAttribute/getAttribute read the default value.
FIX: `dtd_default_decl_lookup`/`dtd_default_decl_lookup_plain` helpers in
tree/mod.rs mirror xmlGetPropNodeInternal's DTD arm (elemQName prefix
expansion, xml-ns special case, in-scope-prefix search) and xmlHasProp's
plain arm; wired into has_prop/has_ns_prop/get_prop/get_ns_prop. The DTD
table key was also wrong: the ATTLIST parser registered the RAW QName
(`p:A`, ns NULL) while upstream xmlSAX2AttributeDecl splits to
(local="A", prefix="p") — namespaced `<!ATTLIST root p:A ...>` lookups
failed. parse_attlist_decl now splits. Serialization unaffected (dumps join
prefix:name).
Flipped: gh22825 (all three cases), bug38474.

## Root cause 3 — DTD defaults materialised as real attributes
SAX2's default tree builder created attribute nodes for EVERY entry in the
SAX atts array, but upstream `xmlSAX2StartElementNs` SUBTRACTS `nb_defaulted`
unless `XML_COMPLETE_ATTRS` is set — so a parse of
`<!DOCTYPE root [<!ATTLIST root A CDATA #FIXED "d">]><root/>` must NOT create
a real A="d". default.rs startElementNs now mirrors the subtraction.
(Part of gh22825 / bug79701 groundwork.)

## Root cause 4 — xmlSetProp ignored QNames; xmlSetNsProp never rebranded (~3)
php's legacy setAttribute paths hand libxml the raw QName
(`dom_create_attribute -> xmlSetProp(node, "xml:id", v)`) and rely on
upstream xmlSetProp SPLITTING the prefix and delegating to xmlSetNsProp with
the LOCAL name — the candidate treated the literal string as the name, so
re-setting a PARSED `xml:id="x"` produced `<test1 xml:id="x" xml:id="y"/>`.
Also upstream's xmlSetNsProp modify branch rebinds `prop->ns = ns` — the
modern DOM ns-mapper passes a fresh-prefix decl so
setAttributeNS("urn:a","y:foo") renames x:foo → y:foo.
FIX: set_prop rewritten to the upstream wrapper (xmlSplitQName4 → searchNs →
setNsProp; unbound prefix keeps the raw QName in no namespace); set_ns_prop
update path sets `attr->ns = ns`.
Flipped: modern/spec/Element_setAttributeNS.

## Root cause 5 — engine ID table never maintained (~4 tests)
Parse-time ID registration existed (is_id → add_id, atype=ID + doc->ids),
but (a) freeing/unlinking an ID attribute never called xmlRemoveID — stale
entries made xmlGetID report removed ids; (b) value changes never moved the
entry; (c) newly created id-ish attributes (html `id`, xml:id) never
registered; (d) xmlFreeDoc never freed doc->ids; (e) duplicate xml:id never
reported.
FIX: free_prop/free_prop_impl/remove_prop drop the entry (attr->id != NULL);
set_ns_prop update branch removes the old entry, keeps atype=ID and
re-registers the new value (xmlAddIDSafe); create branches register via
is_id → add_id_safe (xmlNewPropInternal parity); free_doc frees doc->ids
before the tree; default.rs raises "ID %s already defined\n" (XML_FROM_VALID,
code 513 XML_DTD_ID_REDEFINED, ERROR) at the attribute's input position when
registration collides.
Flipped: bug79701 remove_attribute/set_attribute_xml/swap/toggle.

## Root cause 6 — XPath name() returned the local name only
`name()` must return the QName (prefix:local) for prefixed element/attribute
names (XPath 1.0 §4.1); the core fn_name and the C-API map (which lacked
"name" entirely) both returned node->name. Added node_qname + the
xmlXPathNameFunction shim + registration.
Flipped: gh12455.

## Guards / validation
- cargo test --lib 1241 pass / 1 ignored; fmt clean; clippy baseline.
- C probes kept in consumers/: nsappend-probe.c (php's dom_reconcile_ns flow
  byte-identical candidate == oracle for matching/mismatching root ns),
  dtdqattr-probe.c, idstate-probe.c (atype + doc->ids registration parity).
- targeted phpt batches all green incl. dom005, token_list/attlist,
  gh19612, Element_setAttribute* guards.

## Residuals next (64)
dom 38: canonicalization + DOMNode_C14N_references (C14N engine), bug80268_2
(loadHTML NUL), the malloc_consolidate/double-free aborts
(DOMElement_insertAdjacentElement, saveXML_XML_SAVE_NO_DECL, adoptNode,
bug79968, HTMLDocument_serialize_ns_imported_05 …), bug76285/bug78025/
bug17500 (HTML textContent/doctype serialization), DOMEntity_fields +
delayed_freeing (entity/notation), bug67081, gh17145, gh21544, gh14702,
getLineNo_65536, modern/html + css/token entities, xmlreader 8, xsl 16,
simplexml 1, xmlwriter 1.
