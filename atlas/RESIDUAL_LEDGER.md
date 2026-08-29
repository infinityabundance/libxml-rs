# Residual Ledger

Per §71: every unexplained difference gets an ID (`R-000001`...), and its
history is retained after fixing. This Markdown is generated from
`RESIDUAL_LEDGER.json` (§70 policy: Markdown generated from JSON).

## Current Residuals

**0 open residuals.** All discovered tooling bugs have been fixed.

## Phase 8 Residuals

### R-000101: Parser did not resolve element/attribute namespaces (FIXED)

- **Status:** FIXED (2026-08-28, Phase 8)
- **Component:** `src/xml/parser/state.rs`
- **Surface:** parser / namespaces
- **Oracle versions:** libxml2 2.x (SAX2 namespace processing is core behavior)
- **Root cause:** `sax_start_element` passed the raw qualified name (e.g. `xsl:stylesheet`) as the SAX2 localname and always passed NULL prefix/URI, so the default tree builder never attached namespace pointers to elements or attributes. XSLT compilation therefore could not recognize `xsl:` instructions.
- **Fix:** Split element QNames into prefix + localname, resolve prefixes against the element's namespace declarations (with the `xml` prefix special case), and pass resolved prefix/URI/namespace arrays to the SAX2 dispatcher.
- **Regression courts:** XSLT end-to-end transform tests (`test_end_to_end_*`, `test_xslt_*`).

### R-000102: XPath absolute paths evaluated from the root element (FIXED)

- **Status:** FIXED (2026-08-28, Phase 8)
- **Component:** `src/xml/xpath/eval.rs`
- **Surface:** XPath
- **Root cause:** `eval_absolute_path` searched the document's children for a type-9 node (which never exists as a child), falling back to the root element. `/root/item` therefore looked for `root` *inside* the root element and returned empty.
- **Fix:** The context node for absolute paths is now the document node itself (`doc as *mut _xmlNode`), matching XPath 1.0 `/` semantics.
- **Regression courts:** `test_end_to_end_template_transform`, `test_xslt_variable_and_call_template`.

### R-000103: Template content double-freed with stylesheet document (FIXED)

- **Status:** FIXED (2026-08-28, Phase 8)
- **Component:** `src/xslt/templates/mod.rs`
- **Surface:** memory / ownership
- **Root cause:** `xsltFreeTemplate` freed the template's content tree, but template content nodes are owned by the stylesheet document (`style->doc`) and were freed a second time by `xsltFreeStylesheet`'s `xmlFreeDoc`.
- **Fix:** `xsltFreeTemplate` no longer frees the content tree (matching upstream libxslt); the document owns those nodes. The template's heap-copied name/mode strings are freed.
- **Regression courts:** `test_parse_stylesheet_memory` (double-free would abort).

### R-000104: Result document version/encoding strings double-freed (FIXED)

- **Status:** FIXED (2026-08-28, Phase 8)
- **Component:** `src/xslt/transform/mod.rs`
- **Surface:** memory / ownership
- **Root cause:** `xsltApplyStylesheetUser` pointed the result document's `version` at a static literal and copied the stylesheet's encoding/version pointers; `free_doc` frees those fields with `xmlFree`, causing invalid frees / double frees.
- **Fix:** The result document's version/encoding are heap-copied with `xml_strdup`.
- **Regression courts:** `test_end_to_end_*`.

### R-000105: `node()`/`text()` etc. parsed as function calls (FIXED)

- **Status:** FIXED (2026-08-28, Phase 8)
- **Component:** `src/xslt/patterns/mod.rs`
- **Surface:** XSLT patterns
- **Root cause:** Top-level `node()`, `text()`, `comment()`, `processing-instruction()` parse as `FunctionCall` nodes in the XPath AST, so `collect_steps`/`compute_expr_priority` treated them as unknown (priority 0.5, no match).
- **Fix:** Translate bare node-test function calls into steps (child axis + node test) in `collect_steps` and map their priorities in `compute_expr_priority` (-0.25 for `node()`, 0.0 for the others).
- **Regression courts:** pattern priority and compile tests.

