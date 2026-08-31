//! Custom I/O and resource loaders (§59, §85 Phase 4).
//!
//! Input/output callback infrastructure: buffers, file I/O, encoding
//! integration, input/output buffer management, and helper utilities.
//!
//! This module implements the libxml2 I/O subsystem in native Rust.
//! No C dependencies beyond `libc` for file operations.
//!
//! # Upstream contract
//!
//! Mirrors upstream `xmlIO.c` (`SRC-LIBXML2-2.15.0-XMLIO-C`, parity target
//! libxml2 2.15.3 oracle): the xmlInput/xmlOutput callback registries,
//! `xmlParserInputBuffer` / `xmlOutputBuffer` / `xmlBuffer` / `xmlBuf`
//! families, the entity loader slot (`xmlSetExternalEntityLoader` /
//! `xmlLoadExternalEntity`), and the file/fd/memory/IO input constructors.
//!
//! # Conceptual behavior
//!
//! Buffers grow per the `XML_BUFFER_ALLOC_*` scheme (DOUBLEIT=0, EXACT=1,
//! IMMUTABLE=2 — the upstream xmlBufferAllocationScheme enum order).
//! Input/output callback pairs are registered in a global table and matched
//! by URL scheme; output buffers integrate the encoding layer, where
//! `xmlOutputBufferWrite` takes the encoder path below the 256-byte
//! conversion threshold (xmlIO.c).
//!
//! # Ownership & safety invariants
//!
//! An output buffer owns its write/close callbacks and its encoder;
//! `xmlOutputBufferClose` drains and frees them. A buffer handed to a
//! writer (`xmlNewTextWriterMemory`) is borrowed — the writer appends, the
//! caller frees after the writer (OWNERSHIP_ATLAS §5). `xmlBufferDetach`
//! transfers the content pointer to the caller (caller frees with xmlFree).
//! Callback user-data pointers are stored verbatim and never dereferenced
//! (OWNERSHIP_ATLAS §6).
//!
//! # Historical quirks & epochs
//!
//! R-000151: the encoder-dependent byte-count contract (0 bytes below the
//! 256-byte threshold once an encoder is installed) comes from xmlIO.c and
//! is required by WRITER-001. R-000161: the default buffer alloc scheme is
//! EXACT (xmlBufferAllocScheme default 1) per the 2.15.3 oracle defaults
//! dump. R-000162: the NOENT external-entity path must consult
//! `xmlLoadExternalEntity` so registered entity loaders actually fire.
//!
//! # Deliberate oddities
//!
//! The deprecated `xmlBuffer*` family is kept byte-faithful (alloc scheme
//! constants in enum order) instead of being replaced by `xmlBuf*`;
//! the registry stores callbacks as raw fn pointers plus a context that is
//! passed back verbatim, mirroring xmlIO.c registration semantics.
//!
//! # Proving courts
//!
//! Exercised by WRITER-001, ENCODING-001 and CALLBACK-001
//! (`courts/suites/data-abi/*-family-probe.c`), which require byte-identical
//! output against the oracle DSO; the parallel lib test suite (cargo test)
//! covers the alloc schemes and buffer lifecycle.
//!
//! # Tempting simplifications that would break parity
//!
//! Do not make write paths always return raw byte counts: the encoder
//! threshold (R-000151) and the writer return contract depend on the 0-byte
//! behavior. Do not drop the entity-loader consultation (R-000162) and do
//! not unify the three alloc schemes into one growth policy — both are
//! observable through the C ABI.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_ptr_alignment,
    clippy::missing_safety_doc
)]

use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_uint, c_void};
use std::ptr;

use libc;

use crate::abi::allocator::{xmlFreeImpl, xmlMallocImpl, xmlReallocImpl};
use crate::abi::callbacks::{
    xmlInputCloseCallback, xmlInputReadCallback, xmlOutputCloseCallback, xmlOutputWriteCallback,
};
use crate::abi::structs::{
    _xmlBuf, _xmlBuffer, _xmlCharEncodingHandler, _xmlOutputBuffer, _xmlParserInputBuffer,
};
use crate::abi::types::{xmlChar, xmlCharEncoding};
use crate::xml::encoding;

// ═══════════════════════════════════════════════════════════════════════════════
// Constants
// ═══════════════════════════════════════════════════════════════════════════════

/// Default buffer size for new buffers.
pub(crate) const DEFAULT_BUFFER_SIZE: c_uint = 4000;

/// Minimum buffer size.
const MIN_BUFFER_SIZE: c_uint = 256;

/// Buffer allocation scheme: double the size on growth.
const XML_BUFFER_ALLOC_DOUBLEIT: c_int = 0;

/// Buffer allocation scheme: exact size on growth.
const XML_BUFFER_ALLOC_EXACT: c_int = 1;

/// Buffer allocation scheme: immutable (no growth, no free of content).
const XML_BUFFER_ALLOC_IMMUTABLE: c_int = 2;

// ═══════════════════════════════════════════════════════════════════════════════
// 1. xmlBuffer operations (deprecated)
// ═══════════════════════════════════════════════════════════════════════════════

/// Create a new xmlBuffer with the given initial size.
///
/// If `size` <= 0, a default buffer size is used.
/// The buffer content is initialized to an empty null-terminated string.
///
/// Returns a pointer to the new buffer, or NULL on allocation failure.
pub(crate) fn buf_create(size: c_int) -> *mut _xmlBuffer {
    let buf_size = if size <= 0 {
        DEFAULT_BUFFER_SIZE
    } else {
        size as c_uint
    };

    // Ensure minimum size
    let buf_size = buf_size.max(MIN_BUFFER_SIZE);

    let buf = unsafe { xmlMallocImpl(size_of::<_xmlBuffer>()) as *mut _xmlBuffer };
    if buf.is_null() {
        return ptr::null_mut();
    }

    let content = unsafe { xmlMallocImpl(buf_size as usize) as *mut xmlChar };
    if content.is_null() {
        unsafe { xmlFreeImpl(buf as *mut c_void) };
        return ptr::null_mut();
    }

    // Initialize: null-terminate the empty buffer
    unsafe {
        ptr::write(content, 0);
    }

    unsafe {
        ptr::write(
            buf,
            _xmlBuffer {
                content,
                use_: 0,
                size: buf_size,
                alloc: XML_BUFFER_ALLOC_DOUBLEIT,
                contentIO: content, // Track original allocation for I/O mode
            },
        );
    }

    buf
}

/// Create a new xmlBuffer from a static string.
///
/// The buffer's content points directly to `str` (no copy is made).
/// The buffer's allocation scheme is set to IMMUTABLE, meaning the
/// content will not be freed when the buffer is freed.
///
/// If `size` <= 0, the length is determined by `xmlStrlen` (scanning for null).
pub(crate) fn buf_create_static(str: *const xmlChar, size: c_int) -> *mut _xmlBuffer {
    if str.is_null() {
        return ptr::null_mut();
    }

    let len = if size <= 0 {
        // Calculate length by scanning for null terminator
        let mut len: c_uint = 0;
        unsafe {
            while *str.add(len as usize) != 0 {
                len += 1;
            }
        }
        len
    } else {
        size as c_uint
    };

    let buf = unsafe { xmlMallocImpl(size_of::<_xmlBuffer>()) as *mut _xmlBuffer };
    if buf.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        ptr::write(
            buf,
            _xmlBuffer {
                content: str as *mut xmlChar,
                use_: len,
                size: len + 1, // Include space for null terminator
                alloc: XML_BUFFER_ALLOC_IMMUTABLE,
                contentIO: ptr::null_mut(),
            },
        );
    }

    buf
}

/// Free an xmlBuffer.
///
/// If the buffer's allocation scheme is not IMMUTABLE, the content is freed.
/// The contentIO pointer (if non-NULL and different from content) is also freed.
pub(crate) fn buf_free(buf: *mut _xmlBuffer) {
    if buf.is_null() {
        return;
    }

    unsafe {
        let alloc = (*buf).alloc;
        let content = (*buf).content;
        let content_io = (*buf).contentIO;

        if alloc != XML_BUFFER_ALLOC_IMMUTABLE {
            // contentIO is the original allocation base (set in I/O mode).
            // content may have been advanced during reads.
            // Free the base pointer, not the possibly-advanced content.
            let base = if !content_io.is_null() {
                content_io
            } else {
                content
            };
            if !base.is_null() {
                xmlFreeImpl(base as *mut c_void);
            }
        }

        xmlFreeImpl(buf as *mut c_void);
    }
}

/// Empty an xmlBuffer (reset `use_` to 0).
///
/// The content is kept allocated but the first byte is set to null terminator.
pub(crate) fn buf_empty(buf: *mut _xmlBuffer) {
    if buf.is_null() {
        return;
    }

    unsafe {
        (*buf).use_ = 0;
        if !(*buf).content.is_null() {
            ptr::write((*buf).content, 0);
        }
    }
}

/// Get the content of an xmlBuffer.
pub(crate) fn buf_content(buf: *mut _xmlBuffer) -> *mut xmlChar {
    if buf.is_null() {
        return ptr::null_mut();
    }
    unsafe { (*buf).content }
}

/// Get the used length of an xmlBuffer.
pub(crate) fn buf_length(buf: *mut _xmlBuffer) -> c_int {
    if buf.is_null() {
        return -1;
    }
    unsafe { (*buf).use_ as c_int }
}

/// Write `len` bytes from `str` to an xmlBuffer.
///
/// Grows the buffer if needed. Always maintains null termination.
/// Returns the number of bytes written, or -1 on error.
pub(crate) fn buf_add(buf: *mut _xmlBuffer, str: *const xmlChar, len: c_int) -> c_int {
    if buf.is_null() || str.is_null() || len <= 0 {
        return 0;
    }

    let len = len as c_uint;
    let b = unsafe { &mut *buf };

    // IMMUTABLE buffers cannot be written to
    if b.alloc == XML_BUFFER_ALLOC_IMMUTABLE {
        return -1;
    }

    // Ensure capacity: need use_ + len + 1 (for null terminator)
    let needed = b.use_.saturating_add(len).saturating_add(1);
    if needed > b.size {
        // Grow buffer
        let new_size = if b.alloc == XML_BUFFER_ALLOC_EXACT {
            needed
        } else {
            // DOUBLEIT or default: double until big enough
            let mut doubled = b.size.saturating_mul(2).max(MIN_BUFFER_SIZE);
            while doubled < needed {
                doubled = doubled.saturating_mul(2);
            }
            doubled
        };

        let new_content =
            unsafe { xmlReallocImpl(b.content as *mut c_void, new_size as usize) as *mut xmlChar };
        if new_content.is_null() {
            return -1;
        }
        b.content = new_content;
        b.contentIO = new_content; // Track reallocated base
        b.size = new_size;
    }

    // Copy data
    unsafe {
        ptr::copy_nonoverlapping(str, b.content.add(b.use_ as usize), len as usize);
    }
    b.use_ = b.use_.saturating_add(len);

    // Null-terminate
    unsafe {
        ptr::write(b.content.add(b.use_ as usize), 0);
    }

    len as c_int
}

