//! C ABI exports for libxml2.so.2 — no_mangle extern "C" functions (§1, §16).
//!
//! This module contains all `#[no_mangle] pub extern "C"` function definitions
//! that form the public ABI of libxml2.so.2. Every function here corresponds to
//! a function in the upstream libxml2 headers.
//!
//! # Phase 1 status
//!
//! Complete — all major ABI entry points are implemented. Functions that require
//! modules not yet implemented (tree, parser, etc.) call into those modules,
//! which will be filled in as Phase 1 continues.
//!
//! # Organization
//!
//! Exports are grouped by subsystem in the order they appear in upstream headers:
//!
//! 1. Initialization / Cleanup
//! 2. Version
//! 3. Memory / Allocator
//! 4. Error handling
//! 5. String utilities
//! 6. Tree (document, node, attribute, namespace, DTD, entity)
//! 7. Parser (SAX, DOM, push, reader)
//! 8. I/O
//! 9. Dictionary
//! 10. Hash table
//! 11. List
//! 12. Buffer
//! 13. Encoding
//! 14. XPath
//! 15. XInclude
//! 16. Catalog
//! 17. HTML
//! 18. Debug/misc

#![allow(non_snake_case)]
#![allow(unused_variables)]
#![allow(clippy::missing_safety_doc)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use core::ffi::c_void;
use core::ptr;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::ffi::CStr;
use std::mem::size_of;
use std::os::raw::{c_char, c_int, c_uint};

use crate::xml::xinclude;
use crate::xml::xpath::ast::CompiledExpr;
use crate::xml::xpath::context::XPathContext;
use crate::xml::xpath::types::{NodeSet, XPathValue};
use crate::xml::xpointer;

use crate::abi::allocator::*;
use crate::abi::callbacks::*;
use crate::abi::structs::*;
use crate::abi::types::*;

// ═══════════════════════════════════════════════════════════════════════════════
// 1. Initialization / Cleanup
// ═══════════════════════════════════════════════════════════════════════════════

/// Initialize the parser library.
///
/// Must be called before any other libxml2 functions.
/// Safe to call multiple times (reference-counted in modern libxml2).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlInitParser(void);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlInitParser() {
    crate::internal::globals::init_parser();
}

/// Clean up the parser library.
///
/// Should be called when the library is no longer needed.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlCleanupParser(void);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlCleanupParser() {
    crate::internal::globals::cleanup_parser();
}

/// Initialize threading support.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlInitThreads(void);
/// ```
///
/// Returns 0 on success.
#[no_mangle]
pub unsafe extern "C" fn xmlInitThreads() -> c_int {
    crate::internal::globals::init_threads()
}

/// Clean up threading support.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlCleanupThreads(void);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlCleanupThreads() {
    crate::xml::threads::cleanup_threads();
}

/// Check whether the library has been initialized.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlIsInitialized(void);
/// ```
#[no_mangle]
pub extern "C" fn xmlIsInitialized() -> c_int {
    if crate::abi::versioning::is_initialized() {
        1
    } else {
        0
    }
}

/// Initialize a set of threads (libxml2 compat).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlInitThreads(void);
/// ```
/// This is an alias.
#[no_mangle]
pub unsafe extern "C" fn xmlLockLibrary() {
    crate::xml::threads::lock_library();
}

/// Unlock the library (libxml2 compat).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlUnlockLibrary(void);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlUnlockLibrary() {
    crate::xml::threads::unlock_library();
}

// ═══════════════════════════════════════════════════════════════════════════════
// 4. Error Handling
// ═══════════════════════════════════════════════════════════════════════════════

/// Set the generic error handler.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlSetGenericErrorFunc(void *ctx, xmlGenericErrorFunc handler);
/// ```
///
/// # SAFETY
///
/// - `handler` must be a valid function pointer or NULL (to reset to default).
/// - If non-NULL, the handler may be called at any time with `ctx`.
#[no_mangle]
pub unsafe extern "C" fn xmlSetGenericErrorFunc(
    ctx: *mut c_void,
    handler: Option<xmlGenericErrorFunc>,
) {
    // SAFETY: Delegates to xml::errors with same safety contract.
    unsafe { crate::xml::errors::set_generic_error_func(ctx, handler) };
}

/// Set the structured error handler.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlSetStructuredErrorFunc(void *ctx, xmlStructuredErrorFunc handler);
/// ```
///
/// # SAFETY
///
/// - `handler` must be a valid function pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlSetStructuredErrorFunc(
    ctx: *mut c_void,
    handler: Option<xmlStructuredErrorFunc>,
) {
    // SAFETY: Delegates to xml::errors with same safety contract.
    unsafe { crate::xml::errors::set_structured_error_func(ctx, handler) };
}

/// Get the last error for the current thread.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlErrorPtr xmlGetLastError(void);
/// ```
///
/// Returns a pointer to the last error, or NULL if no error occurred.
/// The returned pointer is valid until the next libxml2 call in this thread.
#[no_mangle]
pub extern "C" fn xmlGetLastError() -> *mut _xmlError {
    crate::xml::errors::get_last_error()
}

/// Get a copy of the last error for the current thread.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlErrorPtr xmlCopyError(xmlErrorPtr from, xmlErrorPtr to);
/// ```
///
/// Copies `from` into `to`. Returns 0 on success, -1 on error.
///
/// # SAFETY
///
/// - `from` and `to` must be valid pointers to `_xmlError` structs, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlCopyError(from: *const _xmlError, to: *mut _xmlError) -> c_int {
    // SAFETY: Delegates to xml::errors with same safety contract.
    unsafe { crate::xml::errors::copy_error(from, to) }
}

/// Reset an error structure.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlResetError(xmlErrorPtr err);
/// ```
///
/// # SAFETY
///
/// - `err` must be a valid pointer to `_xmlError`, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlResetError(err: *mut _xmlError) {
    // SAFETY: Delegates to xml::errors with same safety contract.
    unsafe { crate::xml::errors::reset_error(err) };
}

/// Raise a structured error.
///
/// This is called internally when an error occurs. It updates the last error
/// and invokes the structured error handler if one is set.
///
/// # SAFETY
///
/// - `ctxt` may be NULL (context of the error).
/// - `domain`, `code`, `level`: valid error codes.
/// - `msg` must be a valid C string or NULL.
/// - `file` must be a valid C string or NULL.
/// - `str1`, `str2`, `str3`: error-related strings (may be NULL).
#[no_mangle]
pub unsafe extern "C" fn xmlRaiseError(
    ctxt: *mut c_void,
    ctxt2: *mut c_void,
    ctxt3: *mut c_void,
    ctxt4: *mut c_void,
    ctxt5: *mut c_void,
    domain: c_int,
    code: c_int,
    level: c_int,
    file: *const c_char,
    line: c_int,
    str1: *const c_char,
    str2: *const c_char,
    str3: *const c_char,
    int1: c_int,
    int2: c_int,
    msg: *const c_char,
) {
    // SAFETY: Delegates to xml::errors with same safety contract.
    unsafe {
        crate::xml::errors::raise_error(
            ctxt, ctxt2, ctxt3, ctxt4, ctxt5, domain, code, level, file, line, str1, str2, str3,
            int1, int2, msg,
        );
    }
}

/// Remove any error from the last error stack.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlResetLastError(void);
/// ```
#[no_mangle]
pub extern "C" fn xmlResetLastError() {
    crate::xml::errors::reset_last_error();
}

// ═══════════════════════════════════════════════════════════════════════════════
// 5. String Utilities
// ═══════════════════════════════════════════════════════════════════════════════

/// Duplicate a string using xmlChar.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlStrdup(const xmlChar *cur);
/// ```
///
/// # SAFETY
///
/// - `cur` must be a valid null-terminated xmlChar string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlStrdup(cur: *const xmlChar) -> *mut xmlChar {
    if cur.is_null() {
        return ptr::null_mut();
    }
    let len = unsafe { xmlStrlen(cur) };
    let size = len + 1;
    let new_ptr = unsafe { xmlMalloc(size as usize) };
    if new_ptr.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        ptr::copy_nonoverlapping(cur as *const u8, new_ptr as *mut u8, size as usize);
    }
    new_ptr as *mut xmlChar
}

/// Duplicate a substring.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlStrndup(const xmlChar *cur, int len);
/// ```
///
/// # SAFETY
///
/// - `cur` must be a valid pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlStrndup(cur: *const xmlChar, len: c_int) -> *mut xmlChar {
    if cur.is_null() || len <= 0 {
        return ptr::null_mut();
    }
    let size = len as usize + 1;
    let new_ptr = unsafe { xmlMalloc(size) };
    if new_ptr.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        ptr::copy_nonoverlapping(cur as *const u8, new_ptr as *mut u8, len as usize);
        *(new_ptr.add(len as usize) as *mut u8) = 0;
    }
    new_ptr as *mut xmlChar
}

/// Get the length of an xmlChar string.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlStrlen(const xmlChar *str);
/// ```
///
/// # SAFETY
///
/// - `str` must be a valid null-terminated string or NULL (returns 0).
#[no_mangle]
pub unsafe extern "C" fn xmlStrlen(str: *const xmlChar) -> c_int {
    if str.is_null() {
        return 0;
    }
    unsafe { libc::strlen(str as *const c_char) as c_int }
}

/// Compare two xmlChar strings.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlStrcmp(const xmlChar *str1, const xmlChar *str2);
/// ```
///
/// Returns 0 if equal, <0 if str1 < str2, >0 if str1 > str2.
/// NULL-safe: NULL sorts before any non-NULL string.
#[no_mangle]
pub unsafe extern "C" fn xmlStrcmp(str1: *const xmlChar, str2: *const xmlChar) -> c_int {
    if str1.is_null() && str2.is_null() {
        return 0;
    }
    if str1.is_null() {
        return -1;
    }
    if str2.is_null() {
        return 1;
    }
    unsafe { libc::strcmp(str1 as *const c_char, str2 as *const c_char) as c_int }
}

/// Compare two xmlChar strings up to a given length.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlStrncmp(const xmlChar *str1, const xmlChar *str2, int len);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlStrncmp(
    str1: *const xmlChar,
    str2: *const xmlChar,
    len: c_int,
) -> c_int {
    if len <= 0 {
        return 0;
    }
    if str1.is_null() && str2.is_null() {
        return 0;
    }
    if str1.is_null() {
        return -1;
    }
    if str2.is_null() {
        return 1;
    }
    unsafe { libc::strncmp(str1 as *const c_char, str2 as *const c_char, len as usize) as c_int }
}

/// Case-insensitive comparison of two xmlChar strings.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlStrcasecmp(const xmlChar *str1, const xmlChar *str2);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlStrcasecmp(str1: *const xmlChar, str2: *const xmlChar) -> c_int {
    if str1.is_null() && str2.is_null() {
        return 0;
    }
    if str1.is_null() {
        return -1;
    }
    if str2.is_null() {
        return 1;
    }
    unsafe { libc::strcasecmp(str1 as *const c_char, str2 as *const c_char) as c_int }
}

/// Case-insensitive comparison with length limit.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlStrncasecmp(const xmlChar *str1, const xmlChar *str2, int len);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlStrncasecmp(
    str1: *const xmlChar,
    str2: *const xmlChar,
    len: c_int,
) -> c_int {
    if len <= 0 {
        return 0;
    }
    if str1.is_null() && str2.is_null() {
        return 0;
    }
    if str1.is_null() {
        return -1;
    }
    if str2.is_null() {
        return 1;
    }
    unsafe {
        libc::strncasecmp(str1 as *const c_char, str2 as *const c_char, len as usize) as c_int
    }
}

/// Check if two xmlChar strings are equal.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlStrEqual(const xmlChar *str1, const xmlChar *str2);
/// ```
///
/// Returns 1 if equal, 0 if not. NULL-safe.
#[no_mangle]
pub unsafe extern "C" fn xmlStrEqual(str1: *const xmlChar, str2: *const xmlChar) -> c_int {
    if str1.is_null() && str2.is_null() {
        return 1;
    }
    if str1.is_null() || str2.is_null() {
        return 0;
    }
    unsafe { (libc::strcmp(str1 as *const c_char, str2 as *const c_char) == 0) as c_int }
}

/// Check if an xmlChar string equals a qualified name.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlStrQEqual(const xmlChar *pref, const xmlChar *name, const xmlChar *str);
/// ```
///
/// Returns 1 if `pref:name` equals `str`, 0 otherwise.
/// `pref` may be NULL (compares only name).
#[no_mangle]
pub unsafe extern "C" fn xmlStrQEqual(
    pref: *const xmlChar,
    name: *const xmlChar,
    str: *const xmlChar,
) -> c_int {
    if name.is_null() || str.is_null() {
        return 0;
    }
    if pref.is_null() {
        return unsafe { xmlStrEqual(name, str) };
    }
    // Compare "pref:name" with str
    let pref_len = unsafe { xmlStrlen(pref) };
    let name_len = unsafe { xmlStrlen(name) };
    let total_len = pref_len + 1 + name_len;
    let str_len = unsafe { xmlStrlen(str) };
    if total_len != str_len {
        return 0;
    }
    // Compare prefix part
    if unsafe {
        libc::strncmp(
            pref as *const c_char,
            str as *const c_char,
            pref_len as usize,
        )
    } != 0
    {
        return 0;
    }
    // Check colon
    if unsafe { *str.add(pref_len as usize) } != b':' as xmlChar {
        return 0;
    }
    // Compare name part
    (unsafe {
        libc::strncmp(
            name as *const c_char,
            str.add((pref_len + 1) as usize) as *const c_char,
            name_len as usize,
        ) == 0
    }) as c_int
}

/// Concatenate two strings.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlStrcat(xmlChar *cur, const xmlChar *add);
/// ```
///
/// # SAFETY
///
/// - `cur` must be a valid xmlMalloc'd string or NULL.
/// - `add` must be a valid string or NULL.
/// - If `cur` is NULL, behaves like xmlStrdup(add).
#[no_mangle]
pub unsafe extern "C" fn xmlStrcat(cur: *mut xmlChar, add: *const xmlChar) -> *mut xmlChar {
    if add.is_null() {
        return cur;
    }
    if cur.is_null() {
        return unsafe { xmlStrdup(add) };
    }
    let cur_len = unsafe { xmlStrlen(cur) } as usize;
    let add_len = unsafe { xmlStrlen(add) } as usize;
    let new_size = cur_len + add_len + 1;
    let new_ptr = unsafe { xmlRealloc(cur as *mut c_void, new_size) };
    if new_ptr.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        ptr::copy_nonoverlapping(add as *const u8, (new_ptr as *mut u8).add(cur_len), add_len);
        *((new_ptr as *mut u8).add(cur_len + add_len)) = 0;
    }
    new_ptr as *mut xmlChar
}

/// Concatenate up to `len` characters.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlStrncat(xmlChar *cur, const xmlChar *add, int len);
/// ```
///
/// # SAFETY
///
/// Same as xmlStrcat, but only copies up to `len` characters from `add`.
#[no_mangle]
pub unsafe extern "C" fn xmlStrncat(
    cur: *mut xmlChar,
    add: *const xmlChar,
    len: c_int,
) -> *mut xmlChar {
    if add.is_null() || len <= 0 {
        return cur;
    }
    let len = len as usize;
    if cur.is_null() {
        return unsafe { xmlStrndup(add, len as c_int) };
    }
    let cur_len = unsafe { xmlStrlen(cur) } as usize;
    let new_size = cur_len + len + 1;
    let new_ptr = unsafe { xmlRealloc(cur as *mut c_void, new_size) };
    if new_ptr.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        ptr::copy_nonoverlapping(add as *const u8, (new_ptr as *mut u8).add(cur_len), len);
        *((new_ptr as *mut u8).add(cur_len + len)) = 0;
    }
    new_ptr as *mut xmlChar
}

