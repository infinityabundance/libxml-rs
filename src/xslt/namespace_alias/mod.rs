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
//!
//! # Upstream contract
//!
//! Parity target: upstream libxslt `namespaces.c` + `preproc.c` (1.1.45;
//! `SRC-LIBXSLT-1.1.42-NAMESPACES-C` under oracle/historical/src).
//! Subsystem census: xslt-compilation, xslt-namespace-alias. Behavior is
//! governed by XSLT 1.0 namespace-alias semantics (W3C-XSLT-1.0).
//!
//! # Conceptual behavior
//!
//! Compilation records each `xsl:namespace-alias stylesheet-prefix=
//! result-prefix` pair: the stylesheet-side URI is the key and the
//! result-side URI the value. When result elements/attributes are emitted,
//! any namespace declaration whose URI matches a stylesheet-side key is
//! emitted under the mapped result URI, so literal XSLT-namespace output
//! becomes a plain result-namespace element.
//!
//! # Ownership & safety invariants
//!
//! Each `_xsltNsAlias` is heap-allocated, owns duplicated
//! `resultNs`/`styleNs` strings, and is owned by the stylesheet `nsAliases`
//! chain (freed by `xsltFreeNsAlias` from `xsltFreeStylesheet`). On
//! allocation failure the partial entry and its copied strings are freed
//! exactly once. Aliases are duplicated from the stylesheet document so
//! they outlive the document (documents may be freed independently).
//!
//! # Historical quirks & epochs
//!
//! Namespace aliasing has been part of libxslt since the 1.1 series
//! (2004+; atlas/HISTORY.md) and sits inside the E-008 frozen epoch
//! (2009 → 1.1.45; atlas/SEMANTIC_EPOCHS.md): the alias application is
//! byte-identical across all oracle versions. The candidate keeps the
//! upstream hash-key semantics (stylesheet-side URI) in a linked list
//! storage.
//!
//! # Deliberate oddities
//!
//! - Upstream stores aliases in an `xmlHashTable`; the candidate uses a
//!   linked list in the same `nsAliases` slot (linear lookup), a
//!   documented storage divergence with identical observable semantics.
//! - Result-URI and stylesheet-URI are both duplicated, even though the
//!   stylesheet-side URI usually lives in the stylesheet document — a
//!   defensive choice that keeps alias lifetime independent of the doc.
//!
//! # Proving courts
//!
//! CLI-XSLTPROC (stylesheets that emit stylesheets via namespace-alias),
//! XSLT-001, and the in-crate `cargo test` suites.
//!
//! # Tempting simplifications that would break parity
//!
//! - Dropping the result-side mapping (emitting the stylesheet URI
//!   verbatim) breaks literal-result-element output in the XSLT
//!   namespace — the exact use case namespace-alias exists for.
//! - Borrowing the URI strings instead of duplicating them breaks when
//!   the stylesheet document is freed before the aliases are consumed.
//! - Freeing the borrowed `inst`-style pointers would double-free
//!   stylesheet-document nodes (R-000103 lesson).

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

    /// Allocate a zero-initialized `_xsltStylesheet`.
    ///
    /// # Safety
    ///
    /// - `libc::calloc` returns a zeroed block of the struct size or NULL;
    ///   the caller must check for NULL before dereferencing and must
    ///   release the block with `libc::free` when done.
    fn make_style() -> *mut _xsltStylesheet {
        unsafe { libc::calloc(1, core::mem::size_of::<_xsltStylesheet>()) as *mut _xsltStylesheet }
    }

    /// Register a namespace alias and resolve it back, checking the
    /// returned URI strings.
    ///
    /// # Safety
    ///
    /// - `style` is a live zeroed `_xsltStylesheet` from `make_style`; the
    ///   `c"..."` URI literals are valid NUL-terminated `xmlChar` buffers
    ///   passed to `xsltAddNsAlias`/`xsltResolveNsAlias`.
    /// - `resolved`/`other` are NULL or valid NUL-terminated strings owned
    ///   by `style` (or aliases of the input literals), so `strcmp` is
    ///   bounded; the alias lists are freed with `xsltFreeNsAliases`
    ///   before `libc::free(style)`.
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

    /// NULL arguments to the alias API are rejected without crashing.
    ///
    /// # Safety
    ///
    /// - `xsltAddNsAlias` returns `-1` on NULL `style`/`resultURI`/
    ///   `styleURI` before dereferencing them, and `xsltFreeNsAlias`/
    ///   `xsltFreeNsAliases` no-op on NULL, so the unsafe block reads and
    ///   frees no memory.
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
