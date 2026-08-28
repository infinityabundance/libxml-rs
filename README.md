# libxml-rs

**Phase 8: libxslt — Complete XSLT 1.0 Engine.**

Custodial native-Rust reimplementation of the **libxml2 + libxslt** ecosystem:
a forensic reconstruction of observable behavior, implemented in native Rust,
with C ABI compatibility for drop-in replacement.

This is **not** an XML crate. This is **not** an XSLT crate. This is **not** a wrapper.
This is a custodial forensic archive and native-Rust reimplementation of
the complete observable behavior of libxml2 and libxslt across their
historical lifetimes.

---

## Current Status: Phase 8 — libxslt (§85)

Phase 8 implements the complete native-Rust **XSLT 1.0 engine**: stylesheet
compilation, template matching, pattern compilation, variable/parameter binding,
keys, sorting, numbering, imports/includes, extensions, security, serialization,
and the transform runtime. Per §85:

> *Deliverable: XSLT core oracle courts close.*

The XSLT engine operates exclusively on the Rust libxml implementation —
no upstream libxml2/libxslt is loaded, linked, or shelled out to (§31). The
full pipeline is:

```text
Rust CLI → Rust libxslt compatibility layer → Rust XSLT engine
        → Rust XPath implementation → Rust libxml tree/parser/serializer
```

### What exists now

- **Phases 1–7 complete**: C ABI skeleton, tree/ownership, XML parser + SAX,
  I/O/encoding/URI/catalog/serialization/HTML, XPath 1.0/XPointer/XInclude,
  validation family (DTD/XSD/RELAX NG/Schematron), C14N/reader/writer/regex/automata/debug
- **XSLT stylesheet lifecycle**: `xsltStylesheetCreate`, `xsltParseStylesheetDoc/File/Memory`, `xsltFreeStylesheet`, `xsltGetStylesheetDoc`, `xsltSetStylesheetDoc` — simplified stylesheets (literal result elements) supported
- **XSLT compiler**: template compilation, top-level elements (`key`, `decimal-format`, `namespace-alias`, `attribute-set`, `strip-space`, `preserve-space`, `output`, `variable`, `param`), `xsl:import`/`xsl:include`, import precedence
- **XSLT templates**: priority-ordered template lists, `xsltFindTemplate` (XSLT 1.0 §5.2), named-template lookup, default priority computation (§5.5)
- **XSLT patterns**: full pattern compiler (union `|`, `//`, `@`, `*`, `node()`, `text()`, `comment()`, `processing-instruction()`, predicates), `xsltDefaultPriority`
- **Variables & parameters**: variable stacks, global variable initialization, `xsl:with-param`, caller parameter parsing (`name=value`, `{uri}name=value`)
- **Keys**: key definitions, key table construction, `key()` function support
- **Sorting**: multi-key sort, text/number data types, ascending/descending
- **Numbering**: `xsl:number` with decimal (`1`), zero-padded (`01`), alphabetic (`a`/`A`), and roman (`i`/`I`) formats
- **Transform runtime**: `xsltApplyStylesheet`, `xsltApplyStylesheetUser`, `xsltApplyStylesheetStacked`, transform context lifecycle, instruction execution — `apply-templates`, `call-template`, `apply-imports`, `for-each`, `value-of`, `copy-of`, `copy`, `element`, `attribute`, `text`, `comment`, `processing-instruction`, `number`, `choose`/`when`/`otherwise`, `if`, `variable`, `param`, `sort`, `message` — built-in template rules (§5.8), recursion depth limiting
- **XSLT XPath functions**: `document()`, `key()`, `generate-id()`, `system-property()`, `element-available()`, `function-available()`, `current()`
- **Serialization**: `xsltSaveResultToFile`, `xsltSaveResultToFd`, `xsltSaveResultToString` with output method selection (`xml`/`html`/`text`)
- **Security**: full `xsltSecurityPrefs` API (`xsltNewSecurityPrefs`, `xsltSetSecurityPrefs`, `xsltGetSecurityPrefs`, global defaults)
- **Extensions**: `xsltRegisterExtFunction`, `xsltRegisterExtElement`
- **Errors**: XSLT error domains/levels, per-context error handler wiring, stderr reporting
- **C ABI exports**: all 33 libxslt symbols exported and verified (`nm -D`)
- **1060 passing tests**: `cargo test --lib` — 0 failures

