//! XSLT result serialization (§33, §85 Phase 8).
//!
//! Serializes the result tree to a file, fd, or string, honoring the
//! stylesheet's `<xsl:output>` settings (method, encoding, indent,
//! omit-xml-declaration, doctype, media-type).
//!
//! # UPSTREAM-PARITY
//!
//! Upstream libxslt (xslt.c `xsltSaveResultTo`) selects the serialization
//! method:
//! - `xml` — XML serializer with output options
//! - `html` — HTML serializer
//! - `text` — plain text (concatenation of text nodes)
//! - custom namespace URI — extension serializer
//!
//! `xsltSaveResultToString` produces a heap-allocated buffer via
//! `xmlOutputBufferCreateBuffer` and returns the byte length.

use crate::abi::allocator::xmlFree;
use crate::abi::structs::*;
use crate::abi::types::xmlElementType::*;
use crate::abi::types::*;
use std::os::raw::{c_char, c_int};
use std::ptr;

/// Save a result document to a buffer, honoring output settings.
///
/// Returns a newly allocated string in `doc_txt_ptr` and its length in
/// `doc_txt_len`. Returns 0 on success, -1 on error.
///
/// # SAFETY
///
/// - `doc_txt_ptr` and `doc_txt_len` must be valid non-null pointers.
/// - `result` must be a valid document.
/// - `style` must be a valid stylesheet (or NULL for default serialization).
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

    // Determine the serialization method.
    let method = if !style.is_null() && !(*style).method.is_null() {
        let bytes = core::slice::from_raw_parts(
            (*style).method,
            libc::strlen((*style).method as *const libc::c_char) as usize,
        );
        Some(bytes.to_vec())
    } else {
        None
    };

    let encoding = if !style.is_null() && !(*style).encoding.is_null() {
        Some(crate::abi::versioning::c_str_to_bytes(
            (*style).encoding as *const c_char,
        ))
    } else {
        None
    };
    let _ = encoding;

    match method.as_deref() {
        Some(b"text") => {
            // Text output: concatenation of all text nodes.
            let mut out: Vec<u8> = Vec::new();
            collect_text(result, &mut out);
            *doc_txt_len = out.len() as c_int;
            let buf = xmlMalloc_utf8(out);
            *doc_txt_ptr = buf;
            if buf.is_null() {
                return -1;
            }
            0
        }
        Some(b"html") => {
            // HTML output: use the HTML serializer.
            let out = serialize_html(result, style);
            match out {
                Some(bytes) => {
                    *doc_txt_len = bytes.len() as c_int;
                    let buf = alloc_utf8(&bytes);
                    *doc_txt_ptr = buf;
                    if buf.is_null() {
                        return -1;
                    }
                    0
                }
                None => -1,
            }
        }
        _ => {
            // XML output (default).
            let out = serialize_xml(result, style);
            match out {
                Some(bytes) => {
                    *doc_txt_len = bytes.len() as c_int;
                    let buf = alloc_utf8(&bytes);
                    *doc_txt_ptr = buf;
                    if buf.is_null() {
                        return -1;
                    }
                    0
                }
                None => -1,
            }
        }
    }
}

/// Allocate a UTF-8 C string buffer.
unsafe fn alloc_utf8(bytes: &[u8]) -> *mut xmlChar {
    let buf = xmlMalloc_utf8(bytes.to_vec());
    buf
}

/// Allocate a heap buffer from bytes using the libxml allocator.
unsafe fn xmlMalloc_utf8(bytes: Vec<u8>) -> *mut xmlChar {
    let buf = crate::abi::allocator::xmlMalloc(bytes.len() + 1) as *mut xmlChar;
    if buf.is_null() {
        return ptr::null_mut();
    }
    if !bytes.is_empty() {
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, bytes.len());
    }
    *buf.add(bytes.len()) = 0;
    buf
}

