//! XML parser — native Rust (§19, §20, §85 Phase 3).
//!
//! Implements the XML parser state machine, push parser, SAX/SAX2 integration,
//! declarations, namespaces, entities, DTD construction, recovery modes,
//! parser options, and custom input.
//!
//! Phase 0: scaffolded. Implementation begins in Phase 3.

pub(crate) mod helpers;
pub(crate) mod input;
pub(crate) mod state;

#[cfg(test)]
pub(crate) mod debug_test;
#[cfg(test)]
pub(crate) mod tests;
pub(crate) mod tokenizer;
