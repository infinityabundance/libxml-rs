//! libxml2 implementation — native Rust (§1, §3, §31).
//!
//! This module contains the complete native-Rust implementation of libxml2's
//! observable behavior: parser, SAX, tree, entities, namespaces, DTD,
//! validation, reader, writer, encoding, I/O, catalog, URI, XPath, XPointer,
//! XInclude, RELAX NG, schemas, Schematron, C14N, HTML, regex, automata,
//! dictionary, hash, list, debug, globals, threads, errors, memory.
//!
//! # Phase 0 status
//!
//! All sub-modules are scaffolded. Implementation proceeds in phases per §85:
//! - Phase 2: Tree and ownership
//! - Phase 3: XML parsing + SAX
//! - Phase 4: I/O, encoding, URI, catalog, serialization, HTML
//! - Phase 5: XPath/XPointer/XInclude
//! - Phase 6: Validation family
//! - Phase 7: Remaining libxml2 surfaces
//!
//! See `atlas/PARITY_MATRIX.md` for current status.

pub mod automata;
pub mod c14n;
pub mod catalog;
pub mod debug;
pub mod dictionary;
pub mod dtd;
pub mod encoding;
pub mod entities;
pub mod errors;
pub mod globals;
pub mod hash;
pub mod html;
pub mod io;
pub mod list;
pub mod memory;
pub mod namespaces;
pub mod parser;
pub mod reader;
pub mod regex;
pub mod relaxng;
pub mod sax;
pub mod schemas;
pub mod schematron;
pub mod threads;
pub mod tree;
pub mod uri;
pub mod validation;
pub mod writer;
pub mod xinclude;
pub mod xpath;
pub mod xpointer;
