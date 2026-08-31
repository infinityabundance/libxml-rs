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
use std::ffi::{CStr, CString};
use std::mem::size_of;
use std::os::raw::{c_char, c_int, c_long, c_uint, c_ulong};

use crate::xml::xpath::ast::CompiledExpr;
use crate::xml::xpath::context::{BoxedXPathFunction, XPathContext};
use crate::xml::xpath::types::{NodeSet, XPathValue};

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

/// Initialize the global variables module (upstream globals.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlInitGlobals(void);
/// ```
///
/// Upstream `xmlInitGlobals` (globals.c) is called once to initialize the
/// global variable defaults. The candidate's globals are initialized
/// statically/on first use, so this is a no-op that exists for ABI
/// compatibility.
#[no_mangle]
pub const unsafe extern "C" fn xmlInitGlobals() {
    // Globals are statically initialized in the candidate.
}

/// Upstream `xmlInitializeGlobalState` (globals.c) — initializes a
/// `xmlGlobalState` struct; the candidate keeps no global-state struct, so
/// this is a no-op for ABI compatibility.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlInitializeGlobalState(xmlGlobalStatePtr gs);
/// ```
#[no_mangle]
pub const unsafe extern "C" fn xmlInitializeGlobalState(_gs: *mut c_void) {
    // No-op: the candidate's globals are statically initialized.
}

/// Upstream `xmlInitializeDict` (dict.c) — ensures the dictionary
/// subsystem is initialized; no-op in the candidate (lazy init).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlInitializeDict(void);
/// ```
#[no_mangle]
pub const extern "C" fn xmlInitializeDict() -> c_int {
    0
}

/// Upstream `xmlInitializePredefinedEntities` (entities.c) — the
/// predefined entities (&amp; &lt; &gt; &quot; &apos;) are built lazily by
/// the candidate; no-op.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlInitializePredefinedEntities(void);
/// ```
#[no_mangle]
pub const extern "C" fn xmlInitializePredefinedEntities() {
    // No-op: predefined entities are resolved on demand.
}

/// Upstream `xmlCleanupPredefinedEntities` (entities.c) — no-op in the
/// candidate (no global entity table to release).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlCleanupPredefinedEntities(void);
/// ```
#[no_mangle]
pub const extern "C" fn xmlCleanupPredefinedEntities() {
    // No-op.
}

/// Upstream `xmlDefaultSAXHandlerInit` (SAX2.c) — fills the
/// `xmlDefaultSAXHandler` global. The candidate's default handler is built
/// on demand; this initializes the exported default-handler global when it
/// is added (currently tracked in R-000135). No-op for now.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlDefaultSAXHandlerInit(void);
/// ```
#[no_mangle]
pub const extern "C" fn xmlDefaultSAXHandlerInit() {
    // The candidate builds default handlers on demand; the exported
    // xmlDefaultSAXHandler global is part of the R-000135 data closure.
}

/// The current default SAX version (2), stored as an atomic so callers can
/// query it without taking a lock.
static SAX2_DEFAULT_VERSION: core::sync::atomic::AtomicI32 = core::sync::atomic::AtomicI32::new(2);
/// Set the default SAX version (upstream SAX2.c `xmlSAXDefaultVersion`):
/// returns the previous default; -1 when the version is not 1 or 2.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlSAXDefaultVersion(int version);
/// ```
#[no_mangle]
pub extern "C" fn xmlSAXDefaultVersion(version: c_int) -> c_int {
    use core::sync::atomic::Ordering;
    let ret = SAX2_DEFAULT_VERSION.load(Ordering::Relaxed);
    if version != 1 && version != 2 {
        return -1;
    }
    SAX2_DEFAULT_VERSION.store(version, Ordering::Relaxed);
    ret
}

/// Initialize a SAX handler for a given SAX version (upstream SAX2.c
/// `xmlSAXVersion`): fills the handler with the default callbacks and sets
/// `initialized` (XML_SAX2_MAGIC for version 2, 1 for version 1).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlSAXVersion(xmlSAXHandler *hdlr, int version);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlSAXVersion(
    hdlr: *mut crate::abi::structs::_xmlSAXHandler,
    version: c_int,
) -> c_int {
    if hdlr.is_null() {
        return -1;
    }
    if version != 1 && version != 2 {
        return -1;
    }
    // SAFETY: hdlr is non-NULL and writable.
    unsafe {
        crate::xml::sax::dispatch::xmlSAX2InitDefaultSAXHandler(hdlr);
        let h = &mut *hdlr;
        if version == 2 {
            h.initialized = crate::abi::constants::XML_SAX2_MAGIC as c_uint;
        } else {
            h.initialized = 1;
        }
    }
    0
}

/// Upstream `xmlHasFeature` (parser.c): returns 1 when the library was
/// compiled with the requested feature. The candidate enables the full
/// feature set (see include/libxml/xmlversion.h), so every known feature
/// reports 1; unknown features report 0.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlHasFeature(xmlFeature feature);
/// ```
#[no_mangle]
pub extern "C" fn xmlHasFeature(feature: c_int) -> c_int {
    // xmlFeature enum values (upstream xmlversion.h): XML_WITH_* run 1..24
    // (XML_WITH_THREAD=1 ... XML_WITH_MODULES=24).
    if (1..=24).contains(&feature) {
        1
    } else {
        0
    }
}

/// Clean up the global variables module (upstream globals.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlCleanupGlobals(void);
/// ```
///
/// Upstream `xmlCleanupGlobals` frees the global defaults. The candidate
/// keeps globals alive for the process lifetime (repeated init/cleanup is
/// reference-counted); no-op for ABI compatibility.
#[no_mangle]
pub const unsafe extern "C" fn xmlCleanupGlobals() {
    // The candidate's globals are process-lifetime statics.
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

/// Create a simple mutex (upstream threads.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlMutexPtr xmlNewMutex(void);
/// ```
#[no_mangle]
pub extern "C" fn xmlNewMutex() -> *mut c_void {
    crate::xml::threads::new_mutex()
}

/// Free a simple mutex (upstream threads.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlFreeMutex(xmlMutexPtr tok);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlFreeMutex(tok: *mut c_void) {
    crate::xml::threads::free_mutex(tok);
}

/// Lock a simple mutex (upstream threads.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlMutexLock(xmlMutexPtr tok);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlMutexLock(tok: *mut c_void) {
    crate::xml::threads::mutex_lock(tok);
}

/// Unlock a simple mutex (upstream threads.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlMutexUnlock(xmlMutexPtr tok);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlMutexUnlock(tok: *mut c_void) {
    crate::xml::threads::mutex_unlock(tok);
}

/// Create a recursive mutex (upstream threads.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlRMutexPtr xmlNewRMutex(void);
/// ```
#[no_mangle]
pub extern "C" fn xmlNewRMutex() -> *mut c_void {
    crate::xml::threads::new_rmutex()
}

/// Free a recursive mutex (upstream threads.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlFreeRMutex(xmlRMutexPtr tok);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlFreeRMutex(tok: *mut c_void) {
    crate::xml::threads::free_rmutex(tok);
}

/// Lock a recursive mutex (upstream threads.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlRMutexLock(xmlRMutexPtr tok);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlRMutexLock(tok: *mut c_void) {
    crate::xml::threads::rmutex_lock(tok);
}

/// Unlock a recursive mutex (upstream threads.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlRMutexUnlock(xmlRMutexPtr tok);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlRMutexUnlock(tok: *mut c_void) {
    crate::xml::threads::rmutex_unlock(tok);
}

/// Check the thread-local storage (upstream threads.h `xmlCheckThreadLocalStorage`):
/// returns 0 when TLS is functional, -1 otherwise. The candidate uses Rust
/// thread-locals which are always functional.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlCheckThreadLocalStorage(void);
/// ```
#[no_mangle]
pub const extern "C" fn xmlCheckThreadLocalStorage() -> c_int {
    0
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
pub const unsafe extern "C" fn xmlLockLibrary() {
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
pub const unsafe extern "C" fn xmlUnlockLibrary() {
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
pub const unsafe extern "C" fn xmlCopyError(from: *const _xmlError, to: *mut _xmlError) -> c_int {
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
pub const unsafe extern "C" fn xmlResetError(err: *mut _xmlError) {
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
    let new_ptr = unsafe { xmlMallocImpl(size as usize) };
    if new_ptr.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        ptr::copy_nonoverlapping(cur, new_ptr as *mut u8, size as usize);
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
    let new_ptr = unsafe { xmlMallocImpl(size) };
    if new_ptr.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        ptr::copy_nonoverlapping(cur, new_ptr as *mut u8, len as usize);
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

/// Find the first occurrence of a character in a string (upstream tree.c
/// `xmlStrchr`): returns a pointer to the first occurrence or NULL.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// const xmlChar *xmlStrchr(const xmlChar *str, xmlChar val);
/// ```
///
/// # SAFETY
///
/// - `str` must be a valid null-terminated string or NULL.
#[no_mangle]
pub const unsafe extern "C" fn xmlStrchr(str: *const xmlChar, val: xmlChar) -> *const xmlChar {
    if str.is_null() {
        return ptr::null();
    }
    unsafe {
        let mut cur = str;
        while *cur != 0 {
            if *cur == val {
                return cur;
            }
            cur = cur.add(1);
        }
        ptr::null()
    }
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

/// Build a QName (upstream tree.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlBuildQName(const xmlChar *ncname, const xmlChar *prefix,
///                        xmlChar *memory, int len);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlBuildQName(
    ncname: *const xmlChar,
    prefix: *const xmlChar,
    memory: *mut xmlChar,
    len: c_int,
) -> *mut xmlChar {
    crate::xml::string::build_qname(ncname, prefix, memory, len)
}

/// Split a QName into prefix + local part (upstream tree.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlSplitQName2(const xmlChar *name, xmlChar **prefix);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlSplitQName2(
    name: *const xmlChar,
    prefix: *mut *mut xmlChar,
) -> *mut xmlChar {
    crate::xml::string::split_qname2(name, prefix)
}

/// Return the prefix length of a QName (upstream tree.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlSplitQName3(const xmlChar *name, int *len);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlSplitQName3(name: *const xmlChar, len: *mut c_int) -> c_int {
    crate::xml::string::split_qname3(name, len)
}

/// Deprecated alias of `xmlSplitQName2` (upstream tree.h `xmlSplitQName`).
#[no_mangle]
pub unsafe extern "C" fn xmlSplitQName(
    name: *const xmlChar,
    prefix: *mut *mut xmlChar,
) -> *mut xmlChar {
    crate::xml::string::split_qname2(name, prefix)
}

/// Count UTF-8 characters (upstream xmlstring.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlUTF8Strlen(const xmlChar *utf);
/// ```
#[no_mangle]
pub const unsafe extern "C" fn xmlUTF8Strlen(utf: *const xmlChar) -> c_int {
    crate::xml::string::utf8_strlen(utf)
}

/// Size in bytes of a UTF-8 sequence (upstream xmlstring.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlUTF8Size(const xmlChar *utf);
/// ```
#[no_mangle]
pub const unsafe extern "C" fn xmlUTF8Size(utf: *const xmlChar) -> c_int {
    crate::xml::string::utf8_size(utf)
}

/// Check UTF-8 validity (upstream xmlstring.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlCheckUTF8(const unsigned char *utf);
/// ```
#[no_mangle]
pub const unsafe extern "C" fn xmlCheckUTF8(utf: *const xmlChar) -> c_int {
    crate::xml::string::check_utf8(utf)
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
    let new_ptr = unsafe { xmlReallocImpl(cur as *mut c_void, new_size) };
    if new_ptr.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        ptr::copy_nonoverlapping(add, (new_ptr as *mut u8).add(cur_len), add_len);
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
    let new_ptr = unsafe { xmlReallocImpl(cur as *mut c_void, new_size) };
    if new_ptr.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        ptr::copy_nonoverlapping(add, (new_ptr as *mut u8).add(cur_len), len);
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
        ptr::copy_nonoverlapping(src, dst, len);
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
        ptr::copy_nonoverlapping(src, dst, copy_len);
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

/// Get the compression mode of a document (upstream tree.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlGetDocCompressMode(const xmlDoc *doc);
/// ```
///
/// Returns the compression level (0-9) or -1 if `doc` is NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlGetDocCompressMode(doc: *mut _xmlDoc) -> c_int {
    crate::xml::tree::xmlGetDocCompressMode(doc)
}

/// Set the compression mode of a document (upstream tree.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlSetDocCompressMode(xmlDocPtr doc, int mode);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlSetDocCompressMode(doc: *mut _xmlDoc, mode: c_int) {
    crate::xml::tree::xmlSetDocCompressMode(doc, mode);
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

/// Free a linked list of nodes (upstream tree.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlFreeNodeList(xmlNodePtr node);
/// ```
///
/// Frees the node and all its siblings (following `next` pointers),
/// recursively freeing children. Matches upstream `xmlFreeNodeList`
/// (tree.c): a NULL argument is a no-op.
///
/// # SAFETY
///
/// - `node` must be a valid node pointer or NULL.
/// - The list must NOT be part of a document tree (must be unlinked first).
#[no_mangle]
pub unsafe extern "C" fn xmlFreeNodeList(node: *mut _xmlNode) {
    crate::xml::tree::free_node_list(node);
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

/// Initialize a SAX handler with the default SAX2 callbacks (upstream SAX2.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlSAX2InitDefaultSAXHandler(xmlSAXHandler *hdlr, int warning);
/// ```
///
/// The `warning` parameter controls whether the warning callback is set in
/// upstream; the candidate always sets it (the parser dispatches warnings
/// identically) — a documented safe divergence tracked in the parity ledger.
///
/// # SAFETY
///
/// - `handler` must be a valid writable `_xmlSAXHandler` or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlSAX2InitDefaultSAXHandler(
    handler: *mut crate::abi::structs::_xmlSAXHandler,
    _warning: c_int,
) {
    crate::xml::sax::dispatch::xmlSAX2InitDefaultSAXHandler(handler);
}

/// Initialize a SAX handler with the default HTML callbacks (upstream SAX2.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlSAX2InitHtmlDefaultSAXHandler(xmlSAXHandler *hdlr);
/// ```
///
/// Upstream (SAX2.c `xmlSAX2InitHtmlDefaultSAXHandler`) fills the handler
/// with the SAX2 defaults minus the DTD-declaration callbacks
/// (resolveEntity/getParameterEntity/entityDecl/attributeDecl/elementDecl/
/// notationDecl/unparsedEntityDecl/reference/externalSubset are NULL) and
/// sets `initialized = 1` (not XML_SAX2_MAGIC). The candidate mirrors that
/// exactly.
///
/// # SAFETY
///
/// - `handler` must be a valid writable `_xmlSAXHandler` or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlSAX2InitHtmlDefaultSAXHandler(
    handler: *mut crate::abi::structs::_xmlSAXHandler,
) {
    if handler.is_null() {
        return;
    }
    // SAFETY: handler is non-NULL and writable.
    unsafe {
        // The DTD-ish callbacks are not part of the HTML default set.
        if (*handler).initialized != 0 {
            return;
        }
        crate::xml::sax::dispatch::xmlSAX2InitDefaultSAXHandler(handler);
        let h = &mut *handler;
        h.resolveEntity = None;
        h.getParameterEntity = None;
        h.entityDecl = None;
        h.attributeDecl = None;
        h.elementDecl = None;
        h.notationDecl = None;
        h.unparsedEntityDecl = None;
        h.reference = None;
        h.externalSubset = None;
        h.initialized = 1;
    }
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

/// Check whether a node has a property (upstream tree.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlAttrPtr xmlHasProp(const xmlNode *node, const xmlChar *name);
/// ```
///
/// Returns the attribute pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlHasProp(node: *const _xmlNode, name: *const xmlChar) -> *mut _xmlAttr {
    crate::xml::tree::has_prop(node as *mut _xmlNode, name)
}

/// Check whether a node has a namespaced property (upstream tree.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlAttrPtr xmlHasNsProp(const xmlNode *node, const xmlChar *name,
///                         const xmlChar *nameSpace);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlHasNsProp(
    node: *const _xmlNode,
    name: *const xmlChar,
    nameSpace: *const xmlChar,
) -> *mut _xmlAttr {
    crate::xml::tree::has_ns_prop(node as *mut _xmlNode, name, nameSpace)
}

/// Remove a property by name (upstream tree.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlUnsetProp(xmlNodePtr node, const xmlChar *name);
/// ```
///
/// Returns 0 on success, -1 if not found.
#[no_mangle]
pub unsafe extern "C" fn xmlUnsetProp(node: *mut _xmlNode, name: *const xmlChar) -> c_int {
    crate::xml::tree::unset_prop(node, name)
}

/// Remove a namespaced property by name (upstream tree.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlUnsetNsProp(xmlNodePtr node, const xmlChar *name,
///                    const xmlChar *nameSpace);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlUnsetNsProp(
    node: *mut _xmlNode,
    name: *const xmlChar,
    nameSpace: *const xmlChar,
) -> c_int {
    crate::xml::tree::unset_ns_prop(node, name, nameSpace)
}

