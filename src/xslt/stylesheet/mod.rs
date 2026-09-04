//! XSLT stylesheet representation and lifecycle (§32, §85 Phase 8).
//!
//! # Upstream contract
//!
//! Parity target: upstream libxslt `xslt.c` + `xsltInternals.h` (1.1.45;
//! `SRC-LIBXSLT-1.1.42-XSLT-C` under oracle/historical/src). Subsystem
//! census: xslt-lifecycle, xslt-includes, xslt-priorities, xslt-sorting,
//! xslt-whitespace, xslt-transform-ctxt, xslt-global-state, xslt-exports.
//! ABI surface: `xsltStylesheetCreate`, `xsltParseStylesheetDoc`/`File`/
//! `Memory`, `xsltFreeStylesheet`, `xsltGetStylesheetDoc`,
//! `xsltSetStylesheetDoc`, plus the stylesheet-level globals.
//!
//! # Conceptual behavior
//!
//! `xsltStylesheetCreate` allocates a zero-initialized `_xsltStylesheet`
//! and seeds the default decimal format at the head of the chain
//! (upstream `xsltNewStylesheetInternal`, xslt.c). Parsing hands the
//! document to the compiler module; teardown frees imports first, then
//! templates, keys, aliases, attribute sets, strip/preserve rules,
//! decimal formats, the internal document, and the structure itself.
//!
//! # Ownership & safety invariants
//!
//! The stylesheet owns its style documents and every compiled definition;
//! `xsltFreeStylesheet` tears down the whole graph (atlas/OWNERSHIP_ATLAS.md
//! section 4). `xsltGetStylesheetDoc` returns a borrowed pointer (never
//! free); `xsltSetStylesheetDoc` transfers document ownership into the
//! stylesheet, freeing any previously held document. The returned
//! `xsltStylesheetCreate` pointer is caller-owned and must be freed
//! exactly once with `xsltFreeStylesheet`.
//!
//! # Historical quirks & epochs
//!
//! E-008 (atlas/SEMANTIC_EPOCHS.md): the stylesheet lifecycle decisions
//! (default decimal format, omit/standalone/indent defaults of -1) sit
//! inside the byte-identical xsltproc epoch (1.1.26, 2009, through
//! 1.1.45). R-000140 covered the eight `_xslt*` ABI mirrors; the default
//! decimal-format seeding predates the numbering module port (R-000166).
//!
//! # Deliberate oddities
//! - `omitXmlDeclaration`/`standalone`/`indent` default to -1 (unset),
//!   matching upstream `xsltNewStylesheetInternal`; the serialization
//!   module treats -1 distinctly from 0.
//! - The heavy lifting lives in `super::compiler`; this module is the ABI
//!   facade — an intentional module split.
//!
//! # Proving courts
//!
//! XSLT, CLI-XSLTPROC, EXSLT, ORACLE-IDENTITY, PREPROCESSOR-SURFACE,
//! BUILD-CONFIG-SCRIPT, HEADER-COMPILE (stylesheets compile against the
//! public headers), and the in-crate `cargo test` suites.
//!
//! # Tempting simplifications that would break parity
//!
//! - Defaulting the decimal format fields to zero instead of seeding the
//!   default format breaks format-number without an explicit
//!   xsl:decimal-format (R-000166).
//! - Freeing the internal document before the definitions that borrow it
//!   reintroduces the R-000103/R-000109 double-free family.
//! - Inlining the compiler here would duplicate the compile pipeline and
//!   break the import/priority ordering guarantees.
//!
//! Implements stylesheet creation, parsing, and destruction.
//!
//! # Architecture
//!
//! This module provides the C ABI entry points for stylesheet lifecycle
//! management. The heavy lifting — XSLT element recognition, template
//! compilation, import resolution, output definition parsing, namespace
//! alias processing, key extraction, variable/parameter collection, and
//! whitespace strip/preserve building — lives in `super::compiler`.
//!
//! # Ownership model
//!
//! - `xsltStylesheetCreate` allocates a zero-initialized stylesheet.
//! - `xsltParseStylesheetDoc` takes ownership of the document pointer
//!   (the doc is moved into the stylesheet's `doc` field).
//! - `xsltFreeStylesheet` frees the stylesheet and all owned resources,
//!   including the internal document (via `xmlFreeDoc`).
//! - `xsltGetStylesheetDoc` returns a borrowed pointer (do not free).
//! - `xsltSetStylesheetDoc` transfers document ownership into the
//!   stylesheet, freeing any previously held document.

