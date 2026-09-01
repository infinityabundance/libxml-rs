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
//!
//! # Upstream contract
//!
//! Mirrors upstream globals.c (SRC-LIBXML2-2.15.0-GLOBALS-C, oracle tree
//! `oracle/historical/src/libxml2-2.15.0/globals.c`): the library-wide global
//! variables (xmlDoValidityCheckingDefaultValue, xmlLoadExtDtdDefaultValue,
//! xmlKeepBlanksDefaultValue, xmlGenericError, xmlStructuredError,
//! xmlDefaultSAXHandler, xmlLastError, xmlParserVersion, ...) exposed as
//! public ABI data symbols (R-000135).
//!
//! Phase 13 (HOSTILE-THREADS): the 2.15 LIBXML_THREAD_ENABLED model keeps the
//! parser defaults, the error-handler slots and the node/IO hooks in
//! THREAD-LOCAL storage (globals.c `xmlGetThreadLocalStorage`); the oracle
//! DSO exports only the `__xml*` accessor functions for them. The candidate
//! mirrors that model with `tls` cells (single source of truth per thread);
//! the C-visible data symbols were removed for those 18 globals and the
//! candidate headers now use the upstream macro/accessor contract.
//! Everything else (`xmlParserVersion`, `xmlDefaultSAXHandler`, the
//! allocator hooks, the `xmlLastError` mirror, ...) remains plain global
//! data, matching the executed oracle.
//!
//! # Conceptual behavior
//!
//! Manages parser defaults, generic/structured error callbacks, catalog
//! defaults, memory hooks, thread-local error state and init/cleanup
//! reference counting. The exported data symbols are wired to the parser-
//! default accessors so there is a single source of truth.
//!
//! # Ownership & safety invariants
//!
//! SAFETY: the TLS-era globals are per-thread cells (see `tls`); the
//! (handler, ctx) slot pairs are serialized under ERROR_HANDLER_LOCK so
//! readers never observe a new handler with an old context (R-000171) — the
//! lock now serializes same-thread set/get, since each thread owns its own
//! slots. The xmlLastError mirror is deep- copied under
//! LAST_ERROR_MIRROR_LOCK (R-000170: concurrent sync/reset double-freed the
//! mirror strings). Error state is thread-local.
//!
//! # Historical quirks & epochs
//!
//! Thread support predates the thread-local-globals era: globals.c threading
//! integrated 2001-10-12/13 (commits b847864f, d0463560; LORE-0005). Modern
//! 2.10+ initialization is lazy; deprecated init/cleanup entry points are
//! genuine no-ops (R-000138). R-000161 fixed the exported default values
//! (xmlLineNumbersDefaultValue 1, xmlTreeIndentString two spaces,
//! xmlParserVersion 21503-GITv2.15.3).
//!
//! # Deliberate oddities
//!
//! Deliberate oddities: xmlGenericError and xsltGenericError default to the
//! variadic stderr printers (asm va_list shims; R-000161) rather than NULL;
//! the deprecated init entry points are exported no-ops reproducing upstream
//! empty bodies (R-000138).
//!
//! # Proving courts
//!
//! ABI-DATA, ALLOCATOR, GLOBAL-STATE and THREADING court families;
//! DATA-GLOBALS-001 differential probe (byte-identical), GLOBALS-THREADING
//! probe, DSO-LOADER, and `cargo test --lib` (100/100 parallel runs
//! clean after R-000170/R-000171).
//!
//! # Tempting simplifications that would break parity
//!
//! The tempting simplification is hiding the C globals behind accessors only
//! — downstream code that reads/writes xmlDoValidityCheckingDefaultValue
//! directly would fail to link (R-000135). Do not fix the racy C-visible
//! symbols: the internal locks must not change the documented upstream
//! semantics for direct consumers.
//!
//! # Safety
//!
//! - The module-level statics are synchronization primitives only
//!   (atomics, a `parking_lot` mutex, and a `thread_local` cell); there are
//!   no top-level `unsafe` blocks outside functions. Reads and writes of
//!   the exported C globals in `crate::abi::data_globals` happen only
//!   inside the accessor functions, where the `(handler, ctx)` slot pairs
//!   are serialized under `ERROR_HANDLER_LOCK` so a reader never observes a
//!   new handler with an old context. The thread-local `LAST_ERROR` slot is
//!   only mutated through `set_last_error`/`reset_last_error`, which free
//!   the previous slot's owned strings exactly once.
use core::cell::RefCell;
use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::os::raw::{c_char, c_int};