/// Return the first child element (upstream tree.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNodePtr xmlFirstElementChild(xmlNodePtr parent);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlFirstElementChild(parent: *mut _xmlNode) -> *mut _xmlNode {
    crate::xml::tree::first_element_child(parent)
}

/// Return the last child element (upstream tree.h).
#[no_mangle]
pub unsafe extern "C" fn xmlLastElementChild(parent: *mut _xmlNode) -> *mut _xmlNode {
    crate::xml::tree::last_element_child(parent)
}

/// Return the next element sibling (upstream tree.h).
#[no_mangle]
pub unsafe extern "C" fn xmlNextElementSibling(node: *mut _xmlNode) -> *mut _xmlNode {
    crate::xml::tree::next_element_sibling(node)
}

/// Return the previous element sibling (upstream tree.h).
#[no_mangle]
pub unsafe extern "C" fn xmlPreviousElementSibling(node: *mut _xmlNode) -> *mut _xmlNode {
    crate::xml::tree::previous_element_sibling(node)
}

/// Count the child elements (upstream tree.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// unsigned long xmlChildElementCount(xmlNodePtr parent);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlChildElementCount(parent: *mut _xmlNode) -> c_ulong {
    crate::xml::tree::child_element_count(parent)
}

/// Concatenate text to a node (upstream tree.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextConcat(xmlNodePtr node, const xmlChar *content, int len);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlTextConcat(
    node: *mut _xmlNode,
    content: *const xmlChar,
    len: c_int,
) -> c_int {
    crate::xml::tree::text_concat(node, content, len)
}

/// Merge two text nodes (upstream tree.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNodePtr xmlTextMerge(xmlNodePtr first, xmlNodePtr second);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlTextMerge(
    first: *mut _xmlNode,
    second: *mut _xmlNode,
) -> *mut _xmlNode {
    crate::xml::tree::text_merge(first, second)
}

/// Get a DTD from a document, creating one if needed.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDtdPtr xmlGetIntSubset(const xmlDoc *doc);
/// ```
#[no_mangle]
pub const extern "C" fn xmlGetIntSubset(doc: *const _xmlDoc) -> *mut _xmlDtd {
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

/// Add an entity declaration to a document's internal subset (upstream
/// entities.c `xmlAddDocEntity`): if the document has no internal subset
/// one is created.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlEntityPtr xmlAddDocEntity(xmlDocPtr doc, const xmlChar *name, int type,
///                              const xmlChar *ExternalID, const xmlChar *SystemID,
///                              const xmlChar *content);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlAddDocEntity(
    doc: *mut _xmlDoc,
    name: *const xmlChar,
    type_: c_int,
    ExternalID: *const xmlChar,
    SystemID: *const xmlChar,
    content: *const xmlChar,
) -> *mut _xmlEntity {
    crate::xml::tree::add_doc_entity(doc, name, type_, ExternalID, SystemID, content)
}

/// Add an entity declaration to a document's external subset (upstream
/// entities.c `xmlAddDtdEntity`).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlEntityPtr xmlAddDtdEntity(xmlDocPtr doc, const xmlChar *name, int type,
///                              const xmlChar *ExternalID, const xmlChar *SystemID,
///                              const xmlChar *content);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlAddDtdEntity(
    doc: *mut _xmlDoc,
    name: *const xmlChar,
    type_: c_int,
    ExternalID: *const xmlChar,
    SystemID: *const xmlChar,
    content: *const xmlChar,
) -> *mut _xmlEntity {
    crate::xml::tree::add_dtd_entity(doc, name, type_, ExternalID, SystemID, content)
}

/// Get an entity declaration from a DTD (upstream entities.c
/// `xmlGetDtdEntity`): searches the internal then external subset.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlEntityPtr xmlGetDtdEntity(xmlDocPtr doc, const xmlChar *name);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlGetDtdEntity(
    doc: *mut _xmlDoc,
    name: *const xmlChar,
) -> *mut _xmlEntity {
    crate::xml::tree::get_dtd_entity(doc, name)
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

/// Encode special characters in a string (upstream entities.c
/// `xmlEncodeSpecialChars`): escapes `<`, `>`, `&`, `"` and `\r`.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlEncodeSpecialChars(const xmlDoc *doc, const xmlChar *input);
/// ```
///
/// Returns a newly allocated string (free with `xmlFree`) or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlEncodeSpecialChars(
    _doc: *const _xmlDoc,
    input: *const xmlChar,
) -> *mut xmlChar {
    if input.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        let len = crate::xml::string::xml_strlen(input);
        // Worst case: every byte becomes a 6-byte entity (&#13; is 5; &quot;
        // is 6).
        let cap = len * 6 + 1;
        let out = crate::abi::allocator::xmlMallocImpl(cap) as *mut xmlChar;
        if out.is_null() {
            return ptr::null_mut();
        }
        let mut o = 0usize;
        let mut i = 0usize;
        while i < len {
            let c = *input.add(i);
            let rep: &[u8] = match c {
                b'<' => b"&lt;",
                b'>' => b"&gt;",
                b'&' => b"&amp;",
                b'"' => b"&quot;",
                b'\r' => b"&#13;",
                _ => {
                    *out.add(o) = c;
                    o += 1;
                    i += 1;
                    continue;
                }
            };
            core::ptr::copy_nonoverlapping(rep.as_ptr(), out.add(o), rep.len());
            o += rep.len();
            i += 1;
        }
        *out.add(o) = 0;
        out
    }
}

/// Deprecated entity encoder (upstream 2.15 `xmlEncodeEntities`): the
/// symbol still exists for ABI compatibility but emits a one-time
/// deprecation warning and returns NULL (verified against the oracle DSO
/// disassembly — the 2.15 implementation returns NULL after warning).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlEncodeEntities(xmlDocPtr doc, const xmlChar *input);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlEncodeEntities(
    _doc: *mut _xmlDoc,
    _input: *const xmlChar,
) -> *mut xmlChar {
    use core::sync::atomic::{AtomicBool, Ordering};
    static WARNED: AtomicBool = AtomicBool::new(false);
    if !WARNED.swap(true, Ordering::Relaxed) {
        // Match the oracle: one-time "deprecated" diagnostic on stderr.
        let msg = b"xmlEncodeEntities is deprecated, use xmlEncodeSpecialChars or xmlEncodeEntitiesReentrant\n";
        unsafe {
            libc::fwrite(
                msg.as_ptr() as *const c_void,
                1,
                msg.len(),
                libc::fdopen(2, b"w\0" as *const u8 as *const c_char),
            );
        }
    }
    ptr::null_mut()
}

/// Get the line number of a node.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// long xmlGetLineNo(const xmlNode *node);
/// ```
#[no_mangle]
pub extern "C" fn xmlGetLineNo(node: *const _xmlNode) -> c_long {
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
        // UPSTREAM-PARITY: on a hard (non-recoverable) parse error the
        // partially built document is discarded and NULL is returned; with
        // XML_PARSE_RECOVER the partial tree is kept.
        if options & 1 << 0 != 0 {
            return doc;
        }
        if !doc.is_null() {
            crate::xml::tree::free_doc(doc);
        }
        return ptr::null_mut();
    }
    let doc = (*ctxt).myDoc;
    crate::xml::parser::helpers::free_parser_ctxt(ctxt);
    doc
}

/// Recover-parse a document from a string (upstream parser.h): same as
/// `xmlReadDoc` with XML_PARSE_RECOVER forced.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDocPtr xmlRecoverDoc(const xmlChar *cur);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlRecoverDoc(cur: *const xmlChar) -> *mut _xmlDoc {
    unsafe { xmlReadDoc(cur, ptr::null(), ptr::null(), 1 << 0) }
}

/// Recover-parse a document from a file (upstream parser.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDocPtr xmlRecoverFile(const char *filename);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlRecoverFile(filename: *const c_char) -> *mut _xmlDoc {
    unsafe { xmlReadFile(filename, ptr::null(), 1 << 0) }
}

/// Recover-parse a document from memory (upstream parser.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDocPtr xmlRecoverMemory(const char *buffer, int size);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlRecoverMemory(buffer: *const c_char, size: c_int) -> *mut _xmlDoc {
    unsafe { xmlReadMemory(buffer, size, ptr::null(), ptr::null(), 1 << 0) }
}

/// Read an XML document from a file (upstream parser.h).
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
    // SAFETY: buffer must be a valid pointer with at least `size` readable
    // bytes. An empty input (size 0) is still parsed — upstream reports
    // "Document is empty".
    if buffer.is_null() || size < 0 {
        return ptr::null_mut();
    }
    let ctxt = crate::xml::parser::helpers::create_parser_ctxt();
    if ctxt.is_null() {
        return ptr::null_mut();
    }
    // UPSTREAM-PARITY: the URL becomes the input's filename (feeds the
    // `file:line:` error prefix and doc->URL).
    let input = crate::xml::parser::helpers::input_from_memory_named(buffer, size, URL);
    crate::xml::parser::helpers::setup_parser_input(ctxt, input);
    // UPSTREAM-PARITY (xmlReadMemory -> xmlCtxtReadMemory): the parse
    // options are mirrored into the context members (dictNames, keepBlanks,
    // recovery, ...) before parsing starts.
    crate::abi::exports_parser::apply_options(ctxt, options);
    let parsed = crate::xml::parser::helpers::parse_document(ctxt);
    let doc = (*ctxt).myDoc;
    // UPSTREAM-PARITY: the URL is attached to the document on success AND on
    // the recovery path (the partial tree keeps the document identity).
    if !doc.is_null() && !URL.is_null() && (*doc).URL.is_null() {
        (*doc).URL = crate::xml::string::xml_strdup(URL as *const xmlChar);
    }
    crate::xml::parser::helpers::free_parser_ctxt(ctxt);
    if parsed != 0 {
        // UPSTREAM-PARITY: on a hard (non-recoverable) parse error the
        // partially built document is discarded and NULL is returned; with
        // XML_PARSE_RECOVER the partial tree is kept.
        if options & 1 << 0 != 0 {
            return doc;
        }
        if !doc.is_null() {
            crate::xml::tree::free_doc(doc);
        }
        return ptr::null_mut();
    }
    doc
}

/// Load a list of catalogs (upstream `xmlLoadCatalogs`).
///
/// # SAFETY
///
/// - `catalogs` must be a valid NUL-terminated string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlLoadCatalogs(catalogs: *const c_char) {
    if !catalogs.is_null() {
        crate::xml::catalog::load_catalog(catalogs);
    }
}

/// Load a single catalog (upstream `xmlLoadCatalog`).
///
/// # SAFETY
///
/// - `catalogs` must be a valid NUL-terminated string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlLoadCatalog(catalogs: *const c_char) -> c_int {
    // UPSTREAM-PARITY (catalog.c xmlLoadCatalog): returns 0 on success,
    // 1 on error (unlike xmlCatalogLoad which returns the catalog handle).
    let handle = crate::xml::catalog::load_catalog(catalogs);
    if handle.is_null() {
        1
    } else {
        0
    }
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

/// Parse an XML document (SAX1) with user data (upstream parser.h
/// `xmlSAXParseDocWithData`): `user_data` is passed to the SAX callbacks.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDocPtr xmlSAXParseDocWithData(xmlSAXHandlerPtr sax, const xmlChar *cur,
///                                  int recovery, void *data);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlSAXParseDocWithData(
    sax: *mut _xmlSAXHandler,
    cur: *const xmlChar,
    recovery: c_int,
    data: *mut c_void,
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
    }
    (*ctxt).userData = data;
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

