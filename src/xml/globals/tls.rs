//! Thread-local storage for the TLS-era globals (upstream globals.c 2.15
//! `xmlGetThreadLocalStorage` model, LIBXML_THREAD_ENABLED).
//!
//! Upstream 2.15 keeps the parser defaults, the error-handler slots, the
//! node register/deregister hooks and the input/output create-filename
//! callbacks in **per-thread** storage; the public `xml*` names are
//! macro-aliases of `(*__xmlXxx())` accessor functions (xmlerror.h /
//! parser.h / xmlIO.h / tree.h) that return pointers into the thread-local
//! state. The executed oracle DSO exports ONLY the `__xml*` accessor
//! FUNCTIONS for these globals (`nm -D` shows no matching data symbols), so
//! a handler installed in one thread must not be observable from another —
//! the exact defect the HOSTILE-THREADS court (Phase 13, dimension 6)
//! attacks.
//!
//! This module is the single source of truth for those 18 globals. The
//! `__xml*` accessor exports (`crate::abi::data_globals`) return
//! `tls_ptr(&KEY)` — a pointer into the CURRENT thread's cell — and the
//! internal accessors (`crate::xml::globals`) read/write the same cells, so
//! there is exactly one storage per thread (no global-vs-TLS state split).
//!
//! NOT thread-local (plain global data in the executed oracle, exported as
//! `pub static mut` from `crate::abi::data_globals`): the allocator hooks
//! (`xmlMalloc`/`xmlFree`/`xmlRealloc`/`xmlMemStrdup`), `xmlParserVersion`,
//! `xmlDefaultSAXHandler`, `xmlDefaultSAXLocator`, the `xmlLastError`
//! mirror (R-000135), `xmlBufferAllocScheme`, `xmlDefaultBufferSize`,
//! `xmlParserMaxDepth`, `xmlParserDebugEntities` and the character tables.
//!
//! # Historical quirks & epochs
//!
//! The thread-local globals model is the 2.10+ lazy-globals era: upstream
//! globals.c gained `xmlGetThreadLocalStorage` in 2.10 (commit
//! `b8e1d0a8`, "Cleanup the globals handling"), and 2.15 with
//! LIBXML_THREAD_ENABLED keeps the parser defaults, error slots and node/IO
//! hooks per-thread with `__xml*` accessor FUNCTIONS exported instead of
//! data symbols. Pre-2.10 libxml2 exported plain global data (the
//! candidate's earlier CUSTODIAN_EXTENSION plane). The executed oracle
//! (system 2.15.3) uses the accessor model for exactly the 18 globals in
//! this module and plain data for everything else, so the candidate mirrors
//! that split; the pre-2.15 data-symbol surface for these 18 was removed in
//! Phase 13 (R-000190) rather than kept as a second source of truth.
//!
//! # Deliberate oddities
//!
//! `xmlGenericError`'s default is `xmlGenericErrorDefaultFunc` (the
//! R-000161 x86-64 variadic stderr shim) on x86-64 and NULL elsewhere —
//! upstream error.c defaults it to the stderr printer; the non-x86-64 NULL
//! is the shim-absent equivalent. The generic-error context is lazily
//! defaulted to `stderr` per thread by the default handler, exactly like
//! upstream error.c. The `(handler, ctx)` slot pairs are serialized under
//! `ERROR_HANDLER_LOCK` (R-000171) — same-thread only, since each thread
//! owns its slots; C consumers writing through `__xml*` pointers keep
//! upstream's documented racy TLS semantics.
//!
//! # Safety
//!
//! SAFETY-SCOPE: GLOBALS-TLS-001 — the cells are `UnsafeCell`s because a C
//! consumer can obtain a raw pointer through `__xml*()` (or the header
//! macros) and write it directly, exactly like upstream's racy TLS. The
//! helper functions below are the only Rust-side access path; each `unsafe`
//! block is a single dereference of a valid cell pointer that outlives the
//! thread (thread-local statics are never moved), mirroring upstream's
//! documented racy semantics for direct consumers. The (handler, ctx) slot
//! pairs are additionally serialized under `ERROR_HANDLER_LOCK` by the
//! callers in `crate::xml::globals` (R-000171).

use core::cell::UnsafeCell;
use core::ffi::c_void;
use std::os::raw::{c_char, c_int};

