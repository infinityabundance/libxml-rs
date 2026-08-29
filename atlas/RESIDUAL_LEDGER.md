# Residual Ledger

Per §71: every unexplained difference gets an ID (`R-000001`...), and its
history is retained after fixing. This Markdown is generated from
`RESIDUAL_LEDGER.json` by `tools/evidence/ledger_gen.py` (§70 policy:
Markdown generated from JSON; the JSON is the only hand-maintained truth).

## Current Residuals

**8 open residuals:** R-000119, R-000120, R-000121, R-000122, R-000123, R-000131, R-000136, R-000138

## Phase 0 Residuals

### R-000001: #line directive mapping uses wrong coordinate space (FIXED)

- **Status:** FIXED (2026-08-20, Phase 0)
- **Component:** tools/archaeology/apiatlas.py
- **Surface:** tooling
- **Root cause:** The resolve_origin function used the original source line number from #line directives as dict keys. When multiple directives shared the same original line number, they overwrote each other, causing incorrect file attribution. Furthermore, the #line mapping approach is fundamentally flawed because loc.line from clang's AST is in the original source file coordinate space, while #line directive positions are in the preprocessed output coordinate space — these are different and cannot be compared directly.
- **Fix:** Replaced the #line mapping approach entirely. The new resolve_origin uses clang's AST location fields directly: (1) loc.file for type declarations from included files, (2) loc.includedFrom presence to detect function declarations from included files (filtered out — they'll be captured when their own header is processed), (3) absence of both for direct declarations in the main file.
- **Evidence:** 45 system functions leaked into header inventory; most HTML functions (44+) were missing; tree functions were missing.
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-08-20 (discovered during Phase 0 header census); FIXED 2026-08-20 (resolve_origin rewritten)

### R-000002: System header path filtering missing (FIXED)

- **Status:** FIXED (2026-08-20, Phase 0)
- **Component:** tools/archaeology/apiatlas.py
- **Surface:** tooling
- **Root cause:** No explicit filter for system header paths in the collect() function. When resolve_origin returned None (for declarations from included files), the caller didn't handle it.
- **Fix:** Added None check for origin, and added a comprehensive system function name denylist (SYSTEM_FUNCTION_NAMES) as a secondary defense for declarations that bypass origin-based filtering.
- **Evidence:** 45 system functions (fopen, fprintf, printf, etc.) in header inventory.
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-08-20; FIXED 2026-08-20

### R-000003: Internal __xml* functions not classified (FIXED)

- **Status:** FIXED (2026-08-20, Phase 0)
- **Component:** tools/archaeology/symbols.py
- **Surface:** tooling
- **Root cause:** Internal __xml* function declarations (the implementations behind public function-pointer variables) were listed alongside potentially missing API functions.
- **Fix:** Added INTERNAL_FUNCTIONS set and separate reporting in internal_functions field.
- **Evidence:** 6 __xml* functions now correctly classified.
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-08-20; FIXED 2026-08-20

### R-000004: SAX1 callback names not classified (FIXED)

- **Status:** FIXED (2026-08-20, Phase 0)
- **Component:** tools/archaeology/symbols.py
- **Surface:** tooling
- **Root cause:** SAX1 callback struct field names appeared in DSO symbol tables as OBJECT type but were listed as undocumented function exports.
- **Fix:** Added SAX1_CALLBACK_NAMES set and separate reporting in sax1_callbacks field.
- **Evidence:** 24 SAX1 callback names now correctly classified.
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-08-20; FIXED 2026-08-20

### R-000005: XML_TREE_INTERNALS not defined when processing tree.h (FIXED)

- **Status:** FIXED (2026-08-20, Phase 0)
- **Component:** tools/archaeology/apiatlas.py
- **Surface:** tooling
- **Root cause:** tree.h has a circular dependency workaround: when XML_TREE_INTERNALS is not defined, it just includes parser.h and hides all tree declarations. Other headers (parser.h, entities.h, valid.h, xmlIO.h) define this before including tree.h, but when processing tree.h directly, the define was missing.
- **Fix:** Added -DXML_TREE_INTERNALS to clang include args globally.
- **Evidence:** tree.h showed 0 FunctionDecl declarations; xmlAddChild and all tree functions were missing from the API inventory.
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-08-20; FIXED 2026-08-20

## Phase 8 Residuals

