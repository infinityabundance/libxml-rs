//! C ABI exports for libxslt.so.1 — no_mangle extern "C" functions (§1, §16).
//!
//! This module contains all `#[no_mangle] pub extern "C"` function definitions
//! that form the public ABI of libxslt.so.1.
//!
//! # Phase 1 status
//!
//! Complete — all major XSLT ABI entry points are defined.
//! Most functions are stubs that will be filled in during Phase 8 (libxslt).

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
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltInit(void);
/// ```
#[no_mangle]
pub extern "C" fn xsltInit() {
    // Phase 1: STUB
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
    // Phase 1: STUB
}

// ═══════════════════════════════════════════════════════════════════════════════
// 2. Stylesheet Compilation
// ═══════════════════════════════════════════════════════════════════════════════

/// Parse a stylesheet from a file.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xsltStylesheetPtr xsltParseStylesheetFile(const xmlChar *filename);
/// ```
///
/// Returns a newly allocated stylesheet. Caller must free with `xsltFreeStylesheet`.
#[no_mangle]
pub unsafe extern "C" fn xsltParseStylesheetFile(
    _filename: *const xmlChar,
) -> *mut _xsltStylesheet {
    // Phase 1: STUB
    ptr::null_mut()
}

/// Parse a stylesheet from a document.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xsltStylesheetPtr xsltParseStylesheetDoc(xmlDocPtr doc);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltParseStylesheetDoc(_doc: *mut _xmlDoc) -> *mut _xsltStylesheet {
    // Phase 1: STUB
    ptr::null_mut()
}

/// Parse a stylesheet from memory.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xsltStylesheetPtr xsltParseStylesheetMemory(const char *buf, int len,
///                                             const char *URL);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltParseStylesheetMemory(
    _buf: *const c_char,
    _len: c_int,
    _URL: *const c_char,
) -> *mut _xsltStylesheet {
    // Phase 1: STUB
    ptr::null_mut()
}

/// Free a stylesheet.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltFreeStylesheet(xsltStylesheetPtr style);
/// ```
#[no_mangle]
pub extern "C" fn xsltFreeStylesheet(_style: *mut _xsltStylesheet) {
    // Phase 1: STUB
}

// ═══════════════════════════════════════════════════════════════════════════════
// 3. Transformation
// ═══════════════════════════════════════════════════════════════════════════════

/// Apply a stylesheet to a document.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDocPtr xsltApplyStylesheet(xsltStylesheetPtr style, xmlDocPtr doc,
///                               const char **params);
/// ```
///
/// `params` is a NULL-terminated array of name=value strings.
/// Returns the result document (caller must free with `xmlFreeDoc`).
#[no_mangle]
pub unsafe extern "C" fn xsltApplyStylesheet(
    _style: *mut _xsltStylesheet,
    _doc: *mut _xmlDoc,
    _params: *mut *const c_char,
) -> *mut _xmlDoc {
    // Phase 1: STUB
    ptr::null_mut()
}

/// Apply a stylesheet with a stack of params.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDocPtr xsltApplyStylesheetStacked(xsltStylesheetPtr style, xmlDocPtr doc,
///                                      const char **params,
///                                      xsltTransformStackElemPtr stack);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltApplyStylesheetStacked(
    _style: *mut _xsltStylesheet,
    _doc: *mut _xmlDoc,
    _params: *mut *const c_char,
    _stack: *mut c_void,
) -> *mut _xmlDoc {
    // Phase 1: STUB
    ptr::null_mut()
}

/// Apply a stylesheet with a user data context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDocPtr xsltApplyStylesheetUser(xsltStylesheetPtr style, xmlDocPtr doc,
///                                   const char **params, const char *output,
///                                   FILE *profile, xsltTransformContextPtr userCtxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltApplyStylesheetUser(
    _style: *mut _xsltStylesheet,
    _doc: *mut _xmlDoc,
    _params: *mut *const c_char,
    _output: *const c_char,
    _profile: *mut c_void,
    _userCtxt: *mut _xsltTransformContext,
) -> *mut _xmlDoc {
    // Phase 1: STUB
    ptr::null_mut()
}

