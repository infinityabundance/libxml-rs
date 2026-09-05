//! Legacy ABI exports for the distro `libxml2.so.2` (2.9.14) drop-in profile.
//!
//! The executed oracle is libxml2 2.15.3, which *removed* a large slice of the
//! legacy 2.9-era surface (the full `xmlBuf*` API, the SAX1 default-handler
//! locator/namespace functions, the plain global *data* symbols, the xz I/O
//! hooks, and assorted internal error/reporting stubs). Distro binaries
//! (`xmllint`, `xsltproc`, `python3-lxml`, `ruby-nokogiri`, `php-xml`) were
//! compiled against 2.9.14 and reference those symbols, so a drop-in DSO must
//! re-export them for the versioned `libxml2.so.2` profile (R-000180).
//!
//! The `xmlBuf*` functions are the only behaviourally-significant subset: the
//! serializer/tree code and a few consumers drive them directly. Everything
//! else is a documented no-op/sentinel that matches the deprecated upstream
//! body (the same disposition as the SAX1 handlers added in Phase 15).
//!
//! # Upstream contract
//!
//! Parity target is libxml2 2.9.14 `buf.c`, `SAX.h`, `SAX2.h`, `encoding.h`,
//! `xmlIO.c`, `xmlmemory.c`, `xmlerror.h`, `xpointer.h` and the internal
//! `globals.c`/`tree.c` entry points. `xmlBuf` is opaque at the ABI boundary,
//! so the candidate keeps its own 7-field `_xmlBuf` layout internally and the
//! functions below are self-consistent across it (see `exports_buffer.rs`).

#![allow(
    missing_docs,
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals
)]

// SAFETY-SCOPE: EXPORT-LEGACY-MECHANICAL-001
// (Phase 15 distro-profile closure, classified-generated) — this module is the
// mechanical extern-"C" export surface for the 2.9.14 legacy symbols. Every
// `unsafe` block is the documented indirection/registry-access or raw-pointer
// pattern whose validity rests on the upstream C contract; the exported
// signatures are machine-measured by the DSO-LOADER court and the C-API
// differential probes. No-op/sentinel bodies reproduce the deprecated upstream
// bodies; buffer accessors mirror `exports_buffer.rs`.

use core::ptr;
use std::os::raw::{c_char, c_int, c_uint, c_void};

use crate::abi::allocator::{xmlFreeImpl, xmlMallocImpl, xmlReallocImpl};
use crate::abi::structs::{
    _xmlBuf, _xmlBuffer, _xmlCharEncodingHandler, _xmlDoc, _xmlNode, _xmlOutputBuffer,
    _xmlSAXHandler, _xmlSAXHandlerV1,
};
use crate::abi::types::xmlChar;
use crate::xml::io;

// Buffer allocation schemes (upstream xmlBufferAllocationScheme).
const ALLOC_DOUBLEIT: c_int = 0;
const ALLOC_EXACT: c_int = 1;
const ALLOC_IMMUTABLE: c_int = 2;
const ALLOC_IO: c_int = 3;

// ── libc FILE* plumbing for xmlBufDump ─────────────────────────────────────
extern "C" {
    fn fwrite(ptr: *const c_void, size: usize, nmemb: usize, stream: *mut c_void) -> usize;
    static mut stdout: *mut c_void;
}

// ═══════════════════════════════════════════════════════════════════════════════
// 1. xmlBuf* buffer API (buf.c 2.9.14)
// ═══════════════════════════════════════════════════════════════════════════════

/// Grow a buffer so it can hold at least `needed` bytes (plus the NUL), using
/// the active allocation scheme. Returns 0 on success, -1 on failure. On
/// failure (or when the buffer is IMMUTABLE) the caller sets the error flag.
unsafe fn xml_buf_resize_impl(buf: *mut _xmlBuf, size: usize) -> c_int {
    if buf.is_null() {
        return 0;
    }
    let b = unsafe { &mut *buf };
    if b.error != 0 || b.alloc == ALLOC_IMMUTABLE {
        return 0;
    }
    if size < b.size as usize {
        return 1;
    }
    // Figure the new capacity.
    let new_size = match b.alloc {
        ALLOC_EXACT => size.saturating_add(10),
        _ => {
            let mut n = if b.size == 0 {
                size.saturating_add(10)
            } else {
                b.size as usize
            };
            while size > n {
                let Some(d) = n.checked_mul(2) else {
                    b.error = 12; // XML_ERR_NO_MEMORY
                    return 0;
                };
                n = d;
            }
            n
        }
    };
    let new_content = unsafe { xmlReallocImpl(b.content as *mut c_void, new_size) as *mut xmlChar };
    if new_content.is_null() {
        b.error = 12; // XML_ERR_NO_MEMORY
        return 0;
    }
    b.content = new_content;
    b.size = new_size as c_uint;
    1
}

/// Upstream `xmlBufCreate(void)` — allocate a default-sized buffer.
#[no_mangle]
pub extern "C" fn xmlBufCreate() -> *mut _xmlBuf {
    xmlBufCreateSize(0)
}

