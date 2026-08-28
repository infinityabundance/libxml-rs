# Parity Matrix

Generated from `PARITY_MATRIX.json` (§70). Last updated: 2026-08-20.

## API Completeness

### libxml2 2.15.3

| Surface | Headers | DSO | Both | H-only | DSO-only | Status |
|---------|---------|-----|------|--------|----------|--------|
| Functions | 1403 | 1658 | **1344** | 53 | 290 | 🟢 81% |
| Globals | 17 | 89 | **17** | 0 | 72 | 🟢 100% of captured |
| System leaks | — | — | — | **0** | — | ✅ Clean |
| Internal `__xml*` | 6 | — | — | 6 | — | ✅ Classified |
| SAX1 callbacks | — | 24 | — | — | 24 | ✅ Classified |
| Typedefs | 312 | — | — | — | — | 📋 Cataloged |
| Records (struct/union) | 76 | — | — | — | — | 📋 Cataloged |
| Enums | 39 | — | — | — | — | 📋 Cataloged |
| Enumerators | 1131 | — | — | — | — | 📋 Cataloged |
| Callbacks | 86 | — | — | — | — | 📋 Cataloged |
| Headers processed | 45 | — | — | — | — | ✅ Complete |

### libxslt 1.1.45

| Surface | Headers | DSO | Both | H-only | DSO-only | Status |
|---------|---------|-----|------|--------|----------|--------|
| Functions | 231 | 232 | **225** | 6 | 7 | 🟢 97% |
| Globals | 12 | 39 | **12** | 0 | 27 | 🟢 100% of captured |
| System leaks | — | — | — | **0** | — | ✅ Clean |
| Typedefs | 63 | — | — | — | — | 📋 Cataloged |
| Records | 16 | — | — | — | — | 📋 Cataloged |
| Enums | 7 | — | — | — | — | 📋 Cataloged |
| Enumerators | 65 | — | — | — | — | 📋 Cataloged |
| Callbacks | 17 | — | — | — | — | 📋 Cataloged |
| Headers processed | 23 | — | — | — | — | ✅ Complete |

## Tools Status

| Tool | Status | Notes |
|------|--------|-------|
| `manifest.py` | ✅ Working | 183 libxml2 + 92 libxslt releases cataloged |
| `profileconfig.py` | ✅ Working | Generates xmlversion.h/xsltconfig.h for distro profile |
| `apiatlas.py` | ✅ Working | Clang-AST-based public API extractor. Fixes applied: (1) origin resolution uses loc.file/loc.includedFrom instead of buggy #line mapping, (2) XML_TREE_INTERNALS define for tree.h circular dependency, (3) system function name denylist |
| `symbols.py` | ✅ Working | ABI ground-truth comparison via readelf. Classifies SAX1 callbacks, internal __xml* functions, system leaks |
| `delta.py` | ✅ Built | Ready for multi-version API diffing |
| `court runner` | ✅ Built | Ready for differential testing (needs Docker oracle) |

## Phase 8 — libxslt (XSLT 1.0 Engine) Status

Complete as of 2026-08-28: 1060 library tests passing (0 failures).

### XSLT Subsystem Coverage

| Module | Status | Notes |
|--------|--------|-------|
| Stylesheet lifecycle | ✅ Complete | `xsltStylesheetCreate`, `xsltParseStylesheetDoc/File/Memory`, `xsltFreeStylesheet`, `xsltGet/SetStylesheetDoc` |
| Compiler | ✅ Complete | Template compilation, top-level elements (key, decimal-format, namespace-alias, attribute-set, strip/preserve-space, output, variable, param), imports, includes, simplified stylesheets |
| Templates | ✅ Complete | Priority-ordered list, `xsltFindTemplate` (XSLT §5.2), `xsltLookupTemplate`, default priority per §5.5 |
| Patterns | ✅ Complete | XSLT pattern compiler (union, `//`, `@`, `*`, `node()`, `text()`, `comment()`, `processing-instruction()`, predicates), `xsltDefaultPriority` |
| Variables/Params | ✅ Complete | Stack management, global variable initialization, with-param passing |
| Keys | ✅ Complete | Key definitions, table construction, `key()` function support |
| Sorting | ✅ Complete | Multi-key sort, text/number data types, ascending/descending |
| Numbering | ✅ Complete | `xsl:number` with decimal/alphabetic/roman formats |
| Attributes | ✅ Complete | Attribute set compilation and application |
| Namespace alias | ✅ Complete | `xsl:namespace-alias` compilation and resolution |
| Whitespace | ✅ Complete | strip-space/preserve-space rules with import-depth precedence |
| Imports/Includes | ✅ Complete | Import tree construction, import depth, include merging |
| Documents | ✅ Complete | `document()` cache, loader function plumbing |
| Extensions | ✅ Complete | `xsltRegisterExtFunction`, `xsltRegisterExtElement` |
| Serialization | ✅ Complete | `xsltSaveResultToFile/Fd/String` with output method selection (xml/html/text) |
| Security | ✅ Complete | Full security prefs API with global defaults |
| Errors | ✅ Complete | Error domains, handler wiring, stderr reporting |
| Transform | ✅ Complete | `xsltApplyStylesheet(User/Stacked)`, transform context, instruction execution (apply-templates, call-template, for-each, value-of, copy-of, copy, element, attribute, text, comment, PI, number, choose, if, variable, param, message, apply-imports), XSLT XPath functions (document, key, generate-id, system-property, element-available, function-available, current) |
| ABI exports | ✅ Complete | All 33 libxslt symbols exported from the shared library |

### Phase 8 Fixes to Underlying Subsystems

| Fix | Surface | Details |
|-----|---------|--------|
| Namespace resolution in parser | xml/parser/state.rs | SAX2 `startElementNs` now receives split prefix/localname and resolved URIs for elements and attributes |
| Absolute path evaluation | xml/xpath/eval.rs | `/root/item` now evaluates from the document node, not the root element |
| XPath C ABI helpers | abi/exports_xml2.rs | Added `xmlXPathObjectCopy`, `xmlXPathCastToString`, `xmlXPathCastStringToNumber`, `xmlXPathCmpNodes`, `xmlXPathNodeSetCreate` |
| Node content getter | xml/tree/mod.rs | Added `node_get_content` (upstream `xmlNodeGetContent` semantics) |

## Infrastructure Status

| Component | Status | Notes |
|-----------|--------|-------|
| Docker oracle | 🟡 Scaffolded | Dockerfile and build script created, not yet built |
| Court receipts | 🟡 Scaffolded | Schema and runner created, no cases executed yet |
| Historical API matrix | 🟡 Partial | Only 2.15.3 + 1.1.45 snapshots taken |
| Historical deltas | 🔴 Not started | Need at least 2 versions per project |
| Downstream consumers | 🔴 Not started | |