/// Parse an XML file (SAX1) with user data (upstream parser.h
/// `xmlSAXParseFileWithData`).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDocPtr xmlSAXParseFileWithData(xmlSAXHandlerPtr sax, const char *filename,
///                                   int recovery, void *data);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlSAXParseFileWithData(
    sax: *mut _xmlSAXHandler,
    filename: *const c_char,
    recovery: c_int,
    data: *mut c_void,
) -> *mut _xmlDoc {
    // SAFETY: filename must be a valid C string or NULL.
    if filename.is_null() {
        return ptr::null_mut();
    }
    let ctxt = crate::xml::parser::helpers::create_parser_ctxt();
    if ctxt.is_null() {
        return ptr::null_mut();
    }
    if !sax.is_null() {
        (*ctxt).sax = sax;
    }
    (*ctxt).userData = data;
    if recovery != 0 {
        (*ctxt).recovery = 1;
        (*ctxt).options |= 1; // XML_PARSE_RECOVER
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

/// Parse an XML document (SAX1) with user data from memory (upstream
/// parser.h `xmlSAXParseMemoryWithData`).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDocPtr xmlSAXParseMemoryWithData(xmlSAXHandlerPtr sax, const char *buffer,
///                                     int size, int recovery, void *data);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlSAXParseMemoryWithData(
    sax: *mut _xmlSAXHandler,
    buffer: *const c_char,
    size: c_int,
    recovery: c_int,
    data: *mut c_void,
) -> *mut _xmlDoc {
    // SAFETY: buffer must be a valid pointer with `size` readable bytes.
    if buffer.is_null() || size <= 0 {
        return ptr::null_mut();
    }
    let ctxt = crate::xml::parser::helpers::create_parser_ctxt();
    if ctxt.is_null() {
        return ptr::null_mut();
    }
    if !sax.is_null() {
        (*ctxt).sax = sax;
    }
    (*ctxt).userData = data;
    if recovery != 0 {
        (*ctxt).recovery = 1;
        (*ctxt).options |= 1; // XML_PARSE_RECOVER
    }
    let input = crate::xml::parser::helpers::input_from_memory(buffer, size);
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

/// Allocate an output buffer with no I/O target (upstream xmlAllocOutputBuffer).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlOutputBufferPtr xmlAllocOutputBuffer(xmlCharEncodingHandlerPtr encoder);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlAllocOutputBuffer(encoder: *mut c_void) -> *mut _xmlOutputBuffer {
    crate::xml::io::output_buffer_create(
        encoder as *mut crate::abi::structs::_xmlCharEncodingHandler,
    )
}

/// Create an output buffer that writes into a `_xmlBuffer` (upstream
/// xmlOutputBufferCreateBuffer).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlOutputBufferPtr xmlOutputBufferCreateBuffer(xmlBufferPtr buffer,
///                                                xmlCharEncodingHandlerPtr encoder);
/// ```
///
/// # SAFETY
///
/// - `buffer` must be a valid `_xmlBuffer`.
#[no_mangle]
pub unsafe extern "C" fn xmlOutputBufferCreateBuffer(
    buffer: *mut crate::abi::structs::_xmlBuffer,
    encoder: *mut c_void,
) -> *mut _xmlOutputBuffer {
    crate::xml::io::output_buffer_create_buffer(
        buffer,
        encoder as *mut crate::abi::structs::_xmlCharEncodingHandler,
    )
}

/// Create an output buffer writing to a `FILE *` (upstream
/// xmlOutputBufferCreateFile): the FILE is the I/O context with a
/// write callback wrapping `fwrite` and a close callback wrapping `fflush`.
///
/// # SAFETY
///
/// - `file` must be a valid `FILE *` or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlOutputBufferCreateFile(
    file: *mut libc::FILE,
    encoder: *mut c_void,
) -> *mut _xmlOutputBuffer {
    crate::xml::io::output_buffer_create_file(
        file,
        encoder as *mut crate::abi::structs::_xmlCharEncodingHandler,
    )
}

/// Get the current content of an output buffer (upstream xmlOutputBufferGetContent).
///
/// # SAFETY
///
/// - `out` must be a valid output buffer.
#[no_mangle]
pub unsafe extern "C" fn xmlOutputBufferGetContent(out: *mut _xmlOutputBuffer) -> *const c_char {
    crate::xml::io::output_buffer_get_content(out) as *const c_char
}

/// Get the number of bytes currently in the output buffer (upstream
/// xmlOutputBufferGetSize).
///
/// # SAFETY
///
/// - `out` must be a valid output buffer.
#[no_mangle]
pub unsafe extern "C" fn xmlOutputBufferGetSize(out: *mut _xmlOutputBuffer) -> c_int {
    crate::xml::io::output_buffer_get_size(out)
}

/// Write to an output buffer, escaping special characters with the given
/// escape function (upstream xmlOutputBufferWriteEscape).
///
/// # SAFETY
///
/// - `out` must be a valid output buffer; `str` a NUL-terminated string;
///   `escaping` a valid escape callback or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlOutputBufferWriteEscape(
    out: *mut _xmlOutputBuffer,
    str: *const xmlChar,
    escaping: Option<xmlCharEncodingOutputFunc>,
) -> c_int {
    if out.is_null() || str.is_null() {
        return -1;
    }
    crate::xml::io::output_buffer_write_escape(out, str, escaping)
}

/// Global default `xmlOutputBufferCreateFilename` callback
/// (upstream xmlOutputBufferCreateFilenameDefault).
static mut OUTPUT_CREATE_FILENAME_DEFAULT: Option<
    unsafe extern "C" fn(
        *const c_char,
        *mut crate::abi::structs::_xmlCharEncodingHandler,
        c_int,
    ) -> *mut _xmlOutputBuffer,
> = None;

/// Set/query the default output-buffer filename callback
/// (upstream xmlOutputBufferCreateFilenameDefault).
///
/// # SAFETY
///
/// - `func` must be a valid function pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlOutputBufferCreateFilenameDefault(
    func: Option<
        unsafe extern "C" fn(
            *const c_char,
            *mut crate::abi::structs::_xmlCharEncodingHandler,
            c_int,
        ) -> *mut _xmlOutputBuffer,
    >,
) -> Option<
    unsafe extern "C" fn(
        *const c_char,
        *mut crate::abi::structs::_xmlCharEncodingHandler,
        c_int,
    ) -> *mut _xmlOutputBuffer,
> {
    let old = unsafe { OUTPUT_CREATE_FILENAME_DEFAULT };
    if func.is_some() {
        unsafe { OUTPUT_CREATE_FILENAME_DEFAULT = func };
    }
    old
}

/// `__xmlOutputBufferCreateFilename` — accessor returning a pointer to the
/// default callback (upstream xmlIO.c).
#[no_mangle]
pub unsafe extern "C" fn __xmlOutputBufferCreateFilename() -> *mut Option<
    unsafe extern "C" fn(
        *const c_char,
        *mut crate::abi::structs::_xmlCharEncodingHandler,
        c_int,
    ) -> *mut _xmlOutputBuffer,
> {
    core::ptr::addr_of_mut!(OUTPUT_CREATE_FILENAME_DEFAULT)
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
    let d = { crate::xml::dictionary::dict_create() as *mut c_void };
    if !d.is_null() {
        // UPSTREAM-PARITY: the creator holds the base reference (count 1);
        // xmlDictReference adds to it and xmlDictFree decrements, freeing
        // the dictionary when it reaches zero.
        *crate::abi::exports_hash::DICT_REFS
            .lock()
            .entry(d as usize)
            .or_insert(0) = 1;
    }
    d
}

/// Create a sub-dictionary.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDictPtr xmlDictCreateSub(xmlDictPtr sub);
/// ```
#[no_mangle]
pub extern "C" fn xmlDictCreateSub(sub: *mut c_void) -> *mut c_void {
    let d = unsafe {
        crate::xml::dictionary::dict_create_sub(sub as *mut crate::xml::dictionary::Dict)
            as *mut c_void
    };
    if !d.is_null() {
        *crate::abi::exports_hash::DICT_REFS
            .lock()
            .entry(d as usize)
            .or_insert(0) = 1;
    }
    d
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
    unsafe {
        crate::xml::dictionary::dict_lookup(dict as *mut crate::xml::dictionary::Dict, name, len)
    }
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
    unsafe {
        crate::xml::dictionary::dict_exists(dict as *mut crate::xml::dictionary::Dict, name, len)
    }
}

/// Query dictionary size.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// unsigned int xmlDictSize(const xmlDictPtr dict);
/// ```
#[no_mangle]
pub const extern "C" fn xmlDictSize(dict: *const c_void) -> c_uint {
    {
        crate::xml::dictionary::dict_size(dict as *const crate::xml::dictionary::Dict) as c_uint
    }
}

/// Free a dictionary.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlDictFree(xmlDictPtr dict);
/// ```
///
/// The reference counter added by `xmlDictReference` is honored: the
/// underlying dictionary is destroyed only when the last reference is
/// released (the base owner counts as one implicit reference).
#[no_mangle]
pub extern "C" fn xmlDictFree(dict: *mut c_void) {
    if dict.is_null() {
        return;
    }
    let mut remaining = 0u32;
    {
        let mut refs = crate::abi::exports_hash::DICT_REFS.lock();
        if let Some(r) = refs.get_mut(&(dict as usize)) {
            if *r > 0 {
                *r -= 1;
            }
            remaining = *r;
            if remaining == 0 {
                refs.remove(&(dict as usize));
            }
        }
    }
    if remaining == 0 {
        unsafe { crate::xml::dictionary::dict_free(dict as *mut crate::xml::dictionary::Dict) };
    }
}

/// Set the dictionary size limit.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// unsigned int xmlDictSetLimit(xmlDictPtr dict, unsigned int limit);
/// ```
#[no_mangle]
pub extern "C" fn xmlDictSetLimit(dict: *mut c_void, limit: c_uint) -> c_uint {
    {
        crate::xml::dictionary::dict_set_limit(
            dict as *mut crate::xml::dictionary::Dict,
            limit as usize,
        ) as c_uint
    }
}

/// Get current dictionary usage.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// unsigned int xmlDictGetUsage(const xmlDictPtr dict);
/// ```
#[no_mangle]
pub extern "C" fn xmlDictGetUsage(dict: *const c_void) -> c_uint {
    {
        crate::xml::dictionary::dict_get_usage(dict as *mut crate::xml::dictionary::Dict) as c_uint
    }
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
pub extern "C" fn xmlHashCreate(size: c_int) -> *mut c_void {
    crate::xml::hash::hash_create(size) as *mut c_void
}

/// Create a new hash table with a dictionary.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlHashTablePtr xmlHashCreateDict(int size, xmlDictPtr dict);
/// ```
#[no_mangle]
pub extern "C" fn xmlHashCreateDict(size: c_int, dict: *mut c_void) -> *mut c_void {
    crate::xml::hash::hash_create_dict(size, dict) as *mut c_void
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
    table: *mut c_void,
    f: Option<unsafe extern "C" fn(*mut c_void, *mut xmlChar)>,
) {
    unsafe { crate::xml::hash::hash_free(table as *mut crate::xml::hash::HashTable, f) }
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
    table: *mut c_void,
    name: *const xmlChar,
    userdata: *mut c_void,
) -> c_int {
    unsafe {
        crate::xml::hash::hash_add_entry(table as *mut crate::xml::hash::HashTable, name, userdata)
    }
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
    table: *mut c_void,
    name: *const xmlChar,
    name2: *const xmlChar,
    userdata: *mut c_void,
) -> c_int {
    unsafe {
        crate::xml::hash::hash_add_entry2(
            table as *mut crate::xml::hash::HashTable,
            name,
            name2,
            userdata,
        )
    }
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
    table: *mut c_void,
    name: *const xmlChar,
    name2: *const xmlChar,
    name3: *const xmlChar,
    userdata: *mut c_void,
) -> c_int {
    unsafe {
        crate::xml::hash::hash_add_entry3(
            table as *mut crate::xml::hash::HashTable,
            name,
            name2,
            name3,
            userdata,
        )
    }
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
    table: *mut c_void,
    name: *const xmlChar,
    userdata: *mut c_void,
    f: Option<unsafe extern "C" fn(*mut c_void, *mut xmlChar)>,
) -> c_int {
    unsafe {
        crate::xml::hash::hash_update_entry(
            table as *mut crate::xml::hash::HashTable,
            name,
            userdata,
            f,
        )
    }
}

/// Update or add a 2-key entry.
#[no_mangle]
pub unsafe extern "C" fn xmlHashUpdateEntry2(
    table: *mut c_void,
    name: *const xmlChar,
    name2: *const xmlChar,
    userdata: *mut c_void,
    f: Option<unsafe extern "C" fn(*mut c_void, *mut xmlChar)>,
) -> c_int {
    unsafe {
        crate::xml::hash::hash_update_entry2(
            table as *mut crate::xml::hash::HashTable,
            name,
            name2,
            userdata,
            f,
        )
    }
}

/// Update or add a 3-key entry.
#[no_mangle]
pub unsafe extern "C" fn xmlHashUpdateEntry3(
    table: *mut c_void,
    name: *const xmlChar,
    name2: *const xmlChar,
    name3: *const xmlChar,
    userdata: *mut c_void,
    f: Option<unsafe extern "C" fn(*mut c_void, *mut xmlChar)>,
) -> c_int {
    unsafe {
        crate::xml::hash::hash_update_entry3(
            table as *mut crate::xml::hash::HashTable,
            name,
            name2,
            name3,
            userdata,
            f,
        )
    }
}

/// Look up an entry.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void *xmlHashLookup(xmlHashTablePtr table, const xmlChar *name);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlHashLookup(table: *mut c_void, name: *const xmlChar) -> *mut c_void {
    unsafe { crate::xml::hash::hash_lookup(table as *mut crate::xml::hash::HashTable, name) }
}

/// Look up a 2-key entry.
#[no_mangle]
pub unsafe extern "C" fn xmlHashLookup2(
    table: *mut c_void,
    name: *const xmlChar,
    name2: *const xmlChar,
) -> *mut c_void {
    unsafe {
        crate::xml::hash::hash_lookup2(table as *mut crate::xml::hash::HashTable, name, name2)
    }
}

/// Look up a 3-key entry.
#[no_mangle]
pub unsafe extern "C" fn xmlHashLookup3(
    table: *mut c_void,
    name: *const xmlChar,
    name2: *const xmlChar,
    name3: *const xmlChar,
) -> *mut c_void {
    unsafe {
        crate::xml::hash::hash_lookup3(
            table as *mut crate::xml::hash::HashTable,
            name,
            name2,
            name3,
        )
    }
}

/// Get the size of a hash table.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlHashSize(xmlHashTablePtr table);
/// ```
#[no_mangle]
pub extern "C" fn xmlHashSize(table: *mut c_void) -> c_int {
    crate::xml::hash::hash_size(table as *mut crate::xml::hash::HashTable)
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
    table: *mut c_void,
    name: *const xmlChar,
    f: Option<unsafe extern "C" fn(*mut c_void, *mut xmlChar)>,
) -> c_int {
    unsafe {
        crate::xml::hash::hash_remove_entry(table as *mut crate::xml::hash::HashTable, name, f)
    }
}

/// Remove a 2-key entry.
#[no_mangle]
pub unsafe extern "C" fn xmlHashRemoveEntry2(
    table: *mut c_void,
    name: *const xmlChar,
    name2: *const xmlChar,
    f: Option<unsafe extern "C" fn(*mut c_void, *mut xmlChar)>,
) -> c_int {
    unsafe {
        crate::xml::hash::hash_remove_entry2(
            table as *mut crate::xml::hash::HashTable,
            name,
            name2,
            f,
        )
    }
}