use crate::abi::allocator;
use crate::abi::callbacks::{xmlGenericErrorFunc, xmlStructuredErrorFunc};
use crate::abi::structs::{_xmlError, _xmlNode, _xmlOutputBuffer, _xmlParserInputBuffer};
use crate::abi::types::xmlChar;
use crate::abi::versioning;

pub mod tls;
use tls::{tls_get, tls_ptr, tls_set};

/// Serializes the exported error-handler slot pairs
/// (`xmlGenericError`/`xmlGenericErrorContext` and
/// `xmlStructuredError`/`xmlStructuredErrorContext`). Upstream's globals are
/// bare racy `static mut` slots; the candidate keeps the C-visible symbols
/// but makes internal set/get atomic as a (handler, ctx) pair so readers
/// never observe a new handler with an old context (or vice versa). C
/// consumers that touch the symbols directly keep upstream's documented
/// racy semantics.
static ERROR_HANDLER_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

/// Serializes the error-handler tests that mutate the shared handler slots
/// (11.1-X regression court wiring): `test_error_callbacks_*` (globals) and
/// `test_structured_error_callback` (errors) must not run concurrently, or
/// `test_error_callbacks_default_handlers` observes another test's
/// temporarily-installed structured handler.
#[cfg(test)]
pub(crate) static ERROR_HANDLER_TEST_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

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
// ═══════════════════════════════════════════════════════════════════════════════
// Parser Defaults
// ═══════════════════════════════════════════════════════════════════════════════
//
// The defaults live in the EXPORTED C globals (src/abi/data_globals.rs) so
// that downstream C code reading/writing them directly (the upstream
// contract) observes and controls the same state the parser uses. The
// accessors below are the safe-Rust view of those statics.

// ═══════════════════════════════════════════════════════════════════════════════
// Error Callback Globals
// ═══════════════════════════════════════════════════════════════════════════════
// Also exported as C globals (xmlGenericError/xmlGenericErrorContext,
// xmlStructuredError/xmlStructuredErrorContext).

// ═══════════════════════════════════════════════════════════════════════════════
// Catalog Defaults
// ═══════════════════════════════════════════════════════════════════════════════

/// Catalog default allow value.
/// 0 = strict, 1 = allow, 2 = allow document, -1 = none.
/// Internal only (upstream keeps catalog state inside xmlCatalogSetDefaults;
/// there is no public C global for it).
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

/// Get the default validity checking value (thread-local, upstream 2.15).
pub fn get_validity_checking_default() -> c_int {
    tls_get(&tls::DO_VALIDITY)
}

/// Set the default validity checking value (thread-local, upstream 2.15).
pub fn set_validity_checking_default(val: c_int) {
    tls_set(&tls::DO_VALIDITY, val);
}

/// Get the default warnings value (thread-local, upstream 2.15).
pub fn get_do_warnings_default() -> c_int {
    tls_get(&tls::GET_WARNINGS)
}

/// Set the default warnings value (thread-local, upstream 2.15).
pub fn set_do_warnings_default(val: c_int) {
    tls_set(&tls::GET_WARNINGS, val);
}

/// Get the indent tree output default (thread-local, upstream 2.15).
pub fn get_indent_tree_output() -> c_int {
    tls_get(&tls::INDENT_TREE_OUTPUT)
}

/// Set the indent tree output default (thread-local, upstream 2.15).
pub fn set_indent_tree_output(val: c_int) {
    tls_set(&tls::INDENT_TREE_OUTPUT, val);
}

