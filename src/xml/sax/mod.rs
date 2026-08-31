//! SAX/SAX2 callback infrastructure (§20).
//!
//! SAX event recording and dispatch. Tracks exact event ordering, callback
//! sequencing, and argument capture for forensic comparison.
//!
//! # Upstream contract
//!
//! Facade over the SAX/SAX2 callback machinery; mirrors upstream SAX2.c and
//! the SAX dispatch paths of parser.c (SRC-LIBXML2-2.15.0, oracle tree
//! `oracle/historical/src/libxml2-2.15.0/`). The parity target is the system
//! libxml2 2.15.3 oracle default SAX handler and callback routing.
//!
//! # Conceptual behavior
//!
//! SAX event recording and dispatch: exact event ordering, callback
//! sequencing and argument capture. SAX2 namespace-aware callbacks are
//! preferred when present, with SAX1 fallback (see dispatch.rs); the default
//! handler builds the tree exactly like xmlSAX2StartElementNs and
//! xmlSAX2AttributeNs (R-000147).
//!
//! # Ownership & safety invariants
//!
//! SAFETY: callbacks are invoked with the caller userData verbatim; the
//! library never dereferences or frees application context. The handler
//! struct is borrowed from the parser context; nodes built by the default
//! handler are owned by the document. user-data pointers are caller-owned by
//! convention (atlas/OWNERSHIP_ATLAS.md section 6).
//!
//! # Historical quirks & epochs
//!
//! SAX2 became the default parser path in the 2.5 era (sax2 epoch,
//! atlas/HISTORY.md 1.4); xmlSAX2InitDefaultSAXHandler stores the legacy
//! xmlParserError / xmlParserWarning in the error/warning slots (R-000161).
//! Attribute-value compact-text marking was pinned by the 2.13.0 epoch
//! (E-004, commit 8d04f0ee; R-000120).
//!
//! # Deliberate oddities
//!
//! Deliberate oddities: xmlSAX2AttributeNs resolves attribute prefixes
//! through the element-local nsDef chain then the parent scope (R-000147);
//! entity-containing attribute values are never marked compact (R-000120);
//! the legacy SAX1 variants are kept for drop-in consumers of
//! xmlDefaultSAXHandler (R-000135).
//!
//! # Proving courts
//!
//! Exercised by the PARSER court family, TREE-001 (default-handler stderr in
//! the fingerprint), READER-001 (attribute-namespace resolution), ERROR-001
//! (error-channel routing) and `cargo test --lib`. Receipts under
//! courts/receipts/phase-11.
//!
//! # Tempting simplifications that would break parity
//!
//! Dropping the SAX1 fallback would break consumers of xmlDefaultSAXHandler
//! (R-000135). Not resolving attribute prefixes at attribute time would make
//! xmlHasNsProp / MoveToAttributeNs see NULL namespaces (R-000147). Do not
//! clean up the compact-text rule — entity-containing attribute values must
//! stay non-compact (R-000120, byte-identical since 2.7.8).

pub(crate) mod default;
pub(crate) mod dispatch;

pub(crate) use dispatch::xmlSAX2InitDefaultSAXHandler;