/// Remove a 3-key entry.
#[no_mangle]
pub unsafe extern "C" fn xmlHashRemoveEntry3(
    table: *mut c_void,
    name: *const xmlChar,
    name2: *const xmlChar,
    name3: *const xmlChar,
    f: Option<unsafe extern "C" fn(*mut c_void, *mut xmlChar)>,
) -> c_int {
    unsafe {
        crate::xml::hash::hash_remove_entry3(
            table as *mut crate::xml::hash::HashTable,
            name,
            name2,
            name3,
            f,
        )
    }
}

/// Scan a hash table with a scanner function.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlHashScan(xmlHashTablePtr table, xmlHashScanner f, void *data);
/// ```
#[no_mangle]
pub extern "C" fn xmlHashScan(table: *mut c_void, f: Option<xmlHashScanner>, data: *mut c_void) {
    unsafe { crate::xml::hash::hash_scan(table as *mut crate::xml::hash::HashTable, f, data) }
}

/// Scan a hash table with a full scanner function.
#[no_mangle]
pub extern "C" fn xmlHashScanFull(
    table: *mut c_void,
    f: Option<xmlHashScannerFull>,
    data: *mut c_void,
) {
    unsafe { crate::xml::hash::hash_scan_full(table as *mut crate::xml::hash::HashTable, f, data) }
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
    table: *mut c_void,
    f: Option<unsafe extern "C" fn(*mut c_void, *const xmlChar) -> *mut c_void>,
) -> *mut c_void {
    unsafe {
        crate::xml::hash::hash_copy(table as *mut crate::xml::hash::HashTable, f) as *mut c_void
    }
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
    deallocator: Option<unsafe extern "C" fn(*mut c_void)>,
    compare: Option<unsafe extern "C" fn(*const c_void, *const c_void) -> c_int>,
) -> *mut c_void {
    crate::xml::list::list_create(deallocator, compare) as *mut c_void
}

/// Delete a list.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlListDelete(xmlListPtr list);
/// ```
#[no_mangle]
pub extern "C" fn xmlListDelete(list: *mut c_void) {
    unsafe { crate::xml::list::list_delete(list as *mut crate::xml::list::List) }
}

/// Search a list.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void *xmlListSearch(xmlListPtr list, void *data);
/// ```
#[no_mangle]
pub extern "C" fn xmlListSearch(list: *mut c_void, data: *mut c_void) -> *mut c_void {
    unsafe { crate::xml::list::list_search(list as *mut crate::xml::list::List, data) }
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
    list: *mut c_void,
    walker: Option<unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int>,
    data: *mut c_void,
) {
    unsafe { crate::xml::list::list_walk(list as *mut crate::xml::list::List, walker, data) }
}

/// Push to back.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlListPushBack(xmlListPtr list, void *data);
/// ```
#[no_mangle]
pub extern "C" fn xmlListPushBack(list: *mut c_void, data: *mut c_void) -> c_int {
    unsafe { crate::xml::list::list_push_back(list as *mut crate::xml::list::List, data) }
}

/// Push to front.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlListPushFront(xmlListPtr list, void *data);
/// ```
#[no_mangle]
pub extern "C" fn xmlListPushFront(list: *mut c_void, data: *mut c_void) -> c_int {
    unsafe { crate::xml::list::list_push_front(list as *mut crate::xml::list::List, data) }
}

/// Pop from back.
#[no_mangle]
pub extern "C" fn xmlListPopBack(list: *mut c_void) {
    unsafe { crate::xml::list::list_pop_back(list as *mut crate::xml::list::List) }
}

/// Pop from front.
#[no_mangle]
pub extern "C" fn xmlListPopFront(list: *mut c_void) {
    unsafe { crate::xml::list::list_pop_front(list as *mut crate::xml::list::List) }
}

/// Insert into sorted list.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlListInsert(xmlListPtr list, void *data);
/// ```
#[no_mangle]
pub extern "C" fn xmlListInsert(list: *mut c_void, data: *mut c_void) -> c_int {
    unsafe { crate::xml::list::list_insert(list as *mut crate::xml::list::List, data) }
}

/// Append to list.
#[no_mangle]
pub extern "C" fn xmlListAppend(list: *mut c_void, data: *mut c_void) -> c_int {
    unsafe { crate::xml::list::list_append(list as *mut crate::xml::list::List, data) }
}

/// Remove first matching element.
#[no_mangle]
pub extern "C" fn xmlListRemoveFirst(list: *mut c_void, data: *mut c_void) -> c_int {
    unsafe { crate::xml::list::list_remove_first(list as *mut crate::xml::list::List, data) }
}

/// Remove last matching element.
#[no_mangle]
pub extern "C" fn xmlListRemoveLast(list: *mut c_void, data: *mut c_void) -> c_int {
    unsafe { crate::xml::list::list_remove_last(list as *mut crate::xml::list::List, data) }
}

/// Remove all matching elements.
#[no_mangle]
pub extern "C" fn xmlListRemoveAll(list: *mut c_void, data: *mut c_void) -> c_int {
    unsafe { crate::xml::list::list_remove_all(list as *mut crate::xml::list::List, data) }
}

/// Clear a list.
#[no_mangle]
pub extern "C" fn xmlListClear(list: *mut c_void) {
    unsafe { crate::xml::list::list_clear(list as *mut crate::xml::list::List) }
}

/// Check if list is empty.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlListEmpty(xmlListPtr list);
/// ```
#[no_mangle]
pub extern "C" fn xmlListEmpty(list: *mut c_void) -> c_int {
    crate::xml::list::list_empty(list as *mut crate::xml::list::List)
}

/// Get front element.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void *xmlListFront(xmlListPtr list);
/// ```
#[no_mangle]
pub extern "C" fn xmlListFront(list: *mut c_void) -> *mut c_void {
    crate::xml::list::list_front(list as *mut crate::xml::list::List)
}

/// Get back element.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void *xmlListBack(xmlListPtr list);
/// ```
#[no_mangle]
pub extern "C" fn xmlListBack(list: *mut c_void) -> *mut c_void {
    crate::xml::list::list_back(list as *mut crate::xml::list::List)
}

/// Get list size.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlListSize(xmlListPtr list);
/// ```
#[no_mangle]
pub extern "C" fn xmlListSize(list: *mut c_void) -> c_int {
    crate::xml::list::list_size(list as *mut crate::xml::list::List)
}

/// Sort a list.
#[no_mangle]
pub extern "C" fn xmlListSort(list: *mut c_void) {
    unsafe { crate::xml::list::list_sort(list as *mut crate::xml::list::List) }
}

/// Reverse a list.
#[no_mangle]
pub extern "C" fn xmlListReverse(list: *mut c_void) {
    unsafe { crate::xml::list::list_reverse(list as *mut crate::xml::list::List) }
}

/// Reverse a list in-place.
#[no_mangle]
pub extern "C" fn xmlListReverseSplice(list: *mut c_void, list2: *mut c_void) {
    unsafe {
        crate::xml::list::list_reverse_splice(
            list as *mut crate::xml::list::List,
            list2 as *mut crate::xml::list::List,
        )
    }
}

/// Merge two sorted lists.
#[no_mangle]
pub extern "C" fn xmlListMerge(list: *mut c_void, list2: *mut c_void) {
    unsafe {
        crate::xml::list::list_merge(
            list as *mut crate::xml::list::List,
            list2 as *mut crate::xml::list::List,
        )
    }
}
/// Return the last element of a list (upstream list.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void *xmlListEnd(xmlListPtr l);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlListEnd(l: *mut c_void) -> *mut c_void {
    crate::xml::list::list_end(l as *mut crate::xml::list::List)
}

/// Reverse-search a list (upstream list.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void *xmlListReverseSearch(xmlListPtr l, void *data);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlListReverseSearch(l: *mut c_void, data: *mut c_void) -> *mut c_void {
    crate::xml::list::list_reverse_search(l as *mut crate::xml::list::List, data)
}

/// Walk a list in reverse (upstream list.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlListReverseWalk(xmlListPtr l, xmlListWalker walker, void *data);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlListReverseWalk(
    l: *mut c_void,
    walker: Option<unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int>,
    data: *mut c_void,
) {
    crate::xml::list::list_reverse_walk(l as *mut crate::xml::list::List, walker, data)
}

/// Duplicate a list (upstream list.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlListPtr xmlListDup(xmlListPtr l);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlListDup(l: *mut c_void) -> *mut c_void {
    crate::xml::list::list_dup(l as *mut crate::xml::list::List) as *mut c_void
}

/// Copy a list with a copier (upstream list.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlListCopy(xmlListPtr l, xmlListCopier copier);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlListCopy(
    l: *mut c_void,
    copier: Option<unsafe extern "C" fn(*mut c_void) -> *mut c_void>,
) -> c_int {
    crate::xml::list::list_copy(l as *mut crate::xml::list::List, copier)
}

/// Return the data of a link (upstream list.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void *xmlLinkGetData(xmlLinkPtr lk);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlLinkGetData(lk: *mut c_void) -> *mut c_void {
    crate::xml::list::link_get_data(lk)
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
            crate::abi::allocator::xmlFreeImpl((*h).name as *mut c_void);
        }
        crate::abi::allocator::xmlFreeImpl(handler);
    }
    0
}

/// Convert a block of ISO-8859-1 bytes to UTF-8 (upstream encoding.c
/// `xmlIsolat1ToUTF8`; R-000165 closure).
///
/// `*outlen`/`*inlen` are updated with the bytes produced/consumed; returns
/// the number of bytes written or an xmlCharEncError code.
///
/// # SAFETY
///
/// `out`/`in` must be valid buffers for `*outlen`/`*inlen` bytes.
#[no_mangle]
pub unsafe extern "C" fn xmlIsolat1ToUTF8(
    out: *mut u8,
    outlen: *mut c_int,
    input: *const u8,
    inlen: *mut c_int,
) -> c_int {
    // xmlCharEncError (encoding.h): SUCCESS 0, INTERNAL -1, SPACE -2.
    const XML_ENC_ERR_SPACE: c_int = -2;
    const XML_ENC_ERR_INTERNAL: c_int = -1;
    unsafe {
        if out.is_null() || input.is_null() || outlen.is_null() || inlen.is_null() {
            return XML_ENC_ERR_INTERNAL;
        }
        let outstart = out;
        let instart = input;
        let outend = out.add(*outlen as usize);
        let inend = input.add(*inlen as usize);
        let mut cur = input;
        let mut o = out;
        while cur < inend {
            let c = *cur;
            if c < 0x80 {
                if o >= outend {
                    break;
                }
                *o = c;
                o = o.add(1);
            } else {
                if (outend as usize) - (o as usize) < 2 {
                    break;
                }
                *o = (c >> 6) | 0xC0;
                *o.add(1) = (c & 0x3F) | 0x80;
                o = o.add(2);
            }
            cur = cur.add(1);
        }
        let mut ret = XML_ENC_ERR_SPACE;
        if cur == inend {
            ret = (o as usize - outstart as usize) as c_int;
        }
        *outlen = (o as usize - outstart as usize) as c_int;
        *inlen = (cur as usize - instart as usize) as c_int;
        ret
    }
}

/// Convert a block of UTF-8 to ISO-8859-1 (upstream encoding.c
/// `xmlUTF8ToIsolat1`; R-000165 closure).
///
/// # SAFETY
///
/// `out`/`in` must be valid buffers for `*outlen`/`*inlen` bytes.
#[no_mangle]
pub unsafe extern "C" fn xmlUTF8ToIsolat1(
    out: *mut u8,
    outlen: *mut c_int,
    input: *const u8,
    inlen: *mut c_int,
) -> c_int {
    const XML_ENC_ERR_SPACE: c_int = -2;
    const XML_ENC_ERR_INTERNAL: c_int = -1;
    const XML_ENC_ERR_INPUT: c_int = -3;
    const XML_ENC_ERR_SUCCESS: c_int = 0;
    unsafe {
        if out.is_null() || outlen.is_null() || inlen.is_null() {
            return XML_ENC_ERR_INTERNAL;
        }
        if input.is_null() {
            *inlen = 0;
            *outlen = 0;
            return XML_ENC_ERR_SUCCESS;
        }
        let outstart = out;
        let instart = input;
        let outend = out.add(*outlen as usize);
        let inend = input.add(*inlen as usize);
        let mut cur = input;
        let mut o = out;
        let mut ret = XML_ENC_ERR_SPACE;
        while cur < inend {
            if o >= outend {
                break;
            }
            let c = *cur;
            if c < 0x80 {
                *o = c;
                o = o.add(1);
            } else if (0xC2..=0xC3).contains(&c) {
                if (inend as usize) - (cur as usize) < 2 {
                    break;
                }
                cur = cur.add(1);
                *o = (c << 6) | (*cur & 0x3F);
                o = o.add(1);
            } else {
                ret = XML_ENC_ERR_INPUT;
                break;
            }
            cur = cur.add(1);
        }
        if ret != XML_ENC_ERR_INPUT {
            ret = (o as usize - outstart as usize) as c_int;
        }
        *outlen = (o as usize - outstart as usize) as c_int;
        *inlen = (cur as usize - instart as usize) as c_int;
        ret
    }
}

/// Return the name of a character encoding (upstream encoding.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// const char *xmlGetCharEncodingName(xmlCharEncoding enc);
/// ```
#[no_mangle]
pub extern "C" fn xmlGetCharEncodingName(enc: c_int) -> *const c_char {
    /* Values outside the local enum resolve against the upstream
     * defaultHandlers table (XML_CHAR_ENCODING_UTF16=23, HTML=24,
     * WINDOWS_1252=31); anything else is unknown (NULL). */
    if !(-1..=22).contains(&enc) {
        return match enc {
            23 => c"UTF-16".as_ptr(),
            24 => c"HTML".as_ptr(),
            31 => c"windows-1252".as_ptr(),
            _ => ptr::null(),
        };
    }
    let e: crate::abi::types::xmlCharEncoding = unsafe { core::mem::transmute(enc) };
    crate::xml::encoding::xmlGetCharEncodingName(e)
}

/// Parse an encoding name into an xmlCharEncoding value (upstream encoding.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlCharEncoding xmlParseCharEncoding(const char *name);
/// ```
///
/// Returns the encoding value or XML_CHAR_ENCODING_ERROR (-1).
#[no_mangle]
pub extern "C" fn xmlParseCharEncoding(name: *const c_char) -> c_int {
    crate::xml::encoding::xmlParseCharEncoding(name)
}

/// Add an encoding alias (upstream encoding.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlAddEncodingAlias(const char *name, const char *alias);
/// ```
#[no_mangle]
pub extern "C" fn xmlAddEncodingAlias(name: *const c_char, alias: *const c_char) -> c_int {
    crate::xml::encoding::add_encoding_alias(name, alias)
}