### R-000106: `match="/"` matched the root element (FIXED)

- **Status:** FIXED (2026-08-28, Phase 8)
- **Component:** `src/xslt/patterns/mod.rs`
- **Surface:** XSLT patterns
- **Root cause:** `"/"` parses as a bare `Self_/node()` step; the matcher treated it as matching any node, so the root template also matched the root element.
- **Fix:** A bare `Self_/node()` step with no predicates and no other steps represents the document root pattern: empty steps with `is_absolute=true`, matching only document nodes.
- **Regression courts:** `test_end_to_end_simplified_stylesheet`.

## Phase 9 Residuals

### R-000107: XPath core functions not registered in the XSLT transform context (FIXED)

- **Status:** FIXED (2026-08-29, Phase 9)
- **Component:** `src/xslt/transform/mod.rs`
- **Surface:** XSLT / XPath integration
- **Oracle versions:** libxslt 1.1.x (core XPath functions always available during transforms)
- **Root cause:** `register_xslt_functions` registered only the XSLT extension functions (`document()`, `key()`, …) and EXSLT, never the XPath 1.0 core function library. Every XPath function call (`count(library/book)`, `substring('hello',1,2)`, …) failed evaluation with an unknown-function error, surfacing as "Invalid expression" and empty output.
- **Fix:** Register `crate::xml::xpath::functions::core_functions()` in the transform context before the XSLT-specific functions.
- **Regression courts:** `test_xslt_core_functions_in_value_of`.

### R-000108: Attribute value templates never evaluated (FIXED)

- **Status:** FIXED (2026-08-29, Phase 9)
- **Component:** `src/xslt/transform/mod.rs`
- **Surface:** XSLT transform
- **Oracle versions:** libxslt 1.1.x (XSLT 1.0 §7.6.2)
- **Root cause:** Literal result element attributes were copied verbatim (`id="{@id}"` appeared literally in the output); `xsl:element`/`xsl:attribute`/`xsl:processing-instruction` names were not AVT-evaluated either.
- **Fix:** Implemented `eval_avt` (`{{`/`}}` escapes, `{expr}` evaluation via `eval_xpath`, unmatched `{` literal) and wired it into literal attributes and the `name`/`namespace` attributes of `xsl:element`, `xsl:attribute`, and `xsl:processing-instruction`.
- **Regression courts:** `test_xslt_avt_in_literal_attribute`, `test_xslt_avt_in_xsl_element_name`.

### R-000109: RTF variable tree double-freed; `exsl:node-set` unsupported (FIXED)

- **Status:** FIXED (2026-08-29, Phase 9)
- **Component:** `src/xslt/variables/mod.rs`, `src/xslt/compiler/mod.rs`, `src/xslt/documents/mod.rs`
- **Surface:** memory / ownership; EXSLT
- **Oracle versions:** libxslt 1.1.x (RVT semantics, exsltCommon.c)
- **Root cause:** (1) `compile_variable` set `var->tree = inst->children` — nodes owned by the stylesheet document; `xsltFreeStackElem` freed them, and `xsltFreeStylesheet` freed them again (double-free / heap corruption). (2) Inline variable content was flattened to a string, so `exsl:node-set($var)/path` navigation returned nothing.
- **Fix:** `xsltFreeStackElem` no longer frees the stylesheet-owned `tree`. `register_global_value` deep-copies inline content into a context-owned RVT document registered in the docCache (freed exactly once at context teardown, after the XPath context) and binds the variable to a node-set containing the RVT document node — matching upstream `xmlXPathNewValueTree`.
- **Regression courts:** `test_xslt_variable_inline_content_rtf`, `test_xslt_exsl_node_set_on_rtf`.

### R-000110: `node_get_content` ignored descendant text (FIXED)

