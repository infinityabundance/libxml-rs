# libxml-rs Ownership Atlas

> Phase 11.1-J deliverable. Records the memory/ownership contract of every
> allocating, adopting, borrowing, copying, transferring, caching,
> registering or freeing surface, grounded in the candidate implementation
> and cross-checked against upstream libxml2 2.15.3 / libxslt 1.1.45
> conventions. "Caller" always means the C consumer of the exported ABI.

## 1. Allocator domains

| Domain | Functions | Free with | Notes |
|---|---|---|---|
| XML allocator | `xmlMalloc`, `xmlMallocAtomic`, `xmlMallocZero`, `xmlRealloc`, `xmlMemStrdup`, `xmlMallocLoc`/`xmlReallocLoc`/`xmlMemStrdupLoc`, `xmlMemMalloc`/`xmlMemFree`/`xmlMemRealloc`/`xmlMemoryStrdup` | `xmlFree` | Defaults to libc malloc; swappable via `xmlMemSetup`. The block registry (11.1-J) tracks every block: `xmlMemSize`, `xmlMemDisplayLast`, `xmlMemShow` report per-block data. |
| libc | `libc::calloc`/`libc::malloc`/`libc::realloc` used inside engine internals for `_xsltDocument` shells, key tables, `varsTab` arrays | matching `libc::free` | Never handed to C callers as opaque pointers except where upstream does the same (`xsltNewSecurityPrefs` → `xsltFreeSecurityPrefs`). |
| XPath objects | `xmlXPathNewString`/`NewNodeSet`/… and every `xmlXPath*Eval*` result | `xmlXPathFreeObject` | |
| Buffers | `xmlBufferCreate`/`xmlBufCreate` | `xmlBufferFree`/`xmlBufFree` | `xmlBufferDetach` transfers the content pointer to the caller (caller frees with `xmlFree`). |

Rule: a pointer returned by an `xml*` allocator function must be freed by
`xmlFree`; a pointer returned by `libc::calloc` inside the candidate is freed
internally and never escapes except through the documented `*Free*` ABI pair.

## 2. Tree ownership (tree.h)

| Object | Creator | Owner after creation | Frees | Children |
|---|---|---|---|---|
| `xmlDoc` | `xmlNewDoc`, `xmlReadMemory`, `xmlParseFile`, `xsltApplyStylesheet` (result) | caller | `xmlFreeDoc` | owns the whole subtree |
| `xmlNode` (element/text/comment/PI/cdata) | `xmlNewNode`, `xmlNewText`, … | parent after `xmlAddChild`; otherwise caller | `xmlFreeNode` (no children) / `xmlFreeNodeList` (list) | `xmlUnlinkNode` detaches; `xmlAddChild`/`xmlAddNextSibling` transfer ownership of the subtree to the new parent |
| `xmlAttr` | `xmlNewProp`, `xmlNewNsProp` | parent element after `xmlAddChild`-equivalent attach | freed with the element | value lives in the first text child |
| `xmlNs` | `xmlNewNs`, `xmlNewGlobalNs`, `xsltGetSpecialNamespace` | namespace list of the node | `xmlFreeNs` (only for unlinked standalone `xmlNewNs`) | — |
| DTD / entity decls | `xmlNewDtd`, parser | doc | `xmlFreeDtd` (also freed with doc) | children of entity decls owned by the decl |

Borrowing rule: `node->parent`, `node->doc`, `node->next/prev`, `node->ns`
are borrowed pointers — never freed by the reader.

## 3. Strings