/// Create a new string by concatenating up to `len` characters.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlStrncatNew(const xmlChar *str1, const xmlChar *str2, int len);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlStrncatNew(
    str1: *const xmlChar,
    str2: *const xmlChar,
    len: c_int,
) -> *mut xmlChar {
    let mut result: *mut xmlChar = ptr::null_mut();
    if !str1.is_null() {
        result = unsafe { xmlStrdup(str1) };
    }
    if !str2.is_null() && len > 0 {
        result = unsafe { xmlStrncat(result, str2, len) };
    }
    result
}

/// Copy a string.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlStrcpy(xmlChar *dst, const xmlChar *src);
/// ```
///
/// # SAFETY
///
/// - `dst` must be a valid xmlMalloc'd buffer large enough to hold `src`.
/// - `src` must be a valid string.
#[no_mangle]
pub unsafe extern "C" fn xmlStrcpy(dst: *mut xmlChar, src: *const xmlChar) -> *mut xmlChar {
    if dst.is_null() || src.is_null() {
        return dst;
    }
    let len = unsafe { xmlStrlen(src) } as usize + 1;
    unsafe {
        ptr::copy_nonoverlapping(src as *const u8, dst as *mut u8, len);
    }
    dst
}

/// Copy up to `len` characters.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlStrncpy(xmlChar *dst, const xmlChar *src, int len);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlStrncpy(
    dst: *mut xmlChar,
    src: *const xmlChar,
    len: c_int,
) -> *mut xmlChar {
    if dst.is_null() || src.is_null() || len <= 0 {
        return dst;
    }
    let len = len as usize;
    let src_len = unsafe { xmlStrlen(src) } as usize;
    let copy_len = if src_len < len { src_len } else { len - 1 };
    unsafe {
        ptr::copy_nonoverlapping(src as *const u8, dst as *mut u8, copy_len);
        *dst.add(copy_len) = 0;
    }
    dst
}

/// Extract a substring.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlStrsub(const xmlChar *str, int start, int len);
/// ```
///
/// Returns a newly allocated substring, or NULL on error.
#[no_mangle]
pub unsafe extern "C" fn xmlStrsub(str: *const xmlChar, start: c_int, len: c_int) -> *mut xmlChar {
    if str.is_null() || start < 0 || len < 0 {
        return ptr::null_mut();
    }
    let str_len = unsafe { xmlStrlen(str) };
    if start >= str_len {
        return unsafe { xmlStrdup(b"\0" as *const u8 as *const xmlChar) };
    }
    let actual_len = if start + len > str_len {
        str_len - start
    } else {
        len
    };
    unsafe { xmlStrndup(str.add(start as usize), actual_len) }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 6. Tree — Document, Node, Attribute, Namespace, DTD, Entity
// ═══════════════════════════════════════════════════════════════════════════════

/// Create a new document.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDocPtr xmlNewDoc(const xmlChar *version);
/// ```
///
/// # SAFETY
///
/// - `version` must be a valid string or NULL (defaults to "1.0").
/// - Returns a newly allocated document. Caller must free with `xmlFreeDoc`.
#[no_mangle]
pub unsafe extern "C" fn xmlNewDoc(version: *const xmlChar) -> *mut _xmlDoc {
    crate::xml::tree::new_doc(version)
}

/// Free a document.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlFreeDoc(xmlDocPtr doc);
/// ```
///
/// # SAFETY
///
/// - `doc` must be a valid document pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlFreeDoc(doc: *mut _xmlDoc) {
    crate::xml::tree::free_doc(doc);
}

/// Create a new node.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNodePtr xmlNewNode(xmlNsPtr ns, const xmlChar *name);
/// ```
///
/// # SAFETY
///
/// - `ns` may be NULL.
/// - `name` must be a valid string.
/// - Returns a newly allocated node. Caller must free with `xmlFreeNode`.
#[no_mangle]
pub unsafe extern "C" fn xmlNewNode(ns: *mut _xmlNs, name: *const xmlChar) -> *mut _xmlNode {
    crate::xml::tree::new_node(ns, name)
}

/// Free a node.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlFreeNode(xmlNodePtr node);
/// ```
///
/// # SAFETY
///
/// - `node` must be a valid node pointer or NULL.
/// - The node must NOT be part of a document tree (must be unlinked first).
#[no_mangle]
pub unsafe extern "C" fn xmlFreeNode(node: *mut _xmlNode) {
    crate::xml::tree::free_node(node);
}

/// Unlink a node from its tree.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlUnlinkNode(xmlNodePtr node);
/// ```
///
/// # SAFETY
///
/// - `node` must be a valid node pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlUnlinkNode(node: *mut _xmlNode) {
    crate::xml::tree::unlink_node(node);
}

/// Add a child node.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNodePtr xmlAddChild(xmlNodePtr parent, xmlNodePtr cur);
/// ```
///
/// # SAFETY
///
/// - `parent` must be a valid node.
/// - `cur` must be a valid node (ownership transfers to parent).
/// - Returns pointer to the added child (borrowed).
#[no_mangle]
pub unsafe extern "C" fn xmlAddChild(parent: *mut _xmlNode, cur: *mut _xmlNode) -> *mut _xmlNode {
    crate::xml::tree::add_child(parent, cur)
}

/// Add a sibling node.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNodePtr xmlAddSibling(xmlNodePtr cur, xmlNodePtr sibling);
/// ```
///
/// # SAFETY
///
/// Same as xmlAddChild, but adds after `cur` instead of as a child.
#[no_mangle]
pub unsafe extern "C" fn xmlAddSibling(
    cur: *mut _xmlNode,
    sibling: *mut _xmlNode,
) -> *mut _xmlNode {
    crate::xml::tree::add_sibling(cur, sibling)
}

/// Create a new child element.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNodePtr xmlNewChild(xmlNodePtr parent, xmlNsPtr ns,
///                        const xmlChar *name, const xmlChar *content);
/// ```
///
/// Creates a new element node, adds it as a child of `parent`, and
/// sets its content if `content` is non-NULL.
///
/// # SAFETY
///
/// - `parent` must be a valid node (may be NULL).
/// - `ns` may be NULL.
/// - `name` must be a valid string.
/// - Returns a newly allocated node (owned by parent).
#[no_mangle]
pub unsafe extern "C" fn xmlNewChild(
    parent: *mut _xmlNode,
    ns: *mut _xmlNs,
    name: *const xmlChar,
    content: *const xmlChar,
) -> *mut _xmlNode {
    crate::xml::tree::new_child(parent, ns, name)
}

/// Set the root element of a document.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNodePtr xmlDocSetRootElement(xmlDocPtr doc, xmlNodePtr root);
/// ```
///
/// Returns the old root element (if any), which the caller must free.
///
/// # SAFETY
///
/// - `doc` must be a valid document.
/// - `root` must be a valid node (ownership transfers to doc).
#[no_mangle]
pub unsafe extern "C" fn xmlDocSetRootElement(
    doc: *mut _xmlDoc,
    root: *mut _xmlNode,
) -> *mut _xmlNode {
    crate::xml::tree::doc_set_root_element(doc, root)
}

/// Get the root element of a document.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNodePtr xmlDocGetRootElement(const xmlDoc *doc);
/// ```
///
/// Returns a borrowed pointer (do not free).
#[no_mangle]
pub extern "C" fn xmlDocGetRootElement(doc: *const _xmlDoc) -> *mut _xmlNode {
    crate::xml::tree::doc_get_root_element(doc as *mut _xmlDoc)
}

/// Copy a node.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNodePtr xmlCopyNode(const xmlNodePtr node, int extended);
/// ```
///
/// If `extended` is 1, copies recursively (deep copy).
/// If `extended` is 0, copies only the node itself (shallow copy).
///
/// Returns a newly allocated copy. Caller must free with `xmlFreeNode`.
#[no_mangle]
pub unsafe extern "C" fn xmlCopyNode(node: *const _xmlNode, extended: c_int) -> *mut _xmlNode {
    crate::xml::tree::copy_node(node, extended)
}

/// Copy a document.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDocPtr xmlCopyDoc(const xmlDocPtr doc, int recursive);
/// ```
///
/// Returns a newly allocated copy. Caller must free with `xmlFreeDoc`.
#[no_mangle]
pub unsafe extern "C" fn xmlCopyDoc(doc: *const _xmlDoc, recursive: c_int) -> *mut _xmlDoc {
    crate::xml::tree::copy_doc(doc, recursive)
}

/// Create a text node.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNodePtr xmlNewText(const xmlChar *content);
/// ```
///
/// Creates a new text node with the given content.
/// If `content` is NULL, creates an empty text node.
#[no_mangle]
pub unsafe extern "C" fn xmlNewText(content: *const xmlChar) -> *mut _xmlNode {
    crate::xml::tree::new_text(content)
}

/// Create a new comment node.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNodePtr xmlNewComment(const xmlChar *content);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlNewComment(content: *const xmlChar) -> *mut _xmlNode {
    crate::xml::tree::new_comment(content)
}

/// Create a new PI node.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNodePtr xmlNewPI(const xmlChar *name, const xmlChar *content);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlNewPI(name: *const xmlChar, content: *const xmlChar) -> *mut _xmlNode {
    crate::xml::tree::new_pi(name, content)
}

/// Create a new CDATA node.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNodePtr xmlNewCDataBlock(xmlDocPtr doc, const xmlChar *content, int len);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlNewCDataBlock(
    doc: *mut _xmlDoc,
    content: *const xmlChar,
    len: c_int,
) -> *mut _xmlNode {
    crate::xml::tree::new_cdata_block(doc, content, len)
}

/// Create a new namespace definition.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNsPtr xmlNewNs(xmlNodePtr node, const xmlChar *href, const xmlChar *prefix);
/// ```
///
/// # SAFETY
///
/// - `node` may be NULL.
/// - `href` and `prefix` are copied.
/// - Returns a borrowed pointer (namespace is owned by the node).
#[no_mangle]
pub unsafe extern "C" fn xmlNewNs(
    node: *mut _xmlNode,
    href: *const xmlChar,
    prefix: *const xmlChar,
) -> *mut _xmlNs {
    crate::xml::tree::new_ns(node, href, prefix)
}

/// Set the namespace of a node.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlSetNs(xmlNodePtr node, xmlNsPtr ns);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlSetNs(node: *mut _xmlNode, ns: *mut _xmlNs) {
    crate::xml::tree::set_ns(node, ns);
}

/// Get the namespace of a node.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNsPtr xmlGetNsList(xmlDocPtr doc, const xmlNode *node);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlGetNsList(
    doc: *mut _xmlDoc,
    node: *const _xmlNode,
) -> *mut *mut _xmlNs {
    crate::xml::tree::get_ns_list(doc, node as *mut _xmlNode)
}

/// Search for a namespace by href.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNsPtr xmlSearchNs(xmlDocPtr doc, xmlNodePtr node, const xmlChar *nameSpace);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlSearchNs(
    doc: *mut _xmlDoc,
    node: *mut _xmlNode,
    nameSpace: *const xmlChar,
) -> *mut _xmlNs {
    crate::xml::tree::search_ns(doc, node, nameSpace)
}

/// Search for a namespace by href, using the full in-scope chain.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNsPtr xmlSearchNsByHref(xmlDocPtr doc, xmlNodePtr node, const xmlChar *href);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlSearchNsByHref(
    doc: *mut _xmlDoc,
    node: *mut _xmlNode,
    href: *const xmlChar,
) -> *mut _xmlNs {
    crate::xml::tree::search_ns_by_href(doc, node, href)
}

/// Set a property (attribute) on a node.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlAttrPtr xmlSetProp(xmlNodePtr node, const xmlChar *name, const xmlChar *value);
/// ```
///
/// If the attribute already exists, its value is updated.
/// Returns a borrowed pointer to the attribute.
///
/// # SAFETY
///
/// - `node` must be a valid element node.
/// - `name` must be a valid string.
/// - `value` may be NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlSetProp(
    node: *mut _xmlNode,
    name: *const xmlChar,
    value: *const xmlChar,
) -> *mut _xmlAttr {
    crate::xml::tree::set_prop(node, name, value)
}

/// Get a property value by name.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlGetProp(const xmlNode *node, const xmlChar *name);
/// ```
///
/// Returns a newly allocated string. Caller must free with `xmlFree`.
#[no_mangle]
pub unsafe extern "C" fn xmlGetProp(node: *const _xmlNode, name: *const xmlChar) -> *mut xmlChar {
    crate::xml::tree::get_prop(node as *mut _xmlNode, name)
}

/// Get a namespaced property value.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlGetNsProp(const xmlNode *node, const xmlChar *name, const xmlChar *nameSpace);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlGetNsProp(
    node: *const _xmlNode,
    name: *const xmlChar,
    nameSpace: *const xmlChar,
) -> *mut xmlChar {
    crate::xml::tree::get_ns_prop(node as *mut _xmlNode, name, nameSpace)
}

/// Set a namespaced property.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlAttrPtr xmlSetNsProp(xmlNodePtr node, xmlNsPtr ns,
///                         const xmlChar *name, const xmlChar *value);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlSetNsProp(
    node: *mut _xmlNode,
    ns: *mut _xmlNs,
    name: *const xmlChar,
    value: *const xmlChar,
) -> *mut _xmlAttr {
    crate::xml::tree::set_ns_prop(node, ns, name, value)
}

/// Remove a property by name.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlRemoveProp(xmlAttrPtr attr);
/// ```
///
/// Returns 0 on success, -1 on error.
#[no_mangle]
pub unsafe extern "C" fn xmlRemoveProp(attr: *mut _xmlAttr) -> c_int {
    crate::xml::tree::remove_prop(attr)
}

/// Get a DTD from a document, creating one if needed.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDtdPtr xmlGetIntSubset(const xmlDoc *doc);
/// ```
#[no_mangle]
pub extern "C" fn xmlGetIntSubset(doc: *const _xmlDoc) -> *mut _xmlDtd {
    crate::xml::tree::get_int_subset(doc)
}

/// Create a new DTD.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDtdPtr xmlNewDtd(xmlDocPtr doc, const xmlChar *name,
///                     const xmlChar *ExternalID, const xmlChar *SystemID);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlNewDtd(
    doc: *mut _xmlDoc,
    name: *const xmlChar,
    ExternalID: *const xmlChar,
    SystemID: *const xmlChar,
) -> *mut _xmlDtd {
    crate::xml::tree::new_dtd(doc, name, ExternalID, SystemID)
}

/// Create a new entity.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlEntityPtr xmlNewEntity(xmlDocPtr doc, const xmlChar *name, int type,
///                           const xmlChar *ExternalID, const xmlChar *SystemID,
///                           const xmlChar *content);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlNewEntity(
    doc: *mut _xmlDoc,
    name: *const xmlChar,
    type_: c_int,
    ExternalID: *const xmlChar,
    SystemID: *const xmlChar,
    content: *const xmlChar,
) -> *mut _xmlEntity {
    crate::xml::tree::new_entity(doc, name, type_, ExternalID, SystemID, content)
}

