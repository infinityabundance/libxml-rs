//! C ABI exports for the XInclude family — `xmlXInclude*` (upstream
//! `xinclude.h`, 2.15.3).
//!
//! Implements the context-based XInclude entry points by wrapping the
//! native-Rust XInclude engine (`src/xml/xinclude`), which also powers
//! `xmllint --xinclude`.
//!
//! The context struct (`_xmlXIncludeCtxt`) is opaque at the C boundary — no
//! field is ever dereferenced by the caller. The candidate engine keeps all
//! per-include state (URL stack, circular-reference tracking, fallback
//! handling) in Rust-owned structures that live for the duration of a single
//! process call, so the context only needs to record the document, the
//! processing flags, the error handler and the last error code.
//!
//! # Engine scope
//!
//! The engine's public entry points (`xinclude_process`,
//! `xinclude_process_flags`) always walk the whole document starting at its
//! root element. The tree-based entry points below therefore process the
//! document that owns the given node; for a node that is the document root
//! (the common case) this is exactly the upstream subtree semantics.

#![allow(
    missing_docs,
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals
)]

use core::ffi::c_void;
use core::mem::size_of;
use core::ptr;
use std::os::raw::{c_char, c_int};

use crate::abi::allocator::{xmlFree, xmlMallocZero};
use crate::abi::structs::{_xmlDoc, _xmlNode};
use crate::abi::types::{XML_ERR_ARGUMENT, XML_ERR_INTERNAL_ERROR, XML_ERR_OK};
use crate::xml::xinclude;

/// Generic failure return for the process entry points (upstream `-1`).
const XINCLUDE_ERROR: c_int = -1;

/// XInclude error callback (upstream `xmlXIncludeErrorFunc`, xinclude.h).
///
/// `ctx` is the application data pointer registered with the handler,
/// `code` is the error code, `message` the human-readable message.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// typedef void (*xmlXIncludeErrorFunc) (void *context, int code,
///                                       const char *message);
/// ```
pub type xmlXIncludeErrorFunc =
    unsafe extern "C" fn(ctx: *mut c_void, code: c_int, message: *const c_char);

/// XInclude processing context (upstream `struct _xmlXIncludeCtxt`).
///
/// Opaque to C callers; only the fields the candidate engine needs are kept.
#[repr(C)]
#[derive(Debug)]
pub struct _xmlXIncludeCtxt {
    /// The source document being processed.
    pub doc: *mut _xmlDoc,
    /// Error handling function (never invoked by the candidate engine).
    pub error: Option<xmlXIncludeErrorFunc>,
    /// Application data passed to the error handler.
    pub data: *mut c_void,
    /// Processing flags (e.g. `XML_PARSE_NOXINCNODE`, `XML_PARSE_NONET`).
    pub flags: c_int,
    /// Error code of the last failure during processing.
    pub lastError: c_int,
}

/// XInclude context pointer (upstream `xmlXIncludeCtxtPtr`).
pub type xmlXIncludeCtxtPtr = *mut _xmlXIncludeCtxt;

// ═══════════════════════════════════════════════════════════════════════════════
// Context lifecycle
// ═══════════════════════════════════════════════════════════════════════════════

/// Create a new XInclude processing context.
///
/// Returns the context, or NULL on allocation failure. `doc` may be NULL;
/// processing calls then fail with an argument error.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlXIncludeCtxt *xmlXIncludeNewContext(xmlDoc *doc);
/// ```
///
/// # SAFETY
///
/// `doc` must be NULL or a valid pointer to a parsed `_xmlDoc`.
#[no_mangle]
pub unsafe extern "C" fn xmlXIncludeNewContext(doc: *mut _xmlDoc) -> xmlXIncludeCtxtPtr {
    let ctxt = unsafe { xmlMallocZero(size_of::<_xmlXIncludeCtxt>()) } as *mut _xmlXIncludeCtxt;
    if ctxt.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        (*ctxt).doc = doc;
        (*ctxt).error = None;
        (*ctxt).data = ptr::null_mut();
        (*ctxt).flags = 0;
        (*ctxt).lastError = XML_ERR_OK;
    }
    ctxt
}

/// Free an XInclude processing context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlXIncludeFreeContext(xmlXIncludeCtxt *ctxt);
/// ```
///
/// # SAFETY
///
/// `ctxt` must be NULL or a pointer previously returned by
/// `xmlXIncludeNewContext` that has not already been freed.
#[no_mangle]
pub unsafe extern "C" fn xmlXIncludeFreeContext(ctxt: xmlXIncludeCtxtPtr) {
    if ctxt.is_null() {
        return;
    }
    // The context owns no heap allocations of its own (the engine keeps its
    // per-include state only for the duration of a process call), so the
    // struct itself is the only thing to release.
    unsafe { xmlFree(ctxt as *mut c_void) };
}

