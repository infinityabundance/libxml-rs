# plan-dom — engine root-cause FAMILY map (168 head tests)

Method: each `.phpt`'s per-test `.diff` (captured in `phpbuild-c` container under
`ext/dom/tests/*.diff`) was read to classify by **observed candidate-vs-oracle
symptom**, mapped to the divergence surface in `libxml-rs` (`src/xml/tree`,
namespace reconciliation forced/prefix alloc, `save/xmlsave`, `parser/state`
fragment push-wrap, SAX/DTD decls, `exports_xml2` ABI). Member counts sum to 168.
Obs-Severity: M=output-mismatch (DOM text/attrs/health), C=crash/ownership/segv/abort/double-free,
E=error-message/code text parity only.

---

## CRASH / OWNERSHIP (highest priority; must land before any serialize family can green)

O1 **sax:XPath/DOM-live node & node-list lifetime, freed-node deref, glibc/tcache/abort** — count 18 | M/C.
`DOMNode_isEqualNode`, `DOMElement_replaceChildren`, `DOMElement_insertAdjacentElement`,
`DOMDocument_adoptNode`, `DOMDocument_saveXML_XML_SAVE_NO_DECL`, `bug79968`, `bug80927`,
`namespace_sxe_interaction`, `return_dom_node_from_xpath`, `registerPhpFunctionNS`,
`DOMXPath_constructor_registered_functions`, `xpath_domnamespacenode_advanced`,
`modern/xml/XMLDocument_fromString_02`, `XMLDocument_fromFile_override_encoding`,
`modern/spec/clone_document`, `modern/xml/gh22570`, `gh11500`, `gh12616_3`.
Root: XPath php-function-callback + DOMNode object re-entrancy frees the C-side node underneath
the engine, or save/DOM-live adopt/adopt-or-copy double-frees (adoptNode/SaveNoDecl/fromString02
show `malloc_consolidate`/`free(): double free`/`Aborted`). Engine surface: ownership +
`delayed freeing` of nodes whose engine handles cross the php `_dom_object` lifetime boundary.
Prereq for: many token_list/template/serialize tests that only SIGSEGV when ns prep fails.

## FRAGMENT+innerHTML well-formedness (parser push-wrap) — gates all inner/outerHTML greenups
F1 **content `<!`-markup / fragment-EOF rejection (push xmlParseInNodeContext wrap)** — 7 | C/M.
`modern/xml/Element_innerHTML_writing`, `Element_innerHTML_prefixed_writing`,
`Element_outerHTML_writing`, `Element_innerOuterHTML_reading`, `Element_insertAdjacentHTML`,
`modern/common/innerHTML_cache_invalidation`, `modern/html/parser/Element_innerHTML`, `Element_outerHTML`.
Root: the SP-14.3.1-8 divergence — libxml-rs must clear `wellFormed` (err 68 NAME_REQUIRED) on a
`<!...` in ELEMENT content and on the xml fragment that should serialize raw text (`&`, `<`),
else the DOM wrap emits "XML fragment is not well-formed". **This family MUST be fixed in the
engine, not shelled around, because a separate ext/xml push-EOF change regresses it**
(=SP-14.3.1-8 interlock). Also drives several `saveXml(): Could not save document` cases (see S).
Reduces `xml_parsing_*` interplay risk.

## SERIALIZATION / SAVE / DOCTYPE-ENTITY
S1 **saveXml/HTML save refuses doc → "Could not save document"/empty-node; standalone=yes XML decl & doctype placement dropped; entity-ref attr & `&#xA0;` serialize** — count ~16 | M/E.
`dom001`, `dom005`, `modern/html/parser/Element_innerHTML`(2nd part), `modern/html/parser/Element_outerHTML`,
`modern/spec/bug81468`, `modern/spec/dom_parsing_gh47`, `modern/spec/bug47530`, `modern/common/Document_title_setter`,
`modern/common/template_manual/nested/participation`(serialize tail), `modern/css_selectors/namespaces`,
`modern/css_selectors/pseudo_*` many, `modern/token_list/{remove,toggle}` (save on empty att), `not_serializable`,
`modern/html/interactions/noscript`, `modern/html/interactions/Dom_Element_insertAdjacentHTML`,
`modern/html/parser/xml_style_namespace`, `HTMLDocument_entity_reference`, `HTMLDocument_serialize_ns_imported_01..06`,
`HTMLDocument_serialize_doctype`, `modern/html/parser/HTMLDocument_fromString_old_dtd`, `HTMLDocument_fromString_*`.
Root: `xmlsave` export refusals / decision (`xmlSaveDoc` late error) on subtrees it can serialize only
after ns normalization; encoding of non-ASCII attr text to `&#xN;`; standalone keep; empty-element
`<p/>` vs `<x></x>`; and doctype-duplication in DOM-built clones (`clone_document`) or createDocumentType.
S2 **DOCTYPE/intSubset entity literal content `<context/>` marker, ID-attr warning parity** — ~4 | M/E.
`bug67081`(intSubset text retained block via `createFromString` at base), `bug79701/remove_attribute,set_attribute_xml,swap,toggle`(ID-`x already defined` + detached attr staleness), `DOMDocument_saveXML_XML_SAVE_NO_DECL`.
Root: DTD/doctype DTD serialization & the XML_ID hash lifecycle on setAttribute/removal; and DOM-doc
`xmlDecl`.`standalone=yes` after save.

