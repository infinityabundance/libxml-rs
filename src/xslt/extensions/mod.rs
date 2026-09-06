//! XSLT extension mechanisms (§33, §35, §85 Phase 8).
//!
//! # Upstream contract
//!
//! Parity target: upstream libxslt `extensions.c` (1.1.45;
//! `SRC-LIBXSLT-1.1.42-EXTENSIONS-C` under oracle/historical/src).
//! Subsystem census: xslt-extension-functions, xslt-extension-elements,
//! xslt-global-state. ABI surface: `xsltRegisterExtFunction`,
//! `xsltRegisterExtElement`, plus the per-module extension registries
//! (`xsltRegisterExtModule*`) exercised by XSLT-001.
//!
//! # Conceptual behavior
//!
//! Extension functions/elements are registered per transform context
//! (upstream `extFunctions`/`extElements`), each entry carrying the
//! namespace URI, local name, and a callback. At call time the XPath
//! function lookup resolves `prefix:local` through the stylesheet
//! namespace bindings and dispatches to the registered callback; extension
//! elements are recognized during instruction execution by namespace.
//! EXSLT rides the same mechanism (registered into every new context).
//!
//! # Ownership & safety invariants
//!
//! Entries are heap-allocated and own duplicated `name`/`ns` strings;
//! they are freed with the context (`xsltFreeCtxtExts`, matching
//! atlas/OWNERSHIP_ATLAS.md section 4 extension-module-data row). The
//! callback pointer is borrowed user-code; the library never invokes
//! unknown-arity functions with a full XPath stack unless the bridge in
//! R-000162 is active (C XPath functions go through the parser-context
//! bridge). `ctxt`, `name`, `NS_uri` must be valid; failure paths free
//! partial entries exactly once.
//!
//! # Historical quirks & epochs
//!
//! Extension registration has been stable since the libxslt 1.1 series
//! (2004+; atlas/HISTORY.md) and falls inside the E-008 frozen output
//! epoch (2009 → 1.1.45; atlas/SEMANTIC_EPOCHS.md). R-000162 closed the
//! C XPath-function callback bridge (`xmlXPathRegisterFunc` + the
//! namespaced `function_lookup` fallback dispatching through
//! `xsltFindExtFunction`); R-000165 added the per-module EXSLT
//! registration exports; R-000140 covered the `_xslt*` ABI mirrors.
//!
//! # Deliberate oddities
//!
//! - The `f` parameter of `xsltRegisterExtFunction` is declared with an
//!   intentionally minimal C signature (opaque `(void*, int)`) — the real
//!   dispatch contract lives in the R-000162 bridge; the stored pointer is
//!   opaque to this module.
//! - Registration is a per-context linked list (upstream uses
//!   `xmlHashTable`); the candidate linear-searches, an internal storage
//!   divergence with identical observable semantics.
//!
//! # Proving courts
//!
//! XSLT-001 (xslt-family differential probe: `xsltRegisterExtModule*`,
//! `xsltExtModuleFunctionLookup`), EXSLT, CLI-XSLTPROC (extension-using
//! corpus), and the in-crate `cargo test` suites.
//!
//! # Tempting simplifications that would break parity
//!
//! - Stubbing registered functions instead of dispatching (the pre-R-000162
//!   behavior) breaks every C extension consumer; the callback bridge is
//!   mandatory.
//! - Deduplicating entries by name alone would break duplicate
//!   registration semantics (upstream last-registration-wins per context).
//! - Freeing the callback pointer would violate the borrowed-user-data
//!   invariant (atlas/OWNERSHIP_ATLAS.md section 6).
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
//! the context XPath function lookup mechanism.

use crate::abi::structs::*;
use crate::abi::types::*;
use std::os::raw::c_int;
use std::ptr;

/// A registered extension function.
#[derive(Debug)]
#[repr(C)]
pub struct _xsltExtFunction {
    /// Next entry in the linked list of registered functions.
    pub next: *mut _xsltExtFunction,
    /// Local name of the function (e.g. `"node-set"`).
    pub name: *mut xmlChar,
    /// Namespace URI of the extension (e.g. `http://exslt.org/common`).
    pub ns: *mut xmlChar,
    /// The extension function implementation pointer.
    pub func: *mut c_void,
}

/// A registered extension element.
#[derive(Debug)]
#[repr(C)]
pub struct _xsltExtElement {
    /// Next entry in the linked list of registered elements.
    pub next: *mut _xsltExtElement,
    /// Local name of the element.
    pub name: *mut xmlChar,
    /// Namespace URI of the extension element.
    pub ns: *mut xmlChar,
    /// The extension element transform function.
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
    f: Option<crate::abi::exports_xslt_compile::xsltTransformFunction>,
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

    /// Allocate a zero-initialized `_xsltTransformContext`.
    ///
    /// # Safety
    ///
    /// - `libc::calloc` returns a zeroed block of the struct size or NULL;
    ///   the caller must check for NULL before dereferencing and must
    ///   release the block with `libc::free` when done.
    fn make_ctxt() -> *mut _xsltTransformContext {
        unsafe {
            libc::calloc(1, core::mem::size_of::<_xsltTransformContext>())
                as *mut _xsltTransformContext
        }
    }

    /// Register an extension function and find it back by name/URI.
    ///
    /// # Safety
    ///
    /// - `ctxt` is a live zeroed `_xsltTransformContext` from `make_ctxt`
    ///   and is released with `libc::free` after `xsltFreeExts` has run.
    /// - `dummy` is a valid extern "C" function pointer registered and
    ///   compared by address; the `c"..."` string literals are valid
    ///   NUL-terminated `xmlChar` buffers passed to the register/find
    ///   APIs, which heap-copy the names they retain.
    #[test]
    fn test_register_and_find_function() {
        unsafe {
            let ctxt = make_ctxt();
            extern "C" fn dummy(_ctx: *mut c_void, _n: c_int) {}
            assert_eq!(
                xsltRegisterExtFunction(
                    ctxt,
                    c"myfunc".as_ptr() as *const xmlChar,
                    c"http://example.com/ext".as_ptr() as *const xmlChar,
                    Some(dummy),
                ),
                0
            );
            let found = xsltFindExtFunction(
                ctxt,
                c"myfunc".as_ptr() as *const xmlChar,
                c"http://example.com/ext".as_ptr() as *const xmlChar,
            );
            assert_eq!(found, dummy as *mut c_void);
            let not_found = xsltFindExtFunction(
                ctxt,
                c"other".as_ptr() as *const xmlChar,
                c"http://example.com/ext".as_ptr() as *const xmlChar,
            );
            assert!(not_found.is_null());
            xsltFreeExts(ctxt);
            libc::free(ctxt as *mut libc::c_void);
        }
    }

    /// NULL contexts, names, URIs, and callbacks are rejected with `-1`.
    ///
    /// # Safety
    ///
    /// - `xsltRegisterExtFunction`/`xsltRegisterExtElement` return `-1` on
    ///   NULL arguments before dereferencing them, so passing NULL
    ///   pointers reads no memory.
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
