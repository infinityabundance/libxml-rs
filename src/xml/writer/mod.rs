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
//! This module mirrors the libxml2 `xmlTextWriter` API defined in
//! `xmlwriter.h` (upstream xmlwriter.c, parity target libxml2 2.15.3
//! oracle). The writer maintains a state machine that tracks whether we are
//! inside an element start tag (attributes can be written), inside an
//! attribute value, inside a CDATA section, or inside the DTD internal
//! subset.
//!
//! # Upstream contract
//!
//! Mirrors upstream `xmlwriter.c` (`SRC-LIBXML2-2.15.0-XMLWRITER-C`): the
//! full `xmlTextWriter*` surface — start/end element (incl. NS forms),
//! attributes, text/CDATA/comments/PI, DTD declarations, formatting and
//! the variadic Format functions (R-000155 inline-asm shims).
//!
//! # Conceptual behavior
//!
//! Implements the streaming writer as a state machine over an output
//! buffer: element start tags defer namespace declarations to close
//! (R-000153), the DTD internal-subset bracket is written by the first
//! child declaration (R-000152), return values follow the encoder-
//! dependent byte-count contract (R-000151), and escaping follows
//! xmlEncodeSpecialChars / xmlBufAttrSerializeTxtContent (R-000154).
//!
//! # Ownership & safety invariants
//!
//! The writer owns its output buffer (created via `xmlNewTextWriter*`) and
//! must be closed with `xmlFreeTextWriter` after balanced End* calls;
//! with `xmlNewTextWriterMemory` the buffer is borrowed from the caller
//! (OWNERSHIP_ATLAS §5). Internal Vec state is dropped by the free path;
//! the Format shims restore the stack exactly (R-000155).
//!
//! # Historical quirks & epochs
//!
//! The deferred-separator and return-value behaviors are upstream
//! xmlwriter.c / xmlIO.c contracts locked by WRITER-001 against the 2.15.3
//! oracle (R-000151..R-000156); the 2.13/2.15 series kept them stable, so
//! the crate targets that epoch.
//!
//! # Deliberate oddities
//!
//! The apostrophe is never escaped (R-000154), the default indent string
//! is one space, StartPI writes no indentation, and encoder-installed
//! writers report 0 bytes below the 256-byte conversion threshold — all
//! deliberate reproductions of upstream quirks.
//!
//! # Proving courts
//!
//! WRITER-001 (courts/suites/data-abi/writer-family-probe.c) requires
//! byte-identical output and return values (incl. enddoc counts, DTD
//! brackets, deferred xmlns, >6-GP variadic overflow); cargo test runs
//! the writer unit suites.
//!
//! # Tempting simplifications that would break parity
//!
//! Do not write xmlns inline at StartElementNS (R-000153), do not emit the
//! DTD bracket from StartDTD (R-000152), do not return raw byte counts
//! from encoder-active writes (R-000151) and do not escape the apostrophe
//! (R-000154) — every one of these is observable through the C ABI.
//!
//! # Safety
//!
//! The only module-level `unsafe` block is the `unsafe extern` declaration
//! of the platform `vsnprintf` (system libc). It is only invoked through
//! `vformat_buf`, which supplies a valid destination buffer, a valid printf
//! format string and a valid va_list pointer, and validates the return
//! value before using the output.

#![allow(
    missing_docs,
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals
)]