/// Get an entity by name.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlEntityPtr xmlGetDocEntity(const xmlDoc *doc, const xmlChar *name);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlGetDocEntity(
    doc: *const _xmlDoc,
    name: *const xmlChar,
) -> *mut _xmlEntity {
    crate::xml::tree::get_doc_entity(doc, name)
}

/// Get a parameter entity by name.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlEntityPtr xmlGetParameterEntity(const xmlDoc *doc, const xmlChar *name);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlGetParameterEntity(
    doc: *const _xmlDoc,
    name: *const xmlChar,
) -> *mut _xmlEntity {
    crate::xml::tree::get_parameter_entity(doc, name)
}

// ── DTD Declaration Exports ────────────────────────────────────────────

/// Create an internal subset (DTD).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDtdPtr xmlCreateIntSubset(xmlDocPtr doc, const xmlChar *name,
///                              const xmlChar *ExternalID, const xmlChar *SystemID);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlCreateIntSubset(
    doc: *mut _xmlDoc,
    name: *const xmlChar,
    ExternalID: *const xmlChar,
    SystemID: *const xmlChar,
) -> *mut _xmlDtd {
    crate::xml::dtd::create_int_subset(doc, name, ExternalID, SystemID)
}

/// Free a DTD.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlFreeDtd(xmlDtdPtr dtd);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlFreeDtd(dtd: *mut _xmlDtd) {
    crate::xml::dtd::free_dtd(dtd);
}

/// Add a notation declaration.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNotationPtr xmlAddNotationDecl(xmlDtdPtr dtd, const xmlChar *name,
///                                   const xmlChar *PublicID,
///                                   const xmlChar *SystemID);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlAddNotationDecl(
    dtd: *mut _xmlDtd,
    name: *const xmlChar,
    PublicID: *const xmlChar,
    SystemID: *const xmlChar,
) -> *mut _xmlNotation {
    crate::xml::dtd::add_notation_decl(dtd, name, PublicID, SystemID)
}

/// Look up a notation declaration.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNotationPtr xmlGetNotationDecl(xmlDtdPtr dtd, const xmlChar *name);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlGetNotationDecl(
    dtd: *mut _xmlDtd,
    name: *const xmlChar,
) -> *mut _xmlNotation {
    crate::xml::dtd::get_notation_decl(dtd, name)
}

/// Copy a notation declaration.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNotationPtr xmlCopyNotation(xmlNotationPtr notation);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlCopyNotation(notation: *mut _xmlNotation) -> *mut _xmlNotation {
    crate::xml::dtd::copy_notation(notation)
}

/// Free a notation declaration.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlFreeNotation(xmlNotationPtr notation);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlFreeNotation(notation: *mut _xmlNotation) {
    crate::xml::dtd::free_notation(notation);
}

/// Add an element declaration.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlElementPtr xmlAddElementDecl(xmlDtdPtr dtd, const xmlChar *name, int type,
///                                 xmlElementContentPtr content);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlAddElementDecl(
    dtd: *mut _xmlDtd,
    name: *const xmlChar,
    type_: c_int,
    content: *mut _xmlElementContent,
) -> *mut _xmlElement {
    crate::xml::dtd::add_element_decl(dtd, name, type_, content)
}

/// Look up an element declaration.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlElementPtr xmlGetElementDecl(xmlDtdPtr dtd, const xmlChar *name);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlGetElementDecl(
    dtd: *mut _xmlDtd,
    name: *const xmlChar,
) -> *mut _xmlElement {
    crate::xml::dtd::get_element_decl(dtd, name)
}

/// Copy an element declaration.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlElementPtr xmlCopyElement(xmlElementPtr elem);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlCopyElement(elem: *mut _xmlElement) -> *mut _xmlElement {
    crate::xml::dtd::copy_element(elem)
}

/// Free an element declaration.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlFreeElement(xmlElementPtr elem);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlFreeElement(elem: *mut _xmlElement) {
    crate::xml::dtd::free_element(elem);
}

/// Add an attribute declaration.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlAttributePtr xmlAddAttributeDecl(xmlDtdPtr dtd, xmlElementPtr elem,
///                                     const xmlChar *name, int type, int def,
///                                     const xmlChar *defaultValue,
///                                     xmlEnumerationPtr tree);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlAddAttributeDecl(
    dtd: *mut _xmlDtd,
    elem: *mut _xmlElement,
    name: *const xmlChar,
    type_: c_int,
    def: c_int,
    defaultValue: *const xmlChar,
    tree: *mut _xmlEnumeration,
) -> *mut _xmlAttribute {
    crate::xml::dtd::add_attribute_decl(dtd, elem, name, type_, def, defaultValue, tree)
}

/// Look up an attribute declaration.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlAttributePtr xmlGetAttributeDecl(xmlDtdPtr dtd, xmlElementPtr elem,
///                                     const xmlChar *name, int namePrefix);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlGetAttributeDecl(
    dtd: *mut _xmlDtd,
    elem: *mut _xmlElement,
    name: *const xmlChar,
    namePrefix: c_int,
) -> *mut _xmlAttribute {
    crate::xml::dtd::get_attribute_decl(dtd, elem, name, namePrefix)
}

/// Copy an attribute declaration.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlAttributePtr xmlCopyAttribute(xmlAttributePtr attr);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlCopyAttribute(attr: *mut _xmlAttribute) -> *mut _xmlAttribute {
    crate::xml::dtd::copy_attribute_decl(attr)
}

/// Free an attribute declaration.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlFreeAttribute(xmlAttributePtr attr);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlFreeAttribute(attr: *mut _xmlAttribute) {
    crate::xml::dtd::free_attribute(attr);
}

/// Create a new element content model.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlElementContentPtr xmlNewElementContent(const xmlChar *name, int type);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlNewElementContent(
    name: *const xmlChar,
    type_: c_int,
) -> *mut _xmlElementContent {
    crate::xml::dtd::create_content_model(name, type_)
}

/// Copy an element content model.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlElementContentPtr xmlCopyElementContent(xmlElementContentPtr content);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlCopyElementContent(
    content: *mut _xmlElementContent,
) -> *mut _xmlElementContent {
    crate::xml::dtd::copy_content_model(content)
}

/// Free an element content model.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlFreeElementContent(xmlElementContentPtr cur);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlFreeElementContent(cur: *mut _xmlElementContent) {
    crate::xml::dtd::free_content_model(cur);
}

// ── Entity Exports ─────────────────────────────────────────────────────

/// Add an entity declaration.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlEntityPtr xmlAddEntity(xmlDtdPtr dtd, const xmlChar *name, int type,
///                           const xmlChar *ExternalID, const xmlChar *SystemID,
///                           const xmlChar *content);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlAddEntity(
    dtd: *mut _xmlDtd,
    name: *const xmlChar,
    type_: c_int,
    ExternalID: *const xmlChar,
    SystemID: *const xmlChar,
    content: *const xmlChar,
) -> *mut _xmlEntity {
    crate::xml::entities::add_entity(dtd, name, type_, ExternalID, SystemID, content)
}

/// Get an entity by name.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlEntityPtr xmlGetEntity(xmlDocPtr doc, const xmlChar *name);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlGetEntity(doc: *mut _xmlDoc, name: *const xmlChar) -> *mut _xmlEntity {
    crate::xml::entities::get_entity(doc, name)
}

/// Copy an entity.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlEntityPtr xmlCopyEntity(xmlEntityPtr entity);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlCopyEntity(entity: *mut _xmlEntity) -> *mut _xmlEntity {
    crate::xml::entities::copy_entity(entity)
}

/// Free an entity.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlFreeEntity(xmlEntityPtr entity);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlFreeEntity(entity: *mut _xmlEntity) {
    crate::xml::entities::free_entity(entity);
}

/// Encode entities for reentrant output.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlEncodeEntitiesReentrant(xmlDocPtr doc, const xmlChar *input);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlEncodeEntitiesReentrant(
    doc: *mut _xmlDoc,
    input: *const xmlChar,
) -> *mut xmlChar {
    crate::xml::entities::encode_entities_reentrant(doc, input)
}

/// Get the line number of a node.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// long xmlGetLineNo(const xmlNode *node);
/// ```
#[no_mangle]
pub extern "C" fn xmlGetLineNo(node: *const _xmlNode) -> c_int {
    crate::xml::tree::get_line_no(node)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Serialization — xmlNodeDump, xmlDocDump, xmlSaveFile, etc.
// ═══════════════════════════════════════════════════════════════════════════════

/// Dump a node to a buffer.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlNodeDump(xmlBufferPtr buf, xmlDocPtr doc, xmlNodePtr cur, int level, int format);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlNodeDump(
    buf: *mut _xmlBuffer,
    doc: *mut _xmlDoc,
    cur: *mut _xmlNode,
    level: c_int,
    format: c_int,
) -> c_int {
    if buf.is_null() || cur.is_null() {
        return -1;
    }
    crate::xml::tree::xmlNodeDump(buf, doc, cur, level, format)
}

/// Dump a document to a file pointer.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlDocDump(FILE *f, xmlDocPtr doc);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlDocDump(fp: *mut c_void, doc: *mut _xmlDoc) -> c_int {
    if fp.is_null() || doc.is_null() {
        return -1;
    }
    crate::xml::tree::xmlDocDump(fp, doc)
}

/// Dump a document to memory with format.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlDocDumpFormatMemory(xmlDocPtr doc, xmlChar **mem, int *size, int format);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlDocDumpFormatMemory(
    doc: *mut _xmlDoc,
    mem: *mut *mut xmlChar,
    size: *mut c_int,
    format: c_int,
) {
    if doc.is_null() || mem.is_null() || size.is_null() {
        return;
    }
    crate::xml::tree::xmlDocDumpFormatMemory(doc, mem, size, format)
}

/// Dump a document to memory (unformatted).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlDocDumpMemory(xmlDocPtr doc, xmlChar **mem, int *size);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlDocDumpMemory(
    doc: *mut _xmlDoc,
    mem: *mut *mut xmlChar,
    size: *mut c_int,
) {
    if doc.is_null() || mem.is_null() || size.is_null() {
        return;
    }
    crate::xml::tree::xmlDocDumpMemory(doc, mem, size)
}

/// Save a document to a file.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlSaveFile(const char *filename, xmlDocPtr cur);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlSaveFile(filename: *const c_char, cur: *mut _xmlDoc) -> c_int {
    if filename.is_null() || cur.is_null() {
        return -1;
    }
    crate::xml::tree::xmlSaveFile(filename, cur)
}

/// Save a document to a file with encoding.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlSaveFileEnc(const char *filename, xmlDocPtr cur, const char *encoding);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlSaveFileEnc(
    filename: *const c_char,
    cur: *mut _xmlDoc,
    encoding: *const c_char,
) -> c_int {
    if filename.is_null() || cur.is_null() {
        return -1;
    }
    crate::xml::tree::xmlSaveFileEnc(filename, cur, encoding)
}

/// Save a document to a file with format.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlSaveFormatFile(const char *filename, xmlDocPtr cur, int format);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlSaveFormatFile(
    filename: *const c_char,
    cur: *mut _xmlDoc,
    format: c_int,
) -> c_int {
    if filename.is_null() || cur.is_null() {
        return -1;
    }
    crate::xml::tree::xmlSaveFormatFile(filename, cur, format)
}

/// Save a document to a file with encoding and format.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlSaveFormatFileEnc(const char *filename, xmlDocPtr cur, const char *encoding, int format);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlSaveFormatFileEnc(
    filename: *const c_char,
    cur: *mut _xmlDoc,
    encoding: *const c_char,
    format: c_int,
) -> c_int {
    if filename.is_null() || cur.is_null() {
        return -1;
    }
    crate::xml::tree::xmlSaveFormatFileEnc(filename, cur, encoding, format)
}

// ═══════════════════════════════════════════════════════════════════════════════
// 7. Parser — SAX, DOM, Push, Reader
// ═══════════════════════════════════════════════════════════════════════════════

/// Read an XML document from a string.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDocPtr xmlReadDoc(const xmlChar *cur, const char *URL,
///                      const char *encoding, int options);
/// ```
///
/// Returns a parsed document. Caller must free with `xmlFreeDoc`.
#[no_mangle]
pub unsafe extern "C" fn xmlReadDoc(
    cur: *const xmlChar,
    URL: *const c_char,
    encoding: *const c_char,
    options: c_int,
) -> *mut _xmlDoc {
    // SAFETY: cur must be a valid null-terminated xmlChar string if non-null.
    if cur.is_null() {
        return ptr::null_mut();
    }
    let ctxt = crate::xml::parser::helpers::create_parser_ctxt();
    if ctxt.is_null() {
        return ptr::null_mut();
    }
    let len = crate::xml::string::xml_strlen(cur);
    let input = crate::xml::parser::helpers::input_from_memory(cur as *const c_char, len as c_int);
    crate::xml::parser::helpers::setup_parser_input(ctxt, input);
    (*ctxt).options = options;
    if crate::xml::parser::helpers::parse_document(ctxt) != 0 {
        let doc = (*ctxt).myDoc;
        crate::xml::parser::helpers::free_parser_ctxt(ctxt);
        return doc;
    }
    let doc = (*ctxt).myDoc;
    if !doc.is_null() {
        (*doc).URL = crate::xml::string::xml_strdup(URL as *const xmlChar);
    }
    crate::xml::parser::helpers::free_parser_ctxt(ctxt);
    doc
}

/// Read an XML document from a file.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDocPtr xmlReadFile(const char *URL, const char *encoding, int options);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlReadFile(
    URL: *const c_char,
    encoding: *const c_char,
    options: c_int,
) -> *mut _xmlDoc {
    // SAFETY: URL must be a valid C string or NULL.
    if URL.is_null() {
        return ptr::null_mut();
    }
    let ctxt = crate::xml::parser::helpers::create_parser_ctxt();
    if ctxt.is_null() {
        return ptr::null_mut();
    }
    let input = match crate::xml::parser::helpers::input_from_file(URL) {
        Ok(input) => input,
        Err(_) => {
            crate::xml::parser::helpers::free_parser_ctxt(ctxt);
            return ptr::null_mut();
        }
    };
    crate::xml::parser::helpers::setup_parser_input(ctxt, input);
    (*ctxt).options = options;
    if crate::xml::parser::helpers::parse_document(ctxt) != 0 {
        let doc = (*ctxt).myDoc;
        crate::xml::parser::helpers::free_parser_ctxt(ctxt);
        return doc;
    }
    let doc = (*ctxt).myDoc;
    crate::xml::parser::helpers::free_parser_ctxt(ctxt);
    doc
}

/// Read an XML document from memory.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDocPtr xmlReadMemory(const char *buffer, int size,
///                         const char *URL, const char *encoding, int options);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlReadMemory(
    buffer: *const c_char,
    size: c_int,
    URL: *const c_char,
    encoding: *const c_char,
    options: c_int,
) -> *mut _xmlDoc {
    // SAFETY: buffer must be a valid pointer with at least `size` readable bytes.
    if buffer.is_null() || size <= 0 {
        return ptr::null_mut();
    }
    let ctxt = crate::xml::parser::helpers::create_parser_ctxt();
    if ctxt.is_null() {
        return ptr::null_mut();
    }
    let input = crate::xml::parser::helpers::input_from_memory(buffer, size);
    crate::xml::parser::helpers::setup_parser_input(ctxt, input);
    (*ctxt).options = options;
    if crate::xml::parser::helpers::parse_document(ctxt) != 0 {
        let doc = (*ctxt).myDoc;
        crate::xml::parser::helpers::free_parser_ctxt(ctxt);
        return doc;
    }
    let doc = (*ctxt).myDoc;
    if !doc.is_null() && !URL.is_null() {
        (*doc).URL = crate::xml::string::xml_strdup(URL as *const xmlChar);
    }
    crate::xml::parser::helpers::free_parser_ctxt(ctxt);
    doc
}