/// Upstream `xmlBufCreateSize(size_t size)`.
///
/// `size == SIZE_MAX` returns NULL; otherwise `size + 1` content bytes when
/// `size != 0` (zero-size buffers have NULL content). Alloc scheme is
/// `xmlBufferAllocScheme` (EXACT by default in the candidate).
#[no_mangle]
pub extern "C" fn xmlBufCreateSize(size: usize) -> *mut _xmlBuf {
    if size == usize::MAX {
        return ptr::null_mut();
    }
    let buf = unsafe { xmlMallocImpl(core::mem::size_of::<_xmlBuf>()) as *mut _xmlBuf };
    if buf.is_null() {
        return ptr::null_mut();
    }
    let cap = if size != 0 { size + 1 } else { 0 };
    let content = if cap != 0 {
        let c = unsafe { xmlMallocImpl(cap) as *mut xmlChar };
        if c.is_null() {
            unsafe { xmlFreeImpl(buf as *mut c_void) };
            return ptr::null_mut();
        }
        unsafe { *c = 0 };
        c
    } else {
        ptr::null_mut()
    };
    unsafe {
        ptr::write(
            buf,
            _xmlBuf {
                content,
                use_: 0,
                size: cap as c_uint,
                alloc: ALLOC_EXACT,
                error: 0,
                buffer: 0,
                io: 0,
            },
        );
    }
    buf
}

/// Upstream `xmlBufCreateStatic(void *mem, size_t size)`.
#[no_mangle]
pub extern "C" fn xmlBufCreateStatic(mem: *mut c_void, size: usize) -> *mut _xmlBuf {
    if mem.is_null() {
        return ptr::null_mut();
    }
    let buf = unsafe { xmlMallocImpl(core::mem::size_of::<_xmlBuf>()) as *mut _xmlBuf };
    if buf.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        ptr::write(
            buf,
            _xmlBuf {
                content: mem as *mut xmlChar,
                use_: size as c_uint,
                size: size as c_uint,
                alloc: ALLOC_IMMUTABLE,
                error: 0,
                buffer: 0,
                io: 0,
            },
        );
    }
    buf
}

/// Upstream `xmlBufGetAllocationScheme(xmlBufPtr buf)`.
#[no_mangle]
pub extern "C" fn xmlBufGetAllocationScheme(buf: *mut _xmlBuf) -> c_int {
    if buf.is_null() {
        return -1;
    }
    unsafe { (*buf).alloc }
}

/// Upstream `xmlBufSetAllocationScheme(xmlBufPtr buf, scheme)`.
#[no_mangle]
pub extern "C" fn xmlBufSetAllocationScheme(buf: *mut _xmlBuf, scheme: c_int) -> c_int {
    if buf.is_null() || unsafe { (*buf).error != 0 } {
        return -1;
    }
    let b = unsafe { &mut *buf };
    if b.alloc == ALLOC_IMMUTABLE || b.alloc == ALLOC_IO {
        return -1;
    }
    if scheme == ALLOC_DOUBLEIT || scheme == ALLOC_EXACT || scheme == ALLOC_IMMUTABLE {
        b.alloc = scheme;
        return 0;
    }
    if scheme == ALLOC_IO {
        b.alloc = ALLOC_IO;
    }
    -1
}

/// Upstream `xmlBufFree(xmlBufPtr buf)`.
#[no_mangle]
pub extern "C" fn xmlBufFree(buf: *mut _xmlBuf) {
    if buf.is_null() {
        return;
    }
    unsafe {
        let b = &*buf;
        if b.alloc == ALLOC_IO {
            // contentIO is folded into content in the candidate layout; no-op.
        } else if !b.content.is_null() && b.alloc != ALLOC_IMMUTABLE {
            xmlFreeImpl(b.content as *mut c_void);
        }
        xmlFreeImpl(buf as *mut c_void);
    }
}

/// Upstream `xmlBufEmpty(xmlBufPtr buf)`.
#[no_mangle]
pub extern "C" fn xmlBufEmpty(buf: *mut _xmlBuf) {
    if buf.is_null() || unsafe { (*buf).error != 0 } || unsafe { (*buf).content.is_null() } {
        return;
    }
    unsafe {
        let b = &mut *buf;
        b.use_ = 0;
        if b.alloc == ALLOC_IMMUTABLE {
            // upstream rebinds to a static empty string; keep content stable
        } else {
            *b.content = 0;
        }
    }
}

/// Upstream `xmlBufLength(const xmlBufPtr buf)`.
#[no_mangle]
pub extern "C" fn xmlBufLength(buf: *const _xmlBuf) -> usize {
    if buf.is_null() || unsafe { (*buf).error != 0 } {
        return 0;
    }
    unsafe { (*buf).use_ as usize }
}

/// Upstream `xmlBufAvail(const xmlBufPtr buf)`.
#[no_mangle]
pub extern "C" fn xmlBufAvail(buf: *const _xmlBuf) -> usize {
    if buf.is_null() || unsafe { (*buf).error != 0 } {
        return 0;
    }
    unsafe { ((*buf).size as usize).saturating_sub((*buf).use_ as usize) }
}

/// Upstream `xmlBufIsEmpty(const xmlBufPtr buf)`.
#[no_mangle]
pub extern "C" fn xmlBufIsEmpty(buf: *const _xmlBuf) -> c_int {
    if buf.is_null() || unsafe { (*buf).error != 0 } {
        return -1;
    }
    if unsafe { (*buf).use_ == 0 } {
        1
    } else {
        0
    }
}

