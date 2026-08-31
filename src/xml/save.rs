//! XML save-context API (upstream xmlsave.c, 2.15.3).
//!
//! `xmlSaveToFd` / `xmlSaveToFilename` / `xmlSaveToBuffer` / `xmlSaveToIO`
//! create a save context; `xmlSaveDoc` / `xmlSaveTree` serialize into it;
//! `xmlSaveFlush` / `xmlSaveClose` / `xmlSaveFinish` finalize it.
//!
//! # UPSTREAM-PARITY
//!
//! `xmlSaveCtxt` is opaque in the public headers (xmlsave.h); the candidate
//! defines its own internal representation — there is no ABI constraint on
//! its layout. Behavior mirrors xmlsave.c: options XML_SAVE_FORMAT,
//! XML_SAVE_NO_DECL, XML_SAVE_NO_EMPTY and the deprecated escape callbacks.
//! Formatting/decl handling is provided by the tree serializer
//! (`serialize_node_opts`), which mirrors upstream's `xmlSaveDoc`/
//! `xmlSaveTree`/DumpState mechanics.
//!
//! # Courts
//!
//! SAVE-* differential cases compare `xmlSave*` output byte-for-byte with
//! the oracle DSO across option combinations.

use crate::abi::callbacks::{
    xmlCharEncodingOutputFunc, xmlOutputCloseCallback, xmlOutputWriteCallback,
};
use crate::abi::structs::{_xmlDoc, _xmlNode, _xmlOutputBuffer};
use crate::abi::types::xmlChar;
use crate::xml::io;
use std::os::raw::{c_char, c_int, c_long};
use std::ptr;

/// XML_SAVE_FORMAT — format output (newlines + indentation).
pub const XML_SAVE_FORMAT: c_int = 1 << 0;
/// XML_SAVE_NO_DECL — don't emit an XML declaration.
pub const XML_SAVE_NO_DECL: c_int = 1 << 1;
/// XML_SAVE_NO_EMPTY — don't emit empty tags.
pub const XML_SAVE_NO_EMPTY: c_int = 1 << 2;

/// Candidate-internal save context (opaque upstream).
#[derive(Debug)]
#[repr(C)]
pub struct _xmlSaveCtxt {
    /// The output buffer the context serializes into.
    pub buf: *mut _xmlOutputBuffer,
    /// The `XML_SAVE_*` option bitmask passed to `xmlSaveTo*`.
    pub options: c_int,
    /// Whether `XML_SAVE_FORMAT` (newlines + indentation) is enabled.
    pub format: c_int,
    /// Whether the XML declaration is suppressed (`XML_SAVE_NO_DECL`).
    pub no_decl: c_int,
    /// Whether empty elements must be written with an explicit end tag
    /// (`XML_SAVE_NO_EMPTY`).
    pub no_empty: c_int,
    /// Optional indentation string used when formatting is enabled.
    pub indent: *mut xmlChar,
    /// Character-escaping callback for text content (deprecated upstream).
    pub escape: Option<xmlCharEncodingOutputFunc>,
    /// Character-escaping callback for attribute values (deprecated upstream).
    pub attrEscape: Option<xmlCharEncodingOutputFunc>,
}

/// Create a save context around an output buffer.
unsafe fn save_ctxt_new(buf: *mut _xmlOutputBuffer, options: c_int) -> *mut _xmlSaveCtxt {
    if buf.is_null() {
        return ptr::null_mut();
    }
    let ctxt = libc::calloc(1, core::mem::size_of::<_xmlSaveCtxt>()) as *mut _xmlSaveCtxt;
    if ctxt.is_null() {
        io::output_buffer_close(buf);
        return ptr::null_mut();
    }
    (*ctxt).buf = buf;
    (*ctxt).options = options;
    (*ctxt).format = if (options & XML_SAVE_FORMAT) != 0 {
        1
    } else {
        0
    };
    (*ctxt).no_decl = if (options & XML_SAVE_NO_DECL) != 0 {
        1
    } else {
        0
    };
    (*ctxt).no_empty = if (options & XML_SAVE_NO_EMPTY) != 0 {
        1
    } else {
        0
    };
    ctxt
}

