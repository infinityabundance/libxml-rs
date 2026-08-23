//! Global state management (§57, §85 Phase 1).
//!
//! Manages all library-wide global state:
//!
//! - Parser defaults (validity checking, entity substitution, blanks, etc.)
//! - Generic and structured error callbacks
//! - Catalog defaults
//! - Memory hooks (connects to allocator.rs)
//! - Thread-local state for errors
//! - Initialization/cleanup reference counting
//!
//! # UPSTREAM-PARITY
//!
//! libxml2 has many global variables that control default parser behavior.
//! These are exposed as public ABI symbols that applications can read/write
//! directly. We use atomic operations for thread safety while maintaining
//! the same observable semantics.
//!
//! # Thread safety
//!
//! All global state uses atomic operations or parking_lot locks.
//! Error state is thread-local via `thread_local!`.
//!
//! # Phase 1 status
//!
//! Complete — all global state management is implemented.
//! Future phases may add historical version-specific behavior.

use core::cell::RefCell;
use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, Ordering};
use std::os::raw::c_int;

use crate::abi::allocator;
use crate::abi::callbacks::{xmlGenericErrorFunc, xmlStructuredErrorFunc};
use crate::abi::structs::_xmlError;
use crate::abi::types::xmlErrorLevel::XML_ERR_NONE;
use crate::abi::types::*;
use crate::abi::versioning;

// ═══════════════════════════════════════════════════════════════════════════════
// Initialization Reference Counting
// ═══════════════════════════════════════════════════════════════════════════════

/// Reference count for xmlInitParser / xmlCleanupParser.
static INIT_REF_COUNT: AtomicI32 = AtomicI32::new(0);

/// Whether threading has been initialized.
static THREADS_INITIALIZED: AtomicBool = AtomicBool::new(false);

// ═══════════════════════════════════════════════════════════════════════════════
// Parser Defaults
// ═══════════════════════════════════════════════════════════════════════════════
//
// These are the global parser default variables exposed by libxml2's ABI.
// Applications can read and write them directly to change default behavior.
//
// Upstream declarations (from parser.h / parserInternals.h):
//
// ```c
// extern int xmlDoValidityCheckingDefaultValue;
// extern int xmlDoWarningsDefaultValue;
// extern int xmlIndentTreeOutput;
// extern int xmlKeepBlanksDefaultValue;
// extern int xmlLoadExtDtdDefaultValue;
// extern int xmlPedanticParserDefaultValue;
// extern int xmlSubstituteEntitiesDefaultValue;
// extern int xmlSaveNoEmptyTags;
// extern int xmlGetWarningsDefaultValue;
// ```

/// Default: perform validity checking.
/// Controlled by --valid / --novalid flags in xmllint.
static VALIDITY_CHECKING_DEFAULT: AtomicI32 = AtomicI32::new(0);

/// Default: emit warnings.
static DO_WARNINGS_DEFAULT: AtomicI32 = AtomicI32::new(1);

/// Default: indent tree output.
static INDENT_TREE_OUTPUT: AtomicI32 = AtomicI32::new(0);

/// Default: keep blank nodes.
static KEEP_BLANKS_DEFAULT: AtomicI32 = AtomicI32::new(1);

/// Default: load external DTD subsets.
/// Bitmask of XML_DETECT_IDS, XML_COMPLETE_ATTRS, XML_SKIP_IDS.
static LOAD_EXT_DTD_DEFAULT: AtomicI32 = AtomicI32::new(0);

/// Default: pedantic parser mode.
static PEDANTIC_PARSER_DEFAULT: AtomicI32 = AtomicI32::new(0);

/// Default: substitute entities.
static SUBSTITUTE_ENTITIES_DEFAULT: AtomicI32 = AtomicI32::new(0);

/// Default: save empty tags.
static SAVE_NO_EMPTY_TAGS: AtomicI32 = AtomicI32::new(0);