### R-000101: Parser did not resolve element/attribute namespaces (FIXED)

- **Status:** FIXED (2026-08-28, Phase 8)
- **Component:** src/xml/parser/state.rs
- **Surface:** parser / namespaces
- **Oracle versions:** libxml2 2.x (SAX2 namespace processing is core behavior)
- **Root cause:** sax_start_element passed the raw qualified name (e.g. xsl:stylesheet) as the SAX2 localname and always passed NULL prefix/URI, so the default tree builder never attached namespace pointers to elements or attributes. XSLT compilation therefore could not recognize xsl: instructions.
- **Fix:** Split element QNames into prefix + localname, resolve prefixes against the element's namespace declarations (with the xml prefix special case), and pass resolved prefix/URI/namespace arrays to the SAX2 dispatcher.
- **Regression courts:** XSLT end-to-end transform tests (test_end_to_end_*, test_xslt_*).
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-08-28; FIXED 2026-08-28 (SAX2 namespace resolution)

### R-000102: XPath absolute paths evaluated from the root element (FIXED)

- **Status:** FIXED (2026-08-28, Phase 8)
- **Component:** src/xml/xpath/eval.rs
- **Surface:** XPath
- **Root cause:** eval_absolute_path searched the document's children for a type-9 node (which never exists as a child), falling back to the root element. /root/item therefore looked for root inside the root element and returned empty.
- **Fix:** The context node for absolute paths is now the document node itself (doc as *mut _xmlNode), matching XPath 1.0 / semantics.
- **Regression courts:** test_end_to_end_template_transform, test_xslt_variable_and_call_template.
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-08-28; FIXED 2026-08-28

### R-000103: Template content double-freed with stylesheet document (FIXED)

- **Status:** FIXED (2026-08-28, Phase 8)
- **Component:** src/xslt/templates/mod.rs
- **Surface:** memory / ownership
- **Root cause:** xsltFreeTemplate freed the template's content tree, but template content nodes are owned by the stylesheet document (style->doc) and were freed a second time by xsltFreeStylesheet's xmlFreeDoc.
- **Fix:** xsltFreeTemplate no longer frees the content tree (matching upstream libxslt); the document owns those nodes. The template's heap-copied name/mode strings are freed.
- **Regression courts:** test_parse_stylesheet_memory.
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-08-28; FIXED 2026-08-28 (ownership corrected to match upstream)

### R-000104: Result document version/encoding strings double-freed (FIXED)

- **Status:** FIXED (2026-08-28, Phase 8)
- **Component:** src/xslt/transform/mod.rs
- **Surface:** memory / ownership
- **Root cause:** xsltApplyStylesheetUser pointed the result document's version at a static literal and copied the stylesheet's encoding/version pointers; free_doc frees those fields with xmlFree, causing invalid frees / double frees.
- **Fix:** The result document's version/encoding are heap-copied with xml_strdup.
- **Regression courts:** test_end_to_end_*.
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-08-28; FIXED 2026-08-28

### R-000105: node()/text() etc. parsed as function calls (FIXED)

- **Status:** FIXED (2026-08-28, Phase 8)
- **Component:** src/xslt/patterns/mod.rs
- **Surface:** XSLT patterns
- **Root cause:** Top-level node(), text(), comment(), processing-instruction() parse as FunctionCall nodes in the XPath AST, so collect_steps/compute_expr_priority treated them as unknown (priority 0.5, no match).
- **Fix:** Translate bare node-test function calls into steps (child axis + node test) in collect_steps and map their priorities in compute_expr_priority (-0.25 for node(), 0.0 for the others).
- **Regression courts:** pattern priority and compile tests.
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-08-28; FIXED 2026-08-28

### R-000106: match="/" matched the root element (FIXED)

- **Status:** FIXED (2026-08-28, Phase 8)
- **Component:** src/xslt/patterns/mod.rs
- **Surface:** XSLT patterns
- **Root cause:** "/" parses as a bare Self_/node() step; the matcher treated it as matching any node, so the root template also matched the root element.
- **Fix:** A bare Self_/node() step with no predicates and no other steps represents the document root pattern: empty steps with is_absolute=true, matching only document nodes.
- **Regression courts:** test_end_to_end_simplified_stylesheet.
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-08-28; FIXED 2026-08-28

## Phase 9 Residuals

### R-000107: XPath core functions not registered in the XSLT transform context (FIXED)

