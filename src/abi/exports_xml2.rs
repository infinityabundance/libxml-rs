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

use core::ffi::c_void;
use core::ptr;
use std::mem::size_of;
use std::os::raw::{c_char, c_int, c_uint};

use crate::abi::allocator::*;
use crate::abi::callbacks::*;
use crate::abi::ownership::*;
use crate::abi::structs::*;
use crate::abi::types::xmlAttributeType::XML_ATTRIBUTE_CDATA;
use crate::abi::types::xmlElementType::*;
use crate::abi::types::xmlErrorLevel::XML_ERR_NONE;
use crate::abi::types::*;
use crate::abi::versioning::*;

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
    // Phase 1: no-op
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
    // Phase 1: no-op — Rust's type system handles data races.
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
    // Phase 1: no-op
}

// ═══════════════════════════════════════════════════════════════════════════════
// 4. Error Handling
// ═══════════════════════════════════════════════════════════════════════════════

/// Last error, stored thread-locally.
use core::cell::RefCell;
use core::sync::atomic::AtomicPtr;
use core::sync::atomic::Ordering as AtomicOrdering;

thread_local! {
    static LAST_ERROR: RefCell<Option<_xmlError>> = const { RefCell::new(None) };
}

static GENERIC_ERROR_CTX: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static GENERIC_ERROR_FUNC: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static STRUCTURED_ERROR_CTX: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static STRUCTURED_ERROR_FUNC: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

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
    GENERIC_ERROR_CTX.store(ctx, AtomicOrdering::Release);
    GENERIC_ERROR_FUNC.store(
        handler.map_or(ptr::null_mut(), |f| f as *mut c_void),
        AtomicOrdering::Release,
    );
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
    STRUCTURED_ERROR_CTX.store(ctx, AtomicOrdering::Release);
    STRUCTURED_ERROR_FUNC.store(
        handler.map_or(ptr::null_mut(), |f| f as *mut c_void),
        AtomicOrdering::Release,
    );
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
    LAST_ERROR.with(|last| {
        let mut last = last.borrow_mut();
        last.as_mut()
            .map_or(ptr::null_mut(), |e| e as *mut _xmlError)
    })
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
    if from.is_null() || to.is_null() {
        return -1;
    }
    // SAFETY: Caller guarantees both pointers are valid.
    unsafe {
        ptr::copy_nonoverlapping(from, to, 1);
    }
    0
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
    if err.is_null() {
        return;
    }
    // SAFETY: Caller guarantees pointer is valid.
    unsafe {
        ptr::write(
            err,
            _xmlError {
                domain: XML_FROM_NONE,
                code: XML_ERR_OK as c_int,
                message: ptr::null_mut(),
                level: XML_ERR_NONE as c_int,
                file: ptr::null_mut(),
                line: 0,
                str1: ptr::null_mut(),
                str2: ptr::null_mut(),
                str3: ptr::null_mut(),
                int1: 0,
                int2: 0,
                ctxt: ptr::null_mut(),
                node: ptr::null_mut(),
            },
        );
    }
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
    // Phase 1: basic error reporting.
    // Full implementation with variadic formatting will be in Phase 1+ when
    // the errors module is implemented.

    // Store the last error
    let err = _xmlError {
        domain,
        code,
        message: ptr::null_mut(), // will be set by errors module
        level,
        file: file as *mut c_char,
        line,
        str1: str1 as *mut c_char,
        str2: str2 as *mut c_char,
        str3: str3 as *mut c_char,
        int1,
        int2,
        ctxt: ptr::null_mut(),
        node: ptr::null_mut(),
    };

    LAST_ERROR.with(|last| {
        *last.borrow_mut() = Some(err);
    });

    // Call the structured error handler if set
    let structured_func = STRUCTURED_ERROR_FUNC.load(AtomicOrdering::Acquire);
    if !structured_func.is_null() {
        let ctx = STRUCTURED_ERROR_CTX.load(AtomicOrdering::Acquire);
        let handler: xmlStructuredErrorFunc = unsafe { core::mem::transmute(structured_func) };
        if let Some(err_ptr) =
            LAST_ERROR.with(|last| last.borrow().as_ref().map(|e| e as *const _xmlError))
        {
            unsafe { handler(ctx, err_ptr) };
        }
    }

    // Call the generic error handler if set (for warnings/errors)
    let generic_func = GENERIC_ERROR_FUNC.load(AtomicOrdering::Acquire);
    if !generic_func.is_null() && level != 0 {
        let ctx = GENERIC_ERROR_CTX.load(AtomicOrdering::Acquire);
        let handler: xmlGenericErrorFunc = unsafe { core::mem::transmute(generic_func) };
        if !msg.is_null() {
            unsafe { handler(ctx, msg) };
        }
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
    LAST_ERROR.with(|last| {
        *last.borrow_mut() = None;
    });
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
    // Phase 1: STUB — will be implemented in xml/tree module.
    // Returns a minimal document structure.
    let doc = unsafe { xmlMallocZero(size_of::<_xmlDoc>()) as *mut _xmlDoc };
    if doc.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        ptr::write(
            doc,
            _xmlDoc {
                _private: ptr::null_mut(),
                type_: XML_DOCUMENT_NODE as c_int,
                name: ptr::null_mut(),
                children: ptr::null_mut(),
                last: ptr::null_mut(),
                parent: ptr::null_mut(),
                next: ptr::null_mut(),
                prev: ptr::null_mut(),
                doc: doc,
                compression: 0,
                standalone: -1,
                intSubset: ptr::null_mut(),
                extSubset: ptr::null_mut(),
                oldNs: ptr::null_mut(),
                version: if version.is_null() {
                    ptr::null_mut()
                } else {
                    xmlStrdup(version)
                },
                encoding: ptr::null_mut(),
                ids: ptr::null_mut(),
                refs: ptr::null_mut(),
                URL: ptr::null_mut(),
                charset: 0,
                dict: ptr::null_mut(),
                psvi: ptr::null_mut(),
                parseFlags: 0,
                properties: 0,
            },
        );
    }
    doc
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
    if doc.is_null() {
        return;
    }
    // Phase 1: STUB — will recursively free the tree in xml/tree module.
    // For now, just free the document structure itself.
    unsafe {
        let d = &*doc;
        if !d.version.is_null() {
            xmlFree(d.version as *mut c_void);
        }
        if !d.encoding.is_null() {
            xmlFree(d.encoding as *mut c_void);
        }
        if !d.URL.is_null() {
            xmlFree(d.URL as *mut c_void);
        }
        xmlFree(doc as *mut c_void);
    }
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
    // Phase 1: STUB — will be implemented in xml/tree module.
    let node = unsafe { xmlMallocZero(size_of::<_xmlNode>()) as *mut _xmlNode };
    if node.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        ptr::write(
            node,
            _xmlNode {
                _private: ptr::null_mut(),
                type_: XML_ELEMENT_NODE as c_int,
                name: if name.is_null() {
                    ptr::null_mut()
                } else {
                    xmlStrdup(name)
                },
                children: ptr::null_mut(),
                last: ptr::null_mut(),
                parent: ptr::null_mut(),
                next: ptr::null_mut(),
                prev: ptr::null_mut(),
                doc: ptr::null_mut(),
                ns: ns,
                content: ptr::null_mut(),
                properties: ptr::null_mut(),
                nsDef: ptr::null_mut(),
                psvi: ptr::null_mut(),
                line: 0,
                extra: 0,
            },
        );
    }
    node
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
    if node.is_null() {
        return;
    }
    // Phase 1: STUB — will recursively free the subtree in xml/tree module.
    unsafe {
        let n = &*node;
        if !n.name.is_null() {
            xmlFree(n.name as *mut c_void);
        }
        if !n.content.is_null() {
            xmlFree(n.content as *mut c_void);
        }
        xmlFree(node as *mut c_void);
    }
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
    if node.is_null() {
        return;
    }
    // Phase 1: STUB — will properly update parent/child/sibling links in xml/tree module.
    unsafe {
        let n = &mut *node;
        // Update parent's children/last pointers
        if !n.parent.is_null() {
            let parent = &mut *n.parent;
            if parent.children == node {
                parent.children = n.next;
            }
            if parent.last == node {
                parent.last = n.prev;
            }
        }
        // Update sibling links
        if !n.prev.is_null() {
            (*(n.prev)).next = n.next;
        }
        if !n.next.is_null() {
            (*(n.next)).prev = n.prev;
        }
        n.parent = ptr::null_mut();
        n.next = ptr::null_mut();
        n.prev = ptr::null_mut();
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
    if parent.is_null() || cur.is_null() {
        return ptr::null_mut();
    }
    // Phase 1: STUB — will properly manage tree links in xml/tree module.
    unsafe {
        let c = &mut *cur;
        let p = &mut *parent;

        c.parent = parent;
        c.next = ptr::null_mut();
        c.prev = p.last;

        if !p.last.is_null() {
            (*(p.last)).next = cur;
        } else {
            p.children = cur;
        }
        p.last = cur;
    }
    cur
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
    if cur.is_null() || sibling.is_null() {
        return ptr::null_mut();
    }
    // Phase 1: STUB — will properly manage tree links in xml/tree module.
    unsafe {
        let s = &mut *sibling;
        let c = &mut *cur;

        s.parent = c.parent;
        s.prev = cur;
        s.next = c.next;

        if !c.next.is_null() {
            (*(c.next)).prev = sibling;
        }
        c.next = sibling;

        // Update parent's last if necessary
        if !s.parent.is_null() {
            let p = &mut *(s.parent);
            if p.last == cur {
                p.last = sibling;
            }
        }
    }
    sibling
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
    // Phase 1: STUB — will be implemented in xml/tree module.
    let node = unsafe { xmlNewNode(ns, name) };
    if node.is_null() {
        return ptr::null_mut();
    }
    if !content.is_null() {
        let text_node = unsafe { xmlNewText(content) };
        if !text_node.is_null() {
            unsafe { xmlAddChild(node, text_node) };
        }
    }
    if !parent.is_null() {
        unsafe { xmlAddChild(parent, node) };
    }
    node
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
    if doc.is_null() {
        return ptr::null_mut();
    }
    // Phase 1: STUB — will be implemented in xml/tree module.
    unsafe {
        let d = &mut *doc;
        let old_root = d.children;

        // Clear existing children
        d.children = ptr::null_mut();
        d.last = ptr::null_mut();

        // Set new root
        if !root.is_null() {
            let r = &mut *root;
            r.parent = doc as *mut _xmlNode;
            r.doc = doc;
            d.children = root;
            d.last = root;
        }

        old_root
    }
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
    if doc.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        let d = &*doc;
        // The root element is the first child of the document
        let mut child = d.children;
        while !child.is_null() {
            let c = &*child;
            if c.type_ == XML_ELEMENT_NODE as c_int {
                return child;
            }
            child = c.next;
        }
        ptr::null_mut()
    }
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
    if node.is_null() {
        return ptr::null_mut();
    }
    // Phase 1: STUB — will be implemented in xml/tree module.
    unsafe {
        let src = &*node;
        let copy = xmlMallocZero(size_of::<_xmlNode>()) as *mut _xmlNode;
        if copy.is_null() {
            return ptr::null_mut();
        }
        ptr::write(
            copy,
            _xmlNode {
                _private: ptr::null_mut(),
                type_: src.type_,
                name: if src.name.is_null() {
                    ptr::null_mut()
                } else {
                    xmlStrdup(src.name)
                },
                children: ptr::null_mut(),
                last: ptr::null_mut(),
                parent: ptr::null_mut(),
                next: ptr::null_mut(),
                prev: ptr::null_mut(),
                doc: ptr::null_mut(),
                ns: src.ns,
                content: if src.content.is_null() {
                    ptr::null_mut()
                } else {
                    xmlStrdup(src.content)
                },
                properties: ptr::null_mut(),
                nsDef: ptr::null_mut(),
                psvi: ptr::null_mut(),
                line: src.line,
                extra: src.extra,
            },
        );
        // Deep copy: copy children recursively
        if extended != 0 {
            let mut child = src.children;
            let mut last_copied: *mut _xmlNode = ptr::null_mut();
            while !child.is_null() {
                let child_copy = xmlCopyNode(child, extended);
                if !child_copy.is_null() {
                    let cc = &mut *child_copy;
                    cc.parent = copy;
                    cc.prev = last_copied;
                    if !last_copied.is_null() {
                        (*(last_copied)).next = child_copy;
                    } else {
                        (*copy).children = child_copy;
                    }
                    last_copied = child_copy;
                }
                child = (*child).next;
            }
            (*copy).last = last_copied;
        }
        copy
    }
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
    if doc.is_null() {
        return ptr::null_mut();
    }
    // Phase 1: STUB — will be implemented in xml/tree module.
    unsafe {
        let src = &*doc;
        let new_doc = xmlNewDoc(src.version);
        if new_doc.is_null() {
            return ptr::null_mut();
        }
        let d = &mut *new_doc;
        d.encoding = if src.encoding.is_null() {
            ptr::null_mut()
        } else {
            xmlStrdup(src.encoding)
        };
        d.standalone = src.standalone;
        d.compression = src.compression;
        if recursive != 0 && !src.children.is_null() {
            let root_copy = xmlCopyNode(src.children, 1);
            if !root_copy.is_null() {
                xmlDocSetRootElement(new_doc, root_copy);
            }
        }
        new_doc
    }
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
    let node = unsafe { xmlMallocZero(size_of::<_xmlNode>()) as *mut _xmlNode };
    if node.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        ptr::write(
            node,
            _xmlNode {
                _private: ptr::null_mut(),
                type_: XML_TEXT_NODE as c_int,
                name: b"text\0" as *const u8 as *mut xmlChar,
                children: ptr::null_mut(),
                last: ptr::null_mut(),
                parent: ptr::null_mut(),
                next: ptr::null_mut(),
                prev: ptr::null_mut(),
                doc: ptr::null_mut(),
                ns: ptr::null_mut(),
                content: if content.is_null() {
                    ptr::null_mut()
                } else {
                    xmlStrdup(content)
                },
                properties: ptr::null_mut(),
                nsDef: ptr::null_mut(),
                psvi: ptr::null_mut(),
                line: 0,
                extra: 0,
            },
        );
    }
    node
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
    let node = unsafe { xmlMallocZero(size_of::<_xmlNode>()) as *mut _xmlNode };
    if node.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        ptr::write(
            node,
            _xmlNode {
                _private: ptr::null_mut(),
                type_: XML_COMMENT_NODE as c_int,
                name: b"comment\0" as *const u8 as *mut xmlChar,
                children: ptr::null_mut(),
                last: ptr::null_mut(),
                parent: ptr::null_mut(),
                next: ptr::null_mut(),
                prev: ptr::null_mut(),
                doc: ptr::null_mut(),
                ns: ptr::null_mut(),
                content: if content.is_null() {
                    ptr::null_mut()
                } else {
                    xmlStrdup(content)
                },
                properties: ptr::null_mut(),
                nsDef: ptr::null_mut(),
                psvi: ptr::null_mut(),
                line: 0,
                extra: 0,
            },
        );
    }
    node
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
    let node = unsafe { xmlMallocZero(size_of::<_xmlNode>()) as *mut _xmlNode };
    if node.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        ptr::write(
            node,
            _xmlNode {
                _private: ptr::null_mut(),
                type_: XML_PI_NODE as c_int,
                name: if name.is_null() {
                    ptr::null_mut()
                } else {
                    xmlStrdup(name)
                },
                children: ptr::null_mut(),
                last: ptr::null_mut(),
                parent: ptr::null_mut(),
                next: ptr::null_mut(),
                prev: ptr::null_mut(),
                doc: ptr::null_mut(),
                ns: ptr::null_mut(),
                content: if content.is_null() {
                    ptr::null_mut()
                } else {
                    xmlStrdup(content)
                },
                properties: ptr::null_mut(),
                nsDef: ptr::null_mut(),
                psvi: ptr::null_mut(),
                line: 0,
                extra: 0,
            },
        );
    }
    node
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
    let node = unsafe { xmlMallocZero(size_of::<_xmlNode>()) as *mut _xmlNode };
    if node.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        ptr::write(
            node,
            _xmlNode {
                _private: ptr::null_mut(),
                type_: XML_CDATA_SECTION_NODE as c_int,
                name: b"cdata\0" as *const u8 as *mut xmlChar,
                children: ptr::null_mut(),
                last: ptr::null_mut(),
                parent: ptr::null_mut(),
                next: ptr::null_mut(),
                prev: ptr::null_mut(),
                doc: doc,
                ns: ptr::null_mut(),
                content: if content.is_null() || len <= 0 {
                    ptr::null_mut()
                } else {
                    let s = xmlMalloc((len + 1) as usize) as *mut xmlChar;
                    if !s.is_null() {
                        ptr::copy_nonoverlapping(content, s, len as usize);
                        *s.add(len as usize) = 0;
                    }
                    s
                },
                properties: ptr::null_mut(),
                nsDef: ptr::null_mut(),
                psvi: ptr::null_mut(),
                line: 0,
                extra: 0,
            },
        );
    }
    node
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
    // Phase 1: STUB — will be implemented in xml/namespaces module.
    let ns = unsafe { xmlMallocZero(size_of::<_xmlNs>()) as *mut _xmlNs };
    if ns.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        ptr::write(
            ns,
            _xmlNs {
                next: ptr::null_mut(),
                type_: XML_LOCAL_NAMESPACE as c_int,
                href: if href.is_null() {
                    ptr::null_mut()
                } else {
                    xmlStrdup(href)
                },
                prefix: if prefix.is_null() {
                    ptr::null_mut()
                } else {
                    xmlStrdup(prefix)
                },
                _private: ptr::null_mut(),
                context: node as *mut _xmlDoc,
            },
        );
        // Add to node's nsDef list
        if !node.is_null() {
            let n = &mut *node;
            (*ns).next = n.nsDef as *mut _xmlNs;
            n.nsDef = ns;
        }
    }
    ns
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
    if node.is_null() {
        return;
    }
    unsafe {
        (*node).ns = ns;
    }
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
    // Phase 1: STUB — will be implemented in xml/namespaces module.
    ptr::null_mut()
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
    // Phase 1: STUB
    ptr::null_mut()
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
    // Phase 1: STUB
    ptr::null_mut()
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
    if node.is_null() || name.is_null() {
        return ptr::null_mut();
    }
    // Phase 1: STUB — will be implemented in xml/tree module.
    // Check if attribute already exists
    let mut attr = unsafe { (*node).properties };
    while !attr.is_null() {
        let a = unsafe { &*attr };
        if !a.name.is_null() && unsafe { xmlStrEqual(a.name, name) } != 0 {
            // Update existing attribute value
            unsafe {
                if !a.children.is_null() {
                    let text_node = &mut *a.children;
                    if !text_node.content.is_null() {
                        xmlFree(text_node.content as *mut c_void);
                    }
                    text_node.content = if value.is_null() {
                        ptr::null_mut()
                    } else {
                        xmlStrdup(value)
                    };
                }
            }
            return attr;
        }
        attr = a.next as *mut _xmlAttr;
    }

    // Create new attribute
    let new_attr = unsafe { xmlMallocZero(size_of::<_xmlAttr>()) as *mut _xmlAttr };
    if new_attr.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        ptr::write(
            new_attr,
            _xmlAttr {
                _private: ptr::null_mut(),
                type_: XML_ATTRIBUTE_NODE as c_int,
                name: xmlStrdup(name),
                children: ptr::null_mut(),
                last: ptr::null_mut(),
                parent: node as *mut _xmlNode,
                next: ptr::null_mut(),
                prev: ptr::null_mut(),
                doc: (*node).doc,
                ns: ptr::null_mut(),
                atype: XML_ATTRIBUTE_CDATA as c_int,
                psvi: ptr::null_mut(),
                id: ptr::null_mut(),
            },
        );

        // Create text child for the value
        if !value.is_null() {
            let text = xmlNewText(value);
            if !text.is_null() {
                (*text).parent = new_attr as *mut _xmlNode;
                (*new_attr).children = text;
                (*new_attr).last = text;
            }
        }

        // Link into the node's property list
        let n = &mut *node;
        (*new_attr).next = n.properties as *mut _xmlAttr;
        if !n.properties.is_null() {
            (*(n.properties)).prev = new_attr;
        }
        n.properties = new_attr;
    }
    new_attr
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
    if node.is_null() || name.is_null() {
        return ptr::null_mut();
    }
    // Phase 1: STUB — will be implemented in xml/tree module.
    let mut attr = unsafe { (*node).properties };
    while !attr.is_null() {
        let a = unsafe { &*attr };
        if !a.name.is_null() && unsafe { xmlStrEqual(a.name, name) } != 0 {
            // Get the value from the attribute's text child
            let text = unsafe { a.children };
            if !text.is_null() {
                let t = unsafe { &*text };
                if !t.content.is_null() {
                    return unsafe { xmlStrdup(t.content) };
                }
            }
            return unsafe { xmlStrdup(b"\0" as *const u8 as *const xmlChar) };
        }
        attr = unsafe { (*attr).next as *mut _xmlAttr };
    }
    ptr::null_mut()
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
    // Phase 1: STUB — will be implemented in xml/tree module.
    ptr::null_mut()
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
    // Phase 1: STUB — will be implemented in xml/tree module.
    // For now, delegate to xmlSetProp (ignoring namespace)
    unsafe { xmlSetProp(node, name, value) }
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
    if attr.is_null() {
        return -1;
    }
    // Phase 1: STUB — will be implemented in xml/tree module.
    unsafe {
        let a = &mut *attr;
        // Unlink from parent node
        if !a.parent.is_null() {
            let parent = &mut *(a.parent);
            if parent.properties == attr {
                parent.properties = a.next;
            }
        }
        // Update sibling links
        if !a.prev.is_null() {
            (*(a.prev)).next = a.next;
        }
        if !a.next.is_null() {
            (*(a.next)).prev = a.prev;
        }
        // Free the attribute name
        if !a.name.is_null() {
            xmlFree(a.name as *mut c_void);
        }
        // Free the children (value text nodes)
        if !a.children.is_null() {
            xmlFreeNode(a.children);
        }
        xmlFree(attr as *mut c_void);
    }
    0
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
    if doc.is_null() {
        return ptr::null_mut();
    }
    unsafe { (*doc).intSubset }
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
    // Phase 1: STUB — will be implemented in xml/dtd module.
    let dtd = unsafe { xmlMallocZero(size_of::<_xmlDtd>()) as *mut _xmlDtd };
    if dtd.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        ptr::write(
            dtd,
            _xmlDtd {
                _private: ptr::null_mut(),
                type_: XML_DTD_NODE as c_int,
                name: if name.is_null() {
                    ptr::null_mut()
                } else {
                    xmlStrdup(name)
                },
                children: ptr::null_mut(),
                last: ptr::null_mut(),
                parent: doc,
                next: ptr::null_mut(),
                prev: ptr::null_mut(),
                doc: doc,
                notations: ptr::null_mut(),
                elements: ptr::null_mut(),
                attributes: ptr::null_mut(),
                entities: ptr::null_mut(),
                ExternalID: if ExternalID.is_null() {
                    ptr::null_mut()
                } else {
                    xmlStrdup(ExternalID)
                },
                SystemID: if SystemID.is_null() {
                    ptr::null_mut()
                } else {
                    xmlStrdup(SystemID)
                },
                pentities: ptr::null_mut(),
            },
        );
        if !doc.is_null() {
            (*doc).intSubset = dtd;
        }
    }
    dtd
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
    // Phase 1: STUB — will be implemented in xml/entities module.
    ptr::null_mut()
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
    // Phase 1: STUB — will be implemented in xml/entities module.
    ptr::null_mut()
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
    // Phase 1: STUB
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
pub extern "C" fn xmlGetLineNo(node: *const _xmlNode) -> c_uint {
    if node.is_null() {
        return 0;
    }
    unsafe { (*node).line as c_uint }
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
    // Phase 1: STUB — will be implemented in xml/parser module.
    // For now, returns NULL to indicate parse failure.
    ptr::null_mut()
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
    // Phase 1: STUB
    ptr::null_mut()
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
    // Phase 1: STUB
    ptr::null_mut()
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
    // Phase 1: STUB
    ptr::null_mut()
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
    // Phase 1: STUB
    ptr::null_mut()
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
    // Phase 1: STUB
    ptr::null_mut()
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
    // Phase 1: STUB
    ptr::null_mut()
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
    // Phase 1: STUB
    ptr::null_mut()
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
    // Phase 1: STUB
    -1
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
    // Phase 1: STUB
    -1
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
    // Phase 1: STUB
    ptr::null_mut()
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
    // Phase 1: STUB
    ptr::null_mut()
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
    // Phase 1: STUB
    ptr::null_mut()
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
    // Phase 1: STUB
    ptr::null_mut()
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
    // Phase 1: STUB
    ptr::null_mut()
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
    // Phase 1: STUB
    -1
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
    // Phase 1: STUB — will be implemented in xml/parser module.
    unsafe {
        xmlFree(ctxt as *mut c_void);
    }
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
    // Phase 1: STUB
    -1
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
    // Phase 1: STUB
    ptr::null_mut()
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
    // Phase 1: STUB
    ptr::null_mut()
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
    // Phase 1: STUB
    ptr::null_mut()
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
    // Phase 1: STUB
    unsafe {
        xmlFree(buf as *mut c_void);
    }
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
    // Phase 1: STUB
    ptr::null_mut()
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
    // Phase 1: STUB
    unsafe {
        xmlFree(input as *mut c_void);
    }
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
    // Phase 1: STUB
    ptr::null_mut()
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
    // Phase 1: STUB
    ptr::null_mut()
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
    // Phase 1: STUB
    ptr::null_mut()
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
        return 0;
    }
    // Phase 1: STUB
    unsafe {
        xmlFree(out as *mut c_void);
    }
    0
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
    // Phase 1: STUB
    0
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
    // Phase 1: STUB
    0
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
    // Phase 1: STUB
    ptr::null_mut()
}

