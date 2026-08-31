//! XSLT error handling (§33, §85 Phase 8).
//!
//! Defines error domains, error levels, error handler types, and the
//! public API for reporting and retrieving XSLT errors.
//!
//! # Upstream contract
//!
//! Parity target: upstream libxslt `xsltutils.c` + `xsltInternals.h`
//! (1.1.45; `SRC-LIBXSLT-1.1.42-XSLTUTILS-C` under oracle/historical/src).
//! The observable surface is `xsltTransformError` (with its
//! `xsltPrintErrorContext` context line), `xsltSetTransformErrorFunc`,
//! `xsltSetGenericDebugFunc`/`xsltGenericDebug`, `xsltGetLastError`, and
//! the `XSLT_ERR_*` domain/level constants from xslt.h 1.1.45.
//!
//! # Conceptual behavior
//!
//! Error reporting follows upstream routing: `xsltTransformError` moves
//! the transform context out of the OK state (error or stopped), prints
//! the `xsltPrintErrorContext` context line (one of `error`,
//! `compilation error`, `runtime error`, each optionally with file/line/
//! element), and emits the message verbatim — messages carry their own
//! trailing newline and the function is variadic upstream, so callers
//! format before calling (the candidate never expands `%s`/`%d`
//! placeholders). With a registered per-context handler the message is
//! routed there; otherwise it goes to stderr.
//!
//! # Ownership & safety invariants
//!
//! - `LAST_XSLT_ERROR` is a mutex-guarded thread-safe copy of the last raw
//!   message; `xsltGetLastError` returns a heap copy the caller frees with
//!   libc::free (matching the documented `caller frees` contract for
//!   `xsltGetLastError`).
//! - Handler slots (`error`/`errctx`) are borrowed user-data, never
//!   dereferenced by the library — per atlas/OWNERSHIP_ATLAS.md section 6,
//!   the caller keeps the context alive.
//! - The exported `xsltGenericDebug` / `xsltGenericDebugContext` data globals
//!   (crate::abi::data_globals) are process-global, exactly like the upstream
//!   `xsltGenericDebug` globals; callers must not race them (R-000171
//!   slot-race lesson applied to the debug pair). 11.1-Z.1: `xsltGenericDebug`
//!   is exported as DATA (function pointer, oracle D) with the upstream
//!   default handler, not as a function (R-000174).
//!
//! # Historical quirks & epochs
//!
//! E-008 (atlas/SEMANTIC_EPOCHS.md): error output participates in the
//! byte-identical xsltproc epoch (1.1.26, 2009, through 1.1.45), so the
//! context-line wording is frozen. R-000161 fixed error routing parity for
//! the generic/structured handler chain (xmlFormatError fragment
//! streaming, 6 calls per raise) and the default variadic stderr printers
//! `xmlGenericError`/`xsltGenericError`; the candidate `xsltGenericDebug`
//! default handler writes through the `xsltGenericDebugContext` FILE* with
//! the upstream NULL-context suppression (11.1-Z.1, R-000174).
//! R-000140 covered the `_xslt*` ABI mirrors.
//!
//! # Deliberate oddities
//!
//! - The variadic upstream signature is reduced to a pre-formatted
//!   message (an intentional, documented divergence — see the
//!   `xsltTransformError` docs); the emitted bytes match the oracle.
//! - `xsltSetGenericDebugFunc` keeps the upstream NULL-context
//!   suppression quirk: a NULL handler with a NULL context suppresses
//!   debug output.
//!
//! # Proving courts
//!
//! ERROR-001 (error-family differential probe; R-000161), CLI-XSLTPROC
//! (stderr byte-compare on failing stylesheets), XSLT-001, and the
//! in-crate `cargo test` suites.
//!
//! # Tempting simplifications that would break parity
//!
//! - Replacing the context line with a plain `error:` prefix breaks
//!   stderr byte-parity for compilation and runtime errors (the
//!   `xsltPrintErrorContext` forms are oracle-verified).
//! - Buffering the last-error message with the error state would drop the
//!   frozen `state` transition (OK → ERROR/STOPPED) that the transform
//!   loop checks to stop execution.
//! - Writing debug output unconditionally would break the upstream
//!   NULL-context suppression contract exercised by the CLI corpus.

