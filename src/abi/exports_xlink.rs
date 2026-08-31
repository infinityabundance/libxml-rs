//! exports_xlink — XLink detection module C ABI exports (11.1-X R-000165
//! closure).
//!
//! Ported from `archaeology/libxml2-git/xlink.c` (libxml2 2.15.3). The
//! default handler/detect slots are static globals exactly like upstream;
//! `xlinkIsLink` implements the (deprecated, never-finished) detection
//! rules for XML XLinks.
//!
//! These five symbols are part of the 11.1-W parity obligations (5 xlink
//! obligations) and are declared by the drop-in `include/libxml/xlink.h`.

#![allow(
    missing_docs,
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals
)]

use core::ffi::c_void;
use core::ptr;
use std::os::raw::c_int;

use crate::abi::structs::{_xmlDoc, _xmlNode};
use crate::abi::types::xmlChar;

/// `xlinkType` (xlink.h): NONE 0, SIMPLE 1, EXTENDED 2, EXTENDED_SET 3.
const XLINK_TYPE_NONE: c_int = 0;
const XLINK_TYPE_SIMPLE: c_int = 1;
const XLINK_TYPE_EXTENDED: c_int = 2;
const XLINK_TYPE_EXTENDED_SET: c_int = 3;

/// Upstream XLINK_NAMESPACE (note: the 1999 namespace with trailing slash).
const XLINK_NS: &[u8] = b"http://www.w3.org/1999/xlink/namespace/\0";
/// Upstream XHTML_NAMESPACE.
const XHTML_NS: &[u8] = b"http://www.w3.org/1999/xhtml/\0";

/// `xlinkNodeDetectFunc` — node detection callback.
pub type xlinkNodeDetectFunc = unsafe extern "C" fn(ctx: *mut c_void, node: *mut _xmlNode);

/// `xlinkHandler` — opaque handler block (never dereferenced by the
/// candidate; stored as an opaque pointer exactly like upstream's
/// deprecated API).
#[derive(Debug)]
#[repr(C)]
pub struct _xlinkHandler {
    _private: *mut c_void,
}

/// Default handler slot (upstream `xlinkDefaultHandler`).
static mut XLINK_DEFAULT_HANDLER: *mut _xlinkHandler = ptr::null_mut();

/// Default detection routine slot (upstream `xlinkDefaultDetect`).
static mut XLINK_DEFAULT_DETECT: Option<xlinkNodeDetectFunc> = None;

/// Get the default xlink handler.
///
/// # SAFETY
///
/// The function touches crate-global state only; it is safe
/// as long as the caller respects the library's global
/// initialization/cleanup ordering (xmlInitParser before use,
/// xmlCleanupParser only after all users are done).
///
/// Violating the global lifecycle ordering, or calling this after
/// teardown or from a signal handler, is undefined behavior.
#[no_mangle]
pub unsafe extern "C" fn xlinkGetDefaultHandler() -> *mut _xlinkHandler {
    unsafe { XLINK_DEFAULT_HANDLER }
}

/// Set the default xlink handler.
///
/// # SAFETY
///
/// - `handler` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xlinkSetDefaultHandler(handler: *mut _xlinkHandler) {
    unsafe { XLINK_DEFAULT_HANDLER = handler };
}

/// Get the default xlink detection routine.
///
/// # SAFETY
///
/// The function touches crate-global state only; it is safe
/// as long as the caller respects the library's global
/// initialization/cleanup ordering (xmlInitParser before use,
/// xmlCleanupParser only after all users are done).
///
/// Violating the global lifecycle ordering, or calling this after
/// teardown or from a signal handler, is undefined behavior.
#[no_mangle]
pub unsafe extern "C" fn xlinkGetDefaultDetect() -> Option<xlinkNodeDetectFunc> {
    unsafe { XLINK_DEFAULT_DETECT }
}

