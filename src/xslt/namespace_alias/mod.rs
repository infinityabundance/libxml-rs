//! XSLT namespace aliasing (§33, §85 Phase 8).
//!
//! `<xsl:namespace-alias>` maps a namespace URI in the stylesheet to a
//! different namespace URI in the result. This allows stylesheets to
//! generate elements in the XSLT namespace itself (e.g., generating
//! stylesheets as output).
//!
//! # UPSTREAM-PARITY
//!
//! Upstream libxslt (namespaces.c) stores namespace aliases in a hash on
//! the stylesheet (`nsAliases`), keyed by the stylesheet-side namespace
//! URI. When copying namespace declarations to the result tree, the alias
//! mapping is applied: a namespace declared with the stylesheet URI is
//! emitted with the result URI instead.

use crate::abi::allocator::xmlFreeImpl;
use crate::abi::structs::*;
use crate::abi::types::*;
use std::os::raw::c_int;
use std::ptr;

/// Add a namespace alias to the stylesheet.
///
/// # SAFETY
///
/// - `style` must be a valid `_xsltStylesheet`.
/// - `result_ns` and `style_ns` must be valid NUL-terminated strings.
pub unsafe fn xsltAddNsAlias(
    style: *mut _xsltStylesheet,
    result_ns: *const xmlChar,
    style_ns: *const xmlChar,
) -> c_int {
    if style.is_null() || result_ns.is_null() || style_ns.is_null() {
        return -1;
    }
    let alias = libc::calloc(1, core::mem::size_of::<_xsltNsAlias>()) as *mut _xsltNsAlias;
    if alias.is_null() {
        return -1;
    }
    // Duplicate the strings (they may come from the stylesheet document).
    let rlen = libc::strlen(result_ns as *const libc::c_char);
    let rcopy = libc::malloc(rlen + 1) as *mut xmlChar;
    if rcopy.is_null() {
        xmlFreeImpl(alias as *mut libc::c_void);
        return -1;
    }
    libc::memcpy(
        rcopy as *mut libc::c_void,
        result_ns as *const libc::c_void,
        rlen,
    );
    *rcopy.add(rlen) = 0;
    let slen = libc::strlen(style_ns as *const libc::c_char);
    let scopy = libc::malloc(slen + 1) as *mut xmlChar;
    if scopy.is_null() {
        libc::free(rcopy as *mut libc::c_void);
        xmlFreeImpl(alias as *mut libc::c_void);
        return -1;
    }
    libc::memcpy(
        scopy as *mut libc::c_void,
        style_ns as *const libc::c_void,
        slen,
    );
    *scopy.add(slen) = 0;

    (*alias).resultNs = rcopy;
    (*alias).styleNs = scopy;
    // Prepend to the stylesheet's alias chain.
    (*alias).next = (*style).nsAliases as *mut _xsltNsAlias;
    (*style).nsAliases = alias as *mut c_void;
    0
}

/// Free a namespace alias.
///
/// # SAFETY
///
/// - `alias` must be a valid `_xsltNsAlias` allocated by this library.
pub unsafe fn xsltFreeNsAlias(alias: *mut _xsltNsAlias) {
    if alias.is_null() {
        return;
    }
    if !(*alias).resultNs.is_null() {
        libc::free((*alias).resultNs as *mut libc::c_void);
    }
    if !(*alias).styleNs.is_null() {
        libc::free((*alias).styleNs as *mut libc::c_void);
    }
    (*alias).next = ptr::null_mut();
    xmlFreeImpl(alias as *mut libc::c_void);
}

/// Free all namespace aliases in a stylesheet.
///
/// # SAFETY
///
/// - `style` must be a valid `_xsltStylesheet`.
pub unsafe fn xsltFreeNsAliases(style: *mut _xsltStylesheet) {
    if style.is_null() {
        return;
    }
    let mut cur = (*style).nsAliases as *mut _xsltNsAlias;
    (*style).nsAliases = ptr::null_mut();
    while !cur.is_null() {
        let next = (*cur).next;
        xsltFreeNsAlias(cur);
        cur = next;
    }
}

/// Resolve a stylesheet namespace URI to its result URI.
///
/// Returns the aliased (result) URI if an alias exists, otherwise returns
/// the input URI unchanged.
///
/// # SAFETY
///
/// - `style` must be a valid `_xsltStylesheet`.
/// - `uri` must be a valid NUL-terminated string.
pub unsafe fn xsltResolveNsAlias(
    style: *mut _xsltStylesheet,
    uri: *const xmlChar,
) -> *const xmlChar {
    if style.is_null() || uri.is_null() {
        return uri;
    }
    let mut cur = (*style).nsAliases as *mut _xsltNsAlias;
    while !cur.is_null() {
        if !(*cur).styleNs.is_null()
            && libc::strcmp(
                (*cur).styleNs as *const libc::c_char,
                uri as *const libc::c_char,
            ) == 0
        {
            return (*cur).resultNs;
        }
        cur = (*cur).next;
    }
    uri
}

use std::ffi::c_void;

#[cfg(test)]
mod tests {
    use super::*;
    use core::ptr;

    fn make_style() -> *mut _xsltStylesheet {
        unsafe { libc::calloc(1, core::mem::size_of::<_xsltStylesheet>()) as *mut _xsltStylesheet }
    }

    #[test]
    fn test_add_and_resolve_alias() {
        unsafe {
            let style = make_style();
            assert_eq!(
                xsltAddNsAlias(
                    style,
                    c"http://example.com/result".as_ptr() as *const xmlChar,
                    c"http://example.com/style".as_ptr() as *const xmlChar,
                ),
                0
            );
            let resolved = xsltResolveNsAlias(
                style,
                c"http://example.com/style".as_ptr() as *const xmlChar,
            );
            assert_eq!(
                libc::strcmp(
                    resolved as *const libc::c_char,
                    c"http://example.com/result".as_ptr() as *const libc::c_char
                ),
                0
            );
            // Unaliased URI passes through.
            let other = xsltResolveNsAlias(
                style,
                c"http://example.com/other".as_ptr() as *const xmlChar,
            );
            assert_eq!(
                libc::strcmp(
                    other as *const libc::c_char,
                    c"http://example.com/other".as_ptr() as *const libc::c_char
                ),
                0
            );
            xsltFreeNsAliases(style);
            libc::free(style as *mut libc::c_void);
        }
    }

    #[test]
    fn test_null_args() {
        unsafe {
            assert_eq!(
                xsltAddNsAlias(ptr::null_mut(), ptr::null(), ptr::null()),
                -1
            );
            xsltFreeNsAlias(ptr::null_mut());
            xsltFreeNsAliases(ptr::null_mut());
        }
    }
}