use crate::abi::structs::*;
use std::os::raw::c_int;
use std::ptr;

// ── Error domains ─────────────────────────────────────────────────────────
//
// These constants identify the category of an XSLT error.
// Source: xslt.h / xsltInternals.h (libxslt 1.1.45).

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
// Source: xslt.h (libxslt 1.1.45).

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

/// Global debug handler: the exported `xsltGenericDebug` data global in
/// `crate::abi::data_globals` (upstream `xsltGenericDebug`, a function
/// pointer variable defaulting to `xsltGenericDebugDefaultFunc` — R-000174).
///
/// Set the generic debug handler (upstream `xsltSetGenericDebugFunc`).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltSetGenericDebugFunc(void *ctx, xmlGenericErrorFunc handler);
/// ```
///
/// Upstream (xsltutils.c:650): `xsltGenericDebugContext = ctx;` and — only
/// when `handler != NULL` — `xsltGenericDebug = handler;`. With a NULL
/// context the default handler suppresses output; a NULL handler leaves the
/// current handler installed.
///
/// # SAFETY
///
/// - `ctx` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `handler` must be a valid callback (or None);
///   the callback is invoked with the documented context pointer and
///   must itself uphold the same pointer invariants.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xsltSetGenericDebugFunc(
    ctx: *mut std::ffi::c_void,
    handler: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const std::os::raw::c_char)>,
) {
    unsafe {
        crate::abi::data_globals::xsltGenericDebugContext = ctx;
        if handler.is_some() {
            crate::abi::data_globals::xsltGenericDebug = handler;
        }
    }
}

/// Set the transform error handler for a context.
///
/// Registers a per-context error handler that will be called for every
/// error reported during the transformation. Pass `None` to restore the
/// default handler.
///
/// # Parameters
///
/// * `ctxt`   — The transform context, or `std::ptr::null_mut()` for the
///   global handler.
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
        (*ctxt).error = handler;
        (*ctxt).errctx = ctx;
    }
}

/// Report an XSLT error.
///
/// Faithful port of upstream xsltutils.c `xsltTransformError`: the
/// transform context is moved to the error state, the error context line
/// is printed (upstream `xsltPrintErrorContext`), and the message is
/// emitted verbatim through the registered handler or stderr. Messages
/// carry their own trailing newline, exactly as upstream's do — no
/// newline is added here.
///
/// The upstream signature is variadic (`const char *msg, ...`); the
/// candidate's callers format the message before calling (a `%s`/`%d`
/// placeholder is never expanded by this function).
///
/// # Parameters
///
/// * `ctxt`  — The transform context (may be null).
/// * `style` — The stylesheet (may be null).
/// * `inst`  — The instruction node that triggered the error (may be null).
/// * `msg`   — The message, NUL-terminated, typically ending in `\n`.
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

    // Record the last error (the raw message, as upstream stores the
    // formatted message).
    if let Ok(mut last) = LAST_XSLT_ERROR.lock() {
        *last = Some(text.clone().into_bytes());
    }

    // UPSTREAM-PARITY (xsltutils.c xsltTransformError): an error moves the
    // transform context out of the OK state.
    if !ctxt.is_null() {
        // SAFETY: ctxt must be a valid _xsltTransformContext.
        let ctx = unsafe { &mut *ctxt };
        if ctx.state == crate::xslt::transform::XSLT_STATE_OK {
            ctx.state = crate::xslt::transform::XSLT_STATE_ERROR;
        }
        let mut node = inst;
        if node.is_null() {
            node = ctx.inst;
        }
        // Build the context line (xsltPrintErrorContext) and the full
        // message, then emit through the handler if one is registered.
        let context_line = print_error_context(ctxt, style, node);
        let full = format!("{}{}", context_line, text);
        let mut cmsg = full.into_bytes();
        let msg_len = cmsg.len();
        cmsg.push(0);
        let ctx = unsafe { &*ctxt };
        if let Some(handler) = ctx.error {
            unsafe { handler(ctx.errctx, cmsg.as_ptr() as *const std::os::raw::c_char) };
            return;
        }
        let _ = unsafe { libc::write(2, cmsg.as_ptr() as *const libc::c_void, msg_len) };
        return;
    }

    // No transform context: compile-time errors and standalone messages.
    // (Upstream xsltPrintErrorContext is still invoked with NULL ctxt and
    // the given style/node.)
    let context_line = print_error_context(ptr::null_mut(), style, inst);
    let full = format!("{}{}", context_line, text);
    let mut cmsg = full.into_bytes();
    let msg_len = cmsg.len();
    cmsg.push(0);
    let _ = unsafe { libc::write(2, cmsg.as_ptr() as *const libc::c_void, msg_len) };
    let _ = style;
}