use core::ffi::c_void;
use core::ptr;
use std::os::raw::{c_char, c_int, c_uint};

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
    /// Inside a DTD declaration (bracket not yet written).
    DTD,
    /// Inside a DTD declaration after the internal subset bracket.
    DTDText,
    /// Inside a DTD element declaration.
    DTDElem,
    /// Inside a DTD element declaration after content.
    DTDElemText,
    /// Inside a DTD attribute declaration.
    DTDAttr,
    /// Inside a DTD attribute declaration after content.
    DTDAttrText,
    /// Inside a DTD entity declaration (no content yet).
    DTDEntity,
    /// Inside a DTD entity declaration after content.
    DTDEntityText,
    /// Inside a DTD notation declaration.
    #[allow(dead_code)]
    DTDNotation,
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
#[derive(Debug)]
pub struct XmlTextWriter {
    /// The output buffer where serialized XML is written.
    output: *mut _xmlOutputBuffer,
    /// Whether indentation is enabled (non-zero = enabled).
    indent: c_int,
    /// The string used for one level of indentation.
    indent_string: Vec<u8>,
    /// Quote character for attribute/entity values (upstream `qchar`).
    qchar: u8,
    /// Indent the next closing tag (upstream `doindent`).
    doindent: bool,
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
    /// Whether the current DTD entity declaration is a parameter entity
    /// (upstream XML_TEXTWRITER_DTD_PENT).
    entity_pe: bool,
    /// Whether an output encoder has been installed (xmlTextWriterStartDocument
    /// with a non-NULL encoding). Once set it persists for the writer's life
    /// and makes byte-writes report 0 bytes (upstream encoder path).
    encoder_active: bool,
    /// Pending namespace declarations for the current start tag (upstream
    /// xmlTextWriterOutputNSDecl defers them until the tag closes).
    pending_ns: Vec<(Vec<u8>, Vec<u8>)>,
    /// Open DTD child declarations (upstream stack entries) contributing to
    /// the indentation depth.
    dtd_depth: c_int,
    /// True while a BARE DTD child declaration is open — i.e. a
    /// `<!ENTITY`/`<!ELEMENT`/`<!ATTLIST` written WITHOUT a `<!DOCTYPE`
    /// wrapper (upstream: empty node list, no indentation; returns to None).
    dtd_bare: bool,
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
            (*writer).indent_string = b" \0".to_vec();
            // UPSTREAM-PARITY (R-000154): the default indent string is a
            // single space, not two; xmlTextWriterSetIndentString overrides it.
            (*writer).qchar = b'"';
            (*writer).doindent = true;
            (*writer).depth = 0;
            (*writer).stack = Vec::new();
            (*writer).encoding = b"UTF-8\0".to_vec();
            (*writer).errors = Vec::new();
            (*writer).state = WriterState::None;
            (*writer).doc = ptr::null_mut();
            (*writer).in_start_tag = false;
            (*writer).elem_stack = Vec::new();
            (*writer).entity_pe = false;
            (*writer).encoder_active = false;
            (*writer).pending_ns = Vec::new();
            (*writer).dtd_depth = 0;
            (*writer).dtd_bare = false;
        }
        writer
    }

    /// Write raw bytes to the output buffer.
    ///
    /// # SAFETY
    ///
    /// - `data` must point to `len` valid bytes.
    unsafe fn write_raw(&mut self, data: *const u8, len: c_int) -> c_int {
        if self.output.is_null() || data.is_null() || len <= 0 {
            return -1;
        }
        let rc = io::output_buffer_write(self.output, len, data as *const c_char);
        // UPSTREAM-PARITY: with an output encoder installed, xmlOutputBufferWrite
        // reports 0 bytes for writes below the 256-byte conversion threshold.
        if self.encoder_active {
            0
        } else {
            rc
        }
    }

    /// Write a null-terminated string to the output buffer.
    unsafe fn write_str(&mut self, s: *const u8) -> c_int {
        if self.output.is_null() || s.is_null() {
            return -1;
        }
        let rc = io::output_buffer_write_string(self.output, s as *const c_char);
        if self.encoder_active {
            0
        } else {
            rc
        }
    }

    /// Write a byte slice to the output buffer.
    ///
    /// NOTE: The slice must NOT borrow from `self` to avoid borrow checker conflicts.
    unsafe fn write_slice(&mut self, slice: &[u8]) -> c_int {
        if self.output.is_null() || slice.is_empty() {
            return -1;
        }
        let rc = io::output_buffer_write(
            self.output,
            slice.len() as c_int,
            slice.as_ptr() as *const c_char,
        );
        if self.encoder_active {
            0
        } else {
            rc
        }
    }

    /// Write a single byte to the output buffer.
    unsafe fn write_byte(&mut self, b: u8) -> c_int {
        if self.output.is_null() {
            return -1;
        }
        let rc = io::output_buffer_write_char(self.output, b as c_char);
        if self.encoder_active {
            0
        } else {
            rc
        }
    }

    /// Write indentation (if enabled): `levels` repetitions of the indent
    /// string. Returns the number of indent strings written.
    ///
    /// # SAFETY
    ///
    /// Uses a clone of the indent string to avoid borrow checker conflicts.
    unsafe fn write_indent_levels(&mut self, levels: c_int) -> c_int {
        if self.indent == 0 || levels <= 0 {
            return 0;
        }
        // UPSTREAM-PARITY (xmlTextWriterWriteIndent, R-000151/R-000154):
        // returns the number of indent strings written, not the byte count.
        // The stored indent string is NUL-terminated; the NUL must not reach
        // the output.
        let indent_str = self.indent_string.clone();
        let body = if indent_str.last() == Some(&0) {
            &indent_str[..indent_str.len() - 1]
        } else {
            &indent_str[..]
        };
        for _ in 0..levels {
            self.write_slice(body);
        }
        levels
    }

    /// Write indentation (if enabled): `depth + dtd_depth` levels — used by
    /// element/leaf START paths, where the candidate's element-only `depth`
    /// equals upstream's `nodes-1` after pushing the new node.
    unsafe fn write_indent(&mut self) -> c_int {
        let count = self.depth + self.dtd_depth;
        unsafe { self.write_indent_levels(count) }
    }

    /// Close any open start tag (writing `>` to transition from attribute-writing
    /// mode to content-writing mode). Returns `(closed, bytes)` — whether a tag
    /// was actually closed, and the byte count contributed (encoder-muted).
    /// No newline: the NAME->TEXT transition only emits `>` (the newline after
    /// the first child comes from the child-start paths, matching
    /// xmlTextWriterHandleStateDependencies). Pending namespace declarations
    /// are flushed first (upstream xmlTextWriterOutputNSDecl).
    unsafe fn close_start_tag(&mut self) -> (bool, c_int) {
        if self.in_start_tag {
            self.in_start_tag = false;
            let mut sum: c_int = self.flush_pending_ns();
            sum += self.write_byte(b'>');
            (true, sum)
        } else {
            (false, 0)
        }
    }

    /// Write the pending namespace declarations (upstream
    /// xmlTextWriterOutputNSDecl, R-000153): ` xmlns:prefix="uri"` /
    /// ` xmlns="uri"` — deferred to tag close, after the attributes.
    unsafe fn flush_pending_ns(&mut self) -> c_int {
        let mut sum: c_int = 0;
        let pending = core::mem::take(&mut self.pending_ns);
        for (prefix, uri) in pending {
            sum += self.write_byte(b' ');
            if prefix.is_empty() {
                sum += self.write_slice(b"xmlns=\"");
            } else {
                sum += self.write_slice(b"xmlns:");
                sum += self.write_slice(&prefix);
                sum += self.write_slice(b"=\"");
            }
            sum += self.write_slice(&uri);
            sum += self.write_byte(b'"');
        }
        sum
    }

    /// Check if the writer is in a state where element/attribute content can be written.
    #[allow(dead_code)]
    const fn can_write_content(&self) -> bool {
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
    unsafe { allocator::xmlFreeImpl(writer as *mut c_void) };
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
    // UPSTREAM-PARITY (xmlwriter.c xmlNewTextWriterFilename): the filename
    // open funnels through xmlOutputBufferCreateFilename, which honors a
    // registered default create-filename callback (PHP installs
    // php_libxml_output_buffer_create_filename at request init, so openUri
    // opens a php:// stream or a NO_FCLOSE php file stream — bug71536 /
    // bug79029); with none registered the builtin file open runs.
    // SAFETY: uri is a valid C string.
    let out = io::output_buffer_create_filename_routed(uri, ptr::null_mut(), compression);
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

    // UPSTREAM-PARITY (xmlTextWriterStartDocument): the declaration uses the
    // writer's quote char and always ends with a newline (indent-independent).
    let mut sum: c_int = 0;
    sum += w.write_raw(b"<?xml version=" as *const u8, 14);
    sum += w.write_byte(w.qchar);

    let ver = if version.is_null() {
        b"1.0\0" as *const u8
    } else {
        version as *const u8
    };
    sum += w.write_str(ver);
    sum += w.write_byte(w.qchar);

    if !encoding.is_null() {
        sum += w.write_raw(b" encoding=" as *const u8, 10);
        sum += w.write_byte(w.qchar);
        sum += w.write_str(encoding as *const u8);
        sum += w.write_byte(w.qchar);
        // UPSTREAM-PARITY: the output encoder, once installed, persists for
        // the writer's lifetime (a later StartDocument with encoding=NULL does
        // NOT clear it — xmlTextWriterStartDocument only resets conv).
        w.encoder_active = true;
    }

    if !standalone.is_null() {
        sum += w.write_raw(b" standalone=" as *const u8, 12);
        sum += w.write_byte(w.qchar);
        sum += w.write_str(standalone as *const u8);
        sum += w.write_byte(w.qchar);
    }

    sum += w.write_raw(b"?>\n" as *const u8, 3);

    // UPSTREAM-PARITY: the XML declaration is not a stack state — after it
    // the writer is idle (empty node list), so prolog DTD children
    // (`<!ENTITY`, `<!ELEMENT`, `<!ATTLIST` with or without a DOCTYPE
    // wrapper) and the root element can follow (SP-14.3.6, 008/OO_008).
    w.state = WriterState::None;
    sum
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
#[allow(clippy::while_immutable_condition)]
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterEndDocument(writer: *mut XmlTextWriter) -> c_int {
    if writer.is_null() {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };

    // Close any open elements
    let mut sum: c_int = 0;
    while w.depth > 0 {
        sum += xmlTextWriterEndElement(writer);
    }

    // UPSTREAM-PARITY: the final newline is written when indentation is OFF
    // (each indented EndElement already wrote its own newline).
    if w.indent == 0 {
        sum += w.write_byte(b'\n');
    }

    // Flush output
    if !w.output.is_null() {
        sum += io::output_buffer_flush(w.output);
    }

    w.state = WriterState::None;
    sum
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

    // Close any open start tag from a previous element.
    // UPSTREAM-PARITY: closing the parent's start tag emits `>` and, when
    // indented, a newline (xmlTextWriterStartElement NAME case).
    let (closed, cnt) = w.close_start_tag();
    let mut sum: c_int = cnt;
    if closed && w.indent != 0 {
        sum += w.write_byte(b'\n');
    }

    // Write indentation
    sum += w.write_indent();

    // Write `<name`
    sum += w.write_byte(b'<');
    sum += w.write_str(name);

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

    sum
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

    // UPSTREAM-PARITY (xmlTextWriterEndElement):
    //   NAME state (start tag still open) -> "/>", doindent=1
    //   otherwise (content written)       -> indent if doindent, "</name>"
    //   then, when indented, a trailing newline.
    let mut sum: c_int = 0;
    if w.in_start_tag {
        sum += w.flush_pending_ns();
        sum += w.write_raw(b"/>" as *const u8, 2);
        w.in_start_tag = false;
        w.doindent = true;
        w.stack.pop();
    } else {
        // UPSTREAM-PARITY (xmlTextWriterEndElement TEXT case): the indent is
        // `nodes - 1` levels (the candidate's element-only depth is one more
        // than the node count minus one — a closing tag one level deep sits
        // at column 0, not at one indent).
        if w.indent != 0 && w.doindent {
            sum += w.write_indent_levels(w.depth - 1);
            w.doindent = true;
        } else {
            w.doindent = true;
        }
        let name = w.stack.pop().unwrap_or_default();
        sum += w.write_raw(b"</" as *const u8, 2);
        sum += w.write_slice(&name);
        sum += w.write_byte(b'>');
    }

    if w.indent != 0 {
        sum += w.write_byte(b'\n');
    }

    w.depth -= 1;
    w.elem_stack.pop();
    w.state = WriterState::None;

    sum
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

    // UPSTREAM-PARITY: closing the parent's start tag emits `>` and, when
    // indented, a newline.
    let (closed, cnt) = w.close_start_tag();
    let mut sum: c_int = cnt;
    if closed && w.indent != 0 {
        sum += w.write_byte(b'\n');
    }
    sum += w.write_indent();

    sum += w.write_byte(b'<');

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
        sum += w.write_slice(p);
        sum += w.write_byte(b':');
    }
    // Strip trailing null before writing
    let n = if name_bytes.last() == Some(&0) {
        &name_bytes[..name_bytes.len() - 1]
    } else {
        &name_bytes
    };
    sum += w.write_slice(n);

    // Defer the namespace declaration until the tag closes (upstream
    // xmlTextWriterOutputNSDecl writes it after the attributes).
    if !namespaceURI.is_null() {
        let ns_uri_bytes = unsafe { c_str_to_vec(namespaceURI) };
        let uri_body = if ns_uri_bytes.last() == Some(&0) {
            ns_uri_bytes[..ns_uri_bytes.len() - 1].to_vec()
        } else {
            ns_uri_bytes
        };
        let prefix_body = if prefix_bytes.last() == Some(&0) {
            prefix_bytes[..prefix_bytes.len() - 1].to_vec()
        } else {
            prefix_bytes.clone()
        };
        w.pending_ns.push((prefix_body, uri_body));
    }

    // Build the stack QName BEFORE elem_stack takes ownership of
    // prefix_bytes (UPSTREAM-PARITY: xmlwriter.c StartElementNS pushes
    // `prefix:name`, so End* emit `</prefix:name>` — the old bare-local-name
    // stack dropped the prefix on namespaced end tags; SP-14.3.6 W1).
    let qname = {
        let mut q = Vec::new();
        if !prefix_bytes.is_empty() {
            let p = if prefix_bytes.last() == Some(&0) {
                &prefix_bytes[..prefix_bytes.len() - 1]
            } else {
                &prefix_bytes[..]
            };
            q.extend_from_slice(p);
            q.push(b':');
        }
        let n = if name_bytes.last() == Some(&0) {
            &name_bytes[..name_bytes.len() - 1]
        } else {
            &name_bytes[..]
        };
        q.extend_from_slice(n);
        q
    };
    w.elem_stack.push((prefix_bytes, name_bytes));
    w.stack.push(qname);
    w.depth += 1;
    w.in_start_tag = true;
    w.state = WriterState::Element;

    sum
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
    if ret == -1 {
        return ret;
    }
    if !content.is_null() {
        let ret2 = xmlTextWriterWriteString(writer, content);
        if ret2 == -1 {
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
    if ret == -1 {
        return ret;
    }
    if !content.is_null() {
        let ret2 = xmlTextWriterWriteString(writer, content);
        if ret2 == -1 {
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

    // UPSTREAM-PARITY (xmlTextWriterFullEndElement): always writes `</name>`,
    // closing the start tag with `>` first if needed. When closing straight
    // from the NAME state (empty element), doindent is cleared so the `</name>`
    // follows `>` on the SAME line — no interior indentation (SP-14.3.6 W2,
    // 012/OO_011 `<empty></empty>`).
    let mut sum: c_int = 0;
    if w.in_start_tag {
        sum += w.write_byte(b'>');
        w.in_start_tag = false;
        if w.indent != 0 {
            w.doindent = false;
        }
    }

    if w.indent != 0 && w.doindent {
        sum += w.write_indent_levels(w.depth - 1);
        w.doindent = true;
    } else {
        w.doindent = true;
    }

    // Write `</name>`
    let name = w.stack.pop().unwrap_or_default();
    sum += w.write_raw(b"</" as *const u8, 2);
    sum += w.write_slice(&name);
    sum += w.write_byte(b'>');

    if w.indent != 0 {
        sum += w.write_byte(b'\n');
    }

    w.depth -= 1;
    w.elem_stack.pop();
    w.state = WriterState::None;

    sum
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

    // Write ` name=` and the quote char.
    let mut sum: c_int = 0;
    sum += w.write_byte(b' ');
    sum += w.write_str(name);
    sum += w.write_raw(b"=" as *const u8, 1);
    sum += w.write_byte(w.qchar);

    // Write escaped content (qchar-aware).
    sum += unsafe { write_attr_escaped(w, content) };

    sum += w.write_byte(w.qchar);
    // UPSTREAM-PARITY: a completed attribute returns the writer to the
    // element start-tag state (xmlTextWriterEndAttribute -> NAME).
    w.state = WriterState::Element;

    sum
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
    if writer.is_null() || name.is_null() || content.is_null() {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };

    if !w.in_start_tag {
        return -1;
    }

    // Queue the attribute's `xmlns[:prefix]` declaration (same rule as
    // xmlTextWriterStartAttributeNS).
    if unsafe { queue_attr_ns_decl(w, prefix, nsURI) }.is_err() {
        return -1;
    }

    let mut sum: c_int = 0;
    sum += w.write_byte(b' ');

    if !prefix.is_null() {
        sum += w.write_str(prefix);
        sum += w.write_byte(b':');
    }
    sum += w.write_str(name);

    sum += w.write_raw(b"=" as *const u8, 1);
    sum += w.write_byte(w.qchar);

    // Write escaped content (qchar-aware).
    sum += unsafe { write_attr_escaped(w, content) };

    sum += w.write_byte(w.qchar);
    // UPSTREAM-PARITY: a completed attribute returns the writer to the
    // element start-tag state.
    w.state = WriterState::Element;

    sum
}

/// Queue an `xmlns[:prefix]="uri"` declaration for the current element's
/// start tag (upstream nsstack entry anchored at the element). Returns Err
/// when the same prefix is already bound to a DIFFERENT URI on this element
/// (upstream xmlTextWriterStartAttributeNS/StartElementNS list-search).
///
/// # SAFETY
///
/// - `w` is a valid writer; `prefix`/`nsURI` are valid NUL-terminated
///   strings or NULL.
unsafe fn queue_attr_ns_decl(
    w: &mut XmlTextWriter,
    prefix: *const xmlChar,
    nsURI: *const xmlChar,
) -> Result<(), ()> {
    if nsURI.is_null() {
        return Ok(());
    }
    let prefix_bytes = if prefix.is_null() {
        Vec::new()
    } else {
        unsafe { c_str_to_vec(prefix) }
    };
    let p = if prefix_bytes.last() == Some(&0) {
        prefix_bytes[..prefix_bytes.len() - 1].to_vec()
    } else {
        prefix_bytes
    };
    let uri_bytes = unsafe { c_str_to_vec(nsURI) };
    let u = if uri_bytes.last() == Some(&0) {
        uri_bytes[..uri_bytes.len() - 1].to_vec()
    } else {
        uri_bytes
    };
    for (ep, eu) in w.pending_ns.iter() {
        if ep == &p {
            if eu == &u {
                // Already declared with the same URI on this element: no-op.
                return Ok(());
            }
            // Same prefix bound to a different URI here: error.
            return Err(());
        }
    }
    w.pending_ns.push((p, u));
    Ok(())
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
///
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

    let mut sum: c_int = 0;
    sum += w.write_byte(b' ');
    sum += w.write_str(name);
    sum += w.write_raw(b"=" as *const u8, 1);
    sum += w.write_byte(w.qchar);
    w.state = WriterState::Attribute;

    sum
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
    if writer.is_null() || name.is_null() {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };

    if !w.in_start_tag {
        return -1;
    }

    // UPSTREAM-PARITY (xmlwriter.c xmlTextWriterStartAttributeNS): a
    // namespaced attribute queues an `xmlns[:prefix]` declaration on the ns
    // stack (deduped per element: same prefix + same URI is a no-op, same
    // prefix + different URI is an error). It is flushed when the element's
    // start tag closes — xmlTextWriterOutputNSDecl — so `prefix:id=...`
    // appears with its `xmlns:prefix="uri"` after the attributes
    // (SP-14.3.6, xmlwriter_write_attribute_ns_basic_001).
    if unsafe { queue_attr_ns_decl(w, prefix, nsURI) }.is_err() {
        return -1;
    }

    let mut sum: c_int = 0;
    sum += w.write_byte(b' ');
    if !prefix.is_null() {
        sum += w.write_str(prefix);
        sum += w.write_byte(b':');
    }
    sum += w.write_str(name);
    sum += w.write_raw(b"=" as *const u8, 1);
    sum += w.write_byte(w.qchar);
    w.state = WriterState::Attribute;

    sum
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

    w.write_byte(w.qchar);
    w.state = WriterState::Element;

    1
}

// ═══════════════════════════════════════════════════════════════════════════════
// Content writing
// ═══════════════════════════════════════════════════════════════════════════════

/// Escape text like upstream `xmlEncodeSpecialChars(NULL, content)`:
/// `&` `<` `>` `"` `'` are all escaped. Returns a NUL-terminated vector.
///
/// # SAFETY
///
/// - `content` must be a valid NUL-terminated string.
unsafe fn encode_special_chars(content: *const xmlChar) -> Vec<u8> {
    let mut out = Vec::new();
    let mut p = content;
    unsafe {
        while !p.is_null() && *p != 0 {
            // UPSTREAM-PARITY (xmlEncodeSpecialChars / xmlEscapeText with
            // XML_ESCAPE_QUOT): `&<>"` are escaped; the apostrophe is NOT.
            match *p {
                b'&' => out.extend_from_slice(b"&amp;"),
                b'<' => out.extend_from_slice(b"&lt;"),
                b'>' => out.extend_from_slice(b"&gt;"),
                b'"' => out.extend_from_slice(b"&quot;"),
                c => out.push(c),
            }
            p = p.add(1);
        }
    }
    out.push(0);
    out
}

/// Serialize attribute content with the writer's quote char, mirroring
/// `xmlBufAttrSerializeTxtContent` (xmlsave.c): `\n`/`\r`/`\t` become
/// character references, `&<>` always escape, and the quote char is escaped.
/// Returns the bytes written.
///
/// # SAFETY
///
/// - `content` must be a valid NUL-terminated string.
unsafe fn write_attr_escaped(w: &mut XmlTextWriter, content: *const xmlChar) -> c_int {
    let mut sum: c_int = 0;
    let mut p = content;
    unsafe {
        while !p.is_null() && *p != 0 {
            let c = *p;
            // UPSTREAM-PARITY (xmlBufAttrSerializeTxtContent -> xmlSerializeText
            // with XML_ESCAPE_ATTR): `\n`/`\r`/`\t` become character
            // references, `&<>"` escape; the apostrophe is NEVER escaped
            // (the qchar only selects the outer quotes).
            sum += match c {
                b'\n' => w.write_slice(b"&#10;"),
                b'\r' => w.write_slice(b"&#13;"),
                b'\t' => w.write_slice(b"&#9;"),
                b'&' => w.write_slice(b"&amp;"),
                b'<' => w.write_slice(b"&lt;"),
                b'>' => w.write_slice(b"&gt;"),
                b'"' => w.write_slice(b"&quot;"),
                c => w.write_byte(c),
            };
            p = p.add(1);
        }
    }
    sum
}

/// Write text content.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextWriterWriteString(xmlTextWriterPtr writer, const xmlChar *content);
/// ```
///
/// NAME/TEXT states escape via xmlEncodeSpecialChars (quotes included);
/// ATTRIBUTE escapes via xmlBufAttrSerializeTxtContent (qchar-aware); all
/// other states (CDATA/comment/PI/DTD*) write raw through WriteRaw, which
/// performs the DTD state transitions.
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

    match w.state {
        WriterState::Attribute => unsafe { write_attr_escaped(w, content) },
        WriterState::Element => {
            // UPSTREAM-PARITY (xmlTextWriterWriteString NAME case): the start
            // tag closes (HandleStateDependencies `>`) even for EMPTY content
            // (php writeElement*('', ...) then takes the full-close EndElement
            // path — bug41287 `<test:foo></test:foo>`); an empty payload is
            // not an error. Escaping is xmlEncodeSpecialChars (`&<>"`).
            let esc = unsafe { encode_special_chars(content) };
            let (_, cnt) = w.close_start_tag();
            let mut sum: c_int = cnt;
            if esc.len() > 1 {
                sum += w.write_slice(&esc[..esc.len() - 1]);
            }
            w.doindent = false;
            sum
        }
        WriterState::None if w.depth > 0 => {
            // Inside an element after content: upstream TEXT state escapes.
            let esc = unsafe { encode_special_chars(content) };
            let mut sum: c_int = 0;
            if esc.len() > 1 {
                sum += w.write_slice(&esc[..esc.len() - 1]);
            }
            w.doindent = false;
            sum
        }
        _ => {
            // Raw path (CDATA/comment/PI/DTD*, and top-level with no stack
            // entry — upstream writes raw when no element is open): WriteRaw
            // performs the state transitions (DTD bracket, entity quote,
            // element/attr separators).
            let rc = unsafe { xmlTextWriterWriteRaw(writer, content) };
            w.doindent = false;
            rc
        }
    }
}

/// Write raw content (no XML escaping).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextWriterWriteRaw(xmlTextWriterPtr writer, const xmlChar *content);
/// ```
///
/// Performs the upstream state-dependent transitions before the content:
/// DTD -> " [" (+newline when indented), DTD_ELEM/DTD_ATTL -> " ",
/// DTD_ENTY/PENT -> " " + quote char, PI -> " ".
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

    // UPSTREAM-PARITY (xmlTextWriterHandleStateDependencies).
    let mut sum: c_int = 0;
    match w.state {
        WriterState::Element => {
            let (_, cnt) = w.close_start_tag();
            sum += cnt;
        }
        WriterState::PI => {
            sum += w.write_byte(b' ');
        }
        WriterState::DTD => {
            w.state = WriterState::DTDText;
            if w.indent != 0 {
                sum += w.write_slice(b" [\n");
            } else {
                sum += w.write_slice(b" [");
            }
        }
        WriterState::DTDElem => {
            sum += w.write_byte(b' ');
            w.state = WriterState::DTDElemText;
        }
        WriterState::DTDAttr => {
            sum += w.write_byte(b' ');
            w.state = WriterState::DTDAttrText;
        }
        WriterState::DTDEntity => {
            sum += w.write_byte(b' ');
            sum += w.write_byte(w.qchar);
            w.state = WriterState::DTDEntityText;
        }
        _ => {}
    }

    if w.indent != 0 {
        w.doindent = false;
    }

    sum += w.write_str(content);
    sum
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
    if writer.is_null() || content.is_null() || len < 0 {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };

    // Same state transitions as WriteRaw, then the len-bounded write.
    let mut sum: c_int = 0;
    match w.state {
        WriterState::Element => {
            let (_, cnt) = w.close_start_tag();
            sum += cnt;
        }
        WriterState::PI => {
            sum += w.write_byte(b' ');
        }
        WriterState::DTD => {
            w.state = WriterState::DTDText;
            if w.indent != 0 {
                sum += w.write_slice(b" [\n");
            } else {
                sum += w.write_slice(b" [");
            }
        }
        WriterState::DTDElem => {
            sum += w.write_byte(b' ');
            w.state = WriterState::DTDElemText;
        }
        WriterState::DTDAttr => {
            sum += w.write_byte(b' ');
            w.state = WriterState::DTDAttrText;
        }
        WriterState::DTDEntity => {
            sum += w.write_byte(b' ');
            sum += w.write_byte(w.qchar);
            w.state = WriterState::DTDEntityText;
        }
        _ => {}
    }

    if w.indent != 0 {
        w.doindent = false;
    }

    if len > 0 {
        sum += w.write_raw(content, len);
    }
    sum
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
///
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
    let mut sum: c_int = 0;
    let ret = unsafe { xmlTextWriterStartCDATA(writer) };
    if ret == -1 {
        return -1;
    }
    sum += ret;
    if !content.is_null() {
        let ret2 = unsafe { xmlTextWriterWriteString(writer, content) };
        if ret2 == -1 {
            return -1;
        }
        sum += ret2;
    }
    let ret3 = unsafe { xmlTextWriterEndCDATA(writer) };
    if ret3 == -1 {
        return -1;
    }
    sum + ret3
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

    // UPSTREAM-PARITY (xmlwriter.c xmlTextWriterStartCDATA): closing an open
    // NAME start tag emits `>` and NO newline — the CDATA section continues
    // on the same line (`<elem><![CDATA[...`); upstream only StartComment and
    // StartElement add a newline after the closing `>` (SP-14.3.6 W4, 009).
    let (closed, cnt) = w.close_start_tag();
    let mut sum: c_int = cnt;
    let _ = closed;
    sum += w.write_slice(b"<![CDATA[");
    w.state = WriterState::CData;
    sum
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
    let sum: c_int = w.write_slice(b"]]>");
    w.state = WriterState::None;
    sum
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
    let mut sum: c_int = 0;
    let ret = unsafe { xmlTextWriterStartComment(writer) };
    if ret < 0 {
        return -1;
    }
    sum += ret;
    let ret2 = unsafe { xmlTextWriterWriteString(writer, content) };
    if ret2 < 0 {
        return -1;
    }
    sum += ret2;
    let ret3 = unsafe { xmlTextWriterEndComment(writer) };
    if ret3 < 0 {
        return -1;
    }
    sum + ret3
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
    let (closed, cnt) = w.close_start_tag();
    let mut sum: c_int = cnt;
    if closed && w.indent != 0 {
        sum += w.write_byte(b'\n');
    }
    sum += w.write_indent();
    sum += w.write_slice(b"<!--");
    w.state = WriterState::Comment;
    sum
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
    let mut sum: c_int = w.write_slice(b"-->");
    if w.indent != 0 {
        sum += w.write_byte(b'\n');
    }
    w.state = WriterState::None;
    sum
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
    let mut sum: c_int = 0;
    let ret = unsafe { xmlTextWriterStartPI(writer, target) };
    if ret == -1 {
        return -1;
    }
    sum += ret;
    if !content.is_null() {
        let ret2 = unsafe { xmlTextWriterWriteString(writer, content) };
        if ret2 == -1 {
            return -1;
        }
        sum += ret2;
    }
    let ret3 = unsafe { xmlTextWriterEndPI(writer) };
    if ret3 == -1 {
        return -1;
    }
    sum + ret3
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
    if writer.is_null() || target.is_null() || unsafe { *target } == 0 {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };
    // UPSTREAM-PARITY (xmlwriter.c xmlTextWriterStartPI): closing an open
    // NAME start tag emits `>` and NO newline — the PI content follows inline
    // (`<pi><?php ...`); only EndPI writes the trailing newline (SP-14.3.6 W4,
    // 009).
    let (closed, cnt) = w.close_start_tag();
    let mut sum: c_int = cnt;
    let _ = closed;
    sum += w.write_slice(b"<?");
    sum += w.write_str(target);
    // UPSTREAM-PARITY: no trailing space here — the first content write
    // emits the separator (xmlTextWriterHandleStateDependencies PI case).
    w.state = WriterState::PI;
    sum
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
    let mut sum: c_int = w.write_slice(b"?>");
    if w.indent != 0 {
        sum += w.write_byte(b'\n');
    }
    w.state = WriterState::None;
    sum
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
    let mut sum: c_int = 0;
    let ret = unsafe { xmlTextWriterStartDTD(writer, name, pubid, sysid) };
    if ret == -1 {
        return ret;
    }
    sum += ret;
    if !subset.is_null() {
        let ret2 = unsafe { xmlTextWriterWriteString(writer, subset) };
        if ret2 == -1 {
            return ret2;
        }
        sum += ret2;
    }
    let ret3 = unsafe { xmlTextWriterEndDTD(writer) };
    if ret3 == -1 {
        return ret3;
    }
    sum + ret3
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
    if content.is_null() {
        return -1;
    }
    let mut sum: c_int = 0;
    let ret = unsafe { xmlTextWriterStartDTDElement(writer, name) };
    if ret == -1 {
        return ret;
    }
    sum += ret;
    let ret2 = unsafe { xmlTextWriterWriteString(writer, content) };
    if ret2 == -1 {
        return ret2;
    }
    sum += ret2;
    let ret3 = unsafe { xmlTextWriterEndDTDElement(writer) };
    if ret3 == -1 {
        return ret3;
    }
    sum + ret3
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
    if content.is_null() {
        return -1;
    }
    // UPSTREAM-PARITY: upstream xmlTextWriterWriteDTDAttribute composes
    // StartDTDAttlist + WriteString + EndDTDAttlist (there is no separate
    // StartDTDAttribute API).
    let mut sum: c_int = 0;
    let ret = unsafe { xmlTextWriterStartDTDAttlist(writer, name) };
    if ret == -1 {
        return ret;
    }
    sum += ret;
    let ret2 = unsafe { xmlTextWriterWriteString(writer, content) };
    if ret2 == -1 {
        return ret2;
    }
    sum += ret2;
    let ret3 = unsafe { xmlTextWriterEndDTDAttlist(writer) };
    if ret3 == -1 {
        return ret3;
    }
    sum + ret3
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
    pe: c_int,
    name: *const xmlChar,
    pubid: *const xmlChar,
    sysid: *const xmlChar,
    ndataid: *const xmlChar,
    content: *const xmlChar,
) -> c_int {
    if content.is_null() && pubid.is_null() && sysid.is_null() {
        return -1;
    }
    if pe != 0 && !ndataid.is_null() {
        return -1;
    }
    if pubid.is_null() && sysid.is_null() {
        return unsafe { xmlTextWriterWriteDTDInternalEntity(writer, pe, name, content) };
    }
    unsafe { xmlTextWriterWriteDTDExternalEntity(writer, pe, name, pubid, sysid, ndataid) }
}

/// Write a DTD internal entity (StartDTDEntity + WriteString + EndDTDEntity).
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
/// - `name`, `content` must be valid null-terminated strings or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterWriteDTDInternalEntity(
    writer: *mut XmlTextWriter,
    pe: c_int,
    name: *const xmlChar,
    content: *const xmlChar,
) -> c_int {
    if name.is_null() || unsafe { *name } == 0 || content.is_null() {
        return -1;
    }
    let mut sum: c_int = 0;
    let ret = unsafe { xmlTextWriterStartDTDEntity(writer, pe, name) };
    if ret == -1 {
        return -1;
    }
    sum += ret;
    let ret2 = unsafe { xmlTextWriterWriteString(writer, content) };
    if ret2 == -1 {
        return -1;
    }
    sum += ret2;
    let ret3 = unsafe { xmlTextWriterEndDTDEntity(writer) };
    if ret3 == -1 {
        return -1;
    }
    sum + ret3
}

/// Write a DTD external entity (StartDTDEntity + ExternalEntityContents + EndDTDEntity).
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
/// - `name`, `pubid`, `sysid`, `ndataid` must be valid null-terminated
///   strings or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterWriteDTDExternalEntity(
    writer: *mut XmlTextWriter,
    pe: c_int,
    name: *const xmlChar,
    pubid: *const xmlChar,
    sysid: *const xmlChar,
    ndataid: *const xmlChar,
) -> c_int {
    if pubid.is_null() && sysid.is_null() {
        return -1;
    }
    if pe != 0 && !ndataid.is_null() {
        return -1;
    }
    let mut sum: c_int = 0;
    let ret = unsafe { xmlTextWriterStartDTDEntity(writer, pe, name) };
    if ret == -1 {
        return -1;
    }
    sum += ret;
    let ret2 =
        unsafe { xmlTextWriterWriteDTDExternalEntityContents(writer, pubid, sysid, ndataid) };
    if ret2 < 0 {
        return -1;
    }
    sum += ret2;
    let ret3 = unsafe { xmlTextWriterEndDTDEntity(writer) };
    if ret3 == -1 {
        return -1;
    }
    sum + ret3
}

