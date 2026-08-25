# libxml-rs

**Phase 6: Validation Family** — *DTD, XML Schema, RELAX NG, Schematron.*

Custodial native-Rust reimplementation of the **libxml2 + libxslt** ecosystem:
a forensic reconstruction of observable behavior, implemented in native Rust,
with C ABI compatibility for drop-in replacement.

This is **not** an XML crate. This is **not** an XSLT crate. This is **not** a wrapper.
This is a custodial forensic archive and native-Rust reimplementation of
the complete observable behavior of libxml2 and libxslt across their
historical lifetimes.

---

## Current Status: Phase 6 — Validation Family (§85)

Phase 6 implements the complete validation subsystem: DTD validation, XML Schema
(XSD) parsing and validation, RELAX NG grammar-based validation, and ISO Schematron
rule-based validation. Per §85:

> *Deliverable: Validation suites and error courts close.*

### What exists now

- **Phases 1–5 complete**: C ABI skeleton, tree/ownership, XML parser + SAX, I/O/encoding/URI/catalog/serialization/HTML, XPath 1.0/XPointer/XInclude
- **DTD validation**: Content model validation, ID/IDREF consistency, attribute value validation, notation validation — 56 tests
- **XML Schema (XSD)**: Full parser for element/attribute/complexType/simpleType/restriction/extension/sequence/choice/all/list/union — datatype validation (45+ built-in types with facets) — document validation — 62 tests
- **RELAX NG**: Grammar-based schema language — full XML syntax parser (element/attribute/text/choice/sequence/interleave/zeroOrMore/oneOrMore/optional/list/group/data/value/ref/define/grammar/notAllowed/empty/externalRef) — pattern-based document validation — 40+ tests
- **ISO Schematron**: Rule-based validation — assert/report patterns — XPath context matching — phases — abstract rules + extends — namespace support — diagnostic messages — 42 tests
- **C ABI exports**: Full validation exports for all four subsystems — DTD (`xmlValidateDocument`, `xmlValidateElement`, etc.), XSD (`xmlSchemaNewParserCtxt`, `xmlSchemaParse`, `xmlSchemaValidateDoc`, etc.), RELAX NG (`xmlRelaxNGNewParserCtxt`, `xmlRelaxNGParse`, `xmlRelaxNGValidateDoc`, etc.), Schematron (`xmlSchematronNewParserCtxt`, `xmlSchematronParse`, `xmlSchematronValidateDoc`, etc.)
- **Complete C headers**: `relaxng.h` and `schematron.h` with full type definitions and function declarations
- **802 passing tests**: `cargo test --lib` — 0 failures, 0 errors

### Build

```sh
cargo build                          # Build library + CLI stubs
cargo build --lib                    # Build only the library
cargo test --lib                     # Run library tests (802 passing)

# Test C consumer compilation against our headers:
gcc -I include courts/suites/sanity/ABI-STRUCT-NODE-0001-abicheck.c -o /tmp/abicheck
clang -I include courts/suites/sanity/ABI-ENUM-0001-enumcheck.c -o /tmp/enumcheck

# Build and run oracle container:
docker build -f docker/Dockerfile.oracle -t libxml-rs/oracle:2.12.0 docker/
```

### Oracle verification

The oracle Docker container builds libxml2 2.12.0 and libxslt 1.1.39 from source.
All ABI courts pass against the oracle. The candidate (`libxml-rs`) does not link
against system libxml2/libxslt — verified by oracle contamination court.

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
cargo build              # Build the library
cargo build --lib        # Build only the library (not CLI stubs)
cargo test               # Run tests (Phase 0: limited)
```

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
| API completeness | 🟡 15% (211 symbols exported, all stubs) |
| ABI compatibility | 🟢 6/6 courts passing (struct, symbol, enum) |
| Tree/ownership | 🟢 16 tree tests passing |
| Dictionary/Hash/List | 🟢 27 tests passing |
| Memory/Error/Globals | 🟢 19 tests passing |
| C headers | 🟢 45+19 headers, gcc & clang, zero warnings |
| Parser parity | 🔴 Not started (Phase 2) |
| XPath parity | 🔴 Not started (Phase 5) |
| XSLT parity | 🔴 Not started (Phase 8) |
| CLI parity | 🔴 Not started (Phase 10) |
| Historical atlas | 🟡 30% |
| Oracle infrastructure | 🟢 Docker oracle built and verified |
| Court coverage | 🟢 6 ABI courts passing |
| Downstream testing | 🔴 Not started |

See [`atlas/PARITY_MATRIX.md`](atlas/PARITY_MATRIX.md) for the detailed,
evidence-bounded parity matrix.
