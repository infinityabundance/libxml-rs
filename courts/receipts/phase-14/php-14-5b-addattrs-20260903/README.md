# php-14.5b XSD default/fixed attribute creation (LIBXML_SCHEMA_CREATE)

Date: 2026-09-03
Full six-extension gate: **102 → 100 failed**, ZERO regressions (name-level
diff vs the 102-baseline `phpbuild-c:/out/xpe-six16.log`: 0 new). Log:
`phpbuild-c:/out/xpe-six17.log` (1291 / 100 failed / 40 skipped).

## Root cause
schemaValidate/schemaValidateSource with the LIBXML_SCHEMA_CREATE flag
(XML_SCHEMA_VAL_VC_I_CREATE) must inject a missing attribute's schema
default/fixed value into the instance before its value is validated
(upstream xmlSchemaValidateDoc / xmlSchemaValidateAttributes). The engine
dropped `default`/`fixed` at parse time and ignored the valid-ctxt options.

## Fixes
- `XsdComponent` now carries `default_value`/`fixed_value` (captured by
  xsd_parse_element and xsd_parse_attribute_node).
- `xmlSchemaValidateDoc` reads the registered valid-ctxt options
  (`exports_schema::valid_ctxt_options`) and sets `XsdValidCtxt.create_defaults`.
- `xsd_validate_complex_type` calls `inject_default_attribute` for each
  attribute declaration under the create option: unqualified declarations
  with a default/fixed value create the attribute on the instance element
  (xmlSetProp) when it is absent.
Flipped: `DOMDocument_schemaValidate_addAttrs`, `schemaValidateSource_addAttrs`
(first book gains `is-hardback="false"` from book.xsd).

## Guards
- New unit test `test_xml_schema_validate_creates_default_attrs`: validation
  without the option leaves the instance untouched; with
  XML_SCHEMA_VAL_VC_I_CREATE the default attribute is created and the document
  validates. cargo test --lib 1241 passed (+1 ignored, pre-existing); clippy
  baseline; fmt clean.
- Targeted php re-runs: both addAttrs tests PASS.

## Next residuals
dom 66: XPath namespace-axis/DOMNameSpaceNode family (incl the
xpath_domnamespacenode_advanced segfault), DTD validity net
(validate_external_dtd, validate_on_parse_variation, xmlreader 008), the
load/loadXML_variation + DTD entity-default family, reader schema-attach
flows (007 include-resolution, 013), reader cursor/props family; xsl 16;
xmlreader 16.