use crate::abi::exports_xml2::{xmlReadFile, xmlReadMemory};
use crate::abi::structs::*;
use crate::abi::types::xmlElementType::*;
use crate::abi::types::*;
use crate::xml::tree::free_doc;
use std::os::raw::{c_char, c_int};
use std::ptr;

/// XSLT namespace URI constant.
///
/// `"http://www.w3.org/1999/XSL/Transform"` — the standard XSLT 1.0
/// namespace. Used to identify XSLT instruction elements during
/// stylesheet compilation.
pub const XSLT_NAMESPACE: &[u8] = b"http://www.w3.org/1999/XSL/Transform\0";

/// Create an empty stylesheet.
///
/// Allocates a zero-initialized `_xsltStylesheet` and returns a pointer
/// to it. The caller owns the returned pointer and must eventually free
/// it with [`xsltFreeStylesheet`].
///
/// Returns `NULL` on allocation failure.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xsltStylesheetPtr xsltStylesheetCreate(void);
/// ```
///
/// # Safety
///
/// The caller must ensure the returned pointer is freed exactly once
/// via `xsltFreeStylesheet`. No other thread may access the stylesheet
/// during construction.
#[no_mangle]
pub unsafe extern "C" fn xsltStylesheetCreate() -> *mut _xsltStylesheet {
    let style = libc::calloc(1, core::mem::size_of::<_xsltStylesheet>()) as *mut _xsltStylesheet;
    if style.is_null() {
        return ptr::null_mut();
    }
    (*style).templates = ptr::null_mut();
    (*style).keys = ptr::null_mut();
    (*style).parent = ptr::null_mut();
    (*style).next = ptr::null_mut();
    (*style).imports = ptr::null_mut();
    (*style).omitXmlDeclaration = -1;
    (*style).standalone = -1;
    (*style).indent = -1;

    // UPSTREAM-PARITY (xslt.c): creating a stylesheet initializes the
    // library — the built-in extras (saxon:output / xalan:write /
    // xt:document extension elements) must be registered before the
    // template content is compiled and transformed (php bug54446). The
    // registration is idempotent.
    crate::abi::exports_xslt::xsltInit();
    // UPSTREAM-PARITY (xslt.c xsltNewStylesheetInternal): every stylesheet
    // carries the default decimal format at the head of the chain.
    (*style).decimalFormat =
        crate::abi::exports_xslt_compile::xslt_new_decimal_format(ptr::null(), ptr::null_mut());
    style
}

/// Free a stylesheet and all associated resources.
///
/// Frees the stylesheet's internal document (if any), its template
/// lists, key definitions, variable tables, namespace aliases,
/// attribute sets, strip/preserve-space hashes, extension info,
/// security preferences, and the stylesheet structure itself.
///
/// Safe to call with a null pointer (no-op).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltFreeStylesheet(xsltStylesheetPtr style);
/// ```
///
/// # Safety
///
/// After calling this function the pointer must not be dereferenced.
/// Calling `xsltFreeStylesheet` twice on the same pointer is undefined
/// behaviour.
#[no_mangle]
pub unsafe extern "C" fn xsltFreeStylesheet(style: *mut _xsltStylesheet) {
    if style.is_null() {
        return;
    }
    // Free imported stylesheets first (they depend on this one).
    crate::xslt::imports::xsltFreeImports(style);
    // Free templates.
    crate::xslt::templates::xsltFreeTemplates(style);
    // Free key definitions.
    crate::xslt::keys::xsltFreeKeys(style);
    // Free namespace aliases.
    crate::xslt::namespace_alias::xsltFreeNsAliases(style);
    // Free attribute sets.
    crate::xslt::attributes::xsltFreeAttrSets(style);
    // Free strip/preserve-space rules.
    crate::xslt::whitespace::xsltFreeStripSpaces(style);
    // Free global variables and parameters (both live in `variables`).
    free_stack_elems((*style).variables);
    (*style).variables = ptr::null_mut();
    // Free decimal formats.
    free_decimal_formats((*style).decimalFormat);
    (*style).decimalFormat = ptr::null_mut();
    // Free the preserve-space list (carried in nsDefs; see whitespace).
    if !(*style).nsDefs.is_null() {
        let mut cur = (*style).nsDefs as *mut crate::xslt::whitespace::_xsltStripSpace;
        while !cur.is_null() {
            let next = (*cur).next;
            crate::xslt::whitespace::xsltFreeStripSpaceEntry(cur);
            cur = next;
        }
    }
    // Free the stylesheet document.
    if !(*style).doc.is_null() {
        free_doc((*style).doc);
    }
    // Free the string fields that we own (heap-allocated by the compiler).
    free_owned_str((*style).method);
    free_owned_str((*style).methodURI);
    free_owned_str((*style).version);
    free_owned_str((*style).encoding);
    free_owned_str((*style).doctypePublic);
    free_owned_str((*style).doctypeSystem);
    free_owned_str((*style).mediaType);
    libc::free(style as *mut libc::c_void);
}

