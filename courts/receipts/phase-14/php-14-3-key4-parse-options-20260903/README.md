# KEY-4 part 1 — RECOVER diagnostics + NO_XXE external-entity gating + DTD dump order

Closed 2026-09-03. Full suite **250 → 241**, zero regressions. dom 159 → 152,
simplexml 9 → 7; xml 0 / xmlreader 29 / xmlwriter 1 (W5) / xsl 52 unchanged.
Log: phpbuild-c:/out/k4-six.log (1291-run, 241 failed / 40 skipped). Fixed:
`xml_parsing_LIBXML_RECOVER` + `xml_parsing_LIBXML_NO_XXE` (dom + simplexml),
and dom `modern/spec/{Document_implementation_createDocument,
Document_implementation_createDocumentType, clone_document,
pre_insertion_validation}` + `modern/common/gh21077`.

## RC-1 · Recovery must raise the premature-EOF diagnostic (mirrors parser.c)

The `XmlToken::Eof` arm of parse_element closed the open element SILENTLY
under `XML_PARSE_RECOVER` — no error record, no generic-error delivery, so PHP
never saw the warning block that the non-recover path produces byte-identically
(php relays the engine's generic channel lines). Upstream raises
`XML_ERR_TAG_NOT_FINISHED` (77) in BOTH modes — recovery only decides whether
parsing continues. The raise now fires before the recovery `break`, still
skipped when a prior fatal already cleared `wellFormed`.

## RC-2 · NO_XXE must block external general-entity substitution (mirrors parser.c)

`xmlParseReference`'s parse/substitution phase ran for EXTERNAL general parsed
entities even under `XML_PARSE_NO_XXE`: the entity loader was invoked for
`&xxe;` (SYSTEM "file:///etc/passwd") → spurious
`I/O warning : failed to load …` on every NO_XXE parse. Upstream gates the
phase on `(etype == XML_INTERNAL_GENERAL_ENTITY) || (!NO_XXE &&
(replaceEntities || validate))`. The candidate's NOENT branch now skips the
load+substitution for external entities under NO_XXE — the reference expands
to nothing, internal entities still substitute.

## RC-3 · DTD serialization: children list order + no doctype duplication
(mirrors xmlsave.c xmlDtdDumpOutput + xmlSaveDocInternal)

Exposed by the dom NO_XXE member (its exact `--EXPECT--`):
- **Order**: internal-subset declarations were dumped from the DTD HASH tables
  (`hash_scan`), i.e. hash-bucket order — reversed multi-declaration output
  (tracked RESIDUAL R-DTD-DUMP-ORDER). Upstream walks the DTD node's CHILDREN
  list, where every declaration node sits in declaration order (notations are
  hash-only and stay scanned first). `dtd_dump_output` now walks
  `dtd->children`, dispatching on XML_ELEMENT_DECL / XML_ATTRIBUTE_DECL /
  XML_ENTITY_DECL (parameter entities are type-17 nodes with a param etype).
- **Duplication**: `doc_content_dump_output`'s intSubset-explicit dump fired
  whenever the intSubset was not found among `doc->children` — including when
  `doc->children` was NULL. PHP's modern serializer temporarily NULLs
  `doc->children` around `xmlSaveDoc` (declaration-only pass) and re-dumps the
  children itself, so the extra dump produced a duplicated `<!DOCTYPE>`. The
  explicit dump now requires a NON-EMPTY children chain (still covering the
  xmlCopyDoc / lazily-created-subset construction paths that keep the DTD off
  the chain).

## Guards (src/xml/parser/tests.rs)

- `test_recover_raises_premature_eof_tag_not_finished` — errNo 77 + wf=0 in
  both modes; RECOVER still returns the recovered tree.
- `test_no_xxe_blocks_external_entity_and_doctype_serializes_once_in_order` —
  NOENT|NO_XXE parse serialized to the exact oracle bytes: ONE `<!DOCTYPE`,
  `foo` before `xxe`, `&xxe;` gone, `<set><foo>bar</foo></set>`.

cargo test --lib 1229 pass / 1 ignored; clippy no new warnings (3
pre-existing); fmt clean.

## Probes kept (consumers/)

- recover-probe.c — xmlReadMemory+RECOVER engine pin (error 77 + context).
- savedtd-probe.c — xmlSaveDoc / xmlSaveTree(dtd) engine pin.
- dtd-children.c — DTD child-node structure pin (declaration order +
  intSubset == first child on both engines).
- recover-sxe.php — php simplexml warning-block pin.

Residual in this family (plan-dom L1, not part of this atom): dom
`DOMDocument_loadHTMLfile*`, `bug80268_2`, simplexml S8 (`bug79971_1` NUL
loader warnings) and reader VD — the KEY-4 option net continues there.