| Returned by | Domain | Contract |
|---|---|---|
| `xmlNodeGetContent`, `xmlNodeListGetString`, `xmlGetProp`, `xmlGetNsProp`, `xmlXPathCastToString`, `xsltAttrTemplateValueProcess`, `xsltEvalAVT`, `xsltEvalXPathString`, `xsltStrxfrm` | xml allocator | caller frees with `xmlFree` |
| `xmlGetCharEncodingName`, `xmlGetEncodingAlias`, `xmlStrchr`, dict lookups (`xmlDictLookup`, `xmlDictQLookup`) | static / dict | borrowed — never free |
| `xsltSplitQName` | dict | borrowed (dict-interned) |
| attribute/node `->name`, `->content` | doc/stylesheet doc | owned by the node/doc — never freed separately |

Dict-owned strings: `xmlDictLookup` results are stable for the dict's
lifetime; freeing the dict invalidates them. The candidate's `_xmlDoc.dict`
is shared with the parser context and kept alive for the doc's lifetime.

## 4. Document / stylesheet / result / RVT ownership (libxslt)

| Object | Creator | Owner | Free |
|---|---|---|---|
| Stylesheet | `xsltParseStylesheetFile/Doc/Memory`, `xsltNewStylesheet` | caller | `xsltFreeStylesheet` (frees imports, templates, key defs, attr sets, aliases, the style docs) |
| Style documents | `xsltLoadStyleDocument` → `_xsltDocument` wrapper | stylesheet (`style->docList`) | `xsltFreeStylesheet` via `xsltFreeStyleDocuments` |
| Source document | caller | caller | `xmlFreeDoc` (the transform only borrows) |
| Result document | `xsltApplyStylesheet(User/Stacked)`, `xsltProfileStylesheet` | caller | `xmlFreeDoc`; `xsltFreeTransformResult` is an alias |
| `_xsltDocument` wrapper | `xsltNewDocument`, `xsltLoadDocument` | context (`ctxt->docList`) | `xsltFreeDocuments` (wrapper only; the wrapped doc stays with the caller or the loader cache) |
| RVT (result-tree fragment) | `xsltCreateRVT` | context RVT lists | `xsltReleaseRVT` (local/tmp) or `xsltFreeRVTs` (persist); `xsltFlagRVTs` marks function results |
| Key tables | `xsltInitCtxtKey(s)`, `xsltInitAllDocKeys` | document wrapper (`idoc->keys`) | `xsltFreeDocumentKeys` / `xsltFreeKeys` (defs) |
| Global variables | `xsltParseGlobalVariable/Param`, `xsltEvalUserParams` | stylesheet / context | `xsltFreeGlobalVariables` |
| Locale | `xsltNewLocale` | caller | `xsltFreeLocale` / `xsltFreeLocales` |
| Security prefs | `xsltNewSecurityPrefs` | caller | `xsltFreeSecurityPrefs` |
| Extension module data | `xsltInitCtxtExts` (init func) | context `extInfos` | `xsltShutdownCtxtExts` + `xsltFreeCtxtExts` |

Result-document rule: `xsltApplyStylesheet` returns a fresh doc the caller
owns; the context's `output` pointer is cleared before `xsltFreeTransformContext`
so the caller's free is not double-freed.

## 5. XPath / reader / writer / catalog

| Surface | Contract |
|---|---|
| `xmlXPathCompile` / `xsltXPathCompile` → `xmlXPathCompExprPtr` | caller frees with `xmlXPathFreeCompExpr` |
| `xmlXPathEvalExpression` / `xmlXPathEval` → object | caller frees with `xmlXPathFreeObject` (object owns its node-set / string / number storage) |
| `xmlXPathNodeSetCreate` + `xmlXPathNodeSetAdd` | caller frees with `xmlXPathFreeNodeSet`; the nodes are borrowed |
| `xmlTextReader` | `xmlReaderForMemory`/`xmlNewTextReader` → caller frees with `xmlFreeTextReader`; the reader owns its input buffer; `xmlTextReaderExpand`/`xmlTextReaderCurrentNode` return borrowed nodes (valid until the next read) |
| `xmlTextWriter` | `xmlNewTextWriter*` → caller frees with `xmlFreeTextWriter`; `xmlTextWriterStartDocument`/`StartElement` must be balanced with `EndDocument`/`EndElement` before free (cleanup ordering) |
| `xmlBuffer` given to a writer (`xmlNewTextWriterMemory`) | borrowed — the writer appends, the caller frees the buffer after `xmlFreeTextWriter` |
| Catalog | `xmlLoadCatalog`, `xmlNewCatalog` → caller frees with `xmlFreeCatalog`; `xmlCatalogAdd` values are copied; `xmlCatalogGetEntries` returns the catalog's internal structures (borrowed) |

