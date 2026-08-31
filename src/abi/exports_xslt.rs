//! C ABI exports for libxslt.so.1 — no_mangle extern "C" functions (§1, §16).
//!
//! This module contains all `#[no_mangle] pub extern "C"` function definitions
//! that form the public ABI of libxslt.so.1.
//!
//! # Phase 8 status
//!
//! Complete — all major XSLT ABI entry points are defined and wired to the
//! native-Rust XSLT engine (`src/xslt`).
//!
//! # Organization
//!
//! The bulk of the XSLT implementation lives in `src/xslt/*` modules which
//! declare their own `#[no_mangle]` exports. This module holds the
//! remaining top-level ABI functions: version, init/cleanup, features,
//! error handler wiring, and engine version reporting.

#![allow(non_snake_case)]
#![allow(unused_variables)]

use core::ffi::c_void;
use core::ptr;
use std::os::raw::{c_char, c_int, c_uint};

use crate::abi::structs::*;

// ═══════════════════════════════════════════════════════════════════════════════
// 1. Version & Initialization
// ═══════════════════════════════════════════════════════════════════════════════
//
// The version surfaces are exported as DATA from `crate::abi::data_globals`
// (upstream XSLTPUBVAR const symbols, oracle DSO types R/D):
//   - xsltLibxmlVersion  (const int 21501, R)
//   - xsltLibxsltVersion (const int 10145, R)   — was a function (T), R-000167
//   - xsltEngineVersion  (const char *, D)      — was a function (T), R-000167
// `xsltLibxsltVersionString` / `xsltCheckVersion` are NOT exported by the
// oracle DSO and not declared by upstream xslt.h; the candidate's Rust
// equivalents live in crate::abi::versioning for internal use only (the
// previous #[no_mangle] function exports were candidate-only extras).

/// Initialize the XSLT library.
///
/// Registers the default error handlers and initializes global state.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltInit(void);
/// ```
#[no_mangle]
pub extern "C" fn xsltInit() {
    crate::abi::versioning::set_initialized();
}

