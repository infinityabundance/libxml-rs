//! XML Writer API (§30, §85 Phase 7).
//!
//! Streaming XML writer with indentation, encoding, escaping, document lifecycle.
//!
//! Provides the `xmlTextWriter*` family of functions that allow constructing
//! XML documents in a streaming fashion — start/end element, write attributes,
//! text content, CDATA, comments, processing instructions, and DTD declarations.
//!
//! # UPSTREAM-PARITY
//!
//! This module mirrors libxml2's `xmlTextWriter` API defined in `xmlwriter.h`.
//! The writer maintains a state machine that tracks whether we are inside an
//! element start tag (attributes can be written), inside an attribute value,
//! inside a CDATA section, etc.

#![allow(
    missing_docs,
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals
)]

use core::ffi::c_void;
use core::ptr;
use std::os::raw::{c_char, c_int};

use crate::abi::allocator;

use crate::abi::structs::*;
use crate::abi::types::*;
use crate::xml::io;
use crate::xml::tree;

// ═══════════════════════════════════════════════════════════════════════════════
// Constants
// ═══════════════════════════════════════════════════════════════════════════════

// ═══════════════════════════════════════════════════════════════════════════════
// Writer state enumeration
// ═══════════════════════════════════════════════════════════════════════════════

/// Writer state — tracks what kind of content we are currently inside.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum WriterState {
    /// Initial / idle state — no element open.
    None,
    /// Inside an element start tag (attributes may be written).
    Element,
    /// Inside an attribute value.
    Attribute,
    /// Inside a CDATA section.
    CData,
    /// Inside a comment.
    Comment,
    /// Inside a processing instruction.
    PI,
    /// Inside a DTD declaration.
    DTD,
    /// Inside a DTD element declaration.
    DTDElem,
    /// Inside a DTD attribute declaration.
    DTDAttr,
    /// Inside a DTD entity declaration.
    DTDEntity,
    /// Inside a DTD notation declaration.
    DTDNotation,
    /// Writing XML declaration.
    XMLDecl,
}

// ═══════════════════════════════════════════════════════════════════════════════
// XmlTextWriter struct
// ═══════════════════════════════════════════════════════════════════════════════

/// A streaming XML writer.
///
/// Corresponds to `xmlTextWriterPtr` in libxml2.
///
/// The writer accumulates output into an internal buffer and flushes to the
/// underlying output buffer on demand. It maintains a stack of element names
/// for proper nesting, a state machine for content-type tracking, and optional
/// indentation.
pub struct XmlTextWriter {
    /// The output buffer where serialized XML is written.
    output: *mut _xmlOutputBuffer,
    /// Whether indentation is enabled (non-zero = enabled).
    indent: c_int,
    /// The string used for one level of indentation.
    indent_string: Vec<u8>,
    /// Current nesting depth.
    depth: c_int,
    /// Stack of element local names (for end-element matching).
    stack: Vec<Vec<u8>>,
    /// Output encoding name (e.g. "UTF-8").
    encoding: Vec<u8>,
    /// Collected error messages.
    errors: Vec<String>,
    /// Current writer state.
    state: WriterState,
    /// Optional document reference (used when writing to a document tree).
    doc: *mut _xmlDoc,
    /// Whether we are in the "start tag" portion of an element (attributes can be written).
    in_start_tag: bool,
    /// The element name stack with full qualified names for proper end-element matching.
    /// Stores (prefix, localname) pairs.
    elem_stack: Vec<(Vec<u8>, Vec<u8>)>,
}

impl XmlTextWriter {
    /// Create a new XML text writer.
    ///
    /// # SAFETY
    ///
    /// - `output` must be a valid pointer to a mutable `_xmlOutputBuffer` or NULL.
    unsafe fn new(output: *mut _xmlOutputBuffer) -> *mut Self {
        let writer = allocator::xmlMallocZero(size_of::<XmlTextWriter>() as usize) as *mut Self;
        if writer.is_null() {
            return ptr::null_mut();
        }
        unsafe {
            (*writer).output = output;
            (*writer).indent = 0;
            (*writer).indent_string = b"  \0".to_vec();
            (*writer).depth = 0;
            (*writer).stack = Vec::new();
            (*writer).encoding = b"UTF-8\0".to_vec();
            (*writer).errors = Vec::new();
            (*writer).state = WriterState::None;
            (*writer).doc = ptr::null_mut();
            (*writer).in_start_tag = false;
            (*writer).elem_stack = Vec::new();
        }
        writer
    }

    /// Write raw bytes to the output buffer.
    ///
    /// # SAFETY
    ///
    /// - `data` must point to `len` valid bytes.
    unsafe fn write_raw(&mut self, data: *const u8, len: c_int) {
        if self.output.is_null() || data.is_null() || len <= 0 {
            return;
        }
        io::output_buffer_write(self.output, len, data as *const c_char);
    }

    /// Write a null-terminated string to the output buffer.
    unsafe fn write_str(&mut self, s: *const u8) {
        if self.output.is_null() || s.is_null() {
            return;
        }
        io::output_buffer_write_string(self.output, s as *const c_char);
    }

    /// Write a byte slice to the output buffer.
    ///
    /// NOTE: The slice must NOT borrow from `self` to avoid borrow checker conflicts.
    unsafe fn write_slice(&mut self, slice: &[u8]) {
        if self.output.is_null() || slice.is_empty() {
            return;
        }
        io::output_buffer_write(
            self.output,
            slice.len() as c_int,
            slice.as_ptr() as *const c_char,
        );
    }

    /// Write a single byte to the output buffer.
    unsafe fn write_byte(&mut self, b: u8) {
        if self.output.is_null() {
            return;
        }
        io::output_buffer_write_char(self.output, b as c_char);
    }

    /// Write indentation (if enabled).
    ///
    /// Uses a clone of the indent string to avoid borrow checker conflicts.
    unsafe fn write_indent(&mut self) {
        if self.indent == 0 {
            return;
        }
        self.write_byte(b'\n');
        let indent_str = self.indent_string.clone();
        for _ in 0..self.depth {
            self.write_slice(&indent_str);
        }
    }

    /// Close any open start tag (writing `>` to transition from attribute-writing
    /// mode to content-writing mode).
    unsafe fn close_start_tag(&mut self) {
        if self.in_start_tag {
            self.write_byte(b'>');
            self.in_start_tag = false;
        }
    }

    /// Check if the writer is in a state where element/attribute content can be written.
    fn can_write_content(&self) -> bool {
        matches!(
            self.state,
            WriterState::None
                | WriterState::Element
                | WriterState::Attribute
                | WriterState::CData
                | WriterState::Comment
                | WriterState::PI
                | WriterState::DTD
                | WriterState::DTDElem
                | WriterState::DTDAttr
                | WriterState::DTDEntity
                | WriterState::DTDNotation
                | WriterState::XMLDecl
        )
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Free / destructor
// ═══════════════════════════════════════════════════════════════════════════════

/// Free an XML text writer.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlFreeTextWriter(xmlTextWriterPtr writer);
/// ```
///
/// # SAFETY
///
/// - `writer` must be a valid pointer returned by `xmlNewTextWriter*` or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlFreeTextWriter(writer: *mut XmlTextWriter) {
    if writer.is_null() {
        return;
    }
    // SAFETY: writer is a valid XmlTextWriter allocated by us.
    // Flush any pending data
    if !(*writer).output.is_null() {
        io::output_buffer_flush((*writer).output);
    }
    // Drop Rust-side allocations
    unsafe {
        ptr::drop_in_place(&mut (*writer).indent_string);
        ptr::drop_in_place(&mut (*writer).stack);
        ptr::drop_in_place(&mut (*writer).encoding);
        ptr::drop_in_place(&mut (*writer).errors);
        ptr::drop_in_place(&mut (*writer).elem_stack);
    }
    // Free the struct itself
    unsafe { allocator::xmlFree(writer as *mut c_void) };
}

// ═══════════════════════════════════════════════════════════════════════════════
// Writer creation
// ═══════════════════════════════════════════════════════════════════════════════

/// Create a new XML text writer from an output buffer.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlTextWriterPtr xmlNewTextWriter(xmlOutputBufferPtr out);
/// ```
///
/// # SAFETY
///
/// - `out` must be a valid pointer to an `_xmlOutputBuffer` or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlNewTextWriter(out: *mut _xmlOutputBuffer) -> *mut XmlTextWriter {
    if out.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: out is a valid output buffer.
    XmlTextWriter::new(out)
}