/// Write the external-entity contents after `StartDTDEntity` (PUBLIC/SYSTEM
/// identifiers and NDATA).
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
/// - `pubid`, `sysid`, `ndataid` must be valid null-terminated strings or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterWriteDTDExternalEntityContents(
    writer: *mut XmlTextWriter,
    pubid: *const xmlChar,
    sysid: *const xmlChar,
    ndataid: *const xmlChar,
) -> c_int {
    if writer.is_null() {
        return -1;
    }
    let w = unsafe { &mut *writer };
    // UPSTREAM-PARITY: must be directly inside a StartDTDEntity declaration
    // (DTD_ENTY / DTD_PENT; content already written is rejected).
    if w.state != WriterState::DTDEntity {
        return -1;
    }
    if w.entity_pe && !ndataid.is_null() {
        // UPSTREAM-PARITY: notation not allowed with parameter entities.
        return -1;
    }
    let mut sum: c_int = 0;
    if !pubid.is_null() {
        if sysid.is_null() {
            return -1;
        }
        sum += w.write_slice(b" PUBLIC ");
        sum += w.write_byte(w.qchar);
        sum += w.write_str(pubid);
        sum += w.write_byte(w.qchar);
    }
    if !sysid.is_null() {
        if pubid.is_null() {
            sum += w.write_slice(b" SYSTEM");
        }
        sum += w.write_byte(b' ');
        sum += w.write_byte(w.qchar);
        sum += w.write_str(sysid);
        sum += w.write_byte(w.qchar);
    }
    if !ndataid.is_null() {
        sum += w.write_slice(b" NDATA ");
        sum += w.write_str(ndataid);
    }
    sum
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
    if writer.is_null() || name.is_null() || unsafe { *name } == 0 {
        return -1;
    }
    let w = unsafe { &mut *writer };
    let mut sum: c_int = 0;
    if w.state == WriterState::DTD {
        // UPSTREAM-PARITY: first DTD child writes the internal-subset bracket.
        sum += w.write_slice(b" [");
        if w.indent != 0 {
            sum += w.write_byte(b'\n');
        }
        w.state = WriterState::DTDText;
    } else if w.state != WriterState::DTDText {
        return -1;
    }
    sum += w.write_indent();
    sum += w.write_slice(b"<!NOTATION ");
    sum += w.write_str(name);
    if !pubid.is_null() {
        sum += w.write_slice(b" PUBLIC ");
        sum += w.write_byte(w.qchar);
        sum += w.write_str(pubid);
        sum += w.write_byte(w.qchar);
    }
    if !sysid.is_null() {
        if pubid.is_null() {
            sum += w.write_slice(b" SYSTEM");
        }
        sum += w.write_byte(b' ');
        sum += w.write_byte(w.qchar);
        sum += w.write_str(sysid);
        sum += w.write_byte(w.qchar);
    }
    sum += w.write_byte(b'>');
    sum
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
    if writer.is_null() || name.is_null() || unsafe { *name } == 0 {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };
    if w.depth > 0 {
        // UPSTREAM-PARITY: DTD allowed only in the prolog (no open elements).
        return -1;
    }

    let mut sum: c_int = 0;
    sum += w.write_slice(b"<!DOCTYPE ");
    sum += w.write_str(name);

    if !pubid.is_null() {
        if sysid.is_null() {
            // UPSTREAM-PARITY: PUBLIC requires a system identifier.
            return -1;
        }
        if w.indent != 0 {
            sum += w.write_byte(b'\n');
        } else {
            sum += w.write_byte(b' ');
        }
        sum += w.write_slice(b"PUBLIC ");
        sum += w.write_byte(w.qchar);
        sum += w.write_str(pubid);
        sum += w.write_byte(w.qchar);
    }
    if !sysid.is_null() {
        if pubid.is_null() {
            if w.indent != 0 {
                sum += w.write_byte(b'\n');
            } else {
                sum += w.write_byte(b' ');
            }
            sum += w.write_slice(b"SYSTEM ");
        } else if w.indent != 0 {
            // UPSTREAM-PARITY: continuation line is indented 7 spaces.
            sum += w.write_slice(b"\n       ");
        } else {
            sum += w.write_byte(b' ');
        }
        sum += w.write_byte(w.qchar);
        sum += w.write_str(sysid);
        sum += w.write_byte(w.qchar);
    }

    w.state = WriterState::DTD;
    sum
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

    if w.state != WriterState::DTD && w.state != WriterState::DTDText {
        return -1;
    }
    let mut sum: c_int = 0;
    if w.state == WriterState::DTDText {
        sum += w.write_byte(b']');
    }
    sum += w.write_byte(b'>');
    if w.indent != 0 {
        sum += w.write_byte(b'\n');
    }
    w.state = WriterState::None;
    sum
}