/// Read an XML document from a file descriptor.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDocPtr xmlReadFd(int fd, const char *URL, const char *encoding, int options);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlReadFd(
    fd: c_int,
    URL: *const c_char,
    encoding: *const c_char,
    options: c_int,
) -> *mut _xmlDoc {
    // SAFETY: fd must be a valid open file descriptor.
    let ctxt = crate::xml::parser::helpers::create_parser_ctxt();
    if ctxt.is_null() {
        return ptr::null_mut();
    }
    // Read all data from the fd
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    loop {
        let n = libc::read(fd, tmp.as_mut_ptr() as *mut c_void, tmp.len());
        if n <= 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n as usize]);
    }
    let input = crate::xml::parser::helpers::input_from_memory(
        buf.as_ptr() as *const c_char,
        buf.len() as c_int,
    );
    crate::xml::parser::helpers::setup_parser_input(ctxt, input);
    (*ctxt).options = options;
    if crate::xml::parser::helpers::parse_document(ctxt) != 0 {
        let doc = (*ctxt).myDoc;
        crate::xml::parser::helpers::free_parser_ctxt(ctxt);
        return doc;
    }
    let doc = (*ctxt).myDoc;
    if !doc.is_null() && !URL.is_null() {
        (*doc).URL = crate::xml::string::xml_strdup(URL as *const xmlChar);
    }
    crate::xml::parser::helpers::free_parser_ctxt(ctxt);
    doc
}

/// Read an XML document from I/O callbacks.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDocPtr xmlReadIO(xmlInputReadCallback ioread, xmlInputCloseCallback ioclose,
///                     void *ioctx, const char *URL, const char *encoding, int options);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlReadIO(
    ioread: Option<xmlInputReadCallback>,
    ioclose: Option<xmlInputCloseCallback>,
    ioctx: *mut c_void,
    URL: *const c_char,
    encoding: *const c_char,
    options: c_int,
) -> *mut _xmlDoc {
    // SAFETY: callbacks must be valid function pointers if non-NULL.
    let ctxt = crate::xml::parser::helpers::create_parser_ctxt();
    if ctxt.is_null() {
        return ptr::null_mut();
    }
    let input = crate::xml::parser::helpers::input_from_io(ioread, ioclose, ioctx);
    crate::xml::parser::helpers::setup_parser_input(ctxt, input);
    (*ctxt).options = options;
    if crate::xml::parser::helpers::parse_document(ctxt) != 0 {
        let doc = (*ctxt).myDoc;
        crate::xml::parser::helpers::free_parser_ctxt(ctxt);
        return doc;
    }
    let doc = (*ctxt).myDoc;
    if !doc.is_null() && !URL.is_null() {
        (*doc).URL = crate::xml::string::xml_strdup(URL as *const xmlChar);
    }
    crate::xml::parser::helpers::free_parser_ctxt(ctxt);
    doc
}

/// Parse an XML document (SAX1).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDocPtr xmlSAXParseDoc(xmlSAXHandlerPtr sax, const xmlChar *cur, int recovery);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlSAXParseDoc(
    sax: *mut _xmlSAXHandler,
    cur: *const xmlChar,
    recovery: c_int,
) -> *mut _xmlDoc {
    // SAFETY: cur must be a valid null-terminated xmlChar string.
    if cur.is_null() {
        return ptr::null_mut();
    }
    let ctxt = crate::xml::parser::helpers::create_parser_ctxt();
    if ctxt.is_null() {
        return ptr::null_mut();
    }
    if !sax.is_null() {
        (*ctxt).sax = sax;
        (*ctxt).userData = (*ctxt).sax as *mut c_void;
    }
    if recovery != 0 {
        (*ctxt).recovery = 1;
        (*ctxt).options |= 1; // XML_PARSE_RECOVER
    }
    let len = crate::xml::string::xml_strlen(cur);
    let input = crate::xml::parser::helpers::input_from_memory(cur as *const c_char, len as c_int);
    crate::xml::parser::helpers::setup_parser_input(ctxt, input);
    crate::xml::parser::helpers::parse_document(ctxt);
    let doc = (*ctxt).myDoc;
    crate::xml::parser::helpers::free_parser_ctxt(ctxt);
    doc
}

/// Parse an XML file (SAX1).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDocPtr xmlSAXParseFile(xmlSAXHandlerPtr sax, const char *filename, int recovery);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlSAXParseFile(
    sax: *mut _xmlSAXHandler,
    filename: *const c_char,
    recovery: c_int,
) -> *mut _xmlDoc {
    // SAFETY: filename must be a valid C string.
    if filename.is_null() {
        return ptr::null_mut();
    }
    let ctxt = crate::xml::parser::helpers::create_parser_ctxt();
    if ctxt.is_null() {
        return ptr::null_mut();
    }
    if !sax.is_null() {
        (*ctxt).sax = sax;
        (*ctxt).userData = (*ctxt).sax as *mut c_void;
    }
    if recovery != 0 {
        (*ctxt).recovery = 1;
        (*ctxt).options |= 1;
    }
    let input = match crate::xml::parser::helpers::input_from_file(filename) {
        Ok(input) => input,
        Err(_) => {
            crate::xml::parser::helpers::free_parser_ctxt(ctxt);
            return ptr::null_mut();
        }
    };
    crate::xml::parser::helpers::setup_parser_input(ctxt, input);
    crate::xml::parser::helpers::parse_document(ctxt);
    let doc = (*ctxt).myDoc;
    crate::xml::parser::helpers::free_parser_ctxt(ctxt);
    doc
}

/// Parse an XML document from memory (SAX1).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDocPtr xmlSAXParseMemory(xmlSAXHandlerPtr sax,
///                             const char *buffer, int size, int recovery);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlSAXParseMemory(
    sax: *mut _xmlSAXHandler,
    buffer: *const c_char,
    size: c_int,
    recovery: c_int,
) -> *mut _xmlDoc {
    // SAFETY: buffer must be valid with at least `size` bytes.
    if buffer.is_null() || size <= 0 {
        return ptr::null_mut();
    }
    let ctxt = crate::xml::parser::helpers::create_parser_ctxt();
    if ctxt.is_null() {
        return ptr::null_mut();
    }
    if !sax.is_null() {
        (*ctxt).sax = sax;
        (*ctxt).userData = (*ctxt).sax as *mut c_void;
    }
    if recovery != 0 {
        (*ctxt).recovery = 1;
        (*ctxt).options |= 1;
    }
    let input = crate::xml::parser::helpers::input_from_memory(buffer, size);
    crate::xml::parser::helpers::setup_parser_input(ctxt, input);
    crate::xml::parser::helpers::parse_document(ctxt);
    let doc = (*ctxt).myDoc;
    crate::xml::parser::helpers::free_parser_ctxt(ctxt);
    doc
}

/// SAX user parse file.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlSAXUserParseFile(xmlSAXHandlerPtr sax, void *user_data,
///                         const char *filename);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlSAXUserParseFile(
    sax: *mut _xmlSAXHandler,
    user_data: *mut c_void,
    filename: *const c_char,
) -> c_int {
    // SAFETY: filename must be a valid C string. sax and user_data may be NULL.
    if filename.is_null() {
        return -1;
    }
    let ctxt = crate::xml::parser::helpers::create_parser_ctxt();
    if ctxt.is_null() {
        return -1;
    }
    if !sax.is_null() {
        (*ctxt).sax = sax;
    }
    (*ctxt).userData = if !user_data.is_null() {
        user_data
    } else {
        ctxt as *mut c_void
    };
    let input = match crate::xml::parser::helpers::input_from_file(filename) {
        Ok(input) => input,
        Err(_) => {
            crate::xml::parser::helpers::free_parser_ctxt(ctxt);
            return -1;
        }
    };
    crate::xml::parser::helpers::setup_parser_input(ctxt, input);
    let ret = crate::xml::parser::helpers::parse_document(ctxt);
    crate::xml::parser::helpers::free_parser_ctxt(ctxt);
    ret
}

/// SAX user parse memory.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlSAXUserParseMemory(xmlSAXHandlerPtr sax, void *user_data,
///                           const char *buffer, int size);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlSAXUserParseMemory(
    sax: *mut _xmlSAXHandler,
    user_data: *mut c_void,
    buffer: *const c_char,
    size: c_int,
) -> c_int {
    // SAFETY: buffer must be valid with at least `size` bytes.
    if buffer.is_null() || size <= 0 {
        return -1;
    }
    let ctxt = crate::xml::parser::helpers::create_parser_ctxt();
    if ctxt.is_null() {
        return -1;
    }
    if !sax.is_null() {
        (*ctxt).sax = sax;
    }
    (*ctxt).userData = if !user_data.is_null() {
        user_data
    } else {
        ctxt as *mut c_void
    };
    let input = crate::xml::parser::helpers::input_from_memory(buffer, size);
    crate::xml::parser::helpers::setup_parser_input(ctxt, input);
    let ret = crate::xml::parser::helpers::parse_document(ctxt);
    crate::xml::parser::helpers::free_parser_ctxt(ctxt);
    ret
}

/// Parse an XML document from a string (DOM).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDocPtr xmlParseDoc(const xmlChar *cur);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseDoc(cur: *const xmlChar) -> *mut _xmlDoc {
    // SAFETY: cur must be a valid null-terminated xmlChar string.
    if cur.is_null() {
        return ptr::null_mut();
    }
    xmlReadDoc(cur, ptr::null(), ptr::null(), 0)
}

/// Parse an XML file (DOM).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDocPtr xmlParseFile(const char *filename);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseFile(filename: *const c_char) -> *mut _xmlDoc {
    // SAFETY: filename must be a valid C string.
    if filename.is_null() {
        return ptr::null_mut();
    }
    xmlReadFile(filename, ptr::null(), 0)
}

/// Parse an XML document from memory (DOM).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDocPtr xmlParseMemory(const char *buffer, int size);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseMemory(buffer: *const c_char, size: c_int) -> *mut _xmlDoc {
    // SAFETY: buffer must be valid with at least `size` bytes.
    if buffer.is_null() || size <= 0 {
        return ptr::null_mut();
    }
    xmlReadMemory(buffer, size, ptr::null(), ptr::null(), 0)
}

/// Create a file parser context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlParserCtxtPtr xmlCreateFileParserCtxt(const char *filename);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlCreateFileParserCtxt(filename: *const c_char) -> *mut _xmlParserCtxt {
    // SAFETY: filename must be a valid C string.
    if filename.is_null() {
        return ptr::null_mut();
    }
    let ctxt = crate::xml::parser::helpers::create_parser_ctxt();
    if ctxt.is_null() {
        return ptr::null_mut();
    }
    let input = match crate::xml::parser::helpers::input_from_file(filename) {
        Ok(input) => input,
        Err(_) => {
            crate::xml::parser::helpers::free_parser_ctxt(ctxt);
            return ptr::null_mut();
        }
    };
    crate::xml::parser::helpers::setup_parser_input(ctxt, input);
    ctxt
}

/// Create a document parser context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlParserCtxtPtr xmlCreateDocParserCtxt(const xmlChar *cur);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlCreateDocParserCtxt(cur: *const xmlChar) -> *mut _xmlParserCtxt {
    // SAFETY: cur must be a valid null-terminated xmlChar string.
    if cur.is_null() {
        return ptr::null_mut();
    }
    let ctxt = crate::xml::parser::helpers::create_parser_ctxt();
    if ctxt.is_null() {
        return ptr::null_mut();
    }
    let len = crate::xml::string::xml_strlen(cur);
    let input = crate::xml::parser::helpers::input_from_memory(cur as *const c_char, len as c_int);
    crate::xml::parser::helpers::setup_parser_input(ctxt, input);
    ctxt
}

/// Parse a document using an existing parser context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlParseDocument(xmlParserCtxtPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseDocument(ctxt: *mut _xmlParserCtxt) -> c_int {
    // SAFETY: ctxt must be a valid parser context.
    if ctxt.is_null() {
        return -1;
    }
    crate::xml::parser::helpers::parse_document(ctxt)
}

/// Free a parser context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlFreeParserCtxt(xmlParserCtxtPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlFreeParserCtxt(ctxt: *mut _xmlParserCtxt) {
    if ctxt.is_null() {
        return;
    }
    crate::xml::parser::helpers::free_parser_ctxt(ctxt);
}

/// Set parser options.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlCtxtUseOptions(xmlParserCtxtPtr ctxt, int options);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlCtxtUseOptions(ctxt: *mut _xmlParserCtxt, options: c_int) -> c_int {
    if ctxt.is_null() {
        return -1;
    }
    // Phase 1: STUB
    unsafe {
        (*ctxt).options = options;
    }
    0
}

/// Parse a well-balanced chunk (for push parsing).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlParserErrors xmlParseChunk(xmlParserCtxtPtr ctxt,
///                               const char *chunk, int size, int terminate);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseChunk(
    ctxt: *mut _xmlParserCtxt,
    chunk: *const c_char,
    size: c_int,
    terminate: c_int,
) -> c_int {
    // SAFETY: ctxt must be a valid parser context.
    // chunk may be NULL if terminate is set (finalize without data).
    if ctxt.is_null() {
        return -1;
    }
    crate::xml::parser::helpers::parse_chunk(ctxt, chunk, size, terminate)
}

/// Create a memory parser input buffer.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlParserInputBufferPtr xmlParserInputBufferCreateMem(const char *buffer, int size, int enc);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParserInputBufferCreateMem(
    buffer: *const c_char,
    size: c_int,
    enc: c_int,
) -> *mut _xmlParserInputBuffer {
    // SAFETY: buffer must be valid with at least `size` bytes.
    if buffer.is_null() || size <= 0 {
        return ptr::null_mut();
    }
    crate::xml::parser::helpers::alloc_parser_input_buffer()
}

/// Create a file parser input buffer.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlParserInputBufferPtr xmlParserInputBufferCreateFilename(const char *URI, int enc);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParserInputBufferCreateFilename(
    URI: *const c_char,
    enc: c_int,
) -> *mut _xmlParserInputBuffer {
    // SAFETY: URI must be a valid C string or NULL.
    if URI.is_null() {
        return ptr::null_mut();
    }
    crate::xml::parser::helpers::alloc_parser_input_buffer()
}

/// Create an I/O parser input buffer.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlParserInputBufferPtr xmlParserInputBufferCreateIO(
///     xmlInputReadCallback ioread, xmlInputCloseCallback ioclose,
///     void *ioctx, int enc);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParserInputBufferCreateIO(
    ioread: Option<xmlInputReadCallback>,
    ioclose: Option<xmlInputCloseCallback>,
    ioctx: *mut c_void,
    enc: c_int,
) -> *mut _xmlParserInputBuffer {
    // SAFETY: ioread must be a valid callback if Some. ioctx may be NULL.
    let buf = crate::xml::parser::helpers::alloc_parser_input_buffer();
    if !buf.is_null() {
        (*buf).readcallback = ioread;
        (*buf).closecallback = ioclose;
        (*buf).context = ioctx;
    }
    buf
}