/// Create a new XML text writer for a file.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlTextWriterPtr xmlNewTextWriterFilename(const char *uri, int compression);
/// ```
///
/// # SAFETY
///
/// - `uri` must be a valid null-terminated string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlNewTextWriterFilename(
    uri: *const c_char,
    compression: c_int,
) -> *mut XmlTextWriter {
    if uri.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: uri is a valid C string.
    let out = io::output_buffer_create_filename(uri, ptr::null_mut(), compression);
    if out.is_null() {
        return ptr::null_mut();
    }
    XmlTextWriter::new(out)
}

/// Create a new XML text writer for a memory buffer.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlTextWriterPtr xmlNewTextWriterMemory(xmlBufferPtr buf, int compression);
/// ```
///
/// # SAFETY
///
/// - `buf` must be a valid pointer to an `_xmlBuffer` or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlNewTextWriterMemory(
    buf: *mut _xmlBuffer,
    compression: c_int,
) -> *mut XmlTextWriter {
    let _ = compression;
    if buf.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: buf is a valid xmlBuffer.
    let out = io::output_buffer_create_buffer(buf, ptr::null_mut());
    if out.is_null() {
        return ptr::null_mut();
    }
    XmlTextWriter::new(out)
}

/// Create a new XML text writer for a document (tree mode).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlTextWriterPtr xmlNewTextWriterDoc(xmlDocPtr *doc, int compression);
/// ```
///
/// # SAFETY
///
/// - `doc` must be a valid pointer to a (possibly NULL) xmlDocPtr.
#[no_mangle]
pub unsafe extern "C" fn xmlNewTextWriterDoc(
    doc: *mut *mut _xmlDoc,
    compression: c_int,
) -> *mut XmlTextWriter {
    let _ = compression;
    if doc.is_null() {
        return ptr::null_mut();
    }
    // Create a new document
    // SAFETY: doc is a valid pointer to an xmlDocPtr.
    let new_doc = tree::new_doc(b"1.0\0" as *const u8);
    if new_doc.is_null() {
        return ptr::null_mut();
    }
    unsafe { *doc = new_doc };

    // Create a memory buffer writer
    let buf = io::buf_create(io::DEFAULT_BUFFER_SIZE as c_int);
    if buf.is_null() {
        tree::free_doc(new_doc);
        return ptr::null_mut();
    }

    let out = io::output_buffer_create_buffer(buf, ptr::null_mut());
    if out.is_null() {
        io::buf_free(buf);
        tree::free_doc(new_doc);
        return ptr::null_mut();
    }

    let writer = XmlTextWriter::new(out);
    if !writer.is_null() {
        unsafe { (*writer).doc = new_doc };
    }
    writer
}

/// Create a new XML text writer for a subtree.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlTextWriterPtr xmlNewTextWriterTree(xmlDocPtr doc, xmlNodePtr node, int compression);
/// ```
///
/// # SAFETY
///
/// - `doc` must be a valid pointer to an `_xmlDoc` or NULL.
/// - `node` must be a valid pointer to an `_xmlNode` or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlNewTextWriterTree(
    doc: *mut _xmlDoc,
    node: *mut _xmlNode,
    compression: c_int,
) -> *mut XmlTextWriter {
    let _ = compression;
    let _ = node; // node is kept for future use when we write tree content directly
    if doc.is_null() {
        return ptr::null_mut();
    }

    let buf = io::buf_create(io::DEFAULT_BUFFER_SIZE as c_int);
    if buf.is_null() {
        return ptr::null_mut();
    }

    let out = io::output_buffer_create_buffer(buf, ptr::null_mut());
    if out.is_null() {
        io::buf_free(buf);
        return ptr::null_mut();
    }

    let writer = XmlTextWriter::new(out);
    if !writer.is_null() {
        unsafe { (*writer).doc = doc };
    }
    writer
}

// ═══════════════════════════════════════════════════════════════════════════════
// Document lifecycle
// ═══════════════════════════════════════════════════════════════════════════════

/// Start an XML document.
///
/// Writes the XML declaration `<?xml version="..." encoding="..." standalone="..."?>`.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextWriterStartDocument(xmlTextWriterPtr writer,
///                                 const char *version,
///                                 const char *encoding,
///                                 const char *standalone);
/// ```
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
/// - `version`, `encoding`, `standalone` must be valid null-terminated strings or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterStartDocument(
    writer: *mut XmlTextWriter,
    version: *const c_char,
    encoding: *const c_char,
    standalone: *const c_char,
) -> c_int {
    if writer.is_null() {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };

    w.write_raw(b"<?xml version=\"" as *const u8, 15);

    let ver = if version.is_null() {
        b"1.0\0" as *const u8
    } else {
        version as *const u8
    };
    w.write_str(ver);

    w.write_raw(b"\"" as *const u8, 1);

    let enc = if encoding.is_null() {
        ptr::null()
    } else {
        encoding as *const u8
    };
    if !enc.is_null() {
        w.write_raw(b" encoding=\"" as *const u8, 11);
        w.write_str(enc);
        w.write_byte(b'"');
    }

    if !standalone.is_null() {
        let sa = standalone as *const u8;
        w.write_raw(b" standalone=\"" as *const u8, 13);
        w.write_str(sa);
        w.write_byte(b'"');
    }

    w.write_raw(b"?>" as *const u8, 2);

    if w.indent != 0 {
        w.write_byte(b'\n');
    }

    w.state = WriterState::XMLDecl;
    0
}

/// End an XML document.
///
/// Flushes any pending output and writes a final newline if indentation is enabled.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextWriterEndDocument(xmlTextWriterPtr writer);
/// ```
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterEndDocument(writer: *mut XmlTextWriter) -> c_int {
    if writer.is_null() {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };

    // Close any open elements
    while w.depth > 0 {
        xmlTextWriterEndElement(writer);
    }

    // Final newline if indentation is enabled
    if w.indent != 0 {
        w.write_byte(b'\n');
    }

    // Flush output
    if !w.output.is_null() {
        io::output_buffer_flush(w.output);
    }

    w.state = WriterState::None;
    0
}

// ═══════════════════════════════════════════════════════════════════════════════
// Element writing
// ═══════════════════════════════════════════════════════════════════════════════

/// Start an XML element.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextWriterStartElement(xmlTextWriterPtr writer, const xmlChar *name);
/// ```
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
/// - `name` must be a valid null-terminated xmlChar string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterStartElement(
    writer: *mut XmlTextWriter,
    name: *const xmlChar,
) -> c_int {
    if writer.is_null() || name.is_null() {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };

    // Close any open start tag from a previous element
    w.close_start_tag();

    // Write indentation
    w.write_indent();

    // Write `<name`
    w.write_byte(b'<');
    w.write_str(name);

    // Push onto stack (without null terminator)
    let name_bytes = unsafe { c_str_to_vec(name) };
    w.elem_stack.push((b"".to_vec(), name_bytes.clone()));
    // Strip trailing null for stack storage
    let stack_name = if name_bytes.last() == Some(&0) {
        name_bytes[..name_bytes.len() - 1].to_vec()
    } else {
        name_bytes.clone()
    };
    w.stack.push(stack_name);
    w.depth += 1;
    w.in_start_tag = true;
    w.state = WriterState::Element;

    0
}

/// End an XML element.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextWriterEndElement(xmlTextWriterPtr writer);
/// ```
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterEndElement(writer: *mut XmlTextWriter) -> c_int {
    if writer.is_null() {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };

    if w.depth <= 0 {
        return -1;
    }

    // If we're still in the start tag (no content written), write self-closing tag
    if w.in_start_tag {
        // Remove the trailing `>` or just rewrite as `/>`
        // Since we wrote `<name` and then haven't closed, we just write `/>` and we're done
        w.write_raw(b"/>" as *const u8, 2);
        w.in_start_tag = false;
    } else {
        // Write indentation before end tag for non-inline content
        if w.indent != 0 {
            w.write_byte(b'\n');
            let indent_str = w.indent_string.clone();
            for _ in 0..(w.depth - 1) {
                w.write_slice(&indent_str);
            }
        }

        // Write `</name>`
        let name = w.stack.pop().unwrap_or_default();
        w.write_raw(b"</" as *const u8, 2);
        w.write_slice(&name);
        w.write_byte(b'>');
    }

    w.depth -= 1;
    w.elem_stack.pop();
    w.state = WriterState::None;

    0
}

