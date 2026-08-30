//! C ABI exports for the buffer family — xmlBuf* / xmlBuffer* / xmlCharStr*
//! (upstream buf.c, tree.c, xmlstring.c, 2.15.3).
//!
//! In 2.15 `xmlBuf` and `xmlBuffer` are the same C struct (typedef xmlBuffer
//! xmlBuf), but the candidate mirrors them as two distinct structs
//! (`_xmlBuffer` with `{content, use_, size, alloc, contentIO}` and `_xmlBuf`
//! with `{content, use_, size, alloc, error, buffer, io}`). The `xmlBuf*`
//! exports therefore operate on the `_xmlBuf` fields directly, allocating
//! through the candidate allocator (`xmlMalloc`/`xmlFree`), exactly as
//! upstream manipulates `buf->content`/`buf->use`/`buf->size`.

#![allow(
    missing_docs,
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals
)]

use core::ptr;
use std::os::raw::{c_char, c_int, c_uint, c_void};

use crate::abi::allocator::{xmlFree, xmlMalloc};
use crate::abi::structs::{_xmlBuf, _xmlBuffer, _xmlDoc, _xmlNode, _xmlNs};
use crate::abi::types::{xmlChar, xmlElementType, XML_ERR_ARGUMENT, XML_ERR_NO_MEMORY, XML_ERR_OK};
use crate::xml::io;
use crate::xml::tree;

// ── libc FILE* plumbing for xmlBufferDump ───────────────────────────────
//
// The FILE* is opaque at the ABI boundary and is passed as *mut c_void.
// fwrite(3) is declared here rather than pulled from the libc crate so the
// dependency stays explicit; `stdout` is the libc data symbol used by
// upstream's `if (file == NULL) file = stdout;` fallback.

extern "C" {
    /// libc `size_t fwrite(const void *ptr, size_t size, size_t nmemb, FILE *stream)`.
    fn fwrite(ptr: *const c_void, size: usize, nmemb: usize, stream: *mut c_void) -> usize;
    /// The libc `FILE *stdout` variable.
    static mut stdout: *mut c_void;
}

// ═══════════════════════════════════════════════════════════════════════════════
// 1. xmlBuf operations (modern replacement, tree.h)
// ═══════════════════════════════════════════════════════════════════════════════

/// Get pointer into buffer content.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlBufContent(const xmlBuf *buf);
/// ```
///
/// buf.c 2.15: returns `buf->content`, or NULL when `buf` is NULL or the
/// buffer is in error state (`BUF_ERROR`).
///
/// # SAFETY
///
/// - `buf` must be a valid pointer to a `_xmlBuf` (or NULL).
/// - The returned pointer is owned by `buf` and must not be freed by the
///   caller.
#[no_mangle]
pub unsafe extern "C" fn xmlBufContent(buf: *const _xmlBuf) -> *mut xmlChar {
    if buf.is_null() || unsafe { (*buf).error != 0 } {
        return ptr::null_mut();
    }
    unsafe { (*buf).content }
}

/// Return a pointer to the end of the buffer content.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlBufEnd(xmlBuf *buf);
/// ```
///
/// buf.c 2.15: returns `&buf->content[buf->use]`, or NULL when `buf` is NULL
/// or the buffer is in error state.
///
/// # SAFETY
///
/// - `buf` must be a valid pointer to a `_xmlBuf` (or NULL).
/// - The returned pointer is owned by `buf` and must not be freed by the
///   caller.
#[no_mangle]
pub unsafe extern "C" fn xmlBufEnd(buf: *mut _xmlBuf) -> *mut xmlChar {
    if buf.is_null() || unsafe { (*buf).error != 0 } {
        return ptr::null_mut();
    }
    unsafe { (*buf).content.add((*buf).use_ as usize) }
}