### Underlying subsystem fixes landed during Phase 8

| Fix | Surface | Detail |
|-----|---------|--------|
| SAX2 namespace resolution | parser | `startElementNs` receives split prefix/localname + resolved URIs; element and attribute namespaces now attached by the tree builder |
| Absolute path evaluation | XPath | `/root/item` evaluates from the document node, not the root element (XPath 1.0 `/` semantics) |
| XPath C ABI helpers | ABI | `xmlXPathObjectCopy`, `xmlXPathCastToString`, `xmlXPathCastStringToNumber`, `xmlXPathCmpNodes`, `xmlXPathNodeSetCreate` |
| Node content getter | tree | `node_get_content` (upstream `xmlNodeGetContent` semantics) |
| Template-content ownership | xslt | Template content belongs to the stylesheet document; `xsltFreeTemplate` no longer double-frees it |
| `node()`/`text()` patterns | xslt | Bare node-test calls translate to steps with correct §5.5 priorities |
| `match="/"` semantics | xslt | Matches only the document node, not the root element |

All six Phase 8 residuals are documented in [`atlas/RESIDUAL_LEDGER.md`](atlas/RESIDUAL_LEDGER.md) (R-000101–R-000106).

### Build

```sh
cargo build                          # Build library + CLI binaries
cargo build --lib                    # Build only the library
cargo test --lib                     # Run library tests (1060 passing)

# Test C consumer compilation against our headers:
gcc -I include courts/suites/sanity/ABI-STRUCT-NODE-0001-abicheck.c -o /tmp/abicheck
clang -I include courts/suites/sanity/ABI-ENUM-0001-enumcheck.c -o /tmp/enumcheck

# Build and run oracle container:
docker build -f docker/Dockerfile.oracle -t libxml-rs/oracle:2.12.0 docker/
```

### Published artifacts