/// Start a namespaced XML element.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextWriterStartElementNS(xmlTextWriterPtr writer,
///                                  const xmlChar *prefix,
///                                  const xmlChar *name,
///                                  const xmlChar *namespaceURI);
/// ```
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
/// - `prefix`, `name`, `namespaceURI` must be valid null-terminated strings or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterStartElementNS(
    writer: *mut XmlTextWriter,
    prefix: *const xmlChar,
    name: *const xmlChar,
    namespaceURI: *const xmlChar,
) -> c_int {
    if writer.is_null() || name.is_null() {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };

    w.close_start_tag();
    w.write_indent();

    w.write_byte(b'<');

    let prefix_bytes = if prefix.is_null() {
        Vec::new()
    } else {
        unsafe { c_str_to_vec(prefix) }
    };

    let name_bytes = unsafe { c_str_to_vec(name) };

    if !prefix_bytes.is_empty() {
        // Strip trailing null before writing
        let p = if prefix_bytes.last() == Some(&0) {
            &prefix_bytes[..prefix_bytes.len() - 1]
        } else {
            &prefix_bytes
        };
        w.write_slice(p);
        w.write_byte(b':');
    }
    // Strip trailing null before writing
    let n = if name_bytes.last() == Some(&0) {
        &name_bytes[..name_bytes.len() - 1]
    } else {
        &name_bytes
    };
    w.write_slice(n);

    // Write namespace declaration
    if !namespaceURI.is_null() {
        let ns_uri_bytes = unsafe { c_str_to_vec(namespaceURI) };
        if !prefix_bytes.is_empty() {
            w.write_raw(b" xmlns:" as *const u8, 7);
            let p = if prefix_bytes.last() == Some(&0) {
                &prefix_bytes[..prefix_bytes.len() - 1]
            } else {
                &prefix_bytes
            };
            w.write_slice(p);
        } else {
            w.write_raw(b" xmlns" as *const u8, 6);
        }
        w.write_raw(b"=\"" as *const u8, 2);
        let ns = if ns_uri_bytes.last() == Some(&0) {
            &ns_uri_bytes[..ns_uri_bytes.len() - 1]
        } else {
            &ns_uri_bytes
        };
        w.write_slice(ns);
        w.write_byte(b'"');
    }

    w.elem_stack.push((prefix_bytes, name_bytes.clone()));
    // Strip trailing null for stack storage
    let stack_name = if name_bytes.last() == Some(&0) {
        name_bytes[..name_bytes.len() - 1].to_vec()
    } else {
        name_bytes
    };
    w.stack.push(stack_name);
    w.depth += 1;
    w.in_start_tag = true;
    w.state = WriterState::Element;

    0
}

/// Write an element with inline content.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextWriterWriteElement(xmlTextWriterPtr writer,
///                                const xmlChar *name,
///                                const xmlChar *content);
/// ```
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
/// - `name`, `content` must be valid null-terminated strings or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterWriteElement(
    writer: *mut XmlTextWriter,
    name: *const xmlChar,
    content: *const xmlChar,
) -> c_int {
    if writer.is_null() || name.is_null() {
        return -1;
    }
    let ret = xmlTextWriterStartElement(writer, name);
    if ret != 0 {
        return ret;
    }
    if !content.is_null() {
        let ret2 = xmlTextWriterWriteString(writer, content);
        if ret2 != 0 {
            return ret2;
        }
    }
    xmlTextWriterEndElement(writer)
}

/// Write a namespaced element with inline content.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextWriterWriteElementNS(xmlTextWriterPtr writer,
///                                  const xmlChar *prefix,
///                                  const xmlChar *name,
///                                  const xmlChar *nsURI,
///                                  const xmlChar *content);
/// ```
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
/// - `prefix`, `name`, `nsURI`, `content` must be valid null-terminated strings or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterWriteElementNS(
    writer: *mut XmlTextWriter,
    prefix: *const xmlChar,
    name: *const xmlChar,
    nsURI: *const xmlChar,
    content: *const xmlChar,
) -> c_int {
    if writer.is_null() || name.is_null() {
        return -1;
    }
    let ret = xmlTextWriterStartElementNS(writer, prefix, name, nsURI);
    if ret != 0 {
        return ret;
    }
    if !content.is_null() {
        let ret2 = xmlTextWriterWriteString(writer, content);
        if ret2 != 0 {
            return ret2;
        }
    }
    xmlTextWriterEndElement(writer)
}

/// Write a full end element (always writes `</name>`, never self-closing).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextWriterFullEndElement(xmlTextWriterPtr writer);
/// ```
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterFullEndElement(writer: *mut XmlTextWriter) -> c_int {
    if writer.is_null() {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };

    if w.depth <= 0 {
        return -1;
    }

    // Close any open start tag first (if we're still in the start tag, close it with `>`)
    if w.in_start_tag {
        w.write_byte(b'>');
        w.in_start_tag = false;
    }

    // Write indentation
    if w.indent != 0 {
        w.write_byte(b'\n');
        let indent_str = w.indent_string.clone();
        for _ in 0..(w.depth - 1) {
            w.write_slice(&indent_str);
        }
    }

    // Write `</name>`
    let name = w.stack.pop().unwrap_or_default();
    w.write_raw(b"</" as *const u8, 2);
    w.write_slice(&name);
    w.write_byte(b'>');

    w.depth -= 1;
    w.elem_stack.pop();
    w.state = WriterState::None;

    0
}

// ═══════════════════════════════════════════════════════════════════════════════
// Attribute writing
// ═══════════════════════════════════════════════════════════════════════════════

/// Write an attribute.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextWriterWriteAttribute(xmlTextWriterPtr writer,
///                                  const xmlChar *name,
///                                  const xmlChar *content);
/// ```
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
/// - `name`, `content` must be valid null-terminated strings or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterWriteAttribute(
    writer: *mut XmlTextWriter,
    name: *const xmlChar,
    content: *const xmlChar,
) -> c_int {
    if writer.is_null() || name.is_null() || content.is_null() {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };

    if !w.in_start_tag {
        return -1;
    }

    // Write ` name="`
    w.write_byte(b' ');
    w.write_str(name);
    w.write_raw(b"=\"" as *const u8, 2);

    // Write escaped content
    tree::serialize_attr_value(
        unsafe { &mut *((*w.output).buffer as *mut _xmlBuffer) },
        content,
    );

    w.write_byte(b'"');
    w.state = WriterState::Attribute;

    0
}

/// Write a namespaced attribute.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextWriterWriteAttributeNS(xmlTextWriterPtr writer,
///                                    const xmlChar *prefix,
///                                    const xmlChar *name,
///                                    const xmlChar *nsURI,
///                                    const xmlChar *content);
/// ```
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
/// - `prefix`, `name`, `nsURI`, `content` must be valid null-terminated strings or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterWriteAttributeNS(
    writer: *mut XmlTextWriter,
    prefix: *const xmlChar,
    name: *const xmlChar,
    nsURI: *const xmlChar,
    content: *const xmlChar,
) -> c_int {
    let _ = nsURI;
    if writer.is_null() || name.is_null() || content.is_null() {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };

    if !w.in_start_tag {
        return -1;
    }

    w.write_byte(b' ');

    if !prefix.is_null() {
        w.write_str(prefix);
        w.write_byte(b':');
    }
    w.write_str(name);

    w.write_raw(b"=\"" as *const u8, 2);

    // Write escaped content
    let buf = unsafe { &mut *((*w.output).buffer as *mut _xmlBuffer) };
    tree::serialize_attr_value(buf, content);

    w.write_byte(b'"');
    w.state = WriterState::Attribute;

    0
}

/// Write a formatted attribute.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextWriterWriteFormatAttribute(xmlTextWriterPtr writer,
///                                        const xmlChar *name,
///                                        ...);
/// ```
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
/// - `name` must be a valid null-terminated string.
#[no_mangle]

/// Start an attribute (to be written incrementally).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextWriterStartAttribute(xmlTextWriterPtr writer, const xmlChar *name);
/// ```
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
/// - `name` must be a valid null-terminated string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterStartAttribute(
    writer: *mut XmlTextWriter,
    name: *const xmlChar,
) -> c_int {
    if writer.is_null() || name.is_null() {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };

    if !w.in_start_tag {
        return -1;
    }

    w.write_byte(b' ');
    w.write_str(name);
    w.write_raw(b"=\"" as *const u8, 2);
    w.state = WriterState::Attribute;

    0
}

/// Start a namespaced attribute (to be written incrementally).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextWriterStartAttributeNS(xmlTextWriterPtr writer,
///                                    const xmlChar *prefix,
///                                    const xmlChar *name,
///                                    const xmlChar *nsURI);
/// ```
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
/// - `prefix`, `name`, `nsURI` must be valid null-terminated strings or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterStartAttributeNS(
    writer: *mut XmlTextWriter,
    prefix: *const xmlChar,
    name: *const xmlChar,
    nsURI: *const xmlChar,
) -> c_int {
    let _ = nsURI;
    if writer.is_null() || name.is_null() {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };

    if !w.in_start_tag {
        return -1;
    }

    w.write_byte(b' ');
    if !prefix.is_null() {
        w.write_str(prefix);
        w.write_byte(b':');
    }
    w.write_str(name);
    w.write_raw(b"=\"" as *const u8, 2);
    w.state = WriterState::Attribute;

    0
}

