//! libxml-rs: Custodial native-Rust reimplementation of libxml2 and libxslt
//!
//! This is not an XML crate. This is not an XSLT crate. This is not a wrapper.
//! This is a forensic reconstruction of the observable behavior of the libxml2 + libxslt
//! ecosystem across its historical lifetime, implemented in native Rust.
//!
//! # Architecture
//!
//! The crate is organized into semantic modules corresponding to real ownership boundaries:
//!
//! - `abi` - C ABI compatibility layer: types, structs, constants, exports, callbacks, allocator, ownership, versioning
//! - `xml` - libxml2 implementation: parser, SAX, tree, entities, namespaces, DTD, validation, reader, writer, encoding, I/O, catalog, URI, XPath, XPointer, XInclude, RELAX NG, schemas, Schematron, C14N, HTML, regex, automata, dictionary, hash, list, debug, globals, threads, errors, memory
//! - `xslt` - libxslt implementation: stylesheet, compiler, transform, templates, patterns, variables, parameters, keys, attributes, namespace_alias, whitespace, sorting, numbering, documents, imports, extensions, serialization, security, errors
//! - `exslt` - EXSLT modules: common, math, sets, strings, dynamic, functions, dates
//! - `compatibility` - Historical profiles, quirks, platform-specific behavior
//! - `bin` - CLI tools: xmllint, xmlcatalog, xsltproc
//!
//! # Safety
//!
//! This crate uses `unsafe` only where fundamentally required for C ABI export, raw-pointer
//! compatibility, foreign allocator interoperability, public C structure layout, callbacks,
//! variadic compatibility, OS interfaces, and dynamic-loader interaction.
//!
//! Every unsafe block documents:
//! - What must be true
//! - Who establishes it
//! - How long it remains true
//! - Which oracle/parity court exercises the assumption
//! - What would constitute violation

#![deny(unconditional_recursion, unused_lifetimes, while_true)]
#![warn(
    missing_docs,
    missing_debug_implementations,
    clippy::all,
    clippy::pedantic,
    clippy::nursery,
    clippy::cargo,
    clippy::missing_const_for_fn,
    clippy::missing_inline_in_public_items
)]
#![allow(
    clippy::module_name_repetitions,
    clippy::multiple_crate_versions,
    clippy::too_many_lines,
    clippy::type_complexity
)]

// Public ABI compatibility layer
pub mod abi;

// libxml2 implementation
pub mod xml;

// libxslt implementation
pub mod xslt;

// EXSLT implementation
pub mod exslt;

// Compatibility profiles and historical behavior
pub mod compatibility;

// Binary entry points are defined as [[bin]] targets in Cargo.toml.
// They are NOT library modules — they depend on libxml_rs as a library.
// See src/bin/xmllint.rs, src/bin/xmlcatalog.rs, src/bin/xsltproc.rs

// Phase 0: ABI re-exports will be populated when types are defined.
// The `allow(unused_imports)` is intentional — these will be used in Phase 1+.
#[allow(unused_imports)]
use abi::allocator::*;
#[allow(unused_imports)]
use abi::callbacks::*;
#[allow(unused_imports)]
use abi::constants::*;
#[allow(unused_imports)]
use abi::ownership::*;
#[allow(unused_imports)]
use abi::structs::*;
#[allow(unused_imports)]
use abi::types::*;
#[allow(unused_imports)]
use abi::versioning::*;

// Internal modules (not part of public C ABI)
mod internal;

/// The full version string of the libxml-rs crate (from Cargo.toml).
pub const LIBXML_RS_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Major version number of libxml-rs.
pub const LIBXML_RS_VERSION_MAJOR: u32 = 0;

/// Minor version number of libxml-rs.
pub const LIBXML_RS_VERSION_MINOR: u32 = 1;

/// Micro version (patch) number of libxml-rs.
pub const LIBXML_RS_VERSION_MICRO: u32 = 0;