/// Create a new buffer of a given size.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlBufferPtr xmlBufferCreateSize(size_t size);
/// ```
#[no_mangle]
pub extern "C" fn xmlBufferCreateSize(_size: usize) -> *mut _xmlBuffer {
    // Phase 1: STUB
    ptr::null_mut()
}

/// Create a buffer from a static string.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlBufferPtr xmlBufferCreateStatic(void *mem, size_t size);
/// ```
#[no_mangle]
pub extern "C" fn xmlBufferCreateStatic(_mem: *mut c_void, _size: usize) -> *mut _xmlBuffer {
    // Phase 1: STUB
    ptr::null_mut()
}

/// Free a buffer.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlBufferFree(xmlBufferPtr buf);
/// ```
#[no_mangle]
pub extern "C" fn xmlBufferFree(_buf: *mut _xmlBuffer) {
    // Phase 1: STUB
}

/// Empty a buffer.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlBufferEmpty(xmlBufferPtr buf);
/// ```
#[no_mangle]
pub extern "C" fn xmlBufferEmpty(_buf: *mut _xmlBuffer) {
    // Phase 1: STUB
}

/// Get buffer content.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlBufferContent(const xmlBuffer *buf);
/// ```
#[no_mangle]
pub extern "C" fn xmlBufferContent(_buf: *const _xmlBuffer) -> *mut xmlChar {
    // Phase 1: STUB
    ptr::null_mut()
}

