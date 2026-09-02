# Phase 14.3 — subphase closure plan (to-zero)

Authoritative re-baseline (2026-09-02, candidate DSO set @ ff536c32):
**1291 tests / 306 failed / 40 skipped** (skips == oracle). Failing universe
extracted from `/out/subphase-baseline.log` FAILED TEST SUMMARY.

Method for every root cause: mirror upstream (oracle `oracle/historical/
src/libxml2-2.15.0`) at the Rust engine layer; add a regression guard / probe;
re-run the affected PHP extension subset to confirm the targeted tests flip to
PASS with zero sibling regressions; run the FULL six-extension suite at each
subphase gate to confirm net failure count drops and no regression; commit +
push each closure; keep `CURRENT-STATE.md` current.

## Subphases (ordered; gate between each = full-suite re-measure)

- **SP-14.3.1 ext/xml** (21 fails; measured clusters below). SAX2/expat-compat
  layer. Current re-measure (ext/xml alone, 2026-09-02): 11 skipped; 21 failed;
  famsig groups:
  --- SP14.3.1-1  SAX DTD-decl callbacks absent  -> drives
       xml_set_notation_decl_handler_basic (notation_decl_handler +
       unparsed_entity_decl_handler events never fire at all; candidate drops the
       whole expected block). Root: internal-subset scan registers NOTATION /
       NDATA ENTITY but never dispatches SAX notationDecl/unparsedEntityDecl.
       (Note: unparsed_entity NDATA metadata itself was fixed in Fix-6.)
       FIX DESIGN: parse_internal_subset NOTATION branch after
         Self::parse_notation_decl(d,args) and the NDATA ENTITY branch after
         Self::parse_entity_decl(d,args) must SaxDispatcher::notation_decl(sax,
         ctx,name,pub,sys) and ::unparsed_entity_decl(sax,ctx,name,pub,sys,
         notation), each only when the SAX handler slot is present and only when
         not in the external-subset (inSubset) case that the SAX2 default fires
         from xmlParseNotationDecl/xmlParseEntityDecl. Parse name + optional
         SYSTEM/PUBLIC ids from `args`; for unparsed reuse find_ndata_notation.
         Guard phpt: ext/xml/tests/xml_set_notation_decl_handler_basic.phpt.
  --- SP14.3.1-2  [MEASURED 2026] xml004/xml_closures_001 lose <elem4> and all
       following content after an external-parameter-loaded entity reference
       ("&included-entity;" from inc.ent). NOT chunk/push-specific: the
       xml004-probe shows elem4 dropped in a single xml_parse call too; DOM
       load also can't resolve it without DTDLOAD (external param entity). The
       candidate SAX path drops the trailing element inside the element where
       the external/parameter entity content was expanded (element-scope resume
       after entity push_input). xml004-probe.php is the reproducer.
  --- SP14.3.1-3  structure/namespace tag mangling (bug50576: [://WWW.FPDSNG...]
       - empty-uri default attribute/namespace in xml_parse_into_struct).
  --- SP14.3.1-4  xml_parse_into_struct empty/False/misval (bug35447 character-
       entity attribute recode inside structure attrs; bug71592 empty result;
       bug25666/bug72714/bug73135 crashes/empty).
  --- SP14.3.1-5  end-element locator col/byte (bug26614_libxml_gte2_11).
  --- SP14.3.1-6  PARSE_HUGE option semantics (XML_OPTION_PARSE_HUGE).
  --- SP14.3.1-7  recursion-on-callback guard (gh12254).
  --- SP14.3.1-8  crash cluster gh20439_1/2 + bug27908 (structure/push finalize).
  --- SP14.3.1-9  residual: xml_set_object_multiple_times{,_errors} and
       xml_error_string_basic_libxml (error-code/string table + double handler
       set).
- **SP-14.3.2 ext/simplexml** (9 fails). Engine-backed: `LIBXML_RECOVER` /
  `LIBXML_NO_XXE`, doc/encoding parity, serialization.
- **SP-14.3.3 ext/dom XML engine** (the large dom bucket): serialize /
  save / DOCTYPE entity (mostly done in Fix-5), namespace + prefix
  reconciliation + default-ns, import/append/replace/textContent + innerHTML,
  DOMNode isEqualNode, C14N-references, XMLDocument-from-* / encoding,
  option semantics (`NO_XML_DECL`, `preserveWhiteSpace`, `PARSE_HUGE`).
- **SP-14.3.4 ext/dom validation**: schemaValidate* add-attrs/error message,
  relaxNG error parity, validate external DTD / on-parse.
- **SP-14.3.5 ext/xmlreader** (30): 001-015 stream/read APIs, custom
  constructors, expand, readString, gh16292/gh19098 + php_libxml_node_free.
- **SP-14.3.6 ext/xmlwriter** (19): OO syntax/attribute-ns, indentation, flush,
  shift_jis output, error parity.
- **SP-14.3.7 ext/xsl** (~60): php:function/registerPHPFunctions routing,
  setParameter/getParameter/removeParameter validation, transformTo* APIs +
  doc/result lifetimes, XSLT engine edges (bug26384/bug49634 crash-class),
  namespace_mapper + callables + auto-registration.
- **SP-14.3.8 closure sweep** (any residual crash-class + mix) -> then
  **14.3-Q** binary substitution, **14.3-S** ZTS, **14.3-T** full gate.

## Current atom
- Resolved down from baseline 308 -> 306 via Fix-5/5b layers + Fix-6 (NDATA
  entity metadata). Current in-family residual targeted in SP-14.3.3 (dom):
  DOMEntity_fields empty public id (PUBLIC "") ExternalID="" collapses to NULL.
