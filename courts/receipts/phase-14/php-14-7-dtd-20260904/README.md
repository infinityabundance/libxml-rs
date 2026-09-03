# Phase 14.7 — DTD subset load + parse-time validation + decl-dump parity (95 → 86)

Date: 2026-09-04 · Commit: `0e09d5e3` · Gate log: `phpbuild-c:/out/xpe-six22.log`
Diff vs xpe-six21: `OLD=95 NEW=86 FIXED=9 NEW_ONLY=0`

## Flipped (9)
- `DOMDocument_load_variation1` (DTDLOAD: entity defined by external subset)
- `DOMDocument_load_variation2` (DTDVALID: content-model warning
  "expecting (title , author), got (title author author )")
- `DOMDocument_load_variation4` (DTDATTR|NOENT|...: default attrs + entity text)
- `DOMDocument_loadXML_variation2` (bonus)
- `DOMDocument_loadXML_variation4` (doctype re-dump: no spurious #IMPLIED,
  `(title , author)` separators, title decl present)
- `DOMDocument_validate_on_parse_variation` (two parse validity warnings)
- `DOMDocument_validate_external_dtd` (dom.xml `%incent;` external PE → validate() true)
- `delayed_freeing/element_declaration` (bonus: placeholder upgrade)
- `modern/token_list/attlist` (bonus: ATTLIST default NONE vs #IMPLIED)

## Root causes / fixes
1. **External-subset load trigger** (state.rs parse_dtd): only the raw
   `XML_PARSE_DTDLOAD` option bit loaded the external DTD; upstream
   xmlSAX2ExternalSubset loads when `ctxt->validate || (loadsubset &
   ~XML_SKIP_IDS)` — so DTDVALID and DTDATTR (XML_COMPLETE_ATTRS) also load.
2. **Keyword-included decl bodies**: `load_external_dtd_file` passed the whole
   `<!ENTITY ...>` body (keyword included) into `parse_entity_decl` etc.,
   registering an entity literally named "ENTITY". Shared keyword splitter
   added; external ATTLIST decls also feed `collect_attlist_defaults` so
   start-tag defaulting applies.
3. **resolve_dtd_path** now falls back to `ctxt->directory` (php sets it for
   loadXML) since doc->URL is assigned only after the parse.
4. **Parse-time per-element DTD validation** (sax_end_element →
   `validate_end_element`): mirrors xmlSAX2EndElementNs/xmlValidateOneElement
   across BOTH subsets; UNDEFINED/EMPTY/ANY/MIXED/ELEMENT content checks with
   upstream text; errors raised at the end-tag position (php renders
   "… in <file>, line: N" of the end tag, like oracle line 8).
5. **add_element_decl placeholder upgrade** (dtd/mod.rs): an UNDEFINED
   placeholder created by an ATTLIST for an undeclared element is removed +
   freed with its attributes carried to the real declaration (upstream
   valid.c). Fixes dump completeness + validation of ATTLIST-before-ELEMENT
   docs.
6. **Parameter entities**: internal-subset / external-subset / PE bodies all
   share `process_dtd_fragment`; decl-level `%name;` refs expand in place
   (external PEs fetch the file); `%name;` inside decl args expands outside
   quoted literals, recursion depth-capped at 10.
7. **DTD dumpers**: element content models emit `" , "`/`" | "` when
   ascending c1→c2 (plain-element sequences had lost separators →
   `(titleauthor)`); `parse_attr_default` returns `XML_ATTRIBUTE_NONE` for a
   bare quoted default (was #IMPLIED → dumper printed
   `CDATA #IMPLIED "default title"`).

## Validation
- cargo test --lib 1241 pass / 1 ignored; fmt clean.
- Targeted runs (PHP_TESTS_LIST single-shot): all 6 planned tests PASS first.
- Full six-extension gate xpe-six22.log: 86 failed (was 95), zero new names.

## Follow-ups (impact order)
- dom: adoption/DOMNode families still ~53; xmlreader 16 (reader now benefits
  from ext-subset decl registration for 007/008/013); xsl 16.