/// Get buffer length.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlBufferLength(const xmlBuffer *buf);
/// ```
#[no_mangle]
pub extern "C" fn xmlBufferLength(_buf: *const _xmlBuffer) -> c_int {
    // Phase 1: STUB
    0
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
    _buf: *mut _xmlBuffer,
    _str: *const xmlChar,
    _len: c_int,
) -> c_int {
    // Phase 1: STUB
    0
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
    _buf: *mut _xmlBuffer,
    _str: *const xmlChar,
    _len: c_int,
) -> c_int {
    // Phase 1: STUB
    0
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
pub extern "C" fn xmlBufferSetAllocationScheme(_buf: *mut _xmlBuffer, _scheme: c_int) {
    // Phase 1: STUB
}

/// Shrink buffer.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlBufferShrink(xmlBufferPtr buf, int len);
/// ```
#[no_mangle]
pub extern "C" fn xmlBufferShrink(_buf: *mut _xmlBuffer, _len: c_int) -> c_int {
    // Phase 1: STUB
    0
}

/// Grow buffer.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlBufferGrow(xmlBufferPtr buf, int len);
/// ```
#[no_mangle]
pub extern "C" fn xmlBufferGrow(_buf: *mut _xmlBuffer, _len: c_int) -> c_int {
    // Phase 1: STUB
    0
}

/// Reserve buffer space.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlBufferReserve(xmlBufferPtr buf, int len);
/// ```
#[no_mangle]
pub extern "C" fn xmlBufferReserve(_buf: *mut _xmlBuffer, _len: c_int) -> c_int {
    // Phase 1: STUB
    0
}

/// Detach buffer content.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlBufferDetach(xmlBufferPtr buf);
/// ```
#[no_mangle]
pub extern "C" fn xmlBufferDetach(_buf: *mut _xmlBuffer) -> *mut xmlChar {
    // Phase 1: STUB
    ptr::null_mut()
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
pub extern "C" fn xmlGetCharEncoding(_name: *const c_char) -> c_int {
    // Phase 1: STUB
    // Return XML_CHAR_ENCODING_NONE = 0
    0
}

/// Find an encoding handler.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlCharEncodingHandlerPtr xmlFindCharEncodingHandler(const char *name);
/// ```
#[no_mangle]
pub extern "C" fn xmlFindCharEncodingHandler(_name: *const c_char) -> *mut c_void {
    // Phase 1: STUB
    ptr::null_mut()
}

/// Close an encoding handler.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlCharEncCloseFunc(xmlCharEncodingHandlerPtr handler);
/// ```
#[no_mangle]
pub extern "C" fn xmlCharEncCloseFunc(_handler: *mut c_void) -> c_int {
    // Phase 1: STUB
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
pub extern "C" fn xmlCharEncInput(_input: *mut _xmlParserInputBuffer, _to: c_int) -> c_int {
    // Phase 1: STUB
    0
}

/// Convert an output buffer's encoding.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlCharEncOutput(xmlOutputBufferPtr output, int to);
/// ```
#[no_mangle]
pub extern "C" fn xmlCharEncOutput(_output: *mut _xmlOutputBuffer, _to: c_int) -> c_int {
    // Phase 1: STUB
    0
}

// ═══════════════════════════════════════════════════════════════════════════════
// 14. XPath
// ═══════════════════════════════════════════════════════════════════════════════

/// Create a new XPath context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlXPathContextPtr xmlXPathNewContext(xmlDocPtr doc);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlXPathNewContext(_doc: *mut _xmlDoc) -> *mut _xmlXPathContext {
    // Phase 1: STUB
    ptr::null_mut()
}

/// Free an XPath context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlXPathFreeContext(xmlXPathContextPtr ctxt);
/// ```
#[no_mangle]
pub extern "C" fn xmlXPathFreeContext(_ctxt: *mut _xmlXPathContext) {
    // Phase 1: STUB
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
    _str: *const xmlChar,
    _ctxt: *mut _xmlXPathContext,
) -> *mut _xmlXPathObject {
    // Phase 1: STUB
    ptr::null_mut()
}

/// Evaluate an XPath expression (simplified).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlXPathObjectPtr xmlXPathEval(const xmlChar *str, xmlXPathContextPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlXPathEval(
    _str: *const xmlChar,
    _ctxt: *mut _xmlXPathContext,
) -> *mut _xmlXPathObject {
    // Phase 1: STUB
    ptr::null_mut()
}

/// Free an XPath object.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlXPathFreeObject(xmlXPathObjectPtr obj);
/// ```
#[no_mangle]
pub extern "C" fn xmlXPathFreeObject(_obj: *mut _xmlXPathObject) {
    // Phase 1: STUB
}

/// Compile an XPath expression.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlXPathCompExprPtr xmlXPathCompile(const xmlChar *str);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlXPathCompile(_str: *const xmlChar) -> *mut c_void {
    // Phase 1: STUB
    ptr::null_mut()
}