/// Get the keep blanks default value (thread-local, upstream 2.15).
pub fn get_keep_blanks_default() -> c_int {
    tls_get(&tls::KEEP_BLANKS)
}

/// Set the keep blanks default value (thread-local, upstream 2.15).
pub fn set_keep_blanks_default(val: c_int) {
    tls_set(&tls::KEEP_BLANKS, val);
}

/// Get the load external DTD default value (thread-local, upstream 2.15).
pub fn get_load_ext_dtd_default() -> c_int {
    tls_get(&tls::LOAD_EXT_DTD)
}

/// Set the load external DTD default value (thread-local, upstream 2.15).
pub fn set_load_ext_dtd_default(val: c_int) {
    tls_set(&tls::LOAD_EXT_DTD, val);
}

/// Get the pedantic parser default (thread-local, upstream 2.15).
pub fn get_pedantic_parser_default() -> c_int {
    tls_get(&tls::PEDANTIC)
}

/// Set the pedantic parser default (thread-local, upstream 2.15).
pub fn set_pedantic_parser_default(val: c_int) {
    tls_set(&tls::PEDANTIC, val);
}

/// Get the substitute entities default (thread-local, upstream 2.15).
pub fn get_substitute_entities_default() -> c_int {
    tls_get(&tls::SUBSTITUTE_ENTITIES)
}

/// Set the substitute entities default (thread-local, upstream 2.15).
pub fn set_substitute_entities_default(val: c_int) {
    tls_set(&tls::SUBSTITUTE_ENTITIES, val);
}

/// Get the save no empty tags default (thread-local, upstream 2.15).
pub fn get_save_no_empty_tags() -> c_int {
    tls_get(&tls::SAVE_NO_EMPTY_TAGS)
}

/// Set the save no empty tags default (thread-local, upstream 2.15).
pub fn set_save_no_empty_tags(val: c_int) {
    tls_set(&tls::SAVE_NO_EMPTY_TAGS, val);
}

/// Get the get warnings default (thread-local, upstream 2.15).
pub fn get_get_warnings_default() -> c_int {
    tls_get(&tls::GET_WARNINGS)
}

/// Set the get warnings default (thread-local, upstream 2.15).
pub fn set_get_warnings_default(val: c_int) {
    tls_set(&tls::GET_WARNINGS, val);
}

/// Get `xmlLineNumbersDefaultValue` (thread-local, upstream 2.15).
pub fn get_line_numbers_default() -> c_int {
    tls_get(&tls::LINE_NUMBERS)
}

/// Set `xmlLineNumbersDefaultValue` (thread-local, upstream 2.15).
pub fn set_line_numbers_default(val: c_int) {
    tls_set(&tls::LINE_NUMBERS, val);
}

/// Get `xmlTreeIndentString` (thread-local, upstream 2.15).
pub fn get_tree_indent_string() -> *const xmlChar {
    tls_get(&tls::TREE_INDENT_STRING)
}

/// Set `xmlTreeIndentString` (thread-local, upstream 2.15).
pub fn set_tree_indent_string(val: *const xmlChar) {
    tls_set(&tls::TREE_INDENT_STRING, val);
}

/// Get `xmlRegisterNodeDefaultValue` (thread-local, upstream 2.15).
pub fn get_register_node_default() -> Option<unsafe extern "C" fn(*mut _xmlNode)> {
    tls_get(&tls::REGISTER_NODE)
}

/// Set `xmlRegisterNodeDefaultValue` (thread-local, upstream 2.15).
pub fn set_register_node_default(val: Option<unsafe extern "C" fn(*mut _xmlNode)>) {
    tls_set(&tls::REGISTER_NODE, val);
}

/// Get `xmlDeregisterNodeDefaultValue` (thread-local, upstream 2.15).
pub fn get_deregister_node_default() -> Option<unsafe extern "C" fn(*mut _xmlNode)> {
    tls_get(&tls::DEREGISTER_NODE)
}