/// Resolve an encoding name to an encoding handler.
unsafe fn encoding_handler(
    encoding: *const c_char,
) -> *mut crate::abi::structs::_xmlCharEncodingHandler {
    if encoding.is_null() {
        return ptr::null_mut();
    }
    crate::xml::encoding::xmlFindCharEncodingHandler(encoding)
}

/// `xmlSaveCtxt *xmlSaveToFd(int fd, const char *encoding, int options)`.
///
/// # SAFETY
///
/// - `fd` must be a valid open file descriptor.
#[no_mangle]
pub unsafe extern "C" fn xmlSaveToFd(
    fd: c_int,
    encoding: *const c_char,
    options: c_int,
) -> *mut _xmlSaveCtxt {
    let enc = unsafe { encoding_handler(encoding) };
    let out = io::output_buffer_create_fd(fd, enc);
    unsafe { save_ctxt_new(out, options) }
}

/// `xmlSaveCtxt *xmlSaveToFilename(const char *filename, const char *encoding, int options)`.
///
/// # SAFETY
///
/// - `filename` must be a valid NUL-terminated path.
#[no_mangle]
pub unsafe extern "C" fn xmlSaveToFilename(
    filename: *const c_char,
    encoding: *const c_char,
    options: c_int,
) -> *mut _xmlSaveCtxt {
    let enc = unsafe { encoding_handler(encoding) };
    let out = io::output_buffer_create_filename(filename, enc, 0);
    unsafe { save_ctxt_new(out, options) }
}

/// `xmlSaveCtxt *xmlSaveToBuffer(xmlBuffer *buffer, const char *encoding, int options)`.
///
/// # SAFETY
///
/// - `buffer` must be a valid `_xmlBuffer`.
#[no_mangle]
pub unsafe extern "C" fn xmlSaveToBuffer(
    buffer: *mut crate::abi::structs::_xmlBuffer,
    encoding: *const c_char,
    options: c_int,
) -> *mut _xmlSaveCtxt {
    let enc = unsafe { encoding_handler(encoding) };
    let out = io::output_buffer_create_buffer(buffer, enc);
    unsafe { save_ctxt_new(out, options) }
}

/// `xmlSaveCtxt *xmlSaveToIO(xmlOutputWriteCallback iowrite, xmlOutputCloseCallback ioclose, void *ioctx, const char *encoding, int options)`.
///
/// # SAFETY
///
/// - The callbacks must be valid function pointers or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlSaveToIO(
    iowrite: Option<xmlOutputWriteCallback>,
    ioclose: Option<xmlOutputCloseCallback>,
    ioctx: *mut core::ffi::c_void,
    encoding: *const c_char,
    options: c_int,
) -> *mut _xmlSaveCtxt {
    let enc = unsafe { encoding_handler(encoding) };
    let out = io::output_buffer_create_io(iowrite, ioclose, ioctx, enc);
    unsafe { save_ctxt_new(out, options) }
}

/// Serialize `doc` into the save context's output buffer.
///
/// Returns the number of bytes written, or -1 on error.
///
/// # SAFETY
///
/// - `ctxt` must be a valid save context.
/// - `doc` must be a valid document or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlSaveDoc(ctxt: *mut _xmlSaveCtxt, doc: *mut _xmlDoc) -> c_long {
    unsafe { save_doc_or_tree(ctxt, doc as *mut _xmlNode) }
}

/// Serialize a node tree into the save context's output buffer.
///
/// Returns the number of bytes written, or -1 on error.
///
/// # SAFETY
///
/// - `ctxt` must be a valid save context.
/// - `node` must be a valid node or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlSaveTree(ctxt: *mut _xmlSaveCtxt, node: *mut _xmlNode) -> c_long {
    unsafe { save_doc_or_tree(ctxt, node) }
}