- **Status:** FIXED (2026-08-29, Phase 9)
- **Component:** `src/xml/tree/mod.rs`
- **Surface:** tree / XPath string-value
- **Oracle versions:** libxml2 2.x (`xmlNodeGetContent`)
- **Root cause:** For element nodes, `node_get_content` concatenated only *direct* text/CDATA children, so `<book><title>Rust</title></book>` had an empty string-value and `<library>…</library>` only whitespace.
- **Fix:** Recurse into element children so the string-value is the concatenation of all descendant text nodes (XPath 1.0 §4.2 semantics).
- **Regression courts:** `test_node_get_content_recurses_descendants`, `test_xslt_core_functions_in_value_of`.

### R-000111: Caller parameters parsed as `name=value` single strings (FIXED)

- **Status:** FIXED (2026-08-29, Phase 9)
- **Component:** `src/xslt/parameters/mod.rs`
- **Surface:** XSLT parameters
- **Oracle versions:** libxslt 1.1.x (`xsltEvalUserParams`, variables.c)
- **Root cause:** `xsltParseStylesheetParams` parsed the params array as single `"name=value"` strings, but upstream passes a NULL-terminated array of `(name, value)` pairs where the value is an XPath expression evaluated later.
- **Fix:** Parsed as `(name, value)` pairs; `xsltParseStylesheetParam` takes separate name/value arguments with `{uri}name` namespace support; values bound with `XSLT_VAR_PARAM | XSLT_VAR_INTERNAL`.
- **Regression courts:** `test_parse_params_array_pairs` and the other `xslt::parameters::tests`.

### R-000112: `date:date()`/`date:time()` no-argument default missing (FIXED)

- **Status:** FIXED (2026-08-29, Phase 9)
- **Component:** `src/exslt/dates/mod.rs`
- **Surface:** EXSLT dates
- **Oracle versions:** libxslt 1.1.x (EXSLT dates spec)
- **Root cause:** `date_arg` returned `None` for a missing argument, so no-argument calls to `date:date()`/`date:time()`/`date:year()` etc. returned the empty string instead of operating on the current date-time.
- **Fix:** No-argument calls default to `now()` (matching EXSLT dates spec and upstream `dateArg`).
- **Regression courts:** EXSLT dates tests (`exslt::dates::tests`).

### R-000113: `xsl:if`/`xsl:when` boolean conversion read only `boolval` (FIXED)

- **Status:** FIXED (2026-08-29, Phase 9, differential oracle)
- **Component:** `src/xslt/transform/mod.rs`
- **Surface:** XSLT transform
- **Oracle versions:** libxslt 1.1.45 (differential `xsltproc` corpus)
- **Root cause:** `process_if`/`process_choose` tested `(*obj).boolval`, which is only valid for boolean objects; node-set tests (`test="author"`), numbers, and strings always converted to false.
- **Fix:** Added `xpath_obj_boolean` applying XPath 1.0 §4.3 boolean conversion (node-set non-empty, number non-zero/non-NaN, string non-empty).
- **Regression courts:** `test_xslt_if_node_set_test`; differential corpus `if.xsl`, `basic.xsl`.

### R-000114: Attribute string-value empty in the XPath engine (FIXED)

- **Status:** FIXED (2026-08-29, Phase 9, differential oracle)
- **Component:** `src/xml/xpath/types.rs`
- **Surface:** XPath string-value
- **Oracle versions:** libxslt 1.1.45 / libxml2 2.15.3 (differential corpus)
- **Root cause:** `node_string_value` treated type 13 as attribute — but 13 is `XML_HTML_DOCUMENT_NODE`; attributes are type 2 and their value lives in the first text child (not `content`). Every `string(@attr)`, `@attr='x'` predicate, and attribute-based sort returned empty.
- **Fix:** Attributes (type 2) read the first text child; type 13 handled as a document node (descendant text).
- **Regression courts:** `test_xslt_attribute_string_value`; differential corpus `attr.xsl`, `pred.xsl`, `sets.xsl`.