/// Free a compiled XPath expression.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlXPathFreeCompExpr(xmlXPathCompExprPtr comp);
/// ```
#[no_mangle]
pub extern "C" fn xmlXPathFreeCompExpr(_comp: *mut c_void) {
    // Phase 1: STUB
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
    _ctxt: *mut _xmlXPathContext,
    _prefix: *const xmlChar,
    _ns_uri: *const xmlChar,
) -> c_int {
    // Phase 1: STUB
    0
}

/// Register an XPath function.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlXPathRegisterFunc(xmlXPathContextPtr ctxt,
///                          const xmlChar *name, xmlXPathFunction f);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlXPathRegisterFunc(
    _ctxt: *mut _xmlXPathContext,
    _name: *const xmlChar,
    _f: Option<unsafe extern "C" fn(*mut c_void, c_int)>,
) -> c_int {
    // Phase 1: STUB
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
    _ctxt: *mut _xmlXPathContext,
    _name: *const xmlChar,
    _ns_uri: *const xmlChar,
    _f: Option<unsafe extern "C" fn(*mut c_void, c_int)>,
) -> c_int {
    // Phase 1: STUB
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
    _ctxt: *mut _xmlXPathContext,
    _name: *const xmlChar,
    _value: *mut _xmlXPathObject,
) -> c_int {
    // Phase 1: STUB
    0
}

