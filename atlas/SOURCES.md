# Source Provenance Registry

This file maps **stable source identifiers** (used throughout the codebase and
courts as `SRC-...`) to concrete evidence. Per §55, no archaeological claim may
be made without provenance, and comments in code should reference these
identifiers rather than scattering fragile prose URLs.

Confidence labels (per §8, §11):
- `known` — directly observed/verified from primary evidence
- `inferred` — strongly supported by secondary evidence, not directly verified
- `unknown` — not yet recovered; the gap is recorded, not silently skipped

---

## 1. Upstream git repositories (primary evidence)

| Identifier | Project | Origin | Recovery date | Notes |
|---|---|---|---|---|
| `SRC-LIBXML2-GIT` | libxml2 | `https://gitlab.gnome.org/GNOME/libxml2.git` | 2026-08-20 | Full history, first commit 1998-07-24, tags through v2.15.3 |
| `SRC-LIBXSLT-GIT` | libxslt | `https://gitlab.gnome.org/GNOME/libxslt.git` | 2026-08-20 | Full history, project birth 2001-01-07, tags through v1.1.45 |

Local clones: `archaeology/libxml2-git`, `archaeology/libxslt-git`
(immutable copies for offline reproducibility per §64).

## 2. Release archives (download.gnome.org)

| Identifier | Project | URL pattern | Status |
|---|---|---|---|
| `SRC-GNOME-LIBXML2` | libxml2 | `https://download.gnome.org/sources/libxml2/<X.Y>/libxml2-<V>.tar.xz` | to be archived + checksummed |
| `SRC-GNOME-LIBXSLT` | libxslt | `https://download.gnome.org/sources/libxslt/<X.Y>/libxslt-<V>.tar.xz` | to be archived + checksummed |

## 3. Upstream source files (the `SRC-LIBXML2-<version>-<module>` scheme)

Source identifiers of the form `SRC-LIBXML2-2.9.14-PARSER-C` refer to a specific
file at a specific tagged commit in `SRC-LIBXML2-GIT`. Resolved as:

```
git -C archaeology/libxml2-git show <tag>:parser.c
```

Module names follow upstream source file names:
`parser.c`, `tree.c`, `entities.c`, `namespaces.c`, `valid.c`, `xpath.c`,
`SAX2.c`, `encoding.c`, `xmlIO.c`, `xmlsave.c`, `xmlschemas.c`, `xmlschemastypes.c`,
`relaxng.c`, `schematron.c`, `c14n.c`, `HTMLparser.c`, `HTMLtree.c`,
`xmlreader.c`, `xmlwriter.c`, `xmlregexp.c`, `xmlautomata.c`, `dict.c`,
`hash.c`, `list.c`, `debugXML.c`, `globals.c`, `threads.c`, `error.c`,
`xmlmemory.c`, `catalog.c`, `uri.c`, `xpointer.c`, `xinclude.c`,
`xmlmodule.c`, `xmlunicode.c`, `chvalid.c`, `buf.c`, `xmlstring.c`,
`nanohttp.c`, `nanoftp.c`, `xmlIO.c`, `xmlcatalog.c`, `xmllint.c`, ...

## 4. NEWS / ChangeLog evidence

| Identifier | Meaning |
|---|---|
| `NEWS-<project>-<version>` | The NEWS entry for a given version (e.g. `NEWS-libxml2-2.9.14`) |
| `CHANGELOG-<project>-<version>` | The ChangeLog covering a given version |

Resolved as `git -C archaeology/<project>-git show <tag>:NEWS`.

## 5. Standards

| Identifier | Standard |
|---|---|
| `W3C-XML-1.0` | Extensible Markup Language (XML) 1.0 (Fifth Edition) |
| `W3C-XML-NS-1.0` | Namespaces in XML 1.0 (Third Edition) |
| `W3C-XML-NS-1.1` | Namespaces in XML 1.1 (Second Edition) |
| `W3C-XPATH-1.0` | XML Path Language (XPath) Version 1.0 |
| `W3C-XSLT-1.0` | XSL Transformations (XSLT) Version 1.0 |
| `W3C-XINCLUDE-1.0` | XML Inclusions (XInclude) Version 1.0 |
| `W3C-XPTR-1.0` | XML Pointer Language (XPointer) Version 1.0 |
| `W3C-C14N-1.0` | Canonical XML Version 1.0 |
| `W3C-C14N-EXCL` | Exclusive XML Canonicalization Version 1.0 |
| `OASIS-CATALOG-1.0` | XML Catalogs (OASIS) v1.0 / v1.1 |
| `W3C-XSD-1.0` | XML Schema Part 1: Structures + Part 2: Datatypes (Second Edition) |
| `W3C-RELAXNG` | RELAX NG Specification (OASIS) |
| `ISO-SCHEMATRON` | ISO/IEC 19757-3 Schematron |
| `RFC3986` | Uniform Resource Identifier (URI): Generic Syntax |
| `RFC3987` | Internationalized Resource Identifiers (IRIs) |
| `EXSLT-COMMON` | EXSLT Common (exsl:) |
| `EXSLT-MATH` | EXSLT Math (math:) |
| `EXSLT-SETS` | EXSLT Sets (set:) |
| `EXSLT-STRINGS` | EXSLT Strings (str:) |
| `EXSLT-DYNAMIC` | EXSLT Dynamic (dyn:) |
| `EXSLT-DATES` | EXSLT Dates and Times (date:) |
| `UNICODE-<v>` | Unicode Standard, version <v> |
| `WHATWG-HTML` | HTML Living Standard (historical libxml2 HTML behavior is NOT this) |

## 6. Downstream / issue / mailing-list evidence

| Identifier | Meaning |
|---|---|
| `ISSUE-<project>-<id>` | GitLab issue in the upstream project |
| `ML-<list>-<subject-hash>` | Mailing-list discussion (xml mailing list / libxml mailing list) |
| `DOWNSTREAM-<distro>-<bug>` | Distribution bug tracker entry (Debian, Fedora, etc.) |
| `COMMIT-<hash>` | A specific upstream commit (hash resolvable in SRC-LIBXML2-GIT / SRC-LIBXSLT-GIT) |

## 7. Maintenance

This file is canonical and should be updated whenever a new evidence source is
acquired. Every `SRC-...` identifier used anywhere in the repository must be
registered here (enforced by a court check).

Last updated: 2026-08-20