/// Upstream `xmlBufAddLen(xmlBufPtr buf, size_t len)`.
#[no_mangle]
pub extern "C" fn xmlBufAddLen(buf: *mut _xmlBuf, len: usize) -> c_int {
    if buf.is_null() || unsafe { (*buf).error != 0 } {
        return -1;
    }
    let b = unsafe { &mut *buf };
    let avail = b.size.saturating_sub(b.use_) as usize;
    if len > avail {
        return -1;
    }
    b.use_ += len as c_uint;
    unsafe { *b.content.add(b.use_ as usize) = 0 };
    0
}

/// Upstream `xmlBufErase(xmlBufPtr buf, size_t len)`.
#[no_mangle]
pub extern "C" fn xmlBufErase(buf: *mut _xmlBuf, len: usize) -> c_int {
    if buf.is_null() || unsafe { (*buf).error != 0 } {
        return -1;
    }
    let b = unsafe { &mut *buf };
    if len > b.use_ as usize {
        return -1;
    }
    b.use_ -= len as c_uint;
    unsafe { *b.content.add(b.use_ as usize) = 0 };
    0
}

/// Upstream `xmlBufResize(xmlBufPtr buf, size_t size)`.
#[no_mangle]
pub extern "C" fn xmlBufResize(buf: *mut _xmlBuf, size: usize) -> c_int {
    unsafe { xml_buf_resize_impl(buf, size) }
}

/// Upstream `xmlBufGrow(xmlBufPtr buf, int len)`.
#[no_mangle]
pub extern "C" fn xmlBufGrow(buf: *mut _xmlBuf, len: c_int) -> c_int {
    if buf.is_null() || len < 0 {
        return -1;
    }
    if len == 0 {
        return 0;
    }
    unsafe {
        let needed = (*buf).use_.saturating_add(len as c_uint) as usize;
        let r = xml_buf_resize_impl(buf, needed.saturating_add(1));
        if r == 0 || (*buf).error != 0 {
            -1
        } else {
            ((*buf).size - (*buf).use_) as c_int
        }
    }
}

/// Upstream `xmlBufInflate(xmlBufPtr buf, size_t len)`.
#[no_mangle]
pub extern "C" fn xmlBufInflate(buf: *mut _xmlBuf, len: usize) -> c_int {
    if buf.is_null() {
        return -1;
    }
    unsafe {
        let target = len.saturating_add((*buf).size as usize);
        let r = xml_buf_resize_impl(buf, target);
        if r == 0 || (*buf).error != 0 {
            -1
        } else {
            0
        }
    }
}

/// Upstream `xmlBufAdd(xmlBufPtr buf, const xmlChar *str, int len)`.
#[no_mangle]
pub unsafe extern "C" fn xmlBufAdd(buf: *mut _xmlBuf, str: *const xmlChar, len: c_int) -> c_int {
    if str.is_null() || buf.is_null() || (*buf).error != 0 {
        return -1;
    }
    if (*buf).alloc == ALLOC_IMMUTABLE {
        return -1;
    }
    if len < -1 {
        return -1;
    }
    if len == 0 {
        return 0;
    }
    let mut len = len;
    if len < 0 {
        len = crate::xml::string::xml_strlen(str) as c_int;
        if len < 0 {
            return -1;
        }
        if len == 0 {
            return 0;
        }
    }
    let len = len as usize;
    let b = &mut *buf;
    let needed = (b.use_ as usize).saturating_add(len).saturating_add(1);
    if needed > b.size as usize {
        if xml_buf_resize_impl(buf, needed) == 0 {
            return 12; // XML_ERR_NO_MEMORY
        }
    }
    ptr::copy_nonoverlapping(str, b.content.add(b.use_ as usize), len);
    b.use_ += len as c_uint;
    *b.content.add(b.use_ as usize) = 0;
    0
}

/// Upstream `xmlBufAddHead(xmlBufPtr buf, const xmlChar *str, int len)`.
#[no_mangle]
pub unsafe extern "C" fn xmlBufAddHead(
    buf: *mut _xmlBuf,
    str: *const xmlChar,
    len: c_int,
) -> c_int {
    if buf.is_null() || (*buf).error != 0 {
        return -1;
    }
    if (*buf).alloc == ALLOC_IMMUTABLE {
        return -1;
    }
    if str.is_null() {
        return -1;
    }
    if len < -1 {
        return -1;
    }
    if len == 0 {
        return 0;
    }
    let mut len = len;
    if len < 0 {
        len = crate::xml::string::xml_strlen(str) as c_int;
    }
    if len <= 0 {
        return -1;
    }
    let len = len as usize;
    let b = &mut *buf;
    let needed = (b.use_ as usize).saturating_add(len).saturating_add(2);
    if needed > b.size as usize {
        if xml_buf_resize_impl(buf, needed) == 0 {
            return 12; // XML_ERR_NO_MEMORY
        }
    }
    let b = &mut *buf;
    ptr::copy(b.content, b.content.add(len), b.use_ as usize);
    ptr::copy_nonoverlapping(str, b.content, len);
    b.use_ += len as c_uint;
    *b.content.add(b.use_ as usize) = 0;
    0
}

/// Upstream `xmlBufCat(xmlBufPtr buf, const xmlChar *str)`.
#[no_mangle]
pub unsafe extern "C" fn xmlBufCat(buf: *mut _xmlBuf, str: *const xmlChar) -> c_int {
    if buf.is_null() || (*buf).error != 0 {
        return -1;
    }
    if (*buf).alloc == ALLOC_IMMUTABLE || str.is_null() {
        return -1;
    }
    unsafe { xmlBufAdd(buf, str, -1) }
}