/// Set `xmlDeregisterNodeDefaultValue` (thread-local, upstream 2.15).
pub fn set_deregister_node_default(val: Option<unsafe extern "C" fn(*mut _xmlNode)>) {
    tls_set(&tls::DEREGISTER_NODE, val);
}

/// Get `xmlParserInputBufferCreateFilenameValue` (thread-local, upstream
/// 2.15).
pub fn get_parser_input_buffer_create_filename_value(
) -> Option<unsafe extern "C" fn(*const c_char, c_int) -> *mut _xmlParserInputBuffer> {
    tls_get(&tls::PARSER_INPUT_CREATE_FILENAME)
}

/// Set `xmlParserInputBufferCreateFilenameValue` (thread-local, upstream
/// 2.15).
pub fn set_parser_input_buffer_create_filename_value(
    val: Option<unsafe extern "C" fn(*const c_char, c_int) -> *mut _xmlParserInputBuffer>,
) {
    tls_set(&tls::PARSER_INPUT_CREATE_FILENAME, val);
}

/// Get `xmlOutputBufferCreateFilenameValue` (thread-local, upstream 2.15).
pub fn get_output_buffer_create_filename_value() -> Option<
    unsafe extern "C" fn(
        *const c_char,
        crate::abi::structs::xmlCharEncodingHandlerPtr,
        c_int,
    ) -> *mut _xmlOutputBuffer,
> {
    tls_get(&tls::OUTPUT_CREATE_FILENAME)
}

/// Set `xmlOutputBufferCreateFilenameValue` (thread-local, upstream 2.15).
pub fn set_output_buffer_create_filename_value(
    val: Option<
        unsafe extern "C" fn(
            *const c_char,
            crate::abi::structs::xmlCharEncodingHandlerPtr,
            c_int,
        ) -> *mut _xmlOutputBuffer,
    >,
) {
    tls_set(&tls::OUTPUT_CREATE_FILENAME, val);
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
    // SAFETY: writing the per-thread error-handler slots
    // xmlGenericErrorContext / xmlGenericError (TLS, upstream 2.15
    // xmlSetGenericErrorFunc: NULL resets to the built-in default stderr
    // printer, error.c). The (ctx, func) pair is written atomically under
    // ERROR_HANDLER_LOCK (same-thread serialization, R-000171).
    let resolved = match handler {
        Some(h) => Some(h),
        None => crate::abi::data_globals::default_generic_error_func(),
    };
    let _guard = ERROR_HANDLER_LOCK.lock();
    tls_set(&tls::GENERIC_ERROR_CTX, ctx);
    tls_set(&tls::GENERIC_ERROR, resolved);
}

/// Set the generic error handler context (thread-local; used by the default
/// stderr printer to lazily default the context per thread, upstream
/// error.c).
pub fn set_generic_error_ctx(ctx: *mut c_void) {
    let _guard = ERROR_HANDLER_LOCK.lock();
    tls_set(&tls::GENERIC_ERROR_CTX, ctx);
}

/// Get the generic error handler context (thread-local).
pub fn get_generic_error_ctx() -> *mut c_void {
    let _guard = ERROR_HANDLER_LOCK.lock();
    tls_get(&tls::GENERIC_ERROR_CTX)
}

/// Get the generic error handler function pointer (thread-local).
pub fn get_generic_error_func() -> Option<xmlGenericErrorFunc> {
    let _guard = ERROR_HANDLER_LOCK.lock();
    tls_get(&tls::GENERIC_ERROR)
}

/// Read the generic error (func, ctx) pair atomically.
///
/// The closure runs after the lock is released, so a handler installed
/// by the callback cannot deadlock.
pub fn with_generic_error<R>(f: impl FnOnce(Option<xmlGenericErrorFunc>, *mut c_void) -> R) -> R {
    let (h, c) = {
        let _guard = ERROR_HANDLER_LOCK.lock();
        (
            tls_get(&tls::GENERIC_ERROR),
            tls_get(&tls::GENERIC_ERROR_CTX),
        )
    };
    f(h, c)
}