## NAMESPACE + PREFIX RECONCILIATION (prereq to the S1 serialize-ns sub-bucket AND clone/import)
N1 **duplicate `xmlns:` new-prefix allocation & attr `namespaceURI=NULL` (sp17 forced prefix alloc)** — ~20 | M.
`createAttributeNS_prefix_conflicts/*`(5), `DOMElement_setAttributeNS_prefix_conflict`,
`DOMElement_prefix_empty` (prefix→conflicting-URI must Namespace Error), `delayed_freeing/namespace_definition_crash_in_attribute`,
`clone_attribute_namespace_01`, `clone_attribute_namespace_02`, `DOMDocument_importNode_attribute_prefix_conflict`,
`import_attribute_namespace`, `modern/spec/Element_setAttributeNS`, `Element_setAttributeNS`(modern+xpath), `bug47530`,
`modern/spec/bug47530`, `DOMElement_toggleAttribute`, `DOMElement_insertAdjacentElement`(ns part), `modern/extensions/attribute_renaming_conflict`,
`modern/xml/serialize_empty_xmlns`, `serialize_non_default_empty_xmlns`, `modern/html/serializer/HTMLDocument_serialize_ns_imported_*`.
Root: bind/find a free mangled prefix (`default##`) when re-adding an attr after its decl ns is bound
to an existing prefix; libxml-rs reuses the taken prefix → duplicate `xmlns:foo` + duplicate attr +
`Attr::namespaceURI=NULL`. **Gate: before the HTML-ns serialize family green.**

## IMPORT / CLONE / REPLACE / textContent / isEqual
M1 **import(clone, adopt, html ns adopt, replaceChildren, prepend/append after-first-copy), & element textContent empty-element form, isEqualNode attr-order/local-value** — ~11 | M/C.
`DOMDocument_importNode_attribute_prefix_conflict`+`general`, `modern/spec/HTMLDocument_importNode_01`,
`node_textcontent`, `modern/spec/textContent_edge_cases`, `modern/spec/Element_prefix_readonly`,
`DOMElement_replaceChildren`, `DOMElement_insertAdjacentElement`, `DOMNode_C14N_references`,
`modern/common/innerHTML_cache_invalidation`, bug77686(bug-report fragment) — firstChild/textContent after clone.
Root: `xmlDocCopyNode`/adopt in `exports_xml2::xmlAddChild` drops the ns-owner in clones/child-links, so
child textContent reads NULL-Child.
## DELAYED-FREE+ENTITY (info propagation; on oracle DOM tree internals)
D1 **DOC-limited: notationDecl/elementDecl content-model ` , ` retention + DOMENTITY/REF child graph** — ~7 | M.
`delayed_freeing/notation_declaration`, `element_declaration` (content `(child1child2)` no ` , `), `entity_reference`(ent), `DOMEntity_fields`(publicId NULL ""),
`DOMEntityReference_predefined_free`(child link), `modern/xml/DTDNamedNodeMap`(baseURI), `not_serializable`. Root: DTD/notation/entity-model
declarations not faithfully stored from internal-subset SAX decls (sp-14.3.1-1 sibling), & DOMNameSpaceNode/entity content.

## XInclude / liveness
X1 **getElementsByTagName + liveness expansions missing xi:fallback/xinclude** — 3 | M. `DOMDocument_getElementsByTagName_liveness_xinclude`, `gh14702`, (getLineNo infra). Root: XInclude (XML_XINCLUDE) loader/flag not gated on the engine push-wrap — separate code path in php xinclude lookup that must call into enum include substitution.

## PATH-XGET / ENCODING / OPTION SEMANTICS (sub-families; often shared w/ xmlreader/simplexml)
L1 **LIBXML parse-option: RECOVER warn text, NO_XXE external-entity still attempted+fatal, NOENT attr substitution** — ~8 | M.
`xml_parsing_LIBXML_RECOVER`, `xml_parsing_LIBXML_NO_XXE` + sim/simplexml & xml reader sibling →
SIG to the `src/xml/parser/options.rs` net without regression; `DOMDocument_loadHTMLfile`, `_variation2`
(dir/file connect), `loadHTMLfile_error1`(missing I/O warning); `bug80268_2`(NUL-truncation of
html read via memory); `bug76285`/`bug78025`(fragment html DOM shape).
L2 **input-encoding/over-encoding table + NUL-filename + empty file + getLine col clamp** — 5 | M.
`XMLDocument_createFromFile_override_encoding`+`createFromString_override_encoding` (reject "Windows-1252"→ValueError),
`XMLDocument_fromFile_02`(empty/NUL `\0`→false-mode), `XMLDocument_createFromFile_empty_input`("Document is empty"+throw),
`modern/html/interactions/getLineNo_65536`(line clamp).
Prereq none; independent engine (input/encoding). **Independence: parser EOF/col-layer edits must not clamp**.
L3 **XPath namespace axis + DOMNameSpaceNode yield**; ~5 | M/C. `DOMXPath evaluate_namespace_node_set` (namespace axis count 0), `xpath_domnamespacenode`,
`xpath_domnamespacenode_advanced`, `DOMXPath_evaluate_node_set_to_string`(string() of namespace node), `registerPhpFunctionNS`. Root: XPath `namespace::` / `DOMNameSpaceNode` object absent from engine axis-implementation; and php function↔dom lifetime (O1-colliding).

