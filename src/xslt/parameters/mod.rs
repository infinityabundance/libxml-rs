//! XSLT parameter management (§33, §85 Phase 8).
//!
//! Parameters allow passing values from the caller into the stylesheet.
//! Global parameters are set via `xsltApplyStylesheet`'s params array.
//! Local parameters are passed via `<xsl:with-param>`.
//!
//! # UPSTREAM-PARITY
//!
//! Upstream libxslt parses the `params` argument of `xsltApplyStylesheet`
//! as a NULL-terminated array of strings of the form:
//!
//! - `name=value` (no namespace)
//! - `{namespace-uri}name=value` (with namespace)
//!
//! Each parameter is converted to a string value and bound as a global
//! variable with the `XSLT_VAR_PARAM` flag.

use crate::abi::allocator::xmlFree;
use crate::abi::structs::*;
use crate::abi::types::*;
use std::ffi::c_void;
use std::os::raw::{c_char, c_int};
use std::ptr;

use super::variables::*;

/// Parse the params array from xsltApplyStylesheet.
///
/// The params array is a NULL-terminated array of C strings.
/// Each string is in the format "name=value" or "{ns}name=value".
///
/// # SAFETY
///
/// - `style` must be a valid `_xsltStylesheet`.
/// - `params` must be a NULL-terminated array of NUL-terminated C strings.
pub unsafe fn xsltParseStylesheetParams(
    style: *mut _xsltStylesheet,
    params: *mut *const c_char,
) -> c_int {
    if style.is_null() || params.is_null() {
        return -1;
    }
    let mut i = 0;
    let mut count = 0;
    while !(*params.offset(i)).is_null() {
        let param = *params.offset(i);
        let parsed = xsltParseStylesheetParam(style, param);
        if parsed.is_null() {
            i += 1;
            continue;
        }
        // Add to the stylesheet's params list.
        let p = &mut *parsed;
        p.next = (*style).params as *mut _xsltStackElem;
        (*style).params = parsed as *mut c_void;
        count += 1;
        i += 1;
    }
    count
}

/// Parse a single parameter string into a stack element.
///
/// Format: "name=value" or "{uri}name=value".
///
/// # SAFETY
///
/// - `style` must be a valid `_xsltStylesheet`.
/// - `param` must be a valid NUL-terminated C string.
pub unsafe fn xsltParseStylesheetParam(
    style: *mut _xsltStylesheet,
    param: *const c_char,
) -> *mut _xsltStackElem {
    if style.is_null() || param.is_null() {
        return ptr::null_mut();
    }
    let len = libc::strlen(param);
    let bytes = core::slice::from_raw_parts(param as *const u8, len);

    // Find the '=' separator.
    let mut eq_pos = None;
    for (idx, b) in bytes.iter().enumerate() {
        if *b == b'=' {
            eq_pos = Some(idx);
            break;
        }
    }
    let eq_pos = match eq_pos {
        Some(p) => p,
        None => return ptr::null_mut(),
    };

    // Parse name and namespace.
    let (name, ns_uri): (Vec<u8>, Option<Vec<u8>>) = {
        let name_part = &bytes[..eq_pos];
        if name_part.starts_with(b"{") {
            if let Some(close) = name_part.iter().position(|b| *b == b'}') {
                if close > 0 && close < name_part.len() - 1 {
                    let uri = name_part[1..close].to_vec();
                    let nm = name_part[close + 1..].to_vec();
                    (nm, Some(uri))
                } else {
                    (name_part.to_vec(), None)
                }
            } else {
                (name_part.to_vec(), None)
            }
        } else {
            (name_part.to_vec(), None)
        }
    };
    if name.is_empty() {
        return ptr::null_mut();
    }

    // Value: everything after '=' (may be empty).
    let value = bytes[eq_pos + 1..].to_vec();

    // Allocate the stack element.
    let v = xmlFree_alloc_stack_elem(name, ns_uri, value)
        .map(|p| p as *mut _xsltStackElem)
        .unwrap_or(ptr::null_mut());
    v
}