/// Free a linked list of stack elements.
///
/// # SAFETY
///
/// - `head` must be a valid linked list of `_xsltStackElem`.
unsafe fn free_stack_elems(head: *mut _xsltStackElem) {
    let mut cur = head;
    while !cur.is_null() {
        let next = (*cur).next;
        crate::xslt::variables::xsltFreeStackElem(cur);
        cur = next;
    }
}

/// Free a linked list of decimal formats.
///
/// # SAFETY
///
/// - `head` must be a valid linked list of `_xsltDecimalFormat`.
unsafe fn free_decimal_formats(head: *mut _xsltDecimalFormat) {
    let mut cur = head;
    while !cur.is_null() {
        let next = (*cur).next;
        if !(*cur).name.is_null() {
            libc::free((*cur).name as *mut libc::c_void);
        }
        free_owned_str((*cur).decimalPoint);
        free_owned_str((*cur).grouping);
        free_owned_str((*cur).infinity);
        free_owned_str((*cur).minusSign);
        free_owned_str((*cur).noNumber);
        free_owned_str((*cur).percent);
        free_owned_str((*cur).permille);
        free_owned_str((*cur).zeroDigit);
        free_owned_str((*cur).digit);
        free_owned_str((*cur).patternSeparator);
        libc::free(cur as *mut libc::c_void);
        cur = next;
    }
}

/// Free a string if it is heap-allocated (non-NULL).
///
/// # SAFETY
///
/// - `s` must be NULL or a heap-allocated NUL-terminated string.
unsafe fn free_owned_str(s: *const xmlChar) {
    if !s.is_null() {
        libc::free(s as *mut libc::c_void);
    }
}

/// Parse a stylesheet from a pre-parsed document.
///
/// The document must contain an `xsl:stylesheet` or `xsl:transform`
/// element as its root (or a literal result element if this is a
/// simplified stylesheet).
///
/// Takes ownership of the document pointer. The document is moved into
/// the returned stylesheet's `doc` field.
///
/// Returns `NULL` if the document is not a valid XSLT stylesheet or if
/// compilation fails.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xsltStylesheetPtr xsltParseStylesheetDoc(xmlDocPtr doc);
/// ```
///
/// # Safety
///
/// The caller must not use `doc` after calling this function, even if
/// the function returns `NULL` (ownership is consumed either way in the
/// full implementation).
#[no_mangle]
pub unsafe extern "C" fn xsltParseStylesheetDoc(doc: *mut _xmlDoc) -> *mut _xsltStylesheet {
    if doc.is_null() {
        return ptr::null_mut();
    }
    let style = xsltStylesheetCreate();
    if style.is_null() {
        return ptr::null_mut();
    }
    // Compile the stylesheet (the compiler stores doc in style->doc).
    let ret = crate::xslt::compiler::compile(style, doc);
    if ret != 0 {
        // UPSTREAM-PARITY (xslt.c xsltParseStylesheetDoc): on failure the
        // stylesheet is freed but the DOCUMENT IS NOT — the caller (e.g.
        // PHP's XSLTProcessor::importStylesheet) keeps ownership of the
        // clone and releases it itself.
        (*style).doc = ptr::null_mut();
        xsltFreeStylesheet(style);
        return ptr::null_mut();
    }
    style
}