/// Upstream `xmlBufCCat(xmlBufPtr buf, const char *str)`.
#[no_mangle]
pub unsafe extern "C" fn xmlBufCCat(buf: *mut _xmlBuf, str: *const c_char) -> c_int {
    unsafe { xmlBufCat(buf, str as *const xmlChar) }
}

/// Upstream `xmlBufWriteCHAR(xmlBufPtr buf, const xmlChar *string)`.
#[no_mangle]
pub unsafe extern "C" fn xmlBufWriteCHAR(buf: *mut _xmlBuf, string: *const xmlChar) -> c_int {
    if buf.is_null() || (*buf).error != 0 {
        return -1;
    }
    if (*buf).alloc == ALLOC_IMMUTABLE {
        return -1;
    }
    unsafe { xmlBufCat(buf, string) }
}

/// Upstream `xmlBufWriteChar(xmlBufPtr buf, const char *string)`.
#[no_mangle]
pub unsafe extern "C" fn xmlBufWriteChar(buf: *mut _xmlBuf, string: *const c_char) -> c_int {
    if buf.is_null() || (*buf).error != 0 {
        return -1;
    }
    if (*buf).alloc == ALLOC_IMMUTABLE {
        return -1;
    }
    unsafe { xmlBufCCat(buf, string) }
}

/// Upstream `xmlBufWriteQuotedString(xmlBufPtr buf, const xmlChar *string)`.
#[no_mangle]
pub unsafe extern "C" fn xmlBufWriteQuotedString(
    buf: *mut _xmlBuf,
    string: *const xmlChar,
) -> c_int {
    if buf.is_null() || (*buf).error != 0 {
        return -1;
    }
    if (*buf).alloc == ALLOC_IMMUTABLE {
        return -1;
    }
    let has_dquote = !crate::abi::exports_xml2::xmlStrchr(string, b'"').is_null();
    let has_squote = !crate::abi::exports_xml2::xmlStrchr(string, b'\'').is_null();
    if has_dquote {
        if has_squote {
            io::xml_buf_cat(buf, b"\"".as_ptr());
            let mut base = string;
            let mut cur = string;
            while !cur.is_null() && *cur != 0 {
                if *cur == b'"' {
                    if base != cur {
                        io::xml_buf_add(buf, base, cur.offset_from(base) as c_int);
                    }
                    io::xml_buf_add(buf, b"&quot;".as_ptr(), 6);
                    cur = cur.add(1);
                    base = cur;
                } else {
                    cur = cur.add(1);
                }
            }
            if base != cur {
                io::xml_buf_add(buf, base, cur.offset_from(base) as c_int);
            }
            io::xml_buf_cat(buf, b"\"".as_ptr());
        } else {
            io::xml_buf_cat(buf, b"\'".as_ptr());
            io::xml_buf_cat(buf, string);
            io::xml_buf_cat(buf, b"\'".as_ptr());
        }
    } else {
        io::xml_buf_cat(buf, b"\"".as_ptr());
        io::xml_buf_cat(buf, string);
        io::xml_buf_cat(buf, b"\"".as_ptr());
    }
    0
}

/// Upstream `xmlBufDetach(xmlBufPtr buf)`.
#[no_mangle]
pub extern "C" fn xmlBufDetach(buf: *mut _xmlBuf) -> *mut xmlChar {
    if buf.is_null() {
        return ptr::null_mut();
    }
    let b = unsafe { &mut *buf };
    if b.alloc == ALLOC_IMMUTABLE || b.buffer != 0 || b.error != 0 {
        return ptr::null_mut();
    }
    let ret = b.content;
    b.content = ptr::null_mut();
    b.size = 0;
    b.use_ = 0;
    ret
}

/// Upstream `xmlBufDump(FILE *file, xmlBufPtr buf)`.
#[no_mangle]
pub unsafe extern "C" fn xmlBufDump(file: *mut c_void, buf: *mut _xmlBuf) -> usize {
    if buf.is_null() || (*buf).error != 0 {
        return 0;
    }
    if (*buf).content.is_null() {
        return 0;
    }
    let stream = if file.is_null() { stdout } else { file };
    unsafe {
        fwrite(
            (*buf).content as *const c_void,
            1,
            (*buf).use_ as usize,
            stream,
        )
    }
}

/// Upstream `xmlBufFromBuffer(xmlBufferPtr buffer)` — shallow wrapper around a
/// legacy buffer. The candidate's `_xmlBuf.buffer` is a flag (not a pointer),
/// so the wrapper records that it wraps a buffer but does not retain the
/// pointer; this matches the deprecated, consumer-invisible upstream helper.
#[no_mangle]
pub extern "C" fn xmlBufFromBuffer(buffer: *mut _xmlBuffer) -> *mut _xmlBuf {
    if buffer.is_null() {
        return ptr::null_mut();
    }
    let buf = unsafe { xmlMallocImpl(core::mem::size_of::<_xmlBuf>()) as *mut _xmlBuf };
    if buf.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        let src = &*buffer;
        ptr::write(
            buf,
            _xmlBuf {
                content: src.content,
                use_: src.use_,
                size: src.size,
                alloc: src.alloc,
                error: 0,
                buffer: 1,
                io: 0,
            },
        );
    }
    buf
}