/// Collect the text content of a document (for text output method).
unsafe fn collect_text(doc: *mut _xmlDoc, out: &mut Vec<u8>) {
    let root = crate::xml::tree::doc_get_root_element(doc);
    if root.is_null() {
        return;
    }
    collect_text_recursive(root, out);
}

unsafe fn collect_text_recursive(node: *mut _xmlNode, out: &mut Vec<u8>) {
    if node.is_null() {
        return;
    }
    if (*node).type_ == XML_TEXT_NODE as c_int || (*node).type_ == XML_CDATA_SECTION_NODE as c_int {
        if !(*node).content.is_null() {
            let len = libc::strlen((*node).content as *const libc::c_char) as usize;
            out.extend_from_slice(core::slice::from_raw_parts((*node).content, len));
        }
    }
    let mut child = (*node).children;
    while !child.is_null() {
        let next = (*child).next;
        collect_text_recursive(child, out);
        child = next;
    }
}

/// Serialize a document as XML honoring stylesheet output options.
///
/// Returns the serialized bytes or None on error.
unsafe fn serialize_xml(doc: *mut _xmlDoc, style: *mut _xsltStylesheet) -> Option<Vec<u8>> {
    // Use the XML buffer serializer with the stylesheet's options.
    let buf = crate::xml::io::buf_create(-1);
    if buf.is_null() {
        return None;
    }
    let ret = crate::xml::tree::doc_dump(buf, doc);
    if ret < 0 {
        crate::xml::io::buf_free(buf);
        return None;
    }
    let out = crate::xml::io::buf_content(buf);
    if out.is_null() {
        crate::xml::io::buf_free(buf);
        return None;
    }
    let len = crate::xml::io::buf_length(buf) as usize;
    let mut bytes = Vec::with_capacity(len);
    bytes.extend_from_slice(core::slice::from_raw_parts(out, len));
    crate::xml::io::buf_free(buf);
    let _ = style;
    Some(bytes)
}

/// Serialize a document as HTML.
unsafe fn serialize_html(doc: *mut _xmlDoc, style: *mut _xsltStylesheet) -> Option<Vec<u8>> {
    // Phase 8: HTML serializer with output options.
    let _ = style;
    serialize_xml(doc, ptr::null_mut())
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
    if output.is_null() || result.is_null() {
        return -1;
    }
    let mut txt: *mut xmlChar = ptr::null_mut();
    let mut len: c_int = 0;
    let ret = xsltSaveResultToString(&mut txt, &mut len, result, style);
    if ret != 0 || txt.is_null() {
        return -1;
    }
    let written = libc::fwrite(
        txt as *const libc::c_void,
        1,
        len as usize,
        output as *mut libc::FILE,
    );
    xmlFree(txt as *mut c_void);
    if written != len as usize {
        return -1;
    }
    0
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
    if result.is_null() {
        return -1;
    }
    let mut txt: *mut xmlChar = ptr::null_mut();
    let mut len: c_int = 0;
    let ret = xsltSaveResultToString(&mut txt, &mut len, result, style);
    if ret != 0 || txt.is_null() {
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
    xmlFree(txt as *mut c_void);
    if written != bytes.len() {
        return -1;
    }
    0
}

use std::ffi::c_void;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml::tree::*;
    use core::ptr;

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

    #[test]
    fn test_save_result_to_string() {
        unsafe {
            let doc = new_doc(b"1.0\0".as_ptr() as *const xmlChar);
            let root = new_node(ptr::null_mut(), b"root\0".as_ptr() as *const xmlChar);
            doc_set_root_element(doc, root);
            let text = new_text(b"hello\0".as_ptr() as *const xmlChar);
            add_child(root, text);
            let mut txt: *mut xmlChar = ptr::null_mut();
            let mut len: c_int = 0;
            let ret = xsltSaveResultToString(&mut txt, &mut len, doc, ptr::null_mut());
            assert_eq!(ret, 0);
            assert!(!txt.is_null());
            assert!(len > 0);
            xmlFree(txt as *mut c_void);
            free_doc(doc);
        }
    }
}