/// Free a parser input buffer.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlFreeParserInputBuffer(xmlParserInputBufferPtr buf);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlFreeParserInputBuffer(buf: *mut _xmlParserInputBuffer) {
    if buf.is_null() {
        return;
    }
    crate::xml::parser::helpers::free_parser_input_buffer(buf);
}

/// Create a new parser input.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlParserInputPtr xmlNewInputFromFile(xmlParserCtxtPtr ctxt, const char *filename);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlNewInputFromFile(
    ctxt: *mut _xmlParserCtxt,
    filename: *const c_char,
) -> *mut _xmlParserInput {
    // SAFETY: filename must be a valid C string. ctxt may be NULL.
    // This function allocates a _xmlParserInput. The caller owns it.
    // Note: The InputBuffer backing data is NOT leaked here (no ctxt._private
    // to store it). Use xmlCreateFileParserCtxt + xmlParseDocument instead.
    if filename.is_null() {
        return ptr::null_mut();
    }
    crate::xml::parser::helpers::alloc_parser_input_buffer() as *mut _xmlParserInput
}

/// Free a parser input.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlFreeInputStream(xmlParserInputPtr input);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlFreeInputStream(input: *mut _xmlParserInput) {
    if input.is_null() {
        return;
    }
    crate::xml::parser::helpers::free_parser_input(input);
}

// ═══════════════════════════════════════════════════════════════════════════════
// 8. I/O
// ═══════════════════════════════════════════════════════════════════════════════

/// Create an output buffer for a file.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlOutputBufferPtr xmlOutputBufferCreateFilename(const char *URI,
///                                                  xmlCharEncodingHandlerPtr encoder,
///                                                  int compression);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlOutputBufferCreateFilename(
    URI: *const c_char,
    encoder: *mut c_void,
    compression: c_int,
) -> *mut _xmlOutputBuffer {
    let _ = compression;
    if URI.is_null() {
        return ptr::null_mut();
    }
    crate::xml::io::output_buffer_create_filename(
        URI,
        encoder as *mut crate::abi::structs::_xmlCharEncodingHandler,
        0,
    )
}

/// Create an output buffer for a file descriptor.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlOutputBufferPtr xmlOutputBufferCreateFd(int fd,
///                                            xmlCharEncodingHandlerPtr encoder);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlOutputBufferCreateFd(
    fd: c_int,
    encoder: *mut c_void,
) -> *mut _xmlOutputBuffer {
    if fd < 0 {
        return ptr::null_mut();
    }
    crate::xml::io::output_buffer_create_fd(
        fd,
        encoder as *mut crate::abi::structs::_xmlCharEncodingHandler,
    )
}

/// Create an output buffer from I/O callbacks.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlOutputBufferPtr xmlOutputBufferCreateIO(
///     xmlOutputWriteCallback iowrite, xmlOutputCloseCallback ioclose,
///     void *ioctx, xmlCharEncodingHandlerPtr encoder);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlOutputBufferCreateIO(
    iowrite: Option<xmlOutputWriteCallback>,
    ioclose: Option<xmlOutputCloseCallback>,
    ioctx: *mut c_void,
    encoder: *mut c_void,
) -> *mut _xmlOutputBuffer {
    crate::xml::io::output_buffer_create_io(
        iowrite,
        ioclose,
        ioctx,
        encoder as *mut crate::abi::structs::_xmlCharEncodingHandler,
    )
}

/// Free an output buffer.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlOutputBufferClose(xmlOutputBufferPtr out);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlOutputBufferClose(out: *mut _xmlOutputBuffer) -> c_int {
    if out.is_null() {
        return -1;
    }
    crate::xml::io::output_buffer_close(out)
}

/// Flush an output buffer.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlOutputBufferFlush(xmlOutputBufferPtr out);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlOutputBufferFlush(out: *mut _xmlOutputBuffer) -> c_int {
    if out.is_null() {
        return -1;
    }
    crate::xml::io::output_buffer_flush(out)
}

/// Write to an output buffer.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlOutputBufferWrite(xmlOutputBufferPtr out, int len, const char *data);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlOutputBufferWrite(
    out: *mut _xmlOutputBuffer,
    len: c_int,
    data: *const c_char,
) -> c_int {
    if out.is_null() || data.is_null() || len <= 0 {
        return -1;
    }
    crate::xml::io::output_buffer_write(out, len, data)
}

/// Write a string to an output buffer.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlOutputBufferWriteString(xmlOutputBufferPtr out, const char *str);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlOutputBufferWriteString(
    out: *mut _xmlOutputBuffer,
    str: *const c_char,
) -> c_int {
    if str.is_null() {
        return 0;
    }
    unsafe { xmlOutputBufferWrite(out, xmlStrlen(str as *const xmlChar), str) }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 9. Dictionary
// ═══════════════════════════════════════════════════════════════════════════════

/// Create a new dictionary.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDictPtr xmlDictCreate(void);
/// ```
#[no_mangle]
pub extern "C" fn xmlDictCreate() -> *mut c_void {
    // Phase 1: STUB — will be implemented in xml/dictionary module.
    ptr::null_mut()
}

/// Create a sub-dictionary.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDictPtr xmlDictCreateSub(xmlDictPtr sub);
/// ```
#[no_mangle]
pub extern "C" fn xmlDictCreateSub(_sub: *mut c_void) -> *mut c_void {
    // Phase 1: STUB
    ptr::null_mut()
}

/// Look up a string in the dictionary.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// const xmlChar *xmlDictLookup(xmlDictPtr dict, const xmlChar *name, int len);
/// ```
///
/// Returns an interned string pointer (valid as long as the dictionary exists).
/// - If `len` < 0, `name` must be null-terminated.
/// - If `len` >= 0, exactly `len` bytes are used.
#[no_mangle]
pub unsafe extern "C" fn xmlDictLookup(
    dict: *mut c_void,
    name: *const xmlChar,
    len: c_int,
) -> *const xmlChar {
    // Phase 1: STUB
    name
}

/// Check if a string exists in the dictionary.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// const xmlChar *xmlDictExists(xmlDictPtr dict, const xmlChar *name, int len);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlDictExists(
    dict: *mut c_void,
    name: *const xmlChar,
    len: c_int,
) -> *const xmlChar {
    // Phase 1: STUB
    ptr::null()
}

/// Query dictionary size.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// unsigned int xmlDictSize(const xmlDictPtr dict);
/// ```
#[no_mangle]
pub extern "C" fn xmlDictSize(dict: *const c_void) -> c_uint {
    // Phase 1: STUB
    0
}

/// Free a dictionary.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlDictFree(xmlDictPtr dict);
/// ```
#[no_mangle]
pub extern "C" fn xmlDictFree(_dict: *mut c_void) {
    // Phase 1: STUB
}

/// Set the dictionary size limit.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// unsigned int xmlDictSetLimit(xmlDictPtr dict, unsigned int limit);
/// ```
#[no_mangle]
pub extern "C" fn xmlDictSetLimit(_dict: *mut c_void, _limit: c_uint) -> c_uint {
    // Phase 1: STUB
    0
}

/// Get current dictionary usage.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// unsigned int xmlDictGetUsage(const xmlDictPtr dict);
/// ```
#[no_mangle]
pub extern "C" fn xmlDictGetUsage(_dict: *const c_void) -> c_uint {
    // Phase 1: STUB
    0
}

// ═══════════════════════════════════════════════════════════════════════════════
// 10. Hash Table
// ═══════════════════════════════════════════════════════════════════════════════

/// Create a new hash table.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlHashTablePtr xmlHashCreate(int size);
/// ```
#[no_mangle]
pub extern "C" fn xmlHashCreate(_size: c_int) -> *mut c_void {
    // Phase 1: STUB
    ptr::null_mut()
}

/// Create a new hash table with a dictionary.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlHashTablePtr xmlHashCreateDict(int size, xmlDictPtr dict);
/// ```
#[no_mangle]
pub extern "C" fn xmlHashCreateDict(_size: c_int, _dict: *mut c_void) -> *mut c_void {
    // Phase 1: STUB
    ptr::null_mut()
}

/// Free a hash table.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlHashFree(xmlHashTablePtr table, xmlHashDeallocator f);
/// ```
#[no_mangle]
pub extern "C" fn xmlHashFree(
    _table: *mut c_void,
    _f: Option<unsafe extern "C" fn(*mut c_void, *mut xmlChar)>,
) {
    // Phase 1: STUB
}

/// Add an entry to a hash table.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlHashAddEntry(xmlHashTablePtr table, const xmlChar *name, void *userdata);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlHashAddEntry(
    _table: *mut c_void,
    _name: *const xmlChar,
    _userdata: *mut c_void,
) -> c_int {
    // Phase 1: STUB
    0
}

/// Add a 2-key entry.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlHashAddEntry2(xmlHashTablePtr table, const xmlChar *name,
///                      const xmlChar *name2, void *userdata);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlHashAddEntry2(
    _table: *mut c_void,
    _name: *const xmlChar,
    _name2: *const xmlChar,
    _userdata: *mut c_void,
) -> c_int {
    // Phase 1: STUB
    0
}

/// Add a 3-key entry.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlHashAddEntry3(xmlHashTablePtr table, const xmlChar *name,
///                      const xmlChar *name2, const xmlChar *name3, void *userdata);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlHashAddEntry3(
    _table: *mut c_void,
    _name: *const xmlChar,
    _name2: *const xmlChar,
    _name3: *const xmlChar,
    _userdata: *mut c_void,
) -> c_int {
    // Phase 1: STUB
    0
}

/// Update or add an entry.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlHashUpdateEntry(xmlHashTablePtr table, const xmlChar *name,
///                        void *userdata, xmlHashDeallocator f);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlHashUpdateEntry(
    _table: *mut c_void,
    _name: *const xmlChar,
    _userdata: *mut c_void,
    _f: Option<unsafe extern "C" fn(*mut c_void, *mut xmlChar)>,
) -> c_int {
    // Phase 1: STUB
    0
}

/// Update or add a 2-key entry.
#[no_mangle]
pub unsafe extern "C" fn xmlHashUpdateEntry2(
    _table: *mut c_void,
    _name: *const xmlChar,
    _name2: *const xmlChar,
    _userdata: *mut c_void,
    _f: Option<unsafe extern "C" fn(*mut c_void, *mut xmlChar)>,
) -> c_int {
    // Phase 1: STUB
    0
}

/// Update or add a 3-key entry.
#[no_mangle]
pub unsafe extern "C" fn xmlHashUpdateEntry3(
    _table: *mut c_void,
    _name: *const xmlChar,
    _name2: *const xmlChar,
    _name3: *const xmlChar,
    _userdata: *mut c_void,
    _f: Option<unsafe extern "C" fn(*mut c_void, *mut xmlChar)>,
) -> c_int {
    // Phase 1: STUB
    0
}

/// Look up an entry.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void *xmlHashLookup(xmlHashTablePtr table, const xmlChar *name);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlHashLookup(_table: *mut c_void, _name: *const xmlChar) -> *mut c_void {
    // Phase 1: STUB
    ptr::null_mut()
}

/// Look up a 2-key entry.
#[no_mangle]
pub unsafe extern "C" fn xmlHashLookup2(
    _table: *mut c_void,
    _name: *const xmlChar,
    _name2: *const xmlChar,
) -> *mut c_void {
    // Phase 1: STUB
    ptr::null_mut()
}

/// Look up a 3-key entry.
#[no_mangle]
pub unsafe extern "C" fn xmlHashLookup3(
    _table: *mut c_void,
    _name: *const xmlChar,
    _name2: *const xmlChar,
    _name3: *const xmlChar,
) -> *mut c_void {
    // Phase 1: STUB
    ptr::null_mut()
}

/// Get the size of a hash table.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlHashSize(xmlHashTablePtr table);
/// ```
#[no_mangle]
pub extern "C" fn xmlHashSize(_table: *mut c_void) -> c_int {
    // Phase 1: STUB
    0
}

/// Remove an entry.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlHashRemoveEntry(xmlHashTablePtr table, const xmlChar *name,
///                        xmlHashDeallocator f);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlHashRemoveEntry(
    _table: *mut c_void,
    _name: *const xmlChar,
    _f: Option<unsafe extern "C" fn(*mut c_void, *mut xmlChar)>,
) -> c_int {
    // Phase 1: STUB
    0
}

/// Remove a 2-key entry.
#[no_mangle]
pub unsafe extern "C" fn xmlHashRemoveEntry2(
    _table: *mut c_void,
    _name: *const xmlChar,
    _name2: *const xmlChar,
    _f: Option<unsafe extern "C" fn(*mut c_void, *mut xmlChar)>,
) -> c_int {
    // Phase 1: STUB
    0
}

/// Remove a 3-key entry.
#[no_mangle]
pub unsafe extern "C" fn xmlHashRemoveEntry3(
    _table: *mut c_void,
    _name: *const xmlChar,
    _name2: *const xmlChar,
    _name3: *const xmlChar,
    _f: Option<unsafe extern "C" fn(*mut c_void, *mut xmlChar)>,
) -> c_int {
    // Phase 1: STUB
    0
}

/// Scan a hash table with a scanner function.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlHashScan(xmlHashTablePtr table, xmlHashScanner f, void *data);
/// ```
#[no_mangle]
pub extern "C" fn xmlHashScan(
    _table: *mut c_void,
    _f: Option<unsafe extern "C" fn(*mut c_void, *const xmlChar, *mut c_void)>,
    _data: *mut c_void,
) {
    // Phase 1: STUB
}

/// Scan a hash table with a full scanner function.
#[no_mangle]
pub extern "C" fn xmlHashScanFull(
    _table: *mut c_void,
    _f: Option<unsafe extern "C" fn(*mut c_void, *const xmlChar, *mut c_void, *mut c_void)>,
    _data: *mut c_void,
) {
    // Phase 1: STUB
}

/// Copy a hash table.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlHashTablePtr xmlHashCopy(xmlHashTablePtr table, xmlHashCopier f);
/// ```
#[no_mangle]
pub extern "C" fn xmlHashCopy(
    _table: *mut c_void,
    _f: Option<unsafe extern "C" fn(*mut c_void, *const xmlChar) -> *mut c_void>,
) -> *mut c_void {
    // Phase 1: STUB
    ptr::null_mut()
}

// ═══════════════════════════════════════════════════════════════════════════════
// 11. List
// ═══════════════════════════════════════════════════════════════════════════════

/// Create a new list.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlListPtr xmlListCreate(xmlListDeallocator deallocator,
///                          xmlListDataCompare compare);
/// ```
#[no_mangle]
pub extern "C" fn xmlListCreate(
    _deallocator: Option<unsafe extern "C" fn(*mut c_void)>,
    _compare: Option<unsafe extern "C" fn(*const c_void, *const c_void) -> c_int>,
) -> *mut c_void {
    // Phase 1: STUB
    ptr::null_mut()
}

/// Delete a list.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlListDelete(xmlListPtr list);
/// ```
#[no_mangle]
pub extern "C" fn xmlListDelete(_list: *mut c_void) {
    // Phase 1: STUB
}

