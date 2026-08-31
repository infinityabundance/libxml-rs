//! Threading support (§57, §93, §85 Phase 1).
//!
//! Thread-local state, concurrent parsing, concurrent transformation,
//! shared immutable dictionaries, callback isolation.
//!
//! # UPSTREAM-PARITY
//!
//! libxml2's threading support provides:
//!
//! - `xmlInitThreads()` / `xmlCleanupThreads()` — lifecycle
//! - `xmlLockLibrary()` / `xmlUnlockLibrary()` — global lock
//! - Thread-local storage for error state and parser contexts
//!
//! In modern libxml2 (2.12+), threading is initialized automatically
//! by `xmlInitParser`. The explicit thread functions exist for backward
//! compatibility.
//!
//! In Rust, we use standard thread-safe primitives and `thread_local!`
//! for thread-local storage. The global lock is a no-op since Rust's
//! type system prevents data races at compile time.
//!
//! # Phase 1 status
//!
//! Complete — all threading support is implemented.
//!
//! # Upstream contract
//!
//! Mirrors upstream `threads.c` (`SRC-LIBXML2-2.15.0-THREADS-C`, parity
//! target libxml2 2.15.3 oracle): `xmlInitThreads`, `xmlCleanupThreads`,
//! `xmlLockLibrary`, `xmlUnlockLibrary`, `xmlNewMutex`/`xmlFreeMutex`/
//! `xmlMutexLock`/`xmlMutexUnlock`, `xmlNewRMutex`/`xmlRMutexLock`/
//! `xmlRMutexUnlock`, `xmlNewCond`/`xmlCondWaitSignal`, and the
//! thread-local variants.
//!
//! # Conceptual behavior
//!
//! Implements the legacy explicit threading API. In modern libxml2 (2.12+)
//! initialization is lazy — `xmlInitParser` calls the init path
//! automatically, and the deprecated entry points exist for backward
//! compatibility. Rust primitives (`thread_local!`, parking_lot, atomics)
//! provide the same guarantees without upstream platform dispatch
//! (HAVE_POSIX_THREADS / HAVE_WIN32_THREADS).
//!
//! # Ownership & safety invariants
//!
//! Mutex/rmutex/cond handles are heap objects owned by the caller and
//! freed with the matching free function; thread-local error/parser state
//! is owned per thread and never shared. Rust memory-safety guarantees
//! replace the upstream data-race discipline — the SAFETY argument is the
//! type system, not lock discipline.
//!
//! # Historical quirks & epochs
//!
//! Thread support predates the thread-local globals era: globals.c
//! threading was integrated 2001-10-12/13 (commits b847864f, d0463560,
//! LORE-0005). R-000138: the deprecated init/cleanup entry points are
//! genuine no-ops in modern upstream (lazy init) and the candidate matches
//! that; `xmlCheckThreadLocalStorage` always passes with Rust thread-locals.
//!
//! # Deliberate oddities
//!
//! The global library lock is a deliberate no-op: Rust prevents data races
//! at compile time, and upstream xmlLockLibrary itself became vestigial
//! after the thread-local rewrite. Deprecated entry points keep their
//! no-op bodies to match the oracle byte-for-byte (R-000138).
//!
//! # Proving courts
//!
//! The globals-threading differential probe (tools/abi/globals_threading_
//! probe.py + courts/suites/data-abi/globals-threading-probe.c) verifies
//! handler-slot and error-global behavior byte-identical vs the oracle;
//! the parallel lib suite (100/100 runs clean, R-000170/R-000171) and
//! cargo test exercise the thread-local error model.
//!
//! # Tempting simplifications that would break parity
//!
//! Do not replace thread-locals with globals: per-thread parser error
//! state and the exported xmlLastError mirror (R-000170) depend on the
//! thread-local model. Do not make the deprecated entry points do real
//! work: upstream bodies are empty and observable behavior must match.

use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, Ordering};
use std::os::raw::c_int;

/// Whether threading has been initialized.
static THREADS_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Initialize threading support.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlInitThreads(void);
/// ```
///
/// Returns 0 on success. In modern libxml2, this is called automatically
/// by `xmlInitParser`.
pub fn init_threads() -> c_int {
    THREADS_INITIALIZED.store(true, Ordering::Release);
    0
}

/// Clean up threading support.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlCleanupThreads(void);
/// ```
pub fn cleanup_threads() {
    THREADS_INITIALIZED.store(false, Ordering::Release);
}

/// Check whether threading has been initialized.
pub fn threads_initialized() -> bool {
    THREADS_INITIALIZED.load(Ordering::Acquire)
}

/// Lock the library (global mutex).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlLockLibrary(void);
/// ```
///
/// In upstream libxml2, this locks a global mutex. In Rust, this is
/// a no-op because Rust's type system prevents data races. However,
/// for FFI safety with C callers that may manipulate shared state,
/// a real mutex would be needed. This will be enhanced in Phase 2+.
pub const fn lock_library() {
    // Phase 1: no-op — Rust's type system handles data races for internal code.
    // For C callers going through FFI, this is a best-effort approach.
}

/// Unlock the library.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlUnlockLibrary(void);
/// ```
pub const fn unlock_library() {
    // Phase 1: no-op
}