/// Free the result of a transformation.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltFreeTransformResult(xmlDocPtr result);
/// ```
#[no_mangle]
pub extern "C" fn xsltFreeTransformResult(_result: *mut _xmlDoc) {
    // Phase 1: STUB
}

// ═══════════════════════════════════════════════════════════════════════════════
// 4. Transform Context
// ═══════════════════════════════════════════════════════════════════════════════

/// Create a transform context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xsltTransformContextPtr xsltNewTransformContext(xsltStylesheetPtr style,
///                                                  xmlDocPtr doc);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltNewTransformContext(
    _style: *mut _xsltStylesheet,
    _doc: *mut _xmlDoc,
) -> *mut _xsltTransformContext {
    // Phase 1: STUB
    ptr::null_mut()
}

/// Free a transform context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltFreeTransformContext(xsltTransformContextPtr ctxt);
/// ```
#[no_mangle]
pub extern "C" fn xsltFreeTransformContext(_ctxt: *mut _xsltTransformContext) {
    // Phase 1: STUB
}

// ═══════════════════════════════════════════════════════════════════════════════
// 5. Security
// ═══════════════════════════════════════════════════════════════════════════════

/// Create a security preferences structure.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xsltSecurityPrefsPtr xsltNewSecurityPrefs(void);
/// ```
#[no_mangle]
pub extern "C" fn xsltNewSecurityPrefs() -> *mut c_void {
    // Phase 1: STUB
    ptr::null_mut()
}

/// Free a security preferences structure.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltFreeSecurityPrefs(xsltSecurityPrefsPtr sec);
/// ```
#[no_mangle]
pub extern "C" fn xsltFreeSecurityPrefs(_sec: *mut c_void) {
    // Phase 1: STUB
}

/// Set a security preference.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltSetSecurityPrefs(xsltSecurityPrefsPtr sec,
///                          xsltSecurityOption option, int value);
/// ```
#[no_mangle]
pub extern "C" fn xsltSetSecurityPrefs(_sec: *mut c_void, _option: c_int, _value: c_int) -> c_int {
    // Phase 1: STUB
    0
}

/// Get a security preference.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltGetSecurityPrefs(xsltSecurityPrefsPtr sec,
///                          xsltSecurityOption option);
/// ```
#[no_mangle]
pub extern "C" fn xsltGetSecurityPrefs(_sec: *mut c_void, _option: c_int) -> c_int {
    // Phase 1: STUB
    1
}

/// Set the default security preferences.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltSetDefaultSecurityPrefs(xsltSecurityPrefsPtr sec);
/// ```
#[no_mangle]
pub extern "C" fn xsltSetDefaultSecurityPrefs(_sec: *mut c_void) {
    // Phase 1: STUB
}

/// Get the default security preferences.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xsltSecurityPrefsPtr xsltGetDefaultSecurityPrefs(void);
/// ```
#[no_mangle]
pub extern "C" fn xsltGetDefaultSecurityPrefs() -> *mut c_void {
    // Phase 1: STUB
    ptr::null_mut()
}

// ═══════════════════════════════════════════════════════════════════════════════
// 6. Extensions
// ═══════════════════════════════════════════════════════════════════════════════

/// Register an XSLT extension function.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltRegisterExtFunction(xsltTransformContextPtr ctxt,
///                             const xmlChar *name, const xmlChar *NS_uri,
///                             xmlXPathFunction f);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltRegisterExtFunction(
    _ctxt: *mut _xsltTransformContext,
    _name: *const xmlChar,
    _NS_uri: *const xmlChar,
    _f: Option<unsafe extern "C" fn(*mut c_void, c_int)>,
) -> c_int {
    // Phase 1: STUB
    0
}

