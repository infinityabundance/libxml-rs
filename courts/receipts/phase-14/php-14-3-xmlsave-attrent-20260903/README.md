# php-14.3 xmlsave 0-length write + attribute entity-ref children (2026-09-03)

Full suite **186 → 158 failed**, ZERO regressions (name-level diff vs the
186 baseline `phpbuild-c:/out/xpe-six10.log`: 0 new). Log:
`phpbuild-c:/out/xpe-six11.log`. dom 117 → 89 (28 flipped: the php-8.5
`Dom\` modern serialization surface + gh19612 + dom001 + inner/outerHTML).

## Root cause 1 — `xmlOutputBufferWrite(out, 0, ptr)` returned -1 (~20 tests)
php 8.5's W3C DOM-Parsing serializer (ext/dom/xml_serializer.c
`dom_xml_common_text_serialization`) writes text in chunks and issues
`xmlOutputBufferWrite(out, 0, p)` whenever a text/attribute run STARTS with
a character needing escaping (`&`, `<`, `>` …). The candidate's export AND
`io::output_buffer_write` rejected `len <= 0` with -1, so any such node
aborted the whole save → `Dom\XMLDocument::saveXml(): Could not save
document` across the modern family (css_selectors, title setter, noscript,
insertAdjacentHTML, serialize_* …). Upstream xmlIO.c returns 0 for a
zero-length write. Both layers now accept `len == 0` (only `len < 0` is an
error). Probe: `consumers/save-iso.php` (12 escape-shapes, candidate now
oracle-identical).

## Root cause 2 — declared-entity references in ATTRIBUTE values (~8 tests)
Parsed `<el x="&foo;bar&foo;"/>` (with `<!ENTITY foo "FOO">`) stored ONE
flat text node `&foo;bar&foo;`; upstream keeps the general references as
`XML_ENTITY_REF_NODE` children of the attribute (tree.c
`xmlNodeParseAttValue`; xmlSAX2AttributeNs runs it for every
reference-bearing dup'd value). Every serializer (tree dump
`attr_dump_output`, the xmlsave path, AND php's own spec serializer which
writes ENTREF children as `&name;`) already handled ENTREF-in-attr — only
the parse side was missing. `default.rs parser_build_attr_children` now
walks reference-bearing values: text runs → text nodes, surviving general
`&name;` refs (character/predefined refs were already substituted by the
tokenizer, so only general refs remain) → ENTITY_REF children with the
declared entity attached (children/content), stray `&` (resolved text from
`&amp;`, or `&#...`-looking runs) stays text. Values without surviving
refs keep the single non-compact text node (R-000120). NOENT docs are
unaffected (fully substituted → single text). Probe:
`consumers/attr-entityref-probe.c` — children lists + dumps oracle-identical.
Tracked corner: double-encoded `x="&amp;foo;"` becomes ENTREF foo (oracle:
TEXT `&foo;`) — the substituted value cannot distinguish it; no test pins
it.

Also flipped by these: `gh19612` (xmlsave attr entity-refs), modern
`serialize_entity_reference_in_attribute`, `xml_serialize_formatting`,
`Element_innerHTML{,_writing}`, `serialize_empty_xmlns`,
`dom_parsing_gh47(_bis)`, `xml/gh18979`, `gh21688`, dom001, noscript, the
css_selectors pseudo-class suite, token_list remove/toggle, and more.

## Probes kept (consumers/, candidate == oracle)
- `save-iso.php` — php-save of every escape shape.
- `attr-entityref-probe.c` — declared-entity attr children + round-trip dump.

## Residuals next (dom 89)
namespace-prefix fresh allocation (KEY-5: createAttributeNS_prefix_conflicts
×5, Element_setAttributeNS, attribute_renaming_conflict…), schemaValidate/
relaxNGValidate/validate (SP-14.3.4 ~14), DTD-serializer corners
(token_list/attlist `#IMPLIED "…"`), delayed_freeing ×4, css_selectors/
entities, xsl 52, xmlreader 15.

Validation: cargo test --lib 1239 pass, clippy at the 4 pre-existing
warnings, fmt clean. Full six-gate `phpbuild-c:/out/xpe-six11.log`:
1291 / **158 failed** / 40 skipped.
