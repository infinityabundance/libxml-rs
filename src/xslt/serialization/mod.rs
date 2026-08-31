//! XSLT result serialization (§33, §85 Phase 8).
//!
//! # Upstream contract
//!
//! Parity target: upstream libxslt `xsltutils.c` (1.1.45;
//! `SRC-LIBXSLT-1.1.42-XSLTUTILS-C` under oracle/historical/src), the
//! `xsltSaveResultTo` family. Subsystem census: xslt-output. ABI surface:
//! `xsltSaveResultTo` / `xsltSaveResultToFilename` / `xsltSaveResultToFile` /
//! `xsltSaveResultToString` and the `xsl:output` settings they honor.
//!
//! # Conceptual behavior
//!
//! The result document is serialized honoring the stylesheet output
//! definition (method, encoding, indent, omit-xml-declaration, doctype,
//! media-type), resolved through the import chain like upstream
//! `xsltNextImport`/`XSLT_GET_IMPORT_*`. The XML declaration is written
//! explicitly (version/encoding/standalone resolution rules below); each
//! top-level child is dumped with `xmlNodeDumpOutput`-semantics and
//! newline separators are inserted only for the DTD or non-final comment
//! children when indentation is enabled.
//!
//! # Ownership & safety invariants
//!
//! `xsltSaveResultToString` hands the caller a heap buffer allocated with
//! `xmlMalloc` that the caller frees with `xmlFree` (upstream contract;
//! atlas/OWNERSHIP_ATLAS.md section 8). File/fd sinks are borrowed from
//! the caller; the result document is borrowed (never freed here).
//!
//! # Safety
//!
//! - Every `unsafe` block in this module dereferences a caller-supplied
//!   raw pointer: `result` must be NULL or a valid `_xmlDoc` whose child
//!   list, `encoding`, and `version` fields are consistent, and `style`
//!   must be NULL or a valid `_xsltStylesheet` whose import chain and
//!   output fields are initialized (see the per-function `# Safety`
//!   sections for the exact contract of each entry point).
//! - Output pointers (`doc_txt_ptr`, `doc_txt_len`, `output`, `URL`) are
//!   only dereferenced after NULL checks; the buffer written through
//!   `doc_txt_ptr` is owned by the caller and must be released with
//!   `xmlFree`, while file, fd, document, and stylesheet arguments are
//!   borrowed and never freed by this module.
//!
//! # Historical quirks & epochs
//!
//! E-008 (atlas/SEMANTIC_EPOCHS.md): xsltproc output bytes are identical
//! from libxslt 1.1.26 (2009) through 1.1.45, so the newline/declaration
//! decisions are frozen since 2009. R-000118 (Phase 9) closed the HTML
//! output-method divergence (CLI-XSLTPROC-0011); R-000140 covered the
//! `_xslt*` ABI mirrors.
//!
//! # Deliberate oddities
//!
//! - The C quirk preserved here: `indent == -1` (the default when no
//!   `indent` attribute is present) is truthy for the newline logic while
//!   element-content formatting happens only when `indent == 1`.
//! - A result document with only a DTD child (or none) is treated as
//!   empty, matching upstream `xsltSaveResultTo`.
//!
//! # Proving courts
//! CLI-XSLTPROC (output-method corpus incl. R-000118 HTML cases), XSLT-001
//! (xsltSaveResultToString via the xslt-family probe), the in-crate
//! serialization tests (incl. test_save_result_indent_quirk), and `cargo
//! test`.
//!
//! # Tempting simplifications that would break parity
//!
//! - Treating `indent == -1` as no-indent (a naive read of the attribute)
//!   changes the trailing-newline behavior that the CLI corpus pins
//!   (test_save_result_indent_quirk).
//! - Inserting separators between every top-level element changes
//!   byte-parity for multi-child results (upstream writes none).
//! - Returning the internal buffer instead of a fresh xmlMalloc copy
//!   breaks the caller-frees contract.
//!
//! Serializes the result tree to a file, fd, or string, honoring the
//! stylesheet's `<xsl:output>` settings (method, encoding, indent,
//! omit-xml-declaration, doctype, media-type).
//!
//! # UPSTREAM-PARITY
//!
//! This module is a faithful port of libxslt 1.1.45's `xsltSaveResultTo`
//! family (xsltutils.c). The XML output path is:
//!
//! 1. The XML declaration is written explicitly, with the version taken
//!    from the result document (defaulting to `1.0`), the encoding taken
//!    from the stylesheet's `<xsl:output encoding>` (falling back to the
//!    result document's encoding, then its charset), and `standalone`
//!    only when the stylesheet sets it. The declaration always ends with a
//!    newline.
//! 2. Each top-level child is serialized independently with
//!    `xmlNodeDumpOutput`-semantics (`level = 0`, `format = (indent == 1)`),
//!    i.e. no separators are inserted between top-level elements.
//! 3. A newline is written after a top-level child when indentation is
//!    enabled *and* the child is a DTD node, or a comment that is not the
//!    last child; a final newline is written after the last child.
//!
//! Note the C quirks preserved here: `indent == -1` (the default when no
//! `indent` attribute is present) is truthy for the newline logic while the
//! *formatting* of element content only happens when `indent == 1`.
//!
//! ```text
//! UPSTREAM-PARITY: xsltutils.c, xsltSaveResultTo (v1.1.45)
//! ```

