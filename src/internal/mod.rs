//! Internal implementation details — NOT part of the public C ABI (§4).
//!
//! This module contains internal Rust semantics that are not exposed through
//! the C-compatible ABI. It exists to keep the public ABI surface clean while
//! allowing safe internal Rust implementation patterns.
//!
//! # Safety
//!
//! The `unsafe` boundary is at the ABI membrane (src/abi). Internal code
//! should be as strongly typed and safe as possible, using the ABI membrane
//! to translate between C-compatible representations and internal Rust types.
//!
//! # Phase 0 status
//!
//! This module is scaffolded. Internal implementations will be built as each
//! subsystem is implemented per §85 phases.

pub mod globals;
