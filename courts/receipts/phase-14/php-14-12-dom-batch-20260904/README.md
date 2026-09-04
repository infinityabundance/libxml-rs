# php-14.12 — dom memory/entity/attr-value clusters (2026-09-04)

Full six-gate **54 → 40 failed**, ZERO regressions (name-level diff vs the 54
baseline `phpbuild-c:/out/xpe-six26.log`: **NEW_ONLY=0, FIXED=14**). Log:
`phpbuild-c:/out/xpe-six28.log`. dom 31 → 17 | xsl 13 | xmlreader 8 |
simplexml 1 | xmlwriter 1. Commit on main (Phase 14.12).

## Root cause 1 — cross-doc moves never re-interned dict strings (~3 tests)
`node_set_doc_impl` was a stub ("without dictionary and ID-table handling").
When a node moved to a document with a different dictionary, names interned
in the SOURCE doc dictionary stayed borrowed; once the source doc died its
dict freed them and the destination teardown double-freed (adoptNode →
`double free or corruption`, heap-layout dependent). PHP >= 2.13 skips its
own fixup (`#if LIBXML_VERSION < 21300` in php_dom_adopt_node) and relies on
libxml2 commits 4bc3ebf + bc7ab5a living in xmlNodeSetDoc.
FIX: `node_set_doc_impl` mirrors upstream 2.15 `xmlNodeSetDoc` — names for
ELEMENT/ATTRIBUTE/PI/ENTITY_REF owned by the old dict are re-interned via
`xmlDictLookup` into the new dict or `xmlStrdup`'d when the destination has
no dict; TEXT/CDATA content dup'd to the heap; ID attributes removed from
the old doc ID table; entity-refs re-resolved; DTD detached. Engine
`propagate_doc` (xmlAddChild/append/root-set paths) now delegates to it
per-node under a doc-change guard.
Flipped: DOMDocument_adoptNode, DOMElement_insertAdjacentElement,
HTMLDocument_serialize_ns_imported_05, gh17145, HTMLCollection_named_reads.

## Root cause 2 — output flush skipped the empty write callback (~3 tests)
Our `output_buffer_flush` returned early when the internal buffer was empty;
upstream xmlOutputBufferFlush ALWAYS invokes the installed write callback
(even with len 0). PHP's `php_libxml_write_smart_str` allocates the smart_str
on that len-0 call, so `smart_str_extract` returns a regular heap "" string.
Without it, empty saves returned the interned `zend_empty_string`, which
`RETURN_NEW_STR` later "frees" (`_efree` on an interned string → SEGV /
`zend_mm_heap corrupted` / interned-string double free at shutdown).
Flipped: DOMDocument_saveXML_XML_SAVE_NO_DECL (empty doc), bug79968
(saveXML of adopted empty text), DOMElement_replaceChildren (empty fragment).

## Root cause 3 — entity references to predefined/declared entities (~5 tests)
- `entities::get_entity` returned NULL for a NULL doc; upstream
  xmlGetDocEntity falls back to xmlGetPredefinedEntity. A doc-less
  `new DOMEntityReference("amp")` must bind the predefined declaration as
  children (firstChild/lastChild objects, textContent "&").
- The predefined entity statics lived in `.data.rel.ro`; full RELRO
  mprotects that read-only and php's `_private` store faulted. They are now
  `static mut` (writable `.data`, mirroring upstream's writable globals).
- `node_get_content` ENTITY_REF now mirrors xmlBufGetEntityRefContent:
  PREDEFINED entities contribute content; others contribute their CHILD
  tree (lazily materialized), with the EXPANDING (1<<3) cycle guard. Direct
  ref content therefore reads "" until a parse reference materialized the
  declaration children (delayed_freeing expectations).
Flipped: DOMEntityReference_predefined_free, DOMEntity_fields,
delayed_freeing/entity_reference, delayed_freeing/notation_declaration.

## Root cause 4 — PUBLIC ""/SYSTEM literals collapsed to NULL (~1 test)
`vec_to_cstr_null` maps empty slices to NULL; `<!ENTITY x PUBLIC ""
"sys">` needs an ALLOCATED empty ExternalID (DOMEntity::$publicId "").
Added `vec_to_cstr_keep_empty` for the entity-decl external literals.
Also notation decls ignored the SYSTEM/PUBLIC keyword, storing the SYSTEM
URI in ExternalID (publicId/systemId swapped); the keyword is now honored.

## Root cause 5 — attribute values did not expand general entities (~2 tests)
`getAttribute` on `<root a="x&ent;x"/>` must read "xfoox" while
serialization round-trips `a="x&ent;x"` (the tree keeps an ENTITY_REF child).
`parser_build_attr_children` (sax/default.rs) now mirrors xmlNodeParseAttValue:
when an internal entity is referenced in an attr value, the declaration's
child tree is materialized ONCE from its content (XML_ENT_PARSED) under the
EXPANDING flag; `node_get_content`'s ATTRIBUTE arm walks ALL children
(text + expanded refs) instead of only the first text child; entity child
trees are freed at declaration teardown (free_entity_internal).
Flipped: modern/css_selectors/entities, modern/token_list/entities.
dom001 (content entity + attr entity in one doc) kept green — flag bits
match the parser convention PARSED=1<<0 / EXPANDING=1<<3.

## Guards / validation
- cargo test --lib 1241 pass / 1 ignored; fmt clean.
- Probe byte-parity vs oracle: attr entity expansion + round-trip,
  ref textContent semantics, notation fields (probes kept in consumers/).

## Residuals next (40)
dom 17: xinclude engine (liveness_xinclude, gh14702, bug43364), C14N pair
(DOMNode_C14N_references, canonicalization), textContent edge cases pair
(textContent_edge_cases, bug80268_2), html save/noimplied leftovers
(loadHTMLfile_error1, bug67081, getLineNo_65536, gh12616_3,
serialize_non_default_empty_xmlns), DTDNamedNodeMap, isEqualNode,
override_encoding pair, gh21544; xmlreader 8; xsl 13; simplexml 1;
xmlwriter 1.