/// Set the structured error handler (thread-local, upstream 2.15).
///
/// # SAFETY
///
/// - `handler` must be a valid function pointer or NULL.
pub unsafe fn set_structured_error_func(ctx: *mut c_void, handler: Option<xmlStructuredErrorFunc>) {
    // SAFETY: writing the per-thread error-handler slots
    // xmlStructuredErrorContext / xmlStructuredError (TLS, upstream 2.15
    // xmlSetStructuredErrorFunc). The (ctx, func) pair is written
    // atomically under ERROR_HANDLER_LOCK (same-thread serialization,
    // R-000171).
    let _guard = ERROR_HANDLER_LOCK.lock();
    tls_set(&tls::STRUCTURED_ERROR_CTX, ctx);
    tls_set(&tls::STRUCTURED_ERROR, handler);
}

/// Get the structured error handler context (thread-local).
pub fn get_structured_error_ctx() -> *mut c_void {
    let _guard = ERROR_HANDLER_LOCK.lock();
    tls_get(&tls::STRUCTURED_ERROR_CTX)
}

/// Get the structured error handler function pointer (thread-local).
pub fn get_structured_error_func() -> Option<xmlStructuredErrorFunc> {
    let _guard = ERROR_HANDLER_LOCK.lock();
    tls_get(&tls::STRUCTURED_ERROR)
}

/// Read the structured error (func, ctx) pair atomically.
///
/// The closure runs after the lock is released, so a handler installed by
/// the callback (or an error raised from inside the handler) cannot
/// deadlock on ERROR_HANDLER_LOCK.
///
/// # Safety
///
/// - The two reads of the per-thread `xmlStructuredError`/
///   `xmlStructuredErrorContext` TLS slots are performed under
///   `ERROR_HANDLER_LOCK`, matching the write side (xmlSetStructuredError),
///   so the pair is observed atomically; the values are only borrowed for the
///   duration of the closure and never dereferenced here.
pub fn with_structured_error<R>(
    f: impl FnOnce(Option<xmlStructuredErrorFunc>, *mut c_void) -> R,
) -> R {
    let (h, c) = {
        let _guard = ERROR_HANDLER_LOCK.lock();
        (
            tls_get(&tls::STRUCTURED_ERROR),
            tls_get(&tls::STRUCTURED_ERROR_CTX),
        )
    };
    f(h, c)
}

// ═══════════════════════════════════════════════════════════════════════════════
// `__xmlXxx()` pointer accessors (upstream threads.c / globals.c)
// ═══════════════════════════════════════════════════════════════════════════════
// The deprecated thread-local API exports one `__xmlXxx(void)` accessor per
// TLS-era global; each returns a pointer to the CURRENT thread's slot so
// callers can read/write it directly (upstream `#define xmlXxx
// (*__xmlXxx())`). The candidate's exports (`crate::abi::data_globals`)
// delegate here; the returned pointers alias the `tls` cells and stay valid
// for the lifetime of the thread.