/// End an attribute (closes the attribute value quote).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextWriterEndAttribute(xmlTextWriterPtr writer);
/// ```
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterEndAttribute(writer: *mut XmlTextWriter) -> c_int {
    if writer.is_null() {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };

    if w.state != WriterState::Attribute {
        return -1;
    }

    w.write_byte(b'"');
    w.state = WriterState::Element;

    0
}

// ═══════════════════════════════════════════════════════════════════════════════
// Content writing
// ═══════════════════════════════════════════════════════════════════════════════

/// Write text content (with XML escaping).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextWriterWriteString(xmlTextWriterPtr writer, const xmlChar *content);
/// ```
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
/// - `content` must be a valid null-terminated xmlChar string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterWriteString(
    writer: *mut XmlTextWriter,
    content: *const xmlChar,
) -> c_int {
    if writer.is_null() || content.is_null() {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };

    w.close_start_tag();

    let buf = if w.output.is_null() {
        return -1;
    } else {
        unsafe { &mut *((*w.output).buffer as *mut _xmlBuffer) }
    };

    let len = tree::xml_strlen(content);
    tree::serialize_text(buf, content, len);

    0
}

/// Write raw content (no XML escaping).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextWriterWriteRaw(xmlTextWriterPtr writer, const xmlChar *content);
/// ```
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
/// - `content` must be a valid null-terminated xmlChar string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterWriteRaw(
    writer: *mut XmlTextWriter,
    content: *const xmlChar,
) -> c_int {
    if writer.is_null() || content.is_null() {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };

    w.close_start_tag();
    w.write_str(content);

    0
}

/// Write raw content with explicit length (no XML escaping).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextWriterWriteRawLen(xmlTextWriterPtr writer,
///                               const xmlChar *content,
///                               int len);
/// ```
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
/// - `content` must point to `len` valid bytes or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterWriteRawLen(
    writer: *mut XmlTextWriter,
    content: *const xmlChar,
    len: c_int,
) -> c_int {
    if writer.is_null() || content.is_null() || len <= 0 {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };

    w.close_start_tag();
    w.write_raw(content, len);

    0
}

/// Write a formatted string.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextWriterWriteFormatString(xmlTextWriterPtr writer, const char *fmt, ...);
/// ```
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
#[no_mangle]

/// Write Base64-encoded data.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextWriterWriteBase64(xmlTextWriterPtr writer,
///                               const char *data,
///                               int start,
///                               int len);
/// ```
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
/// - `data` must be a valid pointer to `start + len` bytes or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterWriteBase64(
    writer: *mut XmlTextWriter,
    data: *const c_char,
    start: c_int,
    len: c_int,
) -> c_int {
    if writer.is_null() || data.is_null() || len <= 0 || start < 0 {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };

    w.close_start_tag();

    // Base64 encode the data
    let data_slice =
        unsafe { core::slice::from_raw_parts(data.add(start as usize) as *const u8, len as usize) };
    let encoded = base64_encode(data_slice);
    w.write_slice(&encoded);

    0
}

/// Write BinHex-encoded data.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextWriterWriteBinHex(xmlTextWriterPtr writer,
///                               const char *data,
///                               int start,
///                               int len);
/// ```
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
/// - `data` must be a valid pointer to `start + len` bytes or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterWriteBinHex(
    writer: *mut XmlTextWriter,
    data: *const c_char,
    start: c_int,
    len: c_int,
) -> c_int {
    if writer.is_null() || data.is_null() || len <= 0 || start < 0 {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };

    w.close_start_tag();

    // Hex encode the data
    let data_slice =
        unsafe { core::slice::from_raw_parts(data.add(start as usize) as *const u8, len as usize) };
    let encoded = hex_encode(data_slice);
    w.write_slice(&encoded);

    0
}

/// Write a CDATA section.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextWriterWriteCDATA(xmlTextWriterPtr writer, const xmlChar *content);
/// ```
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
/// - `content` must be a valid null-terminated xmlChar string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterWriteCDATA(
    writer: *mut XmlTextWriter,
    content: *const xmlChar,
) -> c_int {
    if writer.is_null() {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };

    w.close_start_tag();

    w.write_raw(b"<![CDATA[" as *const u8, 9);
    if !content.is_null() {
        w.write_str(content);
    }
    w.write_raw(b"]]>" as *const u8, 3);

    0
}

/// Start a CDATA section.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextWriterStartCDATA(xmlTextWriterPtr writer);
/// ```
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterStartCDATA(writer: *mut XmlTextWriter) -> c_int {
    if writer.is_null() {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };

    w.close_start_tag();
    w.write_raw(b"<![CDATA[" as *const u8, 9);
    w.state = WriterState::CData;

    0
}

/// End a CDATA section.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextWriterEndCDATA(xmlTextWriterPtr writer);
/// ```
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterEndCDATA(writer: *mut XmlTextWriter) -> c_int {
    if writer.is_null() {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };

    if w.state != WriterState::CData {
        return -1;
    }

    w.write_raw(b"]]>" as *const u8, 3);
    w.state = WriterState::None;

    0
}

/// Write a comment.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextWriterWriteComment(xmlTextWriterPtr writer, const xmlChar *content);
/// ```
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
/// - `content` must be a valid null-terminated xmlChar string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterWriteComment(
    writer: *mut XmlTextWriter,
    content: *const xmlChar,
) -> c_int {
    if writer.is_null() {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };

    w.close_start_tag();
    w.write_indent();
    w.write_raw(b"<!--" as *const u8, 4);
    if !content.is_null() {
        w.write_str(content);
    }
    w.write_raw(b"-->" as *const u8, 3);

    0
}

/// Start a comment.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextWriterStartComment(xmlTextWriterPtr writer);
/// ```
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterStartComment(writer: *mut XmlTextWriter) -> c_int {
    if writer.is_null() {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };

    w.close_start_tag();
    w.write_indent();
    w.write_raw(b"<!--" as *const u8, 4);
    w.state = WriterState::Comment;

    0
}

/// End a comment.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextWriterEndComment(xmlTextWriterPtr writer);
/// ```
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterEndComment(writer: *mut XmlTextWriter) -> c_int {
    if writer.is_null() {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };

    if w.state != WriterState::Comment {
        return -1;
    }

    w.write_raw(b"-->" as *const u8, 3);
    w.state = WriterState::None;

    0
}

/// Write a processing instruction.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextWriterWritePI(xmlTextWriterPtr writer,
///                           const xmlChar *target,
///                           const xmlChar *content);
/// ```
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
/// - `target`, `content` must be valid null-terminated strings or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterWritePI(
    writer: *mut XmlTextWriter,
    target: *const xmlChar,
    content: *const xmlChar,
) -> c_int {
    if writer.is_null() || target.is_null() {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };

    w.close_start_tag();
    w.write_indent();
    w.write_raw(b"<?" as *const u8, 2);
    w.write_str(target);
    if !content.is_null() && unsafe { *content != 0 } {
        w.write_byte(b' ');
        w.write_str(content);
    }
    w.write_raw(b"?>" as *const u8, 2);

    0
}

/// Start a processing instruction.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextWriterStartPI(xmlTextWriterPtr writer, const xmlChar *target);
/// ```
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
/// - `target` must be a valid null-terminated string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterStartPI(
    writer: *mut XmlTextWriter,
    target: *const xmlChar,
) -> c_int {
    if writer.is_null() || target.is_null() {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };

    w.close_start_tag();
    w.write_indent();
    w.write_raw(b"<?" as *const u8, 2);
    w.write_str(target);
    w.write_byte(b' ');
    w.state = WriterState::PI;

    0
}

/// End a processing instruction.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextWriterEndPI(xmlTextWriterPtr writer);
/// ```
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterEndPI(writer: *mut XmlTextWriter) -> c_int {
    if writer.is_null() {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };

    if w.state != WriterState::PI {
        return -1;
    }

    w.write_raw(b"?>" as *const u8, 2);
    w.state = WriterState::None;

    0
}

// ═══════════════════════════════════════════════════════════════════════════════
// DTD writing
// ═══════════════════════════════════════════════════════════════════════════════