/// Cat a null-terminated string to an xmlBuffer.
pub(crate) fn buf_cat(buf: *mut _xmlBuffer, str: *const xmlChar) -> c_int {
    if buf.is_null() || str.is_null() {
        return -1;
    }

    // Calculate length of the null-terminated string
    let len = unsafe {
        let mut i: c_uint = 0;
        while *str.add(i as usize) != 0 {
            i += 1;
        }
        i
    };

    buf_add(buf, str, len as c_int)
}

/// Write a single character to an xmlBuffer.
pub(crate) fn buf_ccat(buf: *mut _xmlBuffer, c: xmlChar) -> c_int {
    buf_add(buf, &c as *const xmlChar, 1)
}

/// Shrink an xmlBuffer by `len` bytes from the end.
///
/// If `len` exceeds the used length, the buffer is emptied.
/// Returns the new used length, or -1 on error.
#[allow(dead_code)]
pub(crate) fn buf_shrink(buf: *mut _xmlBuffer, len: c_uint) -> c_int {
    if buf.is_null() {
        return -1;
    }

    let b = unsafe { &mut *buf };
    if b.use_ == 0 {
        return 0;
    }

    b.use_ = b.use_.saturating_sub(len);

    // Null-terminate
    unsafe {
        ptr::write(b.content.add(b.use_ as usize), 0);
    }

    b.use_ as c_int
}

/// Grow an xmlBuffer to at least `size` bytes of capacity.
///
/// Returns 0 on success, -1 on failure.
/// Add data at the head of a buffer.
///
/// Returns 0 on success, -1 on error.
pub(crate) fn buf_add_head(buf: *mut _xmlBuffer, str: *const xmlChar, len: c_int) -> c_int {
    if buf.is_null() || str.is_null() || len <= 0 {
        return -1;
    }
    let len = len as c_uint;
    unsafe {
        let b = &mut *buf;
        let needed = b.use_.saturating_add(len).saturating_add(1);
        if needed > b.size {
            let new_size = needed.saturating_mul(2).max(MIN_BUFFER_SIZE);
            let new_content =
                xmlReallocImpl(b.content as *mut c_void, new_size as usize) as *mut xmlChar;
            if new_content.is_null() {
                return -1;
            }
            b.content = new_content;
            b.contentIO = new_content;
            b.size = new_size;
        }
        // Shift existing content right by len bytes
        if b.use_ > 0 {
            core::ptr::copy(b.content, b.content.add(len as usize), b.use_ as usize);
        }
        // Copy new content to the beginning
        core::ptr::copy_nonoverlapping(str, b.content, len as usize);
        b.use_ = b.use_.saturating_add(len);
        *b.content.add(b.use_ as usize) = 0;
    }
    0
}

pub(crate) fn buf_grow(buf: *mut _xmlBuffer, size: c_uint) -> c_int {
    if buf.is_null() {
        return -1;
    }

    let b = unsafe { &mut *buf };

    if size <= b.size {
        return 0; // Already big enough
    }

    let new_content =
        unsafe { xmlReallocImpl(b.content as *mut c_void, size as usize) as *mut xmlChar };
    if new_content.is_null() {
        return -1;
    }

    b.content = new_content;
    b.contentIO = new_content;
    b.size = size;

    0
}

// ═══════════════════════════════════════════════════════════════════════════════
// 2. xmlBuf operations (modern replacement)
// ═══════════════════════════════════════════════════════════════════════════════

/// Create a new xmlBuf with the given initial size.
///
/// If `size` <= 0, a default buffer size is used.
/// Returns a pointer to the new buffer, or NULL on allocation failure.
#[allow(dead_code)]
pub(crate) fn xml_buf_create(size: c_int) -> *mut _xmlBuf {
    let buf_size = if size <= 0 {
        DEFAULT_BUFFER_SIZE
    } else {
        size as c_uint
    };
    let buf_size = buf_size.max(MIN_BUFFER_SIZE);

    let buf = unsafe { xmlMallocImpl(size_of::<_xmlBuf>()) as *mut _xmlBuf };
    if buf.is_null() {
        return ptr::null_mut();
    }

    let content = unsafe { xmlMallocImpl(buf_size as usize) as *mut xmlChar };
    if content.is_null() {
        unsafe { xmlFreeImpl(buf as *mut c_void) };
        return ptr::null_mut();
    }

    unsafe {
        ptr::write(content, 0);
    }

    unsafe {
        ptr::write(
            buf,
            _xmlBuf {
                content,
                use_: 0,
                size: buf_size,
                alloc: XML_BUFFER_ALLOC_DOUBLEIT,
                error: 0,
                buffer: 0,
                io: 0,
            },
        );
    }

    buf
}

/// Free an xmlBuf.
///
/// Frees the content and the buffer struct itself.
#[allow(dead_code)]
pub(crate) fn xml_buf_free(buf: *mut _xmlBuf) {
    if buf.is_null() {
        return;
    }

    unsafe {
        if !(*buf).content.is_null() {
            xmlFreeImpl((*buf).content as *mut c_void);
        }
        xmlFreeImpl(buf as *mut c_void);
    }
}

/// Get the content of an xmlBuf.
#[allow(dead_code)]
pub(crate) fn xml_buf_content(buf: *mut _xmlBuf) -> *mut xmlChar {
    if buf.is_null() {
        return ptr::null_mut();
    }
    unsafe { (*buf).content }
}

/// Get the used length of an xmlBuf.
#[allow(dead_code)]
pub(crate) fn xml_buf_length(buf: *mut _xmlBuf) -> c_int {
    if buf.is_null() {
        return -1;
    }
    unsafe { (*buf).use_ as c_int }
}

/// Add `len` bytes from `str` to an xmlBuf.
///
/// Returns the number of bytes added, or -1 on error.
pub(crate) fn xml_buf_add(buf: *mut _xmlBuf, str: *const xmlChar, len: c_int) -> c_int {
    if buf.is_null() || str.is_null() || len <= 0 {
        return 0;
    }

    let len = len as c_uint;
    let b = unsafe { &mut *buf };

    let needed = b.use_.saturating_add(len).saturating_add(1);
    if needed > b.size {
        let new_size = needed.saturating_mul(2).max(MIN_BUFFER_SIZE);
        let new_content =
            unsafe { xmlReallocImpl(b.content as *mut c_void, new_size as usize) as *mut xmlChar };
        if new_content.is_null() {
            return -1;
        }
        b.content = new_content;
        b.size = new_size;
    }

    unsafe {
        ptr::copy_nonoverlapping(str, b.content.add(b.use_ as usize), len as usize);
    }
    b.use_ = b.use_.saturating_add(len);

    unsafe {
        ptr::write(b.content.add(b.use_ as usize), 0);
    }

    len as c_int
}

/// Cat a null-terminated string to an xmlBuf.
pub(crate) fn xml_buf_cat(buf: *mut _xmlBuf, str: *const xmlChar) -> c_int {
    if buf.is_null() || str.is_null() {
        return -1;
    }

    let len = unsafe {
        let mut i: c_uint = 0;
        while *str.add(i as usize) != 0 {
            i += 1;
        }
        i
    };

    xml_buf_add(buf, str, len as c_int)
}

/// Grow an xmlBuf to at least `size` bytes of capacity.
///
/// Returns 0 on success, -1 on failure.
#[allow(dead_code)]
pub(crate) fn xml_buf_grow(buf: *mut _xmlBuf, size: c_uint) -> c_int {
    if buf.is_null() {
        return -1;
    }

    let b = unsafe { &mut *buf };
    if size <= b.size {
        return 0;
    }

    let new_content =
        unsafe { xmlReallocImpl(b.content as *mut c_void, size as usize) as *mut xmlChar };
    if new_content.is_null() {
        return -1;
    }

    b.content = new_content;
    b.size = size;
    0
}

/// Shrink an xmlBuf by `len` bytes from the end.
///
/// Returns the new used length, or -1 on error.
#[allow(dead_code)]
pub(crate) fn xml_buf_shrink(buf: *mut _xmlBuf, len: c_uint) -> c_int {
    if buf.is_null() {
        return -1;
    }

    let b = unsafe { &mut *buf };
    if b.use_ == 0 {
        return 0;
    }

    b.use_ = b.use_.saturating_sub(len);

    unsafe {
        ptr::write(b.content.add(b.use_ as usize), 0);
    }

    b.use_ as c_int
}

// ═══════════════════════════════════════════════════════════════════════════════
// 3. Input buffer operations
// ═══════════════════════════════════════════════════════════════════════════════

/// Convert a `c_int` encoding value to an `xmlCharEncoding` enum.
const fn encoding_from_int(enc: c_int) -> xmlCharEncoding {
    match enc {
        -1 => xmlCharEncoding::XML_CHAR_ENCODING_ERROR,
        0 => xmlCharEncoding::XML_CHAR_ENCODING_NONE,
        1 => xmlCharEncoding::XML_CHAR_ENCODING_UTF8,
        2 => xmlCharEncoding::XML_CHAR_ENCODING_UTF16LE,
        3 => xmlCharEncoding::XML_CHAR_ENCODING_UTF16BE,
        4 => xmlCharEncoding::XML_CHAR_ENCODING_UCS4LE,
        5 => xmlCharEncoding::XML_CHAR_ENCODING_UCS4BE,
        6 => xmlCharEncoding::XML_CHAR_ENCODING_EBCDIC,
        7 => xmlCharEncoding::XML_CHAR_ENCODING_UCS4_2143,
        8 => xmlCharEncoding::XML_CHAR_ENCODING_UCS4_3412,
        9 => xmlCharEncoding::XML_CHAR_ENCODING_UCS2,
        10 => xmlCharEncoding::XML_CHAR_ENCODING_8859_1,
        11 => xmlCharEncoding::XML_CHAR_ENCODING_8859_2,
        12 => xmlCharEncoding::XML_CHAR_ENCODING_8859_3,
        13 => xmlCharEncoding::XML_CHAR_ENCODING_8859_4,
        14 => xmlCharEncoding::XML_CHAR_ENCODING_8859_5,
        15 => xmlCharEncoding::XML_CHAR_ENCODING_8859_6,
        16 => xmlCharEncoding::XML_CHAR_ENCODING_8859_7,
        17 => xmlCharEncoding::XML_CHAR_ENCODING_8859_8,
        18 => xmlCharEncoding::XML_CHAR_ENCODING_8859_9,
        19 => xmlCharEncoding::XML_CHAR_ENCODING_2022_JP,
        20 => xmlCharEncoding::XML_CHAR_ENCODING_SHIFT_JIS,
        21 => xmlCharEncoding::XML_CHAR_ENCODING_EUC_JP,
        22 => xmlCharEncoding::XML_CHAR_ENCODING_ASCII,
        _ => xmlCharEncoding::XML_CHAR_ENCODING_ERROR,
    }
}

