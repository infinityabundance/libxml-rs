# Residual Ledger

Per §71: every unexplained difference gets an ID (`R-000001`...), and its
history is retained after fixing. This Markdown is generated from
`RESIDUAL_LEDGER.json` by `tools/evidence/ledger_gen.py` (§70 policy:
Markdown generated from JSON; the JSON is the only hand-maintained truth).

## Current Residuals

**12 open residuals:** R-000119, R-000120, R-000121, R-000122, R-000123, R-000136, R-000138, R-000157, R-000158, R-000159, R-000160, R-000165

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

### R-000158: xsltproc corpus ct.xsl: call-template with a node-set with-param value hangs the transform engine (OPEN)

- **Status:** OPEN
- **Component:** src/xslt/transform/mod.rs
- **Surface:** xsl:call-template / xsl:with-param (transform engine, src/xslt/transform)
- **Oracle versions:** libxslt 1.1.45 (system xsltproc)
- **Root cause:** Pre-existing Phase 9 engine defect (transform/mod.rs unchanged since commit 9b8a2233; this session only added visibility modifiers and ABI exports, neither of which the CLI calls). Passing a node-set expression (//book[1]/title) as a with-param value to a named template drives the engine into an unbounded loop. The in-crate unit test test_xslt_call_template_with_params passes because it uses string params only.
- **Observable residual:** xsltproc ct.xsl doc.xml on the candidate never terminates (system oracle completes in ms).
- **Phase 11 triangulation:** The CLI-XSLTPROC corpus was scaffolded in Phase 9 but never diff-verified in-repo: every receipt is UNKNOWN because the Docker oracle was never built; this hang is one of the corpus gaps.
- **Evidence:** ['courts/corpus/cli/xslt/ct.xsl']
- **Classification:** CANDIDATE_BUG

### R-000159: xsltproc corpus pred.xsl: //book[position() <= 2] matches one extra node (OPEN)

- **Status:** OPEN
- **Component:** src/xslt/transform/mod.rs, src/xml/xpath
- **Surface:** XPath position() inside xsl:for-each predicates (transform engine)
- **Oracle versions:** libxslt 1.1.45 (system xsltproc)
- **Root cause:** Pre-existing Phase 9 engine defect: the candidate produces <b pos="3"> for //book[position() <= 2], i.e. position() evaluates against a different context size than the selected node-set. Same provenance as R-000158 (corpus never diff-verified; engine unchanged this session).
- **Observable residual:** One extra node in the for-each result versus the system oracle.
- **Phase 11 triangulation:** Corpus gap: no CLI-XSLTPROC receipt ever recorded PASS.
- **Evidence:** ['courts/corpus/cli/xslt/pred.xsl']
- **Classification:** CANDIDATE_BUG

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
- **History:** OPEN 2026-08-29; FIXED 2026-08-29; FIXED 2026-08-30

### R-000136: Missing oracle functions: 881 libxml2 + 201 libxslt exports (was 1158 at discovery) (OPEN)

- **Status:** OPEN
- **Component:** src/abi/exports_xml2.rs, src/abi/exports_xslt.rs, src/xml
- **Surface:** DSO function exports
- **Root cause:** The parity obligation census (tools/abi/parity_obligations.py, oracle = system libxml2 2.15.3 / libxslt 1.1.45 DSOs) records 1158 libxml2 and 201 libxslt upstream functions that the candidate does not yet export. These are the 11.1-I obligation ledger entries; each must be implemented (not stubbed) with upstream semantics, court-covered, in dependency order.
- **Fix:** Systematic closure in 11.1-I/X: implement per subsystem (validation, serialization, reader/writer, xpath internals, schemas, relaxng, catalogs, entities, globals, HTML, xslt internals, exslt), adding differential courts per domain. Progress tracked in atlas/PARITY_OBLIGATIONS.json (status MISSING).
- **Evidence:** ['atlas/PARITY_OBLIGATIONS.json']
- **Classification:** UNRESOLVED

### R-000138: Deprecated init/cleanup entry points are no-ops (xmlInitializeGlobalState, xmlInitializeDict, xmlInitializePredefinedEntities, xmlCleanupPredefinedEntities, xmlDefaultSAXHandlerInit, xmlCheckThreadLocalStorage) (OPEN)

- **Status:** OPEN
- **Component:** src/abi/exports_xml2.rs
- **Surface:** DSO function exports
- **Root cause:** Modern libxml2 keeps these as genuine no-ops (subsystems initialize lazily; xmlDefaultSAXHandlerInit fills a global handler the candidate builds on demand; xmlCheckThreadLocalStorage always passes with Rust thread-locals). The candidate exports them with matching no-op behavior; the only observable difference is xmlDefaultSAXHandlerInit not populating the (still missing) xmlDefaultSAXHandler global, tracked in R-000135.
- **Fix:** Monitor: when xmlDefaultSAXHandler/htmlDefaultSAXHandler/xmlDefaultSAXLocator globals are added (R-000135 closure), xmlDefaultSAXHandlerInit must populate xmlDefaultSAXHandler. Otherwise close as documented intentional no-ops.
- **Evidence:** ['atlas/PARITY_OBLIGATIONS.json (EXPORTED_NOOP)']
- **Classification:** INTENTIONAL_SAFE_DIVERGENCE

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

### R-000157: xmlLookupCharEncodingHandler / xmlOpenCharEncodingHandler report XML_ERR_UNSUPPORTED_ENCODING for iconv/ICU-only encodings (OPEN)

- **Status:** OPEN
- **Component:** src/xml/encoding/mod.rs, src/abi/exports_xml2.rs
- **Surface:** xmlLookupCharEncodingHandler, xmlGetCharEncodingHandler, xmlOpenCharEncodingHandler, xmlCreateCharEncodingHandler (encoding.h)
- **Oracle versions:** libxml2 2.15.3 (system)
- **Root cause:** Upstream serves UCS-4LE/BE, EBCDIC, UCS-2, ISO-8859-2..9/10/11/13..16, ISO-2022-JP, Shift_JIS, EUC-JP and windows-1252 via iconv/ICU plus static 8-bit tables; the crate ships no iconv/ICU backend, so those encodings report XML_ERR_UNSUPPORTED_ENCODING (32) where the oracle returns a converter. The native set (UTF-8, UTF-16LE/BE, UTF-16, ISO-8859-1, US-ASCII) and all error paths are byte-identical (ENCODING-001).
- **Observable residual:** A C consumer requesting an iconv-only encoding gets XML_ERR_UNSUPPORTED_ENCODING instead of a handler; conversion through those encodings is unavailable in the candidate.
- **Phase 11 triangulation:** No upstream epoch provides these converters without iconv/ICU; adding an iconv backend is a future work item, not a parity defect.
- **Regression courts:** ENCODING-001.
- **Evidence:** ['courts/suites/data-abi/encoding-family-probe.c', 'courts/receipts/phase-11/encoding-family-*.json']
- **Classification:** INTENTIONAL_SAFE_DIVERGENCE

### R-000160: libxslt exports with literally-trivial upstream 1.1.45 bodies classified as intentional no-ops (OPEN)

- **Status:** OPEN
- **Component:** src/abi/exports_xslt_util.rs, src/abi/exports_xslt_vars.rs, src/abi/exports_xslt_avt.rs
- **Surface:** xsltSecurityAllow, xsltSecurityForbid, xsltGetDebuggerStatus, xsltFreeLocales, xsltFreeAVTList, xsltExtensionInstructionResultRegister (libxslt.so.1)
- **Oracle versions:** libxslt 1.1.45 (system)
- **Root cause:** The upstream 1.1.45 bodies are literally `return(1)` (xsltSecurityAllow), `return(0)` (xsltSecurityForbid, xsltGetDebuggerStatus, xsltExtensionInstructionResultRegister) or empty (xsltFreeLocales, and xsltFreeAVTList in the candidate because AVTs are stored as raw strings). The ledger's static stub heuristic flags constant-return/empty bodies; these are exact upstream semantics, not placeholders.
- **Observable residual:** None — the candidate matches the oracle byte-for-byte on these entry points.
- **Phase 11 triangulation:** Classification-only residual: the ledger labels them INTENTIONAL_NOOP so the closure count is honest.
- **Evidence:** ['archaeology/libxslt-git/libxslt/security.c', 'archaeology/libxslt-git/libxslt/variables.c', 'archaeology/libxslt-git/libxslt/xslt.c', 'archaeology/libxslt-git/libxslt/xsltlocale.c']
- **Classification:** INTENTIONAL_SAFE_DIVERGENCE

## Phase 11.1-J Residuals

### R-000131: Legacy allocator API surface: location tracking, xmlMemSize, debug dumps are simplified (FIXED)

- **Status:** FIXED (, Phase 11.1-J)
- **Component:** src/abi/allocator.rs, include/libxml/xmlmemory.h
- **Surface:** xmlmemory.h API
- **Root cause:** The default candidate allocator does not maintain upstream's per-block metadata table (xmlMallocLoc-style block list). Consequently xmlMemSize returns 0, the *Loc variants accept-and-ignore file/line, and xmlMemDisplayLast/xmlMemoryDump print aggregate counters instead of upstream's per-block dump.
- **Fix:** 11.1-J allocator instrumentation: the default allocator now maintains the upstream-style per-block registry (ptr -> size/file/line). xmlMemSize returns the recorded size, the *Loc variants record the allocation site, xmlMemDisplayLast and xmlMemShow dump per-block listings (ordered by address; xmlMemShow's upstream most-recent ordering is not reproduced — documented divergence), and xmlMemUsed/MEM_BLOCKS are exact (realloc adjusts by the old size). xmlMemSetup custom allocators still bypass the registry (counters only), matching upstream's debug-allocator-only contract.
- **Evidence:** ['src/abi/allocator.rs']
- **Classification:** CANDIDATE_BUG

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
- **Component:** src/abi/exports_xml2.rs, src/xml/xpath/context.rs, src/xml/parser/state.rs, src/xml/tree/mod.rs, src/xml/list/mod.rs, src/xml/sax/default.rs, src/abi/data_globals.rs, src/xslt/transform/mod.rs, include/libxml/*, include/libxslt/xsltutils.h, courts/suites/data-abi/callback-family-probe.c, tools/abi/callback_family_probe.py
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

### R-000165: 65 oracle-DSO exports absent from the candidate (xmlCtxtGet*/Set* parser accessors, xmlNewInputFrom* input constructors, xlink surface, per-module EXSLT registration functions, resource-loader setters, html/encoding/relaxng/xsd/reader/xinclude gaps, xslDebugStatus) (OPEN)

- **Status:** OPEN
- **Component:** src/abi/exports_xml2.rs, src/abi/exports_parserint.rs, src/abi/exports_parser.rs, src/abi/exports_tree.rs, src/abi/exports_misc.rs, src/abi/exports_nano.rs, src/abi/exports_html.rs, src/abi/exports_relaxng.rs, src/abi/exports_schema.rs, src/abi/exports_xinclude.rs, src/abi/exports_xslt.rs, src/abi/exports_xslt_ext.rs, src/abi/exports_string.rs, src/abi/exports_buffer.rs, tools/evidence/subsystem_census.py, atlas/SUBSYSTEM_CENSUS.json
- **Surface:** Parser context accessors (xmlCtxtGetCatalogs/xmlCtxtSetDict/xmlCtxtIsHtml/xmlCtxtParseContent/xmlCtxtPushInput/xmlCtxtValidateDocument family), input-stream constructors (xmlNewInputFromFd/IO/Memory/String/Url), xlink (xlinkGetDefaultDetect/xlinkIsLink family), EXSLT (exsltCommonRegister/exsltMathRegister/... per-module registration), resource loaders (xmlSchemaSetResourceLoader/xmlRelaxNGSetResourceLoader/xmlTextReaderSetResourceLoader/xmlXIncludeSetResourceLoader/xmlCtxtSetResourceLoader), htmlCtxtSetOptions/htmlUTF8ToHtml, xmlIsolat1ToUTF8/xmlUTF8ToIsolat1, xmlRelaxNGValidCtxtClearErrors/xmlRelaxParserSetIncLImit, xslDebugStatus
- **Oracle versions:** libxml2 2.15.3 / libxslt 1.1.45 / libexslt (system DSOs, nm -D --defined-only)
- **Root cause:** The 11.1-O complete subsystem census (tools/evidence/subsystem_census.py; 45 libxml2 + 24 libxslt + 8 EXSLT subsystems, membership from Doxygen inventory + Clang AST atlas + symbol patterns, oracle baseline = system DSO exports) found 65 oracle-DSO-exported symbols with no candidate definition. The PARITY_OBLIGATIONS ledger reported MISSING: 0 because the obligations generator's oracle symbol set omitted these families (parser-context accessors, the xlink module, per-module EXSLT registrations, resource-loader setters and several small helpers), so the export-completeness claim was not actually proven for them.
- **Observable residual:** A C consumer dlopen/dlsym'ing or linking against the candidate for xmlCtxtGetDict, xmlNewInputFromMemory, xlinkIsLink, exsltMathRegister, xmlSchemaSetResourceLoader, xmlUTF8ToIsolat1, xslDebugStatus, etc. fails with an undefined symbol. dlsym returns NULL for each of the 65 names in atlas/SUBSYSTEM_CENSUS.json missing_symbols.
- **Classification:** CANDIDATE_BUG

## Classification Legend

- `CANDIDATE_BUG` — see classification policy in §45/§71
- `INTENTIONAL_SAFE_DIVERGENCE` — see classification policy in §45/§71
- `ORACLE_BUG` — see classification policy in §45/§71
- `UNRESOLVED` — see classification policy in §45/§71
- `VERSION_DIFFERENCE` — see classification policy in §45/§71