use crate::abi::allocator::xmlFreeImpl;
use crate::abi::structs::*;
use crate::abi::types::xmlElementType::*;
use crate::abi::types::*;
use crate::xml::encoding;
use std::ffi::c_void;
use std::os::raw::{c_char, c_int};
use std::ptr;

/// The result document with only a DTD child (or no children at all) is
/// treated as empty by upstream `xsltSaveResultTo`.
unsafe fn result_is_empty(result: *mut _xmlDoc) -> bool {
    if result.is_null() {
        return true;
    }
    let children = (*result).children;
    if children.is_null() {
        return true;
    }
    if (*children).type_ == XML_DTD_NODE as c_int && (*children).next.is_null() {
        return true;
    }
    false
}

/// Walk the import chain like upstream `xsltNextImport`: the current
/// stylesheet, then the last of its imports, and so on.
unsafe fn next_import(style: *mut _xsltStylesheet) -> *mut _xsltStylesheet {
    if style.is_null() || (*style).imports.is_null() {
        return ptr::null_mut();
    }
    let mut imp = (*style).imports;
    while !(*imp).next.is_null() {
        imp = (*imp).next;
    }
    imp
}

/// `XSLT_GET_IMPORT_PTR`: the first non-NULL value in the import chain.
///
/// # Safety
///
/// - `style` must be NULL or a valid `_xsltStylesheet`; the function
///   walks the import chain through the `imports` and `next` links, each
///   of which must be NULL or a valid stylesheet, and dereferences every
///   stylesheet it visits.
/// - `get` must be a valid function pointer callable with a reference to
///   each stylesheet in the chain; each value it returns must be NULL or
///   a pointer to a valid NUL-terminated string, since the caller may
///   read it through `c_str_to_bytes`.
unsafe fn import_chain_str(
    style: *mut _xsltStylesheet,
    get: fn(&_xsltStylesheet) -> *const xmlChar,
) -> *const xmlChar {
    let mut s = style;
    while !s.is_null() {
        let v = get(unsafe { &*s });
        if !v.is_null() {
            return v;
        }
        s = next_import(s);
    }
    ptr::null()
}

/// `XSLT_GET_IMPORT_INT`: the first value != -1 in the import chain.
///
/// # Safety
///
/// - `style` must be NULL or a valid `_xsltStylesheet`; the function
///   walks the import chain through the `imports` and `next` links, each
///   of which must be NULL or a valid stylesheet, and dereferences every
///   stylesheet it visits.
/// - `get` must be a valid function pointer callable with a reference to
///   each stylesheet in the chain.
unsafe fn import_chain_int(
    style: *mut _xsltStylesheet,
    get: fn(&_xsltStylesheet) -> c_int,
) -> c_int {
    let mut s = style;
    while !s.is_null() {
        let v = get(unsafe { &*s });
        if v != -1 {
            return v;
        }
        s = next_import(s);
    }
    -1
}