pub(crate) fn do_validity_ptr() -> *mut c_int {
    tls_ptr(&tls::DO_VALIDITY)
}
pub(crate) fn get_warnings_ptr() -> *mut c_int {
    tls_ptr(&tls::GET_WARNINGS)
}
pub(crate) fn load_ext_dtd_ptr() -> *mut c_int {
    tls_ptr(&tls::LOAD_EXT_DTD)
}
pub(crate) fn pedantic_ptr() -> *mut c_int {
    tls_ptr(&tls::PEDANTIC)
}
pub(crate) fn line_numbers_ptr() -> *mut c_int {
    tls_ptr(&tls::LINE_NUMBERS)
}
pub(crate) fn keep_blanks_ptr() -> *mut c_int {
    tls_ptr(&tls::KEEP_BLANKS)
}
pub(crate) fn substitute_entities_ptr() -> *mut c_int {
    tls_ptr(&tls::SUBSTITUTE_ENTITIES)
}
pub(crate) fn indent_tree_output_ptr() -> *mut c_int {
    tls_ptr(&tls::INDENT_TREE_OUTPUT)
}
pub(crate) fn tree_indent_string_ptr() -> *mut *const xmlChar {
    tls_ptr(&tls::TREE_INDENT_STRING)
}
pub(crate) fn save_no_empty_tags_ptr() -> *mut c_int {
    tls_ptr(&tls::SAVE_NO_EMPTY_TAGS)
}
pub(crate) fn register_node_ptr() -> *mut Option<unsafe extern "C" fn(*mut _xmlNode)> {
    tls_ptr(&tls::REGISTER_NODE)
}
pub(crate) fn deregister_node_ptr() -> *mut Option<unsafe extern "C" fn(*mut _xmlNode)> {
    tls_ptr(&tls::DEREGISTER_NODE)
}
pub(crate) fn parser_input_create_filename_ptr(
) -> *mut Option<unsafe extern "C" fn(*const c_char, c_int) -> *mut _xmlParserInputBuffer> {
    tls_ptr(&tls::PARSER_INPUT_CREATE_FILENAME)
}
pub(crate) fn output_create_filename_ptr() -> *mut Option<
    unsafe extern "C" fn(
        *const c_char,
        crate::abi::structs::xmlCharEncodingHandlerPtr,
        c_int,
    ) -> *mut _xmlOutputBuffer,
> {
    tls_ptr(&tls::OUTPUT_CREATE_FILENAME)
}
pub(crate) fn generic_error_ptr() -> *mut Option<xmlGenericErrorFunc> {
    tls_ptr(&tls::GENERIC_ERROR)
}
pub(crate) fn generic_error_ctx_ptr() -> *mut *mut c_void {
    tls_ptr(&tls::GENERIC_ERROR_CTX)
}
pub(crate) fn structured_error_ptr() -> *mut Option<xmlStructuredErrorFunc> {
    tls_ptr(&tls::STRUCTURED_ERROR)
}
pub(crate) fn structured_error_ctx_ptr() -> *mut *mut c_void {
    tls_ptr(&tls::STRUCTURED_ERROR_CTX)
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
    // UPSTREAM-PARITY: mirror the error into the exported C global
    // `xmlLastError` (data-ABI, residual R-000135). Upstream keeps a single
    // global; the candidate keeps a thread-local truth and a deep-copied
    // mirror so C consumers see the most recent error with upstream
    // lifetime semantics (mirror strings are owned by the mirror and freed
    // on reset, matching xmlResetError).
    //
    // SAFETY: sync_xml_last_error only reads `err` and writes the global
    // mirror with freshly owned copies; the thread-local slot takes
    // ownership of `err` itself.
    unsafe { crate::abi::data_globals::sync_xml_last_error(&err) };
    LAST_ERROR.with(|last| {
        let mut last = last.borrow_mut();
        // Free the previous slot's owned strings (upstream xmlResetError).
        if let Some(prev) = last.as_ref() {
            free_error_strings(prev);
        }
        *last = Some(err);
    });
}

/// Free the owned string fields of a stored error (upstream xmlResetError:
/// message/file/str1/str2/str3 are xmlMalloc'd copies).
///
/// # Safety
///
/// - `err` must point to a valid `_xmlError` whose `message`, `file`,
///   `str1`, `str2` and `str3` fields are NULL or pointers allocated with
///   `xmlFreeImpl`'s allocator; each non-NULL field is freed exactly once
///   and must not be freed or used again afterwards.
pub(crate) fn free_error_strings(err: &_xmlError) {
    use crate::abi::allocator::xmlFreeImpl;
    unsafe {
        if !err.message.is_null() {
            xmlFreeImpl(err.message as *mut core::ffi::c_void);
        }
        if !err.file.is_null() {
            xmlFreeImpl(err.file as *mut core::ffi::c_void);
        }
        if !err.str1.is_null() {
            xmlFreeImpl(err.str1 as *mut core::ffi::c_void);
        }
        if !err.str2.is_null() {
            xmlFreeImpl(err.str2 as *mut core::ffi::c_void);
        }
        if !err.str3.is_null() {
            xmlFreeImpl(err.str3 as *mut core::ffi::c_void);
        }
    }
}