/// Set the default xlink detection routine.
///
/// # SAFETY
///
///
/// - `func` must be a valid callback (or None);
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
pub unsafe extern "C" fn xlinkSetDefaultDetect(func: Option<xlinkNodeDetectFunc>) {
    unsafe { XLINK_DEFAULT_DETECT = func };
}

/// Check whether the given node carries the attributes needed to be a link
/// element (upstream xlink.c `xlinkIsLink`).
///
/// Returns the xlinkType of the node (XLINK_TYPE_NONE if no link is
/// detected).
///
/// # SAFETY
///
/// - `node` must be NULL or a valid `_xmlNode`.
/// - `doc` must be NULL or a valid `_xmlDoc`.
#[no_mangle]
pub unsafe extern "C" fn xlinkIsLink(doc: *mut _xmlDoc, node: *mut _xmlNode) -> c_int {
    unsafe {
        if node.is_null() {
            return XLINK_TYPE_NONE;
        }
        let mut doc = doc;
        if doc.is_null() {
            doc = (*node).doc;
        }
        // HTML documents and XHTML elements are handled upstream without
        // special-casing the element list (the XLink code was never
        // finished); the attribute-based detection below applies.
        let _ = XHTML_NS;

        let type_attr = crate::abi::exports_xml2::xmlGetNsProp(
            node,
            c"type".as_ptr() as *const xmlChar,
            XLINK_NS.as_ptr() as *const xmlChar,
        );
        if type_attr.is_null() {
            return XLINK_TYPE_NONE;
        }
        let mut ret = XLINK_TYPE_NONE;
        if crate::abi::exports_xml2::xmlStrEqual(type_attr, c"simple".as_ptr() as *const xmlChar)
            != 0
        {
            ret = XLINK_TYPE_SIMPLE;
        } else if crate::abi::exports_xml2::xmlStrEqual(
            type_attr,
            c"extended".as_ptr() as *const xmlChar,
        ) != 0
        {
            let role = crate::abi::exports_xml2::xmlGetNsProp(
                node,
                c"role".as_ptr() as *const xmlChar,
                XLINK_NS.as_ptr() as *const xmlChar,
            );
            if !role.is_null() {
                let xlink_ns = crate::abi::exports_xml2::xmlSearchNs(
                    doc,
                    node,
                    XLINK_NS.as_ptr() as *const xmlChar,
                );
                if xlink_ns.is_null() {
                    // Fallback method: role equals "xlink:external-linkset".
                    if crate::abi::exports_xml2::xmlStrEqual(
                        role,
                        c"xlink:external-linkset".as_ptr() as *const xmlChar,
                    ) != 0
                    {
                        ret = XLINK_TYPE_EXTENDED_SET;
                    }
                } else {
                    let prefix = (*(xlink_ns as *const crate::abi::structs::_xmlNs)).prefix;
                    let mut buf = [0u8; 200];
                    let mut n = 0;
                    if !prefix.is_null() {
                        let mut p = prefix;
                        while *p != 0 && n < buf.len() - 20 {
                            buf[n] = *p;
                            n += 1;
                            p = p.add(1);
                        }
                    }
                    let suffix = b":external-linkset\0";
                    for (i, b) in suffix.iter().enumerate() {
                        if n < buf.len() {
                            buf[n] = *b;
                            n += 1;
                        }
                        let _ = i;
                    }
                    // Compare against the constructed "prefix:external-linkset".
                    let role_bytes = crate::abi::exports_xml2::xmlStrlen(role) as usize;
                    let role_slice = core::slice::from_raw_parts(role, role_bytes);
                    let cand = &buf[..role_bytes];
                    if role_bytes <= buf.len() && role_slice == cand {
                        ret = XLINK_TYPE_EXTENDED_SET;
                    }
                }
                crate::abi::allocator::xmlFreeImpl(role as *mut c_void);
            }
            ret = XLINK_TYPE_EXTENDED;
        }
        crate::abi::allocator::xmlFreeImpl(type_attr as *mut c_void);
        ret
    }
}