/// Internal: begin a DTD child declaration. Returns true when the current
/// context accepts one. Mirrors upstream: a child inside `<!DOCTYPE name [`
/// (states DTD/DTDText) emits the deferred ` [` on the first child and
/// contributes one indentation level; a BARE child at the prolog (state None,
/// no open elements — upstream's empty node list) is written with NO
/// indentation and returns to None when closed (SP-14.3.6, 008/OO_008:
/// `xmlwriter_start_dtd_entity` etc. without `start_dtd`).
unsafe fn dtd_child_begin(w: &mut XmlTextWriter) -> bool {
    match w.state {
        WriterState::DTD => {
            w.write_slice(b" [");
            if w.indent != 0 {
                w.write_byte(b'\n');
            }
            w.state = WriterState::DTDText;
            w.dtd_bare = false;
            true
        }
        WriterState::DTDText => {
            w.dtd_bare = false;
            true
        }
        WriterState::None if w.depth == 0 => {
            // No DOCTYPE wrapper: the declaration is written bare at the
            // prolog (upstream: the node list is empty, so no ` [` and no
            // indentation).
            w.dtd_bare = true;
            true
        }
        _ => false,
    }
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
    if writer.is_null() || name.is_null() || unsafe { *name } == 0 {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };
    if !unsafe { dtd_child_begin(w) } {
        return -1;
    }
    let mut sum: c_int = 0;
    if !w.dtd_bare {
        w.dtd_depth += 1;
        sum += w.write_indent();
    }
    sum += w.write_slice(b"<!ELEMENT ");
    sum += w.write_str(name);
    w.state = WriterState::DTDElem;
    sum
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
    if w.state != WriterState::DTDElem && w.state != WriterState::DTDElemText {
        return -1;
    }
    let mut sum: c_int = w.write_byte(b'>');
    if w.indent != 0 {
        sum += w.write_byte(b'\n');
    }
    if w.dtd_bare {
        w.state = WriterState::None;
        w.dtd_bare = false;
    } else {
        w.state = WriterState::DTDText;
        w.dtd_depth -= 1;
    }
    sum
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
    if writer.is_null() || name.is_null() || unsafe { *name } == 0 {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };
    if !unsafe { dtd_child_begin(w) } {
        return -1;
    }
    let mut sum: c_int = 0;
    if !w.dtd_bare {
        w.dtd_depth += 1;
        sum += w.write_indent();
    }
    sum += w.write_slice(b"<!ATTLIST ");
    sum += w.write_str(name);
    w.state = WriterState::DTDAttr;
    sum
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
    if w.state != WriterState::DTDAttr && w.state != WriterState::DTDAttrText {
        return -1;
    }
    let mut sum: c_int = w.write_byte(b'>');
    if w.indent != 0 {
        sum += w.write_byte(b'\n');
    }
    if w.dtd_bare {
        w.state = WriterState::None;
        w.dtd_bare = false;
    } else {
        w.state = WriterState::DTDText;
        w.dtd_depth -= 1;
    }
    sum
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
    pe: c_int,
    name: *const xmlChar,
) -> c_int {
    if writer.is_null() || name.is_null() || unsafe { *name } == 0 {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };
    if !unsafe { dtd_child_begin(w) } {
        return -1;
    }
    let mut sum: c_int = 0;
    if !w.dtd_bare {
        w.dtd_depth += 1;
        sum += w.write_indent();
    }
    sum += w.write_slice(b"<!ENTITY ");
    if pe != 0 {
        sum += w.write_slice(b"% ");
    }
    sum += w.write_str(name);
    w.state = WriterState::DTDEntity;
    w.entity_pe = pe != 0;
    sum
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
    let mut sum: c_int = 0;
    if w.state == WriterState::DTDEntityText {
        sum += w.write_byte(w.qchar);
    } else if w.state != WriterState::DTDEntity {
        return -1;
    }
    sum += w.write_byte(b'>');
    if w.indent != 0 {
        sum += w.write_byte(b'\n');
    }
    if w.dtd_bare {
        w.state = WriterState::None;
        w.dtd_bare = false;
    } else {
        w.state = WriterState::DTDText;
        w.dtd_depth -= 1;
    }
    w.entity_pe = false;
    sum
}