/// Case-insensitive string comparison against a byte literal.
///
/// # Safety
///
/// - `s` must be a valid pointer to a NUL-terminated `xmlChar` buffer, or
///   NULL (NULL compares equal only to an empty literal); the scan reads
///   through `s.add(i)` until a NUL and never past the literal's length.
const unsafe fn cstr_eq_ignore_case(s: *const xmlChar, lit: &[u8]) -> bool {
    if s.is_null() {
        return lit.is_empty();
    }
    let mut i = 0usize;
    while i < lit.len() {
        let c = unsafe { *s.add(i) };
        let l = lit[i];
        if !c.eq_ignore_ascii_case(&l) {
            return false;
        }
        i += 1;
    }
    unsafe { *s.add(i) == 0 }
}

/// Serialize a result document into a UTF-8 byte vector, mirroring
/// upstream `xsltSaveResultTo` (xsltutils.c 1.1.45).
///
/// Returns `Ok(bytes)` on success and `Err(-1)` on error (matching the
/// upstream return convention).
///
/// # SAFETY
///
/// - `result` must be a valid document.
/// - `style` must be a valid stylesheet.
pub(crate) unsafe fn save_result_to_vec(
    result: *mut _xmlDoc,
    style: *mut _xsltStylesheet,
) -> Result<Vec<u8>, c_int> {
    if result.is_null() || style.is_null() {
        return Err(-1);
    }
    if result_is_empty(result) {
        return Ok(Vec::new());
    }

    // Unknown output method guard (upstream checks the *direct* fields).
    if !(*style).methodURI.is_null()
        && ((*style).method.is_null() || !cstr_eq_ignore_case((*style).method, b"xhtml"))
    {
        eprintln!("xsltSaveResultTo : unknown output method");
        return Err(-1);
    }

    let method = import_chain_str(style, |st| st.method);
    let encoding = import_chain_str(style, |st| st.encoding);
    let indent = import_chain_int(style, |st| st.indent);

    let method: Option<Vec<u8>> = if !method.is_null() {
        Some(
            crate::abi::versioning::c_str_to_bytes(method as *const c_char)
                .unwrap_or(b"")
                .to_vec(),
        )
    } else if (*result).type_ == XML_HTML_DOCUMENT_NODE as c_int {
        Some(b"html".to_vec())
    } else {
        None
    };

    let mut out: Vec<u8> = Vec::new();

    match method.as_deref() {
        Some(b"html") => {
            // htmlDocContentDumpFormatOutput equivalent. The upstream code
            // defaults indent to 1 for HTML output and inserts a meta
            // charset element; see RESIDUAL R-HTML-OUTPUT.
            let fmt = if indent != 0 { 1 } else { 0 };
            // UPSTREAM-PARITY: xsltSaveResultTo calls htmlSetMetaEncoding
            // with the stylesheet encoding (defaulting to "UTF-8") before
            // dumping; the HTML serializer reads the document's encoding to
            // emit the <meta charset> element.
            if (*result).encoding.is_null() {
                let enc: *const xmlChar = if !encoding.is_null() {
                    encoding
                } else {
                    c"UTF-8".as_ptr() as *const xmlChar
                };
                (*result).encoding =
                    crate::abi::allocator::xmlMemStrdupImpl(enc as *const c_char) as *mut xmlChar;
            }
            let buf = crate::xml::io::buf_create(-1);
            if buf.is_null() {
                return Err(-1);
            }
            crate::xml::html::serialize_node(result as *mut _xmlNode, buf, fmt, 0);
            let len = crate::xml::io::buf_length(buf);
            let content = crate::xml::io::buf_content(buf);
            if len > 0 && !content.is_null() {
                out.extend_from_slice(core::slice::from_raw_parts(content, len as usize));
            }
            crate::xml::io::buf_free(buf);
        }
        Some(b"xhtml") => {
            // Upstream uses the HTML serializer's non-formatting mode.
            let buf = crate::xml::io::buf_create(-1);
            if buf.is_null() {
                return Err(-1);
            }
            crate::xml::html::serialize_node(result as *mut _xmlNode, buf, 0, 0);
            let len = crate::xml::io::buf_length(buf);
            let content = crate::xml::io::buf_content(buf);
            if len > 0 && !content.is_null() {
                out.extend_from_slice(core::slice::from_raw_parts(content, len as usize));
            }
            crate::xml::io::buf_free(buf);
        }
        Some(b"text") => {
            // Text output: the concatenation of every text node in document
            // order, written raw (no escaping, no trailing newline).
            let mut cur = (*result).children;
            while !cur.is_null() {
                if (*cur).type_ == XML_TEXT_NODE as c_int && !(*cur).content.is_null() {
                    let len = crate::xml::tree::xml_strlen((*cur).content);
                    out.extend_from_slice(core::slice::from_raw_parts(
                        (*cur).content,
                        len as usize,
                    ));
                }
                if !(*cur).children.is_null() {
                    let ct = (*(*cur).children).type_;
                    if ct != XML_ENTITY_DECL as c_int
                        && ct != XML_ENTITY_REF_NODE as c_int
                        && ct != XML_ENTITY_NODE as c_int
                    {
                        cur = (*cur).children;
                        continue;
                    }
                }
                if !(*cur).next.is_null() {
                    cur = (*cur).next;
                    continue;
                }
                loop {
                    cur = (*cur).parent;
                    if cur.is_null() {
                        break;
                    }
                    if cur == style as *mut _xmlNode {
                        cur = ptr::null_mut();
                        break;
                    }
                    if !(*cur).next.is_null() {
                        cur = (*cur).next;
                        break;
                    }
                }
            }
        }
        _ => {
            // XML output (the default).
            let omit = import_chain_int(style, |st| st.omitXmlDeclaration);
            let standalone = import_chain_int(style, |st| st.standalone);

            if omit != 1 {
                out.extend_from_slice(b"<?xml version=\"");
                if !(*result).version.is_null() {
                    if let Some(v) =
                        crate::abi::versioning::c_str_to_bytes((*result).version as *const c_char)
                    {
                        out.extend_from_slice(v);
                    }
                } else {
                    out.extend_from_slice(b"1.0");
                }
                out.push(b'"');
                let mut enc = encoding;
                if enc.is_null() && !(*result).encoding.is_null() {
                    enc = (*result).encoding;
                }
                if !enc.is_null() {
                    out.extend_from_slice(b" encoding=\"");
                    if let Some(e) = crate::abi::versioning::c_str_to_bytes(enc as *const c_char) {
                        out.extend_from_slice(e);
                    }
                    out.push(b'"');
                }
                match standalone {
                    0 => out.extend_from_slice(b" standalone=\"no\""),
                    1 => out.extend_from_slice(b" standalone=\"yes\""),
                    _ => {}
                }
                out.extend_from_slice(b"?>\n");
            }

            if !(*result).children.is_null() {
                let mut child = (*result).children;
                while !child.is_null() {
                    append_serialized_node(&mut out, child, if indent == 1 { 1 } else { 0 });
                    if indent != 0 {
                        let ct = (*child).type_;
                        if ct == XML_DTD_NODE as c_int
                            || (ct == XML_COMMENT_NODE as c_int && !(*child).next.is_null())
                        {
                            out.push(b'\n');
                        }
                    }
                    child = (*child).next;
                }
                if indent != 0 {
                    out.push(b'\n');
                }
            }
        }
    }

    Ok(out)
}