/// Write a DTD declaration.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextWriterWriteDTD(xmlTextWriterPtr writer,
///                            const xmlChar *name,
///                            const xmlChar *pubid,
///                            const xmlChar *sysid,
///                            const xmlChar *subset);
/// ```
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
/// - `name`, `pubid`, `sysid`, `subset` must be valid null-terminated strings or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterWriteDTD(
    writer: *mut XmlTextWriter,
    name: *const xmlChar,
    pubid: *const xmlChar,
    sysid: *const xmlChar,
    subset: *const xmlChar,
) -> c_int {
    if writer.is_null() || name.is_null() {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };

    w.close_start_tag();
    w.write_raw(b"<!DOCTYPE " as *const u8, 10);
    w.write_str(name);

    if !pubid.is_null() {
        w.write_raw(b" PUBLIC \"" as *const u8, 9);
        w.write_str(pubid);
        w.write_byte(b'"');
        if !sysid.is_null() {
            w.write_byte(b' ');
            w.write_byte(b'"');
            w.write_str(sysid);
            w.write_byte(b'"');
        }
    } else if !sysid.is_null() {
        w.write_raw(b" SYSTEM \"" as *const u8, 9);
        w.write_str(sysid);
        w.write_byte(b'"');
    }

    if !subset.is_null() {
        w.write_raw(b" [" as *const u8, 2);
        w.write_str(subset);
        w.write_byte(b']');
    }

    w.write_byte(b'>');

    if w.indent != 0 {
        w.write_byte(b'\n');
    }

    0
}

/// Write a DTD element declaration.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextWriterWriteDTDElement(xmlTextWriterPtr writer,
///                                   const xmlChar *name,
///                                   const xmlChar *content);
/// ```
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
/// - `name`, `content` must be valid null-terminated strings or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterWriteDTDElement(
    writer: *mut XmlTextWriter,
    name: *const xmlChar,
    content: *const xmlChar,
) -> c_int {
    if writer.is_null() || name.is_null() || content.is_null() {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };

    w.close_start_tag();
    w.write_indent();
    w.write_raw(b"<!ELEMENT " as *const u8, 10);
    w.write_str(name);
    w.write_byte(b' ');
    w.write_str(content);
    w.write_raw(b">" as *const u8, 1);

    0
}

/// Write a DTD attribute declaration.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextWriterWriteDTDAttribute(xmlTextWriterPtr writer,
///                                     const xmlChar *name,
///                                     const xmlChar *content);
/// ```
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
/// - `name`, `content` must be valid null-terminated strings or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterWriteDTDAttribute(
    writer: *mut XmlTextWriter,
    name: *const xmlChar,
    content: *const xmlChar,
) -> c_int {
    if writer.is_null() || name.is_null() || content.is_null() {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };

    w.close_start_tag();
    w.write_indent();
    w.write_raw(b"<!ATTLIST " as *const u8, 10);
    w.write_str(name);
    w.write_byte(b' ');
    w.write_str(content);
    w.write_raw(b">" as *const u8, 1);

    0
}

/// Write a DTD entity declaration.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextWriterWriteDTDEntity(xmlTextWriterPtr writer,
///                                  const xmlChar *name,
///                                  const xmlChar *content);
/// ```
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
/// - `name`, `content` must be valid null-terminated strings or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterWriteDTDEntity(
    writer: *mut XmlTextWriter,
    name: *const xmlChar,
    content: *const xmlChar,
) -> c_int {
    if writer.is_null() || name.is_null() || content.is_null() {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };

    w.close_start_tag();
    w.write_indent();
    w.write_raw(b"<!ENTITY " as *const u8, 9);
    w.write_str(name);
    w.write_raw(b" \"" as *const u8, 2);
    w.write_str(content);
    w.write_raw(b"\">" as *const u8, 2);

    0
}

/// Write a DTD notation declaration.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextWriterWriteDTDNotation(xmlTextWriterPtr writer,
///                                    const xmlChar *name,
///                                    const xmlChar *pubid,
///                                    const xmlChar *sysid);
/// ```
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
/// - `name`, `pubid`, `sysid` must be valid null-terminated strings or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterWriteDTDNotation(
    writer: *mut XmlTextWriter,
    name: *const xmlChar,
    pubid: *const xmlChar,
    sysid: *const xmlChar,
) -> c_int {
    if writer.is_null() || name.is_null() {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };

    w.close_start_tag();
    w.write_indent();
    w.write_raw(b"<!NOTATION " as *const u8, 11);
    w.write_str(name);

    if !pubid.is_null() {
        w.write_raw(b" PUBLIC \"" as *const u8, 9);
        w.write_str(pubid);
        w.write_byte(b'"');
        if !sysid.is_null() {
            w.write_byte(b' ');
            w.write_byte(b'"');
            w.write_str(sysid);
            w.write_byte(b'"');
        }
    } else if !sysid.is_null() {
        w.write_raw(b" SYSTEM \"" as *const u8, 9);
        w.write_str(sysid);
        w.write_byte(b'"');
    }

    w.write_raw(b">" as *const u8, 1);

    0
}

// ═══════════════════════════════════════════════════════════════════════════════
// Start/End DTD declaration
/// Start a DTD declaration.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextWriterStartDTD(xmlTextWriterPtr writer,
///                            const xmlChar *name,
///                            const xmlChar *pubid,
///                            const xmlChar *sysid);
/// ```
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
/// - `name`, `pubid`, `sysid` must be valid null-terminated strings or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterStartDTD(
    writer: *mut XmlTextWriter,
    name: *const xmlChar,
    pubid: *const xmlChar,
    sysid: *const xmlChar,
) -> c_int {
    if writer.is_null() || name.is_null() {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };

    w.close_start_tag();
    w.write_raw(b"<!DOCTYPE " as *const u8, 10);
    w.write_str(name);

    if !pubid.is_null() {
        w.write_raw(b" PUBLIC \"" as *const u8, 9);
        w.write_str(pubid);
        w.write_byte(b'"');
        if !sysid.is_null() {
            w.write_byte(b' ');
            w.write_byte(b'"');
            w.write_str(sysid);
            w.write_byte(b'"');
        }
    } else if !sysid.is_null() {
        w.write_raw(b" SYSTEM \"" as *const u8, 9);
        w.write_str(sysid);
        w.write_byte(b'"');
    }

    w.write_raw(b" [" as *const u8, 2);
    w.state = WriterState::DTD;

    if w.indent != 0 {
        w.write_byte(b'\n');
    }

    0
}

/// End a DTD declaration.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextWriterEndDTD(xmlTextWriterPtr writer);
/// ```
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterEndDTD(writer: *mut XmlTextWriter) -> c_int {
    if writer.is_null() {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };

    if w.state != WriterState::DTD {
        return -1;
    }

    if w.indent != 0 {
        w.write_byte(b'\n');
    }
    w.write_raw(b"]>" as *const u8, 2);

    if w.indent != 0 {
        w.write_byte(b'\n');
    }

    w.state = WriterState::None;

    0
}

/// Start a DTD element declaration.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextWriterStartDTDElement(xmlTextWriterPtr writer, const xmlChar *name);
/// ```
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
/// - `name` must be a valid null-terminated string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterStartDTDElement(
    writer: *mut XmlTextWriter,
    name: *const xmlChar,
) -> c_int {
    if writer.is_null() || name.is_null() {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };

    w.close_start_tag();
    w.write_indent();
    w.write_raw(b"<!ELEMENT " as *const u8, 10);
    w.write_str(name);
    w.write_byte(b' ');
    w.state = WriterState::DTDElem;

    0
}

/// End a DTD element declaration.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextWriterEndDTDElement(xmlTextWriterPtr writer);
/// ```
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterEndDTDElement(writer: *mut XmlTextWriter) -> c_int {
    if writer.is_null() {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };

    if w.state != WriterState::DTDElem {
        return -1;
    }

    w.write_raw(b">" as *const u8, 1);
    w.state = WriterState::DTD;

    0
}

/// Start a DTD attribute declaration.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextWriterStartDTDAttribute(xmlTextWriterPtr writer, const xmlChar *name);
/// ```
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
/// - `name` must be a valid null-terminated string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterStartDTDAttribute(
    writer: *mut XmlTextWriter,
    name: *const xmlChar,
) -> c_int {
    if writer.is_null() || name.is_null() {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };

    w.close_start_tag();
    w.write_indent();
    w.write_raw(b"<!ATTLIST " as *const u8, 10);
    w.write_str(name);
    w.write_byte(b' ');
    w.state = WriterState::DTDAttr;

    0
}