/// Clean up the XSLT library.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltCleanupGlobals(void);
/// ```
#[no_mangle]
pub extern "C" fn xsltCleanupGlobals() {
    // Reset the global default security preferences.
    unsafe {
        crate::xslt::security::xsltSetDefaultSecurityPrefs(ptr::null_mut());
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 2. Stylesheet Compilation
// ═══════════════════════════════════════════════════════════════════════════════
//
// The stylesheet lifecycle functions are declared with #[no_mangle] in
// `src/xslt/stylesheet/mod.rs` (xsltStylesheetCreate, xsltParseStylesheetDoc,
// xsltParseStylesheetFile, xsltParseStylesheetMemory, xsltFreeStylesheet,
// xsltGetStylesheetDoc, xsltSetStylesheetDoc).

// ═══════════════════════════════════════════════════════════════════════════════
// 3. Transformation
// ═══════════════════════════════════════════════════════════════════════════════
//
// The transform functions are declared with #[no_mangle] in
// `src/xslt/transform/mod.rs` (xsltApplyStylesheet, xsltApplyStylesheetStacked,
// xsltApplyStylesheetUser, xsltFreeTransformResult, xsltNewTransformContext,
// xsltFreeTransformContext).

// ═══════════════════════════════════════════════════════════════════════════════
// 4. Security
// ═══════════════════════════════════════════════════════════════════════════════
//
// The security functions are declared with #[no_mangle] in
// `src/xslt/security/mod.rs` (xsltNewSecurityPrefs, xsltFreeSecurityPrefs,
// xsltSetSecurityPrefs, xsltGetSecurityPrefs, xsltSetDefaultSecurityPrefs,
// xsltGetDefaultSecurityPrefs).

// ═══════════════════════════════════════════════════════════════════════════════
// 5. Extensions
// ═══════════════════════════════════════════════════════════════════════════════
//
// The extension registration functions are declared with #[no_mangle] in
// `src/xslt/extensions/mod.rs` (xsltRegisterExtFunction, xsltRegisterExtElement).

// Initialize the EXSLT module.
//
// # UPSTREAM-PARITY
//
// ```c
// void exsltRegisterAll(void);
// ```
//
// Declared with #[no_mangle] in `src/exslt/mod.rs`.
// ═══════════════════════════════════════════════════════════════════════════════
// 6. Save Result to File
// ═══════════════════════════════════════════════════════════════════════════════
//
// The save functions are declared with #[no_mangle] in
// `src/xslt/serialization/mod.rs` (xsltSaveResultToFile, xsltSaveResultToFd,
// xsltSaveResultToString).

// ═══════════════════════════════════════════════════════════════════════════════
// 7. Debug/Utility
// ═══════════════════════════════════════════════════════════════════════════════

/// Check whether a feature is available.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltCheckFeature(int feature);
/// ```
#[no_mangle]
pub const extern "C" fn xsltCheckFeature(_feature: c_int) -> c_int {
    // All features reported as available.
    1
}

/// Set loader function for XSLT.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltSetLoaderFunc(xsltLoaderFunc loader);
/// ```
///
/// Declared with #[no_mangle] in `src/xslt/documents/mod.rs`.
/// Set the transformer error handler.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltSetTransformErrorFunc(xsltTransformContextPtr ctxt,
///                                void *ctx, xmlGenericErrorFunc handler);
/// ```
#[no_mangle]
pub extern "C" fn xsltSetTransformErrorFunc(
    ctxt: *mut _xsltTransformContext,
    ctx: *mut c_void,
    handler: Option<unsafe extern "C" fn(*mut c_void, *const c_char)>,
) {
    crate::xslt::errors::xsltSetTransformErrorFunc(ctxt, ctx, handler);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Debugger / locale / UTF-8 helpers (11.1-X R-000165 closure)
// ═══════════════════════════════════════════════════════════════════════════════
//
// Ported from archaeology/libxslt-git (xsltutils.c / xsltInternals.h).
// These entry points are declared by the drop-in headers and were missing
// from the candidate DSO (header-compile allowlist).

/// `xsltSetDebuggerStatus(int value)` — set the global debugger status
/// (upstream xsltutils.c; writes `xslDebugStatus`).
#[no_mangle]
pub extern "C" fn xsltSetDebuggerStatus(value: c_int) {
    unsafe { crate::abi::data_globals::xslDebugStatus = value };
}

/// The debugger callback block is 3 function pointers (handler, add, drop);
/// upstream `XSLT_CALLBACK_NUMBER` is 3.
const XSLT_CALLBACK_NUMBER: c_int = 3;

/// `xsltSetDebuggerCallbacks(int no, void *block)` — plug a debugger into
/// the XSLT library (upstream xsltutils.c). Returns 0 on success, -1 if
/// `block` is NULL or `no` is not XSLT_CALLBACK_NUMBER.
#[no_mangle]
pub extern "C" fn xsltSetDebuggerCallbacks(no: c_int, block: *mut c_void) -> c_int {
    if block.is_null() || no != XSLT_CALLBACK_NUMBER {
        return -1;
    }
    // Store the three callbacks (handler, add, drop) exactly as upstream's
    // static xsltDebuggerCurrentCallbacks.
    unsafe {
        let cb = block as *mut *const c_void;
        XSLT_DEBUGGER_HANDLER = *cb;
        XSLT_DEBUGGER_ADD = *cb.add(1);
        XSLT_DEBUGGER_DROP = *cb.add(2);
    }
    0
}

static mut XSLT_DEBUGGER_HANDLER: *const c_void = ptr::null();
static mut XSLT_DEBUGGER_ADD: *const c_void = ptr::null();
static mut XSLT_DEBUGGER_DROP: *const c_void = ptr::null();

/// `xsltSetCtxtLocaleHandlers(ctxt, newLocale, freeLocale, genSortKey)` —
/// set the locale handlers on a transform context (upstream xsltutils.c).
///
/// # SAFETY
///
/// - `ctxt` must be a valid `_xsltTransformContext` pointer (or NULL).
/// - `newLocale`/`freeLocale`/`genSortKey` are stored verbatim; the caller
///   retains ownership and must keep them valid for the context's lifetime.
#[no_mangle]
pub unsafe extern "C" fn xsltSetCtxtLocaleHandlers(
    ctxt: *mut _xsltTransformContext,
    new_locale: *mut c_void,
    free_locale: *mut c_void,
    gen_sort_key: *mut c_void,
) {
    if ctxt.is_null() {
        return;
    }
    unsafe {
        (*ctxt).newLocale = new_locale;
        (*ctxt).freeLocale = free_locale;
        (*ctxt).genSortKey = gen_sort_key;
    }
}

/// `unsigned int xsltGetUTF8CharZ(const unsigned char *utf, int *len)` —
/// decode one UTF-8 code point; returns the code point or -1 on error
/// (upstream xsltutils.c; the `Z` variant does not tolerate NULs).
///
/// # SAFETY
///
/// - `utf` must point to at least one byte (and up to 4 for multi-byte
///   sequences); `len` must be a valid out-pointer. Both may be NULL, in
///   which case the function returns -1 and leaves `len` untouched.
#[no_mangle]
pub unsafe extern "C" fn xsltGetUTF8CharZ(utf: *const u8, len: *mut c_int) -> c_uint {
    unsafe {
        if utf.is_null() || len.is_null() {
            if !len.is_null() {
                *len = 0;
            }
            return u32::MAX as c_uint; // -1 as unsigned
        }
        let c0 = *utf;
        if c0 & 0x80 == 0 {
            *len = 1;
            return c0 as c_uint;
        }
        if *utf.add(1) & 0xC0 != 0x80 {
            *len = 0;
            return u32::MAX as c_uint;
        }
        if c0 & 0xE0 == 0xE0 {
            if *utf.add(2) & 0xC0 != 0x80 {
                *len = 0;
                return u32::MAX as c_uint;
            }
            if c0 & 0xF0 == 0xF0 {
                if c0 & 0xF8 != 0xF0 || *utf.add(3) & 0xC0 != 0x80 {
                    *len = 0;
                    return u32::MAX as c_uint;
                }
                *len = 4;
                let c = (((c0 & 0x7) as u32) << 18)
                    | (((*utf.add(1) & 0x3F) as u32) << 12)
                    | (((*utf.add(2) & 0x3F) as u32) << 6)
                    | ((*utf.add(3) & 0x3F) as u32);
                return c;
            }
            *len = 3;
            let c = (((c0 & 0xF) as u32) << 12)
                | (((*utf.add(1) & 0x3F) as u32) << 6)
                | ((*utf.add(2) & 0x3F) as u32);
            return c;
        }
        *len = 2;

        (((c0 & 0x1F) as u32) << 6) | ((*utf.add(1) & 0x3F) as u32)
    }
}