/// Helper used by `save_result_to_vec`: serialize one top-level child into a
/// buffer and append it to `out`.
///
/// # SAFETY
///
/// - `node` must be a valid node.
unsafe fn append_serialized_node(out: &mut Vec<u8>, node: *mut _xmlNode, format: c_int) {
    let buf = crate::xml::io::buf_create(-1);
    if buf.is_null() {
        return;
    }
    crate::xml::tree::serialize_node(node, buf, format, 0);
    let len = crate::xml::io::buf_length(buf);
    let content = crate::xml::io::buf_content(buf);
    if len > 0 && !content.is_null() {
        out.extend_from_slice(core::slice::from_raw_parts(content, len as usize));
    }
    crate::xml::io::buf_free(buf);
}

/// Save a result document to a buffer, honoring output settings.
///
/// Returns a newly allocated string in `doc_txt_ptr` and its length in
/// `doc_txt_len`. Returns 0 on success, -1 on error.
///
/// # UPSTREAM-PARITY
///
/// Mirrors `xsltSaveResultToString` (xsltutils.c 1.1.45): the output is the
/// serialized document converted to the stylesheet's output encoding
/// (UTF-8 when no encoding or a UTF-8 encoding is selected). Note that
/// upstream does not validate `style` here; `xsltSaveResultTo` fails for a
/// NULL stylesheet and yields an empty string.
///
/// # SAFETY
///
/// - `doc_txt_ptr` and `doc_txt_len` must be valid non-null pointers.
/// - `result` must be a valid document.
/// - `style` must be a valid stylesheet (or NULL for empty output).
#[no_mangle]
pub unsafe extern "C" fn xsltSaveResultToString(
    doc_txt_ptr: *mut *mut xmlChar,
    doc_txt_len: *mut c_int,
    result: *mut _xmlDoc,
    style: *mut _xsltStylesheet,
) -> c_int {
    if doc_txt_ptr.is_null() || doc_txt_len.is_null() || result.is_null() {
        return -1;
    }
    *doc_txt_ptr = ptr::null_mut();
    *doc_txt_len = 0;

    if (*result).children.is_null() {
        return 0;
    }

    let bytes = match save_result_to_vec(result, style) {
        Ok(b) => b,
        Err(_) => {
            // Upstream produces an empty string when the save fails.
            let empty = crate::abi::allocator::xmlMallocImpl(1) as *mut xmlChar;
            if empty.is_null() {
                return -1;
            }
            *empty = 0;
            *doc_txt_ptr = empty;
            *doc_txt_len = 0;
            return 0;
        }
    };

    // Convert to the output encoding when it is not UTF-8.
    let encoding = if !style.is_null() {
        import_chain_str(style, |st| st.encoding)
    } else {
        ptr::null()
    };
    let converted: Vec<u8> = if !encoding.is_null()
        && !cstr_eq_ignore_case(encoding, b"UTF-8")
        && !cstr_eq_ignore_case(encoding, b"UTF8")
    {
        let enc = crate::abi::versioning::c_str_to_bytes(encoding as *const c_char).unwrap_or(b"");
        let enc_lower = enc.to_ascii_lowercase();
        if enc_lower.as_slice() == b"iso-8859-1"
            || enc_lower.as_slice() == b"latin1"
            || enc_lower.as_slice() == b"latin-1"
        {
            match encoding::utf8_to_latin1(&bytes) {
                Ok(c) => c,
                Err(_) => bytes,
            }
        } else {
            // RESIDUAL R-ENCODING-CONVERSION: encodings other than UTF-8 and
            // ISO-8859-1 are emitted as UTF-8 for now.
            bytes
        }
    } else {
        bytes
    };

    let out = crate::abi::allocator::xmlMallocImpl(converted.len() + 1) as *mut xmlChar;
    if out.is_null() {
        return -1;
    }
    if !converted.is_empty() {
        core::ptr::copy_nonoverlapping(converted.as_ptr(), out, converted.len());
    }
    *out.add(converted.len()) = 0;
    *doc_txt_ptr = out;
    *doc_txt_len = converted.len() as c_int;
    0
}