/// Find an encoding handler for the given encoding integer.
///
/// Returns a pointer to the handler, or NULL if not found or if the
/// encoding is NONE or UTF-8 (which don't need conversion).
fn find_handler_for_encoding(enc: c_int) -> *mut _xmlCharEncodingHandler {
    let enc_enum = encoding_from_int(enc);
    if enc_enum == xmlCharEncoding::XML_CHAR_ENCODING_NONE
        || enc_enum == xmlCharEncoding::XML_CHAR_ENCODING_UTF8
        || enc_enum == xmlCharEncoding::XML_CHAR_ENCODING_ERROR
    {
        return ptr::null_mut();
    }

    // Get the encoding name and create a null-terminated version for lookup
    if let Some(name) = encoding::encoding_name(enc_enum) {
        // Create a null-terminated copy on the stack if small, or heap
        let mut name_nul = name.to_vec();
        name_nul.push(0);
        let handler = encoding::find_encoding_handler(name_nul.as_ptr() as *const xmlChar);
        if !handler.is_null() {
            return handler;
        }
    }

    ptr::null_mut()
}

/// Internal helper: create an _xmlParserInputBuffer struct.
///
/// Allocates the struct and initializes all fields to zero/NULL.
/// The caller is responsible for setting the specific fields.
fn allocate_input_buffer() -> *mut _xmlParserInputBuffer {
    let buf =
        unsafe { xmlMallocImpl(size_of::<_xmlParserInputBuffer>()) as *mut _xmlParserInputBuffer };
    if buf.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        ptr::write(
            buf,
            _xmlParserInputBuffer {
                context: ptr::null_mut(),
                readcallback: None,
                closecallback: None,
                encoder: ptr::null_mut(),
                buffer: ptr::null_mut(),
                raw: ptr::null_mut(),
                compressed: 0,
                error: 0,
                rawconsumed: 0,
            },
        );
    }

    buf
}

/// Create an input buffer from memory.
///
/// The data is copied into the input buffer's internal storage.
/// If `enc` specifies a non-UTF-8 encoding, the data is converted to UTF-8.
pub(crate) fn input_buffer_create_mem(
    buffer: *const c_char,
    size: c_int,
    enc: c_int,
) -> *mut _xmlParserInputBuffer {
    if buffer.is_null() || size <= 0 {
        return ptr::null_mut();
    }

    let buf = allocate_input_buffer();
    if buf.is_null() {
        return ptr::null_mut();
    }

    // Create the raw buffer containing the input data
    let raw_buf = buf_create(size);
    if raw_buf.is_null() {
        unsafe { xmlFreeImpl(buf as *mut c_void) };
        return ptr::null_mut();
    }

    // Copy data into the raw buffer
    buf_add(raw_buf, buffer as *const xmlChar, size);

    // Check if encoding conversion is needed
    let handler = find_handler_for_encoding(enc);
    if !handler.is_null() {
        // Encoding conversion needed
        // Create the output (UTF-8) buffer
        let out_buf = buf_create((size as c_uint).saturating_mul(3).max(MIN_BUFFER_SIZE) as c_int);
        if out_buf.is_null() {
            buf_free(raw_buf);
            unsafe { xmlFreeImpl(buf as *mut c_void) };
            return ptr::null_mut();
        }

        // Convert raw data to UTF-8
        let written = encoding::char_enc_in(handler, out_buf, raw_buf);
        if written < 0 {
            buf_free(raw_buf);
            buf_free(out_buf);
            unsafe { xmlFreeImpl(buf as *mut c_void) };
            return ptr::null_mut();
        }

        unsafe {
            (*buf).encoder = handler as *mut c_void;
            (*buf).buffer = out_buf as *mut c_void;
            (*buf).raw = raw_buf as *mut c_void;
        }
    } else {
        // No encoding conversion needed — data is (or will be treated as) UTF-8
        unsafe {
            (*buf).buffer = raw_buf as *mut c_void;
            (*buf).raw = raw_buf as *mut c_void;
        }
    }

    buf
}

// ── File I/O callbacks ──────────────────────────────────────────────────────

/// Read callback for file descriptor-based input.
#[allow(dead_code)]
unsafe extern "C" fn file_read_callback(
    context: *mut c_void,
    buffer: *mut c_char,
    len: c_int,
) -> c_int {
    if context.is_null() || buffer.is_null() || len <= 0 {
        return -1;
    }

    let fd = context as c_int;
    let ret = libc::read(fd, buffer as *mut c_void, len as usize);
    if ret < 0 {
        return -1;
    }
    ret as c_int
}

/// Close callback for file descriptor-based input.
#[allow(dead_code)]
unsafe extern "C" fn file_close_callback(context: *mut c_void) -> c_int {
    if context.is_null() {
        return -1;
    }

    let fd = context as c_int;
    libc::close(fd)
}

/// Create an input buffer from a file.
///
/// Opens the file, reads its contents into memory, and creates a memory-based
/// input buffer. The file is closed after reading.
pub(crate) fn input_buffer_create_file(
    filename: *const c_char,
    enc: c_int,
) -> *mut _xmlParserInputBuffer {
    if filename.is_null() {
        return ptr::null_mut();
    }

    // Get filename as a Rust string
    let filename_str = unsafe {
        match CStr::from_ptr(filename).to_str() {
            Ok(s) => s,
            Err(_) => return ptr::null_mut(),
        }
    };

    // Open the file
    let fd = unsafe {
        let path_c = std::ffi::CString::new(filename_str).unwrap_or_default();
        libc::open(path_c.as_ptr(), libc::O_RDONLY)
    };

    if fd < 0 {
        return ptr::null_mut();
    }

    // Stat the file to get its size
    let mut stat_buf: libc::stat = unsafe { std::mem::zeroed() };
    let stat_ret = unsafe {
        let path_c = std::ffi::CString::new(filename_str).unwrap_or_default();
        libc::stat(path_c.as_ptr(), &mut stat_buf)
    };

    let file_size = if stat_ret == 0 {
        stat_buf.st_size as usize
    } else {
        // Fall back to reading in chunks
        0
    };

    // Read the file contents
    let read_size = if file_size > 0 {
        file_size
    } else {
        4096 // Default chunk
    };

    let mut data = vec![0u8; read_size];
    let mut total_read: isize = 0;

    loop {
        let remaining = read_size.saturating_sub(total_read as usize);
        if remaining == 0 {
            // Grow buffer
            let new_size = read_size.saturating_mul(2);
            data.resize(new_size, 0u8);
        }

        let ret = unsafe {
            libc::read(
                fd,
                data.as_mut_ptr().add(total_read as usize) as *mut c_void,
                remaining,
            )
        };

        if ret < 0 {
            // Error
            unsafe { libc::close(fd) };
            return ptr::null_mut();
        }

        if ret == 0 {
            // EOF
            break;
        }

        total_read += ret as isize;
    }

    unsafe { libc::close(fd) };

    data.truncate(total_read as usize);

    if data.is_empty() {
        return ptr::null_mut();
    }

    // Create a memory-based input buffer from the data
    input_buffer_create_mem(data.as_ptr() as *const c_char, data.len() as c_int, enc)
}

/// Create an input buffer from I/O callbacks.
///
/// The `ioread` callback is called to fill the raw buffer.
/// The `ioclose` callback is called when the buffer is freed.
#[allow(dead_code)]
pub(crate) fn input_buffer_create_io(
    ioread: Option<xmlInputReadCallback>,
    ioclose: Option<xmlInputCloseCallback>,
    ioctx: *mut c_void,
    enc: c_int,
) -> *mut _xmlParserInputBuffer {
    let buf = allocate_input_buffer();
    if buf.is_null() {
        return ptr::null_mut();
    }

    // Create the raw buffer (used for reading from callback)
    let raw_buf = buf_create(DEFAULT_BUFFER_SIZE as c_int);
    if raw_buf.is_null() {
        unsafe { xmlFreeImpl(buf as *mut c_void) };
        return ptr::null_mut();
    }

    unsafe {
        (*buf).context = ioctx;
        (*buf).readcallback = ioread;
        (*buf).closecallback = ioclose;
        (*buf).raw = raw_buf as *mut c_void;
    }

    // Set up encoder if needed
    let handler = find_handler_for_encoding(enc);
    if !handler.is_null() {
        let out_buf = buf_create(DEFAULT_BUFFER_SIZE as c_int);
        if out_buf.is_null() {
            buf_free(raw_buf);
            unsafe { xmlFreeImpl(buf as *mut c_void) };
            return ptr::null_mut();
        }
        unsafe {
            (*buf).encoder = handler as *mut c_void;
            (*buf).buffer = out_buf as *mut c_void;
        }
    } else {
        unsafe {
            (*buf).buffer = raw_buf as *mut c_void;
        }
    }

    buf
}

/// Create an input buffer from a file descriptor.
///
/// The buffer uses read/close callbacks that wrap `libc::read` and `libc::close`.
#[allow(dead_code)]
pub(crate) fn input_buffer_create_fd(fd: c_int, enc: c_int) -> *mut _xmlParserInputBuffer {
    if fd < 0 {
        return ptr::null_mut();
    }

    input_buffer_create_io(
        Some(file_read_callback as xmlInputReadCallback),
        Some(file_close_callback as xmlInputCloseCallback),
        fd as *mut c_void,
        enc,
    )
}

/// Free an input buffer.
///
/// Calls the close callback if one is set, frees all internal buffers,
/// then frees the input buffer struct itself.
pub(crate) fn input_buffer_free(buf: *mut _xmlParserInputBuffer) {
    if buf.is_null() {
        return;
    }

    unsafe {
        // Call the close callback if one is set
        if let Some(close_cb) = (*buf).closecallback {
            close_cb((*buf).context);
        }

        // Free the raw buffer
        if !(*buf).raw.is_null() {
            buf_free((*buf).raw as *mut _xmlBuffer);
        }

        // Free the (converted) buffer if different from raw
        if !(*buf).buffer.is_null() && (*buf).buffer != (*buf).raw {
            buf_free((*buf).buffer as *mut _xmlBuffer);
        }

        // Note: the encoder is owned by the encoding module, not by us.
        // We do NOT free it here.

        xmlFreeImpl(buf as *mut c_void);
    }
}