/// Allocate a stack element with owned (heap) name/select/value strings.
///
/// # SAFETY
///
/// Returns a heap-allocated `_xsltStackElem` with `XSLT_VAR_INTERNAL` set so
/// that `xsltFreeStackElem` will free the owned strings.
unsafe fn xmlFree_alloc_stack_elem(
    name: Vec<u8>,
    ns_uri: Option<Vec<u8>>,
    value: Vec<u8>,
) -> Option<*mut _xsltStackElem> {
    let sz = core::mem::size_of::<_xsltStackElem>();
    let v = libc::calloc(1, sz) as *mut _xsltStackElem;
    if v.is_null() {
        return None;
    }
    let cname = alloc_c_string(&name);
    if cname.is_null() {
        libc::free(v as *mut libc::c_void);
        return None;
    }
    let cselect = alloc_c_string(&value);
    if cselect.is_null() {
        libc::free(cname as *mut libc::c_void);
        libc::free(v as *mut libc::c_void);
        return None;
    }
    (*v).name = cname;
    (*v).select = cselect;
    (*v).nameURI = match ns_uri {
        Some(uri) => alloc_c_string(&uri),
        None => ptr::null(),
    };
    (*v).flags = XSLT_VAR_PARAM | XSLT_VAR_INTERNAL;
    (*v).style = ptr::null_mut();
    Some(v)
}

/// Allocate a NUL-terminated C string from bytes.
unsafe fn alloc_c_string(bytes: &[u8]) -> *mut xmlChar {
    let p = libc::malloc(bytes.len() + 1) as *mut xmlChar;
    if p.is_null() {
        return ptr::null_mut();
    }
    libc::memcpy(
        p as *mut libc::c_void,
        bytes.as_ptr() as *const libc::c_void,
        bytes.len(),
    );
    *p.add(bytes.len()) = 0;
    p
}

/// Free parsed parameters.
///
/// # SAFETY
///
/// - `params` must be a valid linked list of `_xsltStackElem`.
pub unsafe fn xsltFreeParams(params: *mut _xsltStackElem) {
    let mut cur = params;
    while !cur.is_null() {
        let next = (*cur).next;
        xsltFreeStackElem(cur);
        cur = next;
    }
}

/// Apply the with-param elements for a template invocation.
///
/// # SAFETY
///
/// - `ctxt` must be a valid `_xsltTransformContext`.
/// - `inst` must be a valid instruction node.
pub unsafe fn xsltApplyParams(
    ctxt: *mut _xsltTransformContext,
    inst: *mut _xmlNode,
    params: *mut _xsltStackElem,
) -> c_int {
    if ctxt.is_null() || inst.is_null() {
        return -1;
    }
    // Phase 8: iterate xsl:with-param children of inst, evaluate each
    // select expression, and push onto the parameter stack.
    let _ = params;
    0
}

/// Push a parameter onto the parameter stack.
///
/// # SAFETY
///
/// - `ctxt` must be a valid `_xsltTransformContext`.
/// - `param` must be a valid `_xsltStackElem`.
pub unsafe fn xsltPushParam(ctxt: *mut _xsltTransformContext, param: *mut _xsltStackElem) -> c_int {
    if ctxt.is_null() || param.is_null() {
        return -1;
    }
    let ctx = &mut *ctxt;
    let new_nr = ctx.paramsNr + 1;
    if new_nr > ctx.paramsMax {
        let new_max = if ctx.paramsMax == 0 {
            16
        } else {
            ctx.paramsMax * 2
        };
        let new_tab = libc::realloc(
            ctx.paramsTab as *mut libc::c_void,
            (new_max as usize) * core::mem::size_of::<*mut _xsltStackElem>(),
        ) as *mut *mut _xsltStackElem;
        if new_tab.is_null() {
            return -1;
        }
        ctx.paramsTab = new_tab as *mut c_void;
        ctx.paramsMax = new_max;
    }
    (*param).next = if ctx.paramsNr > 0 {
        *((ctx.paramsTab as *mut *mut _xsltStackElem).offset((ctx.paramsNr - 1) as isize))
    } else {
        ptr::null_mut()
    };
    *((ctx.paramsTab as *mut *mut _xsltStackElem).offset(ctx.paramsNr as isize)) = param;
    ctx.paramsNr = new_nr;
    0
}