/// End a DTD attribute declaration.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextWriterEndDTDAttribute(xmlTextWriterPtr writer);
/// ```
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterEndDTDAttribute(writer: *mut XmlTextWriter) -> c_int {
    if writer.is_null() {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };

    if w.state != WriterState::DTDAttr {
        return -1;
    }

    w.write_raw(b">" as *const u8, 1);
    w.state = WriterState::DTD;

    0
}

/// Start a DTD entity declaration.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextWriterStartDTDEntity(xmlTextWriterPtr writer, const xmlChar *name);
/// ```
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
/// - `name` must be a valid null-terminated string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterStartDTDEntity(
    writer: *mut XmlTextWriter,
    name: *const xmlChar,
) -> c_int {
    if writer.is_null() || name.is_null() {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };

    w.close_start_tag();
    w.write_indent();
    w.write_raw(b"<!ENTITY " as *const u8, 9);
    w.write_str(name);
    w.write_raw(b" \"" as *const u8, 2);
    w.state = WriterState::DTDEntity;

    0
}

/// End a DTD entity declaration.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextWriterEndDTDEntity(xmlTextWriterPtr writer);
/// ```
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterEndDTDEntity(writer: *mut XmlTextWriter) -> c_int {
    if writer.is_null() {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };

    if w.state != WriterState::DTDEntity {
        return -1;
    }

    w.write_raw(b"\">" as *const u8, 2);
    w.state = WriterState::DTD;

    0
}

// ═══════════════════════════════════════════════════════════════════════════════
// Output management
// ═══════════════════════════════════════════════════════════════════════════════

/// Flush the writer's output buffer.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextWriterFlush(xmlTextWriterPtr writer);
/// ```
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterFlush(writer: *mut XmlTextWriter) -> c_int {
    if writer.is_null() {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };

    if w.output.is_null() {
        return -1;
    }

    // Close any open start tag
    w.close_start_tag();

    io::output_buffer_flush(w.output)
}

/// Set indentation on/off.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextWriterSetIndent(xmlTextWriterPtr writer, int indent);
/// ```
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterSetIndent(
    writer: *mut XmlTextWriter,
    indent: c_int,
) -> c_int {
    if writer.is_null() {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    unsafe { (*writer).indent = indent };
    0
}

/// Set the indentation string.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextWriterSetIndentString(xmlTextWriterPtr writer, const xmlChar *str);
/// ```
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
/// - `str` must be a valid null-terminated xmlChar string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterSetIndentString(
    writer: *mut XmlTextWriter,
    str: *const xmlChar,
) -> c_int {
    if writer.is_null() || str.is_null() {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };
    w.indent_string = unsafe { c_str_to_vec(str) };
    0
}

// ═══════════════════════════════════════════════════════════════════════════════
// Internal helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Convert a null-terminated C string to a Vec<u8> (including the null terminator).
///
/// # SAFETY
///
/// - `s` must be a valid pointer to a null-terminated string.
unsafe fn c_str_to_vec(s: *const u8) -> Vec<u8> {
    if s.is_null() {
        return Vec::new();
    }
    let len = tree::xml_strlen(s);
    let mut v = Vec::with_capacity(len as usize + 1);
    unsafe {
        for i in 0..len as isize {
            v.push(*s.offset(i));
        }
        v.push(0);
    }
    v
}

/// Base64 encode a byte slice.
fn base64_encode(data: &[u8]) -> Vec<u8> {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = Vec::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0];
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);

        result.push(CHARS[((b0 >> 2) & 0x3F) as usize]);
        result.push(CHARS[(((b0 << 4) | (b1 >> 4)) & 0x3F) as usize]);
        result.push(if chunk.len() > 1 {
            CHARS[(((b1 << 2) | (b2 >> 6)) & 0x3F) as usize]
        } else {
            b'='
        });
        result.push(if chunk.len() > 2 {
            CHARS[(b2 & 0x3F) as usize]
        } else {
            b'='
        });
    }
    result
}