/// Parse a stylesheet from a file path.
///
/// Opens, parses, and compiles the XSLT stylesheet at the given file
/// path. Returns a fully compiled stylesheet ready for transformation.
///
/// Returns `NULL` if the file cannot be read, parsed, or is not a
/// valid XSLT stylesheet.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xsltStylesheetPtr xsltParseStylesheetFile(const xmlChar *filename);
/// ```
///
/// # Safety
///
/// `filename` must be a null-terminated UTF-8 string or `NULL` (which
/// returns `NULL`). The caller owns the returned stylesheet and must
/// free it with `xsltFreeStylesheet`.
#[no_mangle]
pub unsafe extern "C" fn xsltParseStylesheetFile(filename: *const xmlChar) -> *mut _xsltStylesheet {
    if filename.is_null() {
        return ptr::null_mut();
    }
    // Parse the file with options that preserve the stylesheet tree.
    // XML_PARSE_NOENT | XML_PARSE_DTDLOAD | XML_PARSE_NONET
    let options: c_int = (1 << 1) | (1 << 2) | (1 << 11);
    let doc = xmlReadFile(filename as *const c_char, ptr::null(), options);
    if doc.is_null() {
        return ptr::null_mut();
    }
    let style = xsltParseStylesheetDoc(doc);
    if style.is_null() {
        // The document was parsed here, so this path owns it: on failure
        // xsltParseStylesheetDoc leaves the doc untouched (the caller
        // releases it), so free it here.
        free_doc(doc);
        return ptr::null_mut();
    }
    style
}

/// Parse the `xml-stylesheet` processing instruction value and extract the
/// `type` and `href` pseudo-attributes (upstream `xsltParseStylesheetPI`,
/// xslt.c 1.1.45).
///
/// Returns the href when the type is `text/xml`, `text/xsl` or
/// `application/xslt+xml`; NULL otherwise.
unsafe fn parse_stylesheet_pi(value: *const xmlChar) -> *mut xmlChar {
    if value.is_null() {
        return ptr::null_mut();
    }
    let bytes = crate::abi::versioning::c_str_to_bytes(value as *const c_char).unwrap_or(b"");
    let s = String::from_utf8_lossy(bytes);
    let mut is_xml = false;
    let mut href: Option<String> = None;
    let mut cur = 0usize;
    let b = s.as_bytes();
    while cur < b.len() {
        // Skip blanks.
        while cur < b.len() && (b[cur] == b' ' || b[cur] == b'\t' || b[cur] == b'\n') {
            cur += 1;
        }
        if cur >= b.len() {
            break;
        }
        // token = value
        let start = cur;
        while cur < b.len() && b[cur] != b'=' && b[cur] != b' ' && b[cur] != b'\t' {
            cur += 1;
        }
        let token = &s[start..cur];
        while cur < b.len() && (b[cur] == b' ' || b[cur] == b'\t') {
            cur += 1;
        }
        if cur >= b.len() || b[cur] != b'=' {
            continue;
        }
        cur += 1;
        while cur < b.len() && (b[cur] == b' ' || b[cur] == b'\t') {
            cur += 1;
        }
        if cur >= b.len() || (b[cur] != b'\'' && b[cur] != b'"') {
            continue;
        }
        let quote = b[cur];
        cur += 1;
        let vstart = cur;
        while cur < b.len() && b[cur] != quote {
            cur += 1;
        }
        let val = s[vstart..cur].to_string();
        if cur < b.len() {
            cur += 1;
        }
        if token.eq_ignore_ascii_case("type") {
            if val.eq_ignore_ascii_case("text/xml")
                || val.eq_ignore_ascii_case("text/xsl")
                || val.eq_ignore_ascii_case("application/xslt+xml")
            {
                is_xml = true;
            } else {
                return ptr::null_mut();
            }
        } else if token.eq_ignore_ascii_case("href") && href.is_none() {
            href = Some(val);
        }
    }
    if !is_xml {
        return ptr::null_mut();
    }
    match href {
        Some(h) => {
            let mut v = h.into_bytes();
            v.push(0);
            let p = libc::malloc(v.len()) as *mut xmlChar;
            if !p.is_null() {
                libc::memcpy(
                    p as *mut libc::c_void,
                    v.as_ptr() as *const libc::c_void,
                    v.len(),
                );
            }
            p
        }
        None => ptr::null_mut(),
    }
}