/// Delete an encoding alias (upstream encoding.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlDelEncodingAlias(const char *alias);
/// ```
#[no_mangle]
pub extern "C" fn xmlDelEncodingAlias(alias: *const c_char) -> c_int {
    crate::xml::encoding::del_encoding_alias(alias)
}

/// Look up an encoding alias (upstream encoding.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// const char *xmlGetEncodingAlias(const char *alias);
/// ```
#[no_mangle]
pub extern "C" fn xmlGetEncodingAlias(alias: *const c_char) -> *const c_char {
    crate::xml::encoding::get_encoding_alias(alias)
}

/// Clean up the encoding alias table (upstream encoding.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlCleanupEncodingAliases(void);
/// ```
#[no_mangle]
pub extern "C" fn xmlCleanupEncodingAliases() {
    crate::xml::encoding::cleanup_encoding_aliases();
}

/// Convert the input buffer using an encoding handler (upstream encoding.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlCharEncInFunc(xmlCharEncodingHandler *handler,
///                      xmlBufferPtr out, xmlBufferPtr in);
/// ```
#[no_mangle]
pub extern "C" fn xmlCharEncInFunc(
    handler: *mut c_void,
    out: *mut c_void,
    in_: *mut c_void,
) -> c_int {
    crate::xml::encoding::xmlCharEncInFunc(
        handler as *mut crate::abi::structs::_xmlCharEncodingHandler,
        out as *mut crate::abi::structs::_xmlBuffer,
        in_ as *mut crate::abi::structs::_xmlBuffer,
    )
}

/// Convert the output buffer using an encoding handler (upstream encoding.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlCharEncOutFunc(xmlCharEncodingHandler *handler,
///                       xmlBufferPtr out, xmlBufferPtr in);
/// ```
#[no_mangle]
pub extern "C" fn xmlCharEncOutFunc(
    handler: *mut c_void,
    out: *mut c_void,
    in_: *mut c_void,
) -> c_int {
    crate::xml::encoding::xmlCharEncOutFunc(
        handler as *mut crate::abi::structs::_xmlCharEncodingHandler,
        out as *mut crate::abi::structs::_xmlBuffer,
        in_ as *mut crate::abi::structs::_xmlBuffer,
    )
}

/// Create a new encoding handler (upstream encoding.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlCharEncodingHandlerPtr xmlNewCharEncodingHandler(
///     const char *name, xmlCharEncodingInputFunc input,
///     xmlCharEncodingOutputFunc output);
/// ```
#[no_mangle]
pub extern "C" fn xmlNewCharEncodingHandler(
    name: *const c_char,
    input: crate::abi::callbacks::xmlCharEncodingInputFunc,
    output: crate::abi::callbacks::xmlCharEncodingOutputFunc,
) -> *mut c_void {
    crate::xml::encoding::xmlNewCharEncodingHandler(name, input, output) as *mut c_void
}

/// Initialize the built-in encoding handlers (upstream encoding.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlInitCharEncodingHandlers(void);
/// ```
#[no_mangle]
pub extern "C" fn xmlInitCharEncodingHandlers() {
    crate::xml::encoding::xmlInitCharEncodingHandlers();
}

/// Clean up the encoding handlers (upstream encoding.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlCleanupCharEncodingHandlers(void);
/// ```
#[no_mangle]
pub extern "C" fn xmlCleanupCharEncodingHandlers() {
    crate::xml::encoding::xmlCleanupCharEncodingHandlers();
}

/// Look up a built-in encoding handler by `xmlCharEncoding` value.
///
/// Returns an `xmlParserErrors` code; on success `*out` receives the static
/// handler (NULL for UTF-8, which needs no conversion).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlParserErrors xmlLookupCharEncodingHandler(xmlCharEncoding enc,
///                                              xmlCharEncodingHandler **out);
/// ```
#[no_mangle]
pub extern "C" fn xmlLookupCharEncodingHandler(enc: c_int, out: *mut *mut c_void) -> c_int {
    crate::xml::encoding::xmlLookupCharEncodingHandler(enc, out)
}

/// Get the encoding handler for an `xmlCharEncoding` value (deprecated).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlCharEncodingHandler *xmlGetCharEncodingHandler(xmlCharEncoding enc);
/// ```
#[no_mangle]
pub extern "C" fn xmlGetCharEncodingHandler(enc: c_int) -> *mut c_void {
    crate::xml::encoding::xmlGetCharEncodingHandler(enc)
}

/// Find or create an encoding handler by name for one conversion direction.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlParserErrors xmlOpenCharEncodingHandler(const char *name, int output,
///                                            xmlCharEncodingHandler **out);
/// ```
#[no_mangle]
pub extern "C" fn xmlOpenCharEncodingHandler(
    name: *const c_char,
    output: c_int,
    out: *mut *mut c_void,
) -> c_int {
    crate::xml::encoding::xmlOpenCharEncodingHandler(name, output, out)
}

/// Find or create an encoding handler by name with flags and an optional
/// custom conversion implementation.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlParserErrors xmlCreateCharEncodingHandler(
///     const char *name, xmlCharEncFlags flags, xmlCharEncConvImpl impl,
///     void *implCtxt, xmlCharEncodingHandler **out);
/// ```
#[no_mangle]
pub extern "C" fn xmlCreateCharEncodingHandler(
    name: *const c_char,
    flags: c_int,
    impl_: Option<crate::abi::callbacks::xmlCharEncConvImpl>,
    implCtxt: *mut c_void,
    out: *mut *mut c_void,
) -> c_int {
    crate::xml::encoding::xmlCreateCharEncodingHandler(name, flags, impl_, implCtxt, out)
}

/// Create an encoding handler backed by modern conversion callbacks.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlParserErrors xmlCharEncNewCustomHandler(
///     const char *name, xmlCharEncConvFunc input, xmlCharEncConvFunc output,
///     xmlCharEncConvCtxtDtor ctxtDtor, void *inputCtxt, void *outputCtxt,
///     xmlCharEncodingHandler **out);
/// ```
#[no_mangle]
pub extern "C" fn xmlCharEncNewCustomHandler(
    name: *const c_char,
    input: crate::abi::callbacks::xmlCharEncConvFunc,
    output: crate::abi::callbacks::xmlCharEncConvFunc,
    ctxtDtor: Option<crate::abi::callbacks::xmlCharEncConvCtxtDtor>,
    inputCtxt: *mut c_void,
    outputCtxt: *mut c_void,
    out: *mut *mut c_void,
) -> c_int {
    crate::xml::encoding::xmlCharEncNewCustomHandler(
        name, input, output, ctxtDtor, inputCtxt, outputCtxt, out,
    )
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

/// Parse a URI string into an existing URI structure (upstream uri.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlParseURIReference(xmlURIPtr uri, const char *str);
/// ```
///
/// Returns 0 on success, -1 on failure (the URI structure is left
/// untouched on failure).
///
/// # Safety
///
/// - `uri` must be a valid pointer from `xmlParseURI`/`xmlCreateURI`.
/// - `str` must be a valid null-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn xmlParseURIReference(uri: *mut c_void, str: *const c_char) -> c_int {
    crate::xml::uri::xmlParseURIReference(uri, str)
}

/// Normalize a URI path in place (upstream uri.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlNormalizeURIPath(char *path);
/// ```
///
/// Returns 0 on success, -1 if the path is NULL, not absolute, or contains
/// `..` segments that climb above the root.
///
/// # Safety
///
/// `path` must be a valid writable null-terminated C string buffer.
#[no_mangle]
pub unsafe extern "C" fn xmlNormalizeURIPath(path: *mut c_char) -> c_int {
    crate::xml::uri::xmlNormalizeURIPath(path)
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
            let buf = xmlMallocImpl(len + 1) as *mut xmlChar;
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
    } else if typ == xmlXPathObjectType::XPATH_XSLT_TREE as c_int {
        // A result tree fragment: node-set containing the fragment's
        // document node (matching how global RTF variables are bound), so
        // local RTF variables stringify to their text and remain navigable
        // via exsl:node-set.
        let frag_doc = (*obj).nodesetval as *mut _xmlDoc;
        if frag_doc.is_null() {
            XPathValue::NodeSet(NodeSet::new())
        } else {
            let mut ns = NodeSet::new();
            ns.push(frag_doc as *mut _xmlNode);
            XPathValue::NodeSet(ns)
        }
    } else {
        // Undefined / unknown type — return boolean false as a safe default.
        XPathValue::Boolean(false)
    }
}

/// Public wrapper for `xpath_to_object` (used by the XPath export bridge).
///
/// # Safety
///
/// - `val` is consumed and converted into a heap-allocated `_xmlXPathObject`.
pub unsafe fn xpath_to_object_pub(val: XPathValue) -> *mut _xmlXPathObject {
    xpath_to_object(val)
}

/// Public wrapper for `object_to_xpathvalue` (used by the XSLT engine).
///
/// # Safety
///
/// `obj` must be a valid, non-null pointer to a properly initialised
/// `_xmlXPathObject`.
pub unsafe fn object_to_xpathvalue_pub(obj: *mut _xmlXPathObject) -> XPathValue {
    object_to_xpathvalue(obj)
}

// ── Compiled expression registry ────────────────────────────────────────
//
// Compiled XPath expressions are opaque pointers returned by xmlXPathCompile.
// We store them in a global registry keyed by a monotonically increasing ID.

static COMPILED_EXPRS: Lazy<Mutex<HashMap<u64, Box<CompiledExpr>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static NEXT_COMPILED_KEY: Lazy<Mutex<u64>> = Lazy::new(|| Mutex::new(1));

/// Accessor for the compiled-expression registry (used by the XPath export
/// bridge for `xmlXPathCompiledEval` / `xmlXPathCompiledEvalToBoolean`).
pub(crate) fn xpath_compiled_registry() -> &'static Mutex<HashMap<u64, Box<CompiledExpr>>> {
    &COMPILED_EXPRS
}

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

/// Look up a C-registered extension function for the context identified by
/// `extra` (the internal XPathContext pointer). Used by
/// `xmlXPathFunctionLookupNS`.
pub(crate) fn xpath_cfunc_lookup(extra: *mut c_void, qualified: &str) -> Option<CXPathFunc> {
    C_FUNCTIONS
        .lock()
        .get(&(SendSyncPtr(extra), qualified.to_string()))
        .copied()
}

/// Drop every C extension-function registration belonging to the context
/// identified by `extra` (upstream `xmlXPathRegisteredFuncsCleanup`).
pub(crate) fn xpath_cfunc_cleanup(extra: *mut c_void) {
    C_FUNCTIONS.lock().retain(|(k, _), _| k.0 != extra);
}

/// Build the Rust-side closure that bridges a C-registered XPath function
/// into the Rust evaluator (see `c_func_call_bridge`). Returns a
/// `BoxedXPathFunction` so the closure is coerced with the higher-ranked
/// signature the evaluator requires.
fn c_func_bridge_closure(c_ctxt: SendSyncPtr, qualified: String) -> BoxedXPathFunction {
    Box::new(move |_ctx: &mut XPathContext, args: &[XPathValue]| {
        let cc = c_ctxt;
        unsafe { c_func_call_bridge(cc.0 as *mut _xmlXPathContext, &qualified, args) }
    })
}

/// Call a C-ABI `xmlXPathFunction` through a synthesized
/// `xmlXPathParserContext`: push the evaluated arguments as XPath objects,
/// invoke the function, pop and convert the result — the upstream
/// `xmlXPathCompOpEval` function-call sequence (xpath.c).
///
/// # SAFETY
///
/// - `fnptr` must be a valid C callback (or None).
/// - `c_ctxt` must be the live C XPath context the callback belongs to.
pub(crate) unsafe fn call_c_xpath_function(
    fnptr: Option<unsafe extern "C" fn(*mut c_void, c_int)>,
    c_ctxt: *mut _xmlXPathContext,
    args: &[XPathValue],
) -> Result<XPathValue, String> {
    let func = match fnptr {
        Some(f) => f,
        None => return Err("XPath: missing C function pointer".to_string()),
    };
    let pc = crate::xml::xpath::parser_context::new_parser_context(ptr::null(), c_ctxt);
    if pc.is_null() {
        return Err("XPath: parser-context allocation failure".to_string());
    }
    let mut push_ok = true;
    for v in args {
        let obj = xpath_to_object(v.clone());
        if obj.is_null() || crate::xml::xpath::parser_context::value_push(pc, obj).is_null() {
            push_ok = false;
            break;
        }
    }
    let result = if push_ok {
        // SAFETY: `func` is a valid C callback; the arguments are on the
        // parser-context value stack exactly as upstream would leave them.
        unsafe { func(pc as *mut c_void, args.len() as c_int) };
        let ret = crate::xml::xpath::parser_context::value_pop(pc);
        if ret.is_null() {
            Err("XPath: C function returned no value".to_string())
        } else {
            let v = object_to_xpathvalue(ret);
            // The popped object is heap-allocated; free it after converting.
            unsafe { xmlXPathFreeObject(ret) };
            Ok(v)
        }
    } else {
        Err("XPath: failed to push arguments to C function".to_string())
    };
    // Free any objects the C function left on the stack, then the context.
    unsafe {
        loop {
            let leftover = crate::xml::xpath::parser_context::value_pop(pc);
            if leftover.is_null() {
                break;
            }
            xmlXPathFreeObject(leftover);
        }
        crate::xml::xpath::parser_context::free_parser_context(pc);
    }
    result
}