## 6. Callback user-data

Every handler registration pair owns the user-data pointer *by convention
only*: `xmlSetGenericErrorFunc(ctx, …)`, `xmlSetStructuredErrorFunc`,
`xsltSetGenericErrorFunc`, `xsltSetTransformErrorFunc(ctxt, ctx, …)`,
`xmlRegisterInputCallbacks`/`xmlRegisterOutputCallbacks` contexts,
`xsltSetLoaderFunc`, `xsltSetSecurityPrefs` callbacks. The library stores the
pointer and passes it back verbatim; the caller keeps it alive and frees it
after deregistration. The candidate never dereferences user-data.

## 7. Cleanup ordering

1. Result documents before contexts: `xmlFreeDoc(result)` after
   `xsltApplyStylesheet`, then `xsltFreeTransformContext`.
2. Writer: end document/elements → `xmlFreeTextWriter` → free the buffer.
3. Reader: `xmlFreeTextReader` before freeing the underlying document only
   when the reader was created from a caller-owned doc the caller still owns
   (`xmlNewTextReader(doc, …)` borrows; `xmlReaderForMemory` owns its copy).
4. Dictionaries: dict outlives every string interned from it; free the dict
   after the docs that reference it.
5. Context extension data: `xsltShutdownCtxtExts` (callbacks) before
   `xsltFreeCtxtExts` (storage).

## 8. Mismatched-but-valid upstream patterns preserved

- `xmlFreeDoc` on an `xsltApplyStylesheet` result whose context already ran
  `xsltFreeRVTs`: RVT docs are owned by the context lists, the result tree is
  not an RVT — safe.
- `xsltSaveResultToString` hands the caller a buffer allocated with
  `xmlMalloc` (free with `xmlFree`), matching upstream.
- Double-free protection: the candidate's `xmlFree` on a foreign/unknown
  pointer is a no-op removal from the registry (upstream would corrupt);
  documented safe divergence.

## 10. Memory-tooling verification (11.1-J)

- **AddressSanitizer** (nightly `-Zbuild-std -Zsanitizer=address`): the full
  1135-test suite runs clean (0 invalid reads/writes, 0 use-after-free,
  0 double-free) with `detect_leaks=0` (tests deliberately leak their
  fixture strings). ASan caught and fixed two test-code bugs: an `xmlNs`
  freed via `free_node` (layout misinterpretation — now `xmlFreeNs`), and
  an HTML test passing a non-NUL-terminated buffer to a
  NUL-terminated-string API.
- **Valgrind**: unusable on this host (glibc 2.44 SIGILLs valgrind 3.25.1 in
  `_dl_start` before any program runs); documented limitation.
- **Allocator instrumentation**: the block registry (R-000131) gives
  `xmlMemSize`, exact `xmlMemUsed`, and per-block `xmlMemDisplayLast` /
  `xmlMemShow`; a future double-free detector can poison freed blocks in
  the registry.

## 9. Known divergence (documented)

- `xmlFree` is exported as a *function* while the upstream header declares it
  as a data variable — pre-existing drop-in mismatch, tracked as a follow-up;
  C probes avoid calling `xmlFree` directly.
- Allocator instrumentation (R-000131): `xmlMemSetup` custom allocators keep
  counter-only accounting (no per-block registry), matching upstream's
  debug-allocator-only block table.