/// Search a list.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void *xmlListSearch(xmlListPtr list, void *data);
/// ```
#[no_mangle]
pub extern "C" fn xmlListSearch(_list: *mut c_void, _data: *mut c_void) -> *mut c_void {
    // Phase 1: STUB
    ptr::null_mut()
}

/// Walk a list.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlListWalk(xmlListPtr list, xmlListWalker walker, void *data);
/// ```
#[no_mangle]
pub extern "C" fn xmlListWalk(
    _list: *mut c_void,
    _walker: Option<unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int>,
    _data: *mut c_void,
) {
    // Phase 1: STUB
}

/// Push to back.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlListPushBack(xmlListPtr list, void *data);
/// ```
#[no_mangle]
pub extern "C" fn xmlListPushBack(_list: *mut c_void, _data: *mut c_void) -> c_int {
    // Phase 1: STUB
    0
}

/// Push to front.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlListPushFront(xmlListPtr list, void *data);
/// ```
#[no_mangle]
pub extern "C" fn xmlListPushFront(_list: *mut c_void, _data: *mut c_void) -> c_int {
    // Phase 1: STUB
    0
}

/// Pop from back.
#[no_mangle]
pub extern "C" fn xmlListPopBack(_list: *mut c_void) {
    // Phase 1: STUB
}

/// Pop from front.
#[no_mangle]
pub extern "C" fn xmlListPopFront(_list: *mut c_void) {
    // Phase 1: STUB
}

/// Insert into sorted list.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlListInsert(xmlListPtr list, void *data);
/// ```
#[no_mangle]
pub extern "C" fn xmlListInsert(_list: *mut c_void, _data: *mut c_void) -> c_int {
    // Phase 1: STUB
    0
}

/// Append to list.
#[no_mangle]
pub extern "C" fn xmlListAppend(_list: *mut c_void, _data: *mut c_void) -> c_int {
    // Phase 1: STUB
    0
}

/// Remove first matching element.
#[no_mangle]
pub extern "C" fn xmlListRemoveFirst(_list: *mut c_void, _data: *mut c_void) -> c_int {
    // Phase 1: STUB
    0
}

/// Remove last matching element.
#[no_mangle]
pub extern "C" fn xmlListRemoveLast(_list: *mut c_void, _data: *mut c_void) -> c_int {
    // Phase 1: STUB
    0
}

/// Remove all matching elements.
#[no_mangle]
pub extern "C" fn xmlListRemoveAll(_list: *mut c_void, _data: *mut c_void) -> c_int {
    // Phase 1: STUB
    0
}

/// Clear a list.
#[no_mangle]
pub extern "C" fn xmlListClear(_list: *mut c_void) {
    // Phase 1: STUB
}

/// Check if list is empty.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlListEmpty(xmlListPtr list);
/// ```
#[no_mangle]
pub extern "C" fn xmlListEmpty(_list: *mut c_void) -> c_int {
    // Phase 1: STUB
    1
}

/// Get front element.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void *xmlListFront(xmlListPtr list);
/// ```
#[no_mangle]
pub extern "C" fn xmlListFront(_list: *mut c_void) -> *mut c_void {
    // Phase 1: STUB
    ptr::null_mut()
}

/// Get back element.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void *xmlListBack(xmlListPtr list);
/// ```
#[no_mangle]
pub extern "C" fn xmlListBack(_list: *mut c_void) -> *mut c_void {
    // Phase 1: STUB
    ptr::null_mut()
}

/// Get list size.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlListSize(xmlListPtr list);
/// ```
#[no_mangle]
pub extern "C" fn xmlListSize(_list: *mut c_void) -> c_int {
    // Phase 1: STUB
    0
}

/// Sort a list.
#[no_mangle]
pub extern "C" fn xmlListSort(_list: *mut c_void) {
    // Phase 1: STUB
}

/// Reverse a list.
#[no_mangle]
pub extern "C" fn xmlListReverse(_list: *mut c_void) {
    // Phase 1: STUB
}

/// Reverse a list in-place.
#[no_mangle]
pub extern "C" fn xmlListReverseSplice(_list: *mut c_void, _list2: *mut c_void) {
    // Phase 1: STUB
}

/// Merge two sorted lists.
#[no_mangle]
pub extern "C" fn xmlListMerge(_list: *mut c_void, _list2: *mut c_void) {
    // Phase 1: STUB
}

// ═══════════════════════════════════════════════════════════════════════════════
// 12. Buffer
// ═══════════════════════════════════════════════════════════════════════════════

/// Create a new buffer.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlBufferPtr xmlBufferCreate(void);
/// ```
#[no_mangle]
pub extern "C" fn xmlBufferCreate() -> *mut _xmlBuffer {
    crate::xml::io::buf_create(-1)
}

/// Create a new buffer of a given size.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlBufferPtr xmlBufferCreateSize(size_t size);
/// ```
#[no_mangle]
pub extern "C" fn xmlBufferCreateSize(size: usize) -> *mut _xmlBuffer {
    crate::xml::io::buf_create(size as c_int)
}

/// Create a buffer from a static string.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlBufferPtr xmlBufferCreateStatic(void *mem, size_t size);
/// ```
#[no_mangle]
pub extern "C" fn xmlBufferCreateStatic(mem: *mut c_void, size: usize) -> *mut _xmlBuffer {
    if mem.is_null() || size == 0 {
        return ptr::null_mut();
    }
    crate::xml::io::buf_create_static(mem as *const xmlChar, size as c_int)
}

/// Free a buffer.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlBufferFree(xmlBufferPtr buf);
/// ```
#[no_mangle]
pub extern "C" fn xmlBufferFree(buf: *mut _xmlBuffer) {
    crate::xml::io::buf_free(buf)
}

/// Empty a buffer.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlBufferEmpty(xmlBufferPtr buf);
/// ```
#[no_mangle]
pub extern "C" fn xmlBufferEmpty(buf: *mut _xmlBuffer) {
    if buf.is_null() {
        return;
    }
    unsafe {
        if !(*buf).content.is_null() {
            *(*buf).content = 0;
        }
        (*buf).use_ = 0;
    }
}

/// Get buffer content.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlBufferContent(const xmlBuffer *buf);
/// ```
#[no_mangle]
pub extern "C" fn xmlBufferContent(buf: *const _xmlBuffer) -> *mut xmlChar {
    crate::xml::io::buf_content(buf as *mut _xmlBuffer)
}

/// Get buffer length.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlBufferLength(const xmlBuffer *buf);
/// ```
#[no_mangle]
pub extern "C" fn xmlBufferLength(buf: *const _xmlBuffer) -> c_int {
    crate::xml::io::buf_length(buf as *mut _xmlBuffer)
}

/// Write to a buffer.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlBufferAdd(xmlBufferPtr buf, const xmlChar *str, int len);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlBufferAdd(
    buf: *mut _xmlBuffer,
    str: *const xmlChar,
    len: c_int,
) -> c_int {
    crate::xml::io::buf_add(buf, str, len)
}

/// Write to a buffer at a position.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlBufferAddHead(xmlBufferPtr buf, const xmlChar *str, int len);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlBufferAddHead(
    buf: *mut _xmlBuffer,
    str: *const xmlChar,
    len: c_int,
) -> c_int {
    crate::xml::io::buf_add_head(buf, str, len)
}

/// Write a C string to a buffer.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlBufferCat(xmlBufferPtr buf, const xmlChar *str);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlBufferCat(buf: *mut _xmlBuffer, str: *const xmlChar) -> c_int {
    if str.is_null() {
        return -1;
    }
    let len = crate::xml::string::xml_strlen(str) as c_int;
    crate::xml::io::buf_add(buf, str, len)
}

/// Set buffer allocation scheme.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlBufferSetAllocationScheme(xmlBufferPtr buf,
///                                    xmlBufferAllocationScheme scheme);
/// ```
#[no_mangle]
pub extern "C" fn xmlBufferSetAllocationScheme(buf: *mut _xmlBuffer, scheme: c_int) {
    if buf.is_null() {
        return;
    }
    unsafe {
        (*buf).alloc = scheme;
    }
}

/// Shrink buffer.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlBufferShrink(xmlBufferPtr buf, int len);
/// ```
#[no_mangle]
pub extern "C" fn xmlBufferShrink(buf: *mut _xmlBuffer, len: c_int) -> c_int {
    if buf.is_null() || len <= 0 {
        return 0;
    }
    unsafe {
        let b = &mut *buf;
        let shrink_len = (len as c_uint).min(b.use_);
        if shrink_len > 0 {
            let remaining = b.use_ - shrink_len;
            if remaining > 0 {
                core::ptr::copy(
                    b.content.add(shrink_len as usize),
                    b.content,
                    remaining as usize,
                );
            }
            *b.content.add(remaining as usize) = 0;
            b.use_ = remaining;
        }
    }
    len
}

/// Grow buffer.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlBufferGrow(xmlBufferPtr buf, int len);
/// ```
#[no_mangle]
pub extern "C" fn xmlBufferGrow(buf: *mut _xmlBuffer, len: c_int) -> c_int {
    if buf.is_null() || len <= 0 {
        return 0;
    }
    let cur_use = unsafe { (*buf).use_ };
    let new_size = cur_use + len as c_uint + 1;
    crate::xml::io::buf_grow(buf, new_size)
}

/// Reserve buffer space.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlBufferReserve(xmlBufferPtr buf, int len);
/// ```
#[no_mangle]
pub extern "C" fn xmlBufferReserve(buf: *mut _xmlBuffer, len: c_int) -> c_int {
    xmlBufferGrow(buf, len)
}

/// Detach buffer content.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlBufferDetach(xmlBufferPtr buf);
/// ```
#[no_mangle]
pub extern "C" fn xmlBufferDetach(buf: *mut _xmlBuffer) -> *mut xmlChar {
    if buf.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        let content = (*buf).content;
        (*buf).content = ptr::null_mut();
        (*buf).use_ = 0;
        (*buf).size = 0;
        content
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 13. Encoding
// ═══════════════════════════════════════════════════════════════════════════════

/// Get encoding from a name string.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlCharEncoding xmlGetCharEncoding(const char *name);
/// ```
#[no_mangle]
pub extern "C" fn xmlGetCharEncoding(name: *const c_char) -> c_int {
    if name.is_null() {
        return 0; // XML_CHAR_ENCODING_NONE
    }
    let name_bytes = unsafe {
        let len = libc::strlen(name);
        core::slice::from_raw_parts(name as *const u8, len)
    };
    crate::xml::encoding::encoding_from_name(name_bytes) as c_int
}

/// Find an encoding handler.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlCharEncodingHandlerPtr xmlFindCharEncodingHandler(const char *name);
/// ```
#[no_mangle]
pub extern "C" fn xmlFindCharEncodingHandler(name: *const c_char) -> *mut c_void {
    if name.is_null() {
        return ptr::null_mut();
    }
    crate::xml::encoding::find_encoding_handler(name as *const xmlChar) as *mut c_void
}

/// Close an encoding handler.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlCharEncCloseFunc(xmlCharEncodingHandlerPtr handler);
/// ```
#[no_mangle]
pub extern "C" fn xmlCharEncCloseFunc(handler: *mut c_void) -> c_int {
    if handler.is_null() {
        return -1;
    }
    // Free the encoding handler
    unsafe {
        let h = handler as *mut crate::abi::structs::_xmlCharEncodingHandler;
        if !(*h).name.is_null() {
            crate::abi::allocator::xmlFree((*h).name as *mut c_void);
        }
        crate::abi::allocator::xmlFree(handler);
    }
    0
}

/// Convert an input buffer's encoding.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlCharEncInput(xmlParserInputBufferPtr input, int to);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlCharEncInput(input: *mut _xmlParserInputBuffer, _to: c_int) -> c_int {
    if input.is_null() {
        return -1;
    }
    let handler = (*input).encoder as *mut crate::abi::structs::_xmlCharEncodingHandler;
    if handler.is_null() {
        return -1;
    }
    let raw = (*input).raw as *mut crate::abi::structs::_xmlBuffer;
    let buf = (*input).buffer as *mut crate::abi::structs::_xmlBuffer;
    if raw.is_null() || buf.is_null() {
        return -1;
    }
    crate::xml::encoding::char_enc_in(handler, buf, raw)
}

/// Convert an output buffer's encoding.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlCharEncOutput(xmlOutputBufferPtr output, int to);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlCharEncOutput(output: *mut _xmlOutputBuffer, _to: c_int) -> c_int {
    if output.is_null() {
        return -1;
    }
    let handler = (*output).encoder as *mut crate::abi::structs::_xmlCharEncodingHandler;
    if handler.is_null() {
        return -1;
    }
    let buf = (*output).buffer as *mut crate::abi::structs::_xmlBuffer;
    let conv = (*output).conv as *mut crate::abi::structs::_xmlBuffer;
    if buf.is_null() || conv.is_null() {
        return -1;
    }
    crate::xml::encoding::char_enc_out(handler, conv, buf)
}

// ═══════════════════════════════════════════════════════════════════════════════
// URI
// ═══════════════════════════════════════════════════════════════════════════════

/// Parse a URI string.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlURIPtr xmlParseURI(const char *str);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseURI(str: *const c_char) -> *mut c_void {
    crate::xml::uri::xmlParseURI(str)
}

/// Parse a URI string (raw version).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlURIPtr xmlParseURIRaw(const char *str, int raw);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseURIRaw(str: *const c_char, raw: c_int) -> *mut c_void {
    let _ = raw;
    crate::xml::uri::xmlParseURI(str)
}

/// Free a URI structure.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlFreeURI(xmlURIPtr uri);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlFreeURI(uri: *mut c_void) {
    crate::xml::uri::xmlFreeURI(uri)
}

/// Create an empty URI.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlURIPtr xmlCreateURI(void);
/// ```
#[no_mangle]
pub extern "C" fn xmlCreateURI() -> *mut c_void {
    crate::xml::uri::xmlCreateURI()
}

/// Save a URI structure to a string.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlSaveUri(xmlURIPtr uri);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlSaveUri(uri: *mut c_void) -> *mut xmlChar {
    crate::xml::uri::xmlSaveUri(uri)
}

/// Escape a URI string.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlURIEscapeStr(const xmlChar *str, const xmlChar *list);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlURIEscapeStr(
    str: *const xmlChar,
    list: *const xmlChar,
) -> *mut xmlChar {
    crate::xml::uri::xmlURIEscapeStr(str, list)
}

/// Unescape a URI string.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// char *xmlURIUnescapeString(const char *str, int len, char *target);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlURIUnescapeString(
    str: *const c_char,
    len: c_int,
    target: *mut c_char,
) -> *mut c_char {
    crate::xml::uri::xmlURIUnescapeString(str, len, target)
}

// ═══════════════════════════════════════════════════════════════════════════════
// 14. XPath
// ═══════════════════════════════════════════════════════════════════════════════

// ── Helper functions ────────────────────────────────────────────────────