unsafe fn save_doc_or_tree(ctxt: *mut _xmlSaveCtxt, node: *mut _xmlNode) -> c_long {
    if ctxt.is_null() || node.is_null() {
        return -1;
    }
    let buf = io::buf_create(-1);
    if buf.is_null() {
        return -1;
    }
    let indent = (*ctxt).indent;
    let format = (*ctxt).format;
    let no_decl = (*ctxt).no_decl;
    crate::xml::tree::serialize_node_opts(node, buf, format, 0, indent, no_decl);

    let before = io::buf_length(buf);
    let content = io::buf_content(buf);
    let ret = if before > 0 && !content.is_null() {
        io::output_buffer_write((*ctxt).buf, before, content as *const c_char)
    } else {
        0
    };
    io::buf_free(buf);
    if ret < 0 {
        -1
    } else {
        ret as c_long
    }
}

/// `int xmlSaveFlush(xmlSaveCtxt *ctxt)` — flush the output buffer.
///
/// # SAFETY
///
/// - `ctxt` must be a valid save context.
#[no_mangle]
pub unsafe extern "C" fn xmlSaveFlush(ctxt: *mut _xmlSaveCtxt) -> c_int {
    if ctxt.is_null() {
        return -1;
    }
    io::output_buffer_flush((*ctxt).buf)
}

/// `int xmlSaveClose(xmlSaveCtxt *ctxt)` — flush, close and free the context.
///
/// # UPSTREAM-PARITY
///
/// Returns the number of bytes written (the flush result), like upstream
/// xmlSaveClose (xmlsave.c 2.15); the underlying output buffer is closed by
/// xmlFreeSaveCtxt.
///
/// # SAFETY
///
/// - `ctxt` must be a valid save context; it is freed by this call.
#[no_mangle]
pub unsafe extern "C" fn xmlSaveClose(ctxt: *mut _xmlSaveCtxt) -> c_int {
    if ctxt.is_null() {
        return -1;
    }
    let flush_ret = if (*ctxt).buf.is_null() {
        -1
    } else {
        io::output_buffer_flush((*ctxt).buf)
    };
    // xmlFreeSaveCtxt closes the output buffer and frees the context.
    if !(*ctxt).buf.is_null() {
        io::output_buffer_close((*ctxt).buf);
    }
    if !(*ctxt).indent.is_null() {
        libc::free((*ctxt).indent as *mut libc::c_void);
    }
    libc::free(ctxt as *mut libc::c_void);
    flush_ret
}

/// `xmlParserErrors xmlSaveFinish(xmlSaveCtxt *ctxt)` — flush, close, free;
/// returns an xmlParserErrors code (XML_ERR_OK on success).
///
/// # UPSTREAM-PARITY
///
/// Upstream xmlSaveFinish returns `xmlOutputBufferClose(ctxt->buf)`'s error
/// code (negated when negative), i.e. XML_ERR_OK (0) on success.
///
/// # SAFETY
///
/// - `ctxt` must be a valid save context; it is freed by this call.
#[no_mangle]
pub unsafe extern "C" fn xmlSaveFinish(ctxt: *mut _xmlSaveCtxt) -> c_int {
    if ctxt.is_null() {
        return -1;
    }
    let ret = if (*ctxt).buf.is_null() {
        -1
    } else {
        io::output_buffer_close((*ctxt).buf)
    };
    if !(*ctxt).indent.is_null() {
        libc::free((*ctxt).indent as *mut libc::c_void);
    }
    libc::free(ctxt as *mut libc::c_void);
    if ret < 0 {
        -ret
    } else {
        0
    }
}

/// `int xmlSaveSetIndentString(xmlSaveCtxt *ctxt, const char *indent)`.
///
/// # SAFETY
///
/// - `ctxt` must be a valid save context.
/// - `indent` must be a valid NUL-terminated string or NULL (reset to
///   default).
#[no_mangle]
pub unsafe extern "C" fn xmlSaveSetIndentString(
    ctxt: *mut _xmlSaveCtxt,
    indent: *const c_char,
) -> c_int {
    // UPSTREAM-PARITY: xmlSaveSetIndentString rejects NULL/empty/overlong
    // indents (xmlsave.c 2.15: (ctxt==NULL)||(indent==NULL) -> -1,
    // len<=0 || len>MAX_INDENT -> -1).
    if ctxt.is_null() || indent.is_null() {
        return -1;
    }
    let len = libc::strlen(indent) as usize;
    if len == 0 || len > 60 {
        return -1;
    }
    if !(*ctxt).indent.is_null() {
        libc::free((*ctxt).indent as *mut libc::c_void);
        (*ctxt).indent = ptr::null_mut();
    }
    let copy = libc::malloc(len + 1) as *mut xmlChar;
    if copy.is_null() {
        return -1;
    }
    libc::memcpy(
        copy as *mut libc::c_void,
        indent as *const libc::c_void,
        len + 1,
    );
    (*ctxt).indent = copy;
    0
}