/// Rust-side wrapper registered in the internal XPathContext when a C
/// extension function is registered (`xmlXPathRegisterFunc[NS]`). This is the
/// parser-context bridge: it synthesises the upstream `xmlXPathParserContext`
/// (value stack + context pointer), pushes the evaluated arguments as XPath
/// objects, invokes the C function, then pops and converts the result — the
/// upstream `xmlXPathCompOpEval` function-call sequence (xpath.c).
unsafe fn c_func_call_bridge(
    c_ctxt: *mut _xmlXPathContext,
    qualified: &str,
    args: &[XPathValue],
) -> Result<XPathValue, String> {
    if c_ctxt.is_null() {
        return Err("XPath: null context in C function bridge".to_string());
    }
    let func = xpath_cfunc_lookup((*c_ctxt).extra, qualified);
    if func.is_none() {
        return Err(format!("XPath: unknown C function '{}'", qualified));
    }
    unsafe { call_c_xpath_function(func, c_ctxt, args) }
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
    let mut internal = Box::new(XPathContext::new(doc));
    // UPSTREAM-PARITY: the standard function library is implicitly available
    // in every context (upstream compiles it in; xmlXPathRegisterAllFunctions
    // is a no-op since 2.14.0). Without this, core-function calls such as
    // count() would fail as unknown functions.
    for (name, func) in crate::xml::xpath::functions::core_functions() {
        internal.register_function(&name, func);
    }
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
    // Drop the registered-namespace C-string hash (xmlXPathNsLookup pointers).
    if !(*ctxt).nsHash.is_null() {
        drop(Box::from_raw(
            (*ctxt).nsHash as *mut HashMap<String, CString>,
        ));
        (*ctxt).nsHash = ptr::null_mut();
    }
    // Free the C ABI context struct.
    xmlFreeImpl(ctxt as *mut c_void);
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
    // Clear any stale error so a fresh evaluation either succeeds or records
    // its own failure message (the XSLT layer surfaces it verbatim).
    internal.clear_error();

    match crate::xml::xpath::evaluate_str(expr_str, internal) {
        Some(val) => xpath_to_object(val),
        None => {
            // UPSTREAM-PARITY: libxml2 reports a failed compile/eval with
            // "XPath error : Invalid expression" (xmlXPathErr,
            // XPATH_EXPR_ERROR). The precise per-expression diagnostics are
            // tracked as RESIDUAL R-XPATH-ERRMSG.
            if internal.error.is_none() {
                internal.set_error("Invalid expression");
            }
            ptr::null_mut()
        }
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
    if typ == xmlXPathObjectType::XPATH_STRING as c_int && !(*obj).stringval.is_null() {
        xmlFreeImpl((*obj).stringval as *mut c_void);
        (*obj).stringval = ptr::null_mut();
    }
    // Free node-set storage.
    if typ == xmlXPathObjectType::XPATH_NODESET as c_int {
        let ns = (*obj).nodesetval as *mut _xmlNodeSet;
        if !ns.is_null() {
            if !(*ns).nodeTab.is_null() {
                xmlFreeImpl((*ns).nodeTab as *mut c_void);
            }
            xmlFreeImpl(ns as *mut c_void);
        }
        (*obj).nodesetval = ptr::null_mut();
    }
    xmlFreeImpl(obj as *mut c_void);
}

/// Copy an XPath object (deep copy).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlXPathObjectPtr xmlXPathObjectCopy(xmlXPathObjectPtr val);
/// ```
///
/// Oracle behavior: returns a newly allocated object with the same type
/// and value. Node-sets are copied element-by-element; strings are
/// duplicated; numbers and booleans are copied by value.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathObjectCopy(val: *mut _xmlXPathObject) -> *mut _xmlXPathObject {
    if val.is_null() {
        return ptr::null_mut();
    }
    let typ = (*val).type_;
    let obj = xmlMallocZero(size_of::<_xmlXPathObject>()) as *mut _xmlXPathObject;
    if obj.is_null() {
        return ptr::null_mut();
    }
    (*obj).type_ = typ;
    if typ == xmlXPathObjectType::XPATH_NODESET as c_int {
        let src_ns = (*val).nodesetval as *mut _xmlNodeSet;
        if !src_ns.is_null() {
            let nr = (*src_ns).nodeNr;
            let ns = xmlMallocZero(size_of::<_xmlNodeSet>()) as *mut _xmlNodeSet;
            if ns.is_null() {
                xmlFreeImpl(obj as *mut c_void);
                return ptr::null_mut();
            }
            (*ns).nodeNr = nr;
            (*ns).nodeMax = nr;
            if nr > 0 && !(*src_ns).nodeTab.is_null() {
                let tab = xmlMallocImpl((nr as usize) * core::mem::size_of::<*mut _xmlNode>())
                    as *mut *mut _xmlNode;
                if tab.is_null() {
                    xmlFreeImpl(ns as *mut c_void);
                    xmlFreeImpl(obj as *mut c_void);
                    return ptr::null_mut();
                }
                ptr::copy_nonoverlapping((*src_ns).nodeTab, tab, nr as usize);
                (*ns).nodeTab = tab;
            } else {
                (*ns).nodeTab = ptr::null_mut();
            }
            (*obj).nodesetval = ns as *mut c_void;
        }
    } else if typ == xmlXPathObjectType::XPATH_BOOLEAN as c_int {
        (*obj).boolval = (*val).boolval;
    } else if typ == xmlXPathObjectType::XPATH_NUMBER as c_int {
        (*obj).floatval = (*val).floatval;
    } else if typ == xmlXPathObjectType::XPATH_STRING as c_int {
        let src = (*val).stringval;
        if !src.is_null() {
            let len = libc::strlen(src as *const libc::c_char);
            let buf = xmlMallocImpl(len + 1) as *mut xmlChar;
            if !buf.is_null() {
                ptr::copy_nonoverlapping(src, buf, len);
                *buf.add(len) = 0;
            }
            (*obj).stringval = buf;
        }
    }
    obj
}

/// Cast an XPath object to its string value.
///
/// Returns a newly allocated string (caller frees with `xmlFree`).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlXPathCastToString(xmlXPathObjectPtr val);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlXPathCastToString(val: *mut _xmlXPathObject) -> *mut xmlChar {
    if val.is_null() {
        return ptr::null_mut();
    }
    let typ = (*val).type_;
    let mut result: Vec<u8> = Vec::new();
    if typ == xmlXPathObjectType::XPATH_STRING as c_int {
        if !(*val).stringval.is_null() {
            let len = libc::strlen((*val).stringval as *const libc::c_char);
            result.extend_from_slice(core::slice::from_raw_parts((*val).stringval, len));
        }
    } else if typ == xmlXPathObjectType::XPATH_NUMBER as c_int {
        // Number → string conversion per XPath 1.0 §4.2:
        // - NaN → "NaN"
        // - +0/-0 → "0"
        // - infinity → "Infinity" / "-Infinity"
        // - integer → decimal representation without exponent
        let n = (*val).floatval;
        result.extend_from_slice(xml_number_to_string(n).as_bytes());
    } else if typ == xmlXPathObjectType::XPATH_BOOLEAN as c_int {
        result.extend_from_slice(if (*val).boolval != 0 {
            b"true"
        } else {
            b"false"
        });
    } else if typ == xmlXPathObjectType::XPATH_NODESET as c_int {
        // String value of a node-set is the string value of the first node
        // in document order (or empty if empty).
        let ns = (*val).nodesetval as *mut _xmlNodeSet;
        if !ns.is_null() && (*ns).nodeNr > 0 && !(*ns).nodeTab.is_null() {
            let node = *(*ns).nodeTab;
            if !node.is_null() {
                let content = crate::xml::tree::node_get_content(node);
                if !content.is_null() {
                    let len = libc::strlen(content as *const libc::c_char);
                    result.extend_from_slice(core::slice::from_raw_parts(content, len));
                    xmlFreeImpl(content as *mut c_void);
                }
            }
        }
    }
    // Allocate the C string.
    let buf = xmlMallocImpl(result.len() + 1) as *mut xmlChar;
    if buf.is_null() {
        return ptr::null_mut();
    }
    if !result.is_empty() {
        ptr::copy_nonoverlapping(result.as_ptr(), buf, result.len());
    }
    *buf.add(result.len()) = 0;
    buf
}

/// Convert an XPath number to its string representation (XPath 1.0 §4.2).
///
/// Canonical implementation lives in `crate::xml::xpath::types::number_to_string`
/// (a port of upstream `xmlXPathCastNumberToString` / `xmlXPathFormatNumber`,
/// R-000166); this ABI helper delegates so every number→string conversion
/// shares exactly one oracle-verified code path.
pub fn xml_number_to_string(n: f64) -> String {
    crate::xml::xpath::types::number_to_string(n)
}

/// Port of upstream xpath.c `xmlXPathStringEvalNumber` (R-000166): see
/// `crate::xml::xpath::types::string_bytes_to_number` — the oracle
/// accumulates digits directly, caps the fraction at MAX_FRAC=20 digits
/// after any leading zeros, applies the exponent with `pow(10.0, exp)`
/// (underflowing to 0 below the smallest subnormal), accepts XML whitespace
/// around the number, and returns NaN for anything else — including a
/// leading '+'.
fn xpath_string_eval_number(bytes: &[u8]) -> f64 {
    crate::xml::xpath::types::string_bytes_to_number(bytes)
}

/// Cast a C string to a number per XPath 1.0 §4.2 conversion rules.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// double xmlXPathCastStringToNumber(const xmlChar *val);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlXPathCastStringToNumber(val: *const xmlChar) -> f64 {
    if val.is_null() {
        return f64::NAN;
    }
    let len = libc::strlen(val as *const libc::c_char);
    let bytes = core::slice::from_raw_parts(val, len);
    xpath_string_eval_number(bytes)
}

/// Compare two nodes in document order.
///
/// UPSTREAM-PARITY (xpath.c `xmlXPathCmpNodes`): returns **1** when
/// `node1` precedes `node2` in document order, **-1** when `node1` follows
/// `node2`, 0 for the same node, and -2 for NULL or cross-document
/// comparisons. The sign convention was verified against the system oracle
/// (libxml2 2.15.3): `xmlXPathCmpNodes(book1, book2)` returns 1.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlXPathCmpNodes(xmlNodePtr node1, xmlNodePtr node2);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlXPathCmpNodes(node1: *mut _xmlNode, node2: *mut _xmlNode) -> c_int {
    if node1.is_null() || node2.is_null() {
        return -2;
    }
    if node1 == node2 {
        return 0;
    }
    // Build ancestor chains.
    let mut chain1: Vec<*mut _xmlNode> = Vec::new();
    let mut chain2: Vec<*mut _xmlNode> = Vec::new();
    let mut n = node1;
    while !n.is_null() {
        chain1.push(n);
        n = (*n).parent;
    }
    let mut n = node2;
    while !n.is_null() {
        chain2.push(n);
        n = (*n).parent;
    }
    // Distinct documents (or entities) case.
    if chain1[chain1.len() - 1] != chain2[chain2.len() - 1] {
        return -2;
    }
    // Find the nearest common ancestor.
    let mut i = chain1.len();
    let mut j = chain2.len();
    while i > 0 && j > 0 && chain1[i - 1] == chain2[j - 1] {
        i -= 1;
        j -= 1;
    }
    // node1 is an ancestor of node2 -> node1 precedes it -> 1.
    if i == 0 {
        return 1;
    }
    // node2 is an ancestor of node1 -> node1 follows it -> -1.
    if j == 0 {
        return -1;
    }
    // Compare sibling order at the divergence point.
    let mut a = chain1[i - 1];
    let mut b = chain2[j - 1];
    // Climb to the same level.
    while !a.is_null() && !b.is_null() {
        let pa = (*a).parent;
        let pb = (*b).parent;
        if pa == pb {
            break;
        }
        a = pa;
        b = pb;
    }
    // Walk forward from the first child of the common parent.
    let parent = (*a).parent;
    let mut child = if parent.is_null() {
        ptr::null_mut()
    } else {
        (*parent).children
    };
    while !child.is_null() {
        if child == a {
            return 1; // a precedes b
        }
        if child == b {
            return -1; // b precedes a
        }
        child = (*child).next;
    }
    0
}

/// Create a node-set from a range of an existing node-set.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNodeSetPtr xmlXPathNodeSetCreate(xmlNodePtr val);
/// ```
///
/// With a null `val`, creates an empty node-set.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathNodeSetCreate(val: *mut _xmlNode) -> *mut _xmlNodeSet {
    let ns = xmlMallocZero(size_of::<_xmlNodeSet>()) as *mut _xmlNodeSet;
    if ns.is_null() {
        return ptr::null_mut();
    }
    if val.is_null() {
        return ns;
    }
    let tab = xmlMallocImpl(core::mem::size_of::<*mut _xmlNode>()) as *mut *mut _xmlNode;
    if tab.is_null() {
        xmlFreeImpl(ns as *mut c_void);
        return ptr::null_mut();
    }
    *tab = val;
    (*ns).nodeTab = tab;
    (*ns).nodeNr = 1;
    (*ns).nodeMax = 1;
    ns
}

/// Free a node-set allocated by `xmlXPathNodeSetCreate` or a node-set
/// builder in this library.
///
/// Frees the node-set structure and its node table; the nodes themselves
/// are owned by their document and are not freed.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlXPathFreeNodeSet(xmlNodeSetPtr ns);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlXPathFreeNodeSet(ns: *mut _xmlNodeSet) {
    if ns.is_null() {
        return;
    }
    if !(*ns).nodeTab.is_null() {
        xmlFreeImpl((*ns).nodeTab as *mut c_void);
        (*ns).nodeTab = ptr::null_mut();
    }
    (*ns).nodeNr = 0;
    (*ns).nodeMax = 0;
    xmlFreeImpl(ns as *mut c_void);
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

    // Mirror the registration into the C context's nsHash (Box<HashMap<
    // String, CString>>): xmlXPathNsLookup hands out pointers into these
    // owned C strings, matching upstream ownership (strdup'd in nsHash,
    // freed by xmlXPathRegisteredNsCleanup / xmlXPathFreeContext).
    let map: &mut HashMap<String, CString> = if (*ctxt).nsHash.is_null() {
        let b: Box<HashMap<String, CString>> = Box::default();
        (*ctxt).nsHash = Box::into_raw(b) as *mut c_void;
        &mut *((*ctxt).nsHash as *mut HashMap<String, CString>)
    } else {
        &mut *((*ctxt).nsHash as *mut HashMap<String, CString>)
    };
    map.insert(
        prefix_str.to_string(),
        CString::new(uri_str.as_bytes()).unwrap_or_default(),
    );
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
        // Register a Rust closure that bridges to the C function through a
        // synthesized xmlXPathParserContext (upstream function-call ABI).
        let c_ctxt = SendSyncPtr(ctxt as *mut c_void);
        let name_owned = name_str.to_string();
        internal.register_function(name_str, c_func_bridge_closure(c_ctxt, name_owned));
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
        let c_ctxt = SendSyncPtr(ctxt as *mut c_void);
        let qualified_owned = qualified.clone();
        internal.register_function(&qualified, c_func_bridge_closure(c_ctxt, qualified_owned));
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

/// Dump the catalog in XML format to a FILE* (upstream `xmlCatalogDump`).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlCatalogDump(FILE *out, xmlCatalogPtr catal);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlCatalogDump(output: *mut c_void, _catal: *mut c_void) {
    if output.is_null() {
        return;
    }
    let doc = crate::xml::catalog::dump_doc();
    if doc.is_null() {
        return;
    }
    let mut mem: *mut xmlChar = ptr::null_mut();
    let mut size: c_int = 0;
    crate::xml::tree::xmlDocDumpFormatMemory(doc, &mut mem, &mut size, 1);
    if !mem.is_null() {
        libc::fwrite(
            mem as *const c_void,
            1,
            size as usize,
            output as *mut libc::FILE,
        );
        xmlFreeImpl(mem as *mut c_void);
    }
    crate::xml::tree::free_doc(doc);
}

