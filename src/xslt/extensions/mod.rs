//! XSLT extension mechanisms (§33, §35, §85 Phase 8).
//!
//! Extensions allow stylesheets to call external functions and elements.
//! Registered via `xsltRegisterExtFunction` (functions) and
//! `xsltRegisterExtElement` (elements).
//!
//! # UPSTREAM-PARITY
//!
//! Upstream libxslt (extensions.c) stores extension function registrations
//! in the transform context (`extFunctions`), each entry holding the
//! namespace URI, name, and function pointer. Extension elements are stored
//! similarly with their transform function.
//!
//! Registration is per-context; functions are looked up at call time via
//! the context's XPath function lookup mechanism.

use crate::abi::allocator::xmlFree;
use crate::abi::structs::*;
use crate::abi::types::*;
use std::os::raw::c_int;
use std::ptr;

/// A registered extension function.
#[repr(C)]
pub struct _xsltExtFunction {
    pub next: *mut _xsltExtFunction,
    pub name: *mut xmlChar,
    pub ns: *mut xmlChar,
    pub func: *mut c_void,
}

/// A registered extension element.
#[repr(C)]
pub struct _xsltExtElement {
    pub next: *mut _xsltExtElement,
    pub name: *mut xmlChar,
    pub ns: *mut xmlChar,
    pub func: *mut c_void,
}

/// Register an XSLT extension function.
///
/// # SAFETY
///
/// - `ctxt` must be a valid `_xsltTransformContext`.
/// - `name` and `NS_uri` must be valid NUL-terminated strings.
/// - `f` must be a valid function pointer.
#[no_mangle]
pub unsafe extern "C" fn xsltRegisterExtFunction(
    ctxt: *mut _xsltTransformContext,
    name: *const xmlChar,
    NS_uri: *const xmlChar,
    f: Option<unsafe extern "C" fn(*mut c_void, c_int)>,
) -> c_int {
    if ctxt.is_null() || name.is_null() || NS_uri.is_null() {
        return -1;
    }
    let entry = libc::calloc(1, core::mem::size_of::<_xsltExtFunction>()) as *mut _xsltExtFunction;
    if entry.is_null() {
        return -1;
    }
    (*entry).name = dup_str(name);
    (*entry).ns = dup_str(NS_uri);
    if (*entry).name.is_null() || (*entry).ns.is_null() {
        if !(*entry).name.is_null() {
            libc::free((*entry).name as *mut libc::c_void);
        }
        if !(*entry).ns.is_null() {
            libc::free((*entry).ns as *mut libc::c_void);
        }
        libc::free(entry as *mut libc::c_void);
        return -1;
    }
    (*entry).func = f.map(|fp| fp as *mut c_void).unwrap_or(ptr::null_mut());
    // Prepend to the context's extension function list (extFunctions
    // is a void* chain in the struct; we use a linked list here).
    (*entry).next = (*ctxt).extFunctions as *mut _xsltExtFunction;
    (*ctxt).extFunctions = entry as *mut c_void;
    0
}

/// Register an XSLT extension element.
///
/// # SAFETY
///
/// - `ctxt` must be a valid `_xsltTransformContext`.
/// - `name` and `NS_uri` must be valid NUL-terminated strings.
/// - `f` must be a valid function pointer.
#[no_mangle]
pub unsafe extern "C" fn xsltRegisterExtElement(
    ctxt: *mut _xsltTransformContext,
    name: *const xmlChar,
    NS_uri: *const xmlChar,
    f: Option<
        unsafe extern "C" fn(*mut c_void, *mut c_void, *mut _xmlNode, *mut c_void, *mut _xmlNode),
    >,
) -> c_int {
    if ctxt.is_null() || name.is_null() || NS_uri.is_null() {
        return -1;
    }
    let entry = libc::calloc(1, core::mem::size_of::<_xsltExtElement>()) as *mut _xsltExtElement;
    if entry.is_null() {
        return -1;
    }
    (*entry).name = dup_str(name);
    (*entry).ns = dup_str(NS_uri);
    if (*entry).name.is_null() || (*entry).ns.is_null() {
        if !(*entry).name.is_null() {
            libc::free((*entry).name as *mut libc::c_void);
        }
        if !(*entry).ns.is_null() {
            libc::free((*entry).ns as *mut libc::c_void);
        }
        libc::free(entry as *mut libc::c_void);
        return -1;
    }
    (*entry).func = f.map(|fp| fp as *mut c_void).unwrap_or(ptr::null_mut());
    (*entry).next = (*ctxt).extElements as *mut _xsltExtElement;
    (*ctxt).extElements = entry as *mut c_void;
    0
}

