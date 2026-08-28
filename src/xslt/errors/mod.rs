//! XSLT error handling (§33, §85 Phase 8).
//!
//! Defines error domains, error levels, error handler types, and the
//! public API for reporting and retrieving XSLT errors.
//!
//! # Phase 8 status
//!
//! Constants and function types are fully defined. Functions are stubbed
//! and will be implemented as part of Phase 8.

use crate::abi::structs::*;
use std::os::raw::c_int;

// ── Error domains ─────────────────────────────────────────────────────────
//
// These constants identify the category of an XSLT error.
// Source: xslt.h / xsltInternals.h (libxslt 1.1.39).

/// No error.
pub const XSLT_ERR_NONE: c_int = 0;

/// Unknown error.
pub const XSLT_ERR_UNKNOWN: c_int = 1;

/// Missing required namespace.
pub const XSLT_ERR_MISSING_NAMESPACE: c_int = 2;

/// Invalid namespace.
pub const XSLT_ERR_INVALID_NAMESPACE: c_int = 3;

/// Missing required attribute.
pub const XSLT_ERR_MISSING_ATTRIBUTE: c_int = 4;

/// Invalid attribute value.
pub const XSLT_ERR_INVALID_ATTRIBUTE: c_int = 5;

/// Missing required element.
pub const XSLT_ERR_MISSING_ELEMENT: c_int = 6;

/// Invalid element.
pub const XSLT_ERR_INVALID_ELEMENT: c_int = 7;

/// Missing match attribute.
pub const XSLT_ERR_MISSING_MATCH: c_int = 8;

/// Missing name attribute.
pub const XSLT_ERR_MISSING_NAME: c_int = 9;

/// Missing select attribute.
pub const XSLT_ERR_MISSING_SELECT: c_int = 10;

/// Missing test attribute.
pub const XSLT_ERR_MISSING_TEST: c_int = 11;

/// Missing use attribute.
pub const XSLT_ERR_MISSING_USE: c_int = 12;

/// Invalid match pattern.
pub const XSLT_ERR_INVALID_MATCH: c_int = 13;

/// Invalid select expression.
pub const XSLT_ERR_INVALID_SELECT: c_int = 14;

/// Invalid test expression.
pub const XSLT_ERR_INVALID_TEST: c_int = 15;

/// Invalid use expression.
pub const XSLT_ERR_INVALID_USE: c_int = 16;

/// Missing namespace.
pub const XSLT_ERR_MISSING_NS: c_int = 17;

/// Cyclic reference detected.
pub const XSLT_ERR_CYCLIC_REFERENCE: c_int = 18;

/// Recursion limit exceeded.
pub const XSLT_ERR_RECURSION: c_int = 19;

/// Internal XSLT error.
pub const XSLT_ERR_INTERNAL: c_int = 20;

// ── Error levels ──────────────────────────────────────────────────────────
//
// These constants indicate the severity of an XSLT error.
// Source: xslt.h (libxslt 1.1.39).

/// No error level (unset).
pub const XSLT_ERR_LEVEL_NONE: c_int = 0;

/// Warning — non-fatal issue.
pub const XSLT_ERR_LEVEL_WARNING: c_int = 1;

/// Error — processing may continue but results may be incomplete.
pub const XSLT_ERR_LEVEL_ERROR: c_int = 2;

/// Fatal error — processing cannot continue.
pub const XSLT_ERR_LEVEL_FATAL: c_int = 3;

// ── Error handler types ───────────────────────────────────────────────────

/// Global XSLT error handler function type.
///
/// Matches the upstream `xsltTransformErrorFunc` typedef:
/// ```c
/// typedef void (*xsltTransformErrorFunc)(void *ctxt, void *ctx,
///                                        xsltStylesheetPtr style,
///                                        const xmlChar *msg, ...);
/// ```
pub type xsltTransformErrorFunc = Option<
    unsafe extern "C" fn(
        *mut std::ffi::c_void,
        *mut std::ffi::c_void,
        *mut _xsltStylesheet,
        *const crate::abi::types::xmlChar,
        ...
    ),
>;

// ── Public API ────────────────────────────────────────────────────────────

/// The last XSLT error message (thread-local).
use std::sync::Mutex;

static LAST_XSLT_ERROR: Mutex<Option<Vec<u8>>> = Mutex::new(None);