/// Register an XSLT extension element.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltRegisterExtElement(xsltTransformContextPtr ctxt,
///                            const xmlChar *name, const xmlChar *NS_uri,
///                            xsltTransformFunction f);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltRegisterExtElement(
    _ctxt: *mut _xsltTransformContext,
    _name: *const xmlChar,
    _NS_uri: *const xmlChar,
    _f: Option<
        unsafe extern "C" fn(*mut c_void, *mut c_void, *mut _xmlNode, *mut c_void, *mut _xmlNode),
    >,
) -> c_int {
    // Phase 1: STUB
    0
}

/// Initialize the EXSLT module.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void exsltRegisterAll(void);
/// ```
#[no_mangle]
pub extern "C" fn exsltRegisterAll() {
    // Phase 1: STUB
}

// ═══════════════════════════════════════════════════════════════════════════════
// 7. Save Result to File
// ═══════════════════════════════════════════════════════════════════════════════

/// Save a result document to a file.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltSaveResultToFile(FILE *output, xmlDocPtr result,
///                          xsltStylesheetPtr style);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltSaveResultToFile(
    _output: *mut c_void,
    _result: *mut _xmlDoc,
    _style: *mut _xsltStylesheet,
) -> c_int {
    // Phase 1: STUB
    0
}

/// Save a result document to a file descriptor.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltSaveResultToFd(int fd, xmlDocPtr result,
///                        xsltStylesheetPtr style);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltSaveResultToFd(
    _fd: c_int,
    _result: *mut _xmlDoc,
    _style: *mut _xsltStylesheet,
) -> c_int {
    // Phase 1: STUB
    0
}

/// Save a result document to a buffer.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltSaveResultToString(xmlChar **doc_txt_ptr, int *doc_txt_len,
///                            xmlDocPtr result, xsltStylesheetPtr style);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltSaveResultToString(
    _doc_txt_ptr: *mut *mut xmlChar,
    _doc_txt_len: *mut c_int,
    _result: *mut _xmlDoc,
    _style: *mut _xsltStylesheet,
) -> c_int {
    // Phase 1: STUB
    0
}

// ═══════════════════════════════════════════════════════════════════════════════
// 8. Debug/Utility
// ═══════════════════════════════════════════════════════════════════════════════

/// Get the stylesheet's document.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDocPtr xsltGetStylesheetDoc(xsltStylesheetPtr style);
/// ```
#[no_mangle]
pub extern "C" fn xsltGetStylesheetDoc(_style: *mut _xsltStylesheet) -> *mut _xmlDoc {
    // Phase 1: STUB
    ptr::null_mut()
}

/// Set the stylesheet's document.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltSetStylesheetDoc(xsltStylesheetPtr style, xmlDocPtr doc);
/// ```
#[no_mangle]
pub extern "C" fn xsltSetStylesheetDoc(_style: *mut _xsltStylesheet, _doc: *mut _xmlDoc) {
    // Phase 1: STUB
}

/// Check whether a feature is available.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltCheckFeature(int feature);
/// ```
#[no_mangle]
pub extern "C" fn xsltCheckFeature(_feature: c_int) -> c_int {
    // Phase 1: STUB — all features reported as available
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
    // Phase 1: STUB
    crate::abi::versioning::xsltLibxsltVersionString()
}

/// Set loader function for XSLT.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltSetLoaderFunc(xsltLoaderFunc loader);
/// ```
#[no_mangle]
pub extern "C" fn xsltSetLoaderFunc(
    _loader: Option<
        unsafe extern "C" fn(
            *mut c_void,
            *const c_char,
            *const c_char,
            *const c_char,
            c_int,
        ) -> *mut _xmlParserInput,
    >,
) {
    // Phase 1: STUB
}

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
    _ctxt: *mut _xsltTransformContext,
    _ctx: *mut c_void,
    _handler: Option<unsafe extern "C" fn(*mut c_void, *const c_char)>,
) {
    // Phase 1: STUB
}
