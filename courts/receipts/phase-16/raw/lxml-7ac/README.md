# lxml court — 7ac (commit `53b987dd`)

Status: **101 unique failing ids** (80 failures / 26 errors = 106 raw) in
`python3 test.py -u -v` (2007 tests), down from 109 unique at `883f4295`.
PHP six-gate: **1250 passed / 0 failed**. `cargo test --lib`: 1325 passed /
0 failed / 3 ignored. `cargo fmt --check` clean. No regressions.

Artifacts: `run.txt`, `suite-summary.txt`, `failing-ids.txt`,
`suite-fixed.txt` (26 ids), `suite-regressions.txt` (0).

## Root causes fixed

1. **External DTD subset bypassed the consumer resolver.** The DTD loader read
   files with `std::fs::read` and never consulted the registered external
   entity loader, so lxml's `etree.Resolver` (installed through
   `xmlSetExternalEntityLoader`) was ignored. `load_external_dtd_file` now
   builds the absolute URI with `xmlBuildURI(systemId, input->filename)` (like
   `xmlSAX2ResolveEntity`) and fetches through `sax->resolveEntity` first, else
   `xmlLoadExternalEntity`. `ctxt_read_doc` records the caller's URL as the
   input filename, so relative system ids resolve against the document base.
   Fixes the ten `test_resolve_*` cases and `test_dtd_parse_valid_relative*`.

2. **`xmlNewInputFromFile` was a type-confusion stub** that cast a
   `_xmlParserInputBuffer*` to `_xmlParserInput*`. lxml's `resolve_filename`
   loader crashed on the result. It now opens through the create-filename path
   and builds a real input (filename owned by the input).

3. **`file:` URIs were not converted to local paths.** `input_buffer_create_file`
   now applies `xmlIO.c xmlConvertUriToPath` (prefix handling + percent
   unescape), so `resolve_filename(file:///…)` opens the file.

4. **XInclude did not use the consumer resolver.** Included documents are now
   fetched through the external entity loader with a parser context whose
   `_private` is the xinclude context's `_private` (xinclude.c
   `xmlXIncludeParseFile`), and the document's parse options are applied plus
   `XML_PARSE_DTDLOAD`. Fixes the four XInclude resolver cases.

Also included: the PI separating-space and UTF-16 output BOM fixes (commit
`883f4295`, 10 ids) and the HTML dictionary/`userData`/copy fixes (commit
`e3c80a49`).

## Deferred residual (documented, not silently dropped)

`xmlStaticCopyNode`/`xmlCopyPropInternal` should intern copied element and
attribute names into `doc->dict` (upstream `xmlDictLookup(doc->dict, …)`). This
is required for lxml's address-based matching (`iterchildren(tag)`,
ElementPath predicates), i.e. `test_elementpath.test_find` and the four
objectify `test_object_path_*` cases (8 ids).

Enabling the interning fixes those 8 but makes PHP's modern-DOM clone/adopt
(`Dom\HTMLDocument::adoptNode($doc->cloneNode(true))` on a namespaced element)
double-free at document teardown. Root cause: `new_text`/`new_comment` build a
heap-duplicated `"text"`/`"comment"` name instead of the shared
`xmlStringText`/`xmlStringComment` static markers, so a copied text node's name
gets interned into the dictionary and later freed by a path that does not
consult the dictionary (verified with a `free()`-tracking gdb session on the
`Dom\XMLDocument`/`Dom\HTMLDocument` repro). The correct fix is to make
`new_text`/`new_comment`/`new_cdata_block` use the static markers (which several
direct name-free sites in `exports_treedump.rs`/`exports_tree.rs` must also
learn to respect) and then re-enable the interning. `is_static_node_name` is
already in place as the shared predicate.