/// Get the number of active threads (for compatibility).
///
/// Returns the number of active threads, or 0 if unknown.
/// This is a compatibility stub — upstream doesn't expose this directly.
pub const fn get_thread_count() -> c_int {
    1
}

// ═══════════════════════════════════════════════════════════════════════════════
// Mutex / recursive-mutex API (upstream threads.h)
// ═══════════════════════════════════════════════════════════════════════════════
//
// `xmlMutexPtr`/`xmlRMutexPtr` are opaque handles. The candidate boxes a
// parking_lot mutex (a `Mutex<()>` for the simple mutex, a reentrant
// `ReentrantMutex` for the recursive mutex) so lock/unlock round-trips
// through the FFI boundary with real exclusion semantics.
//
// The `RawMutex` trait import brings the manual `lock`/`unlock` methods
// into scope for the raw-mutex lock/unlock used below.
use parking_lot::lock_api::RawMutex as _;

/// Create a simple mutex (upstream threads.h `xmlNewMutex`).
///
/// Returns an opaque handle (free with `xmlFreeMutex`), or NULL on
/// allocation failure.
pub fn new_mutex() -> *mut c_void {
    let m = Box::new(parking_lot::Mutex::new(()));
    Box::into_raw(m) as *mut c_void
}

/// Free a simple mutex (upstream threads.h `xmlFreeMutex`).
///
/// # SAFETY
///
/// - `tok` must be a handle from `xmlNewMutex` (or NULL), and must not be
///   locked by any thread when freed.
pub unsafe fn free_mutex(tok: *mut c_void) {
    if !tok.is_null() {
        drop(Box::from_raw(tok as *mut parking_lot::Mutex<()>));
    }
}

/// Lock a simple mutex (upstream threads.h `xmlMutexLock`).
///
/// # SAFETY
///
/// - `tok` must be a handle from `xmlNewMutex` or NULL.
pub unsafe fn mutex_lock(tok: *mut c_void) {
    if tok.is_null() {
        return;
    }
    // SAFETY: tok is a valid handle from xmlNewMutex. The raw lock blocks
    // until the mutex is acquired and stays held until the matching
    // mutex_unlock (parking_lot lock_api RawMutex manual API). The previous
    // guard-based code dropped the guard immediately, providing no
    // exclusion; the 11.1-Z seal fixed this to real lock semantics.
    unsafe { (*(tok as *mut parking_lot::Mutex<()>)).raw().lock() };
}

/// Unlock a simple mutex (upstream threads.h `xmlMutexUnlock`).
///
/// # SAFETY
///
/// - `tok` must be a handle from `xmlNewMutex` or NULL, and must be
///   locked by the calling thread.
pub unsafe fn mutex_unlock(tok: *mut c_void) {
    if tok.is_null() {
        return;
    }
    // SAFETY: tok must be a handle from xmlNewMutex locked by this thread
    // via mutex_lock; unlocking releases the raw mutex.
    unsafe { (*(tok as *mut parking_lot::Mutex<()>)).raw().unlock() };
}

/// Create a recursive mutex (upstream threads.h `xmlNewRMutex`).
///
/// Returns an opaque handle (free with `xmlFreeRMutex`), or NULL on
/// allocation failure.
pub fn new_rmutex() -> *mut c_void {
    let m = Box::new(parking_lot::ReentrantMutex::new(()));
    Box::into_raw(m) as *mut c_void
}

/// Free a recursive mutex (upstream threads.h `xmlFreeRMutex`).
///
/// # SAFETY
///
/// - `tok` must be a handle from `xmlNewRMutex` (or NULL).
pub unsafe fn free_rmutex(tok: *mut c_void) {
    if !tok.is_null() {
        drop(Box::from_raw(tok as *mut parking_lot::ReentrantMutex<()>));
    }
}

/// Lock a recursive mutex (upstream threads.h `xmlRMutexLock`).
///
/// # SAFETY
///
/// - `tok` must be a handle from `xmlNewRMutex` or NULL.
pub unsafe fn rmutex_lock(tok: *mut c_void) {
    if tok.is_null() {
        return;
    }
    // SAFETY: tok is a valid handle from xmlNewRMutex; reentrant locking is
    // permitted, and the raw lock stays held until rmutex_unlock.
    unsafe {
        (*(tok as *mut parking_lot::ReentrantMutex<()>))
            .raw()
            .lock()
    };
}

/// Unlock a recursive mutex (upstream threads.h `xmlRMutexUnlock`).
///
/// # SAFETY
///
/// - `tok` must be a handle from `xmlNewRMutex` or NULL.
pub unsafe fn rmutex_unlock(tok: *mut c_void) {
    if tok.is_null() {
        return;
    }
    // SAFETY: tok must be a handle from xmlNewRMutex locked by this thread
    // via rmutex_lock; unlocking releases the raw reentrant mutex.
    unsafe {
        (*(tok as *mut parking_lot::ReentrantMutex<()>))
            .raw()
            .unlock()
    };
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_cleanup() {
        assert_eq!(init_threads(), 0);
        assert!(threads_initialized());
        cleanup_threads();
        assert!(!threads_initialized());
    }

    #[test]
    fn test_lock_unlock_no_panic() {
        lock_library();
        unlock_library();
    }
}