/// Look up a registered extension function.
///
/// # SAFETY
///
/// - `ctxt` must be a valid `_xsltTransformContext`.
/// - `name` and `ns` must be valid NUL-terminated strings.
pub unsafe fn xsltFindExtFunction(
    ctxt: *mut _xsltTransformContext,
    name: *const xmlChar,
    ns: *const xmlChar,
) -> *mut c_void {
    if ctxt.is_null() || name.is_null() || ns.is_null() {
        return ptr::null_mut();
    }
    let mut cur = (*ctxt).extFunctions as *mut _xsltExtFunction;
    while !cur.is_null() {
        if !(*cur).name.is_null()
            && !(*cur).ns.is_null()
            && libc::strcmp(
                (*cur).name as *const libc::c_char,
                name as *const libc::c_char,
            ) == 0
            && libc::strcmp((*cur).ns as *const libc::c_char, ns as *const libc::c_char) == 0
        {
            return (*cur).func;
        }
        cur = (*cur).next;
    }
    ptr::null_mut()
}

/// Look up a registered extension element.
///
/// # SAFETY
///
/// - `ctxt` must be a valid `_xsltTransformContext`.
/// - `name` and `ns` must be valid NUL-terminated strings.
pub unsafe fn xsltFindExtElement(
    ctxt: *mut _xsltTransformContext,
    name: *const xmlChar,
    ns: *const xmlChar,
) -> *mut c_void {
    if ctxt.is_null() || name.is_null() || ns.is_null() {
        return ptr::null_mut();
    }
    let mut cur = (*ctxt).extElements as *mut _xsltExtElement;
    while !cur.is_null() {
        if !(*cur).name.is_null()
            && !(*cur).ns.is_null()
            && libc::strcmp(
                (*cur).name as *const libc::c_char,
                name as *const libc::c_char,
            ) == 0
            && libc::strcmp((*cur).ns as *const libc::c_char, ns as *const libc::c_char) == 0
        {
            return (*cur).func;
        }
        cur = (*cur).next;
    }
    ptr::null_mut()
}

/// Free all extension registrations in a transform context.
///
/// # SAFETY
///
/// - `ctxt` must be a valid `_xsltTransformContext`.
pub unsafe fn xsltFreeExts(ctxt: *mut _xsltTransformContext) {
    if ctxt.is_null() {
        return;
    }
    // Free extension functions.
    let mut cur = (*ctxt).extFunctions as *mut _xsltExtFunction;
    (*ctxt).extFunctions = ptr::null_mut();
    while !cur.is_null() {
        let next = (*cur).next;
        if !(*cur).name.is_null() {
            libc::free((*cur).name as *mut libc::c_void);
        }
        if !(*cur).ns.is_null() {
            libc::free((*cur).ns as *mut libc::c_void);
        }
        libc::free(cur as *mut libc::c_void);
        cur = next;
    }
    // Free extension elements.
    let mut cur = (*ctxt).extElements as *mut _xsltExtElement;
    (*ctxt).extElements = ptr::null_mut();
    while !cur.is_null() {
        let next = (*cur).next;
        if !(*cur).name.is_null() {
            libc::free((*cur).name as *mut libc::c_void);
        }
        if !(*cur).ns.is_null() {
            libc::free((*cur).ns as *mut libc::c_void);
        }
        libc::free(cur as *mut libc::c_void);
        cur = next;
    }
}

/// Duplicate a NUL-terminated string.
unsafe fn dup_str(s: *const xmlChar) -> *mut xmlChar {
    let len = libc::strlen(s as *const libc::c_char);
    let copy = libc::malloc(len + 1) as *mut xmlChar;
    if copy.is_null() {
        return ptr::null_mut();
    }
    libc::memcpy(copy as *mut libc::c_void, s as *const libc::c_void, len);
    *copy.add(len) = 0;
    copy
}

use std::ffi::c_void;

#[cfg(test)]
mod tests {
    use super::*;
    use core::ptr;

    fn make_ctxt() -> *mut _xsltTransformContext {
        unsafe {
            libc::calloc(1, core::mem::size_of::<_xsltTransformContext>())
                as *mut _xsltTransformContext
        }
    }

    #[test]
    fn test_register_and_find_function() {
        unsafe {
            let ctxt = make_ctxt();
            extern "C" fn dummy(_ctx: *mut c_void, _n: c_int) {}
            assert_eq!(
                xsltRegisterExtFunction(
                    ctxt,
                    b"myfunc\0".as_ptr() as *const xmlChar,
                    b"http://example.com/ext\0".as_ptr() as *const xmlChar,
                    Some(dummy),
                ),
                0
            );
            let found = xsltFindExtFunction(
                ctxt,
                b"myfunc\0".as_ptr() as *const xmlChar,
                b"http://example.com/ext\0".as_ptr() as *const xmlChar,
            );
            assert_eq!(found, dummy as *mut c_void);
            let not_found = xsltFindExtFunction(
                ctxt,
                b"other\0".as_ptr() as *const xmlChar,
                b"http://example.com/ext\0".as_ptr() as *const xmlChar,
            );
            assert!(not_found.is_null());
            xsltFreeExts(ctxt);
            libc::free(ctxt as *mut libc::c_void);
        }
    }

    #[test]
    fn test_register_null() {
        unsafe {
            assert_eq!(
                xsltRegisterExtFunction(ptr::null_mut(), ptr::null(), ptr::null(), None),
                -1
            );
            assert_eq!(
                xsltRegisterExtElement(ptr::null_mut(), ptr::null(), ptr::null(), None),
                -1
            );
        }
    }
}