/// Set the transform error handler for a context.
///
/// Registers a per-context error handler that will be called for every
/// error reported during the transformation. Pass `None` to restore the
/// default handler.
///
/// # Parameters
///
/// * `ctxt`   — The transform context, or `std::ptr::null_mut()` for the
///              global handler.
/// * `ctx`    — Opaque user-data pointer passed to the handler.
/// * `handler` — The error handler function, or `None` to reset.
pub fn xsltSetTransformErrorFunc(
    ctxt: *mut _xsltTransformContext,
    ctx: *mut std::ffi::c_void,
    handler: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const std::os::raw::c_char)>,
) {
    if ctxt.is_null() {
        return;
    }
    // SAFETY: ctxt must be a valid _xsltTransformContext.
    unsafe {
        (*ctxt).errFunc = handler
            .map(|h| h as *mut std::ffi::c_void)
            .unwrap_or(std::ptr::null_mut());
        (*ctxt).errCtxt = ctx;
    }
}

/// Report an XSLT error.
///
/// Logs an error message associated with the given transform context,
/// stylesheet, and instruction node. The message is a printf-style format
/// string followed by variadic arguments. The variadic arguments are not
/// expanded (matching the safe subset); the raw message is recorded.
///
/// # Parameters
///
/// * `ctxt`  — The transform context (may be null).
/// * `style` — The stylesheet (may be null).
/// * `inst`  — The instruction node that triggered the error (may be null).
/// * `msg`   — The printf-style format string.
pub fn xsltTransformError(
    ctxt: *mut _xsltTransformContext,
    style: *mut _xsltStylesheet,
    inst: *mut _xmlNode,
    msg: *const std::os::raw::c_char,
) {
    if msg.is_null() {
        return;
    }
    // SAFETY: msg must be a valid NUL-terminated C string.
    let bytes =
        unsafe { core::slice::from_raw_parts(msg as *const u8, libc::strlen(msg) as usize) };
    let text = String::from_utf8_lossy(bytes).into_owned();

    // Record the last error.
    if let Ok(mut last) = LAST_XSLT_ERROR.lock() {
        *last = Some(text.clone().into_bytes());
    }

    // Build the prefix: "file:line: " when the instruction node provides it.
    let mut prefix = String::new();
    if !inst.is_null() {
        // SAFETY: inst must be a valid node.
        let node = unsafe { &*inst };
        // SAFETY: node.doc must be valid while the node is alive.
        let doc = unsafe { &*node.doc };
        if !doc.URL.is_null() {
            // SAFETY: URL must be a valid NUL-terminated string.
            let url = unsafe {
                core::slice::from_raw_parts(
                    doc.URL as *const u8,
                    libc::strlen(doc.URL as *const libc::c_char) as usize,
                )
            };
            prefix.push_str(&String::from_utf8_lossy(url));
            prefix.push(':');
            prefix.push_str(&node.line.to_string());
            prefix.push_str(": ");
        }
    }

    // Invoke the per-context handler if one is registered.
    if !ctxt.is_null() {
        // SAFETY: ctxt must be a valid _xsltTransformContext.
        let ctx = unsafe { &*ctxt };
        if !ctx.errFunc.is_null() {
            // SAFETY: errFunc is a valid handler registered by
            // xsltSetTransformErrorFunc; errCtxt is the matching context.
            let handler: unsafe extern "C" fn(*mut std::ffi::c_void, *const std::os::raw::c_char) =
                unsafe { std::mem::transmute(ctx.errFunc) };
            let full = format!("{}{}", prefix, text);
            let mut cmsg = full.into_bytes();
            cmsg.push(0);
            unsafe { handler(ctx.errCtxt, cmsg.as_ptr() as *const std::os::raw::c_char) };
            return;
        }
    }

    // Default: write to stderr.
    let full = format!("{}{}\n", prefix, text);
    let _ = unsafe { libc::write(2, full.as_ptr() as *const libc::c_void, full.len()) };
    let _ = style;
}

/// Get the last XSLT error message as a NUL-terminated heap string.
///
/// Returns a pointer to the last error message, or `std::ptr::null_mut()`
/// if no error has occurred. The caller frees with `libc::free`.
pub fn xsltGetLastError() -> *mut std::ffi::c_void {
    let guard = match LAST_XSLT_ERROR.lock() {
        Ok(g) => g,
        Err(_) => return std::ptr::null_mut(),
    };
    match guard.as_ref() {
        Some(bytes) => {
            let len = bytes.len();
            // SAFETY: malloc returns writable memory or NULL.
            let p = unsafe { libc::malloc(len + 1) } as *mut u8;
            if p.is_null() {
                return std::ptr::null_mut();
            }
            unsafe {
                core::ptr::copy_nonoverlapping(bytes.as_ptr(), p, len);
                *p.add(len) = 0;
            }
            p as *mut std::ffi::c_void
        }
        None => std::ptr::null_mut(),
    }
}
