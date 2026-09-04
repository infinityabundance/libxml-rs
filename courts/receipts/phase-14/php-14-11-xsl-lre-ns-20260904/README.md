# php-14.11 — xslt literal-result-element in-scope ns + xpath char semantics (2026-09-04)

Full six-gate **57 → 54 failed**, ZERO regressions (name-level diff vs the 57
baseline `phpbuild-c:/out/xpe-six25.log`: **NEW_ONLY=0, FIXED=3**). Log:
`phpbuild-c:/out/xpe-six26.log`. dom 31 | xmlreader 8 | xsl 12 |
simplexml 1 | xmlwriter 1. Commit `b561d56c`.

## Root cause 1 — XPath string functions sliced bytes, not characters (~1-2 tests)
`fn_substring`, `fn_string_length`, `fn_translate` indexed XPath strings by
BYTE with 1-based rounding. XPath string functions are character
(Unicode code point) indexed. `fn_substring` hit a non-char-boundary byte
slice on a Cyrillic value used in an `xsl:key` match -> Rust slice panic
(**bug26384**). FIX: all three now operate over `.chars()`.

## Root cause 2 — literal result elements never copied in-scope ns decls (~3 tests)
`process_literal_element` created result copies with `new_node((*inst).ns, …)`
but never copied the stylesheet's in-scope namespace declarations onto the
result, so prefixed literal result elements and output under an XSLT-ns
inheritance lost `xmlns:php="http://php.net/xsl"` (bug70078,
xsltprocessor_transformToXML — mangled output). Also, an element in NO ns
under a default-ns result parent was silently placed in that default ns.

Oracle probes pinned libxslt's exact rules (probes committed in
courts/suites/phase14/consumers/xslt-lre-ns-probe{,2,3}.php):
- An LRE directly under xsl:template ALWAYS emits its effective in-scope
  decls (`<br/>` -> `<br xmlns:php="…"/>` when php is in scope).
- Inside xsl:for-each/if/choose/when/otherwise the effective-ns set is
  suppressed (compile-time `effectiveNs` only filled outside instruction
  bodies): `<xsl:for-each>..<br/>` emits NO decl. Literal-result ancestors do
  not reset suppression.
- An element's OWN namespace binding is still materialised inside those
  bodies (`<php:x/>` inside xsl:for-each DOES emit xmlns:php).
- xsl:variable between template start and the LRE does not suppress.
- exclusion honors exclude-result-prefixes on the LRE AND the stylesheet
  root; `#default` excludes the default ns; xml prefix and the XSLT
  namespace are never copied; decls already in scope at the result insert
  point are not duplicated; `xmlns=""` undeclares a non-empty default.

New code in src/xslt/transform/mod.rs: `copy_literal_result_ns`,
`read_excluded_prefixes`, `find_stylesheet_root`, `lre_decls_suppressed`.
All probe cases byte-identical with `phporacle-c`.

## Flipped (full gate)
bug26384, bug70078, xsltprocessor_transformToXML. Isolated ext/xsl gate also
fixed bug69168 (the pre-fix regression `<br xmlns:php>` inside for-each):
14 -> 13.

## Guards / validation
- cargo test --lib 1241 pass / 1 ignored; fmt clean.
- Probe byte-parity vs oracle on all six LRE-ns cases.

## Residuals next (54)
dom 31: memory-abort family (DOMDocument_adoptNode, DOMElement_
insertAdjacentElement, DOMElement_replaceChildren,
DOMDocument_saveXML_XML_SAVE_NO_DECL, bug79968,
DOMDocument_getElementsByTagName_liveness_xinclude, gh14702,
HTMLDocument_serialize_ns_imported_05), entity/notation family, C14N pair,
textContent/html serialization pair, xmlreader 8, xsl 12, simplexml 1,
xmlwriter 1.
