# libxml2 / libxslt History Atlas

Provenance: all dates and identities below derive from `SRC-LIBXML2-GIT` and
`SRC-LIBXSLT-GIT` (see `atlas/SOURCES.md`). Semantic-change claims are anchored
to `NEWS-*` entries and upstream commits where stated.

Confidence: `known` unless otherwise marked `inferred`.

---

## 1. libxml2

### 1.1 Origins: the W3C XML library (1998)

- First commit: `01791d57` "Added the XML code developped at W3C, Daniel."
  1998-07-24, by Daniel Veillard. (`known`)
- The initial code was developed at the W3C for the W3C/INRIA "Amaya" and
  related projects, then imported into GNOME CVS in July 1998. (`inferred` from
  commit message and known project history)
- Project name at this stage: **libxml** (later renamed **libxml2** when the
  2.x ABI was introduced).

### 1.2 The 0.x / 1.x era (1998–2000): `libxml`

- Tag `LIBXML_0_99` (1998-12-07) — the "0.99" GNOME-era release.
- 1.x tags recovered: `LIB_XML_1_1` (1999-06-02) through
  `LIBXML_1_8_17` (2002-01-23). Note the tag-spelling inconsistency
  (`LIB_XML_` vs `LIBXML_`) and the `LIBXML_1_8_10_REAL` re-tag
  (packaging mistake corrected 2000-09-06).
- During this era the library was a GNOME dependency and exported the
  `libxml.so.1` SONAME. (`inferred`; to be confirmed by ABI snapshot of
  a 1.x tag)
- Epoch label: **legacy parser epoch** (see `compatibility/profiles.rs`).

### 1.3 The 2.0 ABI break (2000): `libxml2`

- Tag `LIBXML_2_0_0` (2000-04-12) — project renamed to libxml2, new ABI.
- SONAME changed to `libxml2.so.2`. (`known`; preserved through the present)
- Tag `LIBXML_2_1_0` (2000-06-29): "2.1.0 test release".
- The `LIBXML_2_x_y` tag scheme continues through ~2.4/2.5, then transitions
  to `LIBXML2_<maj>_<min>_<mic>` (e.g. `LIBXML2_2_5_7`), then
  `LIBXML2.<min>.<mic>` (e.g. `LIBXML2.6.31` == 2.6.31), then the modern
  `vX.Y.Z` scheme from ~2.7/2.8 onward. The tag-scheme transitions themselves
  are part of the historical record and are captured in the release manifest.

### 1.4 SAX2 and the namespace era (2002–2003)

- libxml2 2.4.21 (2002-04-29) ... 2.5.0 (2003-01-06).
- SAX2 (`SAX2.c`) with namespace-aware callbacks became the default parser
  path in the 2.5 era. (`inferred`; to be confirmed by source archaeology of
  `SAX2.c` history)

### 1.5 Validation-era expansion (2003–2004): 2.6.x

- 2.6.0 (2003-10-21) ... 2.6.32 (2008-04-08).
- The 2.6 series added/solidified XML Schemas, RELAX NG, Schematron, the
  push parser, and the reader API. (`inferred`; to be confirmed)

### 1.6 The 2.7/2.8 era (2008–2009)

- 2.7.0 (2008-08-30) ... 2.8.0 (2009-03-23).
- Tag scheme switched to `vX.Y.Z` (2.7.4+). (`known` from tags)

### 1.7 2.9.x: security hardening era (2012–2022)

- 2.9.0 (2012-09-11) ... 2.9.14 (2022-05-02).
- The `xmlParseEntity`/`XML_PARSE_NOENT`/`XML_PARSE_HUGE` option system and
  entity-expansion limits (XML_MAX_TEXT_LENGTH, XML_MAX_NAME_LENGTH,
  XML_MAX_LOOKUP_LIMIT, XML_MAX_NAMELEN, XML_MAX_HUGE_LENGTH, etc.) were
  introduced, largely in response to the 2013–2016 billion-laughs /
  quadratic-blowup / XXE security work. See `atlas/SECURITY_HISTORY.md`.

