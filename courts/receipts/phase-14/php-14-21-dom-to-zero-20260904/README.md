# Phase 14.21 — dom family to ZERO: 10 -> 4 (candidate-driven 9 -> 2)

Gate: xpe-six38.log = 4 failures (xsl 2 | xmlreader 1 | xmlwriter 1).
NEW_ONLY empty vs six37. cargo fmt clean, cargo test --lib 1241 pass.

ALL SIX remaining dom tests flipped (dom 6 -> 0), each oracle-verified:

1. serialize_non_default_empty_xmlns — xmlParseStartTag2 parity: xmlns URI
   values are validated with xmlParse3986URIReference (the check behind
   xmlParseURISafe) BEFORE the not-absolute heuristic; parse failure warns
   XML_WAR_NS_URI (99) "xmlns:%s: '%s' is not a valid URI" / "xmlns: '%s'"
   for the default form. uri_reference_valid (exports_uri.rs) exposed
   pub(crate); the pedantic-only relative warning is preserved for URIs that
   parse (src/xml/parser/state.rs).

2. DTDNamedNodeMap — xmlSAX2EntityDecl parity: an external entity's `URI` is
   the SystemID resolved with xmlBuildURISafe against the newest input
   filename or ctxt->directory (php sets CWD for memory loads); xmlNodeGetBase
   on an ENTITY_DECL returns ent->URI, so "<dir>/mypicture.gif". parse_entity_decl
   is now a self-method and resolves the URI after add_entity (state.rs).

3. bug80268_2 — HTML NUL bytes are DROPPED in the text-content scan and
   parsing continues (libxml2 >= 2.9.12); the scan previously terminated the
   text node at the NUL (C-string storage truncation) losing the tail
   (src/xml/html/mod.rs).

4. DOMDocument_loadHTMLfile_error1 — htmlCreateFileParserCtxt now allocates
   the host FIRST and, when the main-document load fails, raises the
   xmlCtxtErrIO warning "failed to load \"%s\": %s" through the parser channel
   (reuses exports_parser::{call_loader_materialize, io_load_failure_message,
   emit_io_warning}) before returning NULL — php formats it as the
   loadHTMLFile() warning (src/abi/exports_html.rs).

5. DOMNode_isEqualNode (SEGV) — copy_doc mirrored tree.c xmlCopyDoc +
   xmlStaticCopyNodeList DTD semantics: ONE DTD copy serves both the new
   doc->intSubset and the DocumentType child at its position in the children
   list; the generic node copy (which only duplicates `name` and leaves the
   DTD's ExternalID/SystemID offsets untouched) is never applied to a DTD
   child. Clones of doctype documents were corrupt (php doctype read /
   saveXML SEGVs) (src/xml/tree/mod.rs).

6. gh12616_3 — namespace axis ancestor walk stopped reading nsDef on the
   document node: that offset aliases doc->oldNs, where php's ns elimination
   (dom_eliminate_ns) parks removed declarations (freed href/prefix) — the
   axis surfaced them as ghost `xmlns`/NULL namespace nodes after
   removeAttributeNS (src/xml/xpath/axes.rs).

Remaining candidate-driven: xsl 2 (gh21357_2, xinclude/xinclude) + xmlreader
1 (fromStream_broken_stream); xmlwriter shiftjis is oracle-failing parity
(.exp unsatisfiable; fails on the oracle too).

Probes: consumers/{nul,htmlfile,missing,clone-dtd,childdump,nsaxis,nsaxis-trace,
lineno-chain,clone-dtd}-probe.php / copydoc-dtd.c.