/// Read from an input buffer.
///
/// If the buffer has a read callback, the callback is called to fill the raw
/// buffer, then the data is converted (if an encoder is set) and copied to
/// `buffer`. If no read callback is set (memory-based input), data is read
/// directly from the internal buffer.
///
/// Returns the number of bytes read, or -1 on error.
#[allow(dead_code)]
pub(crate) fn input_buffer_read(
    buf: *mut _xmlParserInputBuffer,
    buffer: *mut c_char,
    len: c_int,
) -> c_int {
    if buf.is_null() || buffer.is_null() || len <= 0 {
        return -1;
    }

    let b = unsafe { &mut *buf };

    if b.error != 0 {
        return -1;
    }

    if let Some(read_cb) = b.readcallback {
        // Callback-based input: read into raw buffer, then convert
        // Read a chunk
        let raw_buf = b.raw as *mut _xmlBuffer;
        if raw_buf.is_null() {
            return -1;
        }

        // Read up to `len` bytes into a temporary buffer
        let mut tmp = vec![0u8; len as usize];
        let ret = unsafe { read_cb(b.context, tmp.as_mut_ptr() as *mut c_char, len) };

        if ret < 0 {
            b.error = 1;
            return -1;
        }

        if ret == 0 {
            // EOF
            return 0;
        }

        // Add read data to raw buffer
        buf_add(raw_buf, tmp.as_ptr() as *const xmlChar, ret);

        // If encoder is set, convert raw -> buffer
        if !b.encoder.is_null() {
            let out_buf = b.buffer as *mut _xmlBuffer;
            if out_buf.is_null() {
                return -1;
            }

            let handler = b.encoder as *mut _xmlCharEncodingHandler;
            let conv_ret = encoding::char_enc_in(handler, out_buf, raw_buf);
            if conv_ret < 0 {
                b.error = 1;
                return -1;
            }

            // Read from the converted buffer
            let out_b = unsafe { &*out_buf };
            let to_copy = (out_b.use_ as c_int).min(len);
            if to_copy > 0 {
                unsafe {
                    ptr::copy_nonoverlapping(
                        out_b.content,
                        buffer as *mut xmlChar,
                        to_copy as usize,
                    );
                }
                // Remove the copied bytes from the output buffer
                buf_shrink(out_buf, to_copy as c_uint);
            }
            return to_copy;
        }

        // No encoder: read from raw buffer directly
        let raw_b = unsafe { &*raw_buf };
        let to_copy = (raw_b.use_ as c_int).min(len);
        if to_copy > 0 {
            unsafe {
                ptr::copy_nonoverlapping(raw_b.content, buffer as *mut xmlChar, to_copy as usize);
            }
            buf_shrink(raw_buf, to_copy as c_uint);
        }
        return to_copy;
    }

    // Memory-based input: read directly from the buffer
    let src_buf = b.buffer as *mut _xmlBuffer;
    if src_buf.is_null() {
        return -1;
    }

    let src = unsafe { &mut *src_buf };
    if src.content.is_null() || src.use_ == 0 {
        return 0;
    }

    let to_copy = (src.use_ as c_int).min(len);
    if to_copy > 0 {
        unsafe {
            ptr::copy_nonoverlapping(src.content, buffer as *mut xmlChar, to_copy as usize);
        }
        // Advance the content pointer and reduce use_
        unsafe {
            src.content = src.content.add(to_copy as usize);
        }
        src.use_ = src.use_.saturating_sub(to_copy as c_uint);
    }

    to_copy
}

/// Push data into an input buffer (for push parser).
///
/// The data is appended to the raw buffer and, if an encoder is set,
/// converted to UTF-8 in the buffer.
pub(crate) fn input_buffer_push(
    buf: *mut _xmlParserInputBuffer,
    buffer: *const c_char,
    len: c_int,
) -> c_int {
    if buf.is_null() || buffer.is_null() || len <= 0 {
        return -1;
    }

    let b = unsafe { &mut *buf };

    if b.error != 0 {
        return -1;
    }

    // Append to raw buffer
    let raw_buf = b.raw as *mut _xmlBuffer;
    if raw_buf.is_null() {
        return -1;
    }

    buf_add(raw_buf, buffer as *const xmlChar, len);

    // If encoder is set, convert raw -> buffer
    if !b.encoder.is_null() {
        let out_buf = b.buffer as *mut _xmlBuffer;
        if out_buf.is_null() {
            return -1;
        }

        let handler = b.encoder as *mut _xmlCharEncodingHandler;
        let ret = encoding::char_enc_in(handler, out_buf, raw_buf);
        if ret < 0 {
            b.error = 1;
            return -1;
        }
    }

    len
}