/// Default: get warnings.
static GET_WARNINGS_DEFAULT: AtomicI32 = AtomicI32::new(1);

// ═══════════════════════════════════════════════════════════════════════════════
// Error Callback Globals
// ═══════════════════════════════════════════════════════════════════════════════

/// Generic error handler context pointer.
static GENERIC_ERROR_CTX: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

/// Generic error handler function pointer.
static GENERIC_ERROR_FUNC: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

/// Structured error handler context pointer.
static STRUCTURED_ERROR_CTX: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

/// Structured error handler function pointer.
static STRUCTURED_ERROR_FUNC: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

// ═══════════════════════════════════════════════════════════════════════════════
// Catalog Defaults
// ═══════════════════════════════════════════════════════════════════════════════

/// Catalog default allow value.
/// 0 = strict, 1 = allow, 2 = allow document, -1 = none.
static CATALOG_DEFAULTS: AtomicI32 = AtomicI32::new(0);

// ═══════════════════════════════════════════════════════════════════════════════
// Thread-Local Error State
// ═══════════════════════════════════════════════════════════════════════════════

thread_local! {
    /// Last error for this thread.
    static LAST_ERROR: RefCell<Option<_xmlError>> = const { RefCell::new(None) };
}

// ═══════════════════════════════════════════════════════════════════════════════
// Public Accessors — Parser Defaults
// ═══════════════════════════════════════════════════════════════════════════════

/// Get the default validity checking value.
pub fn get_validity_checking_default() -> c_int {
    VALIDITY_CHECKING_DEFAULT.load(Ordering::Relaxed)
}

/// Set the default validity checking value.
pub fn set_validity_checking_default(val: c_int) {
    VALIDITY_CHECKING_DEFAULT.store(val, Ordering::Relaxed);
}

/// Get the default warnings value.
pub fn get_do_warnings_default() -> c_int {
    DO_WARNINGS_DEFAULT.load(Ordering::Relaxed)
}

/// Set the default warnings value.
pub fn set_do_warnings_default(val: c_int) {
    DO_WARNINGS_DEFAULT.store(val, Ordering::Relaxed);
}

/// Get the indent tree output default.
pub fn get_indent_tree_output() -> c_int {
    INDENT_TREE_OUTPUT.load(Ordering::Relaxed)
}

/// Set the indent tree output default.
pub fn set_indent_tree_output(val: c_int) {
    INDENT_TREE_OUTPUT.store(val, Ordering::Relaxed);
}

/// Get the keep blanks default value.
pub fn get_keep_blanks_default() -> c_int {
    KEEP_BLANKS_DEFAULT.load(Ordering::Relaxed)
}

/// Set the keep blanks default value.
pub fn set_keep_blanks_default(val: c_int) {
    KEEP_BLANKS_DEFAULT.store(val, Ordering::Relaxed);
}

/// Get the load external DTD default value.
pub fn get_load_ext_dtd_default() -> c_int {
    LOAD_EXT_DTD_DEFAULT.load(Ordering::Relaxed)
}

/// Set the load external DTD default value.
pub fn set_load_ext_dtd_default(val: c_int) {
    LOAD_EXT_DTD_DEFAULT.store(val, Ordering::Relaxed);
}

/// Get the pedantic parser default.
pub fn get_pedantic_parser_default() -> c_int {
    PEDANTIC_PARSER_DEFAULT.load(Ordering::Relaxed)
}

/// Set the pedantic parser default.
pub fn set_pedantic_parser_default(val: c_int) {
    PEDANTIC_PARSER_DEFAULT.store(val, Ordering::Relaxed);
}

/// Get the substitute entities default.
pub fn get_substitute_entities_default() -> c_int {
    SUBSTITUTE_ENTITIES_DEFAULT.load(Ordering::Relaxed)
}

/// Set the substitute entities default.
pub fn set_substitute_entities_default(val: c_int) {
    SUBSTITUTE_ENTITIES_DEFAULT.store(val, Ordering::Relaxed);
}

