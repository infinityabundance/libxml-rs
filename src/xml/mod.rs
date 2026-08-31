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
//!
//! # Upstream contract
//!
//! Parity target is the system libxml2 2.15.3 oracle (`21503-GITv2.15.3`) and
//! libxslt 1.1.45; upstream source trees are `oracle/historical/src/libxml2-2.15.0/*.c`
//! (parser.c, SAX2.c, tree.c, entities.c, namespaces.c, valid.c, error.c,
//! globals.c, dict.c, xmlmemory.c, xmlstring.c, ...), resolved via `SRC-LIBXML2-GIT`
//! in `archaeology/libxml2-git`. The crate is a C-ABI drop-in reimplementation of
//! the upstream library, not a binding: every exported symbol and data global
//! must match the oracle DSO (R-000136: 881 libxml2 + 201 libxslt exports).
//!
//! # Conceptual behavior
//!
//! Each submodule reimplements one upstream subsystem as native Rust: the parser
//! is a state machine over a tokenizer, SAX dispatch mirrors SAX2.c, the tree
//! reproduces libxml2 pointer topology, validation follows valid.c, and the
//! remaining modules (entities, namespaces, DTD, errors, globals, memory,
//! strings, schemas, RELAX NG, Schematron) mirror their upstream .c files.
//! Observable behavior (stdout, stderr, exit codes, tree structure, ABI layout)
//! is the compatibility contract.
//!
//! # Ownership & safety invariants
//!
//! Ownership follows the upstream C contract (atlas/OWNERSHIP_ATLAS.md):
//! documents own their subtrees (caller frees with `xmlFreeDoc`), node
//! parent/doc/ns pointers are borrowed, strings returned by `xmlGetProp` are
//! caller-freed with `xmlFree`, and every xml* allocator result is freed by its
//! matching xmlFree. SAFETY: the crate is a memory-safe Rust reimplementation
//! of an unsafe C library; invariants that upstream enforces by convention are
//! enforced here by the Rust type system plus audited unsafe blocks.
//!
//! # Historical quirks & epochs
//!
//! Current behavior is pinned to the 2.15.3 epoch but carries history:
//! E-001 (xpath node-set newlines, 2.9.10, commit da35eeae), E-002 (parser
//! second diagnostic dropped in 2.12.x, commit c6083a32), E-004 (TEXT compact
//! at 2.13.0, commit 8d04f0ee), E-005 (xmllint exit codes reworked at 2.13.0),
//! E-006 (valid no-DTD exit 0 at 2.15.0), E-007 (HTML single-line at 2.15.0),
//! E-008 (libxslt transform output stable since 2009 or earlier). QUIRK-0001:
//! default parser limits since 2.9.0 (commit 52d8ade7); QUIRK-0002: namespace
//! nodes have no parent (upstream fix 044fc6b7). Security epochs: SEC-0006
//! CVE-2014-3660 amplification guard (fix be2a7eda, regression fix 72a46a51).
//! See atlas/SEMANTIC_EPOCHS.md, atlas/QUIRKS.md, atlas/SECURITY_HISTORY.md.
//!
//! # Deliberate oddities
//!
//! Odd-but-faithful behaviors are preserved on purpose: hybrid-epoch
//! diagnostics (R-000121 reports the entity-in-attribute fatal error once with
//! the 2.13+ exit code 4), exported deprecated init/cleanup entry points as
//! intentional no-ops (R-000138), and the documented safe divergences
//! SD-001..SD-004 in atlas/SECURITY_HISTORY.md where emulating a vulnerability
//! would be unsafe.
//!
//! # Proving courts
//!
//! Exercised by the data-ABI family probes (TREE-001, ERROR-001, READER-001,
//! WRITER-001, CALLBACK-001, DATA-GLOBALS-001, SECURITY-LIMITS, ENCODING-001),
//! the CLI courts (CLI-XMLLINT-*, CLI-XSLTPROC-*, CLI-XMLCATALOG-*), the PARSER,
//! DTD, RELAXNG, XSD, SCHEMATRON, TREE-STRUCTURE and OWNERSHIP court families,
//! and `cargo test --lib` (1135+ tests). Receipts under courts/receipts/phase-11.
//!
//! # Tempting simplifications that would break parity
//!
//! A naive Rust-native API rewrite would break the C-ABI drop-in surface
//! (R-000136). Do not drop exported data globals (R-000135), do not simplify
//! error routing to a single stderr write (R-000161: a counting handler sees
//! 6 xmlFormatError fragments per raise), and never replace the epoch-pinned
//! behaviors with cleaner semantics — byte-identical output against the oracle
//! is the acceptance test.

pub mod automata;
pub mod c14n;
pub mod catalog;
pub mod chvalid;
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
pub mod save;
pub mod sax;
pub mod schemas;
pub mod schematron;
pub mod string;
pub mod threads;
pub mod tree;
/// Unicode character-class tables (upstream libxml2 chvalid data globals,
/// extracted verbatim from `codegen/ranges.inc`).
pub mod unicode_tables;
pub mod uri;
pub mod validation;
pub mod writer;
pub mod xinclude;
pub mod xpath;
pub mod xpointer;