/// Update input buffer encoding.
///
/// Sets the encoder for an input buffer. The handler must already be
/// properly initialized.
pub(crate) fn input_buffer_set_encoder(
    buf: *mut _xmlParserInputBuffer,
    handler: *mut _xmlCharEncodingHandler,
) {
    if buf.is_null() {
        return;
    }

    unsafe {
        (*buf).encoder = handler as *mut c_void;
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 4. Output buffer operations
// ═══════════════════════════════════════════════════════════════════════════════

/// Write callback for file descriptor-based output.
unsafe extern "C" fn file_write_callback(
    context: *mut c_void,
    buffer: *const c_char,
    len: c_int,
) -> c_int {
    if context.is_null() || buffer.is_null() || len <= 0 {
        return -1;
    }

    let fd = context as c_int;
    let ret = libc::write(fd, buffer as *const c_void, len as usize);
    if ret < 0 {
        return -1;
    }
    ret as c_int
}

/// Close callback for file descriptor-based output.
unsafe extern "C" fn file_close_output_callback(context: *mut c_void) -> c_int {
    if context.is_null() {
        return -1;
    }

    let fd = context as c_int;
    libc::close(fd)
}

/// Write callback for buffer-based output (writes into an xmlBuffer).
unsafe extern "C" fn buffer_write_callback(
    context: *mut c_void,
    buffer: *const c_char,
    len: c_int,
) -> c_int {
    if context.is_null() || buffer.is_null() || len <= 0 {
        return -1;
    }

    let target_buf = context as *mut _xmlBuffer;
    buf_add(target_buf, buffer as *const xmlChar, len)
}

/// Internal helper: create an _xmlOutputBuffer struct.
///
/// Allocates the struct and initializes all fields to zero/NULL.
fn allocate_output_buffer() -> *mut _xmlOutputBuffer {
    let buf = unsafe { xmlMallocImpl(size_of::<_xmlOutputBuffer>()) as *mut _xmlOutputBuffer };
    if buf.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        ptr::write(
            buf,
            _xmlOutputBuffer {
                context: ptr::null_mut(),
                writecallback: None,
                closecallback: None,
                encoder: ptr::null_mut(),
                buffer: ptr::null_mut(),
                conv: ptr::null_mut(),
                written: 0,
                error: 0,
            },
        );
    }

    buf
}

/// Create an output buffer for a filename.
///
/// Opens the file for writing and sets up write/close callbacks.
/// If `compression` is nonzero, future versions may support compression.
pub(crate) fn output_buffer_create_filename(
    URI: *const c_char,
    encoder: *mut _xmlCharEncodingHandler,
    _compression: c_int,
) -> *mut _xmlOutputBuffer {
    if URI.is_null() {
        return ptr::null_mut();
    }

    let path_str = unsafe {
        match CStr::from_ptr(URI).to_str() {
            Ok(s) => s,
            Err(_) => return ptr::null_mut(),
        }
    };

    let path_c = std::ffi::CString::new(path_str).unwrap_or_default();

    // Open file for writing (create/truncate)
    let fd = unsafe {
        libc::open(
            path_c.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC,
            0o644,
        )
    };

    if fd < 0 {
        return ptr::null_mut();
    }

    let obuf = allocate_output_buffer();
    if obuf.is_null() {
        unsafe { libc::close(fd) };
        return ptr::null_mut();
    }

    let buf = buf_create(DEFAULT_BUFFER_SIZE as c_int);
    if buf.is_null() {
        unsafe {
            libc::close(fd);
            xmlFreeImpl(obuf as *mut c_void);
        }
        return ptr::null_mut();
    }

    let conv_buf = buf_create(DEFAULT_BUFFER_SIZE as c_int);
    if conv_buf.is_null() {
        unsafe {
            libc::close(fd);
            buf_free(buf);
            xmlFreeImpl(obuf as *mut c_void);
        }
        return ptr::null_mut();
    }

    unsafe {
        (*obuf).context = fd as *mut c_void;
        (*obuf).writecallback = Some(file_write_callback as xmlOutputWriteCallback);
        (*obuf).closecallback = Some(file_close_output_callback as xmlOutputCloseCallback);
        (*obuf).encoder = encoder as *mut c_void;
        (*obuf).buffer = buf as *mut c_void;
        (*obuf).conv = conv_buf as *mut c_void;
        (*obuf).written = 0;
        (*obuf).error = 0;
    }

    obuf
}

/// Create an output buffer for a file descriptor.
pub(crate) fn output_buffer_create_fd(
    fd: c_int,
    encoder: *mut _xmlCharEncodingHandler,
) -> *mut _xmlOutputBuffer {
    if fd < 0 {
        return ptr::null_mut();
    }

    let obuf = allocate_output_buffer();
    if obuf.is_null() {
        return ptr::null_mut();
    }

    let buf = buf_create(DEFAULT_BUFFER_SIZE as c_int);
    if buf.is_null() {
        unsafe { xmlFreeImpl(obuf as *mut c_void) };
        return ptr::null_mut();
    }

    let conv_buf = buf_create(DEFAULT_BUFFER_SIZE as c_int);
    if conv_buf.is_null() {
        unsafe {
            buf_free(buf);
            xmlFreeImpl(obuf as *mut c_void);
        }
        return ptr::null_mut();
    }

    unsafe {
        (*obuf).context = fd as *mut c_void;
        (*obuf).writecallback = Some(file_write_callback as xmlOutputWriteCallback);
        (*obuf).closecallback = Some(file_close_output_callback as xmlOutputCloseCallback);
        (*obuf).encoder = encoder as *mut c_void;
        (*obuf).buffer = buf as *mut c_void;
        (*obuf).conv = conv_buf as *mut c_void;
        (*obuf).written = 0;
        (*obuf).error = 0;
    }

    obuf
}

/// Create an output buffer from I/O callbacks.
pub(crate) fn output_buffer_create_io(
    iowrite: Option<xmlOutputWriteCallback>,
    ioclose: Option<xmlOutputCloseCallback>,
    ioctx: *mut c_void,
    encoder: *mut _xmlCharEncodingHandler,
) -> *mut _xmlOutputBuffer {
    let obuf = allocate_output_buffer();
    if obuf.is_null() {
        return ptr::null_mut();
    }

    let buf = buf_create(DEFAULT_BUFFER_SIZE as c_int);
    if buf.is_null() {
        unsafe { xmlFreeImpl(obuf as *mut c_void) };
        return ptr::null_mut();
    }

    let conv_buf = buf_create(DEFAULT_BUFFER_SIZE as c_int);
    if conv_buf.is_null() {
        unsafe {
            buf_free(buf);
            xmlFreeImpl(obuf as *mut c_void);
        }
        return ptr::null_mut();
    }

    unsafe {
        (*obuf).context = ioctx;
        (*obuf).writecallback = iowrite;
        (*obuf).closecallback = ioclose;
        (*obuf).encoder = encoder as *mut c_void;
        (*obuf).buffer = buf as *mut c_void;
        (*obuf).conv = conv_buf as *mut c_void;
        (*obuf).written = 0;
        (*obuf).error = 0;
    }

    obuf
}

/// Create an output buffer from a pre-existing xmlBuffer.
///
/// Writes to the output buffer will be appended to the given `_xmlBuffer`.
pub(crate) fn output_buffer_create_buffer(
    target_buf: *mut _xmlBuffer,
    encoder: *mut _xmlCharEncodingHandler,
) -> *mut _xmlOutputBuffer {
    if target_buf.is_null() {
        return ptr::null_mut();
    }

    let obuf = allocate_output_buffer();
    if obuf.is_null() {
        return ptr::null_mut();
    }

    // Internal buffer for buffering writes before flush
    let internal_buf = buf_create(DEFAULT_BUFFER_SIZE as c_int);
    if internal_buf.is_null() {
        unsafe { xmlFreeImpl(obuf as *mut c_void) };
        return ptr::null_mut();
    }

    // Conversion buffer (used when encoder is present)
    let conv_buf = buf_create(DEFAULT_BUFFER_SIZE as c_int);
    if conv_buf.is_null() {
        unsafe {
            buf_free(internal_buf);
            xmlFreeImpl(obuf as *mut c_void);
        }
        return ptr::null_mut();
    }

    unsafe {
        (*obuf).context = target_buf as *mut c_void;
        (*obuf).writecallback = Some(buffer_write_callback as xmlOutputWriteCallback);
        (*obuf).closecallback = None;
        (*obuf).encoder = encoder as *mut c_void;
        (*obuf).buffer = internal_buf as *mut c_void;
        (*obuf).conv = conv_buf as *mut c_void;
        (*obuf).written = 0;
        (*obuf).error = 0;
    }

    obuf
}

/// Flush an output buffer.
///
/// Encodes the buffered data (if an encoder is set) and writes it via the
/// write callback. Resets the internal buffer after writing.
///
/// Returns the number of bytes written, or -1 on error.
pub(crate) fn output_buffer_flush(out: *mut _xmlOutputBuffer) -> c_int {
    if out.is_null() {
        return -1;
    }

    let ob = unsafe { &mut *out };

    if ob.error != 0 {
        return -1;
    }

    let buf = ob.buffer as *mut _xmlBuffer;
    if buf.is_null() {
        return 0;
    }

    let b = unsafe { &*buf };
    if b.use_ == 0 {
        return 0;
    }

    let write_cb = match ob.writecallback {
        Some(cb) => cb,
        None => {
            // No write callback — just clear the buffer
            buf_empty(buf);
            return 0;
        }
    };

    let total_written = if !ob.encoder.is_null() {
        // Convert buffer content via encoder
        let handler = ob.encoder as *mut _xmlCharEncodingHandler;
        let conv = ob.conv as *mut _xmlBuffer;

        // Ensure conv buffer is empty before converting
        buf_empty(conv);

        let ret = encoding::char_enc_out(handler, conv, buf);
        if ret < 0 {
            ob.error = 1;
            return -1;
        }

        // Write converted data via callback
        let conv_b = unsafe { &*conv };
        if conv_b.use_ > 0 {
            let written = unsafe {
                write_cb(
                    ob.context,
                    conv_b.content as *const c_char,
                    conv_b.use_ as c_int,
                )
            };

            if written < 0 {
                ob.error = 1;
                return -1;
            }

            ob.written = ob.written.saturating_add(written);
            buf_empty(conv);
            written
        } else {
            0
        }
    } else {
        // No encoder: write buffer content directly
        let written = unsafe { write_cb(ob.context, b.content as *const c_char, b.use_ as c_int) };

        if written < 0 {
            ob.error = 1;
            return -1;
        }

        ob.written = ob.written.saturating_add(written);
        written
    };

    // Clear the buffer after writing
    buf_empty(buf);

    total_written
}

/// Free an output buffer.
///
/// Flushes any pending data, calls the close callback if set,
/// frees all internal buffers, then frees the output buffer struct.
pub(crate) fn output_buffer_close(out: *mut _xmlOutputBuffer) -> c_int {
    if out.is_null() {
        return -1;
    }

    let ob = unsafe { &mut *out };

    // Flush any pending data
    let flush_ret = output_buffer_flush(out);

    // Call the close callback
    if let Some(close_cb) = ob.closecallback {
        unsafe {
            close_cb(ob.context);
        }
    }

    // Free internal buffers
    if !ob.buffer.is_null() {
        buf_free(ob.buffer as *mut _xmlBuffer);
    }
    if !ob.conv.is_null() {
        buf_free(ob.conv as *mut _xmlBuffer);
    }

    // Note: encoder is owned by the caller/encoding module, not by us

    unsafe { xmlFreeImpl(out as *mut c_void) };

    flush_ret
}

/// Write to an output buffer.
///
/// The data is appended to the internal buffer; when a write callback is
/// installed and the buffered size reaches the upstream threshold
/// (`MINLEN` = 256), the buffered data is pushed through the callback.
///
/// # UPSTREAM-PARITY
///
/// Upstream 2.15 `xmlOutputBufferWrite` returns the number of bytes written
/// to the I/O channel **in this call** — 0 when the data merely landed in
/// the internal buffer (observable on the system DSO; verified by the
/// SAVE-001 differential court). With no write callback, `len` is returned.
///
/// Returns the bytes written through the callback (possibly 0), or -1 on error.
pub(crate) fn output_buffer_write(
    out: *mut _xmlOutputBuffer,
    len: c_int,
    data: *const c_char,
) -> c_int {
    if out.is_null() || data.is_null() || len <= 0 {
        return -1;
    }

    let ob = unsafe { &mut *out };

    if ob.error != 0 {
        return -1;
    }

    let buf = ob.buffer as *mut _xmlBuffer;
    if buf.is_null() {
        return -1;
    }

    let ret = buf_add(buf, data as *const xmlChar, len);
    if ret < 0 {
        ob.error = 1;
        return -1;
    }

    if ob.writecallback.is_none() {
        return len; // no I/O channel: upstream returns len
    }

    // Push buffered data through the callback once it reaches the upstream
    // MINLEN threshold; otherwise the write is only buffered and upstream
    // reports 0 bytes written in this call.
    let b = unsafe { &*buf };
    if b.use_ < MIN_BUFFER_SIZE {
        return 0;
    }
    output_buffer_flush(out)
}

/// Write a null-terminated string to an output buffer.
pub(crate) fn output_buffer_write_string(out: *mut _xmlOutputBuffer, str: *const c_char) -> c_int {
    if out.is_null() || str.is_null() {
        return -1;
    }

    let len = unsafe {
        let mut i: c_int = 0;
        while *str.add(i as usize) != 0 {
            i += 1;
        }
        i
    };

    output_buffer_write(out, len, str)
}

/// Write a single character to an output buffer.
pub(crate) fn output_buffer_write_char(out: *mut _xmlOutputBuffer, c: c_char) -> c_int {
    output_buffer_write(out, 1, &c as *const c_char)
}

/// Get the content of an output buffer's internal buffer.
///
/// Returns a pointer to the internal buffer's content, or NULL on error.
pub(crate) fn output_buffer_get_content(out: *mut _xmlOutputBuffer) -> *const xmlChar {
    if out.is_null() {
        return ptr::null();
    }

    let ob = unsafe { &*out };
    let buf = ob.buffer as *mut _xmlBuffer;
    if buf.is_null() {
        return ptr::null();
    }

    buf_content(buf)
}

/// Get the number of bytes currently buffered (upstream xmlOutputBufferGetSize).
pub(crate) fn output_buffer_get_size(out: *mut _xmlOutputBuffer) -> c_int {
    if out.is_null() {
        return -1;
    }
    let ob = unsafe { &*out };
    let buf = ob.buffer as *mut _xmlBuffer;
    if buf.is_null() {
        return -1;
    }
    buf_length(buf)
}

/// Allocate an output buffer with no I/O target (upstream xmlAllocOutputBuffer):
/// a fresh internal buffer and no write/close callbacks.
pub(crate) fn output_buffer_create(
    _encoder: *mut crate::abi::structs::_xmlCharEncodingHandler,
) -> *mut _xmlOutputBuffer {
    let obuf = allocate_output_buffer();
    if obuf.is_null() {
        return ptr::null_mut();
    }
    let buf = buf_create(-1);
    if buf.is_null() {
        unsafe { xmlFreeImpl(obuf as *mut c_void) };
        return ptr::null_mut();
    }
    unsafe {
        (*obuf).buffer = buf as *mut c_void;
        (*obuf).encoder = _encoder as *mut c_void;
    }
    obuf
}

/// Create an output buffer writing to a `FILE *` (upstream
/// xmlOutputBufferCreateFile): the FILE becomes the I/O context, writes go
/// through `fwrite`, close goes through `fflush` (upstream xmlFileWrite /
/// xmlFileFlush).
///
/// # SAFETY
///
/// - `file` must be a valid `FILE *` or NULL.
pub(crate) fn output_buffer_create_file(
    file: *mut libc::FILE,
    _encoder: *mut crate::abi::structs::_xmlCharEncodingHandler,
) -> *mut _xmlOutputBuffer {
    if file.is_null() {
        return ptr::null_mut();
    }
    unsafe extern "C" fn file_write(ctx: *mut c_void, buffer: *const c_char, len: c_int) -> c_int {
        let f = ctx as *mut libc::FILE;
        if f.is_null() || buffer.is_null() || len <= 0 {
            return 0;
        }
        let n = unsafe { libc::fwrite(buffer as *const libc::c_void, 1, len as usize, f) };
        n as c_int
    }
    unsafe extern "C" fn file_flush(ctx: *mut c_void) -> c_int {
        let f = ctx as *mut libc::FILE;
        if f.is_null() {
            return -1;
        }
        unsafe { libc::fflush(f) }
    }
    let obuf = output_buffer_create(_encoder);
    if obuf.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        (*obuf).context = file as *mut c_void;
        (*obuf).writecallback = Some(file_write);
        (*obuf).closecallback = Some(file_flush);
    }
    obuf
}

