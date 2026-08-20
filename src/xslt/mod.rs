//! libxslt implementation — native Rust (§1, §3, §31–§34).
//!
//! This module contains the complete native-Rust implementation of libxslt's
//! observable behavior: stylesheet compilation, transform engine, templates,
//! patterns, variables, parameters, keys, attributes, namespace aliasing,
//! whitespace stripping, sorting, numbering, documents, imports, extensions,
//! serialization, security, errors.
//!
//! The XSLT engine operates exclusively on the Rust libxml implementation
//! (src/xml), never on upstream C libxml2 (§31).
//!
//! # Phase 0 status
//!
//! All sub-modules are scaffolded. Implementation begins in Phase 8 (§85):
//! - Phase 8: libxslt core (stylesheet compilation, patterns, templates,
//!   imports/includes, variables, parameters, keys, sorting, numbering,
//!   output, extensions, transform runtime)
//! - Phase 9: EXSLT + xsltproc
//!
//! See `atlas/PARITY_MATRIX.md` for current status.

pub mod attributes;
pub mod compiler;
pub mod documents;
pub mod errors;
pub mod extensions;
pub mod imports;
pub mod keys;
pub mod namespace_alias;
pub mod numbering;
pub mod parameters;
pub mod patterns;
pub mod security;
pub mod serialization;
pub mod sorting;
pub mod stylesheet;
pub mod templates;
pub mod transform;
pub mod variables;
pub mod whitespace;
