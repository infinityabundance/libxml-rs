//! C ABI compatibility layer (§4, §14).
//!
//! This module implements the external C-compatible world:
//! - Types, structs, constants matching upstream layout
//! - C ABI exports (libxml2.so.2, libxslt.so.1)
//! - Callbacks, allocator hooks, ownership membrane
//! - Versioning macros and runtime reporting
//!
//! # Architecture
//!
//! ```text
//! external C-compatible world
//!         ↕
//! ABI / ownership membrane   ← this module
//!         ↕
//! safe internal Rust semantics (src/xml, src/xslt, ...)
//! ```
//!
//! ## Phase 0 status
//!
//! This module is scaffolded. The ABI membrane will be constructed in Phase 1
//! (Compatibility Skeleton, §85 Phase 1) after the archaeological atlas is
//! complete. See `atlas/PARITY_MATRIX.md` for current API completeness metrics.

pub mod allocator;
pub mod callbacks;
pub mod constants;
pub mod exports_xml2;
pub mod exports_xslt;
pub mod ownership;
pub mod structs;
pub mod types;
pub mod versioning;