/// Start a DTD attribute-list declaration (`<!ATTLIST name`).
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
/// - `name` must be a valid null-terminated string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterStartDTDAttlist(
    writer: *mut XmlTextWriter,
    name: *const xmlChar,
) -> c_int {
    if writer.is_null() || name.is_null() || unsafe { *name } == 0 {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };
    if !unsafe { dtd_child_begin(w) } {
        return -1;
    }
    let mut sum: c_int = 0;
    if !w.dtd_bare {
        w.dtd_depth += 1;
        sum += w.write_indent();
    }
    sum += w.write_slice(b"<!ATTLIST ");
    sum += w.write_str(name);
    w.state = WriterState::DTDAttr;
    sum
}

/// End a DTD attribute-list declaration.
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterEndDTDAttlist(writer: *mut XmlTextWriter) -> c_int {
    if writer.is_null() {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    let w = unsafe { &mut *writer };
    if w.state != WriterState::DTDAttr && w.state != WriterState::DTDAttrText {
        return -1;
    }
    let mut sum: c_int = w.write_byte(b'>');
    if w.indent != 0 {
        sum += w.write_byte(b'\n');
    }
    if w.dtd_bare {
        w.state = WriterState::None;
        w.dtd_bare = false;
    } else {
        w.state = WriterState::DTDText;
        w.dtd_depth -= 1;
    }
    sum
}

/// Write a DTD attribute-list declaration
/// (StartDTDAttlist + WriteString + EndDTDAttlist).
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
/// - `name`, `content` must be valid null-terminated strings or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterWriteDTDAttlist(
    writer: *mut XmlTextWriter,
    name: *const xmlChar,
    content: *const xmlChar,
) -> c_int {
    if content.is_null() {
        return -1;
    }
    let mut sum: c_int = 0;
    let ret = unsafe { xmlTextWriterStartDTDAttlist(writer, name) };
    if ret == -1 {
        return -1;
    }
    sum += ret;
    let ret2 = unsafe { xmlTextWriterWriteString(writer, content) };
    if ret2 == -1 {
        return -1;
    }
    sum += ret2;
    let ret3 = unsafe { xmlTextWriterEndDTDAttlist(writer) };
    if ret3 == -1 {
        return -1;
    }
    sum + ret3
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

    // UPSTREAM-PARITY (xmlwriter.c xmlTextWriterFlush): flush does NOT close
    // an open start tag — php xmlwriter_flush on a writer mid-start-tag
    // returns the raw partial bytes (`<foo`, not `<foo>`) exactly like the
    // oracle (SP-14.3.6, xmlwriter_toMemory_flush_combinations).
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

/// Set the quote character used for attribute and entity values.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextWriterSetQuoteChar(xmlTextWriterPtr writer, xmlChar quotechar);
/// ```
///
/// Only `'` and `'\"'` are accepted; anything else returns -1.
///
/// # SAFETY
///
/// - `writer` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlTextWriterSetQuoteChar(
    writer: *mut XmlTextWriter,
    quotechar: xmlChar,
) -> c_int {
    if writer.is_null() || (quotechar != b'\'' && quotechar != b'"') {
        return -1;
    }
    // SAFETY: writer is a valid XmlTextWriter.
    unsafe { (*writer).qchar = quotechar };
    0
}