/// `int xmlSaveSetEscape(xmlSaveCtxt *ctxt, xmlCharEncodingOutputFunc escape)`.
///
/// # SAFETY
///
/// - `ctxt` must be a valid save context.
#[no_mangle]
pub unsafe extern "C" fn xmlSaveSetEscape(
    ctxt: *mut _xmlSaveCtxt,
    escape: Option<xmlCharEncodingOutputFunc>,
) -> c_int {
    if ctxt.is_null() {
        return -1;
    }
    (*ctxt).escape = escape;
    0
}

/// `int xmlSaveSetAttrEscape(xmlSaveCtxt *ctxt, xmlCharEncodingOutputFunc escape)`.
///
/// # SAFETY
///
/// - `ctxt` must be a valid save context.
#[no_mangle]
pub unsafe extern "C" fn xmlSaveSetAttrEscape(
    ctxt: *mut _xmlSaveCtxt,
    escape: Option<xmlCharEncodingOutputFunc>,
) -> c_int {
    if ctxt.is_null() {
        return -1;
    }
    (*ctxt).attrEscape = escape;
    0
}

/// Wrap an existing output buffer in a save context (candidate-internal;
/// does not close the buffer on allocation failure — upstream xmlSaveFormatFileTo
/// semantics).
unsafe fn save_ctxt_wrap(buf: *mut _xmlOutputBuffer, options: c_int) -> *mut _xmlSaveCtxt {
    if buf.is_null() {
        return ptr::null_mut();
    }
    let ctxt = libc::calloc(1, core::mem::size_of::<_xmlSaveCtxt>()) as *mut _xmlSaveCtxt;
    if ctxt.is_null() {
        return ptr::null_mut();
    }
    (*ctxt).buf = buf;
    (*ctxt).options = options;
    (*ctxt).format = if (options & XML_SAVE_FORMAT) != 0 {
        1
    } else {
        0
    };
    (*ctxt).no_decl = if (options & XML_SAVE_NO_DECL) != 0 {
        1
    } else {
        0
    };
    (*ctxt).no_empty = if (options & XML_SAVE_NO_EMPTY) != 0 {
        1
    } else {
        0
    };
    ctxt
}

/// `int xmlSaveFormatFileTo(xmlOutputBufferPtr buf, xmlDocPtr cur, const char *encoding, int format)`
/// — serialize `cur` into an existing output buffer and close it (upstream
/// xmlsave.c).
///
/// # SAFETY
///
/// - `buf` must be a valid output buffer (closed by this call).
/// - `cur` must be a valid document.
#[no_mangle]
pub unsafe extern "C" fn xmlSaveFormatFileTo(
    buf: *mut _xmlOutputBuffer,
    cur: *mut _xmlDoc,
    encoding: *const c_char,
    format: c_int,
) -> c_int {
    let _ = encoding;
    let options = if format != 0 { XML_SAVE_FORMAT } else { 0 };
    let ctxt = unsafe { save_ctxt_wrap(buf, options) };
    if ctxt.is_null() {
        return -1;
    }
    let ret = unsafe { xmlSaveDoc(ctxt, cur) };
    let close_ret = unsafe { xmlSaveClose(ctxt) };
    if ret < 0 {
        -1
    } else {
        close_ret
    }
}