/// Save a result document to a file (FILE*).
///
/// # SAFETY
///
/// - `output` must be a valid FILE*.
/// - `result` must be a valid document.
#[no_mangle]
pub unsafe extern "C" fn xsltSaveResultToFile(
    output: *mut c_void,
    result: *mut _xmlDoc,
    style: *mut _xsltStylesheet,
) -> c_int {
    if output.is_null() || result.is_null() || style.is_null() {
        return -1;
    }
    if result_is_empty(result) {
        return 0;
    }
    let mut txt: *mut xmlChar = ptr::null_mut();
    let mut len: c_int = 0;
    let ret = xsltSaveResultToString(&mut txt, &mut len, result, style);
    if ret != 0 {
        return -1;
    }
    let written = libc::fwrite(
        txt as *const libc::c_void,
        1,
        len as usize,
        output as *mut libc::FILE,
    );
    xmlFreeImpl(txt as *mut c_void);
    written as c_int
}

/// Save a result document to a filename or URL.
///
/// # UPSTREAM-PARITY
///
/// Mirrors `xsltSaveResultToFilename` (xsltutils.c 1.1.45): opens the file
/// (compression is not supported by the Rust artifacts yet; see
/// RESIDUAL R-COMPRESSION), serializes via `xsltSaveResultTo`, closes it and
/// returns the number of bytes written.
///
/// # SAFETY
///
/// - `URL` must be a valid NUL-terminated path.
/// - `result` must be a valid document.
#[no_mangle]
pub unsafe extern "C" fn xsltSaveResultToFilename(
    URL: *const c_char,
    result: *mut _xmlDoc,
    style: *mut _xsltStylesheet,
    compression: c_int,
) -> c_int {
    let _ = compression;
    if URL.is_null() || result.is_null() || style.is_null() {
        return -1;
    }
    if result_is_empty(result) {
        return 0;
    }
    let file = libc::fopen(URL, c"wb".as_ptr() as *const c_char);
    if file.is_null() {
        return -1;
    }
    let ret = xsltSaveResultToFile(file as *mut c_void, result, style);
    libc::fclose(file);
    ret
}