/// Close the writer's output buffer. The writer itself is NOT freed (upstream
/// contract: xmlFreeTextWriter does that). Returns XML_ERR_OK (0) on success,
/// XML_ERR_ARGUMENT (9) for a NULL writer or NULL output buffer.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextWriterClose(xmlTextWriterPtr writer);
/// ```
///
/// # SAFETY
///
/// - `writer` must be a valid pointer to an `XmlTextWriter` or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterClose(writer: *mut XmlTextWriter) -> c_int {
    if writer.is_null() {
        return crate::abi::types::XML_ERR_ARGUMENT as c_int;
    }
    let w = unsafe { &mut *writer };
    if w.output.is_null() {
        return crate::abi::types::XML_ERR_ARGUMENT as c_int;
    }
    let result = io::output_buffer_close(w.output);
    w.output = ptr::null_mut();
    if result >= 0 {
        crate::abi::types::XML_ERR_OK as c_int
    } else {
        -result
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Format / VFormat family
// ═══════════════════════════════════════════════════════════════════════════════

/// The System V AMD64 `__va_list_tag` (24 bytes): gp_offset, fp_offset,
/// overflow_arg_area, reg_save_area. A C `va_list` parameter decays to a
/// pointer to this structure, which is exactly what the VFormat exports and
/// the Format shims exchange.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct VaListTag {
    gp_offset: c_uint,
    fp_offset: c_uint,
    overflow_arg_area: *mut c_void,
    reg_save_area: *mut c_void,
}

// The platform `vsnprintf` (system libc — not an oracle dependency).
unsafe extern "C" {
    fn vsnprintf(s: *mut c_char, n: usize, format: *const c_char, ap: *mut VaListTag) -> c_int;
}

/// Format a printf-style string with the given va_list into a fresh buffer,
/// mirroring upstream `xmlTextWriterVSprintf` (BUFSIZ start, doubling growth,
/// fresh va_copy per attempt).
///
/// Returns Err(()) on failure (unrepresentable output or absurd size).
///
/// # SAFETY
///
/// - `format` must be a valid printf format string.
/// - `args` must point to a valid va_list.
unsafe fn vformat_buf(format: *const c_char, args: *mut VaListTag) -> Result<Vec<u8>, ()> {
    let mut size: usize = 8192;
    loop {
        let mut buf = vec![0u8; size];
        // Fresh va_copy per attempt: vsnprintf consumes the va_list.
        // SAFETY: args points to a valid va_list; the bitwise copy is va_copy.
        let mut copy = unsafe { core::ptr::read(args) };
        let n = unsafe { vsnprintf(buf.as_mut_ptr() as *mut c_char, size, format, &mut copy) };
        if n >= 0 && (n as usize) < size {
            buf.truncate(n as usize);
            return Ok(buf);
        }
        if size >= (1 << 26) {
            return Err(());
        }
        size *= 2;
    }
}