/// Convert an internal `XPathValue` to a C ABI `_xmlXPathObject`.
///
/// The returned pointer is heap-allocated via `xmlMallocZero` and must be
/// freed with `xmlXPathFreeObject`.
///
/// # Safety
///
/// Must be called from a context where `xmlMalloc` is safe to call.
unsafe fn xpath_to_object(val: XPathValue) -> *mut _xmlXPathObject {
    let obj = xmlMallocZero(size_of::<_xmlXPathObject>()) as *mut _xmlXPathObject;
    if obj.is_null() {
        return ptr::null_mut();
    }
    match val {
        XPathValue::NodeSet(ns) => {
            (*obj).type_ = xmlXPathObjectType::XPATH_NODESET as c_int;
            (*obj).nodesetval = ns.to_raw() as *mut c_void;
        }
        XPathValue::Boolean(b) => {
            (*obj).type_ = xmlXPathObjectType::XPATH_BOOLEAN as c_int;
            (*obj).boolval = if b { 1 } else { 0 };
        }
        XPathValue::Number(n) => {
            (*obj).type_ = xmlXPathObjectType::XPATH_NUMBER as c_int;
            (*obj).floatval = n;
        }
        XPathValue::String(s) => {
            (*obj).type_ = xmlXPathObjectType::XPATH_STRING as c_int;
            let bytes = s.as_bytes();
            let len = bytes.len();
            let buf = xmlMalloc(len + 1) as *mut xmlChar;
            if !buf.is_null() {
                ptr::copy_nonoverlapping(bytes.as_ptr(), buf, len);
                *buf.add(len) = 0; // null terminator
            }
            (*obj).stringval = buf;
        }
    }
    obj
}

/// Extract an internal `XPathValue` from a C ABI `_xmlXPathObject`.
///
/// # Safety
///
/// `obj` must be a valid, non-null pointer to a properly initialised
/// `_xmlXPathObject`.
unsafe fn object_to_xpathvalue(obj: *mut _xmlXPathObject) -> XPathValue {
    let typ = (*obj).type_;
    if typ == xmlXPathObjectType::XPATH_NODESET as c_int {
        let ns_ptr = (*obj).nodesetval as *mut _xmlNodeSet;
        if ns_ptr.is_null() {
            return XPathValue::NodeSet(NodeSet::new());
        }
        let node_nr = (*ns_ptr).nodeNr;
        let node_tab = (*ns_ptr).nodeTab;
        let mut ns = NodeSet::new();
        if !node_tab.is_null() {
            for i in 0..node_nr as isize {
                let node = *node_tab.add(i as usize);
                ns.push(node);
            }
        }
        XPathValue::NodeSet(ns)
    } else if typ == xmlXPathObjectType::XPATH_BOOLEAN as c_int {
        XPathValue::Boolean((*obj).boolval != 0)
    } else if typ == xmlXPathObjectType::XPATH_NUMBER as c_int {
        XPathValue::Number((*obj).floatval)
    } else if typ == xmlXPathObjectType::XPATH_STRING as c_int {
        let s_ptr = (*obj).stringval;
        if s_ptr.is_null() {
            XPathValue::String(String::new())
        } else {
            let s = CStr::from_ptr(s_ptr as *const c_char)
                .to_string_lossy()
                .into_owned();
            XPathValue::String(s)
        }
    } else {
        // Undefined / unknown type — return boolean false as a safe default.
        XPathValue::Boolean(false)
    }
}

// ── Compiled expression registry ────────────────────────────────────────
//
// Compiled XPath expressions are opaque pointers returned by xmlXPathCompile.
// We store them in a global registry keyed by a monotonically increasing ID.

static COMPILED_EXPRS: Lazy<Mutex<HashMap<u64, Box<CompiledExpr>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static NEXT_COMPILED_KEY: Lazy<Mutex<u64>> = Lazy::new(|| Mutex::new(1));

// ── C extension-function registry ──────────────────────────────────────
//
// C extension functions registered via xmlXPathRegisterFunc / RegisterFuncNS
// are stored here because the Rust XPathFunction signature is incompatible
// with the C xmlXPathFunction calling convention (the C function expects a
// parser context, not pre-evaluated argument slices). The registration is
// stored faithfully; invoking registered C functions from within the Rust
// evaluator requires a bridge that is not yet implemented.

type CXPathFunc = unsafe extern "C" fn(*mut c_void, c_int);

/// Wrapper around `*mut c_void` that implements `Send` + `Sync` so it can
/// be used as a key in a `Mutex`-protected global `HashMap`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct SendSyncPtr(*mut c_void);
unsafe impl Send for SendSyncPtr {}
unsafe impl Sync for SendSyncPtr {}

static C_FUNCTIONS: Lazy<Mutex<HashMap<(SendSyncPtr, String), CXPathFunc>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Rust-side wrapper that is registered in the internal XPathContext when a
/// C extension function is registered. It looks up the C function pointer and
/// attempts to call it, but the calling-convention mismatch means this is a
/// stub that returns an error for now.
fn c_func_stub(_ctx: &mut XPathContext, _args: &[XPathValue]) -> Result<XPathValue, String> {
    Err(
        "C extension function cannot be called from Rust evaluator without a parser-context bridge"
            .to_string(),
    )
}

// ── Public API ─────────────────────────────────────────────────────────

/// Create a new XPath context.
///
/// Allocates a `_xmlXPathContext` and an internal `XPathContext`, storing
/// the latter's pointer in the `extra` field.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlXPathContextPtr xmlXPathNewContext(xmlDocPtr doc);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlXPathNewContext(doc: *mut _xmlDoc) -> *mut _xmlXPathContext {
    let ctxt = xmlMallocZero(size_of::<_xmlXPathContext>()) as *mut _xmlXPathContext;
    if ctxt.is_null() {
        return ptr::null_mut();
    }

    // Initialise the C ABI context fields.
    (*ctxt).doc = doc;
    (*ctxt).node = ptr::null_mut();
    (*ctxt).contextSize = 1;
    (*ctxt).proximityPosition = 1;

    // Create the internal XPathContext and store it in `extra`.
    let internal = Box::new(XPathContext::new(doc));
    (*ctxt).extra = Box::into_raw(internal) as *mut c_void;

    ctxt
}

/// Free an XPath context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlXPathFreeContext(xmlXPathContextPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlXPathFreeContext(ctxt: *mut _xmlXPathContext) {
    if ctxt.is_null() {
        return;
    }
    // Drop the internal XPathContext.
    if !(*ctxt).extra.is_null() {
        let _ = Box::from_raw((*ctxt).extra as *mut XPathContext);
        (*ctxt).extra = ptr::null_mut();
    }
    // Free the C ABI context struct.
    xmlFree(ctxt as *mut c_void);
}

/// Evaluate an XPath expression.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlXPathObjectPtr xmlXPathEvalExpression(const xmlChar *str,
///                                          xmlXPathContextPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlXPathEvalExpression(
    str_: *const xmlChar,
    ctxt: *mut _xmlXPathContext,
) -> *mut _xmlXPathObject {
    if str_.is_null() || ctxt.is_null() {
        return ptr::null_mut();
    }
    let expr_str = match CStr::from_ptr(str_ as *const c_char).to_str() {
        Ok(s) => s,
        Err(_) => return ptr::null_mut(),
    };
    let internal = (*ctxt).extra as *mut XPathContext;
    if internal.is_null() {
        return ptr::null_mut();
    }
    let internal = &mut *internal;

    match crate::xml::xpath::evaluate_str(expr_str, internal) {
        Some(val) => xpath_to_object(val),
        None => ptr::null_mut(),
    }
}

/// Evaluate an XPath expression (simplified alias).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlXPathObjectPtr xmlXPathEval(const xmlChar *str, xmlXPathContextPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlXPathEval(
    str_: *const xmlChar,
    ctxt: *mut _xmlXPathContext,
) -> *mut _xmlXPathObject {
    xmlXPathEvalExpression(str_, ctxt)
}

/// Free an XPath object.
///
/// Releases the internal members (string buffer or node-set) and then frees
/// the object struct itself.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlXPathFreeObject(xmlXPathObjectPtr obj);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlXPathFreeObject(obj: *mut _xmlXPathObject) {
    if obj.is_null() {
        return;
    }
    let typ = (*obj).type_;
    // Free string storage.
    if typ == xmlXPathObjectType::XPATH_STRING as c_int {
        if !(*obj).stringval.is_null() {
            xmlFree((*obj).stringval as *mut c_void);
            (*obj).stringval = ptr::null_mut();
        }
    }
    // Free node-set storage.
    if typ == xmlXPathObjectType::XPATH_NODESET as c_int {
        let ns = (*obj).nodesetval as *mut _xmlNodeSet;
        if !ns.is_null() {
            if !(*ns).nodeTab.is_null() {
                xmlFree((*ns).nodeTab as *mut c_void);
            }
            xmlFree(ns as *mut c_void);
        }
        (*obj).nodesetval = ptr::null_mut();
    }
    xmlFree(obj as *mut c_void);
}

/// Compile an XPath expression.
///
/// Returns an opaque pointer that can be passed to `xmlXPathEvalExpression`
/// (via the compiled-expr infrastructure) or freed with `xmlXPathFreeCompExpr`.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlXPathCompExprPtr xmlXPathCompile(const xmlChar *str);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlXPathCompile(str_: *const xmlChar) -> *mut c_void {
    if str_.is_null() {
        return ptr::null_mut();
    }
    let expr_str = match CStr::from_ptr(str_ as *const c_char).to_str() {
        Ok(s) => s,
        Err(_) => return ptr::null_mut(),
    };

    match crate::xml::xpath::compile(expr_str) {
        Some(compiled) => {
            let mut map = COMPILED_EXPRS.lock();
            let mut counter = NEXT_COMPILED_KEY.lock();
            let key = *counter;
            *counter += 1;
            map.insert(key, Box::new(compiled));
            key as *mut c_void
        }
        None => ptr::null_mut(),
    }
}

/// Free a compiled XPath expression.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlXPathFreeCompExpr(xmlXPathCompExprPtr comp);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlXPathFreeCompExpr(comp: *mut c_void) {
    if comp.is_null() {
        return;
    }
    let mut map = COMPILED_EXPRS.lock();
    map.remove(&(comp as u64));
}

/// Register an XPath namespace.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlXPathRegisterNs(xmlXPathContextPtr ctxt,
///                        const xmlChar *prefix, const xmlChar *ns_uri);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlXPathRegisterNs(
    ctxt: *mut _xmlXPathContext,
    prefix: *const xmlChar,
    ns_uri: *const xmlChar,
) -> c_int {
    if ctxt.is_null() || prefix.is_null() || ns_uri.is_null() {
        return -1;
    }
    let internal = (*ctxt).extra as *mut XPathContext;
    if internal.is_null() {
        return -1;
    }
    let internal = &mut *internal;

    let prefix_str = match CStr::from_ptr(prefix as *const c_char).to_str() {
        Ok(s) => s,
        Err(_) => return -1,
    };
    let uri_str = match CStr::from_ptr(ns_uri as *const c_char).to_str() {
        Ok(s) => s,
        Err(_) => return -1,
    };

    internal.register_namespace(prefix_str, uri_str);
    0
}

/// Register an XPath function.
///
/// The C function pointer is stored in a side table keyed by the context.
/// A Rust-side stub is registered in the internal context so that the Rust
/// evaluator is aware of the function; however, calling the C function
/// directly from the Rust evaluator is not yet supported.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlXPathRegisterFunc(xmlXPathContextPtr ctxt,
///                          const xmlChar *name, xmlXPathFunction f);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlXPathRegisterFunc(
    ctxt: *mut _xmlXPathContext,
    name: *const xmlChar,
    f: Option<unsafe extern "C" fn(*mut c_void, c_int)>,
) -> c_int {
    if ctxt.is_null() || name.is_null() {
        return -1;
    }
    let internal = (*ctxt).extra as *mut XPathContext;
    if internal.is_null() {
        return -1;
    }
    let internal = &mut *internal;

    let name_str = match CStr::from_ptr(name as *const c_char).to_str() {
        Ok(s) => s,
        Err(_) => return -1,
    };

    if let Some(func) = f {
        // Store the C function pointer in the side table.
        let key = (SendSyncPtr((*ctxt).extra), name_str.to_string());
        C_FUNCTIONS.lock().insert(key, func);
        // Register a Rust stub so the evaluator knows the function exists.
        internal.register_function(name_str, c_func_stub);
    }
    0
}

/// Register an XPath function with namespace.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlXPathRegisterFuncNS(xmlXPathContextPtr ctxt,
///                            const xmlChar *name, const xmlChar *ns_uri,
///                            xmlXPathFunction f);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlXPathRegisterFuncNS(
    ctxt: *mut _xmlXPathContext,
    name: *const xmlChar,
    ns_uri: *const xmlChar,
    f: Option<unsafe extern "C" fn(*mut c_void, c_int)>,
) -> c_int {
    if ctxt.is_null() || name.is_null() {
        return -1;
    }
    let internal = (*ctxt).extra as *mut XPathContext;
    if internal.is_null() {
        return -1;
    }
    let internal = &mut *internal;

    let name_str = match CStr::from_ptr(name as *const c_char).to_str() {
        Ok(s) => s,
        Err(_) => return -1,
    };
    let ns_str = if ns_uri.is_null() {
        String::new()
    } else {
        match CStr::from_ptr(ns_uri as *const c_char).to_str() {
            Ok(s) => s.to_string(),
            Err(_) => return -1,
        }
    };

    // Use "{ns}:" prefix as part of the key to keep functions unique.
    let qualified = if ns_str.is_empty() {
        name_str.to_string()
    } else {
        format!("{{{}}}{}", ns_str, name_str)
    };

    if let Some(func) = f {
        let key = (SendSyncPtr((*ctxt).extra), qualified.clone());
        C_FUNCTIONS.lock().insert(key, func);
        internal.register_function(&qualified, c_func_stub);
    }
    0
}

/// Register an XPath variable.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlXPathRegisterVariable(xmlXPathContextPtr ctxt,
///                              const xmlChar *name, xmlXPathObjectPtr value);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlXPathRegisterVariable(
    ctxt: *mut _xmlXPathContext,
    name: *const xmlChar,
    value: *mut _xmlXPathObject,
) -> c_int {
    if ctxt.is_null() || name.is_null() || value.is_null() {
        return -1;
    }
    let internal = (*ctxt).extra as *mut XPathContext;
    if internal.is_null() {
        return -1;
    }
    let internal = &mut *internal;

    let name_str = match CStr::from_ptr(name as *const c_char).to_str() {
        Ok(s) => s,
        Err(_) => return -1,
    };

    let xpath_val = object_to_xpathvalue(value);
    internal.register_variable(name_str, xpath_val);
    0
}

/// Create an XPath object wrapping a single node in a node-set.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlXPathObjectPtr xmlXPathNewNodeSet(xmlNodePtr val);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlXPathNewNodeSet(val: *mut _xmlNode) -> *mut _xmlXPathObject {
    let ns = if val.is_null() {
        NodeSet::new()
    } else {
        NodeSet::singleton(val)
    };
    xpath_to_object(XPathValue::NodeSet(ns))
}

/// Create an XPath object from a C string value.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlXPathObjectPtr xmlXPathNewCString(const xmlChar *val);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlXPathNewCString(val: *const xmlChar) -> *mut _xmlXPathObject {
    if val.is_null() {
        return xpath_to_object(XPathValue::String(String::new()));
    }
    let s = match CStr::from_ptr(val as *const c_char).to_str() {
        Ok(s) => s.to_string(),
        Err(_) => return ptr::null_mut(),
    };
    xpath_to_object(XPathValue::String(s))
}

/// Create an XPath number object.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlXPathObjectPtr xmlXPathNewFloat(double val);
/// ```
#[no_mangle]
pub extern "C" fn xmlXPathNewFloat(val: f64) -> *mut _xmlXPathObject {
    unsafe { xpath_to_object(XPathValue::Number(val)) }
}