/// Save a result document to a file descriptor.
///
/// # SAFETY
///
/// - `result` must be a valid document.
#[no_mangle]
pub unsafe extern "C" fn xsltSaveResultToFd(
    fd: c_int,
    result: *mut _xmlDoc,
    style: *mut _xsltStylesheet,
) -> c_int {
    if fd < 0 || result.is_null() || style.is_null() {
        return -1;
    }
    if result_is_empty(result) {
        return 0;
    }
    let mut txt: *mut xmlChar = ptr::null_mut();
    let mut len: c_int = 0;
    let ret = xsltSaveResultToString(&mut txt, &mut len, result, style);
    if ret != 0 {
        return -1;
    }
    let bytes = core::slice::from_raw_parts(txt, len as usize);
    let mut written = 0usize;
    while written < bytes.len() {
        let n = libc::write(
            fd,
            bytes[written..].as_ptr() as *const libc::c_void,
            bytes.len() - written,
        );
        if n < 0 {
            break;
        }
        written += n as usize;
    }
    xmlFreeImpl(txt as *mut c_void);
    written as c_int
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml::tree::*;
    use core::ptr;

    /// Verify that `xsltSaveResultToString` rejects NULL output/document
    /// pointers with -1.
    ///
    /// # Safety
    ///
    /// - Only NULL pointers are passed to `xsltSaveResultToString`, which
    ///   rejects them before any dereference, so no raw pointer is read
    ///   or written inside the unsafe block.
    #[test]
    fn test_save_result_null() {
        unsafe {
            assert_eq!(
                xsltSaveResultToString(
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut()
                ),
                -1
            );
        }
    }

    /// Serialize a small document with `xsltSaveResultToString` and check
    /// the produced bytes, then release the result buffer, stylesheet,
    /// and document.
    ///
    /// # Safety
    ///
    /// - `doc`, `root`, and `text` are live nodes created by `new_doc`,
    ///   `new_node`, and `new_text` and linked with `doc_set_root_element`
    ///   and `add_child`, so they stay valid for the duration of the call.
    /// - `style` is a valid stylesheet returned by `xsltStylesheetCreate`
    ///   and is released with `xsltFreeStylesheet` after the buffer is
    ///   freed.
    /// - `txt` is heap-allocated by `xsltSaveResultToString` with
    ///   `xmlMalloc` and released here with `xmlFreeImpl`; the slice read
    ///   from it is bounded by the returned `len`.
    #[test]
    fn test_save_result_to_string() {
        unsafe {
            let doc = new_doc(c"1.0".as_ptr() as *const xmlChar);
            let root = new_node(ptr::null_mut(), c"root".as_ptr() as *const xmlChar);
            doc_set_root_element(doc, root);
            let text = new_text(c"hello".as_ptr() as *const xmlChar);
            add_child(root, text);
            let style = crate::xslt::stylesheet::xsltStylesheetCreate();
            let mut txt: *mut xmlChar = ptr::null_mut();
            let mut len: c_int = 0;
            let ret = xsltSaveResultToString(&mut txt, &mut len, doc, style);
            assert_eq!(ret, 0);
            assert!(!txt.is_null());
            assert!(len > 0);
            // UPSTREAM-PARITY: no encoding in the declaration (result doc has
            // none), newline after the declaration and after the root child.
            let bytes = core::slice::from_raw_parts(txt, len as usize);
            assert_eq!(bytes, b"<?xml version=\"1.0\"?>\n<root>hello</root>\n");
            xmlFreeImpl(txt as *mut c_void);
            crate::xslt::stylesheet::xsltFreeStylesheet(style);
            free_doc(doc);
        }
    }

    /// Pin the upstream quirk: `indent == -1` (unset) writes a trailing
    /// newline while `indent == 0` (indent="no") does not.
    ///
    /// # Safety
    ///
    /// - `doc` and `root` are live nodes created by `new_doc` and
    ///   `new_node` and linked with `doc_set_root_element`, so they remain
    ///   valid for `xsltSaveResultToString`.
    /// - `style` is a valid stylesheet from `xsltStylesheetCreate`; the
    ///   write to `(*style).indent` is valid because the struct is live
    ///   and is freed with `xsltFreeStylesheet` afterwards.
    /// - `txt` is an `xmlMalloc` buffer owned by the test and released
    ///   with `xmlFreeImpl` before `style` and `doc` are freed.
    #[test]
    fn test_save_result_indent_quirk() {
        // UPSTREAM-PARITY: `indent == -1` (unset) writes the trailing
        // newline while `indent == 0` (indent="no") does not.
        unsafe {
            let doc = new_doc(c"1.0".as_ptr() as *const xmlChar);
            let root = new_node(ptr::null_mut(), c"root".as_ptr() as *const xmlChar);
            doc_set_root_element(doc, root);
            let style = crate::xslt::stylesheet::xsltStylesheetCreate();
            (*style).indent = 0;
            let mut txt: *mut xmlChar = ptr::null_mut();
            let mut len: c_int = 0;
            assert_eq!(xsltSaveResultToString(&mut txt, &mut len, doc, style), 0);
            let bytes = core::slice::from_raw_parts(txt, len as usize);
            assert_eq!(bytes, b"<?xml version=\"1.0\"?>\n<root/>");
            xmlFreeImpl(txt as *mut c_void);
            crate::xslt::stylesheet::xsltFreeStylesheet(style);
            free_doc(doc);
        }
    }

    /// Verify the `text` output method writes the raw concatenated text
    /// of the document with no escaping or trailing newline.
    ///
    /// # Safety
    ///
    /// - `doc`, `root`, and `text` are live nodes created by `new_doc`,
    ///   `new_node`, and `new_text` and linked with `doc_set_root_element`
    ///   and `add_child`, so they stay valid for the call.
    /// - `method` is a 5-byte `libc::malloc` buffer filled with `text\0`
    ///   by `copy_nonoverlapping`, so `(*style).method` is a valid
    ///   NUL-terminated string; `style` is valid from
    ///   `xsltStylesheetCreate` and `xsltFreeStylesheet` releases both
    ///   `style` and its `method` field.
    /// - `txt` is an `xmlMalloc` buffer returned by `xsltSaveResultToString`
    ///   and is freed with `xmlFreeImpl`.
    #[test]
    fn test_save_result_text_method() {
        unsafe {
            let doc = new_doc(c"1.0".as_ptr() as *const xmlChar);
            let root = new_node(ptr::null_mut(), c"root".as_ptr() as *const xmlChar);
            doc_set_root_element(doc, root);
            let text = new_text(c"a".as_ptr() as *const xmlChar);
            add_child(root, text);
            let style = crate::xslt::stylesheet::xsltStylesheetCreate();
            let method = libc::malloc(5) as *mut xmlChar;
            core::ptr::copy_nonoverlapping(c"text".as_ptr() as *const xmlChar, method, 5);
            (*style).method = method;
            let mut txt: *mut xmlChar = ptr::null_mut();
            let mut len: c_int = 0;
            assert_eq!(xsltSaveResultToString(&mut txt, &mut len, doc, style), 0);
            let bytes = core::slice::from_raw_parts(txt, len as usize);
            assert_eq!(bytes, b"a");
            xmlFreeImpl(txt as *mut c_void);
            crate::xslt::stylesheet::xsltFreeStylesheet(style);
            free_doc(doc);
        }
    }
}
