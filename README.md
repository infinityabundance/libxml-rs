# libxml-rs

**Phase 0: Archaeological Seizure** — *Not yet a drop-in replacement.*

Custodial native-Rust reimplementation of the **libxml2 + libxslt** ecosystem:
a forensic reconstruction of observable behavior, implemented in native Rust,
with C ABI compatibility for drop-in replacement.

This is **not** an XML crate. This is **not** an XSLT crate. This is **not** a wrapper.
This is a custodial forensic archive and native-Rust reimplementation of
the complete observable behavior of libxml2 and libxslt across their
historical lifetimes.

---

## Current Status: Phase 0 — Archaeological Seizure (§85)

The project is in its first phase: building the forensic atlas before
writing any implementation code. Per §7 of the project specification:

> *Do not begin by writing a new XML parser from memory. Begin with archaeology.*

### What exists now

- **Release manifests**: 183 libxml2 + 92 libxslt releases cataloged from git history
- **API atlas**: Complete public API inventory for libxml2 2.15.3 (1403 header functions, 312 typedefs, 76 structs, 39 enums) and libxslt 1.1.45 (231 header functions, 63 typedefs, 16 structs, 7 enums)
- **ABI atlas**: Symbol-table comparison between headers and DSO exports
- **Standards atlas**: W3C XML/XPath/XSLT/XInclude/C14N standards mapping
- **History atlas**: Complete release history from 1998 to present
- **Oracle infrastructure**: Docker build environment scaffold (not yet built)
- **Court framework**: Differential testing casefile schema and runner
- **Tooling**: Automated archaeology pipeline (manifest generation, API extraction, ABI comparison, delta analysis)
- **Module scaffold**: Complete module tree matching upstream subsystem boundaries

### What does NOT exist yet

No actual libxml2/libxslt functionality has been implemented. The Rust code
is scaffolding — module declarations, documentation, and stubs only.
Implementation begins in Phase 1 (Compatibility Skeleton) after the
archaeological atlas is complete.

### Why publish in Phase 0?

Per §103: *"README claims must be evidence bounded."* This initial publication
establishes the project identity, archives the archaeological tooling, and
provides transparency about the project's actual state. No compatibility
claims are made — the parity matrix explicitly shows 0% implementation status.

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
| API completeness | 📋 0% (inventoried, not implemented) |
| ABI compatibility | 🔴 Not started |
| Parser parity | 🔴 Not started |
| XPath parity | 🔴 Not started |
| XSLT parity | 🔴 Not started |
| CLI parity | 🔴 Not started |
| Historical atlas | 🟡 30% (current versions captured, historical gaps open) |
| Oracle infrastructure | 🟡 Scaffolded (not yet built) |
| Court coverage | 🟡 Scaffolded (framework exists, no cases executed) |
| Downstream testing | 🔴 Not started |

See [`atlas/PARITY_MATRIX.md`](atlas/PARITY_MATRIX.md) for the detailed,
evidence-bounded parity matrix.