/// Load a stylesheet referenced by an `xml-stylesheet` PI in the document
/// (upstream `xsltLoadStylesheetPI`, xslt.c 1.1.45).
///
/// Only the external-reference case is supported; embedded stylesheets
/// (fragment identifiers) return NULL — see RESIDUAL R-XSLT-EMBEDDED-PI.
///
/// # Safety
///
/// `doc` must be a valid document pointer.
#[no_mangle]
pub unsafe extern "C" fn xsltLoadStylesheetPI(doc: *mut _xmlDoc) -> *mut _xsltStylesheet {
    if doc.is_null() {
        return ptr::null_mut();
    }
    let mut child = (*doc).children;
    let mut href: *mut xmlChar = ptr::null_mut();
    while !child.is_null() && (*child).type_ != XML_ELEMENT_NODE as c_int {
        if (*child).type_ == XML_PI_NODE as c_int
            && !(*child).name.is_null()
            && crate::abi::versioning::c_str_to_bytes((*child).name as *const c_char).unwrap_or(b"")
                == b"xml-stylesheet"
        {
            let h = parse_stylesheet_pi((*child).content);
            if !h.is_null() {
                href = h;
                break;
            }
        }
        child = (*child).next;
    }
    if href.is_null() {
        return ptr::null_mut();
    }
    // Only the external href case is supported.
    let href_bytes = crate::abi::versioning::c_str_to_bytes(href as *const c_char)
        .unwrap_or(b"")
        .to_vec();
    libc::free(href as *mut libc::c_void);
    if href_bytes.starts_with(b"#") {
        return ptr::null_mut();
    }
    let mut url = href_bytes;
    url.push(0);

    xsltParseStylesheetFile(url.as_ptr() as *const xmlChar)
}

/// Parse a stylesheet from a memory buffer.
///
/// Parses and compiles the XSLT stylesheet from a byte buffer in
/// memory. `URL` is an optional base URI for error reporting and
/// relative URI resolution.
///
/// Returns `NULL` if the buffer is invalid, cannot be parsed, or is
/// not a valid XSLT stylesheet.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xsltStylesheetPtr xsltParseStylesheetMemory(const char *buf, int len,
///                                              const char *URL);
/// ```
///
/// # Safety
///
/// `buf` must point to at least `len` readable bytes. `URL` may be
/// `NULL`. The caller owns the returned stylesheet and must free it
/// with `xsltFreeStylesheet`.
#[no_mangle]
pub unsafe extern "C" fn xsltParseStylesheetMemory(
    buf: *const c_char,
    len: c_int,
    URL: *const c_char,
) -> *mut _xsltStylesheet {
    if buf.is_null() || len <= 0 {
        return ptr::null_mut();
    }
    let options: c_int = (1 << 1) | (1 << 2) | (1 << 11);
    let doc = xmlReadMemory(buf, len, URL, ptr::null(), options);
    if doc.is_null() {
        return ptr::null_mut();
    }
    let style = xsltParseStylesheetDoc(doc);
    if style.is_null() {
        // The document was parsed here, so this path owns it; on failure
        // xsltParseStylesheetDoc leaves the doc untouched.
        free_doc(doc);
        return ptr::null_mut();
    }
    style
}

/// Get the stylesheet's document.
///
/// Returns a borrowed pointer to the internal document. The caller
/// must **not** free this pointer — the document is owned by the
/// stylesheet and will be freed when `xsltFreeStylesheet` is called.
///
/// Returns `NULL` if `style` is null or has no associated document.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDocPtr xsltGetStylesheetDoc(xsltStylesheetPtr style);
/// ```
///
/// # Safety
///
/// The returned pointer is valid only as long as `style` is alive.
/// Dereferencing after `xsltFreeStylesheet` is undefined behaviour.
#[no_mangle]
pub unsafe extern "C" fn xsltGetStylesheetDoc(style: *mut _xsltStylesheet) -> *mut _xmlDoc {
    if style.is_null() {
        return ptr::null_mut();
    }
    (*style).doc
}