// ═══════════════════════════════════════════════════════════════════════════════
// Processing
// ═══════════════════════════════════════════════════════════════════════════════

/// Run the internal engine over the context's document, recording the
/// outcome in `lastError`.
///
/// Returns the number of XInclude nodes processed, or -1 on error.
///
/// # SAFETY
///
/// `ctxt` must be a valid, non-null pointer to a context created by
/// `xmlXIncludeNewContext`.
unsafe fn process_ctxt_doc(ctxt: xmlXIncludeCtxtPtr) -> c_int {
    let doc = unsafe { (*ctxt).doc };
    if doc.is_null() {
        unsafe { (*ctxt).lastError = XML_ERR_ARGUMENT };
        return XINCLUDE_ERROR;
    }
    let flags = unsafe { (*ctxt).flags };
    let ret = unsafe { xinclude::xinclude_process_flags(doc, flags) };
    if ret < 0 {
        // The engine reports failure without a fine-grained error code, so
        // record a generic parser error to make xmlXIncludeGetLastError
        // non-zero after a failure.
        unsafe { (*ctxt).lastError = XML_ERR_INTERNAL_ERROR };
    }
    ret
}

/// Process the XInclude nodes in the tree rooted at `tree`, using the
/// context's document and flags.
///
/// Returns the number of XInclude nodes processed, or -1 on error.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlXIncludeProcessNode(xmlXIncludeCtxt *ctxt, xmlNode *tree);
/// ```
///
/// # SAFETY
///
/// `ctxt` must be a valid context created by `xmlXIncludeNewContext`; `tree`
/// must be NULL or a valid `_xmlNode`.
#[no_mangle]
pub unsafe extern "C" fn xmlXIncludeProcessNode(
    ctxt: xmlXIncludeCtxtPtr,
    tree: *mut _xmlNode,
) -> c_int {
    if ctxt.is_null() || tree.is_null() {
        return XINCLUDE_ERROR;
    }
    unsafe { process_ctxt_doc(ctxt) }
}

/// Process all XInclude nodes in the document containing `tree`.
///
/// Returns the number of XInclude nodes processed, or -1 on error.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlXIncludeProcessTree(xmlNode *tree);
/// ```
///
/// # SAFETY
///
/// `tree` must be NULL or a valid `_xmlNode` attached to a document.
#[no_mangle]
pub unsafe extern "C" fn xmlXIncludeProcessTree(tree: *mut _xmlNode) -> c_int {
    if tree.is_null() {
        return XINCLUDE_ERROR;
    }
    let doc = unsafe { (*tree).doc };
    if doc.is_null() {
        return XINCLUDE_ERROR;
    }
    let ctxt = unsafe { xmlXIncludeNewContext(doc) };
    if ctxt.is_null() {
        return XINCLUDE_ERROR;
    }
    let ret = unsafe { xmlXIncludeProcessNode(ctxt, tree) };
    unsafe { xmlXIncludeFreeContext(ctxt) };
    ret
}

/// Process all XInclude nodes in the document containing `tree`, with flags.
///
/// Returns the number of XInclude nodes processed, or -1 on error.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlXIncludeProcessTreeFlags(xmlNode *tree, int flags);
/// ```
///
/// # SAFETY
///
/// `tree` must be NULL or a valid `_xmlNode` attached to a document.
#[no_mangle]
pub unsafe extern "C" fn xmlXIncludeProcessTreeFlags(tree: *mut _xmlNode, flags: c_int) -> c_int {
    if tree.is_null() {
        return XINCLUDE_ERROR;
    }
    let doc = unsafe { (*tree).doc };
    if doc.is_null() {
        return XINCLUDE_ERROR;
    }
    let ctxt = unsafe { xmlXIncludeNewContext(doc) };
    if ctxt.is_null() {
        return XINCLUDE_ERROR;
    }
    unsafe {
        (*ctxt).flags = flags;
    }
    let ret = unsafe { xmlXIncludeProcessNode(ctxt, tree) };
    unsafe { xmlXIncludeFreeContext(ctxt) };
    ret
}