/// Append the string value of a node to `buf`.
///
/// For text/CDATA/comment/PI nodes the string value is the node content;
/// otherwise it is the concatenation of the string values of the node's
/// descendants, with entity references substituted. Namespace declaration
/// nodes contribute their href.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlBufGetNodeContent(xmlBuf *buf, const xmlNode *cur);
/// ```
///
/// tree.c 2.15: returns -1 only when `cur` or `buf` is NULL; otherwise 0
/// (append failures are ignored upstream and here, matching the void
/// contract).
///
/// # SAFETY
///
/// - `buf` must be a valid pointer to a `_xmlBuf` (or NULL).
/// - `cur` must be a valid pointer to a `_xmlNode`/`_xmlNs` (or NULL).
#[no_mangle]
pub unsafe extern "C" fn xmlBufGetNodeContent(buf: *mut _xmlBuf, cur: *const _xmlNode) -> c_int {
    if cur.is_null() || buf.is_null() {
        return -1;
    }

    // Upstream xmlBufGetNodeContent appends the namespace href for
    // XML_NAMESPACE_DECL nodes; node_get_content has no arm for them.
    if unsafe { (*cur).type_ } == xmlElementType::XML_NAMESPACE_DECL as c_int {
        let ns = cur as *const _xmlNs;
        if !unsafe { (*ns).href }.is_null() {
            io::xml_buf_cat(buf, unsafe { (*ns).href });
        }
        return 0;
    }

    // node_get_content mirrors tree.c xmlNodeGetContent: recursive string
    // value of the node (text descendants, entity expansion, attribute
    // value, comment/PI content).
    let content = unsafe { tree::node_get_content(cur as *mut _xmlNode) };
    if content.is_null() {
        // Allocation failure: nothing was appended; upstream still returns 0.
        return 0;
    }
    io::xml_buf_cat(buf, content);
    unsafe { xmlFree(content as *mut c_void) };
    0
}

/// Serialize an XML node to an xmlBuf.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// size_t xmlBufNodeDump(xmlBuf *buf, xmlDoc *doc, xmlNode *cur, int level, int format);
/// ```
///
/// xmlsave.c/tree.c 2.15: returns the number of bytes written to `buf`, or
/// `(size_t)-1` when `buf`/`cur` is NULL or serialization fails. The level
/// is clamped to [0, 100] (xmlNodeDumpOutput). `doc` is ignored, matching
/// upstream's `(void) doc`.
///
/// # SAFETY
///
/// - `buf` must be a valid pointer to a `_xmlBuf` (or NULL).
/// - `cur` must be a valid pointer to a `_xmlNode` (or NULL).
/// - `doc` is unused and may be NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlBufNodeDump(
    buf: *mut _xmlBuf,
    doc: *mut _xmlDoc,
    cur: *mut _xmlNode,
    level: c_int,
    format: c_int,
) -> usize {
    let _ = doc;
    if cur.is_null() || buf.is_null() {
        return usize::MAX; // (size_t)-1
    }
    let level = level.clamp(0, 100);

    // Serialize through a temporary xmlBuffer (the candidate serializer
    // targets _xmlBuffer), then append the serialized bytes to the xmlBuf.
    let tmp = io::buf_create(-1);
    if tmp.is_null() {
        return usize::MAX;
    }
    tree::serialize_node_opts(cur, tmp, format, level, ptr::null(), 0);

    let len = io::buf_length(tmp);
    let content = io::buf_content(tmp);
    let written = if len > 0 && !content.is_null() {
        io::xml_buf_add(buf, content, len)
    } else {
        0
    };
    io::buf_free(tmp);

    if written < 0 {
        usize::MAX
    } else {
        written as usize
    }
}

/// Discard bytes at the start of a buffer.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// size_t xmlBufShrink(xmlBuf *buf, size_t len);
/// ```
///
/// buf.c 2.15: removes `len` bytes from the front by advancing
/// `buf->content` and decreasing `buf->use`/`buf->size`; returns the number
/// of bytes removed, or 0 when `buf` is NULL, the buffer is in error state,
/// `len` is 0, or `len` exceeds `buf->use`. Unlike `xmlBufferShrink`, errors
/// return 0 rather than -1 (size_t return type).
///
/// # SAFETY
///
/// - `buf` must be a valid pointer to a `_xmlBuf` (or NULL).
#[no_mangle]
pub unsafe extern "C" fn xmlBufShrink(buf: *mut _xmlBuf, len: usize) -> usize {
    if buf.is_null() || unsafe { (*buf).error != 0 } {
        return 0;
    }
    if len == 0 {
        return 0;
    }
    let b = unsafe { &mut *buf };
    if len > b.use_ as usize {
        return 0;
    }
    b.use_ -= len as c_uint;
    b.content = unsafe { b.content.add(len) };
    b.size -= len as c_uint;
    len
}