/// Set the stylesheet's document.
///
/// Transfers ownership of `doc` into the stylesheet. If the stylesheet
/// already has a document, the previous document is freed first.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltSetStylesheetDoc(xsltStylesheetPtr style, xmlDocPtr doc);
/// ```
///
/// # Safety
///
/// After calling this function, `doc` must not be used or freed by the
/// caller. The stylesheet takes ownership.
#[no_mangle]
pub unsafe extern "C" fn xsltSetStylesheetDoc(style: *mut _xsltStylesheet, doc: *mut _xmlDoc) {
    if style.is_null() {
        return;
    }
    if !(*style).doc.is_null() && (*style).doc != doc {
        free_doc((*style).doc);
    }
    (*style).doc = doc;
}

#[cfg(test)]
mod tests {
    use super::*;

    use core::ptr;

    /// Create and free a stylesheet, checking the default field values.
    ///
    /// # Safety
    ///
    /// - `style` is a valid `_xsltStylesheet` returned by
    ///   `xsltStylesheetCreate` (asserted non-NULL) and not yet freed, so
    ///   reading `templates`/`omitXmlDeclaration` and passing it to
    ///   `xsltFreeStylesheet` are valid.
    #[test]
    fn test_create_free_stylesheet() {
        unsafe {
            let style = xsltStylesheetCreate();
            assert!(!style.is_null());
            assert!((*style).templates.is_null());
            assert_eq!((*style).omitXmlDeclaration, -1);
            xsltFreeStylesheet(style);
        }
    }

    /// Freeing a NULL stylesheet is a no-op.
    ///
    /// # Safety
    ///
    /// - `xsltFreeStylesheet` returns early on a NULL pointer before
    ///   dereferencing, so passing `ptr::null_mut()` reads and frees
    ///   nothing.
    #[test]
    fn test_free_null() {
        unsafe {
            xsltFreeStylesheet(ptr::null_mut());
        }
    }

    /// Parse a stylesheet from an in-memory buffer and verify the compiled
    /// document and templates list.
    ///
    /// # Safety
    ///
    /// - The `xsl` byte string is a valid NUL-terminated buffer passed to
    ///   `xsltParseStylesheetMemory`, which returns a valid stylesheet
    ///   (asserted non-NULL) or NULL; `xsltFreeStylesheet` releases the
    ///   stylesheet and the document it owns.
    /// - The `templates` and `doc` fields are only read after the
    ///   non-NULL assertions while the stylesheet is still live.
    #[test]
    fn test_parse_stylesheet_memory() {
        unsafe {
            let xsl = b"<?xml version=\"1.0\"?>\n<xsl:stylesheet version=\"1.0\" xmlns:xsl=\"http://www.w3.org/1999/XSL/Transform\">\n<xsl:template match=\"/\"><html/></xsl:template>\n</xsl:stylesheet>\0";
            let style = xsltParseStylesheetMemory(
                xsl.as_ptr() as *const c_char,
                (xsl.len() - 1) as c_int,
                ptr::null(),
            );
            assert!(!style.is_null());
            assert!(!(*style).doc.is_null());
            // The templates list should contain one template.
            assert!(!(*style).templates.is_null());
            xsltFreeStylesheet(style);
        }
    }

    /// NULL inputs to the parse entry points are rejected with NULL.
    ///
    /// # Safety
    ///
    /// - `xsltParseStylesheetMemory`, `xsltParseStylesheetFile`,
    ///   `xsltParseStylesheetDoc`, and `xsltGetStylesheetDoc` all return
    ///   early on NULL arguments before dereferencing them, so passing
    ///   `ptr::null()`/`ptr::null_mut()` reads no memory.
    #[test]
    fn test_parse_stylesheet_null() {
        unsafe {
            assert!(xsltParseStylesheetMemory(ptr::null(), 0, ptr::null()).is_null());
            assert!(xsltParseStylesheetFile(ptr::null()).is_null());
            assert!(xsltParseStylesheetDoc(ptr::null_mut()).is_null());
            assert!(xsltGetStylesheetDoc(ptr::null_mut()).is_null());
        }
    }
}