/// Process all XInclude nodes in the document containing `tree`, with flags
/// and an error-handler data pointer.
///
/// Returns the number of XInclude nodes processed, or -1 on error.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlXIncludeProcessTreeFlagsData(xmlNode *tree, int flags, void *data);
/// ```
///
/// # SAFETY
///
/// `tree` must be NULL or a valid `_xmlNode` attached to a document; `data`
/// must be NULL or a valid pointer for the lifetime of the call.
#[no_mangle]
pub unsafe extern "C" fn xmlXIncludeProcessTreeFlagsData(
    tree: *mut _xmlNode,
    flags: c_int,
    data: *mut c_void,
) -> c_int {
    if tree.is_null() {
        return XINCLUDE_ERROR;
    }
    let doc = unsafe { (*tree).doc };
    if doc.is_null() {
        return XINCLUDE_ERROR;
    }
    let ctxt = unsafe { xmlXIncludeNewContext(doc) };
    if ctxt.is_null() {
        return XINCLUDE_ERROR;
    }
    unsafe {
        (*ctxt).flags = flags;
        (*ctxt).data = data;
        // Upstream also clears the handler function when only data is
        // supplied; the candidate engine never invokes the handler, so the
        // data pointer is the only observable part.
        (*ctxt).error = None;
    }
    let ret = unsafe { xmlXIncludeProcessNode(ctxt, tree) };
    unsafe { xmlXIncludeFreeContext(ctxt) };
    ret
}

/// Process all XInclude nodes in `doc`, with flags and an error-handler data
/// pointer.
///
/// Returns the number of XInclude nodes processed, or -1 on error.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlXIncludeProcessFlagsData(xmlDoc *doc, int flags, void *data);
/// ```
///
/// # SAFETY
///
/// `doc` must be NULL or a valid pointer to a parsed `_xmlDoc`; `data` must
/// be NULL or a valid pointer for the lifetime of the call.
#[no_mangle]
pub unsafe extern "C" fn xmlXIncludeProcessFlagsData(
    doc: *mut _xmlDoc,
    flags: c_int,
    data: *mut c_void,
) -> c_int {
    if doc.is_null() {
        return XINCLUDE_ERROR;
    }
    let ctxt = unsafe { xmlXIncludeNewContext(doc) };
    if ctxt.is_null() {
        return XINCLUDE_ERROR;
    }
    unsafe {
        (*ctxt).flags = flags;
        (*ctxt).data = data;
    }
    let ret = unsafe { process_ctxt_doc(ctxt) };
    unsafe { xmlXIncludeFreeContext(ctxt) };
    ret
}

// ═══════════════════════════════════════════════════════════════════════════════
// Context configuration & state
// ═══════════════════════════════════════════════════════════════════════════════

/// Set the processing flags on the context.
///
/// Returns the previous set of flags, or -1 if `ctxt` is NULL.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlXIncludeSetFlags(xmlXIncludeCtxt *ctxt, int flags);
/// ```
///
/// # SAFETY
///
/// `ctxt` must be NULL or a valid context created by `xmlXIncludeNewContext`.
#[no_mangle]
pub unsafe extern "C" fn xmlXIncludeSetFlags(ctxt: xmlXIncludeCtxtPtr, flags: c_int) -> c_int {
    if ctxt.is_null() {
        return XINCLUDE_ERROR;
    }
    let old_flags = unsafe { (*ctxt).flags };
    unsafe {
        (*ctxt).flags = flags;
    }
    old_flags
}

/// Set the error handler and its data on the context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlXIncludeSetErrorHandler(xmlXIncludeCtxt *ctxt,
///                                 xmlXIncludeErrorFunc handler,
///                                 void *data);
/// ```
///
/// # SAFETY
///
/// `ctxt` must be NULL or a valid context created by `xmlXIncludeNewContext`;
/// `data` must be NULL or a valid pointer for as long as the handler may be
/// called.
#[no_mangle]
pub unsafe extern "C" fn xmlXIncludeSetErrorHandler(
    ctxt: xmlXIncludeCtxtPtr,
    handler: Option<xmlXIncludeErrorFunc>,
    data: *mut c_void,
) {
    if ctxt.is_null() {
        return;
    }
    unsafe {
        (*ctxt).error = handler;
        (*ctxt).data = data;
    }
}

/// Get the error code of the last failure during XInclude processing on this
/// context.
///
/// Returns the last error code, or -1 if `ctxt` is NULL.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlXIncludeGetLastError(xmlXIncludeCtxt *ctxt);
/// ```
///
/// # SAFETY
///
/// `ctxt` must be NULL or a valid context created by `xmlXIncludeNewContext`.
#[no_mangle]
pub unsafe extern "C" fn xmlXIncludeGetLastError(ctxt: xmlXIncludeCtxtPtr) -> c_int {
    if ctxt.is_null() {
        return XINCLUDE_ERROR;
    }
    unsafe { (*ctxt).lastError }
}