use crate::abi::callbacks::{xmlGenericErrorFunc, xmlStructuredErrorFunc};
use crate::abi::structs::{_xmlNode, _xmlOutputBuffer, _xmlParserInputBuffer};
use crate::abi::types::xmlChar;

/// Default `xmlTreeIndentString` ("  ", upstream globals.c
/// `xmlTreeIndentStringThrDef`).
const DEFAULT_TREE_INDENT_STRING: *const xmlChar = c"  ".as_ptr() as *const xmlChar;

// ── parser defaults (upstream globals.c XML_GLOBALS_PARSER) ──────────────────

thread_local! {
    /// `int xmlDoValidityCheckingDefaultValue` (default 0).
    pub(crate) static DO_VALIDITY: UnsafeCell<c_int> = const { UnsafeCell::new(0) };
    /// `int xmlGetWarningsDefaultValue` (default 1).
    pub(crate) static GET_WARNINGS: UnsafeCell<c_int> = const { UnsafeCell::new(1) };
    /// `int xmlLoadExtDtdDefaultValue` (default 0).
    pub(crate) static LOAD_EXT_DTD: UnsafeCell<c_int> = const { UnsafeCell::new(0) };
    /// `int xmlPedanticParserDefaultValue` (default 0).
    pub(crate) static PEDANTIC: UnsafeCell<c_int> = const { UnsafeCell::new(0) };
    /// `int xmlLineNumbersDefaultValue` (default 1).
    pub(crate) static LINE_NUMBERS: UnsafeCell<c_int> = const { UnsafeCell::new(1) };
    /// `int xmlKeepBlanksDefaultValue` (default 1).
    pub(crate) static KEEP_BLANKS: UnsafeCell<c_int> = const { UnsafeCell::new(1) };
    /// `int xmlSubstituteEntitiesDefaultValue` (default 0).
    pub(crate) static SUBSTITUTE_ENTITIES: UnsafeCell<c_int> = const { UnsafeCell::new(0) };
    /// `int xmlIndentTreeOutput` (default 1, XML_GLOBALS_OUTPUT).
    pub(crate) static INDENT_TREE_OUTPUT: UnsafeCell<c_int> = const { UnsafeCell::new(1) };
    /// `const xmlChar *xmlTreeIndentString` (default "  ", XML_GLOBALS_OUTPUT).
    pub(crate) static TREE_INDENT_STRING: UnsafeCell<*const xmlChar> =
        const { UnsafeCell::new(DEFAULT_TREE_INDENT_STRING) };
    /// `int xmlSaveNoEmptyTags` (default 0, XML_GLOBALS_OUTPUT).
    pub(crate) static SAVE_NO_EMPTY_TAGS: UnsafeCell<c_int> = const { UnsafeCell::new(0) };
}

// ── node / IO hooks (upstream globals.c XML_GLOBALS_READER / XML_GLOBALS_IO) ──

thread_local! {
    /// `xmlRegisterNodeFunc xmlRegisterNodeDefaultValue` (default NULL).
    pub(crate) static REGISTER_NODE: UnsafeCell<Option<unsafe extern "C" fn(*mut _xmlNode)>> =
        const { UnsafeCell::new(None) };
    /// `xmlDeregisterNodeFunc xmlDeregisterNodeDefaultValue` (default NULL).
    pub(crate) static DEREGISTER_NODE: UnsafeCell<Option<unsafe extern "C" fn(*mut _xmlNode)>> =
        const { UnsafeCell::new(None) };
    /// `xmlParserInputBufferCreateFilenameFunc xmlParserInputBufferCreateFilenameValue`
    /// (default NULL).
    pub(crate) static PARSER_INPUT_CREATE_FILENAME: UnsafeCell<
        Option<unsafe extern "C" fn(*const c_char, c_int) -> *mut _xmlParserInputBuffer>,
    > = const { UnsafeCell::new(None) };
    /// `xmlOutputBufferCreateFilenameFunc xmlOutputBufferCreateFilenameValue`
    /// (default NULL).
    pub(crate) static OUTPUT_CREATE_FILENAME: UnsafeCell<
        Option<
            unsafe extern "C" fn(
                *const c_char,
                crate::abi::structs::xmlCharEncodingHandlerPtr,
                c_int,
            ) -> *mut _xmlOutputBuffer,
        >,
    > = const { UnsafeCell::new(None) };
}