- crates.io: [`libxml-rs`](https://crates.io/crates/libxml-rs) `0.1.0-alpha.10`
- GitHub: <https://github.com/infinityabundance/libxml-rs>

### Oracle verification

The oracle Docker container builds libxml2 2.12.0 and libxslt 1.1.39 from source.
The candidate (`libxml-rs`) does not link against system libxml2/libxslt — verified
by the oracle contamination court.

---

## Test coverage by subsystem

| Subsystem | Tests | Subsystem | Tests | Subsystem | Tests |
|-----------|------:|-----------|------:|-----------|------:|
| XPath 1.0 | 128 | URI | 69 | Encoding | 65 |
| XML Schema (XSD) | 62 | DTD validation | 56 | RELAX NG | 56 |
| XML parser + SAX | 50 | Regex | 44 | I/O | 44 |
| Schematron | 40 | DTD | 35 | XML Reader | 35 |
| Entities | 31 | HTML | 31 | C14N | 29 |
| Tree/ownership | 28 | XInclude | 23 | XML Writer | 20 |
| Automata | 16 | Catalog | 16 | XPointer | 15 |
| Debug | 11 | Hash | 9 | List | 8 |
| Dictionary | 7 | Globals | 7 | String | 6 |
| Errors | 5 | Memory | 2 | Threads | 2 |
| ABI allocator | 8 | | | | |
| XSLT patterns | 46 | XSLT transform | 14 | XSLT security | 7 |
| XSLT numbering | 5 | XSLT stylesheet | 4 | XSLT variables/params | 8 |
| XSLT compiler | 3 | XSLT sorting | 3 | XSLT misc (keys/space/serial/ns/imports/ext) | 13 |
| **Total (1060 passing, 1 ignored)** | | | | | |

---

## Project Structure

```
libxml-rs/
├── Cargo.toml              # Single Cargo package (no workspace)
├── src/
│   ├── lib.rs              # Library entry point
│   ├── abi/                # C ABI compatibility layer (§4, §14)
│   ├── xml/                # libxml2 implementation (§1, §3, §31)
│   ├── xslt/               # libxslt implementation (§31–§34)
│   ├── exslt/              # EXSLT modules (§35)
│   ├── compatibility/      # Historical profiles, quirks, platform (§68, §69)
│   └── bin/                # CLI tools: xmllint, xmlcatalog, xsltproc (§36)
├── include/                # Compatible C headers (§15)
├── atlas/                  # Forensic archive (§7–§12)
│   ├── releases/           # Release manifests per version
│   ├── api/                # Public API inventories
│   ├── abi/                # ABI snapshots
│   ├── symbols/            # Symbol table comparisons
│   ├── config/             # Build configuration profiles
│   ├── standards/          # Standards mapping
│   ├── HISTORY.md          # Complete release history
│   ├── LORE.md             # Undocumented behavior archive
│   ├── QUIRKS.md           # Confirmed compatibility quirks
│   ├── PARITY_MATRIX.md    # Current parity status
│   ├── RESIDUAL_LEDGER.md  # Unexplained differences
│   └── SECURITY_HISTORY.md # Vulnerability custody
├── oracle/                 # Reproducible upstream build environment (§39)
├── courts/                 # Differential testing framework (§40–§50)
│   ├── schema.json         # Casefile schema
│   ├── suites/             # Court case suites
│   ├── receipts/           # Execution receipts
│   └── tools/              # Court runner
├── tools/                  # Archaeology and analysis tooling
│   ├── archaeology/        # manifest.py, apiatlas.py, symbols.py, delta.py, profileconfig.py
│   └── courts/             # Court runner
├── docker/                 # Reproducible Docker oracle images
├── docs/                   # Technical documentation
└── archaeology/            # Upstream git clones (immutable, offline)
```

---

## Build

```sh
cargo build              # Build the library and CLI binaries
cargo build --lib        # Build only the library
cargo test --lib         # Run library tests (1060 passing)
cargo build --release    # Optimized build (LTO, panic=abort)
```

The crate builds as `cdylib` + `staticlib` + `rlib`; the build script also
emits `libxml-2.0.pc`/`libxslt.pc` pkg-config files, `xml2-config`/`xslt-config`
scripts, and SONAME symlinks into the target directory.

---

## License

Licensed under either of:
- MIT license ([LICENSE-MIT](LICENSE-MIT))
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))

at your option.

---

## Project Status

| Dimension | Status |
|-----------|--------|
| API completeness | 🟢 libxml2 surfaces complete; libxslt ABI (33 symbols) exported |
| ABI compatibility | 🟢 ABI courts passing (struct, symbol, enum) |
| Tree/ownership | 🟢 28 tree tests passing |
| XML parser + SAX | 🟢 50 parser tests; SAX2 namespace resolution |
| XPath 1.0 | 🟢 128 tests, absolute paths, node sets, conversions |
| Validation family | 🟢 DTD (56) + XSD (62) + RELAX NG (56) + Schematron (40) |
| XSLT 1.0 engine | 🟢 103 tests; full instruction surface; end-to-end transforms |
| C headers | 🟢 45+19 headers, gcc & clang, zero warnings |
| CLI parity | 🟡 Scaffolded (Phase 9: `xsltproc`; Phase 10: `xmllint`/`xmlcatalog`) |
| EXSLT | 🔴 Not started (Phase 9) |
| Historical atlas | 🟡 Release manifests + API/ABI snapshots for current versions |
| Oracle infrastructure | 🟢 Docker oracle built and verified |
| Court coverage | 🟢 ABI courts passing; differential suites staged |
| Downstream testing | 🔴 Not started (Phase 12) |

See [`atlas/PARITY_MATRIX.md`](atlas/PARITY_MATRIX.md) for the detailed,
evidence-bounded parity matrix and [`atlas/RESIDUAL_LEDGER.md`](atlas/RESIDUAL_LEDGER.md)
for the residual ledger.
