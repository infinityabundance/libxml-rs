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
    // UPSTREAM-PARITY: the params array is a NULL-terminated sequence of
    // (name, value) pairs (xsltEvalUserParams, variables.c 1.1.45), where
    // the value is an XPath expression evaluated later against the document.
    while !(*params.offset(i)).is_null() {
        let name = *params.offset(i);
        let value = if !(*params.offset(i + 1)).is_null() {
            *params.offset(i + 1)
        } else {
            break;
        };
        let parsed = xsltParseStylesheetParam(style, name, value);
        if !parsed.is_null() {
            let p = &mut *parsed;
            p.next = (*style).params as *mut _xsltStackElem;
            (*style).params = parsed as *mut c_void;
            count += 1;
        }
        i += 2;
    }
    count
}

/// Parse a single parameter (name + value) into a stack element.
///
/// The name may be a QName or of the form `{uri}name`.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xsltStackElemPtr xsltParseStylesheetParam(xsltStylesheetPtr style,
///                                           const xmlChar *name,
///                                           const xmlChar *value);
/// ```
///
/// # SAFETY
///
/// - `style` must be a valid `_xsltStylesheet`.
/// - `name` and `value` must be valid NUL-terminated C strings.
pub unsafe fn xsltParseStylesheetParam(
    style: *mut _xsltStylesheet,
    name: *const c_char,
    value: *const c_char,
) -> *mut _xsltStackElem {
    if style.is_null() || name.is_null() || value.is_null() {
        return ptr::null_mut();
    }
    let name_len = libc::strlen(name);
    let name_bytes = core::slice::from_raw_parts(name as *const u8, name_len);
    let value_len = libc::strlen(value);
    let value_bytes = core::slice::from_raw_parts(value as *const u8, value_len);

    // Parse {uri}name form.
    let (nm, ns_uri): (Vec<u8>, Option<Vec<u8>>) = {
        if name_bytes.starts_with(b"{") {
            if let Some(close) = name_bytes.iter().position(|b| *b == b'}') {
                if close > 0 && close < name_bytes.len() - 1 {
                    let uri = name_bytes[1..close].to_vec();
                    let n = name_bytes[close + 1..].to_vec();
                    (n, Some(uri))
                } else {
                    (name_bytes.to_vec(), None)
                }
            } else {
                (name_bytes.to_vec(), None)
            }
        } else {
            (name_bytes.to_vec(), None)
        }
    };
    if nm.is_empty() {
        return ptr::null_mut();
    }

    xmlFree_alloc_stack_elem(nm, ns_uri, value_bytes.to_vec())
        .map(|p| p as *mut _xsltStackElem)
        .unwrap_or(ptr::null_mut())
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

    // Register the parameter in the internal XPath context's variable hash
    // so `$name` resolves during XPath evaluation (with-param values).
    let xpath_ctxt = ctx.xpathCtxt;
    if !xpath_ctxt.is_null() && !(*param).name.is_null() {
        let internal = (*xpath_ctxt).extra as *mut crate::xml::xpath::context::XPathContext;
        if !internal.is_null() {
            let name_len = libc::strlen((*param).name as *const libc::c_char);
            let name_bytes = core::slice::from_raw_parts((*param).name, name_len);
            let name = String::from_utf8_lossy(name_bytes).into_owned();
            let value = if (*param).value.is_null() {
                crate::xml::xpath::types::XPathValue::String(String::new())
            } else {
                crate::abi::exports_xml2::object_to_xpathvalue_pub((*param).value)
            };
            (*internal).register_variable(&name, value);
        }
    }
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

    // Unregister the parameter from the internal XPath context's hash.
    if !(*param).name.is_null() {
        let xpath_ctxt = ctx.xpathCtxt;
        if !xpath_ctxt.is_null() {
            let internal = (*xpath_ctxt).extra as *mut crate::xml::xpath::context::XPathContext;
            if !internal.is_null() {
                let name_len = libc::strlen((*param).name as *const libc::c_char);
                let name_bytes = core::slice::from_raw_parts((*param).name, name_len);
                let name = String::from_utf8_lossy(name_bytes).into_owned();
                (*internal).unregister_variable(&name);
            }
        }
    }
    param
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::structs::*;
    use core::ptr;
    use std::os::raw::c_char;

    fn make_style() -> *mut _xsltStylesheet {
        unsafe {
            let s =
                libc::calloc(1, core::mem::size_of::<_xsltStylesheet>()) as *mut _xsltStylesheet;
            s
        }
    }

    fn c(s: &[u8]) -> *const c_char {
        s.as_ptr() as *const c_char
    }

    #[test]
    fn test_parse_simple_param() {
        unsafe {
            let style = make_style();
            let elem = xsltParseStylesheetParam(style, c(b"name\0"), c(b"value\0"));
            assert!(!elem.is_null());
            assert_eq!(libc::strcmp((*elem).name as *const c_char, c(b"name\0")), 0);
            assert_eq!(
                libc::strcmp((*elem).select as *const c_char, c(b"value\0")),
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
            let elem = xsltParseStylesheetParam(
                style,
                c(b"{http://example.com/ns}pname\0"),
                c(b"pvalue\0"),
            );
            assert!(!elem.is_null());
            assert_eq!(
                libc::strcmp((*elem).name as *const c_char, c(b"pname\0")),
                0
            );
            assert!(!(*elem).nameURI.is_null());
            assert_eq!(
                libc::strcmp(
                    (*elem).nameURI as *const c_char,
                    c(b"http://example.com/ns\0")
                ),
                0
            );
            assert_eq!(
                libc::strcmp((*elem).select as *const c_char, c(b"pvalue\0")),
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
            let elem = xsltParseStylesheetParam(style, c(b"name\0"), c(b"\0"));
            assert!(!elem.is_null());
            assert_eq!(libc::strcmp((*elem).select as *const c_char, c(b"\0")), 0);
            xsltFreeStackElem(elem);
            libc::free(style as *mut libc::c_void);
        }
    }

    #[test]
    fn test_parse_invalid_param() {
        unsafe {
            let style = make_style();
            // Empty name.
            assert!(xsltParseStylesheetParam(style, c(b"\0"), c(b"v\0")).is_null());
            // Null pointers.
            assert!(xsltParseStylesheetParam(style, ptr::null(), c(b"v\0")).is_null());
            assert!(xsltParseStylesheetParam(style, c(b"n\0"), ptr::null()).is_null());
            assert!(xsltParseStylesheetParam(ptr::null_mut(), c(b"n\0"), c(b"v\0")).is_null());
            libc::free(style as *mut libc::c_void);
        }
    }

    #[test]
    fn test_parse_params_array_pairs() {
        // UPSTREAM-PARITY: the params array is (name, value) pairs.
        unsafe {
            let style = make_style();
            let p1n = alloc_c_string(b"a");
            let p1v = alloc_c_string(b"'1'");
            let p2n = alloc_c_string(b"b");
            let p2v = alloc_c_string(b"'2'");
            let arr: Vec<*const c_char> = vec![
                p1n as *const c_char,
                p1v as *const c_char,
                p2n as *const c_char,
                p2v as *const c_char,
                ptr::null(),
            ];
            let count = xsltParseStylesheetParams(style, arr.as_ptr() as *mut *const c_char);
            assert_eq!(count, 2);
            libc::free(p1n as *mut libc::c_void);
            libc::free(p1v as *mut libc::c_void);
            libc::free(p2n as *mut libc::c_void);
            libc::free(p2v as *mut libc::c_void);
            libc::free(style as *mut libc::c_void);
        }
    }
}