/// Build the error context line printed before an XSLT error message
/// (upstream xsltutils.c `xsltPrintErrorContext`). The line is one of:
///
/// ```text
/// error\n
/// error: file F\n
/// error: file F line N\n
/// error: file F element E\n
/// error: file F line N element E\n
/// error: element E\n
/// compilation error ... / runtime error ...
/// ```
fn print_error_context(
    ctxt: *mut _xsltTransformContext,
    style: *mut _xsltStylesheet,
    node: *mut _xmlNode,
) -> String {
    let mut line = 0i64;
    let mut file: *const std::os::raw::c_char = ptr::null();
    let mut name: *const std::os::raw::c_char = ptr::null();

    if !node.is_null() {
        // SAFETY: node must be valid.
        let node_ref = unsafe { &*node };
        if node_ref.type_ == crate::abi::types::xmlElementType::XML_DOCUMENT_NODE as c_int
            || node_ref.type_ == crate::abi::types::xmlElementType::XML_HTML_DOCUMENT_NODE as c_int
        {
            let doc = node as *mut crate::abi::structs::_xmlDoc;
            // SAFETY: doc->URL is a valid NUL-terminated string or NULL.
            file = unsafe { (*doc).URL } as *const std::os::raw::c_char;
        } else {
            line = crate::abi::exports_xml2::xmlGetLineNo(node) as i64;
            // SAFETY: node->doc must be valid while the node is alive.
            let doc = { node_ref.doc };
            if !doc.is_null() {
                file = unsafe { (*doc).URL } as *const std::os::raw::c_char;
            }
            name = node_ref.name as *const std::os::raw::c_char;
        }
    }

    let errtype = if !ctxt.is_null() {
        "runtime error"
    } else if !style.is_null() {
        "compilation error"
    } else {
        "error"
    };

    let s = |p: *const std::os::raw::c_char| -> String {
        if p.is_null() {
            String::new()
        } else {
            unsafe { std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned() }
        }
    };
    let file_s = s(file);
    let name_s = s(name);
    let has_file = !file.is_null();
    let has_name = !name.is_null();

    if has_file && line != 0 && has_name {
        format!(
            "{}: file {} line {} element {}\n",
            errtype, file_s, line, name_s
        )
    } else if has_file && has_name {
        format!("{}: file {} element {}\n", errtype, file_s, name_s)
    } else if has_file && line != 0 {
        format!("{}: file {} line {}\n", errtype, file_s, line)
    } else if has_file {
        format!("{}: file {}\n", errtype, file_s)
    } else if has_name {
        format!("{}: element {}\n", errtype, name_s)
    } else {
        format!("{}\n", errtype)
    }
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