/// Return the size of the buffer content.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// size_t xmlBufUse(xmlBuf *buf);
/// ```
///
/// buf.c 2.15: returns `buf->use`, or 0 when `buf` is NULL or the buffer is
/// in error state.
///
/// # SAFETY
///
/// - `buf` must be a valid pointer to a `_xmlBuf` (or NULL).
#[no_mangle]
pub unsafe extern "C" fn xmlBufUse(buf: *mut _xmlBuf) -> usize {
    if buf.is_null() || unsafe { (*buf).error != 0 } {
        return 0;
    }
    unsafe { (*buf).use_ as usize }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 2. xmlBuffer operations (deprecated, tree.h)
// ═══════════════════════════════════════════════════════════════════════════════

/// Append a zero-terminated C string to a buffer.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlBufferCCat(xmlBuffer *buf, const char *str);
/// ```
///
/// buf.c 2.15: forwards to `xmlBufferAdd(buf, (const xmlChar *) str, -1)`,
/// returning XML_ERR_ARGUMENT when `buf` or `str` is NULL, XML_ERR_OK on
/// success (including the empty string), and XML_ERR_NO_MEMORY when growth
/// fails.
///
/// # SAFETY
///
/// - `buf` must be a valid pointer to a `_xmlBuffer` (or NULL).
/// - `str` must be a valid NUL-terminated C string (or NULL).
#[no_mangle]
pub unsafe extern "C" fn xmlBufferCCat(buf: *mut _xmlBuffer, str: *const c_char) -> c_int {
    if buf.is_null() || str.is_null() {
        return XML_ERR_ARGUMENT;
    }
    let ret = io::buf_cat(buf, str as *const xmlChar);
    if ret < 0 {
        XML_ERR_NO_MEMORY
    } else {
        XML_ERR_OK
    }
}

/// Dump a buffer to a `FILE`.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlBufferDump(FILE *file, xmlBuffer *buf);
/// ```
///
/// buf.c 2.15: returns 0 when `buf` is NULL or has no content, defaults a
/// NULL `file` to stdout, and otherwise returns the `fwrite` byte count
/// clamped to INT_MAX. Upstream never returns -1 from this function; write
/// errors surface as a short count.
///
/// # SAFETY
///
/// - `file` must be a valid `FILE*` or NULL (then stdout is used).
/// - `buf` must be a valid pointer to a `_xmlBuffer` (or NULL).
#[no_mangle]
pub unsafe extern "C" fn xmlBufferDump(file: *mut c_void, buf: *mut _xmlBuffer) -> c_int {
    if buf.is_null() {
        return 0;
    }
    let b = unsafe { &*buf };
    if b.content.is_null() {
        return 0;
    }
    let stream = if file.is_null() {
        // Upstream: `if (file == NULL) file = stdout;`
        unsafe { stdout }
    } else {
        file
    };
    let n = unsafe { fwrite(b.content as *const c_void, 1, b.use_ as usize, stream) };
    if n > c_int::MAX as usize {
        c_int::MAX
    } else {
        n as c_int
    }
}

/// Resize a buffer to a minimum size.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlBufferResize(xmlBuffer *buf, unsigned int size);
/// ```
///
/// buf.c 2.15: returns 0 when `buf` is NULL, 1 when `size` is below the
/// current capacity, otherwise grows the buffer so its total capacity is at
/// least `size` and returns 1 on success / 0 on allocation failure.
///
/// # SAFETY
///
/// - `buf` must be a valid pointer to a `_xmlBuffer` (or NULL).
#[no_mangle]
pub unsafe extern "C" fn xmlBufferResize(buf: *mut _xmlBuffer, size: c_uint) -> c_int {
    if buf.is_null() {
        return 0;
    }
    if size < unsafe { (*buf).size } {
        return 1;
    }
    let res = io::buf_grow(buf, size);
    if res < 0 {
        0
    } else {
        1
    }
}

/// Append a zero-terminated `xmlChar` string to a buffer.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlBufferWriteCHAR(xmlBuffer *buf, const xmlChar *string);
/// ```
///
/// buf.c 2.15: `xmlBufferAdd(buf, string, -1)`, i.e. append the
/// NUL-terminated string; failures are ignored (void return).
///
/// # SAFETY
///
/// - `buf` must be a valid pointer to a `_xmlBuffer` (or NULL).
/// - `string` must be a valid NUL-terminated xmlChar string (or NULL).
#[no_mangle]
pub unsafe extern "C" fn xmlBufferWriteCHAR(buf: *mut _xmlBuffer, string: *const xmlChar) {
    let _ = io::buf_cat(buf, string);
}

/// Append a zero-terminated C string to a buffer.
///
/// Same as `xmlBufferCCat`.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlBufferWriteChar(xmlBuffer *buf, const char *string);
/// ```
///
/// buf.c 2.15: `xmlBufferAdd(buf, (const xmlChar *) string, -1)`; failures
/// are ignored (void return).
///
/// # SAFETY
///
/// - `buf` must be a valid pointer to a `_xmlBuffer` (or NULL).
/// - `string` must be a valid NUL-terminated C string (or NULL).
#[no_mangle]
pub unsafe extern "C" fn xmlBufferWriteChar(buf: *mut _xmlBuffer, string: *const c_char) {
    let _ = io::buf_cat(buf, string as *const xmlChar);
}

/// Append a quoted string to a buffer.
///
/// Appends the string wrapped in quotes. If the string contains both single
/// and double quotes, double quotes are escaped with `&quot;` (upstream
/// buf.c 2.15; no backslash or CR/LF escaping exists in this version).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlBufferWriteQuotedString(xmlBuffer *buf, const xmlChar *string);
/// ```
///
/// buf.c 2.15: with a double-quote-containing string that also contains a
/// single quote, emits `"` + string with `"` → `&quot;` + `"`; with only
/// double quotes emits `'` + string + `'`; otherwise emits `"` + string +
/// `"`. A NULL string yields the bare quote pair, exactly like upstream
/// (xmlStrchr(NULL) misses, xmlBufferCat(NULL) fails silently).
///
/// # SAFETY
///
/// - `buf` must be a valid pointer to a `_xmlBuffer` (or NULL).
/// - `string` must be a valid NUL-terminated xmlChar string (or NULL).
#[no_mangle]
pub unsafe extern "C" fn xmlBufferWriteQuotedString(buf: *mut _xmlBuffer, string: *const xmlChar) {
    if buf.is_null() {
        return;
    }
    let has_dquote = !crate::abi::exports_xml2::xmlStrchr(string, b'"').is_null();
    let has_squote = !crate::abi::exports_xml2::xmlStrchr(string, b'\'').is_null();
    if has_dquote {
        if has_squote {
            // Escape every double quote with &quot; inside double quotes.
            io::buf_cat(buf, b"\"".as_ptr());
            let mut base = string;
            let mut cur = string;
            while !cur.is_null() && unsafe { *cur } != 0 {
                if unsafe { *cur } == b'"' {
                    if base != cur {
                        let n = cur.offset_from(base) as c_int;
                        io::buf_add(buf, base, n);
                    }
                    io::buf_cat(buf, b"&quot;".as_ptr());
                    cur = cur.add(1);
                    base = cur;
                } else {
                    cur = cur.add(1);
                }
            }
            if base != cur {
                let n = cur.offset_from(base) as c_int;
                io::buf_add(buf, base, n);
            }
            io::buf_cat(buf, b"\"".as_ptr());
        } else {
            // Only double quotes: single-quote the string.
            io::buf_cat(buf, b"\'".as_ptr());
            io::buf_cat(buf, string);
            io::buf_cat(buf, b"\'".as_ptr());
        }
    } else {
        io::buf_cat(buf, b"\"".as_ptr());
        io::buf_cat(buf, string);
        io::buf_cat(buf, b"\"".as_ptr());
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 3. xmlChar string duplication (xmlstring.h)
// ═══════════════════════════════════════════════════════════════════════════════

/// Duplicate a `char *` string to a new `xmlChar *` string.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlCharStrdup(const char *cur);
/// ```
///
/// xmlstring.c 2.15: returns NULL for a NULL input, otherwise
/// `xmlCharStrndup(cur, strlen(cur))`.
///
/// # SAFETY
///
/// - `cur` must be a valid NUL-terminated C string or NULL.
/// - The returned pointer is allocated with `xmlMalloc` and must be freed
///   with `xmlFree`.
#[no_mangle]
pub unsafe extern "C" fn xmlCharStrdup(cur: *const c_char) -> *mut xmlChar {
    if cur.is_null() {
        return ptr::null_mut();
    }
    let len = unsafe { libc::strlen(cur) } as c_int;
    unsafe { xmlCharStrndup(cur, len) }
}

/// Duplicate `len` bytes of a `char *` string to a new `xmlChar *` string.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlCharStrndup(const char *cur, int len);
/// ```
///
/// xmlstring.c 2.15: returns NULL when `cur` is NULL or `len` is negative;
/// otherwise allocates `len + 1` bytes, copies at most `len` bytes, stops
/// early (and returns immediately) if an embedded NUL is copied, and always
/// NUL-terminates.
///
/// # SAFETY
///
/// - `cur` must be a valid pointer to at least `len` readable bytes or NULL.
/// - The returned pointer is allocated with `xmlMalloc` and must be freed
///   with `xmlFree`.
#[no_mangle]
pub unsafe extern "C" fn xmlCharStrndup(cur: *const c_char, len: c_int) -> *mut xmlChar {
    if cur.is_null() || len < 0 {
        return ptr::null_mut();
    }
    let ret = unsafe { xmlMalloc(len as usize + 1) } as *mut xmlChar;
    if ret.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        let mut i: usize = 0;
        while i < len as usize {
            let byte = *cur.add(i) as xmlChar;
            *ret.add(i) = byte;
            if byte == 0 {
                return ret; // Embedded NUL: already terminated.
            }
            i += 1;
        }
        *ret.add(len as usize) = 0;
    }
    ret
}