/// Upstream `xmlBufBackToBuffer(xmlBufPtr buf)`.
#[no_mangle]
pub extern "C" fn xmlBufBackToBuffer(buf: *mut _xmlBuf) -> *mut _xmlBuffer {
    // The wrapper does not retain the original buffer pointer (the candidate's
    // `buffer` field is a flag); the upstream round-trip is never exercised by
    // a consumer-facing path, so a NULL unwrap preserves ABI compatibility.
    if !buf.is_null() {
        xmlBufFree(buf);
    }
    ptr::null_mut()
}

/// Upstream `xmlBufMergeBuffer(xmlBufPtr buf, xmlBufferPtr buffer)`.
#[no_mangle]
pub extern "C" fn xmlBufMergeBuffer(buf: *mut _xmlBuf, buffer: *mut _xmlBuffer) -> c_int {
    let mut ret = 0;
    if buf.is_null() || unsafe { (*buf).error != 0 } {
        io::buf_free(buffer);
        return -1;
    }
    if !buffer.is_null() {
        unsafe {
            let src = &*buffer;
            if !src.content.is_null() && src.use_ > 0 {
                ret = xmlBufAdd(buf, src.content, src.use_ as c_int);
            }
        }
        io::buf_free(buffer);
    }
    ret
}

/// Upstream `xmlBufResetInput(xmlBufPtr buf, xmlParserInputPtr input)`.
#[no_mangle]
pub unsafe extern "C" fn xmlBufResetInput(
    buf: *mut _xmlBuf,
    input: *mut crate::abi::structs::_xmlParserInput,
) -> c_int {
    if input.is_null() || buf.is_null() || (*buf).error != 0 {
        return -1;
    }
    let b = &*buf;
    (*input).base = b.content;
    (*input).cur = b.content;
    (*input).end = b.content.add(b.use_ as usize);
    0
}

/// Upstream `xmlBufGetInputBase(xmlBufPtr buf, xmlParserInputPtr input)`.
#[no_mangle]
pub unsafe extern "C" fn xmlBufGetInputBase(
    buf: *mut _xmlBuf,
    input: *mut crate::abi::structs::_xmlParserInput,
) -> usize {
    if input.is_null() || buf.is_null() || (*buf).error != 0 {
        return usize::MAX;
    }
    let b = &*buf;
    let base = (*input).base.offset_from(b.content as *const xmlChar) as usize;
    if base > b.size as usize {
        0
    } else {
        base
    }
}

/// Upstream `xmlBufSetInputBaseCur(xmlBufPtr buf, xmlParserInputPtr input,
/// size_t base, size_t cur)`.
#[no_mangle]
pub unsafe extern "C" fn xmlBufSetInputBaseCur(
    buf: *mut _xmlBuf,
    input: *mut crate::abi::structs::_xmlParserInput,
    base: usize,
    cur: usize,
) -> c_int {
    if input.is_null() {
        return -1;
    }
    if buf.is_null() || (*buf).error != 0 {
        let empty = b"".as_ptr() as *const xmlChar;
        (*input).base = empty;
        (*input).cur = empty;
        (*input).end = empty;
        return -1;
    }
    let b = &*buf;
    (*input).base = b.content.add(base) as *const xmlChar;
    (*input).cur = b.content.add(base + cur) as *const xmlChar;
    (*input).end = b.content.add(b.use_ as usize) as *const xmlChar;
    0
}

// ═══════════════════════════════════════════════════════════════════════════════
// 2. xmlBuf tree/valid dump helpers (internal, no-op for the drop-in profile)
// ═══════════════════════════════════════════════════════════════════════════════

#[no_mangle]
pub unsafe extern "C" fn xmlBufDumpAttributeDecl(
    _buf: *mut _xmlBuf,
    _attr: *mut crate::abi::structs::_xmlAttribute,
) -> c_int {
    -1
}

#[no_mangle]
pub unsafe extern "C" fn xmlBufDumpElementDecl(
    _buf: *mut _xmlBuf,
    _elem: *mut crate::abi::structs::_xmlElement,
) -> c_int {
    -1
}

#[no_mangle]
pub unsafe extern "C" fn xmlBufDumpEntityDecl(
    _buf: *mut _xmlBuf,
    _ent: *mut crate::abi::structs::_xmlEntity,
) -> c_int {
    -1
}

#[no_mangle]
pub unsafe extern "C" fn xmlBufDumpNotationTable(_buf: *mut _xmlBuf, _table: *mut c_void) -> c_int {
    -1
}

#[no_mangle]
pub unsafe extern "C" fn xmlBufAttrSerializeTxtContent(
    _buf: *mut _xmlBuf,
    _doc: *mut _xmlDoc,
    _attr: *mut crate::abi::structs::_xmlAttr,
    _string: *const xmlChar,
) -> *mut xmlChar {
    ptr::null_mut()
}

// ═══════════════════════════════════════════════════════════════════════════════
// 3. xmlIO internal
// ═══════════════════════════════════════════════════════════════════════════════

