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
//!
//! # Upstream contract
//!
//! The `abi` facade is the drop-in C ABI surface for libxml2.so.2 (libxml2
//! 2.15.3) and libxslt.so.1 (libxslt 1.1.45): every symbol the oracle DSO
//! exports is exported here with the upstream name and signature, and every
//! public header declaration is backed by an export (R-000136: 881 libxml2 +
//! 201 libxslt + libexslt exports, obligations MISSING = 0).
//!
//! # Conceptual behavior
//!
//! This module implements the ABI/ownership membrane between the external
//! C-compatible world and the safe internal Rust semantics (`src/xml`,
//! `src/xslt`): types, structs, constants, callbacks, allocator hooks, data
//! globals, versioning and the per-subsystem export registries. The membrane
//! is where C pointers become Rust objects and back.
//!
//! # Ownership & safety invariants
//!
//! Ownership across the membrane follows OWNERSHIP_ATLAS: xml-allocator
//! results freed with `xmlFree`, borrowed pointers never freed, callback
//! user-data kept alive by the caller. Safety: every `unsafe extern` export
//! documents what must be true, who establishes it, and which court exercises
//! it (lib.rs policy).
//!
//! # Historical quirks & epochs
//!
//! The export surface accumulated across the whole history: the 2.5 `sax2`
//! epoch, the 2.9.0 security-hardening epoch (QUIRK-0001, commit `52d8ade7`),
//! the 2.13.0 error rework (E-005), the 2.15.0 serializer rework (E-007) and
//! the libxslt stable epoch (E-008). Residuals R-000116 through R-000171
//! document the per-family closure work that produced the current file set.
//!
//! # Deliberate oddities
//!
//! The deprecated init/cleanup entry points (`xmlInitializeGlobalState`,
//! `xmlInitializeDict`, ...) are exported as deliberate no-ops because
//! upstreams own bodies are empty — the no-op IS the oracle behavior
//! (R-000138); the 16 STUB marks are dispositioned the same way (R-000160
//! trivial libxslt bodies). The exports are split across one file per
//! subsystem so each closure workstream owns its surface.
//!
//! # Proving courts
//!
//! The DSO-LOADER court loads every exported symbol from the built DSO;
//! the HEADER-COMPILE court compiles every public header against it;
//! and the data-ABI family courts (DATA-GLOBALS-001, CALLBACK-001,
//! ERROR-001, TREE-001, READER-001, WRITER-001, ENCODING-001, ...) require
//! byte-identical output vs the oracle.
//!
//! # Tempting simplifications that would break parity
//!
//! A tempting simplification is to merge all exports into one file or to
//! delete the no-op entry points as dead code — deleting them breaks
//! downstream linking (R-000138: the no-ops are the oracles observable
//! behavior) and merging would lose the per-subsystem ownership that the
//! DSO-LOADER/HEADER-COMPILE courts rely on. The export split and the no-ops
//! must not be simplified away.

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
