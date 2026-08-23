//! SAX/SAX2 callback infrastructure (§20).
//!
//! SAX event recording and dispatch. Tracks exact event ordering, callback
//! sequencing, and argument capture for forensic comparison.

pub(crate) mod default;
pub(crate) mod dispatch;

pub(crate) use dispatch::xmlSAX2InitDefaultSAXHandler;
pub(crate) use dispatch::SaxDispatcher;
