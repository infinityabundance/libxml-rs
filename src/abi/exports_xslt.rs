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

use crate::abi::allocator::*;
use crate::abi::structs::*;
use crate::abi::types::*;

// ═══════════════════════════════════════════════════════════════════════════════
// 1. Version & Initialization
// ═══════════════════════════════════════════════════════════════════════════════

/// Get the libxslt version as an integer.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltLibxsltVersion(void);
/// ```
#[no_mangle]
pub extern "C" fn xsltLibxsltVersion() -> c_int {
    crate::abi::versioning::LIBXSLT_VERSION_NUM
}

/// Get the libxslt version as a string.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// const char *xsltLibxsltVersionString(void);
/// ```
#[no_mangle]
pub extern "C" fn xsltLibxsltVersionString() -> *const c_char {
    crate::abi::versioning::xsltLibxsltVersionString()
}

/// Check the libxslt version.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltCheckVersion(int version);
/// ```
#[no_mangle]
pub extern "C" fn xsltCheckVersion(version: c_int) -> c_int {
    crate::abi::versioning::xsltCheckVersion(version)
}

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

/// Initialize the EXSLT module.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void exsltRegisterAll(void);
/// ```
///
/// Declared with #[no_mangle] in `src/exslt/mod.rs`.

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
pub extern "C" fn xsltCheckFeature(_feature: c_int) -> c_int {
    // All features reported as available.
    1
}

/// Get the XSLT engine version.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// const char *xsltEngineVersion(void);
/// ```
#[no_mangle]
pub extern "C" fn xsltEngineVersion() -> *const c_char {
    crate::abi::versioning::xsltLibxsltVersionString()
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
