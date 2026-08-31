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
use std::os::raw::{c_char, c_int};

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
// oracle DSO and not declared by upstream xslt.h; the candidate keeps Rust
// equivalents in crate::abi::versioning for internal use only (the
// previous #[no_mangle] function exports were candidate-only extras).
//
// # Upstream contract
//
// The parity target is the libxslt.so.1 export surface of the oracle DSO
// (libxslt 1.1.45): `xslt.c`, `xsltutils.c` and `transform.c` entry points
// with the `xslt.h`/`transform.h` signatures. Residuals R-000135, R-000136,
// R-000160, R-000161, R-000165, R-000166, R-000167 and R-000168 touch this
// module.
//
// # Conceptual behavior
//
// This module implements the top-level libxslt ABI: version reporting,
// init/cleanup, feature queries, error-handler wiring and the engine version
// data symbols. The bulk of the XSLT engine lives in `src/xslt/*` with its
// own `#[no_mangle]` exports; this module holds the rest.
//
// # Ownership & safety invariants
//
// The version surfaces are exported as DATA (`xsltLibxsltVersion` const int,
// R; `xsltEngineVersion` const char, D) matching the oracle nm -D types — a
// consumer reading them per the header contract gets the value, not code
// bytes (R-000167). Error-handler contexts are caller-kept user-data
// (OWNERSHIP_ATLAS section 6); global state is lazy-initialized like
// upstream.
//
// # Historical quirks & epochs
//
// E-008 (SEMANTIC_EPOCHS): the libxslt core transform output is byte-identical
// from 1.1.26 (2009) through 1.1.45 — a fully stable epoch. R-000167
// (11.1-S): `xsltLibxsltVersion` was a function (T) and became a data symbol
// (R); `xsltLibxsltVersionString` is a candidate-only extra and is
// deliberately not exported. R-000160: `xsltGetDebuggerStatus`/
// `xsltExtensionInstructionResultRegister` have trivial upstream bodies
// (return 0) dispositioned as intentional no-ops.
//
// # Deliberate oddities
//
// The missing `xsltLibxsltVersionString`/`xsltCheckVersion` exports (upstream
// has no such symbols — the previous function exports were candidate-only
// extras, removed for ABI honesty) and the R-000160 trivial-body no-ops are
// the deliberate oddities here.
//
// # Proving courts
//
// The BUILD-CONFIG-SCRIPT, CLI-XSLTPROC, EXSLT, ORACLE-IDENTITY,
// PREPROCESSOR-SURFACE and XSLT court families plus DSO-LOADER (symbol-type
// parity vs the oracle) and HEADER-COMPILE cover this module.
//
// # Tempting simplifications that would break parity
//
// A tempting simplification is to re-export `xsltLibxsltVersion` as a function
// for convenience — R-000167 proved the symbol type must match the oracle (R
// vs T) or C consumers reading the const int read code bytes. Another
// shortcut, adding convenience exports upstream does not have, pollutes the
// drop-in surface and would break the DSO-LOADER symbol-type parity check.

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
/// set the locale handlers on a transform context (upstream xsltutils.c;
/// typed callback parameters — R-000176, the candidate previously used
/// bare `void *`).
///
/// # SAFETY
///
/// - `ctxt` must be a valid `_xsltTransformContext` pointer (or NULL).
/// - `newLocale`/`freeLocale`/`genSortKey` are stored verbatim; the caller
///   retains ownership and must keep them valid for the context's lifetime.
#[no_mangle]
pub unsafe extern "C" fn xsltSetCtxtLocaleHandlers(
    ctxt: *mut _xsltTransformContext,
    new_locale: Option<crate::abi::exports_xslt_compile::xsltNewLocaleFunc>,
    free_locale: Option<crate::abi::exports_xslt_compile::xsltFreeLocaleFunc>,
    gen_sort_key: Option<crate::abi::exports_xslt_compile::xsltGenSortKeyFunc>,
) {
    if ctxt.is_null() {
        return;
    }
    unsafe {
        (*ctxt).newLocale = new_locale.map_or(ptr::null_mut(), |f| f as *mut c_void);
        (*ctxt).freeLocale = free_locale.map_or(ptr::null_mut(), |f| f as *mut c_void);
        (*ctxt).genSortKey = gen_sort_key.map_or(ptr::null_mut(), |f| f as *mut c_void);
    }
}

/// `int xsltGetUTF8CharZ(const unsigned char *utf, int *len)` — decode one
/// UTF-8 code point; returns the code point or -1 on error (upstream
/// xsltutils.c; the `Z` variant does not tolerate NULs). R-000176: the
/// candidate previously returned `unsigned` with -1 encoded as `UINT_MAX`.
///
/// # SAFETY
///
/// - `utf` must point to at least one byte (and up to 4 for multi-byte
///   sequences); `len` must be a valid out-pointer. Both may be NULL, in
///   which case the function returns -1 and leaves `len` untouched.
#[no_mangle]
pub unsafe extern "C" fn xsltGetUTF8CharZ(utf: *const u8, len: *mut c_int) -> c_int {
    unsafe {
        if utf.is_null() || len.is_null() {
            if !len.is_null() {
                *len = 0;
            }
            return -1;
        }
        let c0 = *utf;
        if c0 & 0x80 == 0 {
            *len = 1;
            return c0 as c_int;
        }
        if *utf.add(1) & 0xC0 != 0x80 {
            *len = 0;
            return -1;
        }
        if c0 & 0xE0 == 0xE0 {
            if *utf.add(2) & 0xC0 != 0x80 {
                *len = 0;
                return -1;
            }
            if c0 & 0xF0 == 0xF0 {
                if c0 & 0xF8 != 0xF0 || *utf.add(3) & 0xC0 != 0x80 {
                    *len = 0;
                    return -1;
                }
                *len = 4;
                let c = (((c0 & 0x7) as u32) << 18)
                    | (((*utf.add(1) & 0x3F) as u32) << 12)
                    | (((*utf.add(2) & 0x3F) as u32) << 6)
                    | ((*utf.add(3) & 0x3F) as u32);
                return c as c_int;
            }
            *len = 3;
            let c = (((c0 & 0xF) as u32) << 12)
                | (((*utf.add(1) & 0x3F) as u32) << 6)
                | ((*utf.add(2) & 0x3F) as u32);
            return c as c_int;
        }
        *len = 2;

        ((((c0 & 0x1F) as u32) << 6) | ((*utf.add(1) & 0x3F) as u32)) as c_int
    }
}
