//! XML parser — native Rust (§19, §20, §85 Phase 3).
//!
//! Implements the XML parser state machine, push parser, SAX/SAX2 integration,
//! declarations, namespaces, entities, DTD construction, recovery modes,
//! parser options, and custom input.
//!
//! Phase 0: scaffolded. Implementation begins in Phase 3.
//!
//! # Upstream contract
//!
//! Mirrors upstream parser.c, parserInternals.c and SAX2.c (SRC-LIBXML2-2.15.0,
//! oracle tree `oracle/historical/src/libxml2-2.15.0/`). The parity target is the
//! system libxml2 2.15.3 oracle: parser diagnostics, tree structure, exit codes
//! and push-parser behavior must match byte-identically.
//!
//! # Conceptual behavior
//!
//! This module is a facade over the parser state machine (state.rs), the lexical
//! tokenizer (tokenizer.rs), input stack management (input.rs) and the C-ABI
//! glue layer (helpers.rs). Together they implement the XML parser state
//! machine, push parser, SAX/SAX2 integration, declarations, namespaces,
//! entities, DTD construction, recovery modes and parser options.
//!
//! # Ownership & safety invariants
//!
//! The parser context owns its input stack and SAX handler; the produced
//! document is transferred to the caller (freed with `xmlFreeDoc`). Input
//! buffers are owned by the parser input stack; filenames are owned dupes
//! (R-000169). SAFETY: raw `_xmlParserCtxt` pointers are only touched through
//! the safe InputBuffer/InputStack wrappers, and the helpers.rs side table
//! keeps boxed inputs alive without borrowing `ctxt._private`.
//!
//! # Historical quirks & epochs
//!
//! E-002: the second parse-error diagnostic (Premature end of data) was
//! regressed in 2.9.10 by the non-recursive refactor (commit 62150ed2), fixed
//! by de5b624f in 2.9.11, and dropped entirely by the 2.12.x error-handling
//! rework (commit c6083a32) — the crate matches the 2.12.6+ single-diagnostic
//! epoch. E-005: xmllint parser exit codes changed 1 to 4 at 2.13.0 (NEWS
//! 2.13.0 xmllint rework of parsing). QUIRK-0001: default parser limits since
//! 2.9.0 (commit 52d8ade7) with XML_PARSE_HUGE as the only lift.
//!
//! # Deliberate oddities
//!
//! Hybrid epochs are deliberate: R-000121 reports the '<' in entity attribute
//! error once (pre-2.13 count) with the 2.13+ exit 4. Deprecated no-op entry
//! points (R-000138) reproduce upstream empty bodies. The push parser keeps
//! chunk-boundary semantics faithful to xmlParseChunk.
//!
//! # Proving courts
//!
//! PARSER court family (PARSER-LIMIT-*, PARSER-ENTITY-*, PARSER-TEXT-LIMIT-*),
//! data-ABI probes ERROR-001 and TREE-001, CLI-XMLLINT-0033/0034, the
//! SECURITY-LIMITS probe, and `cargo test --lib`. Receipts under
//! courts/receipts/phase-11.
//!
//! # Tempting simplifications that would break parity
//!
//! Replacing the tokenizer/state-machine split with a one-pass regex or a
//! third-party parser would break the error-position contract (carets must
//! point at upstream exact columns; R-000163) and the epoch-pinned diagnostic
//! counts. Do not drop push-parser support — xmlParseChunk is part of the
//! oracle surface. Do not lift the default parser limits — that would diverge
//! from every 2.9+ oracle.

pub(crate) mod helpers;
pub(crate) mod input;
pub(crate) mod state;

#[cfg(test)]
pub(crate) mod debug_test;
#[cfg(test)]
pub(crate) mod tests;
pub(crate) mod tokenizer;