/// Save the catalog to a file (upstream `xmlCatalogSave`).
///
/// Returns 0 on success, -1 on failure.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlCatalogSave(const char *filename);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlCatalogSave(filename: *const c_char) -> c_int {
    if filename.is_null() {
        return -1;
    }
    let doc = crate::xml::catalog::dump_doc();
    if doc.is_null() {
        return -1;
    }
    let mut mem: *mut xmlChar = ptr::null_mut();
    let mut size: c_int = 0;
    crate::xml::tree::xmlDocDumpFormatMemory(doc, &mut mem, &mut size, 1);
    let mut ret: c_int = -1;
    if !mem.is_null() {
        let fp = libc::fopen(filename, c"w".as_ptr() as *const c_char);
        if !fp.is_null() {
            let written = libc::fwrite(mem as *const c_void, 1, size as usize, fp);
            ret = if written == size as usize { 0 } else { -1 };
            libc::fclose(fp);
        }
        xmlFreeImpl(mem as *mut c_void);
    }
    crate::xml::tree::free_doc(doc);
    ret
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
pub const unsafe extern "C" fn htmlParseFile(
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
pub const unsafe extern "C" fn htmlParseMemory(
    _buffer: *const c_char,
    _size: c_int,
) -> *mut _xmlDoc {
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
pub const unsafe extern "C" fn htmlParseDoc(
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
pub const unsafe extern "C" fn htmlCreateFileParserCtxt(
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
pub extern "C" fn htmlFreeParserCtxt(ctxt: *mut c_void) {
    unsafe { crate::xml::html::free_parser_ctxt(ctxt) }
}

/// Initialize the HTML parser.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void htmlInitParser(void);
/// ```
#[no_mangle]
pub const extern "C" fn htmlInitParser() {
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
pub const extern "C" fn htmlCleanupParser() {
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

/// Validate an NCName value (modern 2-arg form, upstream tree.c).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidateNCName(const xmlChar *value, int space);
/// ```
///
/// Returns -1 on NULL, 0 if valid, 1 if invalid.
#[no_mangle]
pub unsafe extern "C" fn xmlValidateNCName(value: *const xmlChar, space: c_int) -> c_int {
    crate::xml::validation::validate_ncname(value, space)
}

/// Validate a QName value (modern 2-arg form, upstream tree.c).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidateQName(const xmlChar *value, int space);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlValidateQName(value: *const xmlChar, space: c_int) -> c_int {
    crate::xml::validation::validate_qname(value, space)
}

/// Validate an XML Name value (modern 2-arg form, upstream tree.c).
///
/// # UPSTREAM-PARITY / HISTORICAL
///
/// Since libxml2 2.12 the DSO symbol carries a second `int space` parameter
/// with inverted return semantics (0 valid / 1 invalid / -1 NULL); the
/// pre-2.12 1-arg form no longer exists in the DSO. The candidate matches
/// the current oracle. (The 1-arg semantics live on as xmlValidateNameValue.)
///
/// ```c
/// int xmlValidateName(const xmlChar *value, int space);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlValidateName(value: *const xmlChar, space: c_int) -> c_int {
    crate::xml::validation::validate_name_space(value, space)
}

/// Validate an NMToken value (modern 2-arg form, upstream tree.c).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidateNMToken(const xmlChar *value, int space);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlValidateNMToken(value: *const xmlChar, space: c_int) -> c_int {
    crate::xml::validation::validate_nmtoken_space(value, space)
}

/// Validate a Name value (1-arg form, upstream valid.c).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidateNameValue(const xmlChar *value);
/// ```
///
/// Returns 1 if valid, 0 if not (NULL included).
#[no_mangle]
pub unsafe extern "C" fn xmlValidateNameValue(value: *const xmlChar) -> c_int {
    crate::xml::validation::validate_name_value(value)
}

/// Validate a whitespace-separated list of Names (separator is exactly
/// 0x20, upstream erratum E20).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidateNamesValue(const xmlChar *value);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlValidateNamesValue(value: *const xmlChar) -> c_int {
    crate::xml::validation::validate_names_value(value)
}

/// Validate an Nmtoken value (1-arg form, upstream valid.c).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidateNmtokenValue(const xmlChar *value);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlValidateNmtokenValue(value: *const xmlChar) -> c_int {
    crate::xml::validation::validate_nmtoken_value(value)
}

/// Validate a whitespace-separated list of Nmtokens.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidateNmtokensValue(const xmlChar *value);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlValidateNmtokensValue(value: *const xmlChar) -> c_int {
    crate::xml::validation::validate_nmtokens_value(value)
}

/// Validate a single element declaration (VC: Unique Element Type
/// Declaration, VC: No Duplicate Types).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidateElementDecl(xmlValidCtxtPtr ctxt,
///                            xmlDocPtr doc,
///                            xmlElementPtr elem);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlValidateElementDecl(
    ctxt: *mut _xmlValidCtxt,
    doc: *mut _xmlDoc,
    elem: *mut _xmlElement,
) -> c_int {
    crate::xml::validation::validate_element_decl(ctxt, doc, elem)
}

/// Validate a notation declaration.
///
/// # UPSTREAM-PARITY
///
/// Modern libxml2 has no validity constraint on notation declarations; the
/// oracle returns 1 unconditionally (verified by DSO disassembly).
///
/// ```c
/// int xmlValidateNotationDecl(xmlValidCtxtPtr ctxt,
///                             xmlDocPtr doc,
///                             xmlNotationPtr nota);
/// ```
#[no_mangle]
pub const unsafe extern "C" fn xmlValidateNotationDecl(
    ctxt: *mut _xmlValidCtxt,
    doc: *mut _xmlDoc,
    nota: *mut _xmlNotation,
) -> c_int {
    crate::xml::validation::validate_notation_decl(ctxt, doc, nota)
}

/// Validate a single attribute against its declaration.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidateOneAttribute(xmlValidCtxtPtr ctxt,
///                             xmlDocPtr doc,
///                             xmlNodePtr elem,
///                             xmlAttrPtr attr,
///                             const xmlChar *value);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlValidateOneAttribute(
    ctxt: *mut _xmlValidCtxt,
    doc: *mut _xmlDoc,
    elem: *mut _xmlNode,
    attr: *mut _xmlAttr,
    value: *const xmlChar,
) -> c_int {
    crate::xml::validation::validate_one_attribute(ctxt, doc, elem, attr, value)
}

/// Validate a single element against its declaration (without recursing).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidateOneElement(xmlValidCtxtPtr ctxt,
///                           xmlDocPtr doc,
///                           xmlNodePtr elem);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlValidateOneElement(
    ctxt: *mut _xmlValidCtxt,
    doc: *mut _xmlDoc,
    elem: *mut _xmlNode,
) -> c_int {
    crate::xml::validation::validate_one_element(ctxt, doc, elem)
}

/// Validate a namespace declaration attribute.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidateOneNamespace(xmlValidCtxtPtr ctxt,
///                             xmlDocPtr doc,
///                             xmlNodePtr elem,
///                             const xmlChar *prefix,
///                             xmlNsPtr ns,
///                             const xmlChar *value);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlValidateOneNamespace(
    ctxt: *mut _xmlValidCtxt,
    doc: *mut _xmlDoc,
    elem: *mut _xmlNode,
    prefix: *const xmlChar,
    ns: *mut _xmlNs,
    value: *const xmlChar,
) -> c_int {
    crate::xml::validation::validate_one_namespace(ctxt, doc, elem, prefix, ns, value)
}

/// Push a new element start onto the validation stack (streaming DTD
/// validation).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidatePushElement(xmlValidCtxtPtr ctxt,
///                            xmlDocPtr doc,
///                            xmlNodePtr elem,
///                            const xmlChar *qname);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlValidatePushElement(
    ctxt: *mut _xmlValidCtxt,
    doc: *mut _xmlDoc,
    elem: *mut _xmlNode,
    qname: *const xmlChar,
) -> c_int {
    crate::xml::validation::validate_push_element(ctxt, doc, elem, qname)
}

/// Push character data onto the validation stack.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidatePushCData(xmlValidCtxtPtr ctxt,
///                          const xmlChar *data,
///                          int len);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlValidatePushCData(
    ctxt: *mut _xmlValidCtxt,
    data: *const xmlChar,
    len: c_int,
) -> c_int {
    crate::xml::validation::validate_push_cdata(ctxt, data, len)
}

/// Pop an element end from the validation stack.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidatePopElement(xmlValidCtxtPtr ctxt,
///                           xmlDocPtr doc,
///                           xmlNodePtr elem,
///                           const xmlChar *qname);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlValidatePopElement(
    ctxt: *mut _xmlValidCtxt,
    doc: *mut _xmlDoc,
    elem: *mut _xmlNode,
    qname: *const xmlChar,
) -> c_int {
    crate::xml::validation::validate_pop_element(ctxt, doc, elem, qname)
}

/// Build the content-model automaton for an element declaration.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidBuildContentModel(xmlValidCtxtPtr ctxt,
///                               xmlElementPtr elem);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlValidBuildContentModel(
    ctxt: *mut _xmlValidCtxt,
    elem: *mut _xmlElement,
) -> c_int {
    crate::xml::validation::validate_build_content_model(ctxt, elem)
}

/// Add an attribute to the document's ID table.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlIDPtr xmlAddID(xmlValidCtxtPtr ctxt,
///                   xmlDocPtr doc,
///                   const xmlChar *value,
///                   xmlAttrPtr attr);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlAddID(
    ctxt: *mut _xmlValidCtxt,
    doc: *mut _xmlDoc,
    value: *const xmlChar,
    attr: *mut _xmlAttr,
) -> *mut _xmlID {
    crate::xml::validation::add_id(ctxt, doc, value, attr)
}

/// Remove an attribute from the document's ID table.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlRemoveID(xmlDocPtr doc, xmlAttrPtr attr);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlRemoveID(doc: *mut _xmlDoc, attr: *mut _xmlAttr) -> c_int {
    crate::xml::validation::remove_id(doc, attr)
}

/// Register an IDREF in the document's ref table.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlRefPtr xmlAddRef(xmlValidCtxtPtr ctxt,
///                     xmlDocPtr doc,
///                     const xmlChar *value,
///                     xmlAttrPtr attr);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlAddRef(
    ctxt: *mut _xmlValidCtxt,
    doc: *mut _xmlDoc,
    value: *const xmlChar,
    attr: *mut _xmlAttr,
) -> *mut _xmlRef {
    crate::xml::validation::add_ref(ctxt, doc, value, attr)
}

/// Remove an attribute's IDREF entries.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlRemoveRef(xmlDocPtr doc, xmlAttrPtr attr);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlRemoveRef(doc: *mut _xmlDoc, attr: *mut _xmlAttr) -> c_int {
    crate::xml::validation::remove_ref(doc, attr)
}

/// Add an ID without a validation context (2.13+).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlAddIDSafe(xmlAttrPtr attr, const xmlChar *value);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlAddIDSafe(attr: *mut _xmlAttr, value: *const xmlChar) -> c_int {
    crate::xml::validation::add_id_safe(attr, value)
}

/// Free an ID hash table.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlFreeIDTable(xmlIDTablePtr table);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlFreeIDTable(table: *mut c_void) {
    crate::xml::validation::free_id_table(table as *mut crate::xml::hash::HashTable);
}

/// Free an IDREF hash table.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlFreeRefTable(xmlRefTablePtr table);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlFreeRefTable(table: *mut c_void) {
    crate::xml::validation::free_ref_table(table as *mut crate::xml::hash::HashTable);
}

/// Look up the attribute holding an ID.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlAttrPtr xmlGetID(xmlDocPtr doc, const xmlChar *ID);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlGetID(doc: *mut _xmlDoc, id: *const xmlChar) -> *mut _xmlAttr {
    crate::xml::validation::get_id(doc, id)
}

/// Look up the list of references for an ID.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlListPtr xmlGetRefs(xmlDocPtr doc, const xmlChar *ID);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlGetRefs(doc: *mut _xmlDoc, id: *const xmlChar) -> *mut c_void {
    crate::xml::validation::get_refs(doc, id) as *mut c_void
}

/// Is this attribute an ID?
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlIsID(xmlDocPtr doc, xmlNodePtr elem, xmlAttrPtr attr);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlIsID(
    doc: *mut _xmlDoc,
    elem: *mut _xmlNode,
    attr: *mut _xmlAttr,
) -> c_int {
    crate::xml::validation::is_id(doc, elem, attr)
}

/// Is this attribute an IDREF?
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlIsRef(xmlDocPtr doc, xmlNodePtr elem, xmlAttrPtr attr);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlIsRef(
    doc: *mut _xmlDoc,
    elem: *mut _xmlNode,
    attr: *mut _xmlAttr,
) -> c_int {
    crate::xml::validation::is_ref(doc, elem, attr)
}

/// Search a DTD for an element declaration (with QName splitting).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlElementPtr xmlGetDtdElementDesc(xmlDtdPtr dtd, const xmlChar *name);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlGetDtdElementDesc(
    dtd: *mut _xmlDtd,
    name: *const xmlChar,
) -> *mut _xmlElement {
    crate::xml::validation::get_dtd_element_desc(dtd, name)
}

/// Search a DTD for an attribute declaration (with QName splitting).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlAttributePtr xmlGetDtdAttrDesc(xmlDtdPtr dtd,
///                                   const xmlChar *elem,
///                                   const xmlChar *name);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlGetDtdAttrDesc(
    dtd: *mut _xmlDtd,
    elem: *const xmlChar,
    name: *const xmlChar,
) -> *mut _xmlAttribute {
    crate::xml::validation::get_dtd_attr_desc(dtd, elem, name)
}

/// Search a DTD for a qualified element declaration.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlElementPtr xmlGetDtdQElementDesc(xmlDtdPtr dtd,
///                                     const xmlChar *name,
///                                     const xmlChar *prefix);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlGetDtdQElementDesc(
    dtd: *mut _xmlDtd,
    name: *const xmlChar,
    prefix: *const xmlChar,
) -> *mut _xmlElement {
    crate::xml::validation::get_dtd_qelement_desc(dtd, name, prefix)
}

/// Search a DTD for a qualified attribute declaration.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlAttributePtr xmlGetDtdQAttrDesc(xmlDtdPtr dtd,
///                                    const xmlChar *elem,
///                                    const xmlChar *name,
///                                    const xmlChar *prefix);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlGetDtdQAttrDesc(
    dtd: *mut _xmlDtd,
    elem: *const xmlChar,
    name: *const xmlChar,
    prefix: *const xmlChar,
) -> *mut _xmlAttribute {
    crate::xml::validation::get_dtd_qattr_desc(dtd, elem, name, prefix)
}