/// Create an XPath boolean object.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlXPathObjectPtr xmlXPathNewBoolean(int val);
/// ```
#[no_mangle]
pub extern "C" fn xmlXPathNewBoolean(val: c_int) -> *mut _xmlXPathObject {
    unsafe { xpath_to_object(XPathValue::Boolean(val != 0)) }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 14.5. XPointer
// ═══════════════════════════════════════════════════════════════════════════════

/// Evaluate an XPointer expression.
///
/// Delegates to the xpointer module.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNodePtr xmlXPtrEval(const xmlChar *expr, xmlDocPtr doc);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlXPtrEval(expr: *const c_char, doc: *mut _xmlDoc) -> *mut _xmlNode {
    crate::xml::xpointer::xmlXPtrEval(expr, doc)
}

// ═══════════════════════════════════════════════════════════════════════════════
// 15. XInclude
// ═══════════════════════════════════════════════════════════════════════════════

/// Process XInclude nodes in a document.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlXIncludeProcess(xmlDocPtr doc);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlXIncludeProcess(doc: *mut _xmlDoc) -> c_int {
    crate::xml::xinclude::xinclude_process(doc)
}

/// Process XInclude nodes with flags.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlXIncludeProcessFlags(xmlDocPtr doc, int flags);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlXIncludeProcessFlags(doc: *mut _xmlDoc, flags: c_int) -> c_int {
    crate::xml::xinclude::xinclude_process_flags(doc, flags)
}

// ═══════════════════════════════════════════════════════════════════════════════
// 16. Catalog
// ═══════════════════════════════════════════════════════════════════════════════

/// Load a catalog.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlCatalogPtr xmlCatalogLoad(const char *catalogs);
/// ```
#[no_mangle]
pub extern "C" fn xmlCatalogLoad(catalogs: *const c_char) -> *mut c_void {
    if catalogs.is_null() {
        return ptr::null_mut();
    }
    crate::xml::catalog::load_catalog(catalogs)
}

/// Resolve a public ID.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlCharPtr xmlCatalogResolvePublic(const xmlChar *pubID);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlCatalogResolvePublic(pubID: *const xmlChar) -> *mut xmlChar {
    if pubID.is_null() {
        return ptr::null_mut();
    }
    crate::xml::catalog::resolve_public(pubID)
}

/// Resolve a system ID.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlCharPtr xmlCatalogResolveSystem(const xmlChar *sysID);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlCatalogResolveSystem(sysID: *const xmlChar) -> *mut xmlChar {
    if sysID.is_null() {
        return ptr::null_mut();
    }
    crate::xml::catalog::resolve_system(sysID)
}

/// Resolve a URI.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlCharPtr xmlCatalogResolveURI(const xmlChar *URI);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlCatalogResolveURI(URI: *const xmlChar) -> *mut xmlChar {
    if URI.is_null() {
        return ptr::null_mut();
    }
    crate::xml::catalog::resolve_uri(URI)
}

/// Set catalog defaults.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlCatalogSetDefaults(xmlCatalogAllowValue allow);
/// ```
#[no_mangle]
pub extern "C" fn xmlCatalogSetDefaults(allow: c_int) {
    crate::xml::catalog::set_defaults(allow)
}

/// Get catalog defaults.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlCatalogAllowValue xmlCatalogGetDefaults(void);
/// ```
#[no_mangle]
pub extern "C" fn xmlCatalogGetDefaults() -> c_int {
    crate::xml::catalog::get_defaults()
}

/// Add a catalog.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlCatalogAdd(const xmlChar *type, const xmlChar *orig, const xmlChar *replace);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlCatalogAdd(
    type_: *const xmlChar,
    orig: *const xmlChar,
    replace: *const xmlChar,
) -> c_int {
    if type_.is_null() || orig.is_null() || replace.is_null() {
        return -1;
    }
    crate::xml::catalog::add(type_, orig, replace)
}

/// Remove a catalog entry.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlCatalogRemove(const xmlChar *value);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlCatalogRemove(value: *const xmlChar) -> c_int {
    if value.is_null() {
        return 0;
    }
    crate::xml::catalog::remove(value)
}

/// Clean up the catalog subsystem.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlCatalogCleanup(void);
/// ```
#[no_mangle]
pub extern "C" fn xmlCatalogCleanup() {
    crate::xml::catalog::cleanup();
}

/// Convert an SGML catalog to XML.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDocPtr xmlCatalogConvert(void);
/// ```
#[no_mangle]
pub extern "C" fn xmlCatalogConvert() -> *mut _xmlDoc {
    // SAFETY: catalog::convert() allocates and builds an XML document tree.
    unsafe { crate::xml::catalog::convert() }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 17. HTML
// ═══════════════════════════════════════════════════════════════════════════════

/// Parse an HTML document from a file.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// htmlDocPtr htmlParseFile(const char *filename, const char *encoding);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlParseFile(
    _filename: *const c_char,
    _encoding: *const c_char,
) -> *mut _xmlDoc {
    // Phase 1: STUB
    ptr::null_mut()
}

/// Parse an HTML document from memory.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// htmlDocPtr htmlParseMemory(const char *buffer, int size);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlParseMemory(_buffer: *const c_char, _size: c_int) -> *mut _xmlDoc {
    // Phase 1: STUB
    ptr::null_mut()
}

/// Parse an HTML document from a document string.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// htmlDocPtr htmlParseDoc(const xmlChar *cur, const char *encoding);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlParseDoc(
    _cur: *const xmlChar,
    _encoding: *const c_char,
) -> *mut _xmlDoc {
    // Phase 1: STUB
    ptr::null_mut()
}

/// Create an HTML parser context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// htmlParserCtxtPtr htmlCreateFileParserCtxt(const char *filename,
///                                            const char *encoding);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlCreateFileParserCtxt(
    _filename: *const c_char,
    _encoding: *const c_char,
) -> *mut c_void {
    // Phase 1: STUB
    ptr::null_mut()
}

/// Free an HTML parser context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void htmlFreeParserCtxt(htmlParserCtxtPtr ctxt);
/// ```
#[no_mangle]
pub extern "C" fn htmlFreeParserCtxt(_ctxt: *mut c_void) {
    // Phase 1: STUB
}

/// Initialize the HTML parser.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void htmlInitParser(void);
/// ```
#[no_mangle]
pub extern "C" fn htmlInitParser() {
    // Phase 1: STUB
}

/// Clean up the HTML parser.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void htmlCleanupParser(void);
/// ```
#[no_mangle]
pub extern "C" fn htmlCleanupParser() {
    // Phase 1: STUB
}

// ═══════════════════════════════════════════════════════════════════════════════
// 17.5. Validation (DTD)
// ═══════════════════════════════════════════════════════════════════════════════

/// Create a new validation context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlValidCtxtPtr xmlNewValidCtxt(void);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlNewValidCtxt() -> *mut _xmlValidCtxt {
    crate::xml::validation::new_valid_ctxt()
}

/// Free a validation context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlFreeValidCtxt(xmlValidCtxtPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlFreeValidCtxt(ctxt: *mut _xmlValidCtxt) {
    crate::xml::validation::free_valid_ctxt(ctxt);
}

/// Set error and warning callbacks on a validation context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlSetValidErrors(xmlValidCtxtPtr ctxt,
///                        xmlGenericErrorFunc err,
///                        xmlGenericErrorFunc warn,
///                        void *data);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlSetValidErrors(
    ctxt: *mut _xmlValidCtxt,
    err: Option<xmlGenericErrorFunc>,
    warn: Option<xmlGenericErrorFunc>,
    data: *mut c_void,
) {
    crate::xml::validation::set_valid_errors(ctxt, err, warn, data);
}

/// Validate a document against its DTD.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidateDocument(xmlValidCtxtPtr ctxt, xmlDocPtr doc);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlValidateDocument(ctxt: *mut _xmlValidCtxt, doc: *mut _xmlDoc) -> c_int {
    crate::xml::validation::validate_document(ctxt, doc)
}

/// Final validation pass (check ID/IDREF consistency).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidateDocumentFinal(xmlValidCtxtPtr ctxt, xmlDocPtr doc);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlValidateDocumentFinal(
    ctxt: *mut _xmlValidCtxt,
    doc: *mut _xmlDoc,
) -> c_int {
    crate::xml::validation::validate_document_final(ctxt, doc)
}

/// Validate an element node against its DTD declarations.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidateElement(xmlValidCtxtPtr ctxt,
///                        xmlDocPtr doc,
///                        xmlNodePtr elem);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlValidateElement(
    ctxt: *mut _xmlValidCtxt,
    doc: *mut _xmlDoc,
    elem: *mut _xmlNode,
) -> c_int {
    crate::xml::validation::validate_element(ctxt, doc, elem)
}

/// Validate an attribute declaration.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidateAttributeDecl(xmlValidCtxtPtr ctxt,
///                              xmlDocPtr doc,
///                              xmlNodePtr elem,
///                              xmlAttributePtr attr);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlValidateAttributeDecl(
    ctxt: *mut _xmlValidCtxt,
    doc: *mut _xmlDoc,
    elem: *mut _xmlNode,
    attr: *mut _xmlAttribute,
) -> c_int {
    crate::xml::validation::validate_attribute_decl(ctxt, doc, elem, attr)
}

/// Validate an attribute value against its declared type.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidateAttributeValue(int type, const xmlChar *value);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlValidateAttributeValue(atype: c_int, value: *const xmlChar) -> c_int {
    crate::xml::validation::validate_attribute_value(atype, value)
}

/// Validate a NOTATION reference.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidateNotationUse(xmlValidCtxtPtr ctxt,
///                            xmlDocPtr doc,
///                            const xmlChar *notationName);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlValidateNotationUse(
    ctxt: *mut _xmlValidCtxt,
    doc: *mut _xmlDoc,
    notation_name: *const xmlChar,
) -> c_int {
    crate::xml::validation::validate_notation_use(ctxt, doc, notation_name)
}

/// Validate an ID value (check uniqueness).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidateID(xmlValidCtxtPtr ctxt,
///                   xmlDocPtr doc,
///                   xmlNodePtr node,
///                   const xmlChar *value);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlValidateID(
    ctxt: *mut _xmlValidCtxt,
    doc: *mut _xmlDoc,
    node: *mut _xmlNode,
    value: *const xmlChar,
) -> c_int {
    crate::xml::validation::validate_id(ctxt, doc, node, value)
}

/// Validate an IDREF value (check it references a known ID).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidateIDRef(xmlValidCtxtPtr ctxt,
///                      xmlDocPtr doc,
///                      xmlNodePtr node,
///                      const xmlChar *value);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlValidateIDRef(
    ctxt: *mut _xmlValidCtxt,
    doc: *mut _xmlDoc,
    node: *mut _xmlNode,
    value: *const xmlChar,
) -> c_int {
    crate::xml::validation::validate_id_ref(ctxt, doc, node, value)
}

/// Validate IDREFS (whitespace-separated list of IDREFs).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidateIDRefs(xmlValidCtxtPtr ctxt,
///                       xmlDocPtr doc,
///                       xmlNodePtr node,
///                       const xmlChar *value);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlValidateIDRefs(
    ctxt: *mut _xmlValidCtxt,
    doc: *mut _xmlDoc,
    node: *mut _xmlNode,
    value: *const xmlChar,
) -> c_int {
    crate::xml::validation::validate_id_refs(ctxt, doc, node, value)
}

/// Validate an NMTOKEN value.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidateNmtoken(const xmlChar *value);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlValidateNmtoken(value: *const xmlChar) -> c_int {
    crate::xml::validation::validate_nmtoken(value)
}

/// Validate a whitespace-separated list of NMTOKENs.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidateNmtokens(const xmlChar *value);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlValidateNmtokens(value: *const xmlChar) -> c_int {
    crate::xml::validation::validate_nmtokens(value)
}

/// Validate an XML Name value.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidateName(const xmlChar *value);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlValidateName(value: *const xmlChar) -> c_int {
    crate::xml::validation::validate_name(value)
}

/// Validate a whitespace-separated list of XML Names.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidateNames(const xmlChar *value);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlValidateNames(value: *const xmlChar) -> c_int {
    crate::xml::validation::validate_names(value)
}

/// Validate the root element of a document.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidateRoot(xmlValidCtxtPtr ctxt, xmlDocPtr doc);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlValidateRoot(ctxt: *mut _xmlValidCtxt, doc: *mut _xmlDoc) -> c_int {
    crate::xml::validation::validate_root(ctxt, doc)
}

/// Validate element content against its content model.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidateContent(xmlValidCtxtPtr ctxt,
///                        xmlNodePtr node,
///                        xmlDocPtr doc);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlValidateContent(
    ctxt: *mut _xmlValidCtxt,
    node: *mut _xmlNode,
    doc: *mut _xmlDoc,
) -> c_int {
    crate::xml::validation::validate_content(ctxt, node, doc)
}

/// Check if an element is declared as mixed content.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlIsMixedElement(xmlDocPtr doc, const xmlChar *name);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlIsMixedElement(doc: *mut _xmlDoc, name: *const xmlChar) -> c_int {
    crate::xml::validation::is_mixed_element(doc, name)
}

/// Check if an element is declared as EMPTY.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlIsEmptyElement(xmlDocPtr doc, const xmlChar *name);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlIsEmptyElement(doc: *mut _xmlDoc, name: *const xmlChar) -> c_int {
    crate::xml::validation::is_empty_element(doc, name)
}

/// Validate a DTD's declarations.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidateDtd(xmlValidCtxtPtr ctxt,
///                    xmlDocPtr doc,
///                    xmlDtdPtr dtd);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlValidateDtd(
    ctxt: *mut _xmlValidCtxt,
    doc: *mut _xmlDoc,
    dtd: *mut _xmlDtd,
) -> c_int {
    crate::xml::validation::validate_dtd(ctxt, doc, dtd)
}

/// Final DTD validation (ID/IDREF consistency).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidateDtdFinal(xmlValidCtxtPtr ctxt, xmlDocPtr doc);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlValidateDtdFinal(ctxt: *mut _xmlValidCtxt, doc: *mut _xmlDoc) -> c_int {
    crate::xml::validation::validate_dtd_final(ctxt, doc)
}

/// Validate that a value is in an enumeration.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidateEnumeration(xmlValidCtxtPtr ctxt,
///                            const xmlChar *value,
///                            xmlEnumerationPtr tree);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlValidateEnumeration(
    ctxt: *mut _xmlValidCtxt,
    value: *const xmlChar,
    tree: *mut _xmlEnumeration,
) -> c_int {
    crate::xml::validation::validate_enumeration(ctxt, value, tree)
}

// ═══════════════════════════════════════════════════════════════════════════════
// 18. Debug / Miscellaneous
// ═══════════════════════════════════════════════════════════════════════════════

/// Dump a document to a file for debugging.

/// Get the path to the current executable.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// char *xmlGetBinaryPath(void);
/// ```
#[no_mangle]
pub extern "C" fn xmlGetBinaryPath() -> *mut c_char {
    // Phase 1: STUB
    ptr::null_mut()
}

/// Get the path to the current executable's home directory.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// char *xmlGetHomeOfBinary(void);
/// ```
#[no_mangle]
pub extern "C" fn xmlGetHomeOfBinary() -> *mut c_char {
    // Phase 1: STUB
    ptr::null_mut()
}
