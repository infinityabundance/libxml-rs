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
pub mod data_globals;
pub mod exports_automata;
pub mod exports_buffer;
pub mod exports_hash;
pub mod exports_html;
pub mod exports_misc;
pub mod exports_nano;
pub mod exports_parser;
pub mod exports_parserint;
pub mod exports_relaxng;
pub mod exports_schema;
pub mod exports_shell;
pub mod exports_string;
pub mod exports_tree;
pub mod exports_treedump;
pub mod exports_uri;
pub mod exports_xinclude;
pub mod exports_xlink;
pub mod exports_xml2;
pub mod exports_xptr;
pub mod exports_xslt;
pub mod exports_xslt_apply;
pub mod exports_xslt_avt;
pub mod exports_xslt_compile;
pub mod exports_xslt_exec;
pub mod exports_xslt_ext;
pub mod exports_xslt_functions;
pub mod exports_xslt_internals;
pub mod exports_xslt_util;
pub mod exports_xslt_vars;
pub mod ownership;
pub mod structs;
pub mod types;
pub mod ucs_blocks;
pub mod ucs_cat;
pub mod versioning;