## VALIDATION (SP-14.3.4) — owns schemaValidate*/relaxNG*/validate*
V1 **schemaValidate add-attrs + empty-member bool** — 3 | M. addAttrs+SourceError (root: schema default-attr injection on the *valid-empty attr*)...
V-1 shows driver: DOMDocument_schemaValidateSource_addAttrs/DOMDocument_schemaValidate_addAttrs.
V1-schema **empty/语法/root-no-match + "not a schema" I/O + Error-Text routing** — 5 | M/E.
schemaValidateSource_error1/2, schemaValidate_error1/2, schemaValidate_error5 (I/O + locate resource warn text) — root: engine schema
`xmlSchema` parse-error routing `xmlSchemaValidCtxt` `xmlSchemaValidateDoc` message ("-1 element") + file-path
in error.
V2 **validate external DTD / on-parse** — 2 | M. `DOMDocument_validate_external_dtd`(false vs true), `validate_on_parse_variation`(no warnings under validateOnParse TRUE) — root: internal-subset
DTD-presence `xmlValidateDtd` on DTD-loaded load; and `check_standalone`/`validateOnParse` flag not mapping engine `XML_PARSE_DTDVALID`.
V3 **relaxNG ParseElement-no-content + pear-node error parity** — 4 | E. relaxNGValidate*_error1&2/Source — root: relaxng-error table
`xmlRelaxNGParse` messages that SP-14.3.1-9 error-table rows also pull (mustn't regress).

## ERR / val at dom boundary (message-text only)
E1 **dom/dom-doc name/loader err text: unsupported version, DTD content-model mismatch warnings** — 8 | E.
DOMDocument_loadXML_error{1,2}4/_gte2_12, load_error{...} same: `Opening/end tag mismatch`, `contents does not
follow the DTD expecting (…) got (…)`, `nofollow version`, entity-not-defined. Root: parser-side message parity
`xmlParseElementDecl`/`xmlParseElement` (xml parser) — duplicates upstream parser message rows; strictly a
message-format family (safe to fix after true-parity engine concerns since tests grep `%s` loose).

### Sanity note
Two head entries could not be individually verified against a diff during this pass (only PASS/FAIL corpus +
crash-list): `modern/extensions/Element_substitutedNodeValue` (SIG from its own ext) and `Element_getElementsByClassName/*`
(XML-tree `getElementsByClassName` w/ noscript comment read) → **unverified** but most likely in S1/ns-M1.
Their membership is named so a later diff-read can reassign.

---

## Ordering WITHIN dom (prereq-root-cause dependency)
1. O1 crash/ownership (frees/double-free are the reason any failed save aborts the run); land the 
   index/node lifetime fix + the O1-signaled xmlsave free.
2. F1 fragment inner/outer well-formedness (incl content `<!` rule) — genuinely must precede S1 and every 
   inner/outerHTML writing/serialize greenup; also unblocks ext/xml SP14.3.1-8 without dom-regression.
3. N1 namespace/prefix reconciliation (fresh-prefix allocation + default-ns decl bounds) → gates S1-html-ns,
   M1 clone/import, and the whole html serializer.
4. D1 DTD/entity decl model (sp-S1-NOENT & `, ` content + `publicId`) → gates dom001/dom005 + token_list/entities.
5. L2/encoding + L1 parse-options (RECOVER/NO_XXE/NOENT) + domoption dt (in-silence no-regress xml bits).
6. S1/+S2 XML/HTML serializer hardening (decl/DOCTYPE/empty-form/`&#x;` + standalone) after ns+enc stable.
7. M1 isEqual/import/clone textContent.
8. V1→V2→V3 validation; E1 error-message-parity last (cheap at end).

**Cross-extension must-not-regress:** ext/xml `xml_set_*`/push `bug27908/gh20439_*` (same parser push/EOF+
`<!`-content branch: F1 must be merged here so they stay green) and ext/xmlreader + ext/simplexml
(same RECOVER/NO_XXE/encoding + NOENT attr token flows in L1). Any engine change touching
`parser/state`+`tokenizer` EOF must re-run dom f1 + ext/xml entire XML_OPTION/attr cluster.
