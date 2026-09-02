# Phase 14.3 — SAX general-entity registry root cause (recorded 2026-09-02)

Tracked from head commit `9f354caf` (post-Fix-4: candidate full six-extension suite
**1291 / 308 failed / 40 skipped**; clean authoritative re-measure in `/out/php-cand-it1.log`
when candidates are rebuilt from the current tree, run inside `phpbuild-c`).

## Failure class under investigation

The `ext/xml` suite (and the family of DOM/SAX entity tests) require that a DTD
**internal-subset general entity** declared as `<!ENTITY e "ENT">` be substitutable
when a parser has `XML_PARSE_NOENT` (or the entity is otherwise reported expanded
through expat-paradigm callbacks).

`ext/xml` drives the parser through PHP 8.5 `ext/xml/compat.c`:

```c
parser->parser = xmlCreatePushParserCtxt((xmlSAXHandlerPtr) &php_xml_compat_handlers,
                                         (void *) parser, NULL, 0, NULL);
xmlCtxtUseOptions(parser->parser, XML_PARSE_OLDSAX | XML_PARSE_NOENT);
...
int error = xmlParseChunk(parser->parser, data, data_len, is_final);
```

`php_xml_compat_handlers` provides `getEntity` which resolves the declared entity
through the library's own document registry:

```c
ret = xmlGetPredefinedEntity(name);
if (ret == NULL) ret = xmlGetDocEntity(parser->parser->myDoc, name);
```

**Observed divergence (candidate vs oracle, byte-exact repro in `entprobe.c`):**

| input `a="x&e;y"` and content `x&e;y` with `<!ENTITY e "ENT">` | oracle  | candidate |
|---|---|---|
| SAX char-data callbacks                      | `CD[x] CD[ENT] CD[y]`, parse ok | `CD[x]`, then `rc=-1`, entity `e` not defined |
| `xmlGetDocEntity(myDoc,"e")` at content-time | returns `e` (content `ENT`)      | NULL (no document retained) |

`get_entity(e)` returned NULL because candidate's parser never had a document
(`ctxt->myDoc == NULL`) in the push-SAX case, so the general entity was never
registered. DOM (`loadXML(..., LIBXML_NOENT)`) is unaffected because its SAX2
tree-builder creates `ctxt->myDoc` (whose `intSubset` the internal subset then
populates). The `xml_parser`/push-SAX path has no such build, so registration is
skipped.

## Exact root cause

`src/xml/parser/state.rs::parse_internal_subset` begins:

```rust
let dtd = unsafe {
    let doc = (*self.ctxt).myDoc;
    if doc.is_null() || (*doc).intSubset.is_null() {
        return Ok(());
    }
    (*doc).intSubset
};
```

Under a push/expat-compat SAX parse `(*self.ctxt).myDoc` is NULL (verified by gated
tracing: `[iss] doc_null=true intSubset_null=true` at internal-subset time), so the
whole internal subset is skipped — no `<!ENTITY e>` is ever scanned or registered —
and the later content reference cannot be resolved.

**Upstream does not skip this.** In `oracle/historical/src/libxml2-2.15.0/parser.c`
`xmlParseEntityDecl`, for an internal general entity declaration in a SAX parse that
has not built its own tree, the parser lazily keeps an internal registry document

```c
ctxt->myDoc = xmlNewDoc(SAX_COMPAT_MODE);       // version "SAX compatibility mode document"
ctxt->myDoc->properties = XML_DOC_INTERNAL;     // parserInternals xmlDocProperties 1<<6
if (ctxt->myDoc->intSubset == NULL)
    ctxt->myDoc->intSubset = xmlNewDtd(ctxt->myDoc, "fake", NULL, NULL);
xmlSAX2EntityDecl(ctxt, name, XML_INTERNAL_GENERAL_ENTITY, NULL, NULL, value);
```

(parser.c lines ~5498–5522; the external-entity + `replaceEntities` branch at
~5558–5580 does the same.) `xmlSAX2EntityDecl` -> `xmlAddEntity(ctxt->myDoc, extSubset,
...)` registers the entity into the internal-subset `entities` hash so that later
`xmlGetDocEntity`/`xmlSAX2GetEntity` (and PHP's compat `getEntity`) resolve it — even
though the SAX consumer never receives a tree. The instance is discarded at the end
of the front-end parse (e.g. `xmlSAXParseMemory` frees `ctxt->myDoc` after parsing,
parser.c ~12674–12677).

## Minimal faithful port (implementation plan, not yet landed)

1. In `parse_internal_subset` (or the `ENTITY` branch of the declaration scanner),
   when `(*self.ctxt).myDoc` is NULL or has no `intSubset`, lazily create the
   SAX-compat registry document on the context for internal general entities:
   - `let doc = tree::new_doc(BAD_CAST "SAX compatibility mode document");` (`xmlNewDoc`)
   - `(*doc).properties |= XML_DOC_INTERNAL;`
   - `(*doc).intSubset = dtd::create_int_subset(doc, "fake", NULL, NULL);`
   - `(*self.ctxt).myDoc = doc;`
   Then register `<!ENTITY>` via `parse_entity_decl(intSubset, args)` (which calls
   `entities::add_entity` and stores into `intSubset->entities`, exactly what
   `get_entity` reads). Mirror the upstream gating (internal general entity; and
   external parsed entities only when `replaceEntities`/`XML_PARSE_NOENT`).
2. Lifecycle: the registry doc is never delivered to a SAX consumer. It must be
   freed at front-end parse end (mirror xmlSAXParseMemory) or released where
   candidate already frees a context-owned `myDoc` (exports `xmlCtxtReset` at
   exports_parser.rs frees `myDoc`; `free_parser_ctxt` in helpers.rs currently does
   NOT free `myDoc` and must gain a `XML_DOC_INTERNAL`-guarded free so push-SAX
   contexts do not leak the registry per parse).
3. Add a rustdoc/`# SAFETY` note describing the internal doc; keep the two-plane
   model with the deliverable-tree doc owned by the caller (never internal).

## Regression scaffolds (added, source-controlled)

- `courts/suites/phase14/consumers/entprobe.c` — deterministic C reproducer:
  push parse of `<!DOCTYPE root [<!ENTITY e "ENT">]>...` with a SAX handler whose
  `getEntity` mirrors PHP's compat chain (`xmlGetPredefinedEntity` +
  `xmlGetDocEntity(ctxt->myDoc, name)`). Compiled twice (candidate `/candidate`,
  oracle `/usr/local`); candidate/oracle MUST agree on `CD[x] CD[ENT] CD[y]` ordering
  and the `post myDoc ent e=e content=ENT` lookup.
- Probe runnables (untracked working-tree php probes from the same investigation):
  `attrrefs-probe.php`, `attrws-probe.php`, `noent-{sax,sax2,dom}-probe.php` show the
  same defect from php level and the additional attribute-result gaps (entity refs
  in attributes, whitespace/attribute-value normalization) that share related
  `parse_element`/`substitute_refs` handling.

## Notes / constraints on closing

Closing this root cause correctly requires implementing the registry document
lifecycle and re-measuring the affected `ext/xml` (+ no-regression ext/dom +
ext/simplexml + ext/xsl) suites — a full local iteration each. It is a prerequisite
for several `ext/xml` failures (e.g. `xml_set_object_multiple_times{,_errors}`,
`xml004(xml00X)` whitespace/entity, the NOENT entity families), and fans into the
xmlreader/libxml_global_state upstream bytes leftover. Post-closure, the tracked
`CURRENT-STATE.md` baseline and per-family group counts must be refreshed.
