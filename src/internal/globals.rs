//! Internal global state management (§57, §68).
//!
//! This module provides internal Rust wrappers around the global state
//! in `crate::xml::globals`. It is NOT exported through the C ABI — it is
//! internal Rust state that backs the public ABI functions.
//!
//! # Phase 1 status
//!
//! Complete — delegates to `crate::xml::globals` for all state management.
//! Initialization, cleanup, reference counting, thread-local error state,
//! parser defaults, and catalog defaults are fully implemented.

use std::os::raw::c_int;

/// Initialize the parser globals.
///
/// SAFETY: Must be called before any other libxml2 functions.
/// Not thread-safe to call concurrently with other libxml2 functions
/// during the first initialization.
pub unsafe fn init_parser() {
    // SAFETY: Delegates to the full global state implementation.
    unsafe { crate::xml::globals::init_parser() };
}

/// Clean up parser globals.
///
/// SAFETY: Must not be called while other libxml2 functions are executing.
pub unsafe fn cleanup_parser() {
    // SAFETY: Delegates to the full global state implementation.
    unsafe { crate::xml::globals::cleanup_parser() };
}

/// Initialize threading support.
///
/// Returns 0 on success.
pub unsafe fn init_threads() -> c_int {
    crate::xml::globals::init_threads()
}