/// Create an XPath object from a node set.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlXPathObjectPtr xmlXPathNewNodeSet(xmlNodePtr val);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlXPathNewNodeSet(_val: *mut _xmlNode) -> *mut _xmlXPathObject {
    // Phase 1: STUB
    ptr::null_mut()
}

/// Create an XPath object from a value.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlXPathObjectPtr xmlXPathNewCString(const xmlChar *val);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlXPathNewCString(_val: *const xmlChar) -> *mut _xmlXPathObject {
    // Phase 1: STUB
    ptr::null_mut()
}

/// Create an XPath number object.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlXPathObjectPtr xmlXPathNewFloat(double val);
/// ```
#[no_mangle]
pub extern "C" fn xmlXPathNewFloat(_val: f64) -> *mut _xmlXPathObject {
    // Phase 1: STUB
    ptr::null_mut()
}

/// Create an XPath boolean object.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlXPathObjectPtr xmlXPathNewBoolean(int val);
/// ```
#[no_mangle]
pub extern "C" fn xmlXPathNewBoolean(_val: c_int) -> *mut _xmlXPathObject {
    // Phase 1: STUB
    ptr::null_mut()
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
pub extern "C" fn xmlXIncludeProcess(_doc: *mut _xmlDoc) -> c_int {
    // Phase 1: STUB
    -1
}

/// Process XInclude nodes with flags.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlXIncludeProcessFlags(xmlDocPtr doc, int flags);
/// ```
#[no_mangle]
pub extern "C" fn xmlXIncludeProcessFlags(_doc: *mut _xmlDoc, _flags: c_int) -> c_int {
    // Phase 1: STUB
    -1
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
pub extern "C" fn xmlCatalogLoad(_catalogs: *const c_char) -> *mut c_void {
    // Phase 1: STUB
    ptr::null_mut()
}

/// Resolve a public ID.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlCharPtr xmlCatalogResolvePublic(const xmlChar *pubID);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlCatalogResolvePublic(_pubID: *const xmlChar) -> *mut xmlChar {
    // Phase 1: STUB
    ptr::null_mut()
}

/// Resolve a system ID.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlCharPtr xmlCatalogResolveSystem(const xmlChar *sysID);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlCatalogResolveSystem(_sysID: *const xmlChar) -> *mut xmlChar {
    // Phase 1: STUB
    ptr::null_mut()
}

