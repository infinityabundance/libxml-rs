//! Internal global state management (§57, §68).
//!
//! This module provides internal Rust wrappers around the global state
//! in `crate::xml::globals`. It is NOT exported through the C ABI — it is
//! internal Rust state that backs the public ABI functions
//! (`xmlInitParser`, `xmlCleanupParser`, `xmlInitThreads` in
//! src/abi/exports_xml2.rs).
//!
//! # Upstream contract
//!
//! The upstream mirror is `globals.c` (SRC-LIBXML2-GIT,
//! archaeology/libxml2-git) — the libxml2 per-thread global-state subsystem
//! (subsystem id `globals` in the custodian atlas). The parity target is
//! the *lifecycle semantics* of that file: when initialization is
//! performed, what cleanup frees, and how thread support is initialized.
//! None of the functions here are themselves part of the public ABI; they
//! are the internal Rust side of the exported xmlInitParser/
//! xmlCleanupParser/xmlInitThreads entry points. Note that the exported
//! xmlInitGlobals/xmlCleanupGlobals names are separate ABI relics handled
//! at the ABI membrane, not here.
//!
//! # Conceptual behavior
//!
//! All real state lives in `crate::xml::globals`: an initialization
//! reference count (INIT_REF_COUNT) that bumps on `init_parser`, drops on
//! `cleanup_parser`, and only runs the full subsystem init/cleanup on the
//! 0-boundary crossings; plus the threading initializer. This module is a
//! thin delegation layer that keeps the ABI membrane (src/abi) from
//! reaching directly into the implementation, so the membrane calls stable
//! internal names and the implementation can be refactored freely.
//!
//! # Ownership & safety invariants
//!
//! - SAFETY: `init_parser` must be called before any other libxml2
//!   functions; the first initialization is not thread-safe against
//!   concurrent use (the upstream documented constraint, preserved here).
//! - SAFETY: `cleanup_parser` must not run while other libxml2 functions
//!   are executing; it is reference-counted so nested init/cleanup pairs
//!   are balanced and the final cleanup only happens at the last drop.
//! - The ownership model is single-owner delegation: the reference count
//!   and all global tables are owned by `crate::xml::globals`; this module
//!   never takes ownership of any state, so there is exactly one owner per
//!   lifecycle.
//! - `init_threads` returns 0 on success (upstream xmlInitThreads
//!   contract) and is safe to call once per process lifetime.
//!
//! # Historical quirks & epochs
//!
//! - Epoch `GlobalStateInit` (2.12.0): upstream moved from eager static
//!   initialization to lazy per-context initialization in the 2.12 rework
//!   (atlas/COMPATIBILITY_PROFILES.md, atlas/SEMANTIC_EPOCHS.md). The
//!   reference-counted lazy model here matches the 2.12+ epoch, not the
//!   eager <= 2.11.x era.
//! - globals.c history: thread support was integrated 2001-10-12/13
//!   (commits b847864f, d0463560 — atlas/LORE.md LORE-0005), long before
//!   the thread-local-globals era; the epoch model captures that
//!   transition.
//! - `xmlInitGlobals` is a deprecated alias for `xmlInitParser` and
//!   `xmlCleanupGlobals` is a documented no-op in globals.c — historical
//!   ABI relics whose semantics are reproduced at the membrane, not here.
//!
//! # Deliberate oddities
//!
//! - The delegation is intentionally trivial: the public-ABI-facing
//!   initializers keep their real logic in `crate::xml::globals` so the
//!   synchronization fixes (R-000170/R-000171 mirror locks) live in one
//!   place. A "refactor" that inlined the logic here would split the
//!   locking between two files for no observable gain.
//! - The module does not re-export anything: it is a private shim, not an
//!   API.
//!
//! # Proving courts
//!
//! Exercised by the GLOBAL-STATE, THREADING, ABI-DATA and ALLOCATOR court
//! families (custodian atlas subsystem `globals`), the parallel
//! error-mirror tests in `cargo test` (R-000170/R-000171 regressions), and
//! the threading/global-state probes; the ABI census proves none of these
//! functions appear in the DSO export table.
//!
//! # Tempting simplifications that would break parity
//!
//! - Removing the delegation and letting the ABI membrane call
//!   `crate::xml::globals` directly would couple the exported surface to
//!   implementation details and make the internal/global boundary
//!   meaningless.
//! - Replacing the reference count with a one-shot init flag would break
//!   balanced init/cleanup pairs and the upstream documented constraint
//!   that cleanup runs at most once, at exit.
//! - Moving this state into public ABI globals would break the "NOT part
//!   of the public ABI" contract that keeps the DSO symbol surface at
//!   parity.
//!
//! # Phase status
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