### R-000115: `xsl:sort` never compiled or applied (FIXED)

- **Status:** FIXED (2026-08-29, Phase 9, differential oracle)
- **Component:** `src/xslt/transform/mod.rs`, `src/xslt/sorting/mod.rs`
- **Surface:** XSLT sorting
- **Oracle versions:** libxslt 1.1.45 (differential corpus `sort.xsl`)
- **Root cause:** (1) `find_sort_children` always passed `ptr::null_mut()` as the stylesheet, so `xsltCompileSort` bailed and no sort was applied. (2) `xsltEvalSortKey` evaluated the sort key expression without setting the internal XPath context node, so all keys evaluated against the wrong context and compared equal.
- **Fix:** Pass `(*ctxt).style` into `xsltCompileSort`; set both the C-struct and internal XPath context node in `xsltEvalSortKey`.
- **Regression courts:** `test_xslt_sort_descending`; differential corpus `sort.xsl`.

### R-000116: `key()` XPath function was a stub (FIXED)

- **Status:** FIXED (2026-08-29, Phase 9, differential oracle)
- **Component:** `src/xslt/transform/mod.rs`, `src/xslt/keys/mod.rs`, `src/abi/exports_xml2.rs`
- **Surface:** XSLT keys
- **Oracle versions:** libxslt 1.1.45 (differential corpus `keys.xsl`)
- **Root cause:** (1) The registered `key()` closure returned an empty node-set; the real `xsltEvalKeyFunction` was never bridged (no transform-context pointer reachable from the XPath function). (2) `build_key_table` evaluated the `use` expression without setting the internal XPath context node, so key values were empty strings.
- **Fix:** Stash the transform context in the internal XPath context's `func_lookup_data` slot; the `key()` closure calls `xsltEvalKeyFunction` (string value of the first node when the value is a node-set). Set the internal context node in `build_key_table`. Exported `xmlXPathFreeNodeSet` (missing ABI surface).
- **Regression courts:** `test_xslt_key_function`; differential corpus `keys.xsl`.

### R-000117: Local variables/params invisible to XPath evaluation (FIXED)

- **Status:** FIXED (2026-08-29, Phase 9, differential oracle)
- **Component:** `src/xslt/variables/mod.rs`, `src/xslt/parameters/mod.rs`, `src/xslt/transform/mod.rs`, `src/abi/exports_xml2.rs`
- **Surface:** XSLT variables & parameters
- **Oracle versions:** libxslt 1.1.45 (differential corpus `ct.xsl`)
- **Root cause:** Local `xsl:variable`/`xsl:param` and `xsl:with-param` were pushed onto the transform variable/parameter stacks only — the XPath evaluator reads the internal `XPathContext.variables` hash, so `$name` resolved empty. `process_param` checked the wrong stack for passed values.
- **Fix:** `xsltPushVariable`/`xsltPushParam` register their values in the XPath context hash (unregistered on pop); `process_param` consults the hash; `object_to_xpathvalue` handles `XPATH_XSLT_TREE` (RTF → node-set of the document node) so local RTF variables stringify and remain navigable.
- **Regression courts:** `test_xslt_call_template_with_params`; differential corpus `ct.xsl`.

### R-000118: HTML output method differed from upstream (FIXED)

- **Status:** FIXED (2026-08-29, Phase 9, differential oracle)
- **Component:** `src/xml/html/mod.rs`
- **Surface:** XSLT serialization (method="html")
- **Oracle versions:** libxslt 1.1.45 (differential corpus `html.xsl`)
- **Root cause:** The HTML serializer (1) never inserted `<meta charset="...">` into the root `<head>` and (2) used two-space indentation, while upstream `htmlNodeDumpFormatOutput` writes newlines only.
- **Fix:** Insert `<meta charset="ENCODING">` as the first child of a root `<head>` lacking a `<meta>` element (encoding from the document, default UTF-8); formatting writes newlines without indentation spaces.
- **Regression courts:** `test_xslt_html_method_meta_charset`; differential corpus `html.xsl`.