### 1.8 Modern era: 2.10+ (2022–2026)

- 2.10.0 (2022-08-17) ... current development.
- Removed libxml2's dependency on libiconv where possible; switched to a
  built-in UTF-8/UTF-16 converter on some platforms. (`inferred`; confirm)
- Threading/global-state cleanup; default initialization is now lazy.
- 2.12/2.13/2.14/2.15 series: continued security, XPath, schema, and
  hardening work. System oracle on this machine: 2.15.3.

### 1.9 Current development tip

- `master` at recovery time: `c6324894` "catalog: Fix NULL deref for
  nextCatalog without 'catalog' attribute". (`known`)

---

## 2. libxslt

### 2.1 Origins (2001)

- CVS conversion dates are unreliable (a spurious 1997-01-01 commit exists).
  The genuine project birth is 2001-01-07: "Initial revision" and "creating
  the project, nothing works" (Daniel Veillard). (`known`)
- Tag `LIBXSLT_0_0_0` (2001-01-07) — first release tag.

### 2.2 The 0.x era (2001): bootstrap

- 0.1.0 (2001-02-08) through 0.14.0 (2001-07-05).
- Rapid development; xsl:stylesheet parsing, templates, XPath integration.

### 2.3 1.0.0 (2001-07-10)

- First 1.0 release. (`known`)

### 2.4 1.0.x maintenance (2001–2004)

- 1.0.2 (2001-08-15) through 1.0.33 (2004-09-14). (`known`)

### 2.5 1.1.x: EXSLT and maturity (2004–present)

- 1.1.0 (2004-12-15) ... current 1.1.45 (2025-07-15). (`known`)
- The 1.1 series added EXSLT (exsl:, math:, set:, str:, dyn:, date:),
  extension mechanisms, and most of the current XSLT engine surface.
- The `LIBXSLT_1_1_x` tag scheme persisted through 1.1.24; later tags use
  `v1.1.x` (from v1.1.25). (`known` from tags)

### 2.6 Current development tip

- `master` at recovery time: `ec95343e` "Remove Nick from AUTHORS as requested".
  (`known`)

---

## 3. Relationship between the two projects

- libxslt depends on libxml2's tree, parser, XPath, and serializer.
  In libxml-rs, `src/xslt` operates on `src/xml` exclusively (§31).
- Both projects are maintained by the same GNOME team and share release
  cadence in the modern era.

---

## 4. Epoch map (semantic epochs, per §68)

The implementation should use these epochs rather than scattered
version comparisons:

| Epoch | libxml2 range | libxslt range | Characteristics |
|---|---|---|---|
| `pre2` | 0.99–1.8.17 | — | Original libxml, `libxml.so.1`, legacy parser |
| `legacy_parser` | 2.0–2.4 | 0.0.0–1.0.x | SAX1, no SAX2 default |
| `sax2` | 2.5–2.6 | 1.0.x | SAX2 namespace-aware default |
| `validation_era` | 2.6–2.8 | 1.1.x | Schemas/RELAX NG/reader mature |
| `security_hardening` | 2.9.0–2.9.14 | 1.1.x | Entity limits, option system |
| `modern` | 2.10+ | 1.1.33+ | Current ABI, lazy init, hardening |

Epoch boundaries are provisional (`inferred`) until the historical delta
atlas (§10) confirms the exact transition points.

---

## 5. Open gaps (recorded per §8)

- Pre-0.99 history (before 1998-12-07): earliest commits exist (1998-07-24)
  but no release tags between the initial import and 0.99. The gap is
  recorded in `atlas/releases/libxml2/_gaps.json`.
- 1.x releases between 1.1 and 1.5: tags missing for several micro versions
  (1.2, 1.4.1, 1.5.1, etc. may not have been tagged, or tags not preserved).
  Recorded as gaps, not invented.
- libxslt pre-0.0.0: no recoverable tags before 2001-01-07.
- Source tarball checksums for most historical releases: pending archive
  acquisition (§8 `source_checksum` field marked `unknown`).