// The VFormat functions have heterogeneous fixed-arg lists, so each is written
// explicitly rather than through a macro (mirroring the upstream C).
/// `xmlTextWriterWriteVFormatRaw` — C ABI export.
///
/// # SAFETY
///
/// - `writer`, `argptr` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `format` must point to valid NUL-terminated
///   strings (or NULL where the C contract allows) for the lifetime
///   of the call.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterWriteVFormatRaw(
    writer: *mut XmlTextWriter,
    format: *const c_char,
    argptr: *mut VaListTag,
) -> c_int {
    if writer.is_null() {
        return -1;
    }
    let buf = match unsafe { vformat_buf(format, argptr) } {
        Ok(b) => b,
        Err(()) => return -1,
    };
    unsafe { xmlTextWriterWriteRaw(writer, buf.as_ptr() as *const xmlChar) }
}
/// `xmlTextWriterWriteVFormatString` — C ABI export.
///
/// # SAFETY
///
/// - `writer`, `argptr` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `format` must point to valid NUL-terminated
///   strings (or NULL where the C contract allows) for the lifetime
///   of the call.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterWriteVFormatString(
    writer: *mut XmlTextWriter,
    format: *const c_char,
    argptr: *mut VaListTag,
) -> c_int {
    if writer.is_null() || format.is_null() {
        return -1;
    }
    let buf = match unsafe { vformat_buf(format, argptr) } {
        Ok(b) => b,
        Err(()) => return -1,
    };
    unsafe { xmlTextWriterWriteString(writer, buf.as_ptr() as *const xmlChar) }
}
/// `xmlTextWriterWriteVFormatComment` — C ABI export.
///
/// # SAFETY
///
/// - `writer`, `argptr` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `format` must point to valid NUL-terminated
///   strings (or NULL where the C contract allows) for the lifetime
///   of the call.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterWriteVFormatComment(
    writer: *mut XmlTextWriter,
    format: *const c_char,
    argptr: *mut VaListTag,
) -> c_int {
    if writer.is_null() {
        return -1;
    }
    let buf = match unsafe { vformat_buf(format, argptr) } {
        Ok(b) => b,
        Err(()) => return -1,
    };
    unsafe { xmlTextWriterWriteComment(writer, buf.as_ptr() as *const xmlChar) }
}
/// `xmlTextWriterWriteVFormatCDATA` — C ABI export.
///
/// # SAFETY
///
/// - `writer`, `argptr` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `format` must point to valid NUL-terminated
///   strings (or NULL where the C contract allows) for the lifetime
///   of the call.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterWriteVFormatCDATA(
    writer: *mut XmlTextWriter,
    format: *const c_char,
    argptr: *mut VaListTag,
) -> c_int {
    if writer.is_null() {
        return -1;
    }
    let buf = match unsafe { vformat_buf(format, argptr) } {
        Ok(b) => b,
        Err(()) => return -1,
    };
    unsafe { xmlTextWriterWriteCDATA(writer, buf.as_ptr() as *const xmlChar) }
}
/// `xmlTextWriterWriteVFormatPI` — C ABI export.
///
/// # SAFETY
///
/// - `writer`, `argptr` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `target`, `format` must point to valid NUL-terminated
///   strings (or NULL where the C contract allows) for the lifetime
///   of the call.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterWriteVFormatPI(
    writer: *mut XmlTextWriter,
    target: *const xmlChar,
    format: *const c_char,
    argptr: *mut VaListTag,
) -> c_int {
    if writer.is_null() {
        return -1;
    }
    let buf = match unsafe { vformat_buf(format, argptr) } {
        Ok(b) => b,
        Err(()) => return -1,
    };
    unsafe { xmlTextWriterWritePI(writer, target, buf.as_ptr() as *const xmlChar) }
}
/// `xmlTextWriterWriteVFormatElement` — C ABI export.
///
/// # SAFETY
///
/// - `writer`, `argptr` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `name`, `format` must point to valid NUL-terminated
///   strings (or NULL where the C contract allows) for the lifetime
///   of the call.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterWriteVFormatElement(
    writer: *mut XmlTextWriter,
    name: *const xmlChar,
    format: *const c_char,
    argptr: *mut VaListTag,
) -> c_int {
    if writer.is_null() {
        return -1;
    }
    let buf = match unsafe { vformat_buf(format, argptr) } {
        Ok(b) => b,
        Err(()) => return -1,
    };
    unsafe { xmlTextWriterWriteElement(writer, name, buf.as_ptr() as *const xmlChar) }
}
/// `xmlTextWriterWriteVFormatElementNS` — C ABI export.
///
/// # SAFETY
///
/// - `writer`, `argptr` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `prefix`, `name`, `namespaceURI`, `format` must point to valid NUL-terminated
///   strings (or NULL where the C contract allows) for the lifetime
///   of the call.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterWriteVFormatElementNS(
    writer: *mut XmlTextWriter,
    prefix: *const xmlChar,
    name: *const xmlChar,
    namespaceURI: *const xmlChar,
    format: *const c_char,
    argptr: *mut VaListTag,
) -> c_int {
    if writer.is_null() {
        return -1;
    }
    let buf = match unsafe { vformat_buf(format, argptr) } {
        Ok(b) => b,
        Err(()) => return -1,
    };
    unsafe {
        xmlTextWriterWriteElementNS(
            writer,
            prefix,
            name,
            namespaceURI,
            buf.as_ptr() as *const xmlChar,
        )
    }
}
/// `xmlTextWriterWriteVFormatAttribute` — C ABI export.
///
/// # SAFETY
///
/// - `writer`, `argptr` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `name`, `format` must point to valid NUL-terminated
///   strings (or NULL where the C contract allows) for the lifetime
///   of the call.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterWriteVFormatAttribute(
    writer: *mut XmlTextWriter,
    name: *const xmlChar,
    format: *const c_char,
    argptr: *mut VaListTag,
) -> c_int {
    if writer.is_null() {
        return -1;
    }
    let buf = match unsafe { vformat_buf(format, argptr) } {
        Ok(b) => b,
        Err(()) => return -1,
    };
    unsafe { xmlTextWriterWriteAttribute(writer, name, buf.as_ptr() as *const xmlChar) }
}
/// `xmlTextWriterWriteVFormatAttributeNS` — C ABI export.
///
/// # SAFETY
///
/// - `writer`, `argptr` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `prefix`, `name`, `namespaceURI`, `format` must point to valid NUL-terminated
///   strings (or NULL where the C contract allows) for the lifetime
///   of the call.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterWriteVFormatAttributeNS(
    writer: *mut XmlTextWriter,
    prefix: *const xmlChar,
    name: *const xmlChar,
    namespaceURI: *const xmlChar,
    format: *const c_char,
    argptr: *mut VaListTag,
) -> c_int {
    if writer.is_null() {
        return -1;
    }
    let buf = match unsafe { vformat_buf(format, argptr) } {
        Ok(b) => b,
        Err(()) => return -1,
    };
    unsafe {
        xmlTextWriterWriteAttributeNS(
            writer,
            prefix,
            name,
            namespaceURI,
            buf.as_ptr() as *const xmlChar,
        )
    }
}
/// `xmlTextWriterWriteVFormatDTD` — C ABI export.
///
/// # SAFETY
///
/// - `writer`, `argptr` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `name`, `pubid`, `sysid`, `format` must point to valid NUL-terminated
///   strings (or NULL where the C contract allows) for the lifetime
///   of the call.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterWriteVFormatDTD(
    writer: *mut XmlTextWriter,
    name: *const xmlChar,
    pubid: *const xmlChar,
    sysid: *const xmlChar,
    format: *const c_char,
    argptr: *mut VaListTag,
) -> c_int {
    if writer.is_null() {
        return -1;
    }
    let buf = match unsafe { vformat_buf(format, argptr) } {
        Ok(b) => b,
        Err(()) => return -1,
    };
    unsafe { xmlTextWriterWriteDTD(writer, name, pubid, sysid, buf.as_ptr() as *const xmlChar) }
}
/// `xmlTextWriterWriteVFormatDTDElement` — C ABI export.
///
/// # SAFETY
///
/// - `writer`, `argptr` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `name`, `format` must point to valid NUL-terminated
///   strings (or NULL where the C contract allows) for the lifetime
///   of the call.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterWriteVFormatDTDElement(
    writer: *mut XmlTextWriter,
    name: *const xmlChar,
    format: *const c_char,
    argptr: *mut VaListTag,
) -> c_int {
    if writer.is_null() {
        return -1;
    }
    let buf = match unsafe { vformat_buf(format, argptr) } {
        Ok(b) => b,
        Err(()) => return -1,
    };
    unsafe { xmlTextWriterWriteDTDElement(writer, name, buf.as_ptr() as *const xmlChar) }
}
/// `xmlTextWriterWriteVFormatDTDAttlist` — C ABI export.
///
/// # SAFETY
///
/// - `writer`, `argptr` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `name`, `format` must point to valid NUL-terminated
///   strings (or NULL where the C contract allows) for the lifetime
///   of the call.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterWriteVFormatDTDAttlist(
    writer: *mut XmlTextWriter,
    name: *const xmlChar,
    format: *const c_char,
    argptr: *mut VaListTag,
) -> c_int {
    if writer.is_null() {
        return -1;
    }
    let buf = match unsafe { vformat_buf(format, argptr) } {
        Ok(b) => b,
        Err(()) => return -1,
    };
    unsafe { xmlTextWriterWriteDTDAttlist(writer, name, buf.as_ptr() as *const xmlChar) }
}
/// `xmlTextWriterWriteVFormatDTDInternalEntity` — C ABI export.
///
/// # SAFETY
///
/// - `writer`, `argptr` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `name`, `format` must point to valid NUL-terminated
///   strings (or NULL where the C contract allows) for the lifetime
///   of the call.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlTextWriterWriteVFormatDTDInternalEntity(
    writer: *mut XmlTextWriter,
    pe: c_int,
    name: *const xmlChar,
    format: *const c_char,
    argptr: *mut VaListTag,
) -> c_int {
    if writer.is_null() {
        return -1;
    }
    let buf = match unsafe { vformat_buf(format, argptr) } {
        Ok(b) => b,
        Err(()) => return -1,
    };
    unsafe { xmlTextWriterWriteDTDInternalEntity(writer, pe, name, buf.as_ptr() as *const xmlChar) }
}

/// Assembly shims for the variadic `xmlTextWriterWriteFormat*` exports.
///
/// Stable Rust cannot define variadic `extern "C"` functions (c_variadic is
/// unstable), so each Format export is a #[no_mangle] function whose body is a
/// single `noreturn` inline-asm block: it captures the SysV x86-64 register
/// save area exactly like `va_start`, builds a `va_list`, forwards it to the
/// VFormat implementation, restores the stack and returns directly.
/// `#![no_mangle]` puts these exports into rustc's cdylib export list (a
/// version script localizes every other global).
///
/// Layout: reg_save_area = rsp+0 (6 GP + 8 SSE slots, 176 bytes); the va_list
/// struct lives at rsp+176 (gp_offset, fp_offset, overflow_arg_area,
/// reg_save_area); overflow varargs are above the return address.
///
/// NOTE on the frame: LLVM emits an 8-byte alignment `push` before the block
/// (verified for rustc 1.98.0 at opt-level 0); the block therefore uses a
/// 240-byte frame (≡ 0 mod 16, keeping the `call` 16-aligned), points the
/// overflow area at rsp+256 (= entry_rsp + 8) and pops the alignment push
/// before `ret`. The overflow-argument pointer is only dereferenced when more
/// than 6 general-purpose varargs are passed; WRITER-001 exercises that path.
/// This is native code with no dependency on any XML library.
#[cfg(target_arch = "x86_64")]
mod format_shims {
    use super::*;

    /// `gp` is the gp_offset for the fixed-argument count (8 bytes each);
    /// `aptr` is the register receiving the va_list pointer for the VFormat
    /// call (rdx=2 fixed, rcx=3, r8=4, r9=5). The parameter list is types
    /// only — the values are read directly from registers inside the asm.
    macro_rules! vfmt_shim {
        ($name:ident, $vname:ident, $gp:literal, $aptr:tt, ($($pty:ty),*)) => {
            // No declared parameters: the C caller's fixed arguments arrive in
            // the ABI registers and are read directly inside the asm; with no
            // parameters and a noreturn body LLVM emits only an 8-byte
            // alignment push, which the block pops before `ret`.
            #[no_mangle]
            pub unsafe extern "C" fn $name() -> c_int {
                unsafe {
                    core::arch::asm!(
                        "sub rsp, 240",
                        "mov [rsp+0], rdi",
                        "mov [rsp+8], rsi",
                        "mov [rsp+16], rdx",
                        "mov [rsp+24], rcx",
                        "mov [rsp+32], r8",
                        "mov [rsp+40], r9",
                        "movaps [rsp+48], xmm0",
                        "movaps [rsp+64], xmm1",
                        "movaps [rsp+80], xmm2",
                        "movaps [rsp+96], xmm3",
                        "movaps [rsp+112], xmm4",
                        "movaps [rsp+128], xmm5",
                        "movaps [rsp+144], xmm6",
                        "movaps [rsp+160], xmm7",
                        concat!("mov dword ptr [rsp+176], ", $gp),
                        "mov dword ptr [rsp+180], 48",
                        "lea rax, [rsp+256]",
                        "mov [rsp+184], rax",
                        "lea rax, [rsp]",
                        "mov [rsp+192], rax",
                        concat!("lea ", stringify!($aptr), ", [rsp+176]"),
                        concat!("call ", stringify!($vname)),
                        "add rsp, 240",
                        "add rsp, 8",
                        "ret",
                        options(noreturn),
                    );
                }
            }
        };
    }