- **Status:** FIXED (2026-08-29, Phase 9)
- **Component:** src/xslt/transform/mod.rs
- **Surface:** XSLT / XPath integration
- **Oracle versions:** libxslt 1.1.x (core XPath functions always available during transforms)
- **Root cause:** register_xslt_functions registered only the XSLT extension functions (document(), key(), …) and EXSLT, never the XPath 1.0 core function library. Every XPath function call (count(library/book), substring('hello',1,2), …) failed evaluation with an unknown-function error, surfacing as 'Invalid expression' and empty output.
- **Fix:** Register crate::xml::xpath::functions::core_functions() in the transform context before the XSLT-specific functions.
- **Regression courts:** test_xslt_core_functions_in_value_of.
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-08-29; FIXED 2026-08-29

### R-000108: Attribute value templates never evaluated (FIXED)

- **Status:** FIXED (2026-08-29, Phase 9)
- **Component:** src/xslt/transform/mod.rs
- **Surface:** XSLT transform
- **Oracle versions:** libxslt 1.1.x (XSLT 1.0 §7.6.2)
- **Root cause:** Literal result element attributes were copied verbatim (id="{@id}" appeared literally in the output); xsl:element/xsl:attribute/xsl:processing-instruction names were not AVT-evaluated either.
- **Fix:** Implemented eval_avt ({{/}} escapes, {expr} evaluation via eval_xpath, unmatched { literal) and wired it into literal attributes and the name/namespace attributes of xsl:element, xsl:attribute, and xsl:processing-instruction.
- **Regression courts:** test_xslt_avt_in_literal_attribute, test_xslt_avt_in_xsl_element_name.
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-08-29; FIXED 2026-08-29

### R-000109: RTF variable tree double-freed; exsl:node-set unsupported (FIXED)

- **Status:** FIXED (2026-08-29, Phase 9)
- **Component:** src/xslt/variables/mod.rs, src/xslt/compiler/mod.rs, src/xslt/documents/mod.rs
- **Surface:** memory / ownership; EXSLT
- **Oracle versions:** libxslt 1.1.x (RVT semantics, exsltCommon.c)
- **Root cause:** (1) compile_variable set var->tree = inst->children — nodes owned by the stylesheet document; xsltFreeStackElem freed them, and xsltFreeStylesheet freed them again (double-free / heap corruption). (2) Inline variable content was flattened to a string, so exsl:node-set($var)/path navigation returned nothing.
- **Fix:** xsltFreeStackElem no longer frees the stylesheet-owned tree. register_global_value deep-copies inline content into a context-owned RVT document registered in the docCache (freed exactly once at context teardown, after the XPath context) and binds the variable to a node-set containing the RVT document node — matching upstream xmlXPathNewValueTree.
- **Regression courts:** test_xslt_variable_inline_content_rtf, test_xslt_exsl_node_set_on_rtf.
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-08-29; FIXED 2026-08-29

### R-000110: node_get_content ignored descendant text (FIXED)

- **Status:** FIXED (2026-08-29, Phase 9)
- **Component:** src/xml/tree/mod.rs
- **Surface:** tree / XPath string-value
- **Oracle versions:** libxml2 2.x (xmlNodeGetContent)
- **Root cause:** For element nodes, node_get_content concatenated only direct text/CDATA children, so <book><title>Rust</title></book> had an empty string-value and <library>…</library> only whitespace.
- **Fix:** Recurse into element children so the string-value is the concatenation of all descendant text nodes (XPath 1.0 §4.2 semantics).
- **Regression courts:** test_node_get_content_recurses_descendants, test_xslt_core_functions_in_value_of.
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-08-29; FIXED 2026-08-29

### R-000111: Caller parameters parsed as name=value single strings (FIXED)

- **Status:** FIXED (2026-08-29, Phase 9)
- **Component:** src/xslt/parameters/mod.rs
- **Surface:** XSLT parameters
- **Oracle versions:** libxslt 1.1.x (xsltEvalUserParams, variables.c)
- **Root cause:** xsltParseStylesheetParams parsed the params array as single "name=value" strings, but upstream passes a NULL-terminated array of (name, value) pairs where the value is an XPath expression evaluated later.
- **Fix:** Parsed as (name, value) pairs; xsltParseStylesheetParam takes separate name/value arguments with {uri}name namespace support; values bound with XSLT_VAR_PARAM | XSLT_VAR_INTERNAL.
- **Regression courts:** test_parse_params_array_pairs, xslt::parameters::tests.
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-08-29; FIXED 2026-08-29

### R-000112: date:date()/date:time() no-argument default missing (FIXED)

- **Status:** FIXED (2026-08-29, Phase 9)
- **Component:** src/exslt/dates/mod.rs
- **Surface:** EXSLT dates
- **Oracle versions:** libxslt 1.1.x (EXSLT dates spec)
- **Root cause:** date_arg returned None for a missing argument, so no-argument calls to date:date()/date:time()/date:year() etc. returned the empty string instead of operating on the current date-time.
- **Fix:** No-argument calls default to now() (matching EXSLT dates spec and upstream dateArg).
- **Regression courts:** exslt::dates::tests.
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-08-29; FIXED 2026-08-29

### R-000113: xsl:if/xsl:when boolean conversion read only boolval (FIXED)

- **Status:** FIXED (2026-08-29, Phase 9)
- **Component:** src/xslt/transform/mod.rs
- **Surface:** XSLT transform
- **Oracle versions:** libxslt 1.1.45 (differential xsltproc corpus)
- **Root cause:** process_if/process_choose tested (*obj).boolval, which is only valid for boolean objects; node-set tests (test="author"), numbers, and strings always converted to false.
- **Fix:** Added xpath_obj_boolean applying XPath 1.0 §4.3 boolean conversion (node-set non-empty, number non-zero/non-NaN, string non-empty).
- **Regression courts:** test_xslt_if_node_set_test, CLI-XSLTPROC-0006.
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-08-29 (differential oracle discovery); FIXED 2026-08-29

### R-000114: Attribute string-value empty in the XPath engine (FIXED)

- **Status:** FIXED (2026-08-29, Phase 9)
- **Component:** src/xml/xpath/types.rs
- **Surface:** XPath string-value
- **Oracle versions:** libxslt 1.1.45 / libxml2 2.15.3 (differential corpus)
- **Root cause:** node_string_value treated type 13 as attribute — but 13 is XML_HTML_DOCUMENT_NODE; attributes are type 2 and their value lives in the first text child (not content). Every string(@attr), @attr='x' predicate, and attribute-based sort returned empty.
- **Fix:** Attributes (type 2) read the first text child; type 13 handled as a document node (descendant text).
- **Regression courts:** test_xslt_attribute_string_value, CLI-XSLTPROC-0004, CLI-XSLTPROC-0005.
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-08-29 (differential oracle discovery); FIXED 2026-08-29

### R-000115: xsl:sort never compiled or applied (FIXED)

- **Status:** FIXED (2026-08-29, Phase 9)
- **Component:** src/xslt/transform/mod.rs, src/xslt/sorting/mod.rs
- **Surface:** XSLT sorting
- **Oracle versions:** libxslt 1.1.45 (differential corpus sort.xsl)
- **Root cause:** (1) find_sort_children always passed ptr::null_mut() as the stylesheet, so xsltCompileSort bailed and no sort was applied. (2) xsltEvalSortKey evaluated the sort key expression without setting the internal XPath context node, so all keys evaluated against the wrong context and compared equal.
- **Fix:** Pass (*ctxt).style into xsltCompileSort; set both the C-struct and internal XPath context node in xsltEvalSortKey.
- **Regression courts:** test_xslt_sort_descending, CLI-XSLTPROC-0008.
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-08-29 (differential oracle discovery); FIXED 2026-08-29

### R-000116: key() XPath function was a stub (FIXED)

- **Status:** FIXED (2026-08-29, Phase 9)
- **Component:** src/xslt/transform/mod.rs, src/xslt/keys/mod.rs, src/abi/exports_xml2.rs
- **Surface:** XSLT keys
- **Oracle versions:** libxslt 1.1.45 (differential corpus keys.xsl)
- **Root cause:** (1) The registered key() closure returned an empty node-set; the real xsltEvalKeyFunction was never bridged (no transform-context pointer reachable from the XPath function). (2) build_key_table evaluated the use expression without setting the internal XPath context node, so key values were empty strings.
- **Fix:** Stash the transform context in the internal XPath context's func_lookup_data slot; the key() closure calls xsltEvalKeyFunction (string value of the first node when the value is a node-set). Set the internal context node in build_key_table. Exported xmlXPathFreeNodeSet (missing ABI surface).
- **Regression courts:** test_xslt_key_function, CLI-XSLTPROC-0009.
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-08-29 (differential oracle discovery); FIXED 2026-08-29

### R-000117: Local variables/params invisible to XPath evaluation (FIXED)

- **Status:** FIXED (2026-08-29, Phase 9)
- **Component:** src/xslt/variables/mod.rs, src/xslt/parameters/mod.rs, src/xslt/transform/mod.rs, src/abi/exports_xml2.rs
- **Surface:** XSLT variables & parameters
- **Oracle versions:** libxslt 1.1.45 (differential corpus ct.xsl)
- **Root cause:** Local xsl:variable/xsl:param and xsl:with-param were pushed onto the transform variable/parameter stacks only — the XPath evaluator reads the internal XPathContext.variables hash, so $name resolved empty. process_param checked the wrong stack for passed values.
- **Fix:** xsltPushVariable/xsltPushParam register their values in the XPath context hash (unregistered on pop); process_param consults the hash; object_to_xpathvalue handles XPATH_XSLT_TREE (RTF → node-set of the document node) so local RTF variables stringify and remain navigable.
- **Regression courts:** test_xslt_call_template_with_params, CLI-XSLTPROC-0010.
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-08-29 (differential oracle discovery); FIXED 2026-08-29

### R-000118: HTML output method differed from upstream (FIXED)

- **Status:** FIXED (2026-08-29, Phase 9)
- **Component:** src/xml/html/mod.rs
- **Surface:** XSLT serialization (method="html")
- **Oracle versions:** libxslt 1.1.45 (differential corpus html.xsl)
- **Root cause:** The HTML serializer (1) never inserted <meta charset="..."> into the root <head> and (2) used two-space indentation, while upstream htmlNodeDumpFormatOutput writes newlines only.
- **Fix:** Insert <meta charset="ENCODING"> as the first child of a root <head> lacking a <meta> element (encoding from the document, default UTF-8); formatting writes newlines without indentation spaces.
- **Regression courts:** test_xslt_html_method_meta_charset, CLI-XSLTPROC-0011.
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-08-29 (differential oracle discovery); FIXED 2026-08-29

## Phase 10 Residuals

### R-000119: Entity content children not built at reference time (OPEN)

- **Status:** OPEN
- **Component:** src/xml/parser/state.rs, src/xml/debug/mod.rs
- **Surface:** DTD/entity debug dumps
- **Oracle versions:** libxml2 2.15.3 (xmllint --debug on entity-containing documents)
- **Root cause:** Upstream parses a referenced entity's content into ent->children (xmlCtxtParseEntity) and xmlCtxtDumpEntityDecl dumps that tree; our entity declarations store only the raw content string, so the debug dump synthesizes a TEXT compact node for plain content and nothing for markup content. The document tree, serialization and XPath are unaffected (the --noent re-parse path builds the correct in-document nodes).
- **Observable residual:** xmllint --debug on a document that references an entity whose content contains markup shows the raw content= line but not the parsed child element under ENTITYDECL.
- **Phase 11 triangulation:** E-004 (atlas/SEMANTIC_EPOCHS.md): the historical matrix shows the entity-content child node changed TEXT → TEXT compact at 2.13.0 (commit 8d04f0ee "tree: Refactor text node updates", first release v2.13.0). The crate's synthesized TEXT compact node therefore matches the current (2.13.0+) epoch, i.e. the 2.15.3 system oracle, not the pre-2.13 behavior. The remaining gap (markup entity content not parsed into children) is unchanged in every upstream version from 2.7.8 to 2.15.3.
- **Regression courts:** CLI-XMLLINT-0032.
- **Classification:** CANDIDATE_BUG

### R-000120: Entity-containing attribute values marked compact in --debug (OPEN)

- **Status:** OPEN
- **Component:** src/xml/parser/state.rs, src/xml/sax/default.rs
- **Surface:** debug dumps
- **Oracle versions:** libxml2 2.15.3 (xmllint --debug on <a p="AT&amp;T"/>)
- **Root cause:** Upstream attribute values containing entity/character references take the xmlNodeParseAttValue path and are never compact; our tokenizer decodes references before the SAX layer (substitute_refs), losing the "had references" signal, so short decoded values are marked compact.
- **Observable residual:** TEXT compact vs upstream TEXT for entity-containing attribute values in --debug output. Content, serialization and XPath results are identical.
- **Phase 11 triangulation:** The matrix's attr-entity case (<a p="AT&amp;T">) is byte-identical across the entire 2.7.8 → 2.15.3 span — upstream never changed this observable. The crate's compact marking of entity-containing attribute values is a divergence from every epoch, not a version drift.
- **Regression courts:** CLI-XMLLINT-0033.
- **Classification:** CANDIDATE_BUG

### R-000121: '<' in entity ... is not allowed in attributes values reported once (OPEN)

- **Status:** OPEN
- **Component:** src/xml/parser/state.rs
- **Surface:** parser diagnostics
- **Oracle versions:** libxml2 2.15.3 (xmllint on a document referencing a markup entity in an attribute value)
- **Root cause:** Upstream reports the XML_ERR_LT_IN_ATTRIBUTE fatal error twice (parser + validation paths) with the caret at the &; ours reports it once with the caret past the start tag. The message text and exit code (4) match.
- **Observable residual:** Single diagnostic vs upstream's double diagnostic; caret column differs by one.
- **Phase 11 triangulation:** E-005 (atlas/SEMANTIC_EPOCHS.md): the matrix's attr-markup-entity case shows a real upstream epoch: reported once with exit 1 from 2.7.8 → 2.12.6, reported twice with exit 4 from 2.13.0 → 2.15.3 (boundary pinned to 2.13.0; correlates with NEWS 2.13.0 "xmllint: Rework parsing"/error consolidation). The crate's single report is the pre-2.13 epoch while its exit code 4 is the 2.13+ epoch — a hybrid of two epochs. The caret column differs from all upstream versions (ours points one column right of upstream's).
- **Regression courts:** CLI-XMLLINT-0034.
- **Classification:** CANDIDATE_BUG

### R-000122: xmlcatalog: option parsing does not stop at the first non-option argument (OPEN)

- **Status:** OPEN
- **Component:** src/bin/xmlcatalog.rs
- **Surface:** cli-xmlcatalog option parsing
- **Oracle versions:** libxml2 2.15.3 (CLI-XMLCATALOG-0002, promoted Phase 10 differential suite)
- **Root cause:** Upstream xmlcatalog.c parses options in a loop that breaks at the first non-option argument (if (argv[i][0] != '-') break;). With '--create FILE --noout' the trailing --noout is therefore never parsed as an option: it becomes a resolution operand against the freshly created catalog (upstream prints 'No entry for SYSTEM --noout' + 'No entry for URI --noout', still dumps the catalog because noout was never set, exit 4). Our parser recognizes --noout anywhere in argv, so we suppress the dump and exit 0.
- **Observable residual:** xmlcatalog --create FILE --noout: upstream exit 4 with two 'No entry' diagnostics and a dumped catalog; ours exit 0 with no dump.
- **Regression courts:** CLI-XMLCATALOG-0002.
- **Classification:** CANDIDATE_BUG

### R-000123: xmlcatalog shell 'public' command accepts wrong argument count (OPEN)

- **Status:** OPEN
- **Component:** src/bin/xmlcatalog.rs
- **Surface:** cli-xmlcatalog shell
- **Oracle versions:** libxml2 2.15.3 (CLI-XMLCATALOG-0010, promoted Phase 10 differential suite)
- **Root cause:** Upstream xmlcatalog.c shell command 'public' validates argument count: 'public requires 1 arguments' when the command is not given exactly one argument. Our shell treats the first token as the public identifier and performs a lookup, producing 'No entry for PUBLIC ...' instead.
- **Observable residual:** xmlcatalog --shell with 'public -//OASIS//DTD X//EN': upstream errors 'public requires 1 arguments'; ours answers 'No entry for PUBLIC -//OASIS//DTD'.
- **Regression courts:** CLI-XMLCATALOG-0010.
- **Classification:** CANDIDATE_BUG

## Classification Legend

- `CANDIDATE_BUG` — see classification policy in §45/§71
- `INTENTIONAL_SAFE_DIVERGENCE` — see classification policy in §45/§71
- `ORACLE_BUG` — see classification policy in §45/§71
- `UNRESOLVED` — see classification policy in §45/§71
- `VERSION_DIFFERENCE` — see classification policy in §45/§71