/// `int xmlSaveFileTo(xmlOutputBufferPtr buf, xmlDocPtr cur, const char *encoding)`
/// — upstream xmlsave.c delegates to xmlSaveFormatFileTo(buf, cur, encoding, 0).
///
/// # SAFETY
///
/// - `buf` must be a valid output buffer (closed by this call).
/// - `cur` must be a valid document.
#[no_mangle]
pub unsafe extern "C" fn xmlSaveFileTo(
    buf: *mut _xmlOutputBuffer,
    cur: *mut _xmlDoc,
    encoding: *const c_char,
) -> c_int {
    unsafe { xmlSaveFormatFileTo(buf, cur, encoding, 0) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml::tree::new_doc;

    fn doc_with_root() -> *mut _xmlDoc {
        unsafe {
            let doc = new_doc(c"1.0".as_ptr() as *const xmlChar);
            let root =
                crate::xml::tree::new_node(ptr::null_mut(), c"root".as_ptr() as *const xmlChar);
            crate::xml::tree::doc_set_root_element(doc, root);
            doc
        }
    }

    #[test]
    fn test_save_to_buffer_format_and_nodes() {
        unsafe {
            let doc = doc_with_root();
            let buf = io::buf_create(-1);
            let ctxt = xmlSaveToBuffer(buf, ptr::null(), XML_SAVE_FORMAT);
            assert!(!ctxt.is_null());
            assert!(xmlSaveDoc(ctxt, doc) >= 0);
            assert_eq!(xmlSaveFinish(ctxt), 0);
            let content = io::buf_content(buf);
            let len = io::buf_length(buf);
            let s = core::slice::from_raw_parts(content, len as usize);
            let expected = "<?xml version=\"1.0\"?>\n<root/>\n";
            assert_eq!(s, expected.as_bytes());
            crate::xml::tree::free_doc(doc);
            io::buf_free(buf);
        }
    }

    #[test]
    fn test_save_no_decl() {
        unsafe {
            let doc = doc_with_root();
            let buf = io::buf_create(-1);
            let ctxt = xmlSaveToBuffer(buf, ptr::null(), XML_SAVE_NO_DECL);
            assert!(!ctxt.is_null());
            xmlSaveDoc(ctxt, doc);
            xmlSaveFinish(ctxt);
            let content = io::buf_content(buf);
            let len = io::buf_length(buf);
            let s = core::slice::from_raw_parts(content, len as usize);
            assert_eq!(s, b"<root/>\n");
            crate::xml::tree::free_doc(doc);
            io::buf_free(buf);
        }
    }

    #[test]
    fn test_save_set_indent_string() {
        unsafe {
            let doc = doc_with_root();
            let child =
                crate::xml::tree::new_node(ptr::null_mut(), c"child".as_ptr() as *const xmlChar);
            crate::xml::tree::add_child(crate::xml::tree::doc_get_root_element(doc), child);
            let buf = io::buf_create(-1);
            let ctxt = xmlSaveToBuffer(buf, ptr::null(), XML_SAVE_FORMAT);
            assert!(!ctxt.is_null());
            assert_eq!(
                xmlSaveSetIndentString(ctxt, c"\t".as_ptr() as *const c_char),
                0
            );
            xmlSaveDoc(ctxt, doc);
            xmlSaveFinish(ctxt);
            let content = io::buf_content(buf);
            let len = io::buf_length(buf);
            let s = core::slice::from_raw_parts(content, len as usize);
            let expected = "<?xml version=\"1.0\"?>\n<root>\n\t<child/>\n</root>\n";
            assert_eq!(s, expected.as_bytes());
            crate::xml::tree::free_doc(doc);
            io::buf_free(buf);
        }
    }

    #[test]
    fn test_save_close_null_and_errors() {
        unsafe {
            assert!(xmlSaveToFd(-1, ptr::null(), 0).is_null());
            assert_eq!(xmlSaveFlush(ptr::null_mut()), -1);
            assert_eq!(xmlSaveFinish(ptr::null_mut()), -1);
            assert_eq!(xmlSaveClose(ptr::null_mut()), -1);
            assert_eq!(xmlSaveSetIndentString(ptr::null_mut(), ptr::null()), -1);
            assert_eq!(xmlSaveSetEscape(ptr::null_mut(), None), -1);
            assert_eq!(xmlSaveSetAttrEscape(ptr::null_mut(), None), -1);
            assert_eq!(xmlSaveDoc(ptr::null_mut(), ptr::null_mut()), -1);
            assert_eq!(xmlSaveTree(ptr::null_mut(), ptr::null_mut()), -1);
        }
    }
}