/// Reset the last error for this thread.
pub fn reset_last_error() {
    LAST_ERROR.with(|last| {
        let mut last = last.borrow_mut();
        if let Some(prev) = last.as_ref() {
            free_error_strings(prev);
        }
        *last = None;
    });
    // SAFETY: frees the mirror's owned strings and zeroes the global.
    unsafe { crate::abi::data_globals::reset_xml_last_error() };
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
    use crate::abi::types::xmlErrorLevel::XML_ERR_NONE;
    use crate::abi::types::*;

    #[test]
    fn test_parser_defaults_initial_values() {
        assert_eq!(get_validity_checking_default(), 0);
        assert_eq!(get_do_warnings_default(), 1);
        // UPSTREAM-PARITY (globals.c 2.15): xmlIndentTreeOutputThrDef = 1.
        assert_eq!(get_indent_tree_output(), 1);
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

    /// Increment and decrement the init reference count.
    ///
    /// # Safety
    ///
    /// - `init_parser`/`cleanup_parser` mutate the global reference count
    ///   and may run subsystem init/cleanup; the test balances the two
    ///   calls so the library is left in a clean state, and the reference
    ///   count is only read between calls.
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
    fn test_error_callbacks_default_handlers() {
        // UPSTREAM-PARITY (error.c): xmlGenericError defaults to the built-in
        // stderr printer (never NULL); xmlStructuredError defaults to NULL.
        // Serialized against the other handler-mutating tests (11.1-X): the
        // slots are shared global state.
        let _guard = ERROR_HANDLER_TEST_LOCK.lock();
        #[cfg(target_arch = "x86_64")]
        assert!(get_generic_error_func().is_some());
        assert!(get_structured_error_func().is_none());
    }

    /// Install and reset the generic error handler slot pair.
    ///
    /// # Safety
    ///
    /// - `dummy_handler` is a valid no-op callback pointer; `dummy_ctx`
    ///   points to a stack `i32` alive for the test; `set_generic_error_func`
    ///   writes the exported slot pair under the handler lock, and NULL
    ///   resets to the built-in default printer.
    #[test]
    fn test_error_callbacks_set_and_get() {
        let _guard = ERROR_HANDLER_TEST_LOCK.lock();
        unsafe {
            /// A no-op generic error callback used as a handler pointer.
            ///
            /// # Safety
            ///
            /// - The function is never invoked by this test; when installed as a
            ///   handler it must be a valid function pointer, and the parameters
            ///   would need to be valid C pointers if it were called.
            unsafe extern "C" fn dummy_handler(_ctx: *mut c_void, _msg: *const core::ffi::c_char) {}
            let dummy_func: xmlGenericErrorFunc = dummy_handler;
            let dummy_ctx: *mut c_void = &mut 0 as *mut i32 as *mut c_void;

            set_generic_error_func(dummy_ctx, Some(dummy_func));
            assert!(get_generic_error_func().is_some());
            assert_eq!(get_generic_error_ctx(), dummy_ctx);

            // UPSTREAM-PARITY (xmlSetGenericErrorFunc): NULL resets to the
            // built-in default printer, it does not unset the handler.
            set_generic_error_func(ptr::null_mut(), None);
            #[cfg(target_arch = "x86_64")]
            assert!(get_generic_error_func().is_some());
            assert_eq!(get_generic_error_ctx(), ptr::null_mut());
        }
    }

    /// Store and reset a thread-local last error.
    ///
    /// # Safety
    ///
    /// - `err` has all NULL string fields, so `set_last_error` stores it
    ///   without owning heap strings and `reset_last_error` frees nothing;
    ///   `get_last_error` returns a pointer valid while the thread-local
    ///   slot is not mutated.
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