// ── error handler slots (upstream globals.c XML_GLOBALS_ERROR) ───────────────

#[cfg(target_arch = "x86_64")]
thread_local! {
    /// `xmlGenericErrorFunc xmlGenericError` (default
    /// `xmlGenericErrorDefaultFunc`, R-000161 variadic stderr shim).
    pub(crate) static GENERIC_ERROR: UnsafeCell<Option<xmlGenericErrorFunc>> = const {
        UnsafeCell::new(Some(crate::abi::data_globals::XML_GENERIC_ERROR_DEFAULT))
    };
    /// `void *xmlGenericErrorContext` (default NULL; the default handler
    /// lazily points it at stderr per thread, upstream error.c).
    pub(crate) static GENERIC_ERROR_CTX: UnsafeCell<*mut c_void> =
        const { UnsafeCell::new(core::ptr::null_mut()) };
    /// `xmlStructuredErrorFunc xmlStructuredError` (default NULL).
    pub(crate) static STRUCTURED_ERROR: UnsafeCell<Option<xmlStructuredErrorFunc>> =
        const { UnsafeCell::new(None) };
    /// `void *xmlStructuredErrorContext` (default NULL).
    pub(crate) static STRUCTURED_ERROR_CTX: UnsafeCell<*mut c_void> =
        const { UnsafeCell::new(core::ptr::null_mut()) };
}

#[cfg(not(target_arch = "x86_64"))]
thread_local! {
    /// `xmlGenericErrorFunc xmlGenericError` — no variadic shim off x86-64,
    /// so the default is NULL (matches the non-shim build of upstream).
    pub(crate) static GENERIC_ERROR: UnsafeCell<Option<xmlGenericErrorFunc>> =
        const { UnsafeCell::new(None) };
    /// `void *xmlGenericErrorContext` (default NULL).
    pub(crate) static GENERIC_ERROR_CTX: UnsafeCell<*mut c_void> =
        const { UnsafeCell::new(core::ptr::null_mut()) };
    /// `xmlStructuredErrorFunc xmlStructuredError` (default NULL).
    pub(crate) static STRUCTURED_ERROR: UnsafeCell<Option<xmlStructuredErrorFunc>> =
        const { UnsafeCell::new(None) };
    /// `void *xmlStructuredErrorContext` (default NULL).
    pub(crate) static STRUCTURED_ERROR_CTX: UnsafeCell<*mut c_void> =
        const { UnsafeCell::new(core::ptr::null_mut()) };
}

// ── helpers ──────────────────────────────────────────────────────────────────

/// Read the current thread's value of a TLS cell.
///
/// # Safety
///
/// SAFETY: the cell is a thread-local `UnsafeCell` that is never moved for
/// the lifetime of the thread; the read is a single dereference of a valid,
/// initialized cell (upstream's racy TLS semantics for direct consumers).
#[inline]
pub(crate) fn tls_get<T: Copy>(key: &'static std::thread::LocalKey<UnsafeCell<T>>) -> T {
    key.with(|c| unsafe { *c.get() })
}

/// Write the current thread's value of a TLS cell.
///
/// # Safety
///
/// SAFETY: as `tls_get` — a single dereference of a valid, initialized
/// thread-local cell; the written value is `T: Copy` and stored by value.
#[inline]
pub(crate) fn tls_set<T: Copy>(key: &'static std::thread::LocalKey<UnsafeCell<T>>, value: T) {
    key.with(|c| unsafe { *c.get() = value })
}

/// Return a pointer to the current thread's value of a TLS cell.
///
/// The pointer stays valid for the lifetime of the thread (thread-local
/// statics are never moved or destroyed before thread exit); C consumers
/// may read/write through it exactly as with upstream's `__xmlXxx()`
/// accessors. Writes through the pointer race with concurrent Rust-side
/// accesses by the same thread only if the C code itself re-enters the
/// library; upstream documents the same race.
///
/// # Safety
///
/// SAFETY: the returned pointer aliases the thread-local cell and is valid
/// for the current thread for the rest of its lifetime.
#[inline]
pub(crate) fn tls_ptr<T>(key: &'static std::thread::LocalKey<UnsafeCell<T>>) -> *mut T {
    key.with(|c| c.get())
}