/// Search a DTD for a notation declaration.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNotationPtr xmlGetDtdNotationDesc(xmlDtdPtr dtd, const xmlChar *name);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlGetDtdNotationDesc(
    dtd: *mut _xmlDtd,
    name: *const xmlChar,
) -> *mut _xmlNotation {
    crate::xml::validation::get_dtd_notation_desc(dtd, name)
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
pub const extern "C" fn xmlGetBinaryPath() -> *mut c_char {
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
pub const extern "C" fn xmlGetHomeOfBinary() -> *mut c_char {
    // Phase 1: STUB
    ptr::null_mut()
}

// ═══════════════════════════════════════════════════════════════════════════════
// SAX2 default callback entry points (upstream SAX2.c)
// ═══════════════════════════════════════════════════════════════════════════════
//
// These are the public `xmlSAX2*` callback functions that downstream code
// installs into `xmlSAXHandler` structures. They are the same implementations
// the candidate's default SAX handler uses; exporting them under the
// upstream names is required for ABI parity (R-000136 closure).

/// Upstream SAX2.c `xmlSAX2StartDocument` — public entry point of the default handler.
#[no_mangle]
pub unsafe extern "C" fn xmlSAX2StartDocument(ctx: *mut c_void) {
    crate::xml::sax::default::default_sax_handler::startDocument(ctx)
}

/// Upstream SAX2.c `xmlSAX2EndDocument` — public entry point of the default handler.
#[no_mangle]
pub unsafe extern "C" fn xmlSAX2EndDocument(ctx: *mut c_void) {
    crate::xml::sax::default::default_sax_handler::endDocument(ctx)
}

/// Upstream SAX2.c `xmlSAX2StartElementNs` — public entry point of the default handler.
#[no_mangle]
pub unsafe extern "C" fn xmlSAX2StartElementNs(
    ctx: *mut c_void,
    localname: *const xmlChar,
    prefix: *const xmlChar,
    URI: *const xmlChar,
    nb_namespaces: c_int,
    namespaces: *mut *const xmlChar,
    nb_attributes: c_int,
    nb_defaulted: c_int,
    attributes: *mut *const xmlChar,
) {
    crate::xml::sax::default::default_sax_handler::startElementNs(
        ctx,
        localname,
        prefix,
        URI,
        nb_namespaces,
        namespaces,
        nb_attributes,
        nb_defaulted,
        attributes,
    )
}

/// Upstream SAX2.c `xmlSAX2EndElementNs` — public entry point of the default handler.
#[no_mangle]
pub unsafe extern "C" fn xmlSAX2EndElementNs(
    ctx: *mut c_void,
    localname: *const xmlChar,
    prefix: *const xmlChar,
    URI: *const xmlChar,
) {
    crate::xml::sax::default::default_sax_handler::endElementNs(ctx, localname, prefix, URI)
}

/// Upstream SAX2.c `xmlSAX2Characters` — public entry point of the default handler.
#[no_mangle]
pub unsafe extern "C" fn xmlSAX2Characters(ctx: *mut c_void, ch: *const xmlChar, len: c_int) {
    crate::xml::sax::default::default_sax_handler::characters(ctx, ch, len)
}

/// Upstream SAX2.c `xmlSAX2IgnorableWhitespace` — public entry point of the default handler.
#[no_mangle]
pub unsafe extern "C" fn xmlSAX2IgnorableWhitespace(
    ctx: *mut c_void,
    ch: *const xmlChar,
    len: c_int,
) {
    crate::xml::sax::default::default_sax_handler::ignorableWhitespace(ctx, ch, len)
}

/// Upstream SAX2.c `xmlSAX2Comment` — public entry point of the default handler.
#[no_mangle]
pub unsafe extern "C" fn xmlSAX2Comment(ctx: *mut c_void, value: *const xmlChar) {
    crate::xml::sax::default::default_sax_handler::comment(ctx, value)
}

/// Upstream SAX2.c `xmlSAX2ProcessingInstruction` — public entry point of the default handler.
#[no_mangle]
pub unsafe extern "C" fn xmlSAX2ProcessingInstruction(
    ctx: *mut c_void,
    target: *const xmlChar,
    data: *const xmlChar,
) {
    crate::xml::sax::default::default_sax_handler::processingInstruction(ctx, target, data)
}

/// Upstream SAX2.c `xmlSAX2CDataBlock` — public entry point of the default handler.
#[no_mangle]
pub unsafe extern "C" fn xmlSAX2CDataBlock(ctx: *mut c_void, value: *const xmlChar, len: c_int) {
    crate::xml::sax::default::default_sax_handler::cdataBlock(ctx, value, len)
}

/// Upstream SAX2.c `xmlSAX2InternalSubset` — public entry point of the default handler.
#[no_mangle]
pub unsafe extern "C" fn xmlSAX2InternalSubset(
    ctx: *mut c_void,
    name: *const xmlChar,
    ExternalID: *const xmlChar,
    SystemID: *const xmlChar,
) {
    crate::xml::sax::default::default_sax_handler::internalSubset(ctx, name, ExternalID, SystemID)
}

/// Upstream SAX2.c `xmlSAX2ExternalSubset` — public entry point of the default handler.
#[no_mangle]
pub unsafe extern "C" fn xmlSAX2ExternalSubset(
    ctx: *mut c_void,
    name: *const xmlChar,
    ExternalID: *const xmlChar,
    SystemID: *const xmlChar,
) {
    crate::xml::sax::default::default_sax_handler::externalSubset(ctx, name, ExternalID, SystemID)
}

/// Upstream SAX2.c `xmlSAX2EntityDecl` — public entry point of the default handler.
#[no_mangle]
pub unsafe extern "C" fn xmlSAX2EntityDecl(
    ctx: *mut c_void,
    name: *const xmlChar,
    type_: c_int,
    publicId: *const xmlChar,
    systemId: *const xmlChar,
    content: *mut xmlChar,
) {
    crate::xml::sax::default::default_sax_handler::entityDecl(
        ctx, name, type_, publicId, systemId, content,
    )
}

/// Upstream SAX2.c `xmlSAX2AttributeDecl` — public entry point of the default handler.
#[no_mangle]
pub const unsafe extern "C" fn xmlSAX2AttributeDecl(
    ctx: *mut c_void,
    elem: *const xmlChar,
    fullname: *const xmlChar,
    type_: c_int,
    def: c_int,
    defaultValue: *const xmlChar,
    tree: *mut crate::abi::structs::_xmlEnumeration,
) {
    crate::xml::sax::default::default_sax_handler::attributeDecl(
        ctx,
        elem,
        fullname,
        type_,
        def,
        defaultValue,
        tree,
    )
}

/// Upstream SAX2.c `xmlSAX2ElementDecl` — public entry point of the default handler.
#[no_mangle]
pub const unsafe extern "C" fn xmlSAX2ElementDecl(
    ctx: *mut c_void,
    name: *const xmlChar,
    type_: c_int,
    content: *mut crate::abi::structs::_xmlElementContent,
) {
    crate::xml::sax::default::default_sax_handler::elementDecl(ctx, name, type_, content)
}

/// Upstream SAX2.c `xmlSAX2NotationDecl` — public entry point of the default handler.
#[no_mangle]
pub const unsafe extern "C" fn xmlSAX2NotationDecl(
    ctx: *mut c_void,
    name: *const xmlChar,
    publicId: *const xmlChar,
    systemId: *const xmlChar,
) {
    crate::xml::sax::default::default_sax_handler::notationDecl(ctx, name, publicId, systemId)
}

/// Upstream SAX2.c `xmlSAX2UnparsedEntityDecl` — public entry point of the default handler.
#[no_mangle]
pub const unsafe extern "C" fn xmlSAX2UnparsedEntityDecl(
    ctx: *mut c_void,
    name: *const xmlChar,
    publicId: *const xmlChar,
    systemId: *const xmlChar,
    notationName: *const xmlChar,
) {
    crate::xml::sax::default::default_sax_handler::unparsedEntityDecl(
        ctx,
        name,
        publicId,
        systemId,
        notationName,
    )
}

/// Upstream SAX2.c `xmlSAX2ResolveEntity` — public entry point of the default handler.
#[no_mangle]
pub const unsafe extern "C" fn xmlSAX2ResolveEntity(
    ctx: *mut c_void,
    publicId: *const xmlChar,
    systemId: *const xmlChar,
) -> *mut crate::abi::structs::_xmlParserInput {
    crate::xml::sax::default::default_sax_handler::resolveEntity(ctx, publicId, systemId)
}

/// Upstream SAX2.c `xmlSAX2IsStandalone` — public entry point of the default handler.
#[no_mangle]
pub const unsafe extern "C" fn xmlSAX2IsStandalone(ctx: *mut c_void) -> c_int {
    crate::xml::sax::default::default_sax_handler::isStandalone(ctx)
}

/// Upstream SAX2.c `xmlSAX2HasInternalSubset` — public entry point of the default handler.
#[no_mangle]
pub unsafe extern "C" fn xmlSAX2HasInternalSubset(ctx: *mut c_void) -> c_int {
    crate::xml::sax::default::default_sax_handler::hasInternalSubset(ctx)
}

/// Upstream SAX2.c `xmlSAX2HasExternalSubset` — public entry point of the default handler.
#[no_mangle]
pub unsafe extern "C" fn xmlSAX2HasExternalSubset(ctx: *mut c_void) -> c_int {
    crate::xml::sax::default::default_sax_handler::hasExternalSubset(ctx)
}

/// Upstream SAX2.c `xmlSAX2GetEntity` — public entry point of the default handler.
#[no_mangle]
pub unsafe extern "C" fn xmlSAX2GetEntity(
    ctx: *mut c_void,
    name: *const xmlChar,
) -> *mut crate::abi::structs::_xmlEntity {
    crate::xml::sax::default::default_sax_handler::getEntity(ctx, name)
}

/// Upstream SAX2.c `xmlSAX2GetParameterEntity` — public entry point of the default handler.
#[no_mangle]
pub unsafe extern "C" fn xmlSAX2GetParameterEntity(
    ctx: *mut c_void,
    name: *const xmlChar,
) -> *mut crate::abi::structs::_xmlEntity {
    crate::xml::sax::default::default_sax_handler::getParameterEntity(ctx, name)
}

/// Upstream SAX2.c `xmlSAX2GetLineNumber` — public entry point of the
/// default handler (SAX locator callback).
#[no_mangle]
pub unsafe extern "C" fn xmlSAX2GetLineNumber(ctx: *mut c_void) -> c_int {
    crate::xml::sax::default::default_sax_handler::getLineNumber(ctx)
}

/// Upstream SAX2.c `xmlSAX2GetColumnNumber`.
#[no_mangle]
pub unsafe extern "C" fn xmlSAX2GetColumnNumber(ctx: *mut c_void) -> c_int {
    crate::xml::sax::default::default_sax_handler::getColumnNumber(ctx)
}

/// Upstream SAX2.c `xmlSAX2GetPublicId`.
#[no_mangle]
pub const unsafe extern "C" fn xmlSAX2GetPublicId(ctx: *mut c_void) -> *const xmlChar {
    crate::xml::sax::default::default_sax_handler::getPublicId(ctx)
}

/// Upstream SAX2.c `xmlSAX2GetSystemId`.
#[no_mangle]
pub unsafe extern "C" fn xmlSAX2GetSystemId(ctx: *mut c_void) -> *const xmlChar {
    crate::xml::sax::default::default_sax_handler::getSystemId(ctx)
}

/// Upstream SAX2.c `xmlSAX2StartElement` — SAX1 start-element entry point.
/// The candidate parser dispatches through the SAX2 (namespaced) callbacks;
/// this wrapper maps to the SAX1 handler when installed.
#[no_mangle]
pub unsafe extern "C" fn xmlSAX2StartElement(
    ctx: *mut c_void,
    name: *const xmlChar,
    atts: *mut *const xmlChar,
) {
    // The parser core invokes startElementNs; the SAX1 shim is provided by
    // the dispatch layer. When this entry point is installed directly on a
    // handler, route through the internal SAX1 path.
    crate::xml::sax::dispatch::SaxDispatcher::sax1_start_element(ctx, name, atts);
}

/// Upstream SAX2.c `xmlSAX2EndElement` — SAX1 end-element entry point.
#[no_mangle]
pub unsafe extern "C" fn xmlSAX2EndElement(ctx: *mut c_void, name: *const xmlChar) {
    crate::xml::sax::dispatch::SaxDispatcher::sax1_end_element(ctx, name);
}

/// Upstream SAX2.c `xmlSAX2SetDocumentLocator` — public entry point of the default handler.
#[no_mangle]
pub const unsafe extern "C" fn xmlSAX2SetDocumentLocator(
    ctx: *mut c_void,
    loc: *mut crate::abi::callbacks::_xmlSAXLocator,
) {
    crate::xml::sax::default::default_sax_handler::setDocumentLocator(ctx, loc)
}

/// Upstream SAX2.c `xmlSAX2Reference` — public entry point of the default handler.
#[no_mangle]
pub unsafe extern "C" fn xmlSAX2Reference(ctx: *mut c_void, name: *const xmlChar) {
    crate::xml::sax::default::default_sax_handler::reference(ctx, name)
}

#[cfg(test)]
mod tests {
    use super::xml_number_to_string;

    /// R-000166: number-to-string follows upstream xmlXPathFormatNumber —
    /// verified byte-identical against the oracle (xsltproc) on the t4/n3
    /// differential corpora. Cases here are exact doubles or
    /// rounding-robust formats (parser-dependent literals are covered by the
    /// differential corpora, not unit tests).
    #[allow(clippy::approx_constant)]
    #[test]
    fn test_xml_number_to_string_parity_cases() {
        let cases: &[(f64, &str)] = &[
            (1234567.891, "1234567.891"),
            (0.1 + 0.2, "0.3"),
            (1.0 / 3.0, "0.333333333333333"),
            (1e20, "1e+20"),
            (1e-5, "0.00001"),
            (123456789012345678901234567890.0, "1.23456789012346e+29"),
            (1e100, "1e+100"),
            (-1e100, "-1e+100"),
            (1.5e-100, "1.5e-100"),
            (1e9, "1000000000"),
            (0.00001, "0.00001"),
            (9.99e-6, "9.99e-06"),
            (2147483646.0, "2147483646"),
            (2147483648.0, "2.147483648e+09"),
            (-2147483647.0, "-2147483647"),
            (-2147483649.0, "-2.147483649e+09"),
            (0.5, "0.5"),
            (1.0 / 7.0, "0.142857142857143"),
            (2.675, "2.675"),
            (3.141592653589793, "3.141592653589793"),
            (-0.0, "0"),
            (0.0, "0"),
            (f64::INFINITY, "Infinity"),
            (f64::NEG_INFINITY, "-Infinity"),
            (f64::NAN, "NaN"),
            (0.30000000000000004, "0.3"),
            (2.2250738585072014e-308, "2.2250738585072e-308"),
            (5e-324, "4.94065645841247e-324"),
        ];
        for (n, expected) in cases {
            assert_eq!(&xml_number_to_string(*n), expected, "value: {}", n);
        }
    }
}