## Fixed Residuals

### R-000001: `#line` directive mapping uses wrong coordinate space

- **Status:** FIXED
- **Component:** `tools/archaeology/apiatlas.py`
- **Surface:** tooling
- **Root cause:** The `resolve_origin` function used the original source line number from `#line` directives as dict keys. When multiple directives shared the same original line number, they overwrote each other, causing incorrect file attribution. Furthermore, the `#line` mapping approach is fundamentally flawed because `loc.line` from clang's AST is in the **original source file** coordinate space, while `#line` directive positions are in the **preprocessed output** coordinate space — these are different and cannot be compared directly.
- **Fix:** Replaced the `#line` mapping approach entirely. The new `resolve_origin` uses clang's AST location fields directly: (1) `loc.file` for type declarations from included files, (2) `loc.includedFrom` presence to detect function declarations from included files (filtered out — they'll be captured when their own header is processed), (3) absence of both for direct declarations in the main file.
- **Evidence:** 45 system functions leaked into header inventory; most HTML functions (44+) were missing; tree functions were missing.

### R-000002: System header path filtering missing

- **Status:** FIXED
- **Component:** `tools/archaeology/apiatlas.py`
- **Surface:** tooling
- **Root cause:** No explicit filter for system header paths in the `collect()` function. When `resolve_origin` returned `None` (for declarations from included files), the caller didn't handle it.
- **Fix:** Added `None` check for origin, and added a comprehensive system function name denylist (`SYSTEM_FUNCTION_NAMES`) as a secondary defense for declarations that bypass origin-based filtering.
- **Evidence:** 45 system functions (fopen, fprintf, printf, etc.) in header inventory.

### R-000003: Internal `__xml*` functions not classified

- **Status:** FIXED
- **Component:** `tools/archaeology/symbols.py`
- **Surface:** tooling
- **Root cause:** Internal `__xml*` function declarations (the implementations behind public function-pointer variables) were listed alongside potentially missing API functions.
- **Fix:** Added `INTERNAL_FUNCTIONS` set and separate reporting in `internal_functions` field.
- **Evidence:** 6 `__xml*` functions now correctly classified.

### R-000004: SAX1 callback names not classified

- **Status:** FIXED
- **Component:** `tools/archaeology/symbols.py`
- **Surface:** tooling
- **Root cause:** SAX1 callback struct field names appeared in DSO symbol tables as OBJECT type but were listed as undocumented function exports.
- **Fix:** Added `SAX1_CALLBACK_NAMES` set and separate reporting in `sax1_callbacks` field.
- **Evidence:** 24 SAX1 callback names now correctly classified.

### R-000005: `XML_TREE_INTERNALS` not defined when processing tree.h

- **Status:** FIXED
- **Component:** `tools/archaeology/apiatlas.py`
- **Surface:** tooling
- **Root cause:** `tree.h` has a circular dependency workaround: when `XML_TREE_INTERNALS` is not defined, it just includes `parser.h` and hides all tree declarations. Other headers (parser.h, entities.h, valid.h, xmlIO.h) define this before including tree.h, but when processing tree.h directly, the define was missing.
- **Fix:** Added `-DXML_TREE_INTERNALS` to clang include args globally.
- **Evidence:** tree.h showed 0 FunctionDecl declarations; `xmlAddChild` and all tree functions were missing from the API inventory.

## Classification Legend

- `CANDIDATE_BUG` — Bug in the libxml-rs tooling
- `ORACLE_BUG` — Bug in the upstream implementation
- `VERSION_DIFFERENCE` — Difference due to version mismatch
- `INTENTIONAL_SAFE_DIVERGENCE` — Known safe difference
- `UNRESOLVED` — Not yet classified