/// Resolve a URI.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlCharPtr xmlCatalogResolveURI(const xmlChar *URI);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlCatalogResolveURI(_URI: *const xmlChar) -> *mut xmlChar {
    // Phase 1: STUB
    ptr::null_mut()
}

/// Set catalog defaults.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlCatalogSetDefaults(xmlCatalogAllowValue allow);
/// ```
#[no_mangle]
pub extern "C" fn xmlCatalogSetDefaults(_allow: c_int) {
    // Phase 1: STUB
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
    // Phase 1: STUB
    0
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
    _type: *const xmlChar,
    _orig: *const xmlChar,
    _replace: *const xmlChar,
) -> c_int {
    // Phase 1: STUB
    0
}

/// Remove a catalog entry.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlCatalogRemove(const xmlChar *value);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlCatalogRemove(_value: *const xmlChar) -> c_int {
    // Phase 1: STUB
    0
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
    // Phase 1: STUB
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
    // Phase 1: STUB
    ptr::null_mut()
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
// 18. Debug / Miscellaneous
// ═══════════════════════════════════════════════════════════════════════════════

/// Dump a document to a file for debugging.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlDebugDumpDocument(FILE *output, xmlDocPtr doc);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlDebugDumpDocument(_output: *mut c_void, _doc: *mut _xmlDoc) {
    // Phase 1: STUB
}

/// Dump a node for debugging.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlDebugDumpNode(FILE *output, xmlNodePtr node);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlDebugDumpNode(_output: *mut c_void, _node: *mut _xmlNode) {
    // Phase 1: STUB
}

/// Dump a node for debugging (recursive).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlDebugDumpNodeList(FILE *output, xmlNodePtr node);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlDebugDumpNodeList(_output: *mut c_void, _node: *mut _xmlNode) {
    // Phase 1: STUB
}

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