/// Get the save no empty tags default.
pub fn get_save_no_empty_tags() -> c_int {
    SAVE_NO_EMPTY_TAGS.load(Ordering::Relaxed)
}

/// Set the save no empty tags default.
pub fn set_save_no_empty_tags(val: c_int) {
    SAVE_NO_EMPTY_TAGS.store(val, Ordering::Relaxed);
}

/// Get the get warnings default.
pub fn get_get_warnings_default() -> c_int {
    GET_WARNINGS_DEFAULT.load(Ordering::Relaxed)
}

/// Set the get warnings default.
pub fn set_get_warnings_default(val: c_int) {
    GET_WARNINGS_DEFAULT.store(val, Ordering::Relaxed);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Public Accessors — Error Callbacks
// ═══════════════════════════════════════════════════════════════════════════════

/// Set the generic error handler.
///
/// # SAFETY
///
/// - `handler` must be a valid function pointer or NULL (to reset to default).
/// - If non-NULL, the handler may be called at any time with `ctx`.
pub unsafe fn set_generic_error_func(ctx: *mut c_void, handler: Option<xmlGenericErrorFunc>) {
    GENERIC_ERROR_CTX.store(ctx, Ordering::Release);
    GENERIC_ERROR_FUNC.store(
        handler.map_or(ptr::null_mut(), |f| f as *mut c_void),
        Ordering::Release,
    );
}

/// Get the generic error handler context.
pub fn get_generic_error_ctx() -> *mut c_void {
    GENERIC_ERROR_CTX.load(Ordering::Acquire)
}

/// Get the generic error handler function pointer.
pub fn get_generic_error_func() -> Option<xmlGenericErrorFunc> {
    let ptr = GENERIC_ERROR_FUNC.load(Ordering::Acquire);
    if ptr.is_null() {
        None
    } else {
        // SAFETY: The pointer was set by set_generic_error_func and must be valid.
        Some(unsafe { core::mem::transmute::<*mut c_void, xmlGenericErrorFunc>(ptr) })
    }
}

/// Set the structured error handler.
///
/// # SAFETY
///
/// - `handler` must be a valid function pointer or NULL.
pub unsafe fn set_structured_error_func(ctx: *mut c_void, handler: Option<xmlStructuredErrorFunc>) {
    STRUCTURED_ERROR_CTX.store(ctx, Ordering::Release);
    STRUCTURED_ERROR_FUNC.store(
        handler.map_or(ptr::null_mut(), |f| f as *mut c_void),
        Ordering::Release,
    );
}

/// Get the structured error handler context.
pub fn get_structured_error_ctx() -> *mut c_void {
    STRUCTURED_ERROR_CTX.load(Ordering::Acquire)
}

/// Get the structured error handler function pointer.
pub fn get_structured_error_func() -> Option<xmlStructuredErrorFunc> {
    let ptr = STRUCTURED_ERROR_FUNC.load(Ordering::Acquire);
    if ptr.is_null() {
        None
    } else {
        // SAFETY: The pointer was set by set_structured_error_func and must be valid.
        Some(unsafe { core::mem::transmute::<*mut c_void, xmlStructuredErrorFunc>(ptr) })
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Public Accessors — Thread-Local Error State
// ═══════════════════════════════════════════════════════════════════════════════

/// Get the last error for this thread.
///
/// Returns a mutable pointer to the last error, or NULL if no error occurred.
/// The returned pointer is valid until the next libxml2 call in this thread.
pub fn get_last_error() -> *mut _xmlError {
    LAST_ERROR.with(|last| {
        let mut last = last.borrow_mut();
        last.as_mut()
            .map_or(ptr::null_mut(), |e| e as *mut _xmlError)
    })
}

/// Get a reference to the last error (for structured error callback).
pub fn with_last_error<F, R>(f: F) -> R
where
    F: FnOnce(Option<&_xmlError>) -> R,
{
    LAST_ERROR.with(|last| f(last.borrow().as_ref()))
}

/// Set the last error for this thread.
pub fn set_last_error(err: _xmlError) {
    LAST_ERROR.with(|last| {
        *last.borrow_mut() = Some(err);
    });
}

/// Reset the last error for this thread.
pub fn reset_last_error() {
    LAST_ERROR.with(|last| {
        *last.borrow_mut() = None;
    });
}

// ═══════════════════════════════════════════════════════════════════════════════
// Public Accessors — Catalog Defaults
// ═══════════════════════════════════════════════════════════════════════════════

/// Get the catalog default allow value.
pub fn get_catalog_defaults() -> c_int {
    CATALOG_DEFAULTS.load(Ordering::Relaxed)
}

/// Set the catalog default allow value.
pub fn set_catalog_defaults(val: c_int) {
    CATALOG_DEFAULTS.store(val, Ordering::Relaxed);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Initialization / Cleanup
// ═══════════════════════════════════════════════════════════════════════════════

/// Initialize the parser library.
///
/// Must be called before any other libxml2 functions.
/// Safe to call multiple times (reference-counted in modern libxml2).
///
/// # UPSTREAM-PARITY
///
/// In modern libxml2 (2.12+), `xmlInitParser` is reference-counted.
/// The first call initializes all subsystems; subsequent calls
/// increment a counter. `xmlCleanupParser` decrements the counter
/// and only performs cleanup when it reaches zero.
///
/// # SAFETY
///
/// Not fully thread-safe during the first call; callers should
/// call `xmlInitParser` before creating threads.
pub unsafe fn init_parser() {
    let prev = INIT_REF_COUNT.fetch_add(1, Ordering::AcqRel);
    if prev == 0 {
        // First initialization — initialize all subsystems.
        // 1. Initialize memory subsystem.
        allocator::xmlInitMemory();

        // 2. Mark the library as initialized.
        versioning::set_initialized();

        // 3. Initialize encoding handlers.
        crate::xml::encoding::init_encodings();

        // 4. Initialize thread support.
        init_threads();
    }
}

/// Clean up the parser library.
///
/// Should be called when the library is no longer needed.
/// Only performs actual cleanup when the reference count reaches zero.
///
/// # SAFETY
///
/// Must not be called while other libxml2 functions are executing
/// in any thread.
pub unsafe fn cleanup_parser() {
    let prev = INIT_REF_COUNT.fetch_sub(1, Ordering::AcqRel);
    if prev <= 1 {
        // Last cleanup — clean up all subsystems.
        // 1. Clean up catalog.
        crate::xml::catalog::cleanup();

        // 2. Clean up encoding handlers.
        crate::xml::encoding::cleanup_encodings();

        // 3. Clean up memory.
        allocator::xmlCleanupMemory();

        // 3. Reset initialization state.
        // Note: we do NOT reset the initialized flag in case
        // some code checks it after cleanup. This matches
        // upstream behavior where xmlCleanupParser is best-effort.
    }
}

/// Initialize threading support.
///
/// # UPSTREAM-PARITY
///
/// In modern libxml2, threading is initialized automatically
/// by `xmlInitParser`. This function exists for backward
/// compatibility.
///
/// Returns 0 on success.
pub fn init_threads() -> c_int {
    if !THREADS_INITIALIZED.swap(true, Ordering::Release) {
        // First initialization — no-op in Rust since we use
        // standard thread-safe primitives.
        // In libxml2 this would set up pthread mutexes.
    }
    0
}

/// Clean up threading support.
pub fn cleanup_threads() {
    THREADS_INITIALIZED.store(false, Ordering::Release);
}

/// Check whether threads have been initialized.
pub fn threads_initialized() -> bool {
    THREADS_INITIALIZED.load(Ordering::Acquire)
}

/// Get the current initialization reference count.
pub fn init_ref_count() -> c_int {
    INIT_REF_COUNT.load(Ordering::Relaxed)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parser_defaults_initial_values() {
        assert_eq!(get_validity_checking_default(), 0);
        assert_eq!(get_do_warnings_default(), 1);
        assert_eq!(get_indent_tree_output(), 0);
        assert_eq!(get_keep_blanks_default(), 1);
        assert_eq!(get_load_ext_dtd_default(), 0);
        assert_eq!(get_pedantic_parser_default(), 0);
        assert_eq!(get_substitute_entities_default(), 0);
        assert_eq!(get_save_no_empty_tags(), 0);
        assert_eq!(get_get_warnings_default(), 1);
    }

    #[test]
    fn test_parser_defaults_set_and_get() {
        set_validity_checking_default(1);
        assert_eq!(get_validity_checking_default(), 1);
        set_validity_checking_default(0);
        assert_eq!(get_validity_checking_default(), 0);

        set_keep_blanks_default(0);
        assert_eq!(get_keep_blanks_default(), 0);
        set_keep_blanks_default(1);
        assert_eq!(get_keep_blanks_default(), 1);

        set_substitute_entities_default(1);
        assert_eq!(get_substitute_entities_default(), 1);
        set_substitute_entities_default(0);
        assert_eq!(get_substitute_entities_default(), 0);
    }

    #[test]
    fn test_init_cleanup_ref_count() {
        // Reset for test
        unsafe {
            init_parser();
            assert_eq!(init_ref_count(), 1);

            init_parser();
            assert_eq!(init_ref_count(), 2);

            cleanup_parser();
            assert!(init_ref_count() == 1 || init_ref_count() == 0);

            // Final cleanup
            cleanup_parser();
        }
    }

    #[test]
    fn test_error_callbacks_default_null() {
        assert!(get_generic_error_func().is_none());
        assert!(get_structured_error_func().is_none());
    }

    #[test]
    fn test_error_callbacks_set_and_get() {
        unsafe {
            unsafe extern "C" fn dummy_handler(_ctx: *mut c_void, _msg: *const core::ffi::c_char) {}
            let dummy_func: xmlGenericErrorFunc = dummy_handler;
            let dummy_ctx: *mut c_void = &mut 0 as *mut i32 as *mut c_void;

            set_generic_error_func(dummy_ctx, Some(dummy_func));
            assert!(get_generic_error_func().is_some());
            assert_eq!(get_generic_error_ctx(), dummy_ctx);

            set_generic_error_func(ptr::null_mut(), None);
            assert!(get_generic_error_func().is_none());
        }
    }

    #[test]
    fn test_last_error_thread_local() {
        assert!(get_last_error().is_null());

        let err = _xmlError {
            domain: XML_FROM_PARSER,
            code: XML_ERR_OK as c_int,
            message: ptr::null_mut(),
            level: XML_ERR_NONE as c_int,
            file: ptr::null_mut(),
            line: 0,
            str1: ptr::null_mut(),
            str2: ptr::null_mut(),
            str3: ptr::null_mut(),
            int1: 0,
            int2: 0,
            ctxt: ptr::null_mut(),
            node: ptr::null_mut(),
        };
        set_last_error(err);
        assert!(!get_last_error().is_null());
        unsafe {
            assert_eq!((*get_last_error()).domain, XML_FROM_PARSER);
        }

        reset_last_error();
        assert!(get_last_error().is_null());
    }

    #[test]
    fn test_catalog_defaults() {
        // Save original value (may have been set by init_parser in other tests)
        let orig = get_catalog_defaults();
        set_catalog_defaults(1);
        assert_eq!(get_catalog_defaults(), 1);
        set_catalog_defaults(0);
        assert_eq!(get_catalog_defaults(), 0);
        // Restore
        set_catalog_defaults(orig);
    }
}
