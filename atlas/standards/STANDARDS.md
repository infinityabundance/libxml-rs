# Standards Atlas

Per §12, this maps implementation surfaces against the standards they
implement, and records the *relationship* between upstream behavior and the
standard. Compatibility follows the oracle where safe and intentional; the
atlas explains the relationship to the standard.

Classification labels (§12):
- `STANDARD_CONFORMING` — behavior matches the standard
- `UPSTREAM_EXTENSION` — upstream behavior beyond the standard
- `UPSTREAM_QUIRK` — upstream behavior that deviates without being a bug
- `HISTORICAL_BUG` — upstream behavior that is (or was) a bug
- `VERSION_SPECIFIC` — behavior varies by version
- `PLATFORM_SPECIFIC` — behavior varies by platform
- `UNRESOLVED` — not yet determined

This is a living document. Classification entries are provisional until the
corresponding court case runs (§97 evidence-driven loop).

---

## XML

### XML 1.0 — `W3C-XML-1.0`
- Surface: parser (well-formedness), char production, names, entity handling,
  CDATA, comments, PIs, DOCTYPE, attribute normalization.
- libxml2 status: largely `STANDARD_CONFORMING` with known recovery-mode
  extensions and historical leniencies.
- Notes: XML 1.0 Fifth Edition; libxml2 predates it and carries legacy
  leniency. §19 parser courts.
- Unresolved items: exact conformance boundary in recovery mode.

### Namespaces — `W3C-XML-NS-1.0` / `W3C-XML-NS-1.1`
- Surface: `namespaces.c`, parser ns handling, `xmlNs` topology.
- Notes: Namespaces in XML 1.1 introduced prefix undeclaration
  (`xmlns:p=""`); libxml2's handling is a known `VERSION_SPECIFIC`/quirk
  area. §23 namespace courts.

## XPath

### XPath 1.0 — `W3C-XPATH-1.0`
- Surface: `xpath.c`, `xpathInternals.h`.
- This is the most strict parity surface. §25.
- Known quirks to investigate: NaN/signed-zero, string→number conversion,
  node-set ordering, `substring` rounding, `format-number` divergence
  (libxslt), position/size edge cases.

## XSLT

### XSLT 1.0 — `W3C-XSLT-1.0`
- Surface: `src/xslt/*`.
- libxslt implements XSLT 1.0 plus extensions. §32/§33 courts.
- Known quirks: template priority, import precedence, RTF semantics,
  extension-element fallback, whitespace stripping defaults.

## XInclude / XPointer / URI / Catalogs

### XInclude 1.0 — `W3C-XINCLUDE-1.0`
- Surface: `xinclude.c`. §26 courts.

### XPointer 1.0 — `W3C-XPTR-1.0`
- Surface: `xpointer.c`. §26 courts.

### URI — `RFC3986` / `RFC3987`
- Surface: `uri.c`.
- libxml2's URI parser is its own implementation, NOT a general URI library.
  Parity of malformed-URI handling is `UPSTREAM_QUIRK` territory.

### XML Catalogs — `OASIS-CATALOG-1.0`
- Surface: `catalog.c`, `xmlcatalog` tool.
- Catalog lookup order and precedence are `UPSTREAM_QUIRK`; §26 courts.

## Validation

### XML Schema — `W3C-XSD-1.0`
- Surface: `xmlschemas.c`, `xmlschemastypes.c`.
- libxml2's schema support is `UPSTREAM_EXTENSION`-lite: it is widely known to
  deviate from the standard in places. §27 courts.

### RELAX NG — `W3C-RELAXNG`
- Surface: `relaxng.c`.
- Known to be incomplete/non-conformant in places. §27 courts.

### Schematron — `ISO-SCHEMATRON`
- Surface: `schematron.c`.
- libxml2 implements a subset (Schematron 1.x style). §27 courts.

## Canonicalization

### Canonical XML 1.0 — `W3C-C14N-1.0`
### Exclusive Canonical XML — `W3C-C14N-EXCL`
- Surface: `c14n.c`.
- Must be byte-exact. §28 courts.

## HTML

### HTML (historical libxml2 HTML subset) — `WHATWG-HTML` (NOT what libxml2 does)
- Surface: `HTMLparser.c`, `HTMLtree.c`.
- **Critical**: libxml2's HTML parser is a historical tag-recovery parser,
  NOT a WHATWG HTML5 parser. The atlas must preserve version-specific
  historical behavior. §29 courts.
- Relationship to standard: `UPSTREAM_QUIRK` / `HISTORICAL_BUG` (by modern
  standards); compatibility follows the oracle.

## Encodings

### Character encoding — `UNICODE-*`, IANA charset registry
- Surface: `encoding.c`, `xmlIO.c`.
- Distinguish libxml2 behavior from iconv/converter behavior (§22).

## EXSLT

### EXSLT modules — `EXSLT-COMMON/MATH/SETS/STRINGS/DYNAMIC/DATES`
- Surface: `src/exslt/*`.
- libxslt ships these; §35 inventory.
- Note: EXSLT dates (`date:`) is itself known to have quirks in libxslt.

---

## Registry of upstream-vs-standard divergences to characterize

Each row is a candidate `UPSTREAM_QUIRK`/`HISTORICAL_BUG` that must be
differentially tested and classified before parity can be claimed.

| # | Surface | Standard | Suspected divergence | Status |
|---|---|---|---|---|
| S-0001 | XPath number→string | W3C-XPATH-1.0 | Negative zero / NaN formatting | UNRESOLVED |
| S-0002 | XPath substring | W3C-XPATH-1.0 | Rounding of position | UNRESOLVED |
| S-0003 | XPath node-set order | W3C-XPATH-1.0 | Document-order guarantee | UNRESOLVED |
| S-0004 | XML parser recovery | W3C-XML-1.0 | Lenient recovery mode | UNRESOLVED |
| S-0005 | Namespace undeclaration | W3C-XML-NS-1.1 | Prefix undeclaration handling | UNRESOLVED |
| S-0006 | HTML parser | WHATWG-HTML | Historical tag-recovery subset | UNRESOLVED |
| S-0007 | XML Schema | W3C-XSD-1.0 | Known non-conformances | UNRESOLVED |
| S-0008 | RELAX NG | W3C-RELAXNG | Known non-conformances | UNRESOLVED |
| S-0009 | XSLT number/format | W3C-XSLT-1.0 | format-number vs standard | UNRESOLVED |
| S-0010 | URI malformed handling | RFC3986 | lenient URI parser | UNRESOLVED |
| S-0011 | Catalog precedence | OASIS-CATALOG-1.0 | lookup order quirks | UNRESOLVED |
| S-0012 | XPath equality | W3C-XPATH-1.0 | node-set vs node-set comparison | UNRESOLVED |
