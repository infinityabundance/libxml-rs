# Residual Ledger

Per §71: every unexplained difference gets an ID (`R-000001`...), and its
history is retained after fixing. This Markdown is generated from
`RESIDUAL_LEDGER.json` by `tools/evidence/ledger_gen.py` (§70 policy:
Markdown generated from JSON; the JSON is the only hand-maintained truth).

## Current Residuals

**2 open residuals:** R-000168, R-000179

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

### R-000158: xsltproc corpus ct.xsl: call-template with a node-set with-param value hangs the transform engine (FIXED)

- **Status:** FIXED (, Phase 9)
- **Component:** src/xslt/transform/mod.rs
- **Surface:** xsl:call-template / xsl:with-param (transform engine, src/xslt/transform)
- **Oracle versions:** libxslt 1.1.45 (system xsltproc)
- **Root cause:** Pre-existing Phase 9 engine defect (transform/mod.rs unchanged since commit 9b8a2233; this session only added visibility modifiers and ABI exports, neither of which the CLI calls). Passing a node-set expression (//book[1]/title) as a with-param value to a named template drives the engine into an unbounded loop. The in-crate unit test test_xslt_call_template_with_params passes because it uses string params only.
- **Observable residual:** xsltproc ct.xsl doc.xml on the candidate never terminates (system oracle completes in ms).
- **Fix:** 11.1-X: process_call_template snapshots the with-param list before pushing (xsltPushVariable rewires the next pointers, so iterating while pushing corrupts the list) and pops back to the saved varsNr instead of a fixed param count. The engine terminates on ct.xsl (CLI-XSLTPROC-0010).
- **Phase 11 triangulation:** The CLI-XSLTPROC corpus was scaffolded in Phase 9 but never diff-verified in-repo: every receipt is UNKNOWN because the Docker oracle was never built; this hang is one of the corpus gaps.
- **Regression courts:** CLI-XSLTPROC-0010.
- **Evidence:** ['courts/corpus/cli/xslt/ct.xsl']
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-08-30; FIXED 2026-08-31 (fixed in the 11.1-X residual closure loop; CLI-XSLTPROC-0010 byte-identical (57/57 CLI courts PASS))

### R-000159: xsltproc corpus pred.xsl: //book[position() <= 2] matches one extra node (FIXED)

- **Status:** FIXED (, Phase 9)
- **Component:** src/xslt/transform/mod.rs, src/xml/xpath
- **Surface:** XPath position() inside xsl:for-each predicates (transform engine)
- **Oracle versions:** libxslt 1.1.45 (system xsltproc)
- **Root cause:** Pre-existing Phase 9 engine defect: the candidate produces <b pos="3"> for //book[position() <= 2], i.e. position() evaluates against a different context size than the selected node-set. Same provenance as R-000158 (corpus never diff-verified; engine unchanged this session).
- **Observable residual:** One extra node in the for-each result versus the system oracle.
- **Fix:** 11.1-X: XPath position() now reads the proximity position member set by the step evaluation; both predicate loops (main and axis walk) set/restore proximity_position so //book[position() <= 2] selects exactly the oracle node set (CLI-XSLTPROC-0004).
- **Phase 11 triangulation:** Corpus gap: no CLI-XSLTPROC receipt ever recorded PASS.
- **Regression courts:** CLI-XSLTPROC-0004.
- **Evidence:** ['courts/corpus/cli/xslt/pred.xsl']
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-08-30; FIXED 2026-08-31 (fixed in the 11.1-X residual closure loop; CLI-XSLTPROC-0004 byte-identical (57/57 CLI courts PASS))

## Phase 10 Residuals

### R-000119: Entity content children not built at reference time (FIXED)

- **Status:** FIXED (, Phase 10)
- **Component:** src/xml/parser/state.rs, src/xml/debug/mod.rs
- **Surface:** DTD/entity debug dumps
- **Oracle versions:** libxml2 2.15.3 (xmllint --debug on entity-containing documents)
- **Root cause:** Upstream parses a referenced entity's content into ent->children (xmlCtxtParseEntity) and xmlCtxtDumpEntityDecl dumps that tree; our entity declarations store only the raw content string, so the debug dump synthesizes a TEXT compact node for plain content and nothing for markup content. The document tree, serialization and XPath are unaffected (the --noent re-parse path builds the correct in-document nodes).
- **Observable residual:** xmllint --debug on a document that references an entity whose content contains markup shows the raw content= line but not the parsed child element under ENTITYDECL.
- **Fix:** 11.1-X: the debug dump path now materialises referenced entity content into ent->children (parser/state.rs entity-content handling with XML_PARSE_COMPACT TEXT nodes under the 2.13+ epoch) and the DTD debug dump renders the parsed child tree (xmlDebugDumpNode no longer double-recurses the DTD). CLI-XMLLINT-0033/0034 regress the observable.
- **Phase 11 triangulation:** E-004 (atlas/SEMANTIC_EPOCHS.md): the historical matrix shows the entity-content child node changed TEXT → TEXT compact at 2.13.0 (commit 8d04f0ee "tree: Refactor text node updates", first release v2.13.0). The crate's synthesized TEXT compact node therefore matches the current (2.13.0+) epoch, i.e. the 2.15.3 system oracle, not the pre-2.13 behavior. The remaining gap (markup entity content not parsed into children) is unchanged in every upstream version from 2.7.8 to 2.15.3.
- **Regression courts:** CLI-XMLLINT-0033, CLI-XMLLINT-0034.
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-08-29 (discovered Phase 10 differential suite; epoch-triangulated Phase 11); FIXED 2026-08-31 (fixed in the 11.1-X residual closure loop; CLI-XMLLINT-0033/0034 byte-identical (57/57 CLI courts PASS))

### R-000120: Entity-containing attribute values marked compact in --debug (FIXED)

- **Status:** FIXED (, Phase 10)
- **Component:** src/xml/parser/state.rs, src/xml/sax/default.rs
- **Surface:** debug dumps
- **Oracle versions:** libxml2 2.15.3 (xmllint --debug on <a p="AT&amp;T"/>)
- **Root cause:** Upstream attribute values containing entity/character references take the xmlNodeParseAttValue path and are never compact; our tokenizer decodes references before the SAX layer (substitute_refs), losing the "had references" signal, so short decoded values are marked compact.
- **Observable residual:** TEXT compact vs upstream TEXT for entity-containing attribute values in --debug output. Content, serialization and XPath results are identical.
- **Fix:** 11.1-X: attribute values that contained entity/character references are no longer marked compact. The tokenizer StartTag token now carries attr_start so per-attribute reference presence is signalled to the SAX layer via a non-NULL valueEnd; parser_new_text_node gained force_noncompact and parser_set_prop a had_ref flag. CLI-XMLLINT-0033 regresses the observable.
- **Phase 11 triangulation:** The matrix's attr-entity case (<a p="AT&amp;T">) is byte-identical across the entire 2.7.8 → 2.15.3 span — upstream never changed this observable. The crate's compact marking of entity-containing attribute values is a divergence from every epoch, not a version drift.
- **Regression courts:** CLI-XMLLINT-0033.
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-08-29 (discovered Phase 10 differential suite); FIXED 2026-08-31 (fixed in the 11.1-X residual closure loop; CLI-XMLLINT-0033 byte-identical (57/57 CLI courts PASS))

### R-000121: '<' in entity ... is not allowed in attributes values reported once (FIXED)

- **Status:** FIXED (, Phase 10)
- **Component:** src/xml/parser/state.rs
- **Surface:** parser diagnostics
- **Oracle versions:** libxml2 2.15.3 (xmllint on a document referencing a markup entity in an attribute value)
- **Root cause:** Upstream reports the XML_ERR_LT_IN_ATTRIBUTE fatal error twice (parser + validation paths) with the caret at the &; ours reports it once with the caret past the start tag. The message text and exit code (4) match.
- **Observable residual:** Single diagnostic vs upstream's double diagnostic; caret column differs by one.
- **Fix:** 11.1-X: substitute_refs now scans the entire entity value for '<' from the value start position and reports XML_ERR_LT_IN_ATTRIBUTE twice (parser + validation paths) with the caret at the '&' when XML_PARSE_NOENT is not set, once with --noent — matching the 2.13+ epoch (E-005). CLI-XMLLINT-0034 regresses the observable.
- **Phase 11 triangulation:** E-005 (atlas/SEMANTIC_EPOCHS.md): the matrix's attr-markup-entity case shows a real upstream epoch: reported once with exit 1 from 2.7.8 → 2.12.6, reported twice with exit 4 from 2.13.0 → 2.15.3 (boundary pinned to 2.13.0; correlates with NEWS 2.13.0 "xmllint: Rework parsing"/error consolidation). The crate's single report is the pre-2.13 epoch while its exit code 4 is the 2.13+ epoch — a hybrid of two epochs. The caret column differs from all upstream versions (ours points one column right of upstream's).
- **Regression courts:** CLI-XMLLINT-0034.
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-08-29 (discovered Phase 10 differential suite; epoch-triangulated Phase 11); FIXED 2026-08-31 (fixed in the 11.1-X residual closure loop; CLI-XMLLINT-0034 byte-identical (57/57 CLI courts PASS))

### R-000122: xmlcatalog: option parsing does not stop at the first non-option argument (FIXED)

- **Status:** FIXED (, Phase 10)
- **Component:** src/bin/xmlcatalog.rs
- **Surface:** cli-xmlcatalog option parsing
- **Oracle versions:** libxml2 2.15.3 (CLI-XMLCATALOG-0002, promoted Phase 10 differential suite)
- **Root cause:** Upstream xmlcatalog.c parses options in a loop that breaks at the first non-option argument (if (argv[i][0] != '-') break;). With '--create FILE --noout' the trailing --noout is therefore never parsed as an option: it becomes a resolution operand against the freshly created catalog (upstream prints 'No entry for SYSTEM --noout' + 'No entry for URI --noout', still dumps the catalog because noout was never set, exit 4). Our parser recognizes --noout anywhere in argv, so we suppress the dump and exit 0.
- **Observable residual:** xmlcatalog --create FILE --noout: upstream exit 4 with two 'No entry' diagnostics and a dumped catalog; ours exit 0 with no dump.
- **Fix:** 11.1-X: xmlcatalog option parsing now stops at the first non-option argument (upstream 'if (argv[i][0] != '-') break;'). With '--create FILE --noout' the trailing --noout is resolved as an entity and the catalog is dumped (exit 4); the query loop runs unconditionally and the dump is gated on modified||create exactly as upstream.
- **Regression courts:** CLI-XMLCATALOG-0002.
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-08-29 (discovered by 11.1-A evidence promotion (target/difftest.sh -> committed CLI court)); FIXED 2026-08-31 (fixed in the 11.1-X residual closure loop; CLI-XMLCATALOG-0002 byte-identical (57/57 CLI courts PASS))

### R-000123: xmlcatalog shell 'public' command accepts wrong argument count (FIXED)

- **Status:** FIXED (, Phase 10)
- **Component:** src/bin/xmlcatalog.rs
- **Surface:** cli-xmlcatalog shell
- **Oracle versions:** libxml2 2.15.3 (CLI-XMLCATALOG-0010, promoted Phase 10 differential suite)
- **Root cause:** Upstream xmlcatalog.c shell command 'public' validates argument count: 'public requires 1 arguments' when the command is not given exactly one argument. Our shell treats the first token as the public identifier and performs a lookup, producing 'No entry for PUBLIC ...' instead.
- **Observable residual:** xmlcatalog --shell with 'public -//OASIS//DTD X//EN': upstream errors 'public requires 1 arguments'; ours answers 'No entry for PUBLIC -//OASIS//DTD'.
- **Fix:** 11.1-X: the xmlcatalog shell now uses a quote-aware tokenizer and validates exact argument counts per command ('public requires 1 arguments' when the command is not given exactly one argument).
- **Regression courts:** CLI-XMLCATALOG-0010.
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-08-29 (discovered by 11.1-A evidence promotion (target/difftest.sh -> committed CLI court)); FIXED 2026-08-31 (fixed in the 11.1-X residual closure loop; CLI-XMLCATALOG-0010 byte-identical (57/57 CLI courts PASS))

## Phase 11.1-G Residuals

### R-000124: Candidate headers lacked byte-exact public struct/enum definitions (header-surface gap) (FIXED)

- **Status:** FIXED (2026-08-29, Phase 11.1-G)
- **Component:** tools/headers/header_closure.py, include/libxml/parser.h, include/libxml/schemasInternals.h
- **Surface:** headers / ABI
- **Root cause:** The candidate include/ headers were hand-written stubs or partial reconstructions; the ABI census (tools/abi/abi_probe_gen.py) reported 255 libxml2 and 311 libxslt MISSING struct/enum entities (e.g. _xmlParserCtxt field types xmlStartTag/xmlParserNsData/xmlAttrHashBucket, schemasInternals cluster, xsltInternals cluster, xmlCharEncoding values 23-31, xmlErrorDomain, xmlParserOption enums). A C consumer compiling against the headers would not get the upstream layouts.
- **Fix:** Built tools/headers/header_closure.py: deterministic verbatim extraction of missing public struct/enum/typedef definitions from the oracle archaeology trees (libxml2-2.15.0, libxslt-1.1.42), with a regenerated [11.1-G] section per header, upstream forward-typedef blocks, #define->enum migration (xmlerror.h xmlParserErrors, xmlErrorDomain; parser.h xmlParserOption; tree.h xmlDocProperties), and a compile-fix loop resolving cascading missing types. Libxslt headers were rebuilt verbatim from upstream (xsltInternals.h, numbersInternals.h) with the upstream include conventions (xslt.h constants-only; transform.h hosts the engine entry points).
- **Evidence:** ['atlas/ABI_PARITY_LEDGER.json (libxml2 1889/1889, libxslt 344/344, verdict PASS)', 'courts/receipts/phase-11/header-compile-* (571/571 PASS)', 'tools/headers/header_closure.py']
- **Classification:** CANDIDATE_BUG

### R-000128: Struct field type drift: _xmlAttr.id, _xmlEntity.expandedSize, _xmlXPathContext.opLimit/opCount (FIXED)

- **Status:** FIXED (2026-08-29, Phase 11.1-G)
- **Component:** include/libxml/tree.h, include/libxml/xpath.h
- **Surface:** ABI struct layout
- **Root cause:** Hand-written headers used int for _xmlAttr.id (upstream: xmlIDPtr), _xmlEntity.expandedSize (upstream: unsigned long) and _xmlXPathContext.opLimit/opCount (upstream: unsigned long), producing wrong offsets/sizes.
- **Fix:** Corrected the field types to the upstream declarations; verified by the ABI census (offsetof/sizeof equality).
- **Evidence:** ['atlas/ABI_PARITY_LEDGER.json', 'include/libxml/tree.h', 'include/libxml/xpath.h']
- **Classification:** CANDIDATE_BUG

### R-000129: _xmlCharEncodingHandler Rust struct layout mismatch (48 vs upstream 56 bytes) (FIXED)

- **Status:** FIXED (2026-08-29, Phase 11.1-G)
- **Component:** src/abi/structs.rs, src/abi/callbacks.rs, src/xml/encoding/mod.rs, include/libxml/encoding.h
- **Surface:** ABI struct layout
- **Root cause:** The Rust _xmlCharEncodingHandler stored 7 pointer/int fields in a non-upstream order; upstream has name + two anonymous unions (func/legacyFunc) + inputCtxt/outputCtxt/ctxtDtor/flags (sizeof 56, not 48). The old encoding.h also invented xmlCharEncInput/xmlCharEncOutput which do not exist upstream.
- **Fix:** Rewrote the Rust struct with upstream layout using repr(C) unions (EncodingInputUnion/EncodingOutputUnion), added the missing xmlCharEncConvFunc/xmlCharEncConvCtxtDtor/xmlCharEncConvImpl callbacks with upstream signatures, updated the consumer (input.legacyFunc/output.legacyFunc), rewrote encoding.h to the upstream content, and removed the invented prototypes.
- **Evidence:** ['atlas/ABI_PARITY_LEDGER.json (PASS)', 'src/abi/structs.rs', 'src/xml/encoding/mod.rs']
- **Classification:** CANDIDATE_BUG

### R-000130: Rust callback typedefs xmlResourceLoader / xmlCharEncConvImpl had non-upstream signatures (FIXED)

- **Status:** FIXED (2026-08-29, Phase 11.1-G)
- **Component:** src/abi/callbacks.rs
- **Surface:** ABI callbacks
- **Root cause:** The Rust xmlResourceLoader (void* ctxt, url, options:int, type:int)->xmlParserInput* and xmlCharEncConvImpl signatures did not match upstream parser.h/encoding.h (xmlParserErrors (*)(void*, const char*, const char*, xmlResourceType, xmlParserInputFlags, xmlParserInput**), xmlParserErrors (*)(void*, const char*, xmlCharEncFlags, xmlCharEncodingHandler**)).
- **Fix:** Corrected both callback typedefs to the upstream signatures; neither is invoked by the candidate, so the change is purely ABI-exact.
- **Evidence:** ['src/abi/callbacks.rs']
- **Classification:** CANDIDATE_BUG

### R-000132: xmlParseURI returned a non-C-layout object (UriParts) as xmlURIPtr (FIXED)

- **Status:** FIXED (2026-08-29, Phase 11.1-G)
- **Component:** src/xml/uri/mod.rs, src/abi/exports_xml2.rs
- **Surface:** ABI runtime object
- **Root cause:** xmlParseURI/xmlCreateURI returned Box<UriParts> (Rust struct of Option<Vec<u8>> fields) cast to xmlURIPtr; a C consumer reading uri->scheme would read garbage. The header struct _xmlURI (104 bytes) was correct but the runtime object was not.
- **Fix:** Introduced repr(C) CXmlUri matching struct _xmlURI field-for-field (allocator-owned null-terminated strings); xmlParseURI/xmlCreateURI now return it, xmlFreeURI releases the strings, xmlSaveUri converts back; added xmlParseURIReference (fills an existing struct) and xmlNormalizeURIPath (faithful in-place port of uri.c).
- **Evidence:** ['src/xml/uri/mod.rs (69 tests pass)', 'atlas/ABI_PARITY_LEDGER.json']
- **Classification:** CANDIDATE_BUG

## Phase 11.1-H Residuals

### R-000125: xslt security module used int allow/deny model instead of upstream callback model (FIXED)

- **Status:** FIXED (2026-08-29, Phase 11.1-H)
- **Component:** src/xslt/security/mod.rs, include/libxslt/security.h, src/bin/xsltproc.rs
- **Surface:** libxslt security API
- **Root cause:** xsltSetSecurityPrefs in upstream takes a function pointer (xsltSecurityCheck) per option (enum values 1..5), and WRITE_FILE maps to the createFile quirk; the candidate stored int allow/deny flags, which is ABI-incompatible with downstream code that registers callbacks.
- **Fix:** Rewrote src/xslt/security/mod.rs to the upstream callback model (XsltSecurityPrefs stores five Option<xsltSecurityCheck> callbacks, zeroed defaults); xsltproc -nowrite/-nomkdir register xslt_security_forbid callbacks; callback round-trip tests added; include/libxslt/security.h rewritten to the upstream declarations.
- **Evidence:** ['src/xslt/security/mod.rs (tests pass)', 'include/libxslt/security.h']
- **Classification:** CANDIDATE_BUG

### R-000126: Missing xpathInternals.h header surface (FIXED)

- **Status:** FIXED (2026-08-29, Phase 11.1-H)
- **Component:** include/libxml/xpathInternals.h
- **Surface:** headers
- **Root cause:** The candidate shipped no libxml/xpathInternals.h; downstream consumers including it (common for XPath extension authors) would fail to compile.
- **Fix:** Created include/libxml/xpathInternals.h declaring the 22 candidate-exported XPath-internals functions with upstream-compatible signatures; every declared function verified exported in the DSO by the HEADER-COMPILE court.
- **Evidence:** ['include/libxml/xpathInternals.h', 'courts/receipts/phase-11/header-compile-*']
- **Classification:** CANDIDATE_BUG

### R-000127: Missing libxslt public headers (libxslt.h, xsltexports.h, trio.h, triodef.h) (FIXED)

- **Status:** FIXED (2026-08-29, Phase 11.1-H)
- **Component:** include/libxslt/libxslt.h, include/libxslt/xsltexports.h
- **Surface:** headers
- **Root cause:** The candidate include/libxslt set was missing five public headers present upstream; consumers including them failed.
- **Fix:** Created the missing headers (libxslt.h umbrella, xsltexports.h visibility macros, trio.h/triodef.h); the include set now matches upstream (diff-verified against the archaeology tree); config.h (a build-private artifact) is no longer included.
- **Evidence:** ['include/libxslt/*', 'courts/receipts/phase-11/header-compile-*']
- **Classification:** CANDIDATE_BUG

### R-000133: Declared-but-unexported header functions (33 symbols) (FIXED)

- **Status:** FIXED (2026-08-29, Phase 11.1-H)
- **Component:** src/abi/allocator.rs, src/xml/reader/mod.rs, src/abi/exports_xml2.rs, src/abi/versioning.rs, src/xml/uri/mod.rs
- **Surface:** headers / DSO exports
- **Root cause:** Hand-written headers declared 33 functions the candidate DSO did not export: the legacy allocator names (xmlMemMalloc/xmlMemFree/xmlMemRealloc/xmlMemoryStrdup + 7 more), 13 xmlTextReader* accessors, xmlFreeNodeList, xmlInitGlobals/xmlCleanupGlobals, xmlCheckVersion, xmlSAX2InitDefaultSAXHandler/xmlSAX2InitHtmlDefaultSAXHandler, xmlNormalizeURIPath/xmlParseURIReference. A header declaring a symbol the library does not provide breaks the honest-header rule and would fail downstream linking.
- **Fix:** Implemented all 33 with upstream semantics: legacy allocator wrappers, reader accessors (Close/CurrentNode/Expand/line/column/IsValid/Normalization/ReadAttributeValue/ReadInnerXml/ReadOuterXml/ReadString/Standalone/XmlLang), xmlFreeNodeList wrapper, globals init/cleanup no-ops, xmlCheckVersion export, SAX2 handler initializers (HTML variant matches upstream SAX2.c field set and initialized=1), and the URI functions. xmlSAX2IsInitialized (not an upstream API) was removed from SAX2.h. The HEADER-COMPILE court now proves every declared function is exported.
- **Evidence:** ['courts/receipts/phase-11/header-compile-* (declared-functions-exported PASS)', 'nm -D target/debug/liblibxml_rs.so']
- **Classification:** CANDIDATE_BUG

### R-000134: xmlSAX2IsInitialized declared in header but not an upstream API (FIXED)

- **Status:** FIXED (2026-08-29, Phase 11.1-H)
- **Component:** include/libxml/SAX2.h
- **Surface:** headers
- **Root cause:** The hand-written SAX2.h declared xmlSAX2IsInitialized(void* ctx), which does not exist in upstream libxml2 2.15 (verified against /usr/include and the archaeology tree).
- **Fix:** Removed the declaration; upstream 2.15 declares xmlSAX2InitDefaultSAXHandler(hdlr, warning) and xmlSAX2InitHtmlDefaultSAXHandler(hdlr), both now exported by the candidate.
- **Evidence:** ['include/libxml/SAX2.h', 'courts/receipts/phase-11/header-compile-*']
- **Classification:** CANDIDATE_BUG

## Phase 11.1-I Residuals

### R-000135: Exported C data globals: 11 libxml2 symbols remain (SAX handler structs, char tables, xmlLastError) (FIXED)

- **Status:** FIXED (, Phase 11.1-I)
- **Component:** src/xml/globals/mod.rs, src/abi/exports_xml2.rs, src/abi/exports_xslt.rs, src/abi/data_globals.rs, src/xml/chvalid.rs, src/xml/unicode_tables.rs, src/xml/errors/mod.rs, tools/abi/data_globals_probe.py, courts/suites/data-abi/data-globals-probe.c
- **Surface:** ABI data symbols
- **Root cause:** Upstream exposes ~45 public data globals (xmlDoValidityCheckingDefaultValue, xmlLoadExtDtdDefaultValue, xmlKeepBlanksDefaultValue, xmlGenericError, xmlStructuredError, xmlDefaultSAXHandler, xmlLastError, xmlParserVersion, xmlXPathNAN/PINF/NINF, char-class tables, xmlStringText, xsltMaxDepth/xsltMaxVars, ...). The candidate keeps parser defaults in Rust atomics and has not exported the C-visible globals; downstream code that reads/writes them directly (e.g. xmlDoValidityCheckingDefaultValue = 1) would fail to link. 5 data symbols (xmlFree/xmlMalloc/xmlMallocAtomic/xmlMemStrdup/xmlRealloc) are already exported. CLOSED SO FAR: all parser-default ints, error callbacks/contexts, buffer globals, static strings, XPath NaN/Inf, libxslt globals (xsltLibxmlVersion, xsltGenericError[Context], xsltGenericDebugContext, xsltExtMarker, xsltDocDefaultLoader, xsltMaxDepth=30000 fixed from 3000, xsltMaxVars), and the I/O filename callbacks are exported as #[no_mangle] statics wired to the parser-default accessors (single source of truth). Remaining: xmlDefaultSAXHandler/htmlDefaultSAXHandler/xmlDefaultSAXLocator (const-initialized struct instances), the 7 char-class tables (xmlIs*Group, xmlIsPubidChar_tab — upstream table data must be extracted from xmlunicode.c), and xmlLastError (conflicts with the thread-local error model; needs a global mirror synced by the error module). xsltGenericError/xsltDocDefaultLoader default to NULL instead of upstream's variadic default functions — documented safe divergence (stable Rust cannot define variadic extern fns).
- **Fix:** All 11 remaining data symbols are now exported and court-verified byte-identical against the oracle DSO. (1) Added _xmlSAXHandlerV1, xmlChSRange/xmlChLRange/xmlChRangeGroup mirrors to structs.rs (measured by the RUST-MIRROR-ABI court, 0 mismatches). (2) Created tools/archaeology/gen_chvalid_tables.py — a deterministic generator extracting the char-class tables verbatim from upstream codegen/ranges.inc (sha256 bound) into src/xml/unicode_tables.rs (xmlIsPubidChar_tab[256] + the six xmlIs*Group range tables, counts verified against the declared values). (3) Created src/xml/chvalid.rs implementing xmlCharInRange, xmlIs{BaseChar,Blank,Char,Combining,Digit,Extender,Ideographic,PubidChar}, xmlIsLetter and xmlIsBlankNode with the exact upstream Q-macro semantics. (4) Exported xmlDefaultSAXHandler/htmlDefaultSAXHandler (xmlSAXHandlerV1) and xmlDefaultSAXLocator as #[no_mangle] consts reproducing the upstream globals.c initializer lists exactly — the differential court caught an initializer error (htmlDefaultSAXHandler reference/externalSubset slots) before sealing. (5) Added the xmlParserError/Warning/ValidityError/ValidityWarning legacy SAX handlers (non-variadic documented divergence) referenced by the handler structs. (6) xmlLastError: deep-copy mirror of the thread-local error state (sync on raise, free on reset, upstream xmlResetError semantics). (7) New DATA-GLOBALS-001 court (tools/abi/data_globals_probe.py + committed C probe) compiles the probe against the system libxml2 and the candidate DSO and requires byte-identical output: pubid table hex, every range group entry, SAX handler slot patterns, xmlLastError zero state, FNV-1a hashes of the nine xmlIs* functions over the full BMP + supplementary samples, xmlIsBlankNode behavior. Verdict PASS (oracle sha256 e7575963… == candidate). PARITY_OBLIGATIONS.json regenerated: DATA MISSING 11 → 0 (both projects).
- **Evidence:** ['courts/receipts/phase-11/data-globals-20260829T203735Z.json', 'courts/receipts/phase-11/rust-mirror-abi-2026-08-29T20:08:38Z.json', 'atlas/PARITY_OBLIGATIONS.json (DATA MISSING = 0)']
- **Classification:** UNRESOLVED
- **History:** OPEN 2026-08-29 (discovered during 11.1-I parity census); FIXED 2026-08-29 (closed: 11 data symbols exported with upstream layout and initial values; DATA-GLOBALS-001 differential court byte-identical vs the oracle DSO; obligations regenerated (DATA MISSING = 0). Follow-up (11.1-K): the remaining NULL-default divergence for xsltGenericError/xmlGenericError was closed with the variadic asm shims (R-000161); xsltDocDefaultLoader remains NULL (loader path documented separately). (FIXED->FIXED tail merged by the 11.1-X ledger integrity repair.))

### R-000136: Missing oracle functions: 881 libxml2 + 201 libxslt exports (was 1158 at discovery) (FIXED)

- **Status:** FIXED (, Phase 11.1-I)
- **Component:** src/abi/exports_xml2.rs, src/abi/exports_xslt.rs, src/xml
- **Surface:** DSO function exports
- **Root cause:** The parity obligation census (tools/abi/parity_obligations.py, oracle = system libxml2 2.15.3 / libxslt 1.1.45 DSOs) records 1158 libxml2 and 201 libxslt upstream functions that the candidate does not yet export. These are the 11.1-I obligation ledger entries; each must be implemented (not stubbed) with upstream semantics, court-covered, in dependency order.
- **Fix:** 11.1-X: the 1158-discovery export census is closed. The candidate now exports every oracle DSO symbol: libxml2 881/881, libxslt 201/201, libexslt (parity ledger MISSING = 0 for all three projects, atlas/PARITY_OBLIGATIONS.json). The remaining 16 STUB marks are dispositioned separately (R-000138 deprecated no-ops, R-000160 trivial libxslt bodies) and are not missing symbols: every STUB symbol is exported with a body whose observable behaviour matches the oracle. The dso-loader court loads every exported symbol from the built DSO (25/25) and the header-compile court compiles every public header against the DSO (595/595).
- **Regression courts:** DSO-LOADER, HEADER-COMPILE.
- **Evidence:** ['atlas/PARITY_OBLIGATIONS.json']
- **Classification:** UNRESOLVED
- **History:** OPEN 2026-08-29; FIXED 2026-08-31 (closed in 11.1-X; PARITY_OBLIGATIONS MISSING=0 (libxml2/libxslt/libexslt); dso-loader 25/25, header-compile 595/595)

### R-000138: Deprecated init/cleanup entry points are no-ops (xmlInitializeGlobalState, xmlInitializeDict, xmlInitializePredefinedEntities, xmlCleanupPredefinedEntities, xmlDefaultSAXHandlerInit, xmlCheckThreadLocalStorage) (FIXED)

- **Status:** FIXED (, Phase 11.1-I)
- **Component:** src/abi/exports_xml2.rs
- **Surface:** DSO function exports
- **Root cause:** Modern libxml2 keeps these as genuine no-ops (subsystems initialize lazily; xmlDefaultSAXHandlerInit fills a global handler the candidate builds on demand; xmlCheckThreadLocalStorage always passes with Rust thread-locals). The candidate exports them with matching no-op behavior; the only observable difference is xmlDefaultSAXHandlerInit not populating the (still missing) xmlDefaultSAXHandler global, tracked in R-000135.
- **Fix:** 11.1-X: the deprecated init/cleanup entry points are dispositioned as intentional safe divergences with evidence: each one is exported and its body reproduces the oracle's observable behaviour. Upstream bodies are themselves empty or near-empty (xmlInitializeGlobalState, xmlInitializeDict, xmlInitializePredefinedEntities, xmlCleanupPredefinedEntities, xmlDefaultSAXHandlerInit, xmlCheckThreadLocalStorage), so the candidate's no-op is the oracle's behaviour, not a divergence. The PARITY_OBLIGATIONS STUB census (15 libxml2 + 1 libexslt) records the export+body disposition for each symbol; the remaining no-op set (htmlDefaultSAXHandlerInit, htmlInitAutoClose, htmlParseCharRef, xmlFileMatch, xmlParserInputRead, xmlDictCleanup, xmlRelaxNGCleanupTypes, xmlSchemaCleanupTypes, xmlSprintfElementContent, xmlXPathInit, xmlXPathRegisterAllFunctions) matches the corresponding upstream empty/trivial bodies byte-for-byte in observable effect.

11.1-Z: the disposition is completed for the full 14-symbol no-op set. htmlElementAllowedHere (upstream constant return 1), xmlRelaxNGInitTypes (upstream empty after lazy init; candidate returns 0) and xmlSchemaFreeWildcard (the candidate never allocates wildcard objects, so the free is a safe no-op) are now recorded alongside the other eleven in PARITY_OBLIGATIONS DOCUMENTED_NOOPS, each mapped to this residual. The STUB census therefore drops to 0 for libxml2/libxslt/libexslt — no externally relevant stubs remain (11.1-Z acceptance).
- **Regression courts:** DSO-LOADER, HEADER-COMPILE.
- **Evidence:** ['atlas/PARITY_OBLIGATIONS.json (EXPORTED_NOOP)']
- **Classification:** INTENTIONAL_SAFE_DIVERGENCE
- **History:** OPEN 2026-08-29; FIXED 2026-08-31 (dispositioned in 11.1-X: exported no-ops matching upstream's empty bodies; PARITY_OBLIGATIONS STUB census with per-symbol disposition. Completed in 11.1-Z: all 14 no-op symbols (incl. htmlElementAllowedHere, xmlRelaxNGInitTypes, xmlSchemaFreeWildcard) mapped in PARITY_OBLIGATIONS DOCUMENTED_NOOPS to R-000138; STUB census drops to 0, no externally relevant stubs remain)

### R-000139: Rust _xmlElement ABI mirror diverged from public C layout (56 vs 104 bytes) (FIXED)

- **Status:** FIXED (, Phase 11.1-I)
- **Component:** src/abi/structs.rs, src/xml/dtd/mod.rs, src/xml/validation/mod.rs, src/xml/tree/mod.rs
- **Surface:** Rust #[repr(C)] mirror of struct _xmlElement
- **Root cause:** The ABI evidence system proved upstream-C-header ↔ candidate-C-header parity but never proved implementation-representation parity: the C ABI court compiles probes against C headers only and does not inspect the Rust #[repr(C)] mirrors. The Rust _xmlElement had 7 fields (56 bytes) while the candidate header and upstream tree.h define 14 fields (104 bytes). Every element declaration allocated via xmlMalloc(sizeof(_xmlElement)) was therefore 48 bytes too small; a C consumer reading elem->content at its real offset would read beyond the allocation.
- **Fix:** Rewrote the Rust mirror with the exact upstream layout (_private, type, name, children, last, parent, next, prev, doc, etype, content, attributes, prefix, contModel); reworked add_element_decl/copy_element/free_element for the new field set (etype now carries EMPTY/ANY/MIXED/ELEMENT; type_ is XML_ELEMENT_DECL); switched all readers from type_ to etype; free_element now releases contModel via the validation content-model NFA free. Also added opaque xmlID/xmlRef mirror structs. Committed as 4090561b.
- **Evidence:** ['src/abi/structs.rs', 'src/abi/exports_xml2.rs']
- **Classification:** CANDIDATE_BUG

### R-000140: Eight libxslt Rust #[repr(C)] mirrors diverged from the oracle ABI (121 mismatches) (FIXED)

- **Status:** FIXED (, Phase 11.1-I)
- **Component:** src/abi/structs.rs, src/xslt/compiler/mod.rs, src/xslt/templates/mod.rs, src/xslt/stylesheet/mod.rs, src/xslt/variables/mod.rs, src/xslt/parameters/mod.rs, src/xslt/keys/mod.rs, src/xslt/extensions/mod.rs, src/xslt/errors/mod.rs, src/xslt/transform/mod.rs, src/xslt/sorting/mod.rs, src/xslt/documents/mod.rs
- **Surface:** Rust #[repr(C)] mirrors of _xsltStylesheet, _xsltTransformContext, _xsltTemplate, _xsltStackElem, _xsltKeyDef, _xsltKeyTable, _xsltDocument, _xsltDecimalFormat
- **Root cause:** The candidate's libxslt Rust mirrors were authored against an invented layout rather than the upstream C headers. The RUST-MIRROR-ABI court (first run, commit ef00daf) measured 121 mismatches across eight structs: _xsltStylesheet (280 vs 440 bytes), _xsltTransformContext, _xsltTemplate (missing the match field entirely in the mirror's field order), _xsltStackElem (invented style/inst fields; real comp/computed/level/context fields absent), _xsltKeyDef (missing match/use), _xsltKeyTable (invented table/nb/max/depth array fields; real struct is next/name/nameURI/keys), _xsltDocument and _xsltDecimalFormat. A C consumer traversing these fields directly would read the wrong offsets. This is the same defect class as _xmlElement (R-000139), now caught for the whole libxslt family by the permanent three-way mirror court.
- **Fix:** Rewrote all eight mirrors from the authoritative clang -fdump-record-layouts-complete field lists of the candidate C headers (verbatim 1.1.42) and remapped every downstream accessor: transform context (sec/error/errctx, extFunctions/extElements, output, maxTemplateDepth/maxTemplateVars, varsTab stack, initialContextNode/initialContextDoc, no paramsTab/keyTables/contextSize fields — those live on xpathCtxt or the document wrapper), template (match string in r#match, compiled pattern carried in params, import depth in position), stylesheet (no params field; caller params merge into variables with XSLT_VAR_PARAM; preserve-space head in nsDefs; XSLT_REFACTORED-gated compCtxt/principalData omitted because the oracle DSO ships with XSLT_REFACTORED disabled), key tables (candidate array storage moved behind the keys void* slot as a private sidecar), stack elements (no style field), decimal formats (nsUri is const), plus document wrapper (main/doc/keys/includes/preproc/nbKeysComputed) and the whole xslt module compile+test pass. Committed with the mirror PASS receipt.
- **Evidence:** ['courts/receipts/phase-11/rust-mirror-abi-2026-08-29T20:08:38Z.json', 'src/abi/structs.rs']
- **Classification:** CANDIDATE_BUG

### R-000141: xmlTextReaderQuoteChar: upstream hardcodes '"' unconditionally; candidate returned 0 (FIXED)

- **Status:** FIXED (, Phase 11.1-I)
- **Component:** src/xml/reader/mod.rs, include/libxml/xmlreader.h
- **Surface:** xmlTextReaderQuoteChar (xmlreader.c)
- **Root cause:** The READER-001 differential probe showed the candidate returning quote=0 while the oracle returned 34 ('"') for every attribute. Inspection of upstream xmlreader.c (2.13.0 and 2.15.0) shows xmlTextReaderQuoteChar is a placeholder that returns '"' unconditionally for any non-NULL reader ('/* TODO maybe lookup the attribute value for " first */'); it never inspects the attribute. The candidate had implemented a value-based lookup that produced 0 when the attribute was not on a quote-bearing attribute node. This is an upstream historical accident that the custodian must reproduce verbatim.
- **Fix:** xmlTextReaderQuoteChar now returns b'"' (34) for any non-NULL reader and -1 for a NULL reader, matching the oracle exactly. Covered by READER-001 (quote=34 on real attributes and xmlns: namespace declarations).
- **Evidence:** ['courts/suites/data-abi/reader-family-probe.c', 'courts/receipts/phase-11/reader-family-*.json', 'oracle/historical/src/libxml2-2.15.0/xmlreader.c']
- **Classification:** CANDIDATE_BUG

### R-000142: xmlReaderNew* family: NULL reader must be rejected with -1 without allocating (in-place reuse contract) (FIXED)

- **Status:** FIXED (, Phase 11.1-I)
- **Component:** src/xml/reader/mod.rs, include/libxml/xmlreader.h
- **Surface:** xmlReaderNewDoc/NewFile/NewMemory/NewFd/NewIO/NewWalker (xmlreader.c)
- **Root cause:** READER-001 showed newmem-ret=0 for xmlReaderNewMemory(NULL, ...) while the oracle returns -1 and leaves the caller's pointer untouched. Upstream xmlreader.c begins every xmlReaderNew* with 'if (reader == NULL) return (-1);' before any work, and the family reuses the caller's existing reader allocation in place (xmlTextReaderSetup), so a caller's pointer remains valid across New* calls. The candidate had implemented New* by allocating a fresh reader and, worse, the old reader_renew helper dropped the caller's allocation, leaving the caller's pointer dangling after a successful reuse. Both the NULL-rejection contract and the pointer-stability contract were violated.
- **Fix:** Every xmlReaderNew* now returns -1 immediately when reader is NULL (and when the source argument is NULL, matching upstream's per-function early checks), never allocating. reader_renew was rewritten to move the freshly built reader's contents into the caller's allocation in place (drop_in_place + copy_nonoverlapping + dealloc of the temporary), preserving the caller's pointer identity across reuse exactly as upstream does. Sealed by READER-001 (newmem-ret=-1, reader stays NULL) plus the in-place reuse unit tests.
- **Evidence:** ['courts/suites/data-abi/reader-family-probe.c', 'courts/receipts/phase-11/reader-family-*.json', 'src/xml/reader/mod.rs']
- **Classification:** CANDIDATE_BUG

### R-000143: Reader attribute traversal: namespace declarations count as attributes (xmlns / xmlns:prefix), ordered before properties (FIXED)

- **Status:** FIXED (, Phase 11.1-I)
- **Component:** src/xml/reader/mod.rs
- **Surface:** xmlTextReader attribute navigation (HasAttributes, AttributeCount, MoveToFirstAttribute, MoveToNextAttribute, IsNamespaceDecl)
- **Root cause:** The upstream reader reports namespace declarations as attributes during attribute traversal (they appear in the attribute list with names 'xmlns' and 'xmlns:prefix', ordered before regular properties), and xmlTextReaderIsNamespaceDecl distinguishes them. The candidate's initial attribute iteration only exposed real xmlAttr properties, so counts and MoveTo* navigation diverged from the oracle on any document with xmlns declarations.
- **Fix:** Attribute iteration now walks namespace definitions first (names 'xmlns'/'xmlns:prefix', flagged IsNamespaceDecl=1) followed by regular properties, using a unified AttrTarget enum; HasAttributes/AttributeCount and all MoveTo*/GetAttribute* entry points share the same ordering. Sealed by READER-001 (ns-decl attributes with value=urn:x and quote=34, ordered before id/attr).
- **Evidence:** ['courts/suites/data-abi/reader-family-probe.c', 'courts/receipts/phase-11/reader-family-*.json']
- **Classification:** CANDIDATE_BUG

### R-000144: Reader event stream: empty elements emit no END_ELEMENT; whitespace-only text is SIGNIFICANT_WHITESPACE (14) (FIXED)

- **Status:** FIXED (, Phase 11.1-I)
- **Component:** src/xml/reader/mod.rs
- **Surface:** xmlTextReaderRead/xmlReaderWalker event generation (xmlreader.c xmlTextReaderNextNode)
- **Root cause:** The candidate's traversal emitted an END_ELEMENT event for empty elements (<empty/>) and skipped whitespace-only text nodes; the oracle (verified against system 2.15.3 by a standalone read-path probe and the READER-001 walker) emits no END_ELEMENT for empty elements and reports whitespace-only text as SIGNIFICANT_WHITESPACE (14) unless XML_PARSE_NOBLANKS. The walker-count in READER-001 was 7 vs 9 before the fix.
- **Fix:** build_events now skips the END event for empty elements and classifies whitespace-only text/CDATA content as SIGNIFICANT_WHITESPACE; six unit tests with pre-parity expectations were updated to the oracle-verified sequences. Sealed by READER-001 (walker-count=9 byte-identical) and an independent C read-path probe against the system oracle.
- **Evidence:** ['courts/suites/data-abi/reader-family-probe.c', 'courts/receipts/phase-11/reader-family-*.json', 'src/xml/reader/mod.rs']
- **Classification:** CANDIDATE_BUG

### R-000145: xmlTextReaderGetLastError returns a non-NULL embedded xmlError even before any error (message NULL) (FIXED)

- **Status:** FIXED (, Phase 11.1-I)
- **Component:** src/xml/reader/mod.rs
- **Surface:** xmlTextReaderGetLastError (xmlreader.c)
- **Root cause:** READER-001 showed last-error=(null) for the candidate vs (no msg) for the oracle on a freshly created reader. Upstream returns &reader->ctxt->lastError, which is always present while the parser context exists, so callers that inspect the struct (rather than just testing NULL) observe a valid pointer with message==NULL before any error. The candidate returned NULL when no error had been collected.
- **Fix:** xmlTextReaderGetLastError now always returns a pointer to the reader's embedded _xmlError (message NULL until an error is collected, then a fresh xmlMalloc'd message owned by the reader and freed on replacement/drop). Sealed by READER-001 (last-error=(no msg) byte-identical).
- **Evidence:** ['courts/suites/data-abi/reader-family-probe.c', 'courts/receipts/phase-11/reader-family-*.json']
- **Classification:** CANDIDATE_BUG

### R-000146: Ledger generator silently dropped residuals whose phase label was not in the canonical phases list (21 residuals missing from Markdown) (FIXED)

- **Status:** FIXED (, Phase 11.1-I)
- **Component:** tools/evidence/ledger_gen.py, atlas/RESIDUAL_LEDGER.json, atlas/RESIDUAL_LEDGER.md
- **Surface:** tools/evidence/ledger_gen.py (canonical_md + run_check)
- **Root cause:** canonical_md groups residuals by phase but only renders phases present in the JSON 'phases' display list. Residuals labeled 11.1-G / 11.1-H / 11.1-I (21 of them, including R-000139 and R-000140) were silently omitted from the generated Markdown while run_check's byte-identity comparison passed vacuously against the equally-stale committed file. The evidence-integrity court therefore certified a document that was missing a third of the ledger. This is the same defect class as the original §70 violation (Markdown vs JSON drift): the generator itself had a silent-omission path.
- **Fix:** Added the 11.1-G / 11.1-H / 11.1-I phase labels to the canonical phases list and added a run_check validation that fails when any residual's phase is absent from the list, so a future phase label can never silently disappear from the generated document again. Regenerated the Markdown (49/49 residuals render) and re-passed the evidence-integrity court.
- **Evidence:** ['tools/evidence/ledger_gen.py', 'atlas/RESIDUAL_LEDGER.md']
- **Classification:** CANDIDATE_BUG

### R-000147: Parser never attached a namespace to namespace-qualified attributes (xmlSAX2AttributeNs parity) (FIXED)

- **Status:** FIXED (, Phase 11.1-I)
- **Component:** src/xml/sax/default.rs
- **Surface:** Default SAX handler attribute construction (src/xml/sax/default.rs parser_set_prop)
- **Root cause:** The READER-001 extension (namespaced attribute x:a="nsval") exposed that attributes with a prefix carried no _xmlAttr->ns: the SAX2 default handler stored only the local name and never resolved the prefix. Upstream xmlSAX2AttributeNs resolves the prefix through the parser namespace scope (element's own declarations plus ancestors) and attaches the resulting xmlNs. Downstream consequences are broad: xmlHasNsProp, MoveToAttributeNs, ConstNamespaceUri at attribute positions and any C consumer reading attr->ns all see a NULL namespace.
- **Fix:** parser_set_prop now resolves the attribute prefix exactly like xmlSAX2AttributeNs: scan the element's own nsDef chain (the new element is not yet linked to its parent when attributes are processed), then fall back to tree::search_ns over ctxt->node (the parent before the element push). The existing-attribute update path now matches on name AND namespace so same-local-name different-prefix attributes stay distinct. Sealed by READER-001 (x:a resolves to urn:x; MoveToAttributeNs finds it).
- **Evidence:** ['courts/suites/data-abi/reader-family-probe.c', 'courts/receipts/phase-11/reader-family-20260829T212705Z.json', 'src/xml/sax/default.rs']
- **Classification:** CANDIDATE_BUG

### R-000148: Reader reports namespace-qualified attribute names as prefix:localname (constQString), not the local name (FIXED)

- **Status:** FIXED (, Phase 11.1-I)
- **Component:** src/xml/reader/mod.rs
- **Surface:** xmlTextReaderConstName/Name at attribute positions (xmlreader.c constQString)
- **Root cause:** The extended READER-001 probe showed the oracle reporting attr name=x:a for a namespaced attribute while the candidate reported name=a. Upstream xmlTextReaderConstName returns constQString(reader, node->ns->prefix, node->name) — i.e. 'prefix:localname' — for any element or attribute whose namespace has a prefix; the tree stores only the local name in node->name.
- **Fix:** cache_attribute_info now rebuilds 'prefix:localname' for namespace-qualified attributes (matching the element-name logic already present for elements) and keeps the local name for unqualified ones. Sealed by READER-001 (attr name=x:a byte-identical).
- **Evidence:** ['courts/suites/data-abi/reader-family-probe.c', 'courts/receipts/phase-11/reader-family-20260829T212705Z.json', 'oracle/historical/src/libxml2-2.15.0/xmlreader.c']
- **Classification:** CANDIDATE_BUG

### R-000149: Reader namespace accessors wrong at attribute positions; xmlns-decl quirks missing (FIXED)

- **Status:** FIXED (, Phase 11.1-I)
- **Component:** src/xml/reader/mod.rs
- **Surface:** xmlTextReaderConstNamespaceUri / ConstPrefix / ConstLocalName (xmlreader.c)
- **Root cause:** The extended READER-001 probe revealed three upstream contracts the candidate did not reproduce: (1) at an attribute position the namespace comes from the attribute (or namespace declaration) itself, not from the underlying element node; (2) a namespace-declaration attribute reports the hardcoded URI http://www.w3.org/2000/xmlns/ (not the declared URI); (3) ConstPrefix on a namespace declaration returns the string 'xmlns' (and NULL for the default declaration), while ConstLocalName returns the prefix (or 'xmlns' for the default). The candidate consulted cur_node->ns for every position and had no namespace-decl handling.
- **Fix:** All three accessors now resolve the current attribute target (AttrTarget::Ns/Prop) when positioned on an attribute, reproducing the upstream quirks verbatim; at element positions they keep using the node's ns. Sealed by READER-001 (uri=http://www.w3.org/2000/xmlns/, local=x, prefix=xmlns for xmlns:x; uri=urn:x, local=a, prefix=x for x:a).
- **Evidence:** ['courts/suites/data-abi/reader-family-probe.c', 'courts/receipts/phase-11/reader-family-20260829T212705Z.json', 'oracle/historical/src/libxml2-2.15.0/xmlreader.c']
- **Classification:** CANDIDATE_BUG

### R-000150: Reader typed nodes report fixed names (#text, #comment, #cdata-section, #document) instead of NULL (FIXED)

- **Status:** FIXED (, Phase 11.1-I)
- **Component:** src/xml/reader/mod.rs
- **Surface:** xmlTextReaderConstName for non-element/attribute node kinds (xmlreader.c)
- **Root cause:** Upstream xmlTextReaderConstName returns fixed dictionary strings for typed nodes — '#text' for TEXT, '#cdata-section' for CDATA, '#comment' for COMMENT, '#document' for DOCUMENT/HTML_DOCUMENT, '#document-fragment' for DOCUMENT_FRAG — while the candidate returned NULL for those node kinds. Consumers like Nokogiri/lxml read ConstName on every node and distinguish node kinds by these names.
- **Fix:** cache_name_and_value now sets the upstream fixed name for those node kinds. Sealed by READER-001 (node=#text type=3 / type=14 lines byte-identical) and the updated test_read_simple_document expectation.
- **Evidence:** ['courts/suites/data-abi/reader-family-probe.c', 'courts/receipts/phase-11/reader-family-20260829T212705Z.json', 'src/xml/reader/mod.rs']
- **Classification:** CANDIDATE_BUG

### R-000151: Writer return values are encoder-dependent byte counts, not 0 or raw lengths (FIXED)

- **Status:** FIXED (, Phase 11.1-I)
- **Component:** src/xml/writer/mod.rs
- **Surface:** xmlTextWriter* return contract (xmlwriter.c)
- **Root cause:** The WRITER-001 differential probe showed the oracle returning 0 for StartDocument-with-encoding while the candidate returned the raw byte count. Upstream xmlOutputBufferWrite takes an encoder path once an encoding is installed: writes below the 256-byte conversion threshold report 0 bytes; xmlTextWriterWriteIndent reports the NUMBER OF INDENT STRINGS, not bytes; EndDocument returns the flush count. The encoder, once installed by a StartDocument with encoding, persists for the writer's lifetime (a later StartDocument with encoding=NULL only resets conv).
- **Fix:** The writer now tracks encoder_active (set once, never cleared), mutes byte-write returns to 0 when active, returns the indent string count from write_indent, and returns the output-buffer length delta/flush count from EndDocument. Sealed by WRITER-001 (all return values byte-identical, incl. enddoc=184).
- **Evidence:** ['courts/suites/data-abi/writer-family-probe.c', 'courts/receipts/phase-11/writer-family-*.json', 'oracle/historical/src/libxml2-2.15.0/xmlIO.c', 'oracle/historical/src/libxml2-2.15.0/xmlwriter.c']
- **Classification:** CANDIDATE_BUG

### R-000152: DTD internal-subset bracket is deferred to the first child declaration, not written by StartDTD (FIXED)

- **Status:** FIXED (, Phase 11.1-I)
- **Component:** src/xml/writer/mod.rs
- **Surface:** xmlTextWriterStartDTD/StartDTDElement/StartDTDAttlist/StartDTDEntity/WriteDTDNotation/EndDTD (xmlwriter.c)
- **Root cause:** Upstream xmlTextWriterStartDTD writes `<!DOCTYPE name` WITHOUT the `[`; the first DTD child (StartDTDElement/StartDTDAttlist/StartDTDEntity) or raw content emits ` [` (+newline when indented) on the DTD->DTD_TEXT transition, and EndDTD emits `]` only when content was written. The candidate's original StartDTD wrote `[` immediately and EndDTD wrote `]>` unconditionally — coincidentally identical for the old WriteDTD path but wrong for every Start/End composition (e.g. a bare StartDTD+EndDTD produced `<!DOCTYPE name []>` instead of `<!DOCTYPE name>`).
- **Fix:** StartDTD no longer writes the bracket; a dtd_child_transition helper writes ` [` on the first DTD child; EndDTD writes `]` only from the DTDText state. The DTD child starts also push/pop dtd_depth so the upstream indentation (one indent string per open declaration) and the indent return counts match. Sealed by WRITER-001.
- **Evidence:** ['courts/suites/data-abi/writer-family-probe.c', 'courts/receipts/phase-11/writer-family-*.json', 'oracle/historical/src/libxml2-2.15.0/xmlwriter.c']
- **Classification:** CANDIDATE_BUG

### R-000153: Writer namespace declarations are deferred to tag close (xmlns after attributes), not written inline (FIXED)

- **Status:** FIXED (, Phase 11.1-I)
- **Component:** src/xml/writer/mod.rs
- **Surface:** xmlTextWriterStartElementNS / xmlTextWriterOutputNSDecl (xmlwriter.c)
- **Root cause:** Upstream defers namespace declarations: xmlTextWriterOutputNSDecl emits ` xmlns:prefix="uri"` when the start tag closes (after all attributes), so `<p:node p:attr="v" xmlns:p="urn:test"/>` has the xmlns AFTER the attributes. The candidate wrote the xmlns inline at StartElementNS, producing `xmlns:p` before the attribute.
- **Fix:** StartElementNS now records pending namespace declarations; close_start_tag and the EndElement self-close path flush them (xmlns first, then `>`/`/>`). Sealed by WRITER-001.
- **Evidence:** ['courts/suites/data-abi/writer-family-probe.c', 'courts/receipts/phase-11/writer-family-*.json']
- **Classification:** CANDIDATE_BUG

### R-000154: Writer text/attribute escaping: apostrophe is never escaped; the indent default is one space; StartPI does not indent (FIXED)

- **Status:** FIXED (, Phase 11.1-I)
- **Component:** src/xml/writer/mod.rs
- **Surface:** xmlTextWriterWriteString/WriteAttribute, xmlEncodeSpecialChars, xmlBufAttrSerializeTxtContent, xmlTextWriterStartPI (xmlwriter.c)
- **Root cause:** Three upstream quirks the candidate did not reproduce: (1) xmlEncodeSpecialChars (xmlEscapeText with XML_ESCAPE_QUOT) escapes &<>" but NOT the apostrophe, and xmlBufAttrSerializeTxtContent (XML_ESCAPE_ATTR) likewise never escapes ' regardless of the writer's qchar — the qchar only selects the outer quotes; (2) the default indent string is a single space, not two; (3) xmlTextWriterStartPI writes no indentation (PIs appear at column 0).
- **Fix:** encode_special_chars and write_attr_escaped no longer escape the apostrophe (the qchar-aware quote logic was removed); the default indent string is now one space; StartPI no longer writes an indent. Sealed by WRITER-001.
- **Evidence:** ['courts/suites/data-abi/writer-family-probe.c', 'courts/receipts/phase-11/writer-family-*.json', 'oracle/historical/src/libxml2-2.15.0/entities.c', 'oracle/historical/src/libxml2-2.15.0/xmlsave.c']
- **Classification:** CANDIDATE_BUG

### R-000155: Variadic xmlTextWriterWriteFormat* exports cannot be defined on stable Rust (c_variadic unstable); solved with #[no_mangle] inline-asm shims (FIXED)

- **Status:** FIXED (, Phase 11.1-I)
- **Component:** src/xml/writer/mod.rs, build.rs
- **Surface:** xmlTextWriterWriteFormat*/WriteVFormat* (xmlwriter.h)
- **Root cause:** The 13 variadic Format functions require a va_list construction at the callee. Stable Rust cannot define variadic extern "C" functions. A global_asm approach failed because rustc's cdylib version script (global: no_mangle-exports; local: *) localizes every symbol not declared #[no_mangle], and --export-dynamic-symbol cannot override the version script.
- **Fix:** Each Format export is a #[no_mangle] function whose body is a single noreturn inline-asm block: it captures the SysV register save area like va_start, builds a 24-byte __va_list_tag (gp_offset/fp_offset/overflow_arg_area/reg_save_area), calls the corresponding VFormat implementation by name, restores the stack (including LLVM's 8-byte alignment push) and returns. The VFormat functions take a *mut VaListTag and format via the system vsnprintf with fresh va_copy per attempt (xmlTextWriterVSprintf semantics). Sealed by WRITER-001 including the >6-GP overflow-argument path.
- **Evidence:** ['courts/suites/data-abi/writer-family-probe.c', 'courts/receipts/phase-11/writer-family-*.json', 'src/xml/writer/mod.rs']
- **Classification:** CANDIDATE_BUG

### R-000156: xmlTextWriterStartDTDEntity and WriteDTDEntity had wrong signatures; WriteDTDAttribute composed the wrong primitives (FIXED)

- **Status:** FIXED (, Phase 11.1-I)
- **Component:** src/xml/writer/mod.rs, include/libxml/xmlwriter.h
- **Surface:** xmlTextWriterStartDTDEntity(writer, pe, name), xmlTextWriterWriteDTDEntity(writer, pe, name, pubid, sysid, ndataid, content), xmlTextWriterWriteDTDAttribute (xmlwriter.h)
- **Root cause:** The candidate's pre-existing StartDTDEntity exported (writer, name) but upstream is (writer, pe, name) with a %-prefix for parameter entities; WriteDTDEntity exported (writer, name, content) but upstream takes seven arguments and dispatches internal/external; WriteDTDAttribute composed an invented StartDTDAttribute/EndDTDAttribute pair instead of upstream's StartDTDAttlist/WriteString/EndDTDAttlist. The stub header masked these ABI mismatches from the header court.
- **Fix:** All three signatures now match upstream exactly; WriteDTDInternalEntity/WriteDTDExternalEntity/WriteDTDExternalEntityContents implement the dispatch targets; the full upstream xmlwriter.h was written (the stub was replaced), exposing every writer declaration to the header court. Sealed by WRITER-001 and the 571/571 header court.
- **Evidence:** ['src/xml/writer/mod.rs', 'include/libxml/xmlwriter.h', 'courts/receipts/phase-11/writer-family-*.json']
- **Classification:** CANDIDATE_BUG

### R-000157: xmlLookupCharEncodingHandler / xmlOpenCharEncodingHandler report XML_ERR_UNSUPPORTED_ENCODING for iconv/ICU-only encodings (FIXED)

- **Status:** FIXED (2026-09-05, Phase 11.1-I)
- **Component:** src/xml/encoding/mod.rs, src/abi/exports_xml2.rs
- **Surface:** xmlLookupCharEncodingHandler, xmlGetCharEncodingHandler, xmlOpenCharEncodingHandler, xmlCreateCharEncodingHandler (encoding.h)
- **Oracle versions:** libxml2 2.15.3 (system)
- **Root cause:** Upstream serves UCS-4LE/BE, EBCDIC, UCS-2, ISO-8859-2..9/10/11/13..16, ISO-2022-JP, Shift_JIS, EUC-JP and windows-1252 via iconv/ICU plus static 8-bit tables; the crate ships no iconv/ICU backend, so those encodings report XML_ERR_UNSUPPORTED_ENCODING (32) where the oracle returns a converter. The native set (UTF-8, UTF-16LE/BE, UTF-16, ISO-8859-1, US-ASCII) and all error paths are byte-identical (ENCODING-001).
- **Observable residual:** A C consumer requesting an iconv-only encoding gets XML_ERR_UNSUPPORTED_ENCODING instead of a handler; conversion through those encodings is unavailable in the candidate.
- **Fix:** Phase 14.27 + 14.29: the enumerated iconv/ICU-only set is now served by NATIVE converters. Phase 14.27: encoding_rs-backed Shift_JIS/EUC-JP (CP932-compatible WHATWG Shift_JIS / WHATWG EUC-JP) + writer output-encoder install. Phase 14.29: encoding_rs-backed ISO-8859-2..11/13..16 (ISO-8859-11 == windows-874/TIS-620; ISO-8859-9 via the WHATWG windows-1254 index) + ISO-2022-JP, and NATIVE fixed-width UCS-2 (glibc host-order = little-endian on x86-64) and UCS-4LE/BE codecs + a full EBCDIC code page 037 table derived from the oracle container's glibc iconv (a bijection onto U+0000..U+00FF). All are registered in the handler registry under their canonical names + aliases and are usable by every registry consumer (xmlFindCharEncodingHandler, the writer/save output path, char_enc_in/out). The parser INPUT layer (src/xml/parser/input.rs) now (a) whole-buffer-decodes any registry-served encoding named in a BOM-less XML declaration and (b) pattern-detects the non-ASCII-compatible family exactly like upstream xmlDetectCharEncoding (UCS-4LE 3C 00 00 00, UCS-4BE 00 00 00 3C, EBCDIC 4C 6F A7 94, BOM-less UTF-16LE/BE) and decodes through the registry handlers.
- **Phase 11 triangulation:** The reference system oracle (libxml2 2.15.3, built with Iconv and ICU enabled) returns usable converters for UCS-4LE/BE, EBCDIC, UCS-2, ISO-8859-2..16, ISO-2022-JP, Shift_JIS, EUC-JP and windows-1252, while the candidate reports XML_ERR_UNSUPPORTED_ENCODING — a REAL current Linux-x86-64 observable parity gap. No upstream epoch provides these converters without iconv/ICU, so closing the gap requires implementing an iconv (or ICU) backend in the crate: a future implementation work item, not a permanent waiver.
- **Regression courts:** ENCODING-001.
- **Evidence:** ['courts/receipts/phase-14/php-14-29-encoding-backend-20260905/enc-remainder-probe.php', 'courts/receipts/phase-14/php-14-29-encoding-backend-20260905/enc-input-probe.php']
- **Classification:** UNRESOLVED
- **History:** OPEN 2026-08-30 ( | 11.1-X final disposition: the iconv/ICU-only encodings (UCS-4LE/BE, EBCDIC, UCS-2, ISO-8859-2..16, ISO-2022-JP, Shift_JIS, EUC-JP, windows-1252) remain INTENTIONAL_SAFE_DIVERGENCE: the crate ships no iconv/ICU backend, so XML_ERR_UNSUPPORTED_ENCODING is the correct native answer. ENCODING-001 is byte-identical on the native set (UTF-8, UTF-16LE/BE, UTF-16, ISO-8859-1, US-ASCII) and on every error path. Closing this residual would require adding an iconv backend — a future work item, not a parity defect (triangulated against every upstream epoch: none provides these converters without iconv/ICU). | 11.1-Z.1 amendment: reclassified from INTENTIONAL_SAFE_DIVERGENCE to UNRESOLVED — the iconv/ICU-only encodings are an actual current Linux-x86-64 observable difference vs the Iconv+ICU-enabled 2.15.3 oracle, not a platform obligation; closing the residual requires implementing an iconv/ICU backend (future implementation work item). Phase 14.27 (2026-09-05) partial closure: Shift_JIS + EUC-JP are now served by NATIVE encoding_rs-backed converters (input and output; CP932-compatible WHATWG Shift_JIS + WHATWG EUC-JP, byte-identical to the oracle's glibc-iconv output on the shared JIS X 0208 repertoire and on the unmappable->decimal-charref error path; WHATWG/CP932 extension-range differences are the residual remainder), joining the earlier native windows-1252 converter. Still iconv/ICU-only (no converter): UCS-4LE/BE, EBCDIC, UCS-2, ISO-8859-2..16 and ISO-2022-JP (the last is stateful — a chunked native implementation must carry escape state across flushes).); FIXED 2026-09-05 (Closure evidence (Phase 14.29): enc-remainder-probe.php (writer OUTPUT for UCS-4LE/UCS-4BE/UCS-2/EBCDIC-US/IBM037/ISO-8859-2..11/13..16/ISO-2022-JP) is cmp-identical to the oracle (1897 bytes); enc-input-probe.php (DOMDocument::load of files physically encoded in UCS-4LE/UCS-4BE/UCS-2/EBCDIC-US/ISO-8859-2/ISO-8859-7/ISO-2022-JP incl. ISO-8859-1 control) is byte-identical to the oracle; cargo test --lib 1254 pass (7 new codec tests incl. the EBCDIC bijection, UCS-2 astral unmappable and ISO-2022-JP escape-sequence checks); NTS+ZTS six-extension gates 0 failed each; valgrind 0 errors on the input-decode path. Bounded remainder (not part of the enumerated set): other glibc-iconv names beyond the enumeration (KOI8-R/U, IBM866, macintosh, GBK/Big5/EUC-KR ...) are still unregistered, and ISO-2022-JP chunked output resets its escape state per conversion call (whole-document single-flush output is oracle-identical).)

### R-000160: libxslt exports with literally-trivial upstream 1.1.45 bodies classified as intentional no-ops (FIXED)

- **Status:** FIXED (, Phase 11.1-I)
- **Component:** src/abi/exports_xslt_util.rs, src/abi/exports_xslt_vars.rs, src/abi/exports_xslt_avt.rs
- **Surface:** xsltSecurityAllow, xsltSecurityForbid, xsltGetDebuggerStatus, xsltFreeLocales, xsltFreeAVTList, xsltExtensionInstructionResultRegister (libxslt.so.1)
- **Oracle versions:** libxslt 1.1.45 (system)
- **Root cause:** The upstream 1.1.45 bodies are literally `return(1)` (xsltSecurityAllow), `return(0)` (xsltSecurityForbid, xsltGetDebuggerStatus, xsltExtensionInstructionResultRegister) or empty (xsltFreeLocales, and xsltFreeAVTList in the candidate because AVTs are stored as raw strings). The ledger's static stub heuristic flags constant-return/empty bodies; these are exact upstream semantics, not placeholders.
- **Observable residual:** None — the candidate matches the oracle byte-for-byte on these entry points.
- **Fix:** 11.1-X: the libxslt exports with literally-trivial upstream 1.1.45 bodies are dispositioned as intentional safe divergences with evidence: each exported symbol's body reproduces the upstream trivial body's observable behaviour (verified against the system oracle DSO via the dso-loader and the encoding-family probe; ENCODING-001 byte-identical on the native set and all error paths).
- **Phase 11 triangulation:** Classification-only residual: the ledger labels them INTENTIONAL_NOOP so the closure count is honest.
- **Regression courts:** DSO-LOADER.
- **Evidence:** ['archaeology/libxslt-git/libxslt/security.c', 'archaeology/libxslt-git/libxslt/variables.c', 'archaeology/libxslt-git/libxslt/xslt.c', 'archaeology/libxslt-git/libxslt/xsltlocale.c']
- **Classification:** INTENTIONAL_SAFE_DIVERGENCE
- **History:** OPEN 2026-08-30; FIXED 2026-08-31 (dispositioned in 11.1-X: INTENTIONAL_SAFE_DIVERGENCE with per-symbol evidence in PARITY_OBLIGATIONS)

## Phase 11.1-J Residuals

### R-000131: Legacy allocator API surface: location tracking, xmlMemSize, debug dumps are simplified (FIXED)

- **Status:** FIXED (, Phase 11.1-J)
- **Component:** src/abi/allocator.rs, include/libxml/xmlmemory.h
- **Surface:** xmlmemory.h API
- **Root cause:** The default candidate allocator does not maintain upstream's per-block metadata table (xmlMallocLoc-style block list). Consequently xmlMemSize returns 0, the *Loc variants accept-and-ignore file/line, and xmlMemDisplayLast/xmlMemoryDump print aggregate counters instead of upstream's per-block dump.
- **Fix:** 11.1-J allocator instrumentation evolved into the 11.1-Z.3 final shape: the DEFAULT allocator (the five exported variables) is plain libc malloc/realloc/free/strdup with no tracking — byte-identical with upstream 2.15.0 globals.c defaults (xmlMalloc = malloc etc.), so xmlMemSize returns 0 and xmlMemUsed/xmlMemBlocks are 0 under the default (R-000178). The debug-named surface (xmlMemMalloc/xmlMemFree/xmlMemRealloc/xmlMemoryStrdup and the *Loc variants) maintains the upstream-style per-block registry (ptr -> size/file/line): xmlMemSize returns the recorded size for debug-surface blocks, the *Loc variants record the allocation site and accept-and-ignore file/line exactly like upstream's ATTRIBUTE_UNUSED parameters, and the counters track the debug surface — matching upstream's MEMHDR debug allocator (verified byte-identical with the oracle). xmlMemDisplayLast/xmlMemShow/xmlMemDisplay/xmlMemoryDump are no-ops (upstream 2.15.0 removed the feature).
- **Evidence:** ['src/abi/allocator.rs']
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-08-29 (discovered during 11.1-H declared-function closure); FIXED 2026-08-29 (closed by 11.1-J allocator instrumentation (per-block registry, xmlMemSize, *Loc site recording, xmlMemDisplayLast/xmlMemShow per-block dumps); history tail repaired by the 11.1-X ledger integrity repair)

## Phase 11.1-K Residuals

### R-000161: Error routing parity: generic handler must stream xmlFormatError fragments (6 calls per raise), and xmlGenericError/xsltGenericError default to variadic stderr printers (FIXED)

- **Status:** FIXED (, Phase 11.1-K)
- **Component:** src/xml/errors/mod.rs, src/xml/globals/mod.rs, src/abi/data_globals.rs, src/abi/exports_xslt_util.rs, src/xml/parser/state.rs, src/xml/sax/dispatch.rs, src/abi/exports_xml2.rs, src/xml/parser/helpers.rs, src/xml/reader/mod.rs
- **Surface:** xmlRaiseError routing, xmlSetGenericErrorFunc, xmlGenericError/xmlGenericErrorContext/xsltGenericError/xsltGenericErrorContext defaults (xmlerror.c/xsltutils.c)
- **Oracle versions:** libxml2 2.15.3 + 2.13.9, libxslt 1.1.45 (system DSOs)
- **Root cause:** Upstream xmlVRaiseError (error.c 2.15) routes each raise through ONE channel: a structured handler, else the SAX channel (custom slot receives `channel(data, msg)` once), else the legacy default which streams the xmlFormatError fragments (file/line, domain, level, message, source window, caret) as 6 separate variadic calls through xmlGenericError. The candidate's parser emitted one generic call plus an unconditional direct stderr write, so a counting handler observed err-count 1 vs the oracle's 6, and stderr was written even when a handler was installed. Additionally xmlGenericError/xsltGenericError defaulted to NULL, but upstream defaults them to the variadic stderr printers xmlGenericErrorDefaultFunc/xsltGenericErrorDefaultFunc (which also self-default the context to stderr).
- **Fix:** (1) Implemented xmlGenericErrorDefaultFunc/xsltGenericErrorDefaultFunc as x86_64 SysV inline-asm va_list shims (same pattern as xsltTransformError) forwarding to vfprintf receivers that default the context to stderr; the exported data globals now default to them and xmlSetGenericErrorFunc/xsltSetGenericErrorFunc reset to them on NULL (upstream semantics). (2) Added ch_call0/1/2 asm trampolines and format_error_streamed replicating the xmlFormatError fragment sequence byte-for-byte (verified: counting handler sees the same 6 format strings as the oracle). (3) Parser set_error/set_warning now route through raise_error_streamed with the upstream channel selection (structured XOR custom-SAX XOR fragment stream); the unconditional stderr write was removed (stderr output now flows through the default handler only). (4) xmlSAX2InitDefaultSAXHandler stores xmlParserError/xmlParserWarning in the error/warning slots like upstream SAX2.c. (5) xmlReadMemory/xmlReaderForMemory propagate the URL into the input filename (was empty, producing `:1:` prefixes instead of `e.xml:1:`). Also fixed the 5 wrong exported default values discovered by the defaults dump: xmlLineNumbersDefaultValue 0->1, xmlIndentTreeOutput 0->1, xmlTreeIndentString NULL->"  ", xmlBufferAllocScheme 0->1 (XML_BUFFER_ALLOC_EXACT), xmlParserVersion "21503"->"21503-GITv2.15.3".
- **Evidence:** ['courts/suites/data-abi/globals-threading-probe.c', 'tools/abi/globals_threading_probe.py', 'courts/receipts/phase-11/globals-threading-*.json', 'target/scratch/globals_dump.c (DEFAULTS-IDENTICAL vs oracle)', 'cargo test --lib 1135 pass', 'ASan full-suite clean']
- **Classification:** CANDIDATE_BUG

## Phase 11.1-L Residuals

### R-000162: Callback surface: C XPath functions were stubbed, external entity loader never consulted, node register/deregister hooks never fired, xmlListAppend/walk diverged, allocator entry points exported as functions instead of data (FIXED)

- **Status:** FIXED (, Phase 11.1-L)
- **Component:** src/abi/exports_xml2.rs, src/xml/xpath/context.rs, src/xml/parser/state.rs, src/xml/tree/mod.rs, src/xml/list/mod.rs, src/xml/sax/default.rs, src/abi/data_globals.rs, src/xslt/transform/mod.rs, include/libxml, include/libxslt/xsltutils.h, courts/suites/data-abi/callback-family-probe.c, tools/abi/callback_family_probe.py
- **Surface:** xmlXPathRegisterFunc[NS], xmlSetExternalEntityLoader/xmlLoadExternalEntity, xmlRegisterNodeDefault/xmlDeregisterNodeDefault, xmlListAppend/xmlListWalk/xmlListReverseWalk, allocator globals, callback header declarations
- **Oracle versions:** libxml2 2.15.3, libxslt 1.1.45 (system)
- **Root cause:** The 11.1-L callback audit found five real ABI/behavior gaps. (1) xmlXPathRegisterFunc[NS] stored the C function pointer but registered a Rust stub that always errored ('C extension function cannot be called ... not yet supported') — registered XPath functions never ran. (2) The parser's NOENT entity-substitution path left external entities unresolved: xmlLoadExternalEntity (which does consult the registered loader) was never called, so custom entity loaders never fired. (3) xmlRegisterNodeDefault/xmlDeregisterNodeDefault stored the hooks but no node creation/free path invoked them (upstream gates them on the xmlRegisterCallbacks flag). (4) xmlListAppend did a plain push_back (upstream inserts sorted when a comparator is set) and xmlListWalk/xmlListReverseWalk had the stop-return INVERTED (stopped on non-zero; upstream stops on 0). (5) The callback surface was invisible in the headers: xpathInternals.h declared nothing (and the initial extraction captured only first lines of multi-line #define macros, leaving xmlXPathReturn* EMPTY), xmlIO.h lacked the match/open typedefs, parser.h lacked the entity-loader declarations, tree.h lacked the node-registration callbacks, hash.h/list.h/xmlerror.h had non-const or wrong callback typedefs, parserInternals.h had no parser.h include, and libxslt/xsltutils.h lacked xsltDebugSetDefaultTrace/xsltDebugGetDefaultTrace. Separately, the 5 allocator entry points (xmlMalloc/xmlMallocAtomic/xmlRealloc/xmlFree/xmlMemStrdup) were exported as FUNCTIONS while upstream exports them as DATA function-pointer variables (xmlmemory.h XMLPUBVAR) — the documented allocator-override mechanism (xmlMalloc = custom) could not link.
- **Fix:** (1) Built the parser-context bridge: xmlXPathRegisterFunc[NS] now registers a Rust closure that synthesizes the upstream xmlXPathParserContext (value stack + context pointer), pushes the evaluated args as XPath objects, invokes the C function, and pops/converts the result (call_c_xpath_function); the XSLT engine registers a namespaced function_lookup fallback that resolves prefix:local against the stylesheet namespaces and dispatches to xsltFindExtFunction through the same bridge. (2) The NOENT external-entity path now calls xmlLoadExternalEntity and pushes the loaded content as a new input; the DTD SYSTEM-only declaration no longer misstores the URI as the public ID. (3) Added the xmlRegisterCallbacks gate + register_node_hook/deregister_node_hook, invoked from new_doc/new_node/new_text/new_comment/new_pi + the parser's compact-text path and free_doc/free_node. (4) xmlListAppend does the upstream sorted insert; both walks honor the 0-stops return. (5) Closed the callback header surface: full oracle-verbatim xpathInternals.h declarations incl. correctly-extracted multi-line macros + valuePush/valuePop aliases, xmlIO.h callback typedefs + declarations, parser.h entity loader, tree.h node-registration callbacks, hash/list/xmlerror const-correct typedefs, parserInternals.h parser.h include, xsltutils.h debug-trace declarations; fixed xmlSAXHandlerV1 typedef ordering (parser.h) and removed the duplicate xmlSchemaPtr plus wrong xmlXPathValuePush/xmlXPathNodeSetDel int prototypes. The allocator entry points are now DATA function-pointer globals initialized to the impls (xmlMallocImpl etc.), matching the oracle ABI; ~950 internal call sites updated mechanically.
- **Evidence:** ['courts/suites/data-abi/callback-family-probe.c', 'tools/abi/callback_family_probe.py', 'courts/receipts/phase-11/callback-family-*.json (CALLBACK-001 byte-identical)', 'cargo test --lib 1135 pass', 'ASan full-suite clean', 'header court 571/571', '10/10 data-ABI family courts byte-identical']
- **Classification:** CANDIDATE_BUG

## Phase 11.1-M Residuals

### R-000163: Error semantics parity: parser errors emitted wrong codes/levels/messages, wrong or missing columns and source windows, unbuffered-stderr redirect leak, dangling last-error file pointers, missing xmlCtxtGetLastError/xmlFormatError header declarations, shifted XML_ERR_* constants in the Rust types module (FIXED)

- **Status:** FIXED (, Phase 11.1-M)
- **Component:** src/xml/parser/state.rs, src/xml/parser/tokenizer.rs, src/xml/parser/input.rs, src/xml/errors/mod.rs, src/xml/globals/mod.rs, src/abi/data_globals.rs, src/abi/types.rs, src/abi/exports_xml2.rs, include/libxml/xmlerror.h, courts/suites/data-abi/error-family-probe.c, tools/abi/error_family_probe.py
- **Surface:** xmlCtxtVErr/xmlVRaiseError/xmlFormatError/xmlParserPrintFileContextInternal/xmlParserInputGetWindow routing; xmlReadMemory/xmlCtxtReadMemory; xmlGetLastError/xmlCtxtGetLastError/xmlCtxtResetLastError/xmlFormatError; xmlGenericErrorDefaultFunc; XML_ERR_* enum constants; xmlerror.h declarations
- **Oracle versions:** libxml2 2.15.3 (system)
- **Root cause:** The 11.1-M error-semantics audit (ERROR-001 corpus: 48 deterministic malformed/edge inputs x 4 passes each) found that parser diagnostics were not byte-exact. Every parser error was raised with XML_ERR_ERROR level instead of upstream's FATAL, with wrong codes (e.g. 'Unclosed element tag' code 5 instead of 'Premature end of data in tag %s line %d' code 77; 'Empty entity reference' instead of 'EntityRef: expecting ';'' code 23; 'StartTag: invalid element name' code 68), no str1/str2/int1/column fields, and the tokenizer had already consumed past the error position so carets pointed one byte past. The source window ignored upstream's 80-column cap, the caret clamp (col >= n -> size-1, 2.15), the UTF-8-aware forward scan and EOL skip-back. The default stderr handler wrote through a private fully-buffered fdopen(2) FILE* so redirected-fd2 captures landed at exit on the restored fd; the thread-local last error stored dangling file/str1-3 pointers from transient CStrings; the XML_ERR_* constants 64-96 in src/abi/types.rs were a synthetic renumbered enum (SPACE_REQUIRED 89 instead of 65, NAME_REQUIRED 90 instead of 68, GT_REQUIRED 94 instead of 73, TAG_NAME_MISMATCH 95 instead of 76, TAG_NOT_FINISHED 96 instead of 77); xmlReadMemory rejected size 0 so '' never parsed; xmlerror.h lacked xmlCtxtGetLastError/xmlCtxtResetLastError/xmlFormatError/xmlParser* legacy declarations.
- **Fix:** Rebuilt the parser error path end-to-end on upstream routing: a tokenizer error queue records each error at its exact detection point with upstream code/level/message/str1-3/int1/line/char-column/source-window (record_error/record_error_at), the parser drains and raises via raise_parser_error -> raise_error_streamed which now owns file/str1-3 copies, fills int2 with the 1-based char column, streams the xmlFormatError fragment sequence (file/line prefix, domain, level, message with %s vs %s\n, the invalid-encoding 'Bytes:' dump, source window, caret), applies the xmlGetWarningsDefaultValue gate, and updates errNo/wellFormed(FATAL)/nbErrors/nbWarnings per xmlCtxtVErr. All upstream messages/codes/levels per corpus case are reproduced (EntityRef/CharRef/entity-not-defined, attribute errors incl. the 3-error construct sequence, PCDATA invalid Char, ']]>', encoding errors, PI/comment/CDATA/XML-decl, doc-level 'Document is empty'/'Start tag expected'/'Extra content'/invalid element name, Premature end of data, mismatch with str1/str2/int1). The source window replicates xmlParserInputGetWindow incl. the 80-char cap, continuation-byte skip, UTF-8 forward scan and the 2.15 caret clamp; columns are char-based (input->col). The default handler now targets the real glibc stderr FILE* (unbuffered, fd-2 relative) instead of a buffered fdopen copy. xmlReadMemory parses size-0 input. Fixed the XML_ERR_* 64-96 constants to the header/upstream numbering and closed the xmlerror.h declarations. One test fixture (XSLT stylesheet with a missing space between attributes) was corrected.
- **Evidence:** ['courts/suites/data-abi/error-family-probe.c', 'tools/abi/error_family_probe.py', 'courts/receipts/phase-11/error-family-*.json (ERROR-001 byte-identical, 48/48 cases)', 'cargo test --lib 1135 pass', 'ASan full-suite clean', 'header court 571/571', '11/11 data-ABI family courts byte-identical (10 prior + ERROR-001)']
- **Classification:** CANDIDATE_BUG

## Phase 11.1-N Residuals

### R-000164: Parser/tree structural parity: DTD node absent from doc child chain, decl double-frees at doc free, element-decl type_ carrying the element type, entity content not cached in ent->children, entity-ref nodes missing shared content/entity child, duplicate default-ns nsDef entries, xmlns="" href NULL, parsed attr atype != 0, ATTLIST for undeclared element dropped, attribute hash key order, missing parse-time ID/IDREF registration, xmlGetLineNo type/-1 walk, xmlReadMemory options/URL handling, CDATA node name, standalone=-2, NOBLANKS, missing xmlns relative-URI warning, copy_node parent/last/line (FIXED)

- **Status:** FIXED (, Phase 11.1-N)
- **Component:** src/xml/parser/state.rs, src/xml/parser/tokenizer.rs, src/xml/sax/default.rs, src/xml/tree/mod.rs, src/xml/dtd/mod.rs, src/xml/entities/mod.rs, src/xml/validation/mod.rs, src/abi/exports_xml2.rs, src/abi/exports_tree.rs, src/abi/exports_treedump.rs, src/abi/exports_parser.rs, include/libxml/tree.h, courts/suites/data-abi/tree-structure-probe.c, tools/abi/tree_structure_probe.py
- **Surface:** xmlReadMemory/xmlCreateIntSubset/xmlNewDtd/xmlFreeDoc/xmlFreeNodeList/xmlFreeNode/xmlFreeDtd/xmlAddElementDecl/xmlAddAttributeDecl/xmlAddEntity/xmlGetDtdElementDesc/xmlNewReference/xmlSAX2Reference/xmlSAX2InternalSubset/xmlSAX2StartElementNs/xmlSAX2AttributeNs/xmlIsID/xmlAddID/xmlGetLineNo/xmlCopyNode/xmlSetProp; parser options NOBLANKS/COMPACT/NOENT/RECOVER/PEDANTIC; namespace relative-URI warnings; DTD hash laziness and (name,prefix,elem) key order
- **Oracle versions:** libxml2 2.15.3 (system)
- **Root cause:** The 11.1-N TREE-001 differential probe (27-block structural fingerprint of 20 corpus docs x 8 option variants plus mutation checks) found the tree the parser builds was not byte-identical to libxml2's. The SAX internalSubset handler used xmlNewDtd instead of xmlCreateIntSubset so the DTD node never joined doc->children. xmlFreeDtd/xmlFreeNodeList freed declaration nodes from the DTD child list in addition to the hash tables that own them (double free; also the element-decl node type_ held the ELEMENT_TYPE value (4) instead of XML_ELEMENT_DECL (15), breaking the skip check), and the string blocks in dtd::free_dtd were duplicated. Entity references were not modeled like xmlNewReference (content shared with the entity, child list = the entity decl) and entity content was never parsed into ent->children with the XML_ENT_PARSED/XML_ENT_EXPANDING flags. The default namespace declaration was registered twice (once from the URI param, once from the namespaces array) and xmlns="" produced href=NULL instead of "". Parsed and xmlSetProp attributes carried atype=XML_ATTRIBUTE_CDATA instead of 0. xmlParseAttlistDecl dropped ATTLISTs for undeclared elements instead of creating the UNDEFINED element (xmlGetDtdElementDesc semantics). The attribute hash was keyed (elem,name) instead of upstream's (name,prefix,elem), and parse-time ID/IDREF registration (xmlIsID/xmlAddID) was missing so ids stayed NULL and atype never became XML_ATTRIBUTE_ID. xmlGetLineNo returned int instead of long and lacked the -1 walk for non-element nodes; xmlReadMemory skipped apply_options (dictNames/keepBlanks) and set doc->URL only on the success path; CDATA nodes carried name="cdata"; the XML declaration did not set standalone=-2; NOBLANKS did not drop blank-only runs; the xmlns relative-URI warning was missing; copy_node did not maintain child parent/last pointers and copied line numbers for text nodes.
- **Fix:** Aligned the parse-time DOM construction with upstream: internalSubset now routes through create_int_subset (unlink+free of a pre-existing subset, DTD inserted before the first element node); xmlFreeDtd (all three copies) frees only non-declaration children and runs the child walk before the hash-table frees; free_node routes decl/ns/attr nodes to their dedicated free functions and never descends into entity-ref children; add_element_decl stores XML_ELEMENT_DECL in type_ and the element type in etype; DTD hash tables are created lazily by the xmlAdd* functions; entity content is parsed into ent->children on first reference (parse_entity_content, XML_ENT_PARSED/EXPANDING guards, coalesced text nodes at line 1) and the reference handler builds nodes with xmlNewReference semantics; namespaces are built from the namespaces array only with own-decl ns resolution (no duplicate default-ns nsDef, xmlns="" yields href=""); instance attributes keep atype=0; parse_attlist_decl uses get_element_decl_created (UNDEFINED placeholder, not linked to DTD children); the attribute table is keyed (name,prefix,elem) in add/get/validate; startElementNs registers ID/IDREF attributes via xmlIsID/xmlAddID (doc->ids, atype=XML_ATTRIBUTE_ID); xmlGetLineNo returns long with the upstream -1 walk; xmlReadMemory applies options and sets doc->URL on both success and recovery paths; CDATA nodes have NULL names; the XML declaration sets standalone=-2; NOBLANKS drops blank-only runs; the default-ns relative-URI warning (XML_WAR_NS_URI_RELATIVE=100, exact caret after the value) is raised for scheme-less URIs (prefixed form only in pedantic mode); copy_node preserves line only for elements, sets parent/last across copied children, and text copies keep line 0. The probe captures default-handler stderr into the fingerprint. Two unit tests asserting the old type_ semantics were corrected.
- **Evidence:** ['courts/suites/data-abi/tree-structure-probe.c', 'tools/abi/tree_structure_probe.py', 'courts/receipts/phase-11/tree-structure-20260830T161639Z.json (TREE-001 byte-identical, 0 mismatch lines)', 'cargo test --lib 1135 pass / 0 fail', 'ASan probe run clean (1134-test suite clean; one pre-existing schematron UB test excluded, reproduced without these changes)', 'header court 571/571']
- **Classification:** CANDIDATE_BUG

## Phase 11.1-O Residuals

### R-000165: 65 oracle-DSO exports absent from the candidate (xmlCtxtGet*/Set* parser accessors, xmlNewInputFrom* input constructors, xlink surface, per-module EXSLT registration functions, resource-loader setters, html/encoding/relaxng/xsd/reader/xinclude gaps, xslDebugStatus) (FIXED)

- **Status:** FIXED (, Phase 11.1-O)
- **Component:** src/abi/exports_xml2.rs, src/abi/exports_parserint.rs, src/abi/exports_parser.rs, src/abi/exports_tree.rs, src/abi/exports_misc.rs, src/abi/exports_nano.rs, src/abi/exports_html.rs, src/abi/exports_relaxng.rs, src/abi/exports_schema.rs, src/abi/exports_xinclude.rs, src/abi/exports_xslt.rs, src/abi/exports_xslt_ext.rs, src/abi/exports_string.rs, src/abi/exports_buffer.rs, tools/evidence/subsystem_census.py, atlas/SUBSYSTEM_CENSUS.json
- **Surface:** Parser context accessors (xmlCtxtGetCatalogs/xmlCtxtSetDict/xmlCtxtIsHtml/xmlCtxtParseContent/xmlCtxtPushInput/xmlCtxtValidateDocument family), input-stream constructors (xmlNewInputFromFd/IO/Memory/String/Url), xlink (xlinkGetDefaultDetect/xlinkIsLink family), EXSLT (exsltCommonRegister/exsltMathRegister/... per-module registration), resource loaders (xmlSchemaSetResourceLoader/xmlRelaxNGSetResourceLoader/xmlTextReaderSetResourceLoader/xmlXIncludeSetResourceLoader/xmlCtxtSetResourceLoader), htmlCtxtSetOptions/htmlUTF8ToHtml, xmlIsolat1ToUTF8/xmlUTF8ToIsolat1, xmlRelaxNGValidCtxtClearErrors/xmlRelaxParserSetIncLImit, xslDebugStatus
- **Oracle versions:** libxml2 2.15.3 / libxslt 1.1.45 / libexslt (system DSOs, nm -D --defined-only)
- **Root cause:** The 11.1-O complete subsystem census (tools/evidence/subsystem_census.py; 45 libxml2 + 24 libxslt + 8 EXSLT subsystems, membership from Doxygen inventory + Clang AST atlas + symbol patterns, oracle baseline = system DSO exports) found 65 oracle-DSO-exported symbols with no candidate definition. The PARITY_OBLIGATIONS ledger reported MISSING: 0 because the obligations generator's oracle symbol set omitted these families (parser-context accessors, the xlink module, per-module EXSLT registrations, resource-loader setters and several small helpers), so the export-completeness claim was not actually proven for them.
- **Observable residual:** A C consumer dlopen/dlsym'ing or linking against the candidate for xmlCtxtGetDict, xmlNewInputFromMemory, xlinkIsLink, exsltMathRegister, xmlSchemaSetResourceLoader, xmlUTF8ToIsolat1, xslDebugStatus, etc. fails with an undefined symbol. dlsym returns NULL for each of the 65 names in atlas/SUBSYSTEM_CENSUS.json missing_symbols.
- **Fix:** 11.1-X: all 65 oracle-DSO exports absent at discovery are now exported and verified: parser accessors (xmlCtxtGet*/Set*), input constructors (xmlNewInputFrom*), the xlink surface (xlinkIsLink), per-module EXSLT registration (exsltMathRegister et al.), resource-loader setters (xmlSchemaSetResourceLoader), html/encoding/relaxng/xsd/reader/xinclude gaps, and xslDebugStatus. The subsystem census (atlas/SUBSYSTEM_CENSUS.json) enumerates the symbols; the dso-loader court resolves each from the built DSO (25/25) and the header-compile court compiles every public header against it (595/595).
- **Regression courts:** DSO-LOADER, HEADER-COMPILE.
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-08-30 (discovered by the 11.1-O subsystem census; recorded OPEN with the full 65-symbol list in atlas/SUBSYSTEM_CENSUS.json (missing_symbols per subsystem); closure scheduled for the 11.1-X residual closure loop with a DSO-LOADER family court | 11.1-S header-surface audit: with the complete upstream header surface installed (all 2.15.3/1.1.45/0.8.25 header declarations now present in include/), the header-compile court check #5 exposes 63 of the 65 symbols as declared-but-not-exported. The explicit residual allowlist courts/suites/header-compile/residual-exports.txt tracks them (owned by this residual); it must be empty for the 11.1-Z seal. | 11.1-W generated-parity-matrix work: the obligations generator now covers three projects (libexslt added as its own oracle DSO) and the xlink* / xsl* prefixes; regeneration against the true system oracle (/usr/lib/libxml2.so.16, 21503-GITv2.15.3) surfaces 69 missing obligations total: 52 libxml2 (47 xml/html/__xml + 5 xlink), 1 libxslt (xslDebugStatus, data), 16 libexslt (12 per-module exslt*Register + exsltLibexsltVersion/exsltLibraryVersion/exsltLibxmlVersion/exsltLibxsltVersion data). All 69 remain to be implemented in 11.1-X; the header-compile residual allowlist (courts/suites/header-compile/residual-exports.txt) tracks the declared-but-unexported subset and must be empty for the 11.1-Z seal. | fixed in 11.1-X; 65 symbols exported; dso-loader 25/25, header-compile 595/595); FIXED 2026-08-31 (fixed in 11.1-X; 65 symbols exported; dso-loader 25/25, header-compile 595/595)

## Phase 11.1-P Residuals

### R-000166: Standards three-way divergences: candidate misses WFC diagnostics (comment '--', '<' in attr values), namespace-declaration errors (xmlns:p="", xmlns:xml wrong URI, XML ns as default), C14N absolute-URI rule + inclusive ns propagation, XSLT number formatting (format-number empty, value-of full double precision) (FIXED)

- **Status:** FIXED (, Phase 11.1-P)
- **Component:** src/xml/parser/state.rs, src/xml/parser/tokenizer.rs, src/xml/sax/default.rs, src/abi/exports_xslt_apply.rs, src/abi/exports_xslt_exec.rs, src/abi/exports_xptr.rs, src/xml/xpath/mod.rs, src/xml/c14n/mod.rs, tools/evidence/standards_reconciliation.py, atlas/standards/STANDARDS_RECONCILIATION.json
- **Surface:** xmlParseComment WFC 'Double hyphen within comment'; xmlParseAttValue 'Unescaped <' not allowed'; xmlParseStartTag2 namespace declaration errors (XML_NS_ERR_XML_NAMESPACE family); xmlC14NExecute absolute-URI enforcement and inclusive namespace propagation; libxslt xsltFormatNumberConversion / xmlXPathCastNumberToString (XSLT 1.0 12.3 / 7.6)
- **Oracle versions:** libxml2 2.15.3 / libxslt 1.1.45 (xmllint/xsltproc probes)
- **Root cause:** The 11.1-P three-way reconciliation (SPECIFICATION / UPSTREAM ORACLE / LIBXML-RS, 14 standards areas, executable probes on both binaries) found four candidate divergence clusters the existing courts do not cover: (1) well-formedness WFC diagnostics for comments containing '--' and '<' inside attribute values are not raised; (2) namespace-declaration errors (empty xmlns:p="", xmlns:xml mapped to a wrong URI, XML namespace URI as default namespace) are accepted silently; (3) canonicalization accepts relative namespace URIs that upstream rejects with 'Failed to canonicalize' and re-declares in-scope namespaces per element (inclusive C14N propagation needs audit); (4) XSLT number formatting diverges hard: format-number(1234567.891,'#,##0.00') yields empty output (oracle '1,234,567.89') and value-of 1234567.891 prints full double precision '1234567.891000000061467' (oracle '1234567.891').
- **Observable residual:** xmllint probes: '<a><!-- a -- b --></a>', '<a b="x < y"/>', '<a xmlns:p=""><p:b/></a>', '<a xmlns:xml="urn:x"/>', '<a xmlns="http://www.w3.org/XML/1998/namespace"/>' all parse without the oracle's diagnostics; 'xmllint --c14n' on relative-URI docs canonicalizes instead of failing; xsltproc format-number produces empty output with rc=0.
- **Fix:** 11.1-X: all four standards divergence clusters are closed with oracle-verified differential courts. (1) WFC diagnostics: '<' in attribute values (XML_ERR_LT_IN_ATTRIBUTE, caret at the offending '<', exit 4) and '--' in comments match the oracle byte-for-byte. (2) Namespace-declaration errors: empty xmlns:p="", xmlns:xml wrong URI, XML ns as default, and undefined prefixes on elements and attributes (XML_NS_ERR_UNDEFINED_NAMESPACE, caret at the tag end) match, including the double-report on <a xmlns:p=""><p:b/></a>; ancestor-declared prefixes stay silent. (3) C14N: the relative-URI rejection ('Failed to canonicalize', exit 6) now applies in BOTH inclusive and exclusive modes; inclusive namespace propagation was rebuilt as a faithful port of xmlC14NProcessNamespacesAxis + xmlExcC14NProcessNamespacesAxis (ns_rendered prefix-scoped find, rebinding chains, xmlns="" undeclarations, the xml namespace never rendered, lexicographic prefix sorting, document-level PI/comment newlines, CR normalization); subset canonicalization now implements the visibility node-set semantics (orphan xml:lang/xml:space inheritance, xml:base fixup, invisible elements processed but not rendered) and the C ABI signature of xmlC14NDocDumpMemory/xmlC14NDocSaveTo/ xmlC14NDocSave was corrected to xmlNodeSet* (upstream). (4) XSLT number formatting: format-number() is the canonical numbers.c port (CLI-XSLTPROC-0014/0015/0017); value-of full double precision is the xmlXPathFormatNumber port (integer shortcut, 1e9/1e-5 scientific threshold, DBL_DIG=15 fraction digits, e+NN/e-NN exponent form, trailing-zero trim); number parsing (xmlXPathCompNumber literal lexer + xmlXPathStringEvalNumber string-to-number) reproduces the oracle's digit accumulation, MAX_FRAC=20 cap and pow(10,exp) underflow (5e-324 -> 0).
- **Regression courts:** CLI-XSLTPROC-0014, CLI-XSLTPROC-0015, CLI-XSLTPROC-0017, C14N, test_c14n_exclusive_skips_ancestor_rendered_ns, test_c14n_namespace_sorting, test_c14n_xml_ns_never_rendered, test_c14n_empty_default_undeclaration, test_c14n_relative_ns_rejected_exclusive, test_c14n_pi_document_level_newlines, test_c14n_subset_visibility, test_c14n_subset_hidden_parent_xml_lang, test_c14n_rebinding_chain_rere_declares, test_xml_number_to_string_parity_cases.
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-08-30; FIXED 2026-08-31 (fixed in 11.1-X; 246/246 CLI C14N matrix + 576/576 C-API C14N matrix + 967/967 number() corpus + ns/wfc probes byte-identical; 1173 lib tests pass)

## Phase 11.1-S Residuals

### R-000167: xsltLibxsltVersion exported as a function; upstream 1.1.45 declares it as a read-only data variable (ABI type divergence) (FIXED)

- **Status:** FIXED (, Phase 11.1-S)
- **Component:** src/abi/versioning.rs, src/abi/data_globals.rs, src/abi/exports_xslt.rs, include/libxslt/xslt.h, include/libxslt/transform.h
- **Surface:** libxslt version reporting data symbol: xsltLibxsltVersion (upstream XSLTPUBVAR const int in xslt.h; oracle DSO symbol type R). xsltLibxsltVersionString is a candidate-only extra (absent from upstream headers and DSO).
- **Oracle versions:** libxslt 1.1.45 (system DSO /usr/lib/libxslt.so.1; headers /usr/include/libxslt/xslt.h)
- **Root cause:** 11.1-S version-reporting audit: upstream libxslt exposes xsltLibxsltVersion as a const int data symbol (XSLTPUBVAR const int xsltLibxsltVersion; symbol type R in nm). The candidate exports a #[no_mangle] function of the same name (symbol type T). A consumer reading the value per the header contract links against a function symbol and reads code bytes as an int. xsltLibxsltVersionString has no upstream counterpart at all.
- **Observable residual:** nm -D --defined-only /usr/lib/libxslt.so.1 shows 'R xsltLibxsltVersion' while the candidate shows 'T xsltLibxsltVersion'. A C consumer declaring extern const int xsltLibxsltVersion (upstream header) gets a garbage value at runtime.
- **Fix:** 11.1-X: the exported symbol types now match the oracle DSO (nm -D verified against the system oracle): xsltLibxsltVersion is a data symbol (R), xsltEngineVersion a data symbol (D), exsltLibexsltVersion/exsltLibxsltVersion are data symbols (R), and exsltLibraryVersion a data symbol (D) — upstream 1.1.45 declares all four as read-only data variables, not functions.
- **Regression courts:** DSO-LOADER, ABI-DATA.
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-08-30 (discovered by the 11.1-S build/version-reporting audit; the header-compile court keeps xslt.h's upstream-correct XSLTPUBVAR declaration while the build-pkgconfig court deliberately avoids the divergent API (see courts/suites/build-pkgconfig/test-libxslt.c note). Closure scheduled for 11.1-X: export the value as a data symbol (const int, matching upstream R type) and drop/rename the function-form export. | 11.1-T DSO-LOADER court confirms the divergence at the dynamic-loader level: dlsym(xsltLibxsltVersion) resolves to a function symbol (T) while the oracle exports it as read-only data (R). The court's symbol-type parity check excludes this one symbol by design and defers to this residual. Also fixed in 11.1-T: the DSO now carries the upstream SONAME libxml2.so.16 (consumers record NEEDED=libxml2.so.16 like oracle-linked binaries), and top-level versioned symlinks make LD_LIBRARY_PATH=<artifact> resolve all three SONAMEs to the candidate (a missing libdir in the path previously let the loader silently fall back to the system libxml2 — contamination, now guarded by the court's dladdr identity check). | fixed in 11.1-X; nm -D symbol-type comparison matches the oracle for all four version symbols); FIXED 2026-08-31 (fixed in 11.1-X; nm -D symbol-type comparison matches the oracle for all four version symbols)

## Phase 11.1-U Residuals

### R-000168: Platform surface: runtime execution unexecuted outside Linux x86-64 (Windows DLL ABI, macOS dylib naming, BSD/POSIX variants, 32-bit/arm64/musl runtime, big-endian) (OPEN)

- **Status:** OPEN
- **Component:** atlas/PLATFORM_SURFACE_ATLAS.json, atlas/PLATFORM_SURFACE_ATLAS.md, tools/evidence/platform_surface_atlas.py, src/xml/errors/mod.rs, src/abi/exports_schema.rs, src/abi/exports_xslt_functions.rs, src/abi/exports_xslt_util.rs, src/abi/exports_shell.rs, src/exslt/dates/mod.rs
- **Surface:** Platform-conditioned API/ABI families: threads (HAVE_POSIX_THREADS/HAVE_WIN32_THREADS/USE_TLS), file IO (_WIN32/HAVE_DECL_MMAP/HAVE_DECL_GLOB), module loading (HAVE_DLOPEN/_WIN32/HAVE_SHLLOAD), encoding iconv (__APPLE__), libxslt locale (XSLT_LOCALE_POSIX/WINAPI/NONE), export macros (XMLCALL/XMLPUBFUN/XMLPUBVAR dllexport), config detection (HAVE_*/SIZEOF_*), word-size/endian behavior.
- **Oracle versions:** libxml2 2.15.0 source (oracle/historical/src), libxslt 1.1.45 (archaeology), config.h/xmlversion.h captures
- **Root cause:** The reference system executes only Linux x86-64. 11.1-U classified every upstream platform-conditional family from source archaeology and generated cross-compile expectations for the available targets. Cross-compilation exposed and fixed real portability defects: x86_64-only cfg gates on the streamed generic-error channel (now a portable fallback on other ABIs), c_ulong-vs-u64 width bugs in xmlSchemaValidateFacetWhtsp and generate-id(), c_long calibration arithmetic, c_char(u8-on-aarch64) buffer typing in the xmlShell debugger, LC_ALL_MASK missing on musl, and i32 time_t on 32-bit (y2038 — inherent to 32-bit time_t).
- **Observable residual:** On Windows/macOS/BSD/big-endian and for 32-bit/arm64/musl RUNTIME execution there is no executed evidence; the atlas documents each obligation explicitly (OBLIG-PLATFORM-*) so the surface cannot silently disappear.
- **Classification:** INTENTIONAL_SAFE_DIVERGENCE

## Phase 11.1-X Residuals

### R-000169: Dangling doc->URL / parser-input filename: xml_strdup on a non-NUL-terminated Rust String (heap-buffer-overflow) and borrowed filename pointers in parserInternals input construction (FIXED)

- **Status:** FIXED (, Phase 11.1-X)
- **Component:** src/xml/parser/helpers.rs, src/abi/exports_parserint.rs, src/xml/xpath/parser_context.rs
- **Surface:** doc->URL, xmlNodeGetBase, _xmlParserInput.filename lifecycle (xmlReadMemory/xmlCtxtRead* and xmlParseCtxtExternalEntity/xmlParseBalancedChunkMemoryRecover/xmlParseInNodeContext/xmlParseDTD paths); XPath pop_string
- **Oracle versions:** libxml2 2.15.3 (system)
- **Root cause:** The TREE-001 structural probe (11.1-N) observed URL=t.xml<V> (heap-reuse garbage appended to the URL) and the same for xmlNodeGetBase. Two defects: (1) alloc_parser_input duplicated the filename with xml_strdup(fname.as_ptr()) where fname is a Rust String whose as_ptr() is NOT NUL-terminated — xml_strlen scans past the allocation (ASan: heap-buffer-overflow) and the copy lands in freed/reused memory; (2) the four parserInternals entry points used populate_parser_input directly, which borrows the boxed InputBuffer's Rust String into _xmlParserInput.filename — dangling once the context is freed. xpath pop_string had the same non-NUL-terminated xml_strdup defect.
- **Observable residual:** doc->URL / base print t.xml followed by heap-reuse garbage (non-deterministic single character) on the second parse; ASan heap-buffer-overflow in xml_strdup via alloc_parser_input.
- **Fix:** alloc_parser_input and the parserInternals sites now duplicate filenames with xml_strndup(fname.as_ptr(), fname.len()) (exact length, explicit NUL); populate_parser_input was replaced by populate_parser_input_without_filename + owned dup at all four parserInternals sites; pi_parse_content_node_list frees the popped input (struct + owned filename) and pi_pop_pe frees the filename before the struct, making every _xmlParserInput free path symmetric with free_parser_input; xpath pop_string uses xml_strndup. The input filename is now owned uniformly across every construction path.
- **Evidence:** ['courts/suites/data-abi/tree-structure-probe.c', 'tools/abi/tree_structure_probe.py', 'courts/receipts/phase-11/tree-structure-20260831T053510Z.json (TREE-001 byte-identical, 0 mismatch lines)', 'ASan repro clean (second-parse URL=[t.xml])', 'cargo test --lib 1146 pass / 0 fail']
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-08-31 (discovered while sealing 11.1-X: TREE-001 probe mismatch URL=t.xmlV and base=t.xmlU; ASan pinned xml_strdup on a Rust String as_ptr in alloc_parser_input); FIXED 2026-08-31 (fixed by owning the filename at every _xmlParserInput construction path (xml_strndup, populate_parser_input_without_filename) and symmetric frees; TREE-001 byte-identical PASS)

### R-000170: xmlLastError global mirror races: concurrent sync/reset double-free the mirror strings (FIXED)

- **Status:** FIXED (, Phase 11.1-X)
- **Component:** src/abi/data_globals.rs, src/xml/globals/mod.rs
- **Surface:** xmlLastError data symbol; set_last_error/reset_last_error mirror sync (xml/globals, abi/data_globals)
- **Oracle versions:** libxml2 2.15.3 (system)
- **Root cause:** The exported xmlLastError mirror is deep-copied on every error raise (sync_xml_last_error) and freed on reset (reset_xml_last_error) with no synchronization. Two threads raising/resetting concurrently free the same mirror strings (or free strings just installed by the other thread): glibc 'double free or corruption (!prev)' aborts in the parallel lib test suite (~10% of runs, victim tests anywhere). Pre-existing: reproduced at the committed 11.1-W state (12/100 aborts).
- **Observable residual:** SIGABRT 'double free or corruption' under parallel error raising (the full parallel test suite, any allocating test as victim).
- **Fix:** sync_xml_last_error/reset_xml_last_error are serialized by LAST_ERROR_MIRROR_LOCK (parking_lot::Mutex); the internal helpers reset_xml_last_error_locked/sync_xml_last_error_locked run under the lock with no re-lock. C consumers reading the symbol directly keep upstream's documented racy semantics. Two regression courts (test_last_error_mirror_concurrent_sync_reset, test_last_error_mirror_many_threads) hammer the interleavings.
- **Evidence:** ['cargo test --lib 1146 pass / 0 fail', '100/100 parallel full-suite runs clean (was ~12/100 SIGABRT at 11.1-W)', 'test_last_error_mirror_concurrent_sync_reset', 'test_last_error_mirror_many_threads']
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-08-31 (discovered while sealing 11.1-X: parallel lib suite aborts; bisected to xml::errors tests racing with any other raising thread; reproduced at committed 11.1-W); FIXED 2026-08-31 (fixed with the mirror lock; 0/100 parallel-suite aborts after the fix)

### R-000171: Error-handler slot pairs (xmlStructuredError/xmlGenericError + contexts) read/written non-atomically; handler-slot tests race (FIXED)

- **Status:** FIXED (, Phase 11.1-X)
- **Component:** src/xml/globals/mod.rs, src/xml/errors/mod.rs, src/abi/data_globals.rs
- **Surface:** xmlSetGenericErrorFunc/xmlSetStructuredErrorFunc, get_generic_error_ctx/get_structured_error_ctx, raise_error dispatch
- **Oracle versions:** libxml2 2.15.3 (system)
- **Root cause:** The exported handler slots were read/written as two independent static mut globals, so a reader could observe a new handler with an old context (or vice versa), and test_error_callbacks_default_handlers (xml::globals) could observe another test's temporarily-installed structured handler and fail its assertions (~30% of parallel runs).
- **Observable residual:** Incoherent (handler, ctx) pairs under concurrent set; flaky test_error_callbacks_default_handlers assertion failure.
- **Fix:** Handler slot pairs are now written and read atomically under ERROR_HANDLER_LOCK; with_structured_error/with_generic_error read the pair under the lock and invoke the callback outside it (no deadlock on re-entrant error raising). The three handler-mutating tests are serialized by ERROR_HANDLER_TEST_LOCK.
- **Evidence:** ['cargo test --lib 1146 pass / 0 fail', '100/100 parallel full-suite runs clean', 'test_error_callbacks_default_handlers', 'test_error_callbacks_set_and_get', 'test_structured_error_callback']
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-08-31 (discovered while sealing 11.1-X: after the R-000170 fix the parallel suite surfaced flaky handler-slot assertion failures); FIXED 2026-08-31 (fixed with the handler-pair lock and test serialization; 100/100 parallel-suite runs clean)

## Phase 11.1-Z Residuals

### R-000172: xsl:value-of / xsl:copy-of atomic casts append an empty text node when the cast string is empty; empty result elements serialize as <out></out> instead of the oracle <out/> (FIXED)

- **Status:** FIXED (, Phase 11.1-Z)
- **Component:** src/xslt/transform/mod.rs
- **Surface:** xsltValueOf / xsltCopyOf result construction
- **Oracle versions:** libxslt 1.1.45 (system)
- **Root cause:** The candidate cast the XPath result to string and appended a text node unconditionally. Upstream xsltValueOf/xsltCopyOf (transform.c 1.1.45) guard with `if (value[0] != 0)` — an empty cast result must not create a text node, otherwise an otherwise-empty element serializes as <out></out> rather than the oracle's <out/>.
- **Fix:** 11.1-Z: process_value_of and the copy-of atomic path now skip append_text_node when the first byte of the cast string is NUL, matching upstream's `if (value[0] != 0)` guard. New regression case CLI-XSLTPROC-0020 (byte-identical vs the oracle).
- **Regression courts:** CLI-XSLTPROC-0020.
- **Evidence:** ['oracle/historical/src/libxslt-1.1.42/libxslt/transform.c (xsltValueOf, xsltCopyOf)']
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-08-31 (discovered during 11.2 custodian commentary audit); FIXED 2026-08-31 (fixed in 11.1-Z/11.2; CLI-XSLTPROC-0020 byte-identical)

### R-000173: XPath child axis includes the DTD node when the source document has a DOCTYPE; /root matches the doctype name and string(/root) returns the DTD's empty value (FIXED)

- **Status:** FIXED (, Phase 11.1-Z)
- **Component:** src/xml/xpath/axes.rs
- **Surface:** XPath child axis traversal
- **Oracle versions:** libxml2 2.15.3 (system)
- **Root cause:** child_axis walked doc->children including the XML_DTD_NODE that heads the list. Upstream xmlXPathNextChild/xmlXPathNextChildElement (xpath.c) either start at the root element (xmlDocGetRootElement for name tests) or skip XML_DTD_NODE while walking; the XPath 1.0 child axis of the document contains only element/text/comment/PI nodes.
- **Fix:** 11.1-Z: child_axis now skips XML_DTD_NODE (type 14) before applying the node test. count(/root) returns 1 (not 2) and string(/root) returns the element text (not the DTD's empty value) on DOCTYPE documents. New regression case CLI-XSLTPROC-0021 (byte-identical vs the oracle).
- **Regression courts:** CLI-XSLTPROC-0021.
- **Evidence:** ['oracle/historical/src/libxml2-2.15.0/xpath.c (xmlXPathNextChild, xmlXPathNextChildElement)']
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-08-31 (discovered during 11.2 custodian commentary audit); FIXED 2026-08-31 (fixed in 11.1-Z/11.2; CLI-XSLTPROC-0021 byte-identical)

### R-000174: xsltGenericDebug exported as a function; upstream libxslt declares it as a data variable (xmlGenericErrorFunc function pointer, oracle symbol type D) (FIXED)

- **Status:** FIXED (, Phase 11.1-Z)
- **Component:** src/abi/data_globals.rs, src/xslt/errors/mod.rs
- **Surface:** xsltGenericDebug, xsltSetGenericDebugFunc (xsltutils.h)
- **Oracle versions:** libxslt 1.1.45 (system)
- **Root cause:** The candidate exported xsltGenericDebug as a #[no_mangle] extern fn (symbol type T). Upstream xsltutils.c:632 defines `xmlGenericErrorFunc xsltGenericDebug = xsltGenericDebugDefaultFunc;` — a global function-pointer DATA variable (oracle nm type D) defaulting to a handler that writes the message to xsltGenericDebugContext (a FILE*) when it is non-NULL. The 11.1-Z.1 full-surface type scan (every oracle symbol vs the candidate DSOs) found this single remaining R-000167-class ABI type divergence.
- **Fix:** 11.1-Z.1: xsltGenericDebug is now exported as DATA from src/abi/data_globals.rs (`#[no_mangle] pub static mut xsltGenericDebug: Option<xmlGenericErrorFunc> = Some(XSLT_GENERIC_DEBUG_DEFAULT);`) with a faithful default handler reproducing upstream xsltGenericDebugDefaultFunc (NULL-context suppression + write to the context FILE*). xsltSetGenericDebugFunc now writes the exported data global and xsltGenericDebugContext (upstream xsltutils.c:650 semantics). The old exported fn was removed. Full-surface type scan: 0 mismatches across libxml2/libxslt/libexslt.
- **Regression courts:** DSO-LOADER, DATA-GLOBALS-001.
- **Evidence:** ['courts/suites/dso-loader/court-runner.sh (symtype:xsltGenericDebug=D)', 'atlas/PARITY_MATRIX.json (libxslt exported_data fully_reconciled)']
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-08-31 (discovered by the 11.1-Z.1 full-surface symbol-type scan (parity matrix libxslt data reconciliation 10/11)); FIXED 2026-08-31 (fixed in 11.1-Z.1; xsltGenericDebug exported as data with the upstream default handler; type scan 0 mismatches)

### R-000175: xmlC14NDocDumpMemory serializes unexpanded entity references; upstream c14n.c fails canonicalization with -1 (XML_ENTITY_REF_NODE is an invalid node) (FIXED)

- **Status:** FIXED (, Phase 11.1-Z)
- **Component:** src/xml/c14n/mod.rs
- **Surface:** xmlC14NDocDumpMemory, xmlC14NDocSaveTo, xmlC14NExecute (c14n.h)
- **Oracle versions:** libxml2 2.15.3 (system)
- **Root cause:** The candidate's c14n_serialize_node emitted the reference text (&foo;) for XML_ENTITY_REF_NODE nodes. Upstream c14n.c xmlC14NProcessNode treats XML_ENTITY_REF_NODE (and XML_ENTITY_NODE / XML_NAMESPACE_DECL) as invalid: xmlC14NErrInvalidNode(ctx, "XML_ENTITY_REF_NODE", "processing node") and the whole dump returns -1. A C consumer that depends on the -1 error (e.g. rejecting unexpanded entity documents) silently got canonical output instead. Found by the 11.1-Z.1 broad differential run of the C14N API probe over the full CLI corpus (dclent.xml).
- **Fix:** 11.1-Z.1: C14nContext gains an invalid_node flag set by the XML_ENTITY_REF_NODE branch; all three dump paths (c14n_doc_dump_memory, c14n_execute, c14n_doc_save_to) return -1 with no output when it is set — matching the oracle. New regression test test_c14n_entity_ref_node_fails_like_upstream (parses a DTD doc with NOENT unset, expects -1 + NULL result); the C14N API probe differential over the full CLI corpus is 0 mismatches.
- **Regression courts:** test_c14n_entity_ref_node_fails_like_upstream, CLI-XMLLINT-0043, CLI-XMLLINT-0044, CLI-XMLLINT-0045, CLI-XMLLINT-0046, CLI-XMLLINT-0047.
- **Evidence:** ['courts/suites/data-abi/c14n-api-probe.c', 'oracle/historical/src/libxml2-2.15.0/c14n.c (xmlC14NProcessNode, xmlC14NErrInvalidNode)']
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-08-31 (discovered by the 11.1-Z.1 broad C14N API differential (dclent.xml)); FIXED 2026-08-31 (fixed in 11.1-Z.1; entity-ref nodes fail the dump with -1 like the oracle; regression test + 0-mismatch corpus differential)

## Phase 11.1-Z.2 Residuals

### R-000176: Function-signature ABI plane: the allocator hooks and 20+ exported functions had C prototypes diverging from the 2.15.3 oracle (missing args, wrong returns, shifted register layouts) (FIXED)

- **Status:** FIXED (, Phase 11.1-Z.2)
- **Component:** src/abi/allocator.rs, src/abi/exports_xml2.rs, src/abi/exports_automata.rs, src/abi/exports_xslt.rs, src/abi/exports_xslt_compile.rs, src/abi/exports_xslt_ext.rs, src/abi/exports_xslt_vars.rs, src/abi/callbacks.rs, src/abi/types.rs, src/xml/entities/mod.rs, src/xml/dtd/mod.rs, src/xml/c14n/mod.rs, src/xml/automata/mod.rs, src/xml/regex/mod.rs, src/xml/xpath/exports.rs, src/xml/errors/mod.rs, src/xml/string.rs, src/xml/list/mod.rs, src/xml/reader/mod.rs, src/xml/schematron/mod.rs, src/xslt/templates/mod.rs, src/xslt/documents/mod.rs, tools/abi/function_signature_court.py, include/libxml/tree.h, include/libxml/uri.h, include/libxml/debugXML.h, include/libxml/xmlreader.h, include/libxml/xmlwriter.h, include/libxml/xmlregexp.h, include/libxslt/extensions.h, include/libxslt/documents.h, include/libxslt/xsltInternals.h
- **Surface:** xmlMemSetup/xmlMemGet/xmlGcMemSetup/xmlGcMemGet (xmlmemory.h); xmlAddAttributeDecl/xmlAddElementDecl/xmlAddNotationDecl/xmlAddEntity/xmlValidateIDRef/xmlValidateIDRefs/xmlSplitQName/xmlSplitQName3 (valid.h/entities.h/parserInternals.h); xmlC14NExecute (c14n.h); xmlAutomataCompile/xmlAutomataNewCountTrans/xmlAutomataNewOnceTrans (xmlautomata.h); xmlCatalogConvert/xmlCatalogDump (catalog.h); xmlListCopy (list.h); xmlRegNewExecCtxt/xmlRegExecPushString/xmlRegexpPrint (xmlregexp.h); xmlSchematronNewValidCtxt (schematron.h); xmlXPathValuePush/xmlXPathNodeSetDel/xmlXPathNodeSetRemove (xpath.h); xmlMemoryDump (xmlmemory.h); xsltAddTemplate/xsltGetUTF8CharZ/xsltSetCtxtLocaleHandlers/xsltRegisterExtElement/xsltRegisterExtModuleElement/xsltRegisterExtModuleFunction/xsltRegisterExtModuleTopLevel/xsltExtElementLookup/xsltExtModuleElementLookup/xsltExtModuleElementPreComputeLookup/xsltExtModuleFunctionLookup/xsltExtModuleTopLevelLookup/xsltSetLoaderFunc/xsltInitElemPreComp/xsltNewElemPreComp (libxslt); xmlParserError/xmlParserWarning/xmlParserValidityError/xmlParserValidityWarning/xsltTransformError/xmlStrPrintf (variadic shims)
- **Oracle versions:** libxml2 2.15.3, libxslt 1.1.45 (system)
- **Root cause:** The exported surface was built against an older header snapshot without a function-level mirror. The worst instance was xmlGcMemSetup/xmlGcMemGet: the header (correctly mirroring the oracle) declares five arguments including mallocAtomicFunc and an int return, but the Rust exports took four arguments, omitted mallocAtomicFunc and returned nothing — on x86-64 SysV a C caller's RDX (mallocAtomic) was read as realloc and RCX (realloc) as strdup, installing the wrong callbacks. xmlC14NExecute's entire register layout was shifted (7 oracle args vs 6 candidate), xmlAutomataNewCountTrans/xmlAutomataNewOnceTrans swapped min/max with data, and the allocator state itself had two sources of truth (an internal ALLOCATOR RwLock consulted by the *Impl bodies vs the five exported xmlMalloc-family variables). A function-signature court now compares oracle header, candidate header and actual Rust extern "C" signature for every export.
- **Observable residual:** A C consumer calling the pre-fix signatures would misread arguments and receive wrong callbacks/returns; after the fix the three planes match (ABI-FUNCTION-SIGNATURE 3319 compared, 0 findings).
- **Fix:** 11.1-Z.2: (1) allocator merged to the upstream single-source-of-truth model — the five exported function-pointer variables ARE the state; xmlMemSetup/xmlGcMemSetup validate NULLs (return -1), assign the variables (xmlGcMemSetup assigns xmlMallocAtomic = mallocAtomicFunc), xmlMemGet/xmlGcMemGet read them through NULL-tolerant outputs and return 0; the *Impl bodies are indirections through the variables so every internal allocation observes the override; the ALLOCATOR RwLock was removed. (2) Every remaining real signature mismatch fixed to the 2.15.3 oracle (list in surface). (3) Candidate header drift fixed to the oracle (xmlTextWriter* int returns, xmlDebugDumpNode depth arg, xmlParseURIReference/URIUnescapeString returns, xmlNewTextReaderFilename 1-arg, xslt function-pointer typedefs). (4) Variadic shims: the four legacy SAX v1 handlers became x86-64 asm shims formatting the varargs (upstream xmlVFormatLegacyError); xmlStrPrintf/xsltTransformError classified as shims.
- **Phase 11 triangulation:** ABI-FUNCTION-SIGNATURE court (tools/abi/function_signature_court.py): oracle-vs-candidate-vs-Rust prototype mirror, PASS at 3319 compared / 0 findings. ALLOCATOR-HOOK differential court: xmlMemSetup/get, Gc variant, direct variable assignment and NULL rejection are byte-identical with the oracle.
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-08-31 (discovered by the 11.1-Z.2 review (xmlGcMemSetup class) and quantified by the new ABI-FUNCTION-SIGNATURE court (207 findings at first run)); FIXED 2026-08-31 (fixed in 11.1-Z.2; court PASS at 3319 compared / 0 findings; ALLOCATOR-HOOK differential byte-identical with the oracle)

### R-000177: Cross-DSO state partitioning: the whole-archive libxslt/libexslt facades carry private copies of the entire libxml2 core, so hooks/globals installed through one DSO are not observed by the others (FIXED)

- **Status:** FIXED (, Phase 11.1-Z.2)
- **Component:** tools/packaging/facade-gen.sh, tools/packaging/linker-wrapper.sh, courts/suites/data-abi/dso-state-coherence-probe.c, tools/abi/dso_state_coherence_probe.py, build.rs
- **Surface:** three-DSO packaging (tools/packaging/facade-gen.sh, linker-wrapper.sh); every libxml2 hook/global read from within libxslt/libexslt: allocator hooks (xmlMemSetup/direct variable assignment), xmlRegisterNodeDefault/xmlDeregisterNodeDefault, xmlSetExternalEntityLoader, parser globals (xmlKeepBlanksDefault etc.), error handlers
- **Oracle versions:** libxml2 2.15.3, libxslt 1.1.45 (system)
- **Root cause:** Cargo emits one cdylib (the core, also installed as libxml2.so.16 via the symlink chain). The libxslt/libexslt DSOs are whole-archive re-links of the same staticlib with version-script localization (local: *), so every non-xslt/exslt symbol resolves to a private copy inside the facade — including all Rust statics (the allocator variables, hook globals, parser globals, error state). Upstream libxslt.so.1 leaves the libxml2 symbols undefined and resolves them into the single libxml2.so.16 instance, so hooks/globals are shared. The candidate's facades are ELF-correct (SONAME, NEEDED chain, export surface, consumer link+run) but state-partitioned. Thin re-export facades were tested and cannot work: with the symbols undefined the consumer static link fails (modern ld refuses a U entry as a definition), and the core cannot satisfy the facades' references to the crate's mangled internal symbols (xmlMallocImpl etc. are not exported). Fixing this requires splitting the crate so the xslt/exslt code references only exported core symbols — a Phase-13-scale restructuring, out of scope for 11.1-Z.2.
- **Observable residual:** A C consumer that installs an allocator/entity loader/node callbacks or changes a parser global through -lxml2 and then runs an XSLT transform through -lxslt does not see those hooks fire inside the transform (the DSO-STATE-COHERENCE court pins the exact profile: transform-phase allocator/register/deregister/loader observations all 0, keepBlanks not shared — 56-byte result vs the oracle's 41). Transforms themselves work: documents built through -lxml2 are consumed correctly by -lxslt (shared struct layouts); only cross-DSO state is partitioned. This is the deliberately-open Phase 12 architectural target (machine-enforced DSO boundary lint), not a Phase-11 defect.
- **Fix:** Phase 14.30 (R-000177 FIXED): the cross-DSO state boundary is bridged so the whole-archive libxslt/libexslt facades observe consumer registrations exactly like upstream's single shared core. (a) Allocator: xml{Malloc,MallocAtomic,Realloc,Free,MemStrdup}Impl read the process-visible exported allocator slot (dlsym'd __xml* accessor over the core DSO's xmlMemSetup-visible variables) before the local copy. (b) Node hooks: register_node_hook/deregister_node_hook read the bridged per-thread __xml{Register,Deregister}NodeDefaultValue cell instead of short-circuiting on the facade-private xmlRegisterCallbacks copy. (c) External entity loader: xmlLoadExternalEntity prefers the core DSO's xmlSetExternalEntityLoader registration; main-document file/URL opens route through the registered external entity loader first (upstream 2.14+ xmlLoadResource / xmlCtxtNewInputFromUrl layering, verified entry-by-entry against the executed oracle: xmlReadFile/xmlCtxtReadFile/xmlCreateURLParserCtxt/xmlCreateFileParserCtxt/xmlSAXParseFile*/xmlParseFile/xmlParseEntity/xmlSAXParseEntity/xmlParseDTD/xmlParseCtxtExternalEntity fire the loader; the xmlTextReader does NOT) with a silent EntityLoaderFailed arm; the loader's empty result is a valid zero-length input (php://memory 'Document is empty'). (d) Fresh parser contexts snapshot the deprecated per-thread defaults (keepBlanks, replaceEntities) exactly like xmlInitParserCtxt; keepBlanks is only ever lowered (NOBLANKS), never re-raised by option application — the executed-oracle semantics (xmlKeepBlanksDefault(0) governs fresh-context reads). (e) The deprecated xmlXxxDefault/xmlThrDef* setters store unconditionally (0 included) and return the PREVIOUS value (executed-oracle semantics; the old conditional-set-and-return-new pattern made xmlKeepBlanksDefault(0) a no-op); xmlThrDefLineNumbersDefaultValue/xmlLineNumbersDefault are no-ops returning 1. (f) The xslt default document loader passes the transform's parserOptions (XSLT_PARSE_OPTIONS = NOENT|DTDLOAD|DTDATTR|NOCDATA) and routes through the resource loader (documents.c xsltDocDefaultLoaderFunc parity). The DSO-STATE-COHERENCE court now asserts FULL PARITY with the oracle on all ten observations (mode full-parity R-000177 bridged).
- **Phase 11 triangulation:** DSO-STATE-COHERENCE court (courts/suites/data-abi/dso-state-coherence-probe.c): every hook observation is True for the oracle (shared instance) and False for the candidate facades; the documented-partition profile is asserted so any silent architectural change is caught. The three-DSO ELF contract (SONAMEs, NEEDED chains, per-DSO export surfaces, consumer link+run) is verified by the DSO-LOADER court.
- **Evidence:** ['courts/suites/data-abi/dso-state-coherence-probe.c', 'tools/abi/dso_state_coherence_probe.py', 'courts/receipts/phase-11/dso-state-coherence-20260905T004955Z.json', 'courts/receipts/phase-11/dso-state-coherence-20260905T005616Z.json', 'courts/suites/phase14/consumers/php-court-stage.sh']
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-08-31 (demonstrated by the 11.1-Z.2 DSO-STATE-COHERENCE court; thin-facade alternative proven impossible at the ELF level (consumer static link + mangled-internal dependency); accepted as a bounded divergence with the court pinning the exact partition profile. Phase 12 target: eliminate the partitioning with a machine-enforced DSO boundary lint. Phase 14.26 (ZTS php gate): the partition surfaced as a REAL consumer failure under php ZTS — ext/libxml MINIT installs its streams loader through the CORE DSO's exported xmlParserInputBufferCreateFilenameDefault (per-thread cell in the core) while xslt's internal document()/xinclude opens run in the whole-archive libxslt facade's PRIVATE core copy whose same-named TLS cell stays empty; with php-ZTS chdir() virtualized, the facade's raw relative opens resolve against the process start dir and xsl xinclude/document() loads fail (xinclude/xinclude.phpt). Mitigation: a cross-DSO loader-slot bridge — globals::get_{parser,output}_buffer_create_filename_value_cross_dso() — the io open paths consult the process-visible exported __xml{Parser,Output}BufferCreateFilenameValue accessor (the core DSO's cell for the CURRENT thread) via dlsym(RTLD_DEFAULT) whenever the local TLS cell is empty, restoring upstream's single-core-DSO hook visibility across the facade boundary. Same-thread, cross-DSO only: in a single-DSO link the accessor aliases the same cell, so the HOSTILE-THREADS per-thread invariant is unchanged. Gate evidence: ZTS six-extension suite 1290 tests / 1250 passed / 40 skipped / 0 failed (identical to the NTS seal) with the standard oracle-parity exclusion; NTS suite unchanged 0 failed; cargo test --lib 1242 pass; valgrind clean on the xinclude path. The structural partition remains OPEN.); FIXED 2026-09-05 (Phase 14.30: bridged the process-visible state across the three-DSO boundary (allocator slots, node register/deregister hooks, external entity loader incl. main-document resource loads, the fresh-context parser-default seeding incl. keepBlanks, and the deprecated setter semantics). The DSO-STATE-COHERENCE court now PASSES in full-parity mode: all ten observations (loader main parse, p1/p2 allocator+reg+dereg, p2 loader, p2_result_size 41, has_entity 0) match the oracle byte-for-byte. Validation: NTS six-extension php gate 1290/1250/0 failed; ZTS gate 1290/1250/0 failed; cargo test --lib 1254/0; ABI differential courts byte-identical (globals-threading, allocator-default, callback-family, data-globals, save-family); valgrind 3.19 clean on the probe and the php://memory createFromFile/simplexml paths in the phpbuild container.)

## Phase 11.1-Z.3 Residuals

### R-000178: Default allocator invalid-layout UB: default_free/default_realloc used Rust std::alloc with fabricated Layouts (1-byte dealloc layout; requested new size as the old realloc layout) (FIXED)

- **Status:** FIXED (2026-08-31, Phase 11.1-Z.3)
- **Component:** src/abi/allocator.rs, tools/abi/allocator_default_probe.py, courts/suites/data-abi/allocator-default-probe.c
- **Surface:** default allocator hooks (xmlMallocDefault/xmlReallocDefault/xmlFreeDefault/xmlMemStrdupDefault) and the default path of xmlMalloc/xmlMallocAtomic/xmlRealloc/xmlFree/xmlMemStrdup
- **Root cause:** The pre-Z.3 default allocator routed through Rust's global allocator with fabricated Layouts: default_free deallocated every pointer with Layout::from_size_align_unchecked(1, 1) and default_realloc passed the requested NEW size as the OLD allocation layout to std::alloc::realloc. Rust's allocator API requires the deallocation/reallocation layout to correspond to the original allocation; both uses are invalid-layout UB (the source itself commented the free path as 'technically UB'). It does not become valid because the platform allocator underneath probably ignores the size. The default also maintained an accounting registry (MEM_USED/MEM_BLOCKS/BLOCKS), which diverges from upstream 2.15.0: globals.c initializes the exported variables to the plain C runtime functions (xmlMalloc = malloc etc.), so xmlMemUsed/xmlMemBlocks/xmlMemSize are 0 while the default is installed (verified empirically against the system oracle). A second latent defect in the same surface: xmlMallocAtomicLoc zeroed its allocation via xmlMallocZero, while upstream xmlMallocAtomicLoc -> xmlMemMalloc is a plain (non-zeroed) allocation.
- **Observable residual:** None on the executed platform: the ALLOCATOR-DEFAULT-001 differential court proves byte-identical behavior with the system oracle across many sizes, zero-size, grow/shrink realloc, realloc-to-zero, realloc/malloc failure, strdup, free(NULL), direct exported-variable calls, 100k alloc/free churn, xmlMemSize/xmlMemUsed/xmlMemBlocks exactness (0 under the default on both sides) and no-op display entry points. The debug surface (xmlMemMalloc/*Loc) is also byte-identical with the oracle (tracked sizes/counters, verified by the same differential probes and unit tests). The valgrind memory-safety sweep is not executable in this environment (valgrind 3.25.1 SIGILLs in the dynamic loader on every binary, including /bin/true) — recorded as unavailable in the court receipt.
- **Fix:** Replaced the default hooks with libc malloc/realloc/free/strdup wrappers (default_malloc = libc::malloc, default_realloc = libc::realloc, default_free = libc::free, default_strdup = libc::malloc + copy with NULL -> NULL, matching upstream xmlPosixStrdup/xmlCharStrdup). The *Default bodies are now pure libc calls with NO accounting, so xmlMemUsed/xmlMemBlocks/xmlMemSize return 0 under the default exactly like the oracle's plain-malloc default. The registry + counters now back ONLY the debug-named surface (xmlMemMalloc/xmlMemFree/xmlMemRealloc/xmlMemoryStrdup and the *Loc variants), which upstream keeps as a separately-tagged, always-libc-backed, tracked debug allocator; the *Loc variants now delegate to shared debug_malloc/debug_realloc/debug_strdup/debug_free helpers (counter + registry maintenance, matching upstream xmlMemMalloc et al.). xmlMallocAtomicLoc no longer zeroes (upstream xmlMallocAtomicLoc -> xmlMemMalloc is not zeroed). xmlMemDisplay/xmlMemDisplayLast/xmlMemShow/xmlMemoryDump are now no-ops matching upstream 2.15.0 ('This feature was removed.').
- **Phase 11 triangulation:** ALLOCATOR-DEFAULT-001 (tools/abi/allocator_default_probe.py + courts/suites/data-abi/allocator-default-probe.c): identical SHA-256 of the oracle and candidate probe outputs; plus ALLOCATOR-HOOK-001 (hook swap still byte-identical), the allocator unit tests (test_default_libc_semantics, test_debug_surface_tracked, test_mem_stats), and the empirical oracle probes recorded during Z.3.
- **Regression courts:** ALLOCATOR-DEFAULT, ALLOCATOR-HOOK.
- **Evidence:** ['courts/receipts/phase-11/allocator-default-*.json', 'courts/suites/data-abi/allocator-default-probe.c', 'tools/abi/allocator_default_probe.py', 'src/abi/allocator.rs']
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-08-31 (discovered in the 11.1-Z.3 review: default_free deallocates every pointer with a fabricated 1-byte layout and default_realloc passes the requested new size as the old allocation layout — both invalid-layout UB under the Rust allocator contract; the default also diverged from the oracle's untracked plain-malloc default); FIXED 2026-08-31 (closed by 11.1-Z.3: default hooks are now libc malloc/realloc/free/strdup (C allocation semantics, no layout exists), the default no longer maintains the accounting registry (xmlMemUsed/xmlMemBlocks/xmlMemSize match the oracle's 0s), the registry backs only the debug-named surface (matching upstream's tracked debug allocator), the display entry points are upstream-faithful no-ops, and xmlMallocAtomicLoc no longer zeroes; ALLOCATOR-DEFAULT-001 differential court passes byte-identical)

## Phase 12 Residuals

### R-000179: Versioned-distro binary contract gap: the executed oracle (system 2.15.3) is UNVERSIONED, so the candidate core is unversioned; distro binaries that require the upstream LIBXML2_2.x named-version nodes (libxml2-2.13.5 libxml2.syms chain, e.g. Debian-built consumers) are not yet satisfied (OPEN)

- **Status:** OPEN
- **Component:** tools/phase12/export_surface.py, tools/packaging/libxml2.syms, courts/suites/phase12/elf-versioning/court-runner.sh
- **Surface:** libxml2.so.16 ELF version-definition plane (DT_VERDEF / symbol version indices)
- **Root cause:** Upstream 2.15 removed libxml2.syms and exports everything unversioned; the executed distro oracle follows that. Versioning the candidate core with the upstream LIBXML2_2.x chain made every oracle-linked consumer emit ld.so 'no version information available' warnings when the DSO resolves via RUNPATH — an observable substitution difference (measured in the first Phase-12 iteration, reverted).
- **Observable residual:** Executed-oracle parity is exact (both unversioned; ELF-VERSIONING court 14/14). The named-node contract for NON-executed distro binaries (Debian-style consumers requiring LIBXML2_2.x nodes) is a bounded, documented gap: the candidate exports everything the oracle exports unversioned; a distro-built binary's versioned references bind to the unversioned definitions (version-info mismatch warnings possible) until a versioned-profile build is produced.
- **Phase 11 triangulation:** ELF-VERSIONING + BINARY-SUBSTITUTION court (courts/suites/phase12/elf-versioning): oracle-linked consumer -> candidate runtime byte-identical, no ld.so version warnings; libxslt carries the exact 27-node LIBXML2_1.x graph with the oracle's per-symbol nodes (the versioned side of the contract is fully implemented).
- **Regression courts:** ELF-VERSIONING, DOCKER-SUBSTITUTION.
- **Evidence:** ['courts/receipts/phase-12/elf-versioning-*.json', 'courts/receipts/phase-12/docker-substitution-*.json', 'atlas/EXPORT_SURFACE_DISPOSITION.json']
- **Classification:** DOCUMENTED_DIVERGENCE

### R-000180: xmlNewChild dropped a non-NULL content argument: upstream tree.c xmlNewChild -> xmlNewDocNode appends a text child (xmlNewDocText + xmlAddChild); the candidate created only the element (FIXED)

- **Status:** FIXED (2026-09-01, Phase 12)
- **Component:** src/abi/exports_xml2.rs, courts/suites/phase12/consumers/court-runner.sh
- **Surface:** xmlNewChild (tree.c consumer-visible constructor; tree2.c depends on it)
- **Root cause:** The exported xmlNewChild delegated to new_child(parent, ns, name) and ignored the content parameter entirely — the element was created childless, so tree2.c's <node1>content of node 1</node1> serialized as <node1/>.
- **Observable residual:** None on the executed platform: tree2.c (unmodified upstream example) is byte-identical with the oracle (EXTERNAL-CONSUMERS 15/15); unit test test_xml_new_child_with_content.
- **Fix:** xmlNewChild now mirrors upstream exactly: create the element (new_child), then if content is non-NULL append a text child (new_text + add_child) so the document propagates.
- **Phase 11 triangulation:** EXTERNAL-CONSUMERS court tree2 case + src/abi/exports_xml2.rs test_xml_new_child_with_content.
- **Regression courts:** EXTERNAL-CONSUMERS.
- **Evidence:** ['courts/receipts/phase-12/consumers-*.json', 'src/abi/exports_xml2.rs']
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-09-01 (discovered by the Phase-12 EXTERNAL-CONSUMERS court (tree2.c output diff: <node1/> vs <node1>content of node 1</node1>)); FIXED 2026-09-01 (xmlNewChild appends the content text child like upstream xmlNewDocNode; tree2.c byte-identical)

### R-000181: XML_PARSE_DTDVALID no-DTD validity hook missing: validating parses of DTD-less documents did not raise 'Validation failed: no DTD found !' nor clear ctxt->valid (parse2/parse4/reader2 observe ctxt->valid) (FIXED)

- **Status:** FIXED (2026-09-01, Phase 12)
- **Component:** src/xml/parser/state.rs, src/xml/reader/mod.rs, courts/suites/phase12/consumers/court-runner.sh
- **Surface:** parser SAX2 start-element validity check (SAX2.c xmlSAX2StartElementNs)
- **Root cause:** The parser never implemented the SAX2.c first-validity-check: with ctxt->validate set (XML_PARSE_DTDVALID) and no external subset and no populated internal subset, upstream raises XML_DTD_NO_DTD (522) through the DTD domain ('validity error'), clears ctxt->valid, and disables further validation. The candidate ignored the option at parse time (the CLI reimplemented --valid separately). The reader also never mirrored options into ctxt->validate (it set only ctxt->options), so the reader path never validated at all.
- **Observable residual:** None on the executed platform: parse2/parse4/reader2 byte-identical with the oracle including the error text, caret position and 'Failed to validate'/'does not validate' paths; unit test test_reader_dtdvalid_no_dtd.
- **Fix:** sax_start_element now runs the upstream no-DTD check first (validate != 0 && no extSubset && (no intSubset || all four tables NULL)): raise_error_at(XML_FROM_DTD, XML_DTD_NO_DTD, ERROR, 'Validation failed: no DTD found !', end_pos) with ctxt->valid = 0 and ctxt->validate = 0; the reader now mirrors options through apply_options (validate/loadsubset/etc.).
- **Phase 11 triangulation:** EXTERNAL-CONSUMERS court parse2/parse4/reader2 cases (byte-identical stderr incl. the source-window caret) + reader unit test.
- **Regression courts:** EXTERNAL-CONSUMERS.
- **Evidence:** ['courts/receipts/phase-12/consumers-*.json', 'src/xml/parser/state.rs', 'src/xml/reader/mod.rs']
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-09-01 (discovered by the Phase-12 EXTERNAL-CONSUMERS court (parse2/parse4/reader2: no validity error, ctxt->valid stayed 1; reader2 additionally reported 'does not validate' unconditionally because xmlTextReaderIsValid was a constant-0 stub)); FIXED 2026-09-01 (no-DTD validity check in sax_start_element (upstream SAX2.c semantics + error position); reader mirrors options into validate; all three consumers byte-identical)

### R-000182: Push-parser chunk accumulation lost: xmlParseChunk with terminate=0 dropped the accumulated input, so the terminating call parsed only the last chunk ('Document is empty' — parse4) (FIXED)

- **Status:** FIXED (2026-09-01, Phase 12)
- **Component:** src/xml/parser/helpers.rs, src/xml/parser/input.rs, courts/suites/phase12/consumers/court-runner.sh
- **Surface:** xmlParseChunk / xmlCreatePushParserCtxt (push parsing; parse4.c)
- **Root cause:** parse_chunk took the stashed base buffer, pushed the new chunk, built an XmlParser, and for terminate=0 returned 0 WITHOUT re-stashing the combined input — the accumulated stream vanished and the terminating call parsed only its own (often empty) chunk.
- **Observable residual:** None on the executed platform: parse4.c byte-identical with the oracle; unit test test_push_chunk_accumulates.
- **Fix:** parse_chunk now appends each chunk to the base InputBuffer (new InputBuffer::push_bytes — upstream xmlParseChunk grows the parser input) and re-stashes it when terminate=0; the terminating call parses the whole accumulated stream.
- **Phase 11 triangulation:** EXTERNAL-CONSUMERS court parse4 case + src/xml/parser/helpers.rs test_push_chunk_accumulates.
- **Regression courts:** EXTERNAL-CONSUMERS.
- **Evidence:** ['courts/receipts/phase-12/consumers-*.json', 'src/xml/parser/helpers.rs', 'src/xml/parser/input.rs']
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-09-01 (discovered by the Phase-12 EXTERNAL-CONSUMERS court (parse4: 'Document is empty' on a chunked parse)); FIXED 2026-09-01 (chunk accumulation into the stashed base buffer with re-stash on non-terminating calls)

### R-000183: xmlTextReaderPreservePattern was a no-op: pattern-based selective preservation (upstream NODE_IS_PRESERVED streaming prune; reader3.c) was not implemented, so xmlTextReaderCurrentDoc returned the full document (FIXED)

- **Status:** FIXED (2026-09-01, Phase 12)
- **Component:** src/xml/reader/mod.rs, courts/suites/phase12/consumers/court-runner.sh
- **Surface:** xmlTextReaderPreservePattern / xmlTextReaderPreserve / xmlTextReaderCurrentDoc (reader3.c)
- **Root cause:** The reader is a whole-tree implementation (parse once, build traversal events) while upstream prunes the stream as it reads (NODE_IS_PRESERVED / NODE_IS_SPRESERVED bits, preserves counter). PreservePattern returned 0 and preserved nothing.
- **Observable residual:** None on the executed platform: reader3.c byte-identical with the oracle (only preserved nodes survive the reader free; document dumped unformatted); unit test test_preserve_pattern_prunes.
- **Fix:** Implemented pattern preservation as the equivalent post-parse pass: xmlTextReaderPreservePattern compiles the pattern (xmlPatterncompile) into reader->patternTab (freed on Drop); after the parse, apply_pattern_preservation marks matched nodes + their element ancestors and prunes every other node (a node survives iff it or an element ancestor is matched — SPRESERVED subtrees survive whole, DTD nodes survive); prune_unpreserved unlinks + frees the rest. The earlier reader->preserve flag fix (CurrentDoc ownership) completes the reader3.c lifecycle.
- **Phase 11 triangulation:** EXTERNAL-CONSUMERS court reader3 case (byte-identical dump of the pruned doc) + reader unit test.
- **Regression courts:** EXTERNAL-CONSUMERS.
- **Evidence:** ['courts/receipts/phase-12/consumers-*.json', 'src/xml/reader/mod.rs']
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-09-01 (discovered by the Phase-12 EXTERNAL-CONSUMERS court (reader3: the doc retained discarded nodes; also a double-free in the CurrentDoc -> xmlFreeTextReader -> xmlDocDump -> xmlFreeDoc pattern)); FIXED 2026-09-01 (pattern compile + post-parse preservation prune + CurrentDoc ownership transfer (reader->preserve); reader3 byte-identical)

### R-000184: XInclude: (a) XINCLUDE_NS used the 2001 draft URI instead of the 2003 namespace, so no xi:include element was ever recognized; (b) XInclude hrefs never routed through registered input callbacks (xmlRegisterInputCallbacks), so custom I/O schemes like io1.c's sql: failed (FIXED)

- **Status:** FIXED (2026-09-01, Phase 12)
- **Component:** src/xml/xinclude/mod.rs, src/abi/exports_parser.rs, src/xml/debug/mod.rs, courts/suites/phase12/consumers/court-runner.sh
- **Surface:** XInclude namespace identity + input-callback dispatch (io1.c custom I/O + XInclude)
- **Root cause:** (a) XINCLUDE_NS was b"http://www.w3.org/2001/XInclude" — the upstream XINCLUDE_OLD_NS draft — so is_xinclude_element never matched the 2003 namespace the parser materializes. (b) io_read_file/parse_xml_document opened hrefs with libc::open only; upstream xmlXIncludeLoadDoc -> xmlNewInputFromFile -> xmlParserInputBufferCreateFilename consults the registered input callback table, so sql: URIs were never dispatched.
- **Observable residual:** None on the executed platform: io1.c byte-identical with the oracle (custom sql: scheme + XInclude + xmlDocDump); the C-level xmlParserInputBufferCreateFilename export remains a documented shallow allocation for the general file path (the parser's own file path uses the internal input machinery).
- **Fix:** XINCLUDE_NS is now the 2003 namespace with the 2001 draft accepted as a legacy alias (is_xinclude_ns_uri, mirroring upstream XINCLUDE_NS/XINCLUDE_OLD_NS); the XInclude loader routes hrefs through a new read_uri_via_input_callbacks helper (match -> open -> read loop -> close, upstream xmlParserInputBufferCreateFilename semantics) before falling back to the file path; the debug dump namespace check updated to 2003.
- **Phase 11 triangulation:** EXTERNAL-CONSUMERS court io1 case + the xinclude module tests.
- **Regression courts:** EXTERNAL-CONSUMERS.
- **Evidence:** ['courts/receipts/phase-12/consumers-*.json', 'src/xml/xinclude/mod.rs', 'src/abi/exports_parser.rs']
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-09-01 (discovered by the Phase-12 EXTERNAL-CONSUMERS court (io1: 'XInclude processing failed'; the include node carried the 2003 namespace the 2001 constant never matched)); FIXED 2026-09-01 (2003 namespace (2001 as legacy alias) + registered input callback dispatch in the XInclude loader; io1 byte-identical)

### R-000185: tree.c save-family wrappers (xmlSaveFile/xmlSaveFileEnc/xmlSaveFormatFile/xmlSaveFormatFileEnc) ignored their encoding and format arguments and dumped unformatted without the encoding declaration (FIXED)

- **Status:** FIXED (2026-09-01, Phase 12)
- **Component:** src/xml/tree/mod.rs, src/xml/save.rs, src/xml/io/mod.rs, include/libxml/tree.h, courts/suites/phase12/consumers/court-runner.sh
- **Surface:** xmlSaveFile family (tree.c -> xmlsave.c xmlSaveFormatFileEnc -> xmlDocDumpInternal; tree2.c uses xmlSaveFormatFileEnc("-", doc, "UTF-8", 1))
- **Root cause:** The four wrappers delegated to save_doc_to_filename(..., 0) with encoding/format discarded: no XML_SAVE_FORMAT, no encoding in the declaration. Upstream tree.c/xmlsave.c route through xmlSaveFormatFileEnc -> xmlSaveToFilename(filename, encoding, format ? XML_SAVE_FORMAT : 0) + xmlSaveDoc + xmlSaveClose. The save-context machinery also never carried the encoding name into the declaration (only doc->encoding), and the io layer did not map filename "-" to stdout (upstream xmlOutputDefaultOpen dup(STDOUT_FILENO)).
- **Observable residual:** None on the executed platform: tree2.c byte-identical with the oracle (decl encoding="UTF-8", formatted output, stdout target); unit test test_save_format_file_to_encoding_decl.
- **Fix:** The wrappers now delegate through crate::xml::save exactly like upstream (xmlSaveToFilename with the resolved encoder + options, xmlSaveDoc, xmlSaveClose); the save context stores the encoding name (xmlStrdup, freed by close/finish) and serialize_node_opts_enc/DumpState carry it into the XML declaration (falling back to doc->encoding like upstream's `if (encoding == NULL) encoding = cur->encoding`); io::output_buffer_create_filename maps "-" to dup(STDOUT_FILENO) like upstream xmlOutputDefaultOpen; the tree.h declarations for the legacy save family were added so tree.h-only consumers compile.
- **Phase 11 triangulation:** EXTERNAL-CONSUMERS court tree2 case + save.rs unit test + SAVE-* differential courts (unchanged, byte-identical).
- **Regression courts:** EXTERNAL-CONSUMERS.
- **Evidence:** ['courts/receipts/phase-12/consumers-*.json', 'src/xml/save.rs', 'src/xml/tree/mod.rs', 'src/xml/io/mod.rs']
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-09-01 (discovered by the Phase-12 EXTERNAL-CONSUMERS court (tree2: missing encoding="UTF-8" declaration and no formatting)); FIXED 2026-09-01 (upstream-faithful delegation through the save machinery + encoding-carrying declaration + "-" stdout mapping; tree2 byte-identical)

### R-000186: Facade DSOs required GLIBC_2.39 (weak pidfd_spawnp/pidfd_getpid refs from Rust std survived the plain cc re-link), making the facades unloadable on older glibc (Debian bookworm 2.36) — the core itself required only GLIBC_2.34 (FIXED)

- **Status:** FIXED (2026-09-01, Phase 12)
- **Component:** tools/packaging/facade-gen.sh, courts/suites/phase12/docker-substitution/court-runner.sh
- **Surface:** libxslt.so.1 / libexslt.so.0 facade ELF version requirements (Docker substitution portability)
- **Root cause:** The core cdylib is linked by rustc + lld with --gc-sections, which drops the unreferenced weak pidfd_* entries Rust std emits on new glibc; the facade re-link (facade-gen.sh, plain cc -shared) did not GC, so the weak refs bound to the host glibc's GLIBC_2.39 versions and the facades carried GLIBC_2.39 requirements.
- **Observable residual:** None on the executed platform: the DOCKER-SUBSTITUTION court loads the facades inside Debian bookworm; export surface unchanged (dynsym-surface 12/12, elf-versioning 14/14).
- **Fix:** facade-gen.sh passes -Wl,--gc-sections (the exported symbols are kept as roots by the version scripts); the facades now require only GLIBC_2.34 like the core and load on Debian bookworm (glibc 2.36).
- **Phase 11 triangulation:** DOCKER-SUBSTITUTION court (in-VM substitution) + objdump -T GLIBC_2.34 ceiling + DYNSYM-SURFACE.
- **Regression courts:** DOCKER-SUBSTITUTION, DYNSYM-SURFACE, ELF-VERSIONING.
- **Evidence:** ['courts/receipts/phase-12/docker-substitution-*.json', 'tools/packaging/facade-gen.sh']
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-09-01 (discovered by the first DOCKER-SUBSTITUTION run: 'version GLIBC_2.39 not found (required by /candidate/libxslt.so.1)' on Debian bookworm); FIXED 2026-09-01 (--gc-sections on the facade re-links; GLIBC ceiling now 2.34 on all three DSOs)

### R-000187: The LIBXML_THREAD_ALLOC_ENABLED accessor functions (__xmlMalloc/__xmlMallocAtomic/__xmlRealloc/__xmlFree/__xmlMemStrdup, upstream globals.c) were missing, so source-built consumers (upstream headers with thread-alloc enabled; the canonical source oracle's libxslt) could not resolve their xmlFree/xmlMalloc references (FIXED)

- **Status:** FIXED (2026-09-01, Phase 12)
- **Component:** src/abi/allocator.rs, tools/phase12/export_surface.py, tools/phase12/dlsym_surface_court.py, courts/suites/phase12/docker-substitution/court-runner.sh
- **Surface:** allocator accessor plane (source-profile consumers; DOCKER-SUBSTITUTION)
- **Root cause:** The executed distro oracle hides the five thread-alloc accessors (no --with-thread-alloc / hidden visibility), so the candidate surface — generated from the executed oracle — omitted them; a source-built upstream libxml2 (--with-thread-alloc, the Dockerfile.oracle profile) exports them and its libxslt references __xmlFree/__xmlMalloc/__xmlRealloc, so substitution failed with 'undefined symbol: __xmlFree'.
- **Observable residual:** None on the executed platform: DOCKER-SUBSTITUTION 17/17 with the source-profile oracle; the executed distro oracle surface is a strict subset (1718 = 1713 + 5 documented CUSTODIAN_EXTENSION accessors).
- **Fix:** Implemented the five accessors per upstream semantics (each returns addr_of_mut! of the corresponding exported variable — R-000176 single source of truth, so (*__xmlMalloc()) is the candidate's xmlMalloc variable) and registered them as CUSTODIAN_EXTENSION exports in the disposition ledger + generated libxml2.syms. Fixed a latent fail-open in export_surface.py: the candidate surface is now the DSO dynsym UNION the staticlib #[no_mangle] surface, so a new export hidden by the version script can never be silently omitted (the accessors were invisible to nm_dyn until the map included them).
- **Phase 11 triangulation:** DOCKER-SUBSTITUTION court (source-profile substitution) + EXPORT-SURFACE-DISPOSITION ledger + DYNSYM-SURFACE (all 1718 shipped resolve; 584 INTERNAL_LEAK hidden) + SHIPPED-SURFACE --check.
- **Regression courts:** DOCKER-SUBSTITUTION, DYNSYM-SURFACE, ELF-VERSIONING.
- **Evidence:** ['courts/receipts/phase-12/docker-substitution-*.json', 'atlas/EXPORT_SURFACE_DISPOSITION.json', 'src/abi/allocator.rs', 'tools/phase12/export_surface.py']
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-09-01 (discovered by the DOCKER-SUBSTITUTION court with the canonical source-built oracle: 'undefined symbol: __xmlFree' (source-profile consumers use the thread-alloc accessor plane)); FIXED 2026-09-01 (five accessors implemented (CUSTODIAN_EXTENSION, single-source-of-truth semantics) + export_surface.py staticlib-union fail-closed discovery; source-profile substitution byte-identical)

### R-000188: DYNSYM-SURFACE hidden-check false positives: a handle-scoped dlsym also resolves symbols the DSO merely inherits from its DT_NEEDED chain (libgcc_s compiler-rt builtins like __absvdi2, libc helpers), so INTERNAL_LEAK verification must use the DSO's own dynsym (FIXED)

- **Status:** FIXED (2026-09-01, Phase 12)
- **Component:** tools/phase12/dlsym_surface_court.py
- **Surface:** DYNSYM-SURFACE court negative test (evidence tooling)
- **Root cause:** ctypes getattr/handle-dlsym searches the DSO AND its dependency tree; after the staticlib-union enlarged the INTERNAL_LEAK set, 149 inherited symbols (libgcc_s builtins) appeared 'visible' although they are not defined by the candidate DSO (readelf --dyn-syms shows none of them).
- **Observable residual:** None: the court now measures exactly 'defined by the DSO' (12/12).
- **Fix:** The negative check now reads the DSO's own nm -D --defined-only dynsym: an INTERNAL_LEAK is hidden iff it is not a defined dynamic export of that DSO (documented in the court docstring).
- **Phase 11 triangulation:** DYNSYM-SURFACE court 12/12 + readelf --dyn-syms spot check.
- **Regression courts:** DYNSYM-SURFACE.
- **Evidence:** ['tools/phase12/dlsym_surface_court.py']
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-09-01 (discovered while verifying the __xml* accessor surface: the negative test flagged 149 inherited libgcc_s/libc symbols as visible leaks); FIXED 2026-09-01 (negative check switched to the DSO's own dynsym)

### R-000189: docker/Dockerfile.oracle could not build: configure failed with 'ICU not found' because libicu-dev was not installed although oracle/build.sh passes --with-icu (FIXED)

- **Status:** FIXED (2026-09-01, Phase 12)
- **Component:** docker/Dockerfile.oracle
- **Surface:** Docker oracle image build (canonical source-built oracle for the DOCKER-SUBSTITUTION court)
- **Root cause:** The Dockerfile installed the build toolchain without libicu-dev while the build script enables --with-icu (the executed host oracle is Iconv+ICU-enabled — R-000157's oracle profile).
- **Observable residual:** None: the canonical oracle image builds and the DOCKER-SUBSTITUTION court runs entirely inside it.
- **Fix:** Added libicu-dev to the apt install list; the oracle image now builds libxml2 2.15.3 + libxslt 1.1.45 with the ICU profile.
- **Phase 11 triangulation:** DOCKER-SUBSTITUTION court image build + in-VM oracle version receipt.
- **Regression courts:** DOCKER-SUBSTITUTION.
- **Evidence:** ['docker/Dockerfile.oracle']
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-09-01 (first docker build of the oracle image failed at ./configure: ICU not found); FIXED 2026-09-01 (libicu-dev installed in the Dockerfile)

## Phase 13 Residuals

### R-000190: HOSTILE-THREADS: the error-handler slots (xmlGenericError/Context, xmlStructuredError/Context) and the other 14 TLS-era globals were GLOBAL data instead of thread-local (upstream 2.15 LIBXML_THREAD_ENABLED xmlGetThreadLocalStorage model) (FIXED)

- **Status:** FIXED (2026-09-01, Phase 13)
- **Component:** src/xml/globals/tls.rs, src/xml/globals/mod.rs, src/abi/data_globals.rs, include/libxml/globals.h, include/libxml/parser.h, include/libxml/xmlerror.h, include/libxml/xmlIO.h, include/libxml/tree.h
- **Surface:** Error-handler/parser-default/node-hook/IO-hook globals (globals.c 2.15, xmlerror.h/parser.h/xmlIO.h/tree.h)
- **Root cause:** The candidate exported the 18 TLS-era globals as plain data symbols and the __xml* accessors returned addr_of_mut! of them; upstream 2.15 with LIBXML_THREAD_ENABLED keeps them in per-thread storage (globals.c xmlGetThreadLocalStorage) and the oracle DSO exports ONLY the __xml* accessor FUNCTIONS. A handler installed in one thread was therefore observable from every other thread (HOSTILE-THREADS T2: after the workers installed a no-op structured handler, the main thread's own diagnostics were swallowed on the candidate while the oracle still printed them).
- **Observable residual:** None: xmlSetStructuredErrorFunc/xmlSetGenericErrorFunc are thread-scoped exactly like the oracle; the xmlLastError global mirror (R-000135) is unchanged.
- **Fix:** Moved all 18 TLS-era globals into thread_local! cells (src/xml/globals/tls.rs), single source of truth per thread; the __xml* accessors now return pointers into the current thread's cells; removed the 18 plain data-symbol exports and switched the candidate headers to the upstream macro/accessor contract (#define xmlXxx (*__xmlXxx())); regenerated the export-surface disposition (libxml2 CUSTODIAN_EXTENSION 45->31).
- **Phase 11 triangulation:** HOSTILE-THREADS probe (T1 concurrent parses, T2 error isolation, T3 concurrent global reads) byte-identical vs the system oracle; GLOBALS-THREADING and DATA-GLOBALS-001 courts still byte-identical; ABI-FUNCTION-SIGNATURE 3277/3277.
- **Regression courts:** HOSTILE-THREADS, GLOBALS-THREADING-001, DATA-GLOBALS-001, ABI-FUNCTION-SIGNATURE, HEADER-COMPILE, EXPORT-SURFACE-DISPOSITION.
- **Evidence:** ['courts/receipts/phase-13/hostile-threads-*.json', 'src/xml/globals/tls.rs']
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-09-01 (HOSTILE-THREADS probe failed: candidate stderr empty where oracle printed the main-thread T2 error — workers' TLS-scoped structured handler was globally visible on the candidate); FIXED 2026-09-01 (18 TLS-era globals moved to thread-local cells; data symbols removed; headers use the upstream accessor/macro contract)

### R-000191: HOSTILE-ABI: buffer/limits contracts diverged under extreme sizes and NULLs (xmlReadMemory INT_MAX wild-read, xmlBufferCreate/CreateSize negative/overflow, xmlParseChunk NULL/negative, buf_add/buf_add_head edge contract) (FIXED)

- **Status:** FIXED (2026-09-01, Phase 13)
- **Component:** src/abi/exports_xml2.rs, src/abi/exports_parser.rs, src/abi/exports_buffer.rs
- **Surface:** Parser entry points and buffer primitives under hostile arguments
- **Root cause:** xmlReadMemory accepted size >= INT_MAX and streamed ~2 GiB from the caller's buffer (upstream's own unsized wild read); xmlBufferCreate/CreateSize mishandled negative/overflow sizes; xmlParseChunk did not reject NULL/negative chunks with XML_ERR_ARGUMENT; buf_add/buf_add_head did not reproduce the upstream edge contract (NULL -> -1, negative len -> strlen, 0 -> 0, 0x80000000 cap).
- **Observable residual:** None on the executed platform: 72 NULL/extreme-size attacks byte-identical vs the oracle.
- **Fix:** xmlReadMemory now rejects size == INT_MAX up front (observable result matches the oracle's deterministic probe outcome); xmlBufferCreate/CreateSize follow the upstream contract (xml_buffer_create_upstream); xmlParseChunk NULL/negative -> XML_ERR_ARGUMENT; buf_add/buf_add_head implement the upstream edge contract exactly.
- **Phase 11 triangulation:** HOSTILE-ABI probe (72 attacks) byte-identical; security-limits probe unchanged.
- **Regression courts:** HOSTILE-ABI.
- **Evidence:** ['courts/receipts/phase-13/hostile-abi-*.json']
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-09-01 (HOSTILE-ABI probe exposed the buffer/limits divergences); FIXED 2026-09-01 (upstream-faithful buffer contracts + INT_MAX rejection)

### R-000192: HOSTILE-OWNERSHIP: NULL-handling divergences (xmlNewNode/xmlNewPI accepted NULL names, create_int_subset NULL-doc, utf8_strlen(NULL)) (FIXED)

- **Status:** FIXED (2026-09-01, Phase 13)
- **Component:** src/xml/tree/mod.rs, src/xml/dtd/mod.rs, src/xml/string.rs
- **Surface:** Tree construction and string helpers under NULL attacks
- **Root cause:** xmlNewNode/xmlNewPI did not reject NULL names like upstream; create_int_subset with a NULL document created an attached DTD instead of an unattached one; utf8_strlen(NULL) did not return -1.
- **Observable residual:** None: HOSTILE-OWNERSHIP O1-O12 byte-identical.
- **Fix:** NULL names rejected with upstream messages; create_int_subset(NULL, ...) allocates an unattached DTD exactly like upstream; utf8_strlen(NULL) -> -1.
- **Phase 11 triangulation:** HOSTILE-OWNERSHIP probe (O1-O12) byte-identical.
- **Regression courts:** HOSTILE-OWNERSHIP.
- **Evidence:** ['courts/receipts/phase-13/hostile-ownership-*.json']
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-09-01 (HOSTILE-OWNERSHIP probe exposed the NULL-handling divergences); FIXED 2026-09-01 (upstream NULL contracts implemented)

### R-000193: HOSTILE-ALLOCATOR: xmlStrcat/xmlStrncat leaked the old buffer on realloc failure; xml_buf_add did not reproduce the upstream failure contract (FIXED)

- **Status:** FIXED (2026-09-01, Phase 13)
- **Component:** src/xml/string.rs, src/abi/exports_buffer.rs
- **Surface:** String/buffer append paths under allocator-failure injection
- **Root cause:** xmlStrcat/xmlStrncat did not free the previous buffer when the reallocation failed (upstream xmlstring.c frees it and returns NULL), leaking under failure injection; xml_buf_add returned the wrong result on failure.
- **Observable residual:** None: HOSTILE-ALLOCATOR H1-H6 byte-identical under size-based failure injection.
- **Fix:** xmlStrcat/xmlStrncat free `cur` on realloc failure and return NULL; xml_buf_add implements the upstream contract (0 on success, -1 on failure, no partial write).
- **Phase 11 triangulation:** HOSTILE-ALLOCATOR probe (H1-H6) byte-identical; allocator-hook and allocator-default courts unchanged.
- **Regression courts:** HOSTILE-ALLOCATOR, ALLOCATOR-HOOK.
- **Evidence:** ['courts/receipts/phase-13/hostile-allocator-*.json']
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-09-01 (HOSTILE-ALLOCATOR probe exposed the realloc-failure leak); FIXED 2026-09-01 (upstream xmlstring.c failure contract implemented)

### R-000194: HOSTILE-CALLBACKS: xmlSAXUserParseMemory/File freed the caller's SAX handler (stack ownership), returned -1 instead of errNo, lost the error-context parent input, and did not propagate I/O source failures (FIXED)

- **Status:** FIXED (2026-09-01, Phase 13)
- **Component:** src/abi/exports_parser.rs, src/xml/parser/state.rs, src/xml/parser/input.rs, src/xml/errors/mod.rs
- **Surface:** SAX callback lifecycle and error-context plumbing under hostile callbacks
- **Root cause:** xmlSAXUserParseMemory/xmlSAXUserParseFile copied the caller's SAX into the parser context and then freed it at teardown (freeing a stack object or the caller's storage), returned -1 instead of the raised error number, resolved the error context against the wrong input when the user SAX raised during a nested input, and swallowed read-callback failures (reporting an empty document instead of the I/O error).
- **Observable residual:** None: HOSTILE-CALLBACKS C1-C10 byte-identical.
- **Fix:** The user-SAX wrappers deep-copy the handler into library-owned storage and free only that copy; they return errNo; the error context falls back to the parent input; I/O source failures raise XML_IO_UNKNOWN and the 'Document is empty' path is only taken when the source genuinely produced no bytes.
- **Phase 11 triangulation:** HOSTILE-CALLBACKS probe (C1-C10) byte-identical.
- **Regression courts:** HOSTILE-CALLBACKS, CALLBACK-001.
- **Evidence:** ['courts/receipts/phase-13/hostile-callbacks-*.json']
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-09-01 (HOSTILE-CALLBACKS probe exposed the SAX ownership/errNo/error-context/I/O-failure divergences); FIXED 2026-09-01 (user-SAX copy ownership, errNo return, parent-input error context, I/O failure propagation implemented)

### R-000195: HOSTILE-FAILURE: diagnostic-surface divergences (regexp handle typedefs missing, depth-limit error not streamed, entity-loop legacy 'cur input' tail, XPath compile diagnostics, xmlParseDTD I/O warning, xmlRegexpCompile invalid patterns) (FIXED)

- **Status:** FIXED (2026-09-01, Phase 13)
- **Component:** include/libxml/xmlregexp.h, src/xml/parser/tokenizer.rs, src/xml/errors/mod.rs, src/xml/xpath/, src/xml/regex/mod.rs, src/abi/exports_xml2.rs
- **Surface:** Error/diagnostic output for hostile documents and malformed inputs
- **Root cause:** The drop-in headers did not declare the xmlRegexpPtr/xmlRegExecCtxtPtr handles (F8); the depth-limit error was raised without the source window (F1); the entity-loop error lacked upstream's legacy 'cur input' tail line (F2); xmlXPathCompile produced no diagnostics with byte offsets (F3); xmlParseDTD did not emit the file-load I/O warning (F7); xmlRegexpCompile returned a compiled object for invalid patterns instead of NULL (F8).
- **Observable residual:** None: HOSTILE-FAILURE F1-F10 byte-identical, including the legacy tail line.
- **Fix:** Declared the regexp handle typedefs verbatim from the oracle header; factorized window_at_data and streamed the depth error with a window; plumbed the legacy tail through raise_error_streamed/format_error_streamed; added compile_result + byte offsets to the XPath lexer diagnostics; made xmlParseDTD emit the I/O warning; xmlRegexpCompile returns NULL when the NFA cannot be built.
- **Phase 11 triangulation:** HOSTILE-FAILURE probe (F1-F10) byte-identical; HEADER-COMPILE 596/596.
- **Regression courts:** HOSTILE-FAILURE, HEADER-COMPILE.
- **Evidence:** ['courts/receipts/phase-13/hostile-failure-*.json']
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-09-01 (HOSTILE-FAILURE probe exposed the diagnostic-surface divergences); FIXED 2026-09-01 (regexp typedefs, streamed windows, legacy tail, XPath diagnostics, DTD I/O warning, invalid-regexp NULL implemented)

### R-000196: format-number(-inf) emitted heap garbage: xml_strdup_joined was called on a NON-NUL-terminated '-Infinity' buffer (read-past-end until a NUL), exposed by the Phase-13 TLS data-segment layout shift (CLI-XSLTPROC-0017) (FIXED)

- **Status:** FIXED (2026-09-01, Phase 13)
- **Component:** src/xslt/numbering/mod.rs
- **Surface:** xsltFormatNumberConversion negative-infinity path (numbering/mod.rs)
- **Root cause:** The -Infinity branch assembled `joined = minusSign + infinity` as a Vec WITHOUT a NUL terminator and passed it to xml_strdup_joined, which measures the input with strlen (xml_strdup) — so the copy read past the Vec's end until an arbitrary NUL, emitting heap garbage (6 bytes EF BF BD 74 71 7F in the CLI-XSLTPROC-0017 heap layout). The bug predates Phase 13 (present at the Phase-12 seal) and was hidden by the previous heap layout; the Phase-13 TLS conversion (18 data symbols removed) shifted the layout and made it observable.
- **Observable residual:** None: CLI-XSLTPROC-0017 and the full xsltproc court (21/21) are byte-identical again.
- **Fix:** NUL-terminate the joined buffer (joined.push(0)) before xml_strdup_joined; the positive-infinity/NaN branches already point at NUL-terminated statics.
- **Phase 11 triangulation:** xsltproc CLI court 21/21; minimal repro (fmtmin3.xsl) byte-identical; the failing heap layout was reproduced before the fix and eliminated after it.
- **Regression courts:** CLI-XSLTPROC.
- **Evidence:** ['courts/receipts/phase-09/xsltproc-*.json']
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-09-01 (CLI-XSLTPROC-0017 failed after the Phase-13 TLS data-segment shift: '-Infinity' followed by 6 garbage bytes); FIXED 2026-09-01 (joined buffer NUL-terminated before xml_strdup_joined)

## Phase 14 Residuals

### R-000198: xmlTextWriterStartDocument does not install the output encoder on the writer's output buffer: non-UTF-8 writer output (xmlTextWriterWriteComment/String/... after startDocument(encoding:...)) is emitted unconverted as UTF-8 (FIXED)

- **Status:** FIXED (2026-09-05, Phase 14)
- **Component:** src/xml/writer/mod.rs, src/xml/io/mod.rs
- **Surface:** xmlTextWriterStartDocument, xmlTextWriterSetOutputEncoding, xmlOutputBufferFlush encoder path (xmlwriter.c / xmlIO.c)
- **Oracle versions:** libxml2 2.15.3 (system, Iconv+ICU)
- **Root cause:** Upstream xmlTextWriterStartDocument installs the found char-encoding handler on writer->out (out->encoder + a 4000-byte conv buffer) and flushes through it, so comment/text/attribute content written after startDocument(encoding:"SHIFT_JIS") is transcoded to the target encoding on the output-buffer flush. The candidate only sets an encoder_active byte-count flag and never installs out->encoder/conv, so the flush machinery (io::output_buffer_flush converts only when ob.encoder is set) passes UTF-8 through unchanged: XMLWriter::toStream() + startDocument(encoding:"SHIFT_JIS") emits raw UTF-8 comment bytes where the oracle emits Shift_JIS (0x82 0x9F ...) bytes. The same install is missing from the writer's SetOutputEncoding path. Sibling slice of R-000157: even for codecs the registry serves (UTF-16, ASCII, ISO-8859-1, windows-1252) the writer never activates them.
- **Observable residual:** XMLWriter output declared with a non-UTF-8 encoding is byte-different from the oracle whenever the content contains non-ASCII characters.
- **Fix:** xmlTextWriterStartDocument (src/xml/writer/mod.rs) now resolves the declared encoding with xmlFindCharEncodingHandler and installs it on the output buffer (out->encoder = registry handler; out->conv = io::buf_create(4000)) before writing the XML declaration, so every later write is transcoded at output-buffer flush; an unservable encoding returns -1 without writing (upstream unsupported-encoding semantics). Supported by new native Shift_JIS/EUC-JP converters (src/xml/encoding/mod.rs, encoding_rs-backed enc_rs_input/enc_rs_output with the -2 input-error convention for unmappable characters -> char_enc_out decimal charrefs).
- **Phase 11 triangulation:** The 2.15.3 oracle (Iconv+ICU) emits the transcoded bytes; the php .exp for the only in-suite probe (xmlwriter_toStream_encoding_shiftjis) is unsatisfiable by any correct libxml2, so the gate cannot observe this — byte-parity probes vs the oracle are the court.
- **Regression courts:** WRITER, ENCODING-001.
- **Evidence:** ['courts/receipts/phase-14/php-14-27-writer-encoder-sjis-20260905/sjis-euc-byte-parity-probe.php']
- **Classification:** CANDIDATE_BUG
- **History:** OPEN 2026-09-04 (Discovered in Phase 14.25 while disposing of the shiftjis .exp case: the candidate writer emitted raw UTF-8 for the Shift_JIS comment while the oracle emitted 0x82 0x9F per U+3041 — a genuine candidate-vs-oracle output divergence hidden by the unsatisfiable .exp. Phase 14.26 (ZTS gate) re-confirmed: the ZTS oracle passes the doctored phpt with real SJIS bytes; the candidate still emits UTF-8.); FIXED 2026-09-05 (Phase 14.27 closure: xmlTextWriterStartDocument now mirrors upstream xmlwriter.c — the declared encoding's handler is resolved via xmlFindCharEncodingHandler and INSTALLED on the output buffer (out->encoder + a 4000-byte conv buffer) before the declaration is written, so comment/text/attribute content is transcoded at output-buffer flush (io::output_buffer_flush converts when ob.encoder is set). An encoding the registry cannot serve returns -1 and writes nothing (upstream unsupported-encoding behavior; php XMLWriter::startDocument -> FALSE). Byte-parity probe (courts/receipts/phase-14/php-14-27-writer-encoder-sjis-20260905/sjis-euc-byte-parity-probe.php) is byte-identical to the oracle for SHIFT_JIS and EUC-JP comment/attr/text output and for the unmappable-character decimal-charref path; the writer encoder path is clean under valgrind. The php .phpt exclusion stays (its .exp demands an empty comment, unsatisfiable by any correct libxml2).)

### R-000199: Recursive-descent parse stack envelope: deeply nested documents (~4k+ levels on an 8MB stack at the -O0 dev profile) overflow where upstream xmlParseChunk's iterative xmlParseTryOrFinish state machine is unbounded (FIXED)

- **Status:** FIXED (2026-09-05, Phase 14)
- **Component:** src/xml/parser/state.rs
- **Surface:** xmlParseChunk / xmlParseDocument nested-element descent (parser.c xmlParseContentInternal vs state.rs parse_element recursion)
- **Oracle versions:** libxml2 2.15.3 (system)
- **Root cause:** The candidate parses by recursive descent (parse_element wrapper -> parse_element_content loop -> parse_element for each nested child). Phase 14.25 split the ~8KB monolithic parse_element frame into a slim wrapper + content loop (~3.3KB per level total at the -O0 dev profile), fixing the php-suite depth-1000 crash (bug65236) with 3x margin, but the envelope still caps at ~3-4k nesting levels on an 8MB stack. Upstream xmlParseChunk drives the identical work through the ITERATIVE xmlParseTryOrFinish + ctxt->instate state machine (no per-element recursion): the oracle parses depth-20000 documents on a 1MB stack without issue.
- **Observable residual:** A consumer parsing an XML document nested deeper than ~3-4k elements segfaults (stack overflow) on the candidate where the oracle succeeds.
- **Fix:** src/xml/parser/state.rs: parse_element is now an ITERATIVE element driver (R-000199 closure, Phase 14.28): the recursive parse_element -> parse_element_content -> parse_element descent was converted to a flat token loop over an explicit heap stack of open-element frames (each a name/open_line/ns_scope_mark OpenElement). A nested start tag pushes the current frame and switches to the child; an end tag closes the current element (SAX end event + ns-scope truncate + name pop) and resumes the parent's loop. Every branch is a verbatim continuation of the old content loop, so SAX/name/namespace push-pop order and each error path's pop behavior are unchanged. No C-stack growth with nesting depth: ext/xml xml_parse now parses depth-100000 crash-free at the -O0 dev profile where depth-4000 SEGFAULTed before; the oracle (iterative xmlParseTryOrFinish) handles 20000.
- **Phase 11 triangulation:** Measured in Phase 14.25: candidate depth 3000 passes 8/8, depth 4000 crashes 8/8 (8MB stack); oracle depth 20000 passes on a 1MB stack. Closing requires converting the element-content descent to an explicit/iterative driver (upstream xmlParseTryOrFinish model) - a future implementation work item, not a suite blocker (php max ~1000).
- **Regression courts:** PARSER.
- **Evidence:** ['courts/receipts/phase-14/php-14-25-stack-parity-green-20260904/README.md']
- **Classification:** UNRESOLVED
- **History:** OPEN 2026-09-04 (Recorded at the 14.25 closure after the parse_element split: the stack envelope is measured, bounded and documented; closure is an explicit iterative-parser conversion work item.); FIXED 2026-09-05 (Phase 14.28 closure evidence: (a) ext/xml xml_parse depth sweep — candidate depth 4000 SEGFAULTed before, now 4000/20000/100000 all parse (rc=0); (b) deep-doc parity probe (courts/receipts/phase-14/php-14-28-iterative-parser-20260905/deep-doc-parity-probe.php) is IDENTICAL to the oracle for depth-5000 SAX event sequence (events=10002, max=5001) and for DOM tree depth-2000 serialize (14027 bytes) and the DOM tree-depth cap at 2048/2049 (ok=false on both — the upstream nodePush cap at 256/2048 [HUGE] is mirrored in sax/default.rs and unchanged); (c) full six-extension php gates NTS + ZTS 0 failed each; (d) cargo test --lib 1247 pass; (e) valgrind 0 errors on a depth-20000 parse.)

## Classification Legend

- `CANDIDATE_BUG` — see classification policy in §45/§71
- `DOCUMENTED_DIVERGENCE` — see classification policy in §45/§71
- `INTENTIONAL_SAFE_DIVERGENCE` — see classification policy in §45/§71
- `ORACLE_BUG` — see classification policy in §45/§71
- `UNRESOLVED` — see classification policy in §45/§71
- `VERSION_DIFFERENCE` — see classification policy in §45/§71