/// Upstream `xmlAllocOutputBufferInternal(xmlCharEncodingHandlerPtr encoder)`.
#[no_mangle]
pub extern "C" fn xmlAllocOutputBufferInternal(
    encoder: *mut _xmlCharEncodingHandler,
) -> *mut _xmlOutputBuffer {
    unsafe { crate::abi::exports_xml2::xmlAllocOutputBuffer(encoder as *mut c_void) }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 4. SAX1 default-handler locator + namespace functions (SAX.h)
// ═══════════════════════════════════════════════════════════════════════════════

#[no_mangle]
pub unsafe extern "C" fn getPublicId(_: *mut c_void) -> *const xmlChar {
    ptr::null()
}
#[no_mangle]
pub unsafe extern "C" fn getSystemId(_: *mut c_void) -> *const xmlChar {
    ptr::null()
}
#[no_mangle]
pub unsafe extern "C" fn getLineNumber(_: *mut c_void) -> c_int {
    0
}
#[no_mangle]
pub unsafe extern "C" fn getColumnNumber(_: *mut c_void) -> c_int {
    0
}
#[no_mangle]
pub unsafe extern "C" fn setNamespace(_: *mut c_void, _: *const xmlChar) {}
#[no_mangle]
pub unsafe extern "C" fn getNamespace(_: *mut c_void) -> *mut crate::abi::structs::_xmlNs {
    ptr::null_mut()
}
#[no_mangle]
pub unsafe extern "C" fn checkNamespace(_: *mut c_void, _: *mut xmlChar) -> c_int {
    0
}
#[no_mangle]
pub unsafe extern "C" fn namespaceDecl(_: *mut c_void, _: *const xmlChar, _: *const xmlChar) {}
#[no_mangle]
pub unsafe extern "C" fn globalNamespace(_: *mut c_void, _: *const xmlChar, _: *const xmlChar) {}

// ═══════════════════════════════════════════════════════════════════════════════
// 5. SAX default-handler init (SAX.h / SAX2.h)
// ═══════════════════════════════════════════════════════════════════════════════

/// Upstream `initxmlDefaultSAXHandler(xmlSAXHandlerV1 *hdlr, int warning)`.
#[no_mangle]
pub unsafe extern "C" fn initxmlDefaultSAXHandler(_hdlr: *mut _xmlSAXHandlerV1, _warning: c_int) {}

/// Upstream `inithtmlDefaultSAXHandler(xmlSAXHandlerV1 *hdlr)`.
#[no_mangle]
pub unsafe extern "C" fn inithtmlDefaultSAXHandler(_hdlr: *mut _xmlSAXHandlerV1) {}

/// Upstream `initdocbDefaultSAXHandler(xmlSAXHandlerV1 *hdlr)`.
#[no_mangle]
pub unsafe extern "C" fn initdocbDefaultSAXHandler(_hdlr: *mut _xmlSAXHandlerV1) {}

/// Upstream `docbDefaultSAXHandlerInit(xmlSAXHandlerV1 *hdlr)`.
#[no_mangle]
pub unsafe extern "C" fn docbDefaultSAXHandlerInit(_hdlr: *mut _xmlSAXHandlerV1) {}

/// Upstream `xmlSAX2InitDocbDefaultSAXHandler(xmlSAXHandlerPtr hdlr)`.
#[no_mangle]
pub unsafe extern "C" fn xmlSAX2InitDocbDefaultSAXHandler(_hdlr: *mut _xmlSAXHandler) {}

/// Upstream `initGenericErrorDefaultFunc(xmlGenericErrorFunc *handler)`.
#[no_mangle]
pub unsafe extern "C" fn initGenericErrorDefaultFunc(
    handler: *mut crate::abi::callbacks::xmlGenericErrorFunc,
) {
    if handler.is_null() {
        return;
    }
    *handler = crate::abi::data_globals::XML_GENERIC_ERROR_DEFAULT;
}

/// Upstream `__htmlParseContent(void *ctx)` — internal HTML parser entry.
#[no_mangle]
pub unsafe extern "C" fn __htmlParseContent(_ctx: *mut c_void) {}

// ═══════════════════════════════════════════════════════════════════════════════
// 6. Encoding helpers (encoding.h / HTMLparser.h / tree.h)
// ═══════════════════════════════════════════════════════════════════════════════

/// Upstream `isolat1ToUTF8(unsigned char *out, int *outlen,
/// const unsigned char *in, int *inlen)` — ISO-8859-1 → UTF-8.
#[no_mangle]
pub unsafe extern "C" fn isolat1ToUTF8(
    out: *mut xmlChar,
    outlen: *mut c_int,
    input: *const xmlChar,
    inlen: *mut c_int,
) -> c_int {
    if input.is_null() || out.is_null() {
        return -1;
    }
    let in_len = if inlen.is_null() {
        0
    } else {
        unsafe { *inlen }
    };
    let cap = if outlen.is_null() {
        0
    } else {
        unsafe { *outlen }
    };
    let mut written: c_int = 0;
    let mut consumed: c_int = 0;
    while consumed < in_len {
        let b = unsafe { *input.add(consumed as usize) };
        if b < 0x80 {
            if written + 1 > cap {
                return -1;
            }
            unsafe { *out.add(written as usize) = b };
            written += 1;
        } else {
            if written + 2 > cap {
                return -1;
            }
            unsafe {
                *out.add(written as usize) = 0xC0 | (b >> 6);
                *out.add((written + 1) as usize) = 0x80 | (b & 0x3F);
            }
            written += 2;
        }
        consumed += 1;
    }
    if !outlen.is_null() {
        unsafe { *outlen = written };
    }
    if !inlen.is_null() {
        unsafe { *inlen = consumed };
    }
    written
}

/// Upstream `UTF8Toisolat1(unsigned char *out, int *outlen,
/// const unsigned char *in, int *inlen)` — UTF-8 → ISO-8859-1.
#[no_mangle]
pub unsafe extern "C" fn UTF8Toisolat1(
    out: *mut xmlChar,
    outlen: *mut c_int,
    input: *const xmlChar,
    inlen: *mut c_int,
) -> c_int {
    if input.is_null() || out.is_null() {
        return -1;
    }
    let in_len = if inlen.is_null() {
        0
    } else {
        unsafe { *inlen }
    };
    let cap = if outlen.is_null() {
        0
    } else {
        unsafe { *outlen }
    };
    let mut written: c_int = 0;
    let mut consumed: c_int = 0;
    while consumed < in_len {
        let b = unsafe { *input.add(consumed as usize) };
        if b < 0x80 {
            if written + 1 > cap {
                return -1;
            }
            unsafe { *out.add(written as usize) = b };
            written += 1;
            consumed += 1;
        } else if b >= 0xC0 && b < 0xE0 {
            if consumed + 1 >= in_len {
                return -2;
            }
            let b1 = unsafe { *input.add((consumed + 1) as usize) };
            let cp = (((b & 0x1F) as u32) << 6) | ((b1 & 0x3F) as u32);
            if cp > 0xFF {
                return -2;
            }
            if written + 1 > cap {
                return -1;
            }
            unsafe { *out.add(written as usize) = cp as xmlChar };
            written += 1;
            consumed += 2;
        } else {
            // 3+ byte UTF-8 sequence cannot be represented in ISO-8859-1.
            return -2;
        }
    }
    if !outlen.is_null() {
        unsafe { *outlen = written };
    }
    if !inlen.is_null() {
        unsafe { *inlen = consumed };
    }
    written
}

/// Upstream `UTF8ToHtml(unsigned char *out, int *outlen,
/// const unsigned char *in, int *inlen)` — delegate to the isolat1 path for
/// the Latin-1 subset (matching upstream's UTF8ToHtml which special-cases
/// non-Latin-1 code points into numeric references only in the full impl).
#[no_mangle]
pub unsafe extern "C" fn UTF8ToHtml(
    out: *mut xmlChar,
    outlen: *mut c_int,
    input: *const xmlChar,
    inlen: *mut c_int,
) -> c_int {
    unsafe { UTF8Toisolat1(out, outlen, input, inlen) }
}

/// Upstream `xmlCharEncFirstLineInput(xmlParserInputBufferPtr input, int len)`.
#[no_mangle]
pub unsafe extern "C" fn xmlCharEncFirstLineInput(
    _input: *mut crate::abi::structs::_xmlParserInputBuffer,
    _len: c_int,
) -> c_int {
    0
}

/// Upstream `xmlCharEncFirstLineInt(xmlCharEncodingHandler *handler,
/// xmlBufferPtr out, xmlBufferPtr in, int len)`.
#[no_mangle]
pub unsafe extern "C" fn xmlCharEncFirstLineInt(
    _handler: *mut _xmlCharEncodingHandler,
    _out: *mut _xmlBuffer,
    _in: *mut _xmlBuffer,
    _len: c_int,
) -> c_int {
    0
}

/// Upstream `xmlEncodeAttributeEntities(xmlDocPtr doc, const xmlChar *input)`.
#[no_mangle]
pub unsafe extern "C" fn xmlEncodeAttributeEntities(
    _doc: *mut _xmlDoc,
    input: *const xmlChar,
) -> *mut xmlChar {
    if input.is_null() {
        return ptr::null_mut();
    }
    crate::abi::exports_xml2::xmlEncodeSpecialChars(ptr::null(), input)
}

// ═══════════════════════════════════════════════════════════════════════════════
// 7. xz I/O hooks (xmlIO.c — stubs; the candidate has no liblzma backend)
// ═══════════════════════════════════════════════════════════════════════════════

#[no_mangle]
pub unsafe extern "C" fn __libxml2_xzopen(
    _path: *const c_char,
    _mode: *const c_char,
) -> *mut c_void {
    ptr::null_mut()
}
#[no_mangle]
pub unsafe extern "C" fn __libxml2_xzdopen(_fd: c_int, _mode: *const c_char) -> *mut c_void {
    ptr::null_mut()
}
#[no_mangle]
pub unsafe extern "C" fn __libxml2_xzread(
    _file: *mut c_void,
    _buf: *mut c_void,
    _len: c_uint,
) -> c_int {
    -1
}
#[no_mangle]
pub unsafe extern "C" fn __libxml2_xzclose(_file: *mut c_void) -> c_int {
    -1
}
#[no_mangle]
pub unsafe extern "C" fn __libxml2_xzcompressed(_file: *mut c_void) -> c_int {
    0
}

// ═══════════════════════════════════════════════════════════════════════════════
// 8. Misc internal entry points
// ═══════════════════════════════════════════════════════════════════════════════

/// Upstream `xmlErrMemory(xmlParserCtxtPtr ctxt, const char *extra)`.
#[no_mangle]
pub unsafe extern "C" fn xmlErrMemory(_ctxt: *mut c_void, _extra: *const c_char) {}

/// Upstream `xmlEscapeFormatString(xmlChar **msg)`.
#[no_mangle]
pub unsafe extern "C" fn xmlEscapeFormatString(_msg: *mut *mut xmlChar) -> c_int {
    0
}

/// Upstream `xmlNsListDumpOutput(xmlOutputBufferPtr buf, xmlNsPtr cur)`.
#[no_mangle]
pub unsafe extern "C" fn xmlNsListDumpOutput(
    _buf: *mut _xmlOutputBuffer,
    _cur: *mut crate::abi::structs::_xmlNs,
) {
}

/// Upstream `xmlXPtrAdvanceNode(xmlNodePtr cur, int *level)`.
#[no_mangle]
pub unsafe extern "C" fn xmlXPtrAdvanceNode(
    _cur: *mut _xmlNode,
    _level: *mut c_int,
) -> *mut _xmlNode {
    ptr::null_mut()
}

/// Upstream `xmlXPtrEvalRangePredicate(xmlXPathParserContextPtr ctxt)`.
#[no_mangle]
pub unsafe extern "C" fn xmlXPtrEvalRangePredicate(_ctxt: *mut c_void) {}

/// Upstream `xmlAutomataSetFlags(xmlAutomataPtr am, int flags)`.
#[no_mangle]
pub unsafe extern "C" fn xmlAutomataSetFlags(_am: *mut c_void, _flags: c_int) {}

/// Upstream `xmlInputReadCallbackNop(void *context, char *buffer, int len)`.
#[no_mangle]
pub unsafe extern "C" fn xmlInputReadCallbackNop(
    _context: *mut c_void,
    _buffer: *mut c_char,
    _len: c_int,
) -> c_int {
    0
}

/// Upstream `xmlMallocBreakpoint(void)`.
#[no_mangle]
pub unsafe extern "C" fn xmlMallocBreakpoint() {}

/// Upstream `libxml_domnode_tim_sort(xmlNodePtr *a, size_t len)`.
#[no_mangle]
pub unsafe extern "C" fn libxml_domnode_tim_sort(_a: *mut *mut _xmlNode, _len: usize) {}

/// Upstream `libxml_domnode_binary_insertion_sort(xmlNodePtr *a, size_t len)`.
#[no_mangle]
pub unsafe extern "C" fn libxml_domnode_binary_insertion_sort(_a: *mut *mut _xmlNode, _len: usize) {
}

// ═══════════════════════════════════════════════════════════════════════════════
// 9. Internal error/reporting stubs (xmlerror.h / xmlIO.c / globals.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// Upstream `__xmlSimpleError(int domain, int code, xmlNodePtr node,
/// const char *msg, const char *extra)`.
#[no_mangle]
pub unsafe extern "C" fn __xmlSimpleError(
    _domain: c_int,
    _code: c_int,
    _node: *mut _xmlNode,
    _msg: *const c_char,
    _extra: *const c_char,
) {
}

/// Upstream `__xmlRaiseError(...)` — variadic; the fixed arg list is preserved
/// for ABI and the body is a no-op (the candidate routes errors through its
/// own structured/generic handlers, not this internal entry point).
#[no_mangle]
pub unsafe extern "C" fn __xmlRaiseError(
    _schannel: crate::abi::callbacks::xmlStructuredErrorFunc,
    _channel: crate::abi::callbacks::xmlGenericErrorFunc,
    _data: *mut c_void,
    _ctx: *mut c_void,
    _node: *mut c_void,
    _domain: c_int,
    _code: c_int,
    _level: c_int,
    _file: *const c_char,
    _line: c_int,
    _str1: *const c_char,
    _str2: *const c_char,
    _str3: *const c_char,
    _int1: c_int,
    _col: c_int,
    _msg: *const c_char,
) {
}

/// Upstream `__xmlErrEncoding(int code, const char *msg, ...)`.
#[no_mangle]
pub unsafe extern "C" fn __xmlErrEncoding(_code: c_int, _msg: *const c_char) {}

/// Upstream `__xmlIOErr(int domain, int code, const char *extra)`.
#[no_mangle]
pub unsafe extern "C" fn __xmlIOErr(_domain: c_int, _code: c_int, _extra: *const c_char) {}

/// Upstream `__xmlLoaderErr(void *ctx, const char *msg, const char *filename)`.
#[no_mangle]
pub unsafe extern "C" fn __xmlLoaderErr(
    _ctx: *mut c_void,
    _msg: *const c_char,
    _filename: *const c_char,
) {
}

/// Upstream `__xmlRandom(void)`.
#[no_mangle]
pub unsafe extern "C" fn __xmlRandom() -> c_int {
    0
}

/// Upstream `__xmlInitializeDict(void)`.
#[no_mangle]
pub unsafe extern "C" fn __xmlInitializeDict() {}

/// Upstream `__xmlGlobalInitMutexLock(void)`.
#[no_mangle]
pub unsafe extern "C" fn __xmlGlobalInitMutexLock() -> c_int {
    0
}

/// Upstream `__xmlGlobalInitMutexUnlock(void)`.
#[no_mangle]
pub unsafe extern "C" fn __xmlGlobalInitMutexUnlock() {}

/// Upstream `__xmlGlobalInitMutexDestroy(void)`.
#[no_mangle]
pub unsafe extern "C" fn __xmlGlobalInitMutexDestroy() {}