    vfmt_shim!(
        xmlTextWriterWriteFormatRaw,
        xmlTextWriterWriteVFormatRaw,
        16,
        rdx,
        (*mut XmlTextWriter, *const c_char)
    );
    vfmt_shim!(
        xmlTextWriterWriteFormatString,
        xmlTextWriterWriteVFormatString,
        16,
        rdx,
        (*mut XmlTextWriter, *const c_char)
    );
    vfmt_shim!(
        xmlTextWriterWriteFormatComment,
        xmlTextWriterWriteVFormatComment,
        16,
        rdx,
        (*mut XmlTextWriter, *const c_char)
    );
    vfmt_shim!(
        xmlTextWriterWriteFormatCDATA,
        xmlTextWriterWriteVFormatCDATA,
        16,
        rdx,
        (*mut XmlTextWriter, *const c_char)
    );
    vfmt_shim!(
        xmlTextWriterWriteFormatPI,
        xmlTextWriterWriteVFormatPI,
        24,
        rcx,
        (*mut XmlTextWriter, *const xmlChar, *const c_char)
    );
    vfmt_shim!(
        xmlTextWriterWriteFormatElement,
        xmlTextWriterWriteVFormatElement,
        24,
        rcx,
        (*mut XmlTextWriter, *const xmlChar, *const c_char)
    );
    vfmt_shim!(
        xmlTextWriterWriteFormatAttribute,
        xmlTextWriterWriteVFormatAttribute,
        24,
        rcx,
        (*mut XmlTextWriter, *const xmlChar, *const c_char)
    );
    vfmt_shim!(
        xmlTextWriterWriteFormatDTDElement,
        xmlTextWriterWriteVFormatDTDElement,
        24,
        rcx,
        (*mut XmlTextWriter, *const xmlChar, *const c_char)
    );
    vfmt_shim!(
        xmlTextWriterWriteFormatDTDAttlist,
        xmlTextWriterWriteVFormatDTDAttlist,
        24,
        rcx,
        (*mut XmlTextWriter, *const xmlChar, *const c_char)
    );
    vfmt_shim!(
        xmlTextWriterWriteFormatDTDInternalEntity,
        xmlTextWriterWriteVFormatDTDInternalEntity,
        32,
        r8,
        (*mut XmlTextWriter, c_int, *const xmlChar, *const c_char)
    );
    vfmt_shim!(
        xmlTextWriterWriteFormatDTD,
        xmlTextWriterWriteVFormatDTD,
        40,
        r9,
        (
            *mut XmlTextWriter,
            *const xmlChar,
            *const xmlChar,
            *const xmlChar,
            *const c_char
        )
    );
    vfmt_shim!(
        xmlTextWriterWriteFormatElementNS,
        xmlTextWriterWriteVFormatElementNS,
        40,
        r9,
        (
            *mut XmlTextWriter,
            *const xmlChar,
            *const xmlChar,
            *const xmlChar,
            *const c_char
        )
    );
    vfmt_shim!(
        xmlTextWriterWriteFormatAttributeNS,
        xmlTextWriterWriteVFormatAttributeNS,
        40,
        r9,
        (
            *mut XmlTextWriter,
            *const xmlChar,
            *const xmlChar,
            *const xmlChar,
            *const c_char
        )
    );
}

// Non-x86-64 fallback: honest stubs (the variadic ABI cannot be forwarded on
// stable Rust); the platform surface is documented as not yet executable there.
#[cfg(not(target_arch = "x86_64"))]
mod format_fallback {
    use super::*;
    macro_rules! fmt_stub {
        ($($name:ident),*) => {$(
            #[no_mangle]
            pub unsafe extern "C" fn $name(_writer: *mut XmlTextWriter, _format: *const c_char) -> c_int {
                -1
            }
        )*};
    }
    fmt_stub!(
        xmlTextWriterWriteFormatRaw,
        xmlTextWriterWriteFormatString,
        xmlTextWriterWriteFormatComment,
        xmlTextWriterWriteFormatCDATA,
        xmlTextWriterWriteFormatPI,
        xmlTextWriterWriteFormatElement,
        xmlTextWriterWriteFormatElementNS,
        xmlTextWriterWriteFormatAttribute,
        xmlTextWriterWriteFormatAttributeNS,
        xmlTextWriterWriteFormatDTD,
        xmlTextWriterWriteFormatDTDElement,
        xmlTextWriterWriteFormatDTDAttlist,
        xmlTextWriterWriteFormatDTDInternalEntity
    );
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
    let mut result = Vec::with_capacity(data.len().div_ceil(3) * 4);
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
    ///
    /// # Safety
    ///
    /// - `buf` must be NULL or a valid pointer to a `_xmlBuffer`; its
    ///   `content` must be NULL or point to `use_` readable bytes, and the
    ///   pointer must stay valid for the duration of the call.
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

    /// Verify a simple document is written through the memory buffer.
    ///
    /// # Safety
    ///
    /// - `create_test_writer` returns a valid writer and `_xmlBuffer`; the
    ///   NUL-terminated string literals passed to the writer functions are
    ///   valid, and the writer and buffer are freed exactly once at the end.
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
            assert!(r > 0, "EndDocument failed (rc={})", r);

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

    /// Verify elements with attributes are written and escaped.
    ///
    /// # Safety
    ///
    /// - The writer and buffer from `create_test_writer` are valid, the
    ///   NUL-terminated string literals passed to the writer functions are
    ///   valid, and the writer and buffer are freed exactly once at the end.
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

    /// Verify namespaced elements and attributes are written.
    ///
    /// # Safety
    ///
    /// - The writer and buffer from `create_test_writer` are valid, the
    ///   NUL-terminated string literals passed to the writer functions are
    ///   valid, and the writer and buffer are freed exactly once at the end.
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

    /// Verify text, CDATA, comments and processing instructions are written.
    ///
    /// # Safety
    ///
    /// - The writer and buffer from `create_test_writer` are valid, the
    ///   NUL-terminated string literals passed to the writer functions are
    ///   valid, and the writer and buffer are freed exactly once at the end.
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

    /// Verify a DTD declaration is written.
    ///
    /// # Safety
    ///
    /// - The writer and buffer from `create_test_writer` are valid, the
    ///   NUL-terminated string literals passed to the writer functions are
    ///   valid, and the writer and buffer are freed exactly once at the end.
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

    /// Verify a DTD with an internal subset is written.
    ///
    /// # Safety
    ///
    /// - The writer and buffer from `create_test_writer` are valid, the
    ///   NUL-terminated string literals passed to the writer functions are
    ///   valid, and the writer and buffer are freed exactly once at the end.
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
                0, // pe
                b"copy\0" as *const u8,
                ptr::null(), // pubid
                ptr::null(), // sysid
                ptr::null(), // ndataid
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

    /// Verify indentation control with a custom indent string.
    ///
    /// # Safety
    ///
    /// - The writer and buffer from `create_test_writer` are valid, the
    ///   NUL-terminated indent string is valid, and the writer and buffer
    ///   are freed exactly once at the end.
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

    /// Verify writing to a memory buffer created with `xmlNewTextWriterMemory`.
    ///
    /// # Safety
    ///
    /// - `buf` is a valid buffer from `io::buf_create`; the writer borrows
    ///   it and is freed with `xmlFreeTextWriter`, then `buf` is freed
    ///   exactly once with `io::buf_free`.
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

    /// Verify flushing mid-document and closing the writer.
    ///
    /// # Safety
    ///
    /// - The writer and buffer from `create_test_writer` are valid, the
    ///   NUL-terminated string literals are valid, and the writer and buffer
    ///   are freed exactly once at the end.
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

    /// Verify every writer entry point accepts a NULL writer.
    ///
    /// # Safety
    ///
    /// - NULL writers and NULL string arguments are accepted and return -1
    ///   without dereferencing; `xmlFreeTextWriter` accepts NULL.
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

    /// Verify deeply nested elements are written in order.
    ///
    /// # Safety
    ///
    /// - The writer and buffer from `create_test_writer` are valid, the
    ///   NUL-terminated string literals are valid, and the writer and buffer
    ///   are freed exactly once at the end.
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

    /// Verify an empty element is self-closing.
    ///
    /// # Safety
    ///
    /// - The writer and buffer from `create_test_writer` are valid, the
    ///   NUL-terminated element name is valid, and the writer and buffer are
    ///   freed exactly once at the end.
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

    /// Verify `xmlTextWriterFullEndElement` emits an explicit end tag.
    ///
    /// # Safety
    ///
    /// - The writer and buffer from `create_test_writer` are valid, the
    ///   NUL-terminated element name is valid, and the writer and buffer are
    ///   freed exactly once at the end.
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

    /// Verify `xmlTextWriterWriteElement` writes an element with inline
    /// content.
    ///
    /// # Safety
    ///
    /// - The writer and buffer from `create_test_writer` are valid, the
    ///   NUL-terminated string literals are valid, and the writer and buffer
    ///   are freed exactly once at the end.
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

    /// Verify XML escaping of text content.
    ///
    /// # Safety
    ///
    /// - The writer and buffer from `create_test_writer` are valid, the
    ///   NUL-terminated string literals are valid, and the writer and buffer
    ///   are freed exactly once at the end.
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

    /// Verify raw content is written without escaping.
    ///
    /// # Safety
    ///
    /// - The writer and buffer from `create_test_writer` are valid, the
    ///   NUL-terminated string literals are valid, and the writer and buffer
    ///   are freed exactly once at the end.
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

    /// Verify base64 content is written with explicit byte length.
    ///
    /// # Safety
    ///
    /// - The writer and buffer from `create_test_writer` are valid; `test_data`
    ///   is a valid byte slice whose length is passed explicitly, and the
    ///   writer and buffer are freed exactly once at the end.
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

    /// Verify incremental CDATA, comment and PI writing.
    ///
    /// # Safety
    ///
    /// - The writer and buffer from `create_test_writer` are valid, the
    ///   NUL-terminated string literals are valid, and the writer and buffer
    ///   are freed exactly once at the end.
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

    /// Verify `xmlNewTextWriterFilename` returns NULL for a NULL uri.
    ///
    /// # Safety
    ///
    /// - A NULL uri is accepted and returns NULL without dereferencing.
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

    /// Verify `xmlNewTextWriter` returns NULL for a NULL output buffer.
    ///
    /// # Safety
    ///
    /// - A NULL output pointer is accepted and returns NULL without
    ///   dereferencing.
    #[test]
    fn test_new_writer_null_output() {
        unsafe {
            let writer = xmlNewTextWriter(ptr::null_mut());
            assert!(writer.is_null(), "Expected NULL for null output");
        }
    }
}
