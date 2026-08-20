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

pub mod historical;
pub mod platform;
pub mod profiles;
pub mod quirks;
