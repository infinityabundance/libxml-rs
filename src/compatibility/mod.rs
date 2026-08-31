//! Compatibility profiles, historical behavior, and platform quirks (§68, §69).
//!
//! This module provides version-aware compatibility profiles that allow
//! libxml-rs to emulate the behavior of specific historical upstream versions.
//!
//! # Semantic epochs (§68)
//!
//! | Epoch | libxml2 range | libxslt range | Characteristics |
//! |-------|---------------|---------------|-----------------|
//! | `pre2` | 0.99–1.8.17 | — | Original libxml, libxml.so.1, legacy parser |
//! | `legacy_parser` | 2.0–2.4 | 0.0.0–1.0.x | SAX1, no SAX2 default |
//! | `sax2` | 2.5–2.6 | 1.0.x | SAX2 namespace-aware default |
//! | `validation_era` | 2.6–2.8 | 1.1.x | Schemas/RELAX NG/reader mature |
//! | `security_hardening` | 2.9.0–2.9.14 | 1.1.x | Entity limits, option system |
//! | `modern` | 2.10+ | 1.1.33+ | Current ABI, lazy init, hardening |
//!
//! # Phase 0 status
//!
//! This module is scaffolded. Historical profiling will be built as the
//! historical delta atlas (§10) is populated and semantic epochs are validated.
//!
//! See `atlas/HISTORY.md` for the full history atlas.
//!
//! # Upstream contract
//!
//! This module is the Rust-side umbrella for emulating historical upstream
//! behavior. It is NOT part of the C ABI: upstream has no "compatibility"
//! subsystem, so there is no upstream .c file to mirror. The parity target
//! is the observable behavior of upstream across versions, evidenced by
//! SRC-LIBXML2-GIT / SRC-LIBXSLT-GIT (archaeology clones), the historical
//! oracle matrix (oracle/historical/), and the epoch findings E-001..E-008
//! in atlas/SEMANTIC_EPOCHS.md. The four submodules own the four evidence
//! families: `profiles` (capability epochs), `historical` (docs live in
//! atlas/HISTORY.md), `platform` (atlas/PLATFORM_SURFACE_ATLAS.md), and
//! `quirks` (atlas/QUIRKS.md).
//!
//! # Conceptual behavior
//!
//! Version-dependent behavior is modelled as capability epochs: each
//! behavioral capability whose semantics changed at a documented upstream
//! boundary resolves to a value for a target version pair, so historical
//! emulation has one deliberate structure (the resolver in `profiles`)
//! instead of scattered `if version == ...` branches throughout the engine.
//!
//! # Ownership & safety invariants
//!
//! - All resolved capabilities are pure, `Copy`-able value data; nothing in
//!   this module owns heap memory or holds pointers into the engine.
//! - The resolver is deterministic and read-only: no global state and no
//!   interior mutability, so profile resolution is thread-safe by
//!   construction.
//! - `CompatibilityProfile::for_libxml2` deliberately panics on versions
//!   newer than the system oracle rather than inventing an unverified epoch
//!   (a fail-fast invariant).
//!
//! # Historical quirks & epochs
//!
//! The epoch table above and the capability table in `profiles` are the
//! core evidence of the module; every boundary is pinned by the historical matrix
//! and by upstream commits (da35eeae, e85f9b98, 387a952b, 8d04f0ee,
//! de5b624f — see atlas/SEMANTIC_EPOCHS.md). QUIRK-* entries (atlas/QUIRKS.md)
//! record confirmed quirks such as the 2.9.0 default parser limits and the
//! misspelled XML_MAX_TEXT_LENGHT macro.
//!
//! # Deliberate oddities
//!
//! - `GlobalStateInit` is a capability epoch upstream never documented as a
//!   behavior change (the 2.12 lazy-init rework); modelling it as an epoch
//!   keeps emulation uniform rather than special-casing one subsystem.
//! - The `XslTransform` capability has exactly one value (Stable): E-008
//!   proved a 15-year stable epoch, so the single-variant enum documents
//!   the proven absence of a boundary instead of pretending one exists.
//!
//! # Proving courts
//!
//! The capability boundaries are regressed by the unit tests in
//! `profiles` (`cargo test`) and by the differential court families that
//! exercise the behaviors they describe: CLI-XMLLINT-* (xpath
//! serialization, exit-code epochs), XPATH, PARSER, DTD, HTML, C14N,
//! XINCLUDE, XSD, RELAXNG, SCHEMATRON and XPOINTER, plus the historical
//! HIST-EPOCH-* casefiles and their receipts
//! (courts/receipts/historical-matrix-*).
//!
//! # Tempting simplifications that would break parity
//!
//! - Replacing the resolver with direct version comparisons at each call
//!   site would scatter the epoch boundaries across the engine and make
//!   regression triage against older oracles impossible to audit.
//! - Deleting single-variant capabilities such as `XslTransform` would
//!   erase the documented evidence that a boundary does not exist.
//! - Letting `for_libxml2` extrapolate past the system oracle would
//!   fabricate epochs for versions no oracle measures — a hazard for every
//!   future completeness claim.

pub mod historical;
pub mod platform;
pub mod profiles;
pub mod quirks;
