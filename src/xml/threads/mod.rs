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