/// Pop a parameter from the parameter stack.
///
/// # SAFETY
///
/// - `ctxt` must be a valid `_xsltTransformContext`.
pub unsafe fn xsltPopParam(ctxt: *mut _xsltTransformContext) -> *mut _xsltStackElem {
    if ctxt.is_null() {
        return ptr::null_mut();
    }
    let ctx = &mut *ctxt;
    if ctx.paramsNr == 0 {
        return ptr::null_mut();
    }
    ctx.paramsNr -= 1;
    let param = *((ctx.paramsTab as *mut *mut _xsltStackElem).offset(ctx.paramsNr as isize));
    (*param).next = ptr::null_mut();
    param
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::structs::*;
    use core::ptr;

    fn make_style() -> *mut _xsltStylesheet {
        unsafe {
            let s =
                libc::calloc(1, core::mem::size_of::<_xsltStylesheet>()) as *mut _xsltStylesheet;
            s
        }
    }

    #[test]
    fn test_parse_simple_param() {
        unsafe {
            let style = make_style();
            let param = b"name=value\0".as_ptr() as *const c_char;
            let elem = xsltParseStylesheetParam(style, param);
            assert!(!elem.is_null());
            assert_eq!(
                libc::strcmp(
                    (*elem).name as *const c_char,
                    b"name\0".as_ptr() as *const c_char
                ),
                0
            );
            assert_eq!(
                libc::strcmp(
                    (*elem).select as *const c_char,
                    b"value\0".as_ptr() as *const c_char
                ),
                0
            );
            assert!((*elem).nameURI.is_null());
            xsltFreeStackElem(elem);
            libc::free(style as *mut libc::c_void);
        }
    }

    #[test]
    fn test_parse_ns_param() {
        unsafe {
            let style = make_style();
            let param = b"{http://example.com/ns}pname=pvalue\0".as_ptr() as *const c_char;
            let elem = xsltParseStylesheetParam(style, param);
            assert!(!elem.is_null());
            assert_eq!(
                libc::strcmp(
                    (*elem).name as *const c_char,
                    b"pname\0".as_ptr() as *const c_char
                ),
                0
            );
            assert!(!(*elem).nameURI.is_null());
            assert_eq!(
                libc::strcmp(
                    (*elem).nameURI as *const c_char,
                    b"http://example.com/ns\0".as_ptr() as *const c_char
                ),
                0
            );
            xsltFreeStackElem(elem);
            libc::free(style as *mut libc::c_void);
        }
    }

    #[test]
    fn test_parse_empty_value() {
        unsafe {
            let style = make_style();
            let param = b"name=\0".as_ptr() as *const c_char;
            let elem = xsltParseStylesheetParam(style, param);
            assert!(!elem.is_null());
            assert_eq!(
                libc::strcmp(
                    (*elem).select as *const c_char,
                    b"\0".as_ptr() as *const c_char
                ),
                0
            );
            xsltFreeStackElem(elem);
            libc::free(style as *mut libc::c_void);
        }
    }

    #[test]
    fn test_parse_invalid_param() {
        unsafe {
            let style = make_style();
            // No '=' separator.
            let param = b"noequals\0".as_ptr() as *const c_char;
            assert!(xsltParseStylesheetParam(style, param).is_null());
            // Null pointer.
            assert!(xsltParseStylesheetParam(style, ptr::null()).is_null());
            libc::free(style as *mut libc::c_void);
        }
    }
}