/// Write to an output buffer, applying an escape callback to the string
/// (upstream xmlOutputBufferWriteEscape).
///
/// # UPSTREAM-PARITY
///
/// With a NULL escape callback upstream runs the string through
/// `xmlEscapeText(str, 0)` (xmlIO.c 2.15, codegen/escape.inc) and then
/// `xmlOutputBufferWrite` — so `&`/`<`/`>` and CR become entities, tab/LF
/// and quotes are left verbatim, and the return value follows the write
/// path (0 while data is only buffered).
///
/// Returns the bytes written through the callback, or -1 on error.
pub(crate) fn output_buffer_write_escape(
    out: *mut _xmlOutputBuffer,
    str: *const xmlChar,
    escaping: Option<unsafe extern "C" fn(*mut u8, *mut c_int, *const u8, *mut c_int) -> c_int>,
) -> c_int {
    if out.is_null() || str.is_null() {
        return -1;
    }
    if escaping.is_none() {
        // Upstream xmlEscapeText(str, 0): only & < > and CR are escaped.
        // SAFETY: escape_text reads `str` and allocates a fresh copy.
        let escaped = unsafe { escape_text(str) };
        if escaped.is_null() {
            unsafe {
                let ob = &mut *out;
                ob.error = 1;
            }
            return -1;
        }
        let len = unsafe { libc::strlen(escaped as *const libc::c_char) as c_int };
        let ret = output_buffer_write(out, len, escaped as *const c_char);
        unsafe { libc::free(escaped as *mut libc::c_void) };
        return ret;
    }
    let ob = unsafe { &mut *out };
    if ob.error != 0 {
        return -1;
    }
    let buf = ob.buffer as *mut _xmlBuffer;
    if buf.is_null() {
        return -1;
    }
    let inlen = unsafe { libc::strlen(str as *const libc::c_char) as c_int };
    let mut inpos = 0i32;
    let mut total = 0i32;
    while inpos < inlen {
        let mut outbuf = [0u8; 1024];
        let mut outlen = 1024i32;
        let chunk_in = unsafe { str.add(inpos as usize) };
        let mut chunk_len = inlen - inpos;
        // SAFETY: escaping is a valid callback; buffers are valid for the call.
        let ret = unsafe {
            escaping.unwrap()(outbuf.as_mut_ptr(), &mut outlen, chunk_in, &mut chunk_len)
        };
        if ret < 0 || outlen < 0 {
            ob.error = 1;
            return -1;
        }
        if outlen > 0 {
            let r = output_buffer_write(out, outlen, outbuf.as_ptr() as *const c_char);
            if r < 0 {
                ob.error = 1;
                return -1;
            }
            total += r;
        }
        if chunk_len <= 0 {
            break; // escape consumed nothing: avoid an infinite loop
        }
        inpos += chunk_len;
        if inpos > inlen {
            break;
        }
    }
    total
}

/// Upstream `xmlEscapeText(str, 0)` (xmlIO.c 2.15, codegen/escape.inc):
/// escapes `&`/`<`/`>` and CR; tab/LF/quotes pass through; multi-byte UTF-8
/// is copied verbatim (no XML_ESCAPE_NON_ASCII flag). Returns a
/// heap-allocated NUL-terminated string (caller frees).
unsafe fn escape_text(str: *const xmlChar) -> *mut xmlChar {
    if str.is_null() {
        return core::ptr::null_mut();
    }
    let mut out = Vec::<u8>::with_capacity(64);
    let mut cur = str;
    loop {
        let c = unsafe { *cur };
        if c == 0 {
            break;
        }
        match c {
            b'&' => out.extend_from_slice(b"&amp;"),
            b'<' => out.extend_from_slice(b"&lt;"),
            b'>' => out.extend_from_slice(b"&gt;"),
            0x0d => out.extend_from_slice(b"&#13;"),
            _ => {
                // Copy a whole UTF-8 sequence verbatim (upstream copies
                // bytes until the next escapable char).
                let len = utf8_seq_len(c);
                for _ in 0..len {
                    let b = unsafe { *cur };
                    if b == 0 {
                        break;
                    }
                    out.push(b);
                    cur = cur.add(1);
                }
                continue;
            }
        }
        cur = cur.add(1);
    }
    out.push(0);
    let p = libc::malloc(out.len()) as *mut xmlChar;
    if p.is_null() {
        return core::ptr::null_mut();
    }
    unsafe {
        libc::memcpy(
            p as *mut libc::c_void,
            out.as_ptr() as *const libc::c_void,
            out.len(),
        );
    }
    p
}

/// Byte length of the UTF-8 sequence starting with `lead` (1 when invalid).
const fn utf8_seq_len(lead: u8) -> usize {
    match lead {
        0x00..=0x7f => 1,
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => 1,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 5. I/O helper functions
// ═══════════════════════════════════════════════════════════════════════════════

/// Check if a file exists.
///
/// Returns 1 if the file exists, 0 if not, -1 on error.
#[allow(dead_code)]
pub(crate) fn check_file_exists(filename: *const c_char) -> c_int {
    if filename.is_null() {
        return -1;
    }

    let path_str = unsafe {
        match CStr::from_ptr(filename).to_str() {
            Ok(s) => s,
            Err(_) => return -1,
        }
    };

    let path_c = match std::ffi::CString::new(path_str) {
        Ok(c) => c,
        Err(_) => return -1,
    };

    let mut stat_buf: libc::stat = unsafe { std::mem::zeroed() };
    let ret = unsafe { libc::stat(path_c.as_ptr(), &mut stat_buf) };

    if ret == 0 {
        1
    } else {
        0
    }
}

/// Read a file into memory.
///
/// Reads the entire file contents into a newly allocated buffer.
/// Returns a pointer to the buffer, or NULL on failure.
/// The size of the buffer is stored in `size` if it's non-NULL.
///
/// The returned buffer must be freed with `xmlFree`.
#[allow(dead_code)]
pub(crate) fn read_file_to_memory(filename: *const c_char, size: *mut c_int) -> *mut c_char {
    if filename.is_null() {
        return ptr::null_mut();
    }

    let path_str = unsafe {
        match CStr::from_ptr(filename).to_str() {
            Ok(s) => s,
            Err(_) => return ptr::null_mut(),
        }
    };

    let path_c = match std::ffi::CString::new(path_str) {
        Ok(c) => c,
        Err(_) => return ptr::null_mut(),
    };

    let fd = unsafe { libc::open(path_c.as_ptr(), libc::O_RDONLY) };
    if fd < 0 {
        return ptr::null_mut();
    }

    // Stat to get file size
    let mut stat_buf: libc::stat = unsafe { std::mem::zeroed() };
    let file_size = if unsafe { libc::stat(path_c.as_ptr(), &mut stat_buf) } == 0 {
        stat_buf.st_size as usize
    } else {
        0
    };

    // Read in chunks
    let chunk_size = 4096usize;
    let initial_capacity = if file_size > 0 { file_size } else { chunk_size };

    let mut data = Vec::with_capacity(initial_capacity);
    let mut buf = vec![0u8; chunk_size];

    loop {
        let ret = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut c_void, chunk_size) };

        if ret < 0 {
            unsafe { libc::close(fd) };
            return ptr::null_mut();
        }

        if ret == 0 {
            break; // EOF
        }

        data.extend_from_slice(&buf[..ret as usize]);
    }

    unsafe { libc::close(fd) };

    if data.is_empty() {
        return ptr::null_mut();
    }

    // Allocate via xmlMalloc and copy
    let result = unsafe { xmlMallocImpl(data.len()) as *mut c_char };
    if result.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        ptr::copy_nonoverlapping(data.as_ptr(), result as *mut u8, data.len());
    }

    if !size.is_null() {
        unsafe {
            *size = data.len() as c_int;
        }
    }

    result
}

/// Write memory to a file.
///
/// Creates or truncates the file and writes `size` bytes from `data`.
/// Returns 0 on success, -1 on error.
#[allow(dead_code)]
pub(crate) fn write_memory_to_file(
    filename: *const c_char,
    data: *const c_char,
    size: c_int,
) -> c_int {
    if filename.is_null() || data.is_null() || size <= 0 {
        return -1;
    }

    let path_str = unsafe {
        match CStr::from_ptr(filename).to_str() {
            Ok(s) => s,
            Err(_) => return -1,
        }
    };

    let path_c = match std::ffi::CString::new(path_str) {
        Ok(c) => c,
        Err(_) => return -1,
    };

    let fd = unsafe {
        libc::open(
            path_c.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC,
            0o644,
        )
    };

    if fd < 0 {
        return -1;
    }

    let mut remaining = size as usize;
    let mut offset: usize = 0;

    while remaining > 0 {
        let ret = unsafe {
            libc::write(
                fd,
                (data as *const u8).add(offset) as *const c_void,
                remaining,
            )
        };

        if ret < 0 {
            unsafe { libc::close(fd) };
            return -1;
        }

        let written = ret as usize;
        remaining -= written;
        offset += written;
    }

    unsafe { libc::close(fd) };
    0
}

