//! Internal global state management (§57, §68).
//!
//! Manages initialization, cleanup, thread-local state, and global defaults.
//! This module is NOT exported through the C ABI — it is internal Rust state
//! that backs the public ABI functions.
//!
//! # Phase 0 status
//!
//! Scaffolded with minimal stubs for xmlInitParser, xmlCleanupParser,
//! xmlInitThreads. Full global/thread-local state management will be
//! implemented in Phase 1/2.

/// Initialize the parser globals.
///
/// SAFETY: Must be called before any other libxml2 functions.
/// Not thread-safe to call concurrently with other libxml2 functions.
pub unsafe fn init_parser() {
    // Phase 0: stub
}

/// Clean up parser globals.
///
/// SAFETY: Must not be called while other libxml2 functions are executing.
pub unsafe fn cleanup_parser() {
    // Phase 0: stub
}

/// Initialize threading support.
///
/// Returns 0 on success.
pub unsafe fn init_threads() -> std::os::raw::c_int {
    // Phase 0: stub — always succeeds
    0
}