/// Hex encode a byte slice (lowercase).
fn hex_encode(data: &[u8]) -> Vec<u8> {
    const CHARS: &[u8] = b"0123456789abcdef";
    let mut result = Vec::with_capacity(data.len() * 2);
    for &b in data {
        result.push(CHARS[((b >> 4) & 0x0F) as usize]);
        result.push(CHARS[(b & 0x0F) as usize]);
    }
    result
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use core::ptr;

    /// Helper: create a memory buffer writer for testing.
    unsafe fn create_test_writer() -> (*mut XmlTextWriter, *mut _xmlBuffer) {
        let buf = io::buf_create(256);
        assert!(!buf.is_null(), "buf_create failed");
        let out = io::output_buffer_create_buffer(buf, ptr::null_mut());
        assert!(!out.is_null(), "output_buffer_create_buffer failed");
        let writer = xmlNewTextWriter(out);
        assert!(!writer.is_null(), "xmlNewTextWriter failed");
        (writer, buf)
    }

    /// Helper: get the buffer content as a string.
    unsafe fn buf_to_string(buf: *mut _xmlBuffer) -> String {
        let content = io::buf_content(buf);
        let len = io::buf_length(buf);
        if content.is_null() || len <= 0 {
            return String::new();
        }
        let slice = unsafe { core::slice::from_raw_parts(content, len as usize) };
        String::from_utf8_lossy(slice).to_string()
    }

    /// Helper: flush writer and return buffer content.
    unsafe fn flush_and_get(writer: *mut XmlTextWriter, buf: *mut _xmlBuffer) -> String {
        xmlTextWriterFlush(writer);
        buf_to_string(buf)
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Test: Write a simple document
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_write_simple_document() {
        unsafe {
            let (writer, buf) = create_test_writer();

            let r = xmlTextWriterStartDocument(writer, ptr::null(), ptr::null(), ptr::null());
            assert_eq!(r, 0, "StartDocument failed");

            let r = xmlTextWriterStartElement(writer, b"root\0" as *const u8);
            assert_eq!(r, 0, "StartElement(root) failed");

            let r = xmlTextWriterWriteString(writer, b"Hello, World!\0" as *const u8);
            assert_eq!(r, 0, "WriteString failed");

            let r = xmlTextWriterEndElement(writer);
            assert_eq!(r, 0, "EndElement failed");

            let r = xmlTextWriterEndDocument(writer);
            assert_eq!(r, 0, "EndDocument failed");

            let result = flush_and_get(writer, buf);
            assert!(
                result.contains("<?xml version=\"1.0\"?>"),
                "Missing XML declaration. Got: {}",
                result
            );
            assert!(
                result.contains("<root>"),
                "Missing <root> start tag. Got: {}",
                result
            );
            assert!(
                result.contains("Hello, World!"),
                "Missing content. Got: {}",
                result
            );
            assert!(
                result.contains("</root>"),
                "Missing </root> end tag. Got: {}",
                result
            );

            xmlFreeTextWriter(writer);
            io::buf_free(buf);
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Test: Write elements with attributes
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_write_element_with_attributes() {
        unsafe {
            let (writer, buf) = create_test_writer();

            xmlTextWriterStartDocument(writer, ptr::null(), ptr::null(), ptr::null());
            xmlTextWriterStartElement(writer, b"root\0" as *const u8);
            xmlTextWriterWriteAttribute(writer, b"id\0" as *const u8, b"123\0" as *const u8);
            xmlTextWriterWriteAttribute(
                writer,
                b"name\0" as *const u8,
                b"test & demo\0" as *const u8,
            );
            xmlTextWriterEndElement(writer);
            xmlTextWriterEndDocument(writer);

            let result = flush_and_get(writer, buf);
            assert!(
                result.contains("id=\"123\""),
                "Missing id attribute. Got: {}",
                result
            );
            assert!(
                result.contains("name=\"test &amp; demo\""),
                "Missing or improperly escaped name attribute. Got: {}",
                result
            );
            assert!(
                result.contains("<root"),
                "Missing root element. Got: {}",
                result
            );

            xmlFreeTextWriter(writer);
            io::buf_free(buf);
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Test: Write with namespaces
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_write_with_namespaces() {
        unsafe {
            let (writer, buf) = create_test_writer();

            xmlTextWriterStartDocument(writer, ptr::null(), ptr::null(), ptr::null());
            xmlTextWriterStartElementNS(
                writer,
                b"ns\0" as *const u8,
                b"root\0" as *const u8,
                b"http://example.com/ns\0" as *const u8,
            );
            xmlTextWriterWriteAttributeNS(
                writer,
                ptr::null(),
                b"attr\0" as *const u8,
                ptr::null(),
                b"value\0" as *const u8,
            );
            xmlTextWriterEndElement(writer);
            xmlTextWriterEndDocument(writer);

            let result = flush_and_get(writer, buf);
            assert!(
                result.contains("ns:root"),
                "Missing namespace prefix. Got: {}",
                result
            );
            assert!(
                result.contains("xmlns:ns=\"http://example.com/ns\""),
                "Missing xmlns declaration. Got: {}",
                result
            );

            xmlFreeTextWriter(writer);
            io::buf_free(buf);
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Test: Write text, CDATA, comments, PIs
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_write_text_cdata_comment_pi() {
        unsafe {
            let (writer, buf) = create_test_writer();

            xmlTextWriterStartDocument(writer, ptr::null(), ptr::null(), ptr::null());

            xmlTextWriterStartElement(writer, b"doc\0" as *const u8);
            xmlTextWriterWriteString(writer, b"text content\0" as *const u8);
            xmlTextWriterEndElement(writer);

            xmlTextWriterWriteComment(writer, b"a comment\0" as *const u8);

            xmlTextWriterWritePI(writer, b"target\0" as *const u8, b"data\0" as *const u8);

            xmlTextWriterStartElement(writer, b"cdata\0" as *const u8);
            xmlTextWriterWriteCDATA(writer, b"<greeting>Hello</greeting>\0" as *const u8);
            xmlTextWriterEndElement(writer);

            xmlTextWriterEndDocument(writer);

            let result = flush_and_get(writer, buf);
            assert!(
                result.contains("text content"),
                "Missing text content. Got: {}",
                result
            );
            assert!(
                result.contains("<!--a comment-->"),
                "Missing comment. Got: {}",
                result
            );
            assert!(
                result.contains("<?target data?>"),
                "Missing PI. Got: {}",
                result
            );
            assert!(
                result.contains("<![CDATA["),
                "Missing CDATA start. Got: {}",
                result
            );
            assert!(
                result.contains("<greeting>Hello</greeting>"),
                "Missing CDATA content. Got: {}",
                result
            );

            xmlFreeTextWriter(writer);
            io::buf_free(buf);
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Test: DTD writing
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_write_dtd() {
        unsafe {
            let (writer, buf) = create_test_writer();

            xmlTextWriterStartDocument(writer, ptr::null(), ptr::null(), ptr::null());

            xmlTextWriterWriteDTD(
                writer,
                b"html\0" as *const u8,
                ptr::null(),
                b"http://www.w3.org/TR/html4/strict.dtd\0" as *const u8,
                ptr::null(),
            );

            xmlTextWriterStartElement(writer, b"html\0" as *const u8);
            xmlTextWriterEndElement(writer);
            xmlTextWriterEndDocument(writer);

            let result = flush_and_get(writer, buf);
            assert!(
                result.contains("<!DOCTYPE html SYSTEM"),
                "Missing DTD. Got: {}",
                result
            );

            xmlFreeTextWriter(writer);
            io::buf_free(buf);
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Test: DTD with internal subset declarations
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_write_dtd_with_subset() {
        unsafe {
            let (writer, buf) = create_test_writer();

            xmlTextWriterStartDocument(writer, ptr::null(), ptr::null(), ptr::null());

            xmlTextWriterStartDTD(writer, b"root\0" as *const u8, ptr::null(), ptr::null());
            xmlTextWriterWriteDTDElement(
                writer,
                b"child\0" as *const u8,
                b"(#PCDATA)\0" as *const u8,
            );
            xmlTextWriterWriteDTDAttribute(
                writer,
                b"child\0" as *const u8,
                b"id CDATA #IMPLIED\0" as *const u8,
            );
            xmlTextWriterWriteDTDEntity(
                writer,
                b"copy\0" as *const u8,
                b"Copyright Me\0" as *const u8,
            );
            xmlTextWriterWriteDTDNotation(
                writer,
                b"note\0" as *const u8,
                b"PublicID\0" as *const u8,
                ptr::null(),
            );
            xmlTextWriterEndDTD(writer);

            xmlTextWriterStartElement(writer, b"root\0" as *const u8);
            xmlTextWriterEndElement(writer);
            xmlTextWriterEndDocument(writer);

            let result = flush_and_get(writer, buf);
            assert!(
                result.contains("<!DOCTYPE root"),
                "Missing DTD start. Got: {}",
                result
            );
            assert!(
                result.contains("<!ELEMENT child (#PCDATA)>"),
                "Missing DTD element. Got: {}",
                result
            );
            assert!(
                result.contains("<!ATTLIST child id CDATA #IMPLIED>"),
                "Missing DTD attribute. Got: {}",
                result
            );
            assert!(
                result.contains("<!ENTITY copy \"Copyright Me\">"),
                "Missing DTD entity. Got: {}",
                result
            );
            assert!(
                result.contains("<!NOTATION note PUBLIC \"PublicID\">"),
                "Missing DTD notation. Got: {}",
                result
            );

            xmlFreeTextWriter(writer);
            io::buf_free(buf);
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Test: Indentation control
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_indentation_control() {
        unsafe {
            let (writer, buf) = create_test_writer();

            // Enable indentation with tabs
            xmlTextWriterSetIndent(writer, 1);
            xmlTextWriterSetIndentString(writer, b"\t\0" as *const u8);

            xmlTextWriterStartDocument(writer, ptr::null(), ptr::null(), ptr::null());
            xmlTextWriterStartElement(writer, b"root\0" as *const u8);
            xmlTextWriterStartElement(writer, b"child\0" as *const u8);
            xmlTextWriterWriteString(writer, b"content\0" as *const u8);
            xmlTextWriterEndElement(writer);
            xmlTextWriterEndElement(writer);
            xmlTextWriterEndDocument(writer);

            let result = flush_and_get(writer, buf);

            // Check that we have indentation
            assert!(
                result.contains('\t'),
                "Expected tab indentation. Got: {}",
                result
            );
            // Check the XML declaration and elements are present
            assert!(result.contains("<root>"), "Missing root. Got: {}", result);
            assert!(result.contains("<child>"), "Missing child. Got: {}", result);

            xmlFreeTextWriter(writer);
            io::buf_free(buf);
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Test: Memory output
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_memory_output() {
        unsafe {
            let buf = io::buf_create(256);
            assert!(!buf.is_null(), "buf_create failed");

            let writer = xmlNewTextWriterMemory(buf, 0);
            assert!(!writer.is_null(), "xmlNewTextWriterMemory failed");

            xmlTextWriterStartDocument(writer, ptr::null(), ptr::null(), ptr::null());
            xmlTextWriterStartElement(writer, b"root\0" as *const u8);
            xmlTextWriterWriteString(writer, b"memory test\0" as *const u8);
            xmlTextWriterEndElement(writer);
            xmlTextWriterEndDocument(writer);

            xmlTextWriterFlush(writer);
            let result = buf_to_string(buf);
            assert!(
                result.contains("memory test"),
                "Missing content in memory output. Got: {}",
                result
            );

            xmlFreeTextWriter(writer);
            io::buf_free(buf);
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Test: Flush and close
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_flush_and_close() {
        unsafe {
            let (writer, buf) = create_test_writer();

            xmlTextWriterStartDocument(writer, ptr::null(), ptr::null(), ptr::null());
            xmlTextWriterStartElement(writer, b"root\0" as *const u8);
            xmlTextWriterWriteString(writer, b"flush me\0" as *const u8);

            // Flush mid-document
            let r = xmlTextWriterFlush(writer);
            assert!(r >= 0, "Flush should return non-negative, got {}", r);

            xmlTextWriterEndElement(writer);
            xmlTextWriterEndDocument(writer);

            xmlFreeTextWriter(writer);
            io::buf_free(buf);
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Test: Edge cases — null writer, null parameters
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_null_handling() {
        unsafe {
            // All functions should gracefully handle NULL writer
            assert_eq!(
                xmlTextWriterStartDocument(ptr::null_mut(), ptr::null(), ptr::null(), ptr::null()),
                -1
            );
            assert_eq!(xmlTextWriterEndDocument(ptr::null_mut()), -1);
            assert_eq!(
                xmlTextWriterStartElement(ptr::null_mut(), b"x\0" as *const u8),
                -1
            );
            assert_eq!(xmlTextWriterEndElement(ptr::null_mut()), -1);
            assert_eq!(
                xmlTextWriterWriteString(ptr::null_mut(), b"x\0" as *const u8),
                -1
            );
            assert_eq!(
                xmlTextWriterWriteRaw(ptr::null_mut(), b"x\0" as *const u8),
                -1
            );
            assert_eq!(
                xmlTextWriterWriteCDATA(ptr::null_mut(), b"x\0" as *const u8),
                -1
            );
            assert_eq!(
                xmlTextWriterWriteComment(ptr::null_mut(), b"x\0" as *const u8),
                -1
            );
            assert_eq!(
                xmlTextWriterWritePI(ptr::null_mut(), b"x\0" as *const u8, ptr::null()),
                -1
            );
            assert_eq!(xmlTextWriterFlush(ptr::null_mut()), -1);
            assert_eq!(xmlTextWriterSetIndent(ptr::null_mut(), 1), -1);
            assert_eq!(
                xmlTextWriterSetIndentString(ptr::null_mut(), b"  \0" as *const u8),
                -1
            );
            assert_eq!(
                xmlTextWriterWriteAttribute(
                    ptr::null_mut(),
                    b"n\0" as *const u8,
                    b"v\0" as *const u8
                ),
                -1
            );

            // Null writer should not crash free
            xmlFreeTextWriter(ptr::null_mut());
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Test: Nested elements
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_nested_elements() {
        unsafe {
            let (writer, buf) = create_test_writer();

            xmlTextWriterStartDocument(writer, ptr::null(), ptr::null(), ptr::null());
            xmlTextWriterStartElement(writer, b"a\0" as *const u8);
            xmlTextWriterStartElement(writer, b"b\0" as *const u8);
            xmlTextWriterStartElement(writer, b"c\0" as *const u8);
            xmlTextWriterWriteString(writer, b"deep\0" as *const u8);
            xmlTextWriterEndElement(writer);
            xmlTextWriterEndElement(writer);
            xmlTextWriterEndElement(writer);
            xmlTextWriterEndDocument(writer);

            let result = flush_and_get(writer, buf);
            assert!(result.contains("<a>"), "Missing <a>. Got: {}", result);
            assert!(result.contains("<b>"), "Missing <b>. Got: {}", result);
            assert!(result.contains("<c>"), "Missing <c>. Got: {}", result);
            assert!(result.contains("</a>"), "Missing </a>. Got: {}", result);
            assert!(result.contains("</b>"), "Missing </b>. Got: {}", result);
            assert!(result.contains("</c>"), "Missing </c>. Got: {}", result);

            xmlFreeTextWriter(writer);
            io::buf_free(buf);
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Test: Self-closing element (no content)
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_self_closing_element() {
        unsafe {
            let (writer, buf) = create_test_writer();

            xmlTextWriterStartDocument(writer, ptr::null(), ptr::null(), ptr::null());
            xmlTextWriterStartElement(writer, b"empty\0" as *const u8);
            xmlTextWriterEndElement(writer);
            xmlTextWriterEndDocument(writer);

            let result = flush_and_get(writer, buf);
            assert!(
                result.contains("<empty/>"),
                "Expected self-closing <empty/>. Got: {}",
                result
            );

            xmlFreeTextWriter(writer);
            io::buf_free(buf);
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Test: Full end element (not self-closing)
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_full_end_element() {
        unsafe {
            let (writer, buf) = create_test_writer();

            xmlTextWriterStartDocument(writer, ptr::null(), ptr::null(), ptr::null());
            xmlTextWriterStartElement(writer, b"container\0" as *const u8);
            xmlTextWriterFullEndElement(writer);
            xmlTextWriterEndDocument(writer);

            let result = flush_and_get(writer, buf);
            assert!(
                result.contains("<container>"),
                "Missing <container>. Got: {}",
                result
            );
            assert!(
                result.contains("</container>"),
                "Missing </container>. Got: {}",
                result
            );

            xmlFreeTextWriter(writer);
            io::buf_free(buf);
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Test: WriteElement (element with inline content)
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_write_element_inline() {
        unsafe {
            let (writer, buf) = create_test_writer();

            xmlTextWriterStartDocument(writer, ptr::null(), ptr::null(), ptr::null());
            xmlTextWriterWriteElement(writer, b"greeting\0" as *const u8, b"Hello\0" as *const u8);
            xmlTextWriterEndDocument(writer);

            let result = flush_and_get(writer, buf);
            assert!(
                result.contains("<greeting>Hello</greeting>"),
                "Expected <greeting>Hello</greeting>. Got: {}",
                result
            );

            xmlFreeTextWriter(writer);
            io::buf_free(buf);
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Test: XML escaping in text content
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_text_escaping() {
        unsafe {
            let (writer, buf) = create_test_writer();

            xmlTextWriterStartDocument(writer, ptr::null(), ptr::null(), ptr::null());
            xmlTextWriterStartElement(writer, b"esc\0" as *const u8);
            xmlTextWriterWriteString(writer, b"a < b & b > a\0" as *const u8);
            xmlTextWriterEndElement(writer);
            xmlTextWriterEndDocument(writer);

            let result = flush_and_get(writer, buf);
            assert!(
                result.contains("a &lt; b &amp; b &gt; a"),
                "Expected escaped content. Got: {}",
                result
            );

            xmlFreeTextWriter(writer);
            io::buf_free(buf);
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Test: Raw content (no escaping)
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_raw_content() {
        unsafe {
            let (writer, buf) = create_test_writer();

            xmlTextWriterStartDocument(writer, ptr::null(), ptr::null(), ptr::null());
            xmlTextWriterStartElement(writer, b"raw\0" as *const u8);
            xmlTextWriterWriteRaw(writer, b"<unencoded>&special;</unencoded>\0" as *const u8);
            xmlTextWriterEndElement(writer);
            xmlTextWriterEndDocument(writer);

            let result = flush_and_get(writer, buf);
            assert!(
                result.contains("<unencoded>&special;</unencoded>"),
                "Expected raw unencoded content. Got: {}",
                result
            );

            xmlFreeTextWriter(writer);
            io::buf_free(buf);
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Test: Base64 writing
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_base64_write() {
        unsafe {
            let (writer, buf) = create_test_writer();

            xmlTextWriterStartDocument(writer, ptr::null(), ptr::null(), ptr::null());
            xmlTextWriterStartElement(writer, b"data\0" as *const u8);
            let test_data = b"Hello, World!";
            xmlTextWriterWriteBase64(
                writer,
                test_data.as_ptr() as *const c_char,
                0,
                test_data.len() as c_int,
            );
            xmlTextWriterEndElement(writer);
            xmlTextWriterEndDocument(writer);

            let result = flush_and_get(writer, buf);
            assert!(
                result.contains("SGVsbG8sIFdvcmxkIQ"),
                "Expected Base64-encoded content. Got: {}",
                result
            );

            xmlFreeTextWriter(writer);
            io::buf_free(buf);
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Test: Incremental CDATA/comment/PI
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_incremental_cdata_comment_pi() {
        unsafe {
            let (writer, buf) = create_test_writer();

            xmlTextWriterStartDocument(writer, ptr::null(), ptr::null(), ptr::null());

            // Incremental CDATA
            xmlTextWriterStartElement(writer, b"inc\0" as *const u8);
            xmlTextWriterStartCDATA(writer);
            xmlTextWriterWriteString(writer, b"cdata content\0" as *const u8);
            xmlTextWriterEndCDATA(writer);
            xmlTextWriterEndElement(writer);

            // Incremental comment
            xmlTextWriterStartComment(writer);
            xmlTextWriterWriteString(writer, b"comment text\0" as *const u8);
            xmlTextWriterEndComment(writer);

            // Incremental PI
            xmlTextWriterStartPI(writer, b"xml-stylesheet\0" as *const u8);
            xmlTextWriterWriteString(
                writer,
                b"type=\"text/xsl\" href=\"style.xsl\"\0" as *const u8,
            );
            xmlTextWriterEndPI(writer);

            xmlTextWriterEndDocument(writer);

            let result = flush_and_get(writer, buf);
            assert!(
                result.contains("<![CDATA["),
                "Missing CDATA. Got: {}",
                result
            );
            assert!(
                result.contains("<!--comment text-->"),
                "Missing comment. Got: {}",
                result
            );
            assert!(
                result.contains("<?xml-stylesheet"),
                "Missing PI. Got: {}",
                result
            );

            xmlFreeTextWriter(writer);
            io::buf_free(buf);
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Test: xmlNewTextWriterFilename returns NULL for NULL uri
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_new_writer_filename_null() {
        unsafe {
            let writer = xmlNewTextWriterFilename(ptr::null(), 0);
            assert!(writer.is_null(), "Expected NULL for null URI");
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Test: xmlNewTextWriter returns NULL for NULL output
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_new_writer_null_output() {
        unsafe {
            let writer = xmlNewTextWriter(ptr::null_mut());
            assert!(writer.is_null(), "Expected NULL for null output");
        }
    }
}