/// Get the current working directory.
///
/// Returns a newly allocated null-terminated string, or NULL on failure.
/// The returned pointer must be freed with `xmlFree`.
#[allow(dead_code)]
pub(crate) fn get_cwd() -> *mut c_char {
    // Use a reasonable initial buffer size
    let mut size: usize = 1024;

    loop {
        let buf = unsafe { xmlMallocImpl(size) as *mut c_char };
        if buf.is_null() {
            return ptr::null_mut();
        }

        let ret = unsafe { libc::getcwd(buf as *mut c_char, size) };
        if !ret.is_null() {
            return buf;
        }

        unsafe { xmlFreeImpl(buf as *mut c_void) };

        // Check if the error was ERANGE (buffer too small)
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::ERANGE) {
            size = size.saturating_mul(2);
            if size > 65536 {
                return ptr::null_mut(); // Sanity cap
            }
        } else {
            return ptr::null_mut();
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;
    use std::os::raw::c_char;

    // ── Helpers ────────────────────────────────────────────────────────────

    fn c(s: &str) -> CString {
        CString::new(s).unwrap()
    }

    /// Create a CString from raw bytes (may contain non-ASCII).
    unsafe fn c_bytes(bytes: &[u8]) -> CString {
        CString::from_vec_unchecked(bytes.to_vec())
    }

    /// Interpret a &[u8] as &[i8] for comparison with c_char buffers.
    fn i8_slice(s: &[u8]) -> &[i8] {
        unsafe { std::slice::from_raw_parts(s.as_ptr() as *const i8, s.len()) }
    }

    // ── xmlBuffer tests ────────────────────────────────────────────────────

    #[test]
    fn test_buf_create_free() {
        let buf = buf_create(100);
        assert!(!buf.is_null());

        let b = unsafe { &*buf };
        assert!(!b.content.is_null());
        assert_eq!(b.use_, 0);
        assert!(b.size >= 100);
        assert_eq!(b.alloc, XML_BUFFER_ALLOC_DOUBLEIT);

        // Content should be null-terminated empty string
        unsafe {
            assert_eq!(*b.content, 0);
        }

        buf_free(buf);
    }

    #[test]
    fn test_buf_create_default_size() {
        let buf = buf_create(0);
        assert!(!buf.is_null());

        let b = unsafe { &*buf };
        assert!(b.size >= MIN_BUFFER_SIZE);

        buf_free(buf);
    }

    #[test]
    fn test_buf_create_static() {
        let s: &[u8] = b"hello\0";
        let buf = buf_create_static(s.as_ptr() as *const xmlChar, 5);
        assert!(!buf.is_null());

        let b = unsafe { &*buf };
        assert_eq!(b.use_, 5);
        assert_eq!(b.alloc, XML_BUFFER_ALLOC_IMMUTABLE);

        // Content should point to the original string
        unsafe {
            assert_eq!(*b.content.offset(0), b'h');
            assert_eq!(*b.content.offset(4), b'o');
            assert_eq!(*b.content.offset(5), 0);
        }

        buf_free(buf); // Should not free the static content
    }

    #[test]
    fn test_buf_add() {
        let buf = buf_create(10);
        assert!(!buf.is_null());

        let s1: &[u8] = b"Hello\0";
        let ret = buf_add(buf, s1.as_ptr() as *const xmlChar, 5);
        assert_eq!(ret, 5);

        let b = unsafe { &*buf };
        assert_eq!(b.use_, 5);
        unsafe {
            assert_eq!(*b.content.offset(0), b'H');
            assert_eq!(*b.content.offset(4), b'o');
            assert_eq!(*b.content.offset(5), 0); // null-terminated
        }

        // Add more to trigger growth
        let s2: &[u8] = b" World!\0";
        let ret = buf_add(buf, s2.as_ptr() as *const xmlChar, 7);
        assert_eq!(ret, 7);

        let b = unsafe { &*buf };
        assert_eq!(b.use_, 12);
        unsafe {
            assert_eq!(*b.content.offset(6), b'W');
            assert_eq!(*b.content.offset(11), b'!');
            assert_eq!(*b.content.offset(12), 0);
        }

        buf_free(buf);
    }

    #[test]
    fn test_buf_add_null() {
        let buf = buf_create(10);
        let ret = buf_add(buf, ptr::null(), 5);
        assert_eq!(ret, 0);
        buf_free(buf);
    }

    #[test]
    fn test_buf_cat() {
        let buf = buf_create(10);
        let s: &[u8] = b"Hello\0";
        let ret = buf_cat(buf, s.as_ptr() as *const xmlChar);
        assert_eq!(ret, 5);

        let b = unsafe { &*buf };
        assert_eq!(b.use_, 5);

        buf_free(buf);
    }

    #[test]
    fn test_buf_ccat() {
        let buf = buf_create(10);
        let ret = buf_ccat(buf, b'A' as xmlChar);
        assert_eq!(ret, 1);

        let b = unsafe { &*buf };
        assert_eq!(b.use_, 1);
        unsafe {
            assert_eq!(*b.content, b'A');
        }

        buf_free(buf);
    }

    #[test]
    fn test_buf_empty() {
        let buf = buf_create(10);
        let s: &[u8] = b"Hello\0";
        buf_add(buf, s.as_ptr() as *const xmlChar, 5);
        assert_eq!(unsafe { &*buf }.use_, 5);

        buf_empty(buf);
        let b = unsafe { &*buf };
        assert_eq!(b.use_, 0);
        unsafe {
            assert_eq!(*b.content, 0);
        }

        buf_free(buf);
    }

    #[test]
    fn test_buf_content() {
        let buf = buf_create(10);
        let content = buf_content(buf);
        assert!(!content.is_null());
        buf_free(buf);
    }

    #[test]
    fn test_buf_length() {
        let buf = buf_create(10);
        assert_eq!(buf_length(buf), 0);

        let s: &[u8] = b"Hi\0";
        buf_add(buf, s.as_ptr() as *const xmlChar, 2);
        assert_eq!(buf_length(buf), 2);

        buf_free(buf);
    }

    #[test]
    fn test_buf_shrink() {
        let buf = buf_create(10);
        let s: &[u8] = b"Hello World\0";
        buf_add(buf, s.as_ptr() as *const xmlChar, 11);
        assert_eq!(buf_length(buf), 11);

        buf_shrink(buf, 5);
        assert_eq!(buf_length(buf), 6);

        let b = unsafe { &*buf };
        unsafe {
            assert_eq!(*b.content.offset(6), 0); // null-terminated
        }

        // Shrink more than available
        buf_shrink(buf, 100);
        assert_eq!(buf_length(buf), 0);

        buf_free(buf);
    }

    #[test]
    fn test_buf_grow() {
        let buf = buf_create(10);
        assert!(unsafe { &*buf }.size >= 10);

        let ret = buf_grow(buf, 1000);
        assert_eq!(ret, 0);
        assert!(unsafe { &*buf }.size >= 1000);

        buf_free(buf);
    }

    #[test]
    fn test_buf_free_null() {
        buf_free(ptr::null_mut()); // Should not crash
    }

    // ── xmlBuf tests ───────────────────────────────────────────────────────

    #[test]
    fn test_xml_buf_create_free() {
        let buf = xml_buf_create(100);
        assert!(!buf.is_null());

        let b = unsafe { &*buf };
        assert!(!b.content.is_null());
        assert_eq!(b.use_, 0);
        assert!(b.size >= 100);
        assert_eq!(b.error, 0);
        assert_eq!(b.buffer, 0);
        assert_eq!(b.io, 0);

        xml_buf_free(buf);
    }

    #[test]
    fn test_xml_buf_add() {
        let buf = xml_buf_create(10);
        let s: &[u8] = b"Hello\0";
        let ret = xml_buf_add(buf, s.as_ptr() as *const xmlChar, 5);
        assert_eq!(ret, 5);

        let b = unsafe { &*buf };
        assert_eq!(b.use_, 5);

        xml_buf_free(buf);
    }

    #[test]
    fn test_xml_buf_cat() {
        let buf = xml_buf_create(10);
        let s: &[u8] = b"Hello\0";
        let ret = xml_buf_cat(buf, s.as_ptr() as *const xmlChar);
        assert_eq!(ret, 5);

        xml_buf_free(buf);
    }

    #[test]
    fn test_xml_buf_content() {
        let buf = xml_buf_create(10);
        let content = xml_buf_content(buf);
        assert!(!content.is_null());
        xml_buf_free(buf);
    }

    #[test]
    fn test_xml_buf_length() {
        let buf = xml_buf_create(10);
        assert_eq!(xml_buf_length(buf), 0);

        let s: &[u8] = b"Hi\0";
        xml_buf_add(buf, s.as_ptr() as *const xmlChar, 2);
        assert_eq!(xml_buf_length(buf), 2);

        xml_buf_free(buf);
    }

    #[test]
    fn test_xml_buf_grow() {
        let buf = xml_buf_create(10);
        let ret = xml_buf_grow(buf, 500);
        assert_eq!(ret, 0);
        assert!(unsafe { &*buf }.size >= 500);

        xml_buf_free(buf);
    }

    #[test]
    fn test_xml_buf_shrink() {
        let buf = xml_buf_create(10);
        let s: &[u8] = b"Hello\0";
        xml_buf_add(buf, s.as_ptr() as *const xmlChar, 5);
        assert_eq!(xml_buf_length(buf), 5);

        xml_buf_shrink(buf, 3);
        assert_eq!(xml_buf_length(buf), 2);

        xml_buf_free(buf);
    }

    // ── Input buffer tests ─────────────────────────────────────────────────

    #[test]
    fn test_input_buffer_create_mem() {
        let data = c("Hello XML");
        let buf = input_buffer_create_mem(data.as_ptr(), 9, 0); // NONE encoding
        assert!(!buf.is_null());

        let b = unsafe { &*buf };
        assert!(b.readcallback.is_none());
        assert!(!b.buffer.is_null());
        assert_eq!(b.error, 0);

        // Read back
        let mut out = [0i8; 16];
        let ret = input_buffer_read(buf, out.as_mut_ptr(), 16);
        assert_eq!(ret, 9);
        assert_eq!(&out[..9], i8_slice(b"Hello XML"));

        input_buffer_free(buf);
    }

    #[test]
    fn test_input_buffer_create_mem_empty() {
        let buf = input_buffer_create_mem(ptr::null(), 0, 0);
        assert!(buf.is_null());
    }

    #[test]
    fn test_input_buffer_read_partial() {
        let data = c("Hello XML World");
        let buf = input_buffer_create_mem(data.as_ptr(), 15, 0);
        assert!(!buf.is_null());

        // Read in two parts
        let mut out1 = [0i8; 5];
        let ret = input_buffer_read(buf, out1.as_mut_ptr(), 5);
        assert_eq!(ret, 5);
        assert_eq!(&out1[..5], i8_slice(b"Hello"));

        let mut out2 = [0i8; 10];
        let ret = input_buffer_read(buf, out2.as_mut_ptr(), 10);
        assert_eq!(ret, 10);
        assert_eq!(&out2[..10], i8_slice(b" XML World"));

        input_buffer_free(buf);
    }

    #[test]
    fn test_input_buffer_push() {
        let buf = input_buffer_create_io(None, None, ptr::null_mut(), 0);
        assert!(!buf.is_null());

        let data1 = c("<root>");
        let ret = input_buffer_push(buf, data1.as_ptr(), 6);
        assert_eq!(ret, 6);

        let data2 = c("</root>");
        let ret = input_buffer_push(buf, data2.as_ptr(), 7);
        assert_eq!(ret, 7);

        // Read back the pushed data
        let mut out = [0i8; 32];
        let ret = input_buffer_read(buf, out.as_mut_ptr(), 32);
        assert_eq!(ret, 13);
        assert_eq!(&out[..13], i8_slice(b"<root></root>"));

        input_buffer_free(buf);
    }

    #[test]
    fn test_input_buffer_set_encoder() {
        let _buf = input_buffer_create_mem(ptr::null(), 0, 0);
        // Create a fresh buffer
        let data = c("test");
        let buf = input_buffer_create_mem(data.as_ptr(), 4, 0);
        assert!(!buf.is_null());

        // Set encoder to null (no encoding)
        input_buffer_set_encoder(buf, ptr::null_mut());
        let b = unsafe { &*buf };
        assert!(b.encoder.is_null());

        input_buffer_free(buf);
    }

    // ── Output buffer tests ────────────────────────────────────────────────

    #[test]
    fn test_output_buffer_create_buffer() {
        let internal_buf = buf_create(100);
        assert!(!internal_buf.is_null());

        let obuf = output_buffer_create_buffer(internal_buf, ptr::null_mut());
        assert!(!obuf.is_null());

        // Write data
        let data = c("Hello Output");
        // UPSTREAM-PARITY: with a write callback and < MINLEN buffered,
        // xmlOutputBufferWrite returns 0 (verified against the system DSO
        // by the SAVE-001 differential court).
        let ret = output_buffer_write(obuf, 12, data.as_ptr());
        assert_eq!(ret, 0);

        // Flush - this writes buffered data via callback to the target buffer
        let flushed = output_buffer_flush(obuf);
        assert_eq!(flushed, 12);

        // After flush, the internal buffer should be empty again
        let content = output_buffer_get_content(obuf);
        assert!(content.is_null() || unsafe { *content } == 0);

        // The data was written via callback to the context buffer (internal_buf)
        let ctx = unsafe { (*obuf).context as *mut _xmlBuffer };
        let ctx_b = unsafe { &*ctx };
        assert_eq!(ctx_b.use_, 12);
        unsafe {
            assert_eq!(*ctx_b.content.offset(0), b'H' as xmlChar);
        }

        // NOTE: output_buffer_close frees all internal buffers including
        // the internal_buf passed as target. Don't free it again here.
        output_buffer_close(obuf);
    }

    #[test]
    fn test_output_buffer_write_string() {
        let internal_buf = buf_create(100);
        let obuf = output_buffer_create_buffer(internal_buf, ptr::null_mut());
        assert!(!obuf.is_null());

        let s = c("Hello");
        // UPSTREAM-PARITY: buffered write returns 0 (see above).
        let ret = output_buffer_write_string(obuf, s.as_ptr());
        assert_eq!(ret, 0);

        // output_buffer_close frees internal_buf via obuf.buffer
        output_buffer_close(obuf);
    }

    #[test]
    fn test_output_buffer_write_char() {
        let internal_buf = buf_create(100);
        let obuf = output_buffer_create_buffer(internal_buf, ptr::null_mut());
        assert!(!obuf.is_null());

        let ret = output_buffer_write_char(obuf, b'X' as c_char);
        // UPSTREAM-PARITY: buffered write returns 0 (see above).
        assert_eq!(ret, 0);

        output_buffer_close(obuf);
    }

    #[test]
    fn test_output_buffer_get_content() {
        let internal_buf = buf_create(100);
        let obuf = output_buffer_create_buffer(internal_buf, ptr::null_mut());
        assert!(!obuf.is_null());

        let content = output_buffer_get_content(obuf);
        assert!(!content.is_null());

        output_buffer_close(obuf);
    }

    // ── File I/O tests ─────────────────────────────────────────────────────

    #[test]
    fn test_check_file_exists() {
        // This file should exist
        let exists = check_file_exists(c("/dev/null").as_ptr());
        assert!(exists == 1);

        // This file should not exist
        let not_exists = check_file_exists(c("/tmp/__nonexistent_file_xyz123__").as_ptr());
        assert!(not_exists == 0);
    }

    #[test]
    fn test_read_write_file() {
        let tmpfile = c("/tmp/libxml_rs_test_io_file.txt");

        // Write data to file
        let data = c("Hello File I/O!");
        let ret = write_memory_to_file(tmpfile.as_ptr(), data.as_ptr(), 15);
        assert_eq!(ret, 0);

        // Check it exists
        assert!(check_file_exists(tmpfile.as_ptr()) == 1);

        // Read it back
        let mut size: c_int = 0;
        let read_data = read_file_to_memory(tmpfile.as_ptr(), &mut size as *mut c_int);
        assert!(!read_data.is_null());
        assert_eq!(size, 15);

        unsafe {
            let slice = std::slice::from_raw_parts(read_data as *const u8, size as usize);
            assert_eq!(slice, b"Hello File I/O!");
        }

        unsafe { xmlFreeImpl(read_data as *mut c_void) };

        // Clean up
        std::fs::remove_file("/tmp/libxml_rs_test_io_file.txt").ok();
    }

    #[test]
    fn test_read_file_nonexistent() {
        let result = read_file_to_memory(
            c("/tmp/__nonexistent_file_xyz456__").as_ptr(),
            ptr::null_mut(),
        );
        assert!(result.is_null());
    }

    #[test]
    fn test_write_file_null() {
        let ret = write_memory_to_file(ptr::null(), c("data").as_ptr(), 4);
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_get_cwd() {
        let cwd = get_cwd();
        assert!(!cwd.is_null());
        unsafe {
            let s = CStr::from_ptr(cwd);
            assert!(!s.to_bytes().is_empty());
            xmlFreeImpl(cwd as *mut c_void);
        }
    }

    // ── Edge case tests ────────────────────────────────────────────────────

    #[test]
    fn test_buf_add_large_data() {
        let buf = buf_create(10);
        let mut large_data = Vec::new();
        large_data.resize(5000, b'X');
        large_data.push(0);

        let ret = buf_add(buf, large_data.as_ptr() as *const xmlChar, 5000);
        assert_eq!(ret, 5000);

        let b = unsafe { &*buf };
        assert_eq!(b.use_, 5000);
        assert!(b.size >= 5001);

        buf_free(buf);
    }

    #[test]
    fn test_input_buffer_free_null() {
        input_buffer_free(ptr::null_mut()); // Should not crash
    }

    #[test]
    fn test_output_buffer_close_null() {
        let ret = output_buffer_close(ptr::null_mut());
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_buf_add_to_immutable() {
        let s: &[u8] = b"static\0";
        let buf = buf_create_static(s.as_ptr() as *const xmlChar, 6);
        assert!(!buf.is_null());

        // Try to add to immutable buffer
        let data: &[u8] = b"more\0";
        let ret = buf_add(buf, data.as_ptr() as *const xmlChar, 4);
        assert_eq!(ret, -1); // Should fail

        buf_free(buf);
    }

    // ── Encoding integration test ──────────────────────────────────────────

    #[test]
    fn test_encoding_from_int() {
        assert_eq!(
            encoding_from_int(0),
            xmlCharEncoding::XML_CHAR_ENCODING_NONE
        );
        assert_eq!(
            encoding_from_int(1),
            xmlCharEncoding::XML_CHAR_ENCODING_UTF8
        );
        assert_eq!(
            encoding_from_int(10),
            xmlCharEncoding::XML_CHAR_ENCODING_8859_1
        );
        assert_eq!(
            encoding_from_int(22),
            xmlCharEncoding::XML_CHAR_ENCODING_ASCII
        );
        assert_eq!(
            encoding_from_int(999),
            xmlCharEncoding::XML_CHAR_ENCODING_ERROR
        );
    }

    #[test]
    fn test_find_handler_for_encoding() {
        // UTF-8 should return null (no conversion needed)
        let handler = find_handler_for_encoding(1);
        assert!(handler.is_null());

        // NONE should return null
        let handler = find_handler_for_encoding(0);
        assert!(handler.is_null());

        // ERROR should return null
        let handler = find_handler_for_encoding(-1);
        assert!(handler.is_null());
    }

    #[test]
    fn test_input_buffer_with_encoding_latin1() {
        // Initialize encodings
        encoding::init_encodings();

        // Latin-1 byte 0xE9 = é in Latin-1, which is U+00E9 = 0xC3 0xA9 in UTF-8
        let latin1_data: &[u8] = &[0x48, 0x65, 0x6C, 0x6C, 0xF6, 0x00]; // "Hellö" in Latin-1

        let buf = input_buffer_create_mem(
            latin1_data.as_ptr() as *const c_char,
            5,
            10, // ISO-8859-1
        );
        assert!(!buf.is_null());

        // Read back as UTF-8
        let mut out = [0i8; 16];
        let ret = input_buffer_read(buf, out.as_mut_ptr(), 16);
        assert!(ret > 0);

        // The output should be UTF-8 encoded "Hellö" = b"Hell\xC3\xB6"
        let expected = b"Hell\xC3\xB6";
        assert_eq!(&out[..ret as usize], i8_slice(expected));

        input_buffer_free(buf);
    }

    #[test]
    fn test_output_buffer_with_encoding() {
        // Initialize encodings
        encoding::init_encodings();

        // Find Latin-1 encoder
        let enc_name: &[u8] = b"ISO-8859-1\0";
        let handler = encoding::find_encoding_handler(enc_name.as_ptr() as *const xmlChar);
        assert!(!handler.is_null(), "Latin-1 handler should be available");

        let internal_buf = buf_create(100);
        let obuf = output_buffer_create_buffer(internal_buf, handler);
        assert!(!obuf.is_null());

        // Write UTF-8 "Hellö" = [0x48, 0x65, 0x6C, 0x6C, 0xC3, 0xB6]
        let utf8_data = unsafe { c_bytes(&[0x48, 0x65, 0x6C, 0x6C, 0xC3, 0xB6]) };
        // UPSTREAM-PARITY: buffered write returns 0 (see above).
        let ret = output_buffer_write(obuf, 6, utf8_data.as_ptr());
        assert_eq!(ret, 0);

        // Flush - this should convert via Latin-1 encoder
        let flushed = output_buffer_flush(obuf);
        assert!(flushed > 0);

        // The context buffer should have the Latin-1 encoded data
        let ctx = unsafe { (*obuf).context as *mut _xmlBuffer };
        let ctx_b = unsafe { &*ctx };
        // Latin-1 "Hellö" = [0x48, 0x65, 0x6C, 0x6C, 0xF6]
        assert_eq!(ctx_b.use_, 5);
        unsafe {
            assert_eq!(*ctx_b.content.offset(0), 0x48); // 'H'
            assert_eq!(*ctx_b.content.offset(4), 0xF6); // 'ö'
        }

        // output_buffer_close frees the internal buffer, but NOT the
        // context buffer (internal_buf) since that's the user's buffer.
        // Actually it frees obuf.buffer (a separate internal buffer) and
        // obuf.conv. The context buffer is NOT freed by output_buffer_close.
        // However, obuf.buffer was set to internal_buf in the OLD code.
        // With the fix, obuf.buffer is a separate internal buffer, so
        // we still need to free internal_buf ourselves.
        //
        // Wait -- let me check what output_buffer_close frees:
        // - ob.buffer: this is the internal buffer (SEPARATE from internal_buf)
        // - ob.conv: the conversion buffer
        // - The context (internal_buf) is NOT freed by output_buffer_close
        //
        // Actually, let me re-read the function...
        // output_buffer_close frees ob.buffer and ob.conv.
        // The context is the user's buffer (internal_buf), which is NOT freed.
        // So we DO need to free internal_buf here.
        output_buffer_close(obuf);
        buf_free(internal_buf);
    }

    // ── Input buffer from fd (requires /dev/null) ──────────────────────────

    #[test]
    fn test_input_buffer_create_fd() {
        // Open /dev/null and create an fd-based input buffer
        let fd =
            unsafe { libc::open(b"/dev/null\0" as *const u8 as *const c_char, libc::O_RDONLY) };
        assert!(fd >= 0);

        let buf = input_buffer_create_fd(fd, 0);
        assert!(!buf.is_null());

        // Reading from /dev/null should return 0 (EOF)
        let mut out = [0i8; 16];
        let ret = input_buffer_read(buf, out.as_mut_ptr(), 16);
        assert_eq!(ret, 0);

        input_buffer_free(buf); // This will also close the fd via closecallback
    }

    // ── Input buffer create_io ─────────────────────────────────────────────

    #[test]
    fn test_input_buffer_create_io() {
        // Create a simple callback that provides data
        static mut TEST_DATA: &[u8] = b"Hello from callback!";
        static mut CALLED: bool = false;

        unsafe extern "C" fn test_read(
            _ctx: *mut c_void,
            buffer: *mut c_char,
            len: c_int,
        ) -> c_int {
            if CALLED {
                return 0; // EOF on second call
            }
            CALLED = true;
            let data = TEST_DATA;
            let to_copy = (data.len() as c_int).min(len);
            if to_copy > 0 {
                std::ptr::copy_nonoverlapping(data.as_ptr(), buffer as *mut u8, to_copy as usize);
            }
            to_copy
        }

        unsafe extern "C" fn test_close(_ctx: *mut c_void) -> c_int {
            0
        }

        let buf = input_buffer_create_io(
            Some(test_read as xmlInputReadCallback),
            Some(test_close as xmlInputCloseCallback),
            ptr::null_mut(),
            0,
        );
        assert!(!buf.is_null());

        let mut out = [0i8; 32];
        let ret = input_buffer_read(buf, out.as_mut_ptr(), 32);
        assert!(ret > 0);

        input_buffer_free(buf);
    }
}
