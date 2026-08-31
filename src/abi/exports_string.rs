//! exports_string — xmlStr*/xmlUTF8*/xmlString* C ABI family (§11.1-I).
//!
//! Completes the string family that `exports_xml2.rs` does not already
//! provide, with exact upstream signatures:
//!
//! - xmlstring.h: `xmlStrPrintf`, `xmlStrVPrintf`, `xmlStrcasestr`,
//!   `xmlStrstr`, `xmlUTF8Charcmp`, `xmlUTF8Strloc`, `xmlUTF8Strndup`,
//!   `xmlUTF8Strpos`, `xmlUTF8Strsize`, `xmlUTF8Strsub`
//! - parserInternals.h: `xmlStringCurrentChar`, `xmlStringDecodeEntities`,
//!   `xmlStringLenDecodeEntities`
//! - tree.h: `xmlStringLenGetNodeList`
//!
//! Semantics follow archaeology/libxml2-git (xmlstring.c,
//! parserInternals.c, parser.c, tree.c). `xmlStrPrintf` is variadic in C,
//! which stable Rust cannot express (`c_variadic` is unstable); it is
//! provided through the same inline-assembly forwarder used by the writer
//! module's `xmlTextWriterWriteFormat*` exports (see
//! `src/xml/writer/mod.rs`, `format_shims`).
//!
//! # Upstream contract
//!
//! Parity target is upstream `xmlstring.c` (libxml2 2.15.3,
//! SRC-LIBXML2-2.15.0-XMLSTRING-C) plus the `xmlstring.h`/`parserInternals.h`/
//! `tree.h` signatures; R-000165 closed the string export gaps (the string
//! family here completes the rest).
//!
//! # Conceptual behavior
//!
//! This module implements the string-family ABI: printf-style construction
//! (`xmlStrPrintf`/`xmlStrVPrintf` — the former variadic, provided through the
//! same inline-assembly forwarder as the writers `xmlTextWriterWriteFormat*`,
//! R-000155 pattern), substring and case search, UTF-8 position/size/char
//! helpers, and the entity-decoding string entry points (`xmlStringCurrentChar`,
//! `xmlStringDecodeEntities`, `xmlStringLenDecodeEntities`).
//!
//! # Ownership & safety invariants
//!
//! All returned strings are xml-allocator allocations the caller frees with
//! `xmlFree` (OWNERSHIP_ATLAS section 3); `xmlStrVPrintf` output is
//! caller-freed. The decode-entities entry points take a live `xmlParserCtxt*`
//! (or NULL) and return fresh strings; input buffers must be readable per the
//! documented SAFETY sections.
//!
//! # Historical quirks & epochs
//!
//! The decode-entities API is the 2.0-era entity path that the modern parser
//! no longer uses for its main flow; SECURITY_HISTORY section 5.3 records that
//! `xmlStringDecodeEntities`/`xmlStringLenDecodeEntities` are a documented
//! simplified port (the depth-20/XML_ENT_EXPANDING guards exist but errors are
//! silent — the main parser path carries the full semantics). R-000165
//! (11.1-O) added the string gaps.
//!
//! # Deliberate oddities
//!
//! The silent-error simplification of the decode-entities port is the
//! deliberate oddity here (fidelity note in SECURITY_HISTORY 5.3); the
//! git-version contract (NULL on `str[len] != 0` or any non-zero end marker)
//! is reproduced verbatim.
//!
//! # Proving courts
//!
//! The DSO-LOADER and HEADER-COMPILE courts plus the string
//! unit tests under cargo test cover this module; the ENCODING-001 probe
//! exercises the UTF-8 helpers against the oracle.
//!
//! # Tempting simplifications that would break parity
//!
//! A tempting simplification is to make the decode-entities entry points raise
//! errors through the full parser path — the upstream API here is silent
//! (fidelity note), so adding errors would change observable behavior;
//! conversely, dropping the depth/expansion guards entirely would reintroduce
//! the entity-expansion class that SEC-0006 (CVE-2014-3660) bounded. Both
//! simplifications must not be applied.

#![allow(
    missing_docs,
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals
)]
#![allow(unused_variables)]
#![allow(private_interfaces)]
#![allow(clippy::missing_safety_doc)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]

// SAFETY-SCOPE: EXPORT-STRING-MECHANICAL-001
// (11.1-Z.3 proof scope, classified-generated) — this module is the
// mechanical extern-"C" export surface: every `unsafe` block in it is
// the documented indirection/registry-access pattern whose validity
// rests on the upstream C contract, and the exported signatures are
// machine-measured by the ABI-FUNCTION-SIGNATURE and DSO-LOADER
// courts and the C-API differential probes. The safety contract of
// each export is stated in its own doc comment; this scope covers the
// mechanical wrappers' unsafe blocks.

use core::ffi::c_void;
use core::ptr;
use std::mem::size_of;
use std::os::raw::{c_char, c_int, c_uint};
use std::slice;

use crate::abi::allocator::{xmlFreeImpl, xmlMallocImpl, xmlMallocZero};
use crate::abi::structs::{_xmlDoc, _xmlEntity, _xmlNode, _xmlParserCtxt};
use crate::abi::types::xmlChar;
use crate::abi::types::xmlElementType::*;
use crate::abi::types::xmlEntityType::*;
use crate::xml::entities::get_entity;
use crate::xml::string::{utf8_size, xml_strdup, xml_strlen, xml_strndup};
use crate::xml::tree::{free_node_list, get_doc_entity, new_text};

// ═══════════════════════════════════════════════════════════════════════════════
// Shared internal helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Upstream `xmlStrncmp` (xmlstring.c): length-limited byte comparison,
/// NULL-aware (NULL sorts before any non-NULL string; equal pointers are
/// equal).
///
/// # SAFETY
///
/// - `str1`/`str2` must be valid pointers or NULL; only `len` bytes are
///   read from each.
unsafe fn xml_strncmp(str1: *const xmlChar, str2: *const xmlChar, len: c_int) -> c_int {
    if len <= 0 {
        return 0;
    }
    if str1 == str2 {
        return 0;
    }
    if str1.is_null() {
        return -1;
    }
    if str2.is_null() {
        return 1;
    }
    unsafe { libc::strncmp(str1 as *const c_char, str2 as *const c_char, len as usize) as c_int }
}

/// Upstream `xmlGetUTF8Char` (xmlstring.c): decode the UTF-8 character
/// starting at `utf`; sets `*len` to the number of bytes consumed and
/// returns the code point, or -1 (with `*len = 0`) on error.
///
/// # SAFETY
///
/// - `utf` must point to at least `*len` readable bytes (NUL-terminated
///   buffers pass `*len = 4`).
/// - `len` must be a valid `int*`.
unsafe fn get_utf8_char(utf: *const xmlChar, len: *mut c_int) -> c_int {
    if utf.is_null() || len.is_null() {
        if !len.is_null() {
            *len = 0;
        }
        return -1;
    }
    unsafe {
        let mut c: u32 = *utf as u32;
        if c < 0x80 {
            if *len < 1 {
                *len = 0;
                return -1;
            }
            *len = 1;
        } else {
            if (*len < 2) || ((*utf.add(1) & 0xc0) != 0x80) {
                *len = 0;
                return -1;
            }
            if c < 0xe0 {
                if c < 0xc2 {
                    *len = 0;
                    return -1;
                }
                /* 2-byte code */
                *len = 2;
                c = (c & 0x1f) << 6;
                c |= (*utf.add(1) & 0x3f) as u32;
            } else {
                if (*len < 3) || ((*utf.add(2) & 0xc0) != 0x80) {
                    *len = 0;
                    return -1;
                }
                if c < 0xf0 {
                    /* 3-byte code */
                    *len = 3;
                    c = (c & 0xf) << 12;
                    c |= ((*utf.add(1) & 0x3f) as u32) << 6;
                    c |= (*utf.add(2) & 0x3f) as u32;
                    if (c < 0x800) || (0xd800..0xe000).contains(&c) {
                        *len = 0;
                        return -1;
                    }
                } else {
                    if (*len < 4) || ((*utf.add(3) & 0xc0) != 0x80) {
                        *len = 0;
                        return -1;
                    }
                    /* 4-byte code */
                    *len = 4;
                    c = (c & 0x7) << 18;
                    c |= ((*utf.add(1) & 0x3f) as u32) << 12;
                    c |= ((*utf.add(2) & 0x3f) as u32) << 6;
                    c |= (*utf.add(3) & 0x3f) as u32;
                    if !(0x10000..0x110000).contains(&c) {
                        *len = 0;
                        return -1;
                    }
                }
            }
        }
        c as c_int
    }
}

/// Upstream `xmlUTF8Strsize` (xmlstring.c): byte size of the first `len`
/// UTF-8 characters of `utf`; returns 0 for NULL/`len <= 0` and stops at
/// the end of the string.
///
/// # SAFETY
///
/// - `utf` must be a valid null-terminated byte string or NULL.
const unsafe fn utf8_strsize(utf: *const xmlChar, len: c_int) -> c_int {
    if utf.is_null() || len <= 0 {
        return 0;
    }
    unsafe {
        let mut ptr = utf;
        let mut n = len;
        while n > 0 {
            if *ptr == 0 {
                break;
            }
            let mut ch = *ptr;
            ptr = ptr.add(1);
            if (ch & 0x80) != 0 {
                loop {
                    ch <<= 1;
                    if (ch & 0x80) == 0 {
                        break;
                    }
                    if *ptr == 0 {
                        break;
                    }
                    ptr = ptr.add(1);
                }
            }
            n -= 1;
        }
        let ret = ptr.offset_from(utf) as usize;
        if ret > c_int::MAX as usize {
            0
        } else {
            ret as c_int
        }
    }
}

/// Encode a Unicode code point as UTF-8 and append it to `out` (upstream
/// `xmlCopyCharMultiByte`).
fn utf8_encode_char(out: &mut Vec<u8>, val: u32) {
    if val < 0x80 {
        out.push(val as u8);
    } else if val < 0x800 {
        out.push(0xC0 | ((val >> 6) as u8));
        out.push(0x80 | ((val & 0x3F) as u8));
    } else if val < 0x10000 {
        out.push(0xE0 | ((val >> 12) as u8));
        out.push(0x80 | (((val >> 6) & 0x3F) as u8));
        out.push(0x80 | ((val & 0x3F) as u8));
    } else if val < 0x110000 {
        out.push(0xF0 | ((val >> 18) as u8));
        out.push(0x80 | (((val >> 12) & 0x3F) as u8));
        out.push(0x80 | (((val >> 6) & 0x3F) as u8));
        out.push(0x80 | ((val & 0x3F) as u8));
    }
}

/// `IS_CHAR` (chvalid.h): XML [2] Char production.
#[inline]
fn is_xml_char(c: u32) -> bool {
    c == 0x9
        || c == 0xA
        || c == 0xD
        || (0x20..=0xD7FF).contains(&c)
        || (0xE000..=0xFFFD).contains(&c)
        || (0x10000..=0x10FFFF).contains(&c)
}

/// Content of the five predefined entities (upstream `xmlGetPredefinedEntity`).
const fn predefined_entity_content(name: *const xmlChar) -> Option<&'static [u8]> {
    if name.is_null() {
        return None;
    }
    // SAFETY: the caller passes a NUL-terminated name.
    let bytes = unsafe { slice::from_raw_parts(name, xml_strlen(name)) };
    match bytes {
        b"lt" => Some(b"<"),
        b"gt" => Some(b">"),
        b"amp" => Some(b"&"),
        b"quot" => Some(b"\""),
        b"apos" => Some(b"'"),
        _ => None,
    }
}

/// Upstream `xmlParseStringCharRef` (parser.c): parse `&#NN;` / `&#xHH;`
/// at `*str`, advancing `*str` past the reference. Returns the code point,
/// or 0 on error (upstream reports the error through the parser context
/// and returns 0; the error callback is not replicated here).
///
/// # SAFETY
///
/// - `str` must point to a valid `*const xmlChar` into a NUL-terminated
///   string.
unsafe fn parse_string_char_ref(str: &mut *const xmlChar) -> u32 {
    unsafe {
        let ptr = *str;
        if ptr.is_null() || *ptr != b'&' {
            return 0;
        }
        if *ptr.add(1) != b'#' {
            return 0;
        }
        if *ptr.add(2) == b'x' {
            /* hex: &#xHH; */
            let mut p = ptr.add(3);
            let mut cur = *p;
            let mut val: u32 = 0;
            while cur != b';' {
                let digit = match cur {
                    b'0'..=b'9' => (cur - b'0') as u32,
                    b'a'..=b'f' => (cur - b'a' + 10) as u32,
                    b'A'..=b'F' => (cur - b'A' + 10) as u32,
                    _ => {
                        val = 0;
                        break;
                    }
                };
                val = val.wrapping_mul(16).wrapping_add(digit);
                if val > 0x110000 {
                    val = 0x110000;
                }
                p = p.add(1);
                cur = *p;
            }
            if cur == b';' {
                p = p.add(1);
            }
            *str = p;
            if val >= 0x110000 || !is_xml_char(val) {
                return 0;
            }
            val
        } else {
            /* decimal: &#NN; */
            let mut p = ptr.add(2);
            let mut cur = *p;
            let mut val: u32 = 0;
            while cur != b';' {
                if !cur.is_ascii_digit() {
                    val = 0;
                    break;
                }
                val = val.wrapping_mul(10).wrapping_add((cur - b'0') as u32);
                if val > 0x110000 {
                    val = 0x110000;
                }
                p = p.add(1);
                cur = *p;
            }
            if cur == b';' {
                p = p.add(1);
            }
            *str = p;
            if val >= 0x110000 || !is_xml_char(val) {
                return 0;
            }
            val
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlStrPrintf / xmlStrVPrintf (xmlstring.h)
// ═══════════════════════════════════════════════════════════════════════════════

/// The System V AMD64 `__va_list_tag` (24 bytes): gp_offset, fp_offset,
/// overflow_arg_area, reg_save_area. A C `va_list` parameter decays to a
/// pointer to this structure, which is exactly what the VFormat exports and
/// the Format shims exchange (same layout as src/xml/writer/mod.rs).
#[repr(C)]
#[derive(Clone, Copy)]
struct VaListTag {
    gp_offset: c_uint,
    fp_offset: c_uint,
    overflow_arg_area: *mut c_void,
    reg_save_area: *mut c_void,
}

// The platform `vsnprintf` (system libc — not an oracle dependency).
unsafe extern "C" {
    fn vsnprintf(s: *mut c_char, n: usize, format: *const c_char, ap: *mut VaListTag) -> c_int;
}

/// Format `msg` and place the result into `buf` (upstream xmlstring.c
/// `xmlStrVPrintf`).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlStrVPrintf(xmlChar *buf, int len, const char *msg, va_list ap);
/// ```
///
/// The C `va_list` (SysV AMD64 `__va_list_tag[1]`) decays to a pointer to
/// the tag struct, hence the `*mut VaListTag` parameter. Returns the number
/// of characters that would have been written had `buf` been large enough,
/// or -1 when `buf`/`msg` is NULL or `len <= 0`. As upstream, `buf[len-1]`
/// is always zeroed ("be safe !").
///
/// # SAFETY
///
/// - `buf` must point to a writable buffer of at least `len` bytes.
/// - `msg` must be a valid printf format string.
/// - `ap` must point to a valid `va_list` matching `msg`'s specifiers.
#[no_mangle]
pub unsafe extern "C" fn xmlStrVPrintf(
    buf: *mut xmlChar,
    len: c_int,
    msg: *const c_char,
    ap: *mut VaListTag,
) -> c_int {
    if buf.is_null() || msg.is_null() || len <= 0 {
        return -1;
    }
    let ret = unsafe { vsnprintf(buf as *mut c_char, len as usize, msg, ap) };
    unsafe {
        *buf.add(len as usize - 1) = 0; /* be safe ! */
    }
    ret
}

/// Assembly shim for the variadic `xmlStrPrintf` export.
///
/// Stable Rust cannot define variadic `extern "C"` functions (c_variadic is
/// unstable), so this `#[no_mangle]` export is a `noreturn` inline-asm block
/// that captures the SysV x86-64 register save area exactly like `va_start`,
/// builds a `va_list` and forwards it to `xmlStrVPrintf`, then restores the
/// stack and returns directly. Same technique as the writer module's
/// `vfmt_shim!` (see `src/xml/writer/mod.rs`).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlStrPrintf(xmlChar *buf, int len, const char *msg, ...);
/// ```
///
/// Three fixed arguments (buf, len, msg) precede the varargs, so the
/// `va_list` is built with `gp_offset = 24` and passed as the fourth
/// argument (register `rcx`).
///
/// Layout: reg_save_area = rsp+0 (6 GP + 8 SSE slots, 176 bytes); the
/// va_list struct lives at rsp+176 (gp_offset, fp_offset,
/// overflow_arg_area, reg_save_area); overflow varargs are above the
/// return address. LLVM emits an 8-byte alignment `push` before the block,
/// so a 240-byte frame (≡ 0 mod 16) keeps the `call` 16-aligned, the
/// overflow area points at rsp+256 (= entry_rsp + 8) and the alignment
/// push is popped before `ret`.
///
/// # SAFETY
///
/// - Must only be called from C with `(xmlChar*, int, const char*, ...)`
///   arguments matching the format string.
#[cfg(target_arch = "x86_64")]
#[no_mangle]
pub unsafe extern "C" fn xmlStrPrintf() -> c_int {
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
            "mov dword ptr [rsp+176], 24",
            "mov dword ptr [rsp+180], 48",
            "lea rax, [rsp+256]",
            "mov [rsp+184], rax",
            "lea rax, [rsp]",
            "mov [rsp+192], rax",
            "lea rcx, [rsp+176]",
            "call xmlStrVPrintf",
            "add rsp, 240",
            "add rsp, 8",
            "ret",
            options(noreturn),
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlStrstr / xmlStrcasestr (xmlstring.h)
// ═══════════════════════════════════════════════════════════════════════════════

/// Find the first occurrence of `val` in `str` (upstream xmlstring.c
/// `xmlStrstr`).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// const xmlChar *xmlStrstr(const xmlChar *str, const xmlChar *val);
/// ```
///
/// Returns a pointer to the first occurrence, `str` itself when `val` is
/// empty, or NULL when not found / either argument is NULL.
///
/// # SAFETY
///
/// - `str` and `val` must be valid null-terminated byte strings or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlStrstr(str: *const xmlChar, val: *const xmlChar) -> *const xmlChar {
    if str.is_null() || val.is_null() {
        return ptr::null();
    }
    let n = unsafe { xml_strlen(val) };
    if n == 0 {
        return str;
    }
    unsafe {
        let mut cur = str;
        while *cur != 0 {
            if *cur == *val && libc::strncmp(cur as *const c_char, val as *const c_char, n) == 0 {
                return cur;
            }
            cur = cur.add(1);
        }
    }
    ptr::null()
}

/// Case-insensitive variant of `xmlStrstr` (upstream xmlstring.c
/// `xmlStrcasestr`).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// const xmlChar *xmlStrcasestr(const xmlChar *str, const xmlChar *val);
/// ```
///
/// Returns a pointer to the first case-insensitive occurrence, `str` itself
/// when `val` is empty, or NULL when not found / either argument is NULL.
/// The upstream `casemap[]` ASCII fold is matched by `tolower`/`strncasecmp`
/// in the C locale.
///
/// # SAFETY
///
/// - `str` and `val` must be valid null-terminated byte strings or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlStrcasestr(str: *const xmlChar, val: *const xmlChar) -> *const xmlChar {
    if str.is_null() || val.is_null() {
        return ptr::null();
    }
    let n = unsafe { xml_strlen(val) };
    if n == 0 {
        return str;
    }
    unsafe {
        let mut cur = str;
        while *cur != 0 {
            if libc::tolower(*cur as c_int) == libc::tolower(*val as c_int)
                && libc::strncasecmp(cur as *const c_char, val as *const c_char, n) == 0
            {
                return cur;
            }
            cur = cur.add(1);
        }
    }
    ptr::null()
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlStringCurrentChar (parserInternals.h)
// ═══════════════════════════════════════════════════════════════════════════════

/// Decode the current character starting at `cur` (upstream
/// parserInternals.c `xmlStringCurrentChar`).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlStringCurrentChar(xmlParserCtxt *ctxt, const xmlChar *cur, int *len);
/// ```
///
/// Returns the character value (as a UCS-4 code point) and sets `*len` to
/// the number of bytes consumed. Returns 0 (with `*len = 0`) on error or
/// NULL arguments. The upstream implementation ignores `ctxt` (it only
/// influences encoding detection, and the candidate is UTF-8 only), so it
/// is unused here as well.
///
/// # SAFETY
///
/// - `cur` must be a valid pointer into a NUL-terminated byte string (a
///   single NUL-terminated buffer suffices; the byte length is probed
///   through `*len`, initialized to 4 as upstream).
/// - `len` must be a valid `int*`.
#[no_mangle]
pub unsafe extern "C" fn xmlStringCurrentChar(
    ctxt: *mut _xmlParserCtxt,
    cur: *const xmlChar,
    len: *mut c_int,
) -> c_int {
    if cur.is_null() || len.is_null() {
        return 0;
    }
    unsafe {
        /* cur is zero-terminated, so we can lie about its length. */
        *len = 4;
        let c = get_utf8_char(cur, len);
        if c < 0 {
            0
        } else {
            c
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlStringDecodeEntities / xmlStringLenDecodeEntities (parserInternals.h)
// ═══════════════════════════════════════════════════════════════════════════════

/// Port of upstream `xmlExpandEntityInAttValue` (parser.c) restricted to
/// the `normalize == 0` path taken by the two decode-entities exports.
///
/// This is a faithful simplified port: numeric character references
/// (`&#NN;` / `&#xHH;`), the five predefined entities and general entities
/// declared in `doc`'s DTD are expanded (recursively, with the upstream
/// depth limit and `XML_ENT_EXPANDING` loop detection). Deviations from the
/// full upstream machinery:
///
/// - errors that upstream reports through the parser context are handled
///   silently (undeclared entity references are dropped, malformed
///   references stop decoding, exactly like upstream, but no error
///   callback fires);
/// - entity resolution uses `doc` (the caller's `ctxt->myDoc`) directly
///   instead of the SAX `getEntity` hook chain.
///
/// # SAFETY
///
/// - `str` must be a valid NUL-terminated string (which the exported
///   entry points guarantee for the `len`-bounded variant).
unsafe fn expand_entity_into(
    doc: *mut _xmlDoc,
    out: &mut Vec<u8>,
    mut str: *const xmlChar,
    depth: c_int,
    pent: *mut _xmlEntity,
) {
    let depth = depth + 1;
    if depth > 20 {
        /* upstream: XML_ERR_RESOURCE_LIMIT "Maximum entity nesting depth exceeded" */
        return;
    }
    if !pent.is_null() && ((*pent).flags & XML_ENT_EXPANDING) != 0 {
        /* upstream: XML_ERR_ENTITY_LOOP */
        return;
    }

    let mut chunk: *const xmlChar = str;
    'scan: loop {
        if *str == 0 {
            break 'scan;
        }
        let c = *str;
        if c != b'&' {
            /*
             * If this function is called without an entity, it is used to
             * expand entities in attribute content where '<' was already
             * unescaped and is allowed; inside entity content it is not.
             */
            if !pent.is_null() && c == b'<' {
                /* upstream: fatal error + break; the chunk accumulated
                 * before '<' is still flushed by the tail below. */
                break 'scan;
            }
            if c < 0x20 {
                /* whitespace is converted to space (normalize == 0) */
                if chunk != str {
                    out.extend_from_slice(slice::from_raw_parts(
                        chunk,
                        str.offset_from(chunk) as usize,
                    ));
                }
                out.push(b' ');
                chunk = str.add(1);
            }
            /* c == 0x20 is kept inside the chunk */
            str = str.add(1);
        } else if *str.add(1) == b'#' {
            /* numeric character reference */
            if chunk != str {
                out.extend_from_slice(slice::from_raw_parts(
                    chunk,
                    str.offset_from(chunk) as usize,
                ));
            }
            let val = parse_string_char_ref(&mut str);
            if val == 0 {
                /* upstream: invalid reference -> stop, return the prefix */
                chunk = str;
                break 'scan;
            }
            if val == b' ' as u32 {
                out.push(b' ');
            } else {
                utf8_encode_char(out, val);
            }
            chunk = str;
        } else {
            /* named entity reference */
            if chunk != str {
                out.extend_from_slice(slice::from_raw_parts(
                    chunk,
                    str.offset_from(chunk) as usize,
                ));
            }
            str = str.add(1);
            let name_start = str;
            while *str != 0 && *str != b';' {
                str = str.add(1);
            }
            if *str != b';' {
                /* upstream: XML_ERR_ENTITYREF_SEMICOL_MISSING -> stop */
                chunk = str;
                break 'scan;
            }
            let name = xml_strndup(name_start, str.offset_from(name_start) as usize);
            if name.is_null() {
                chunk = str;
                break 'scan;
            }
            if let Some(content) = predefined_entity_content(name) {
                out.extend_from_slice(content);
            } else {
                let ent = get_entity(doc, name);
                if !ent.is_null() && (*ent).etype == XML_INTERNAL_PREDEFINED_ENTITY as c_int {
                    if (*ent).content.is_null() {
                        /* upstream: fatal "predefined entity has no content" */
                        xmlFreeImpl(name as *mut c_void);
                        chunk = str;
                        break 'scan;
                    }
                    let content = (*ent).content;
                    let clen = xml_strlen(content);
                    out.extend_from_slice(slice::from_raw_parts(content, clen));
                } else if !ent.is_null() && !(*ent).content.is_null() {
                    if !pent.is_null() {
                        (*pent).flags |= XML_ENT_EXPANDING;
                    }
                    expand_entity_into(doc, out, (*ent).content, depth, ent);
                    if !pent.is_null() {
                        (*pent).flags &= !XML_ENT_EXPANDING;
                    }
                }
                /* ent == NULL (undeclared): the reference is dropped */
            }
            xmlFreeImpl(name as *mut c_void);
            str = str.add(1); /* skip ';' */
            chunk = str;
        }
    }
    if chunk != str {
        out.extend_from_slice(slice::from_raw_parts(
            chunk,
            str.offset_from(chunk) as usize,
        ));
    }
}

/// Upstream `xmlExpandEntitiesInAttValue` (parser.c) with `normalize = 0`:
/// expand entity references in a NUL-terminated string into a freshly
/// allocated `xmlChar*` (caller frees with `xmlFree`).
///
/// # SAFETY
///
/// - `str` must be a valid NUL-terminated string.
unsafe fn expand_entities_in_att_value(doc: *mut _xmlDoc, str: *const xmlChar) -> *mut xmlChar {
    let mut out: Vec<u8> = Vec::new();
    expand_entity_into(doc, &mut out, str, 0, ptr::null_mut());
    let p = xmlMallocImpl(out.len() + 1) as *mut xmlChar;
    if p.is_null() {
        return ptr::null_mut();
    }
    if !out.is_empty() {
        ptr::copy_nonoverlapping(out.as_ptr(), p, out.len());
    }
    *p.add(out.len()) = 0;
    p
}

/// Expand general entity references in a string with a known length
/// (upstream parser.c `xmlStringLenDecodeEntities`).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlStringLenDecodeEntities(xmlParserCtxt *ctxt,
///                                     const xmlChar *str, int len,
///                                     int what, xmlChar end,
///                                     xmlChar end2, xmlChar end3);
/// ```
///
/// Returns NULL when `ctxt`/`str` is NULL, `len < 0`, `str[len] != 0`, or
/// any end marker is non-zero (the git-version contract). `what` is
/// ignored, matching upstream where it is marked `ATTRIBUTE_UNUSED`.
/// Otherwise returns a freshly allocated string with references expanded
/// (numeric references and predefined/general entities; see
/// `expand_entity_into` for the simplifications).
///
/// # SAFETY
///
/// - `ctxt` must be a valid `xmlParserCtxt*` or NULL.
/// - `str` must point to a buffer of at least `len + 1` readable bytes
///   with `str[len] == 0` (upstream reads `str[len]` unconditionally).
#[no_mangle]
pub unsafe extern "C" fn xmlStringLenDecodeEntities(
    ctxt: *mut _xmlParserCtxt,
    str: *const xmlChar,
    len: c_int,
    what: c_int,
    end: xmlChar,
    end2: xmlChar,
    end3: xmlChar,
) -> *mut xmlChar {
    if ctxt.is_null() || str.is_null() || len < 0 {
        return ptr::null_mut();
    }
    if unsafe { *str.add(len as usize) } != 0 || end != 0 || end2 != 0 || end3 != 0 {
        return ptr::null_mut();
    }
    unsafe { expand_entities_in_att_value((*ctxt).myDoc, str) }
}

/// Expand general entity references in a NUL-terminated string (upstream
/// parser.c `xmlStringDecodeEntities`, the macro-less variant).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlStringDecodeEntities(xmlParserCtxt *ctxt,
///                                  const xmlChar *str, int what,
///                                  xmlChar end, xmlChar end2,
///                                  xmlChar end3);
/// ```
///
/// Returns NULL when `ctxt`/`str` is NULL or any end marker is non-zero
/// (the git-version contract). `what` is ignored, matching upstream where
/// it is marked `ATTRIBUTE_UNUSED`.
///
/// # SAFETY
///
/// - `ctxt` must be a valid `xmlParserCtxt*` or NULL.
/// - `str` must be a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn xmlStringDecodeEntities(
    ctxt: *mut _xmlParserCtxt,
    str: *const xmlChar,
    what: c_int,
    end: xmlChar,
    end2: xmlChar,
    end3: xmlChar,
) -> *mut xmlChar {
    // SECURITY_HISTORY 5.3 fidelity note: this port keeps the depth-20 /
    // XML_ENT_EXPANDING guards but raises errors silently — deliberate; the
    // main parser path carries the full error semantics.
    if ctxt.is_null() || str.is_null() {
        return ptr::null_mut();
    }
    if end != 0 || end2 != 0 || end3 != 0 {
        return ptr::null_mut();
    }
    unsafe { expand_entities_in_att_value((*ctxt).myDoc, str) }
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlStringLenGetNodeList (tree.h)
// ═══════════════════════════════════════════════════════════════════════════════

/// Entity flags (include/private/entities.h).
const XML_ENT_PARSED: c_int = 1 << 0;
const XML_ENT_EXPANDING: c_int = 1 << 3;

/// Upstream `xmlNewDocText` (tree.c): a text node associated with `doc`
/// (NULL allowed). The dictionary lookup of the name is skipped — names
/// are heap-allocated copies throughout this crate.
///
/// # SAFETY
///
/// - `doc` must be a valid `xmlDoc*` or NULL.
/// - `content` must be a valid NUL-terminated string or NULL.
unsafe fn new_doc_text(doc: *const _xmlDoc, content: *const xmlChar) -> *mut _xmlNode {
    if !doc.is_null() {
        let t = (*doc).type_;
        if t != XML_DOCUMENT_NODE as c_int && t != XML_HTML_DOCUMENT_NODE as c_int {
            return ptr::null_mut();
        }
    }
    let node = new_text(content);
    if node.is_null() {
        return ptr::null_mut();
    }
    if !doc.is_null() {
        (*node).doc = doc as *mut _xmlDoc;
    }
    node
}

/// Upstream `xmlNewEntityReference` (tree.c): an `XML_ENTITY_REF_NODE`
/// carrying the entity's name.
///
/// # SAFETY
///
/// - `doc` must be a valid `xmlDoc*` or NULL.
/// - `name` must be a valid NUL-terminated string.
unsafe fn new_entity_ref(doc: *const _xmlDoc, name: *const xmlChar) -> *mut _xmlNode {
    if name.is_null() {
        return ptr::null_mut();
    }
    if !doc.is_null() {
        let t = (*doc).type_;
        if t != XML_DOCUMENT_NODE as c_int && t != XML_HTML_DOCUMENT_NODE as c_int {
            return ptr::null_mut();
        }
    }
    let node = xmlMallocZero(size_of::<_xmlNode>()) as *mut _xmlNode;
    if node.is_null() {
        return ptr::null_mut();
    }
    let name_copy = xml_strdup(name);
    if name_copy.is_null() {
        xmlFreeImpl(node as *mut c_void);
        return ptr::null_mut();
    }
    unsafe {
        (*node).type_ = XML_ENTITY_REF_NODE as c_int;
        (*node).name = name_copy;
        if !doc.is_null() {
            (*node).doc = doc as *mut _xmlDoc;
        }
    }
    node
}

/// Port of upstream `xmlNodeParseAttValue` (tree.c) for the
/// `xmlStringLenGetNodeList` path: parse an attribute value into a list of
/// text nodes and entity reference nodes. `attr` is the entity whose
/// `children`/`last` receive the parsed list during recursive entity
/// content parsing (NULL for the top-level call). The node list is
/// returned through `list_ptr` (may be NULL); returns 0 on success, -1 on
/// allocation failure.
///
/// # SAFETY
///
/// - `doc` must be a valid `xmlDoc*` or NULL.
/// - `value` must be a valid NUL-terminated string of at least `len` bytes
///   or NULL.
/// - `list_ptr` must be a valid `xmlNode**` or NULL.
unsafe fn node_parse_att_value(
    doc: *const _xmlDoc,
    attr: *mut _xmlNode,
    value: *const xmlChar,
    len: usize,
    list_ptr: *mut *mut _xmlNode,
) -> c_int {
    let mut head: *mut _xmlNode = ptr::null_mut();
    let mut last: *mut _xmlNode = ptr::null_mut();

    if !list_ptr.is_null() {
        *list_ptr = ptr::null_mut();
    }

    if value.is_null() || unsafe { *value } == 0 {
        return 0;
    }

    let mut buf: Vec<u8> = Vec::new();
    let mut cur = value;
    let mut q = cur;
    let mut remaining = len;

    'scan: loop {
        if remaining == 0 || unsafe { *cur } == 0 {
            break 'scan;
        }
        if unsafe { *cur } == b'&' {
            let mut charval: u32 = 0;

            /* Save the current text. */
            if cur != q {
                unsafe {
                    buf.extend_from_slice(slice::from_raw_parts(q, cur.offset_from(q) as usize));
                }
                // `q` is re-established by each reference branch below.
            }

            if remaining > 2 && unsafe { *cur.add(1) } == b'#' && unsafe { *cur.add(2) } == b'x' {
                /* hex character reference */
                let mut tmp: u8 = 0;
                unsafe {
                    cur = cur.add(3);
                }
                remaining -= 3;
                loop {
                    if remaining == 0 {
                        break;
                    }
                    tmp = unsafe { *cur };
                    if tmp == b';' {
                        break;
                    }
                    let digit: u32 = match tmp {
                        b'0'..=b'9' => (tmp - b'0') as u32,
                        b'a'..=b'f' => (tmp - b'a' + 10) as u32,
                        b'A'..=b'F' => (tmp - b'A' + 10) as u32,
                        _ => {
                            charval = 0;
                            break;
                        }
                    };
                    charval = charval.wrapping_mul(16).wrapping_add(digit);
                    if charval > 0x110000 {
                        charval = 0x110000;
                    }
                    unsafe {
                        cur = cur.add(1);
                    }
                    remaining -= 1;
                }
                if tmp == b';' {
                    unsafe {
                        cur = cur.add(1);
                    }
                    remaining -= 1;
                }
                q = cur;
            } else if remaining > 1 && unsafe { *cur.add(1) } == b'#' {
                /* decimal character reference */
                let mut tmp: u8 = 0;
                unsafe {
                    cur = cur.add(2);
                }
                remaining -= 2;
                loop {
                    if remaining == 0 {
                        break;
                    }
                    tmp = unsafe { *cur };
                    if tmp == b';' {
                        break;
                    }
                    if !tmp.is_ascii_digit() {
                        charval = 0;
                        break;
                    }
                    charval = charval.wrapping_mul(10).wrapping_add((tmp - b'0') as u32);
                    if charval > 0x110000 {
                        charval = 0x110000;
                    }
                    unsafe {
                        cur = cur.add(1);
                    }
                    remaining -= 1;
                }
                if tmp == b';' {
                    unsafe {
                        cur = cur.add(1);
                    }
                    remaining -= 1;
                }
                q = cur;
            } else {
                /* read the entity name */
                unsafe {
                    cur = cur.add(1);
                }
                remaining -= 1;
                q = cur;
                while remaining > 0 && unsafe { *cur } != 0 && unsafe { *cur } != b';' {
                    unsafe {
                        cur = cur.add(1);
                    }
                    remaining -= 1;
                }
                if remaining == 0 || unsafe { *cur } == 0 {
                    break 'scan;
                }
                if cur != q {
                    let name = unsafe { xml_strndup(q, cur.offset_from(q) as usize) };
                    if name.is_null() {
                        free_node_list(head);
                        return -1;
                    }
                    let ent = get_doc_entity(doc, name);
                    if !ent.is_null() && (*ent).etype == XML_INTERNAL_PREDEFINED_ENTITY as c_int {
                        /* predefined entities don't generate nodes */
                        let content = (*ent).content;
                        let clen = xml_strlen(content);
                        unsafe {
                            buf.extend_from_slice(slice::from_raw_parts(content, clen));
                        }
                    } else if ent.is_null() || ((*ent).flags & XML_ENT_EXPANDING) == 0 {
                        /* flush the buffer so far */
                        if !buf.is_empty() {
                            buf.push(0); /* NUL-terminate for the text-node dup */
                            let node = new_doc_text(doc, buf.as_ptr() as *const xmlChar);
                            buf.pop();
                            if node.is_null() {
                                xmlFreeImpl(name as *mut c_void);
                                free_node_list(head);
                                return -1;
                            }
                            (*node).parent = attr;
                            if last.is_null() {
                                head = node;
                            } else {
                                (*last).next = node;
                                (*node).prev = last;
                            }
                            last = node;
                            buf.clear();
                        }

                        /* parse the entity content if not parsed yet */
                        if !ent.is_null()
                            && ((*ent).flags & XML_ENT_PARSED) == 0
                            && !(*ent).content.is_null()
                        {
                            (*ent).flags |= XML_ENT_EXPANDING;
                            let res = node_parse_att_value(
                                doc,
                                ent as *mut _xmlNode,
                                (*ent).content,
                                usize::MAX,
                                ptr::null_mut(),
                            );
                            (*ent).flags &= !XML_ENT_EXPANDING;
                            if res < 0 {
                                xmlFreeImpl(name as *mut c_void);
                                free_node_list(head);
                                return -1;
                            }
                            (*ent).flags |= XML_ENT_PARSED;
                        }

                        /* create a new REFERENCE_REF node */
                        let node = new_entity_ref(doc, name);
                        if node.is_null() {
                            xmlFreeImpl(name as *mut c_void);
                            free_node_list(head);
                            return -1;
                        }
                        (*node).parent = attr;
                        (*node).last = ent as *mut _xmlNode;
                        if !ent.is_null() {
                            (*node).children = ent as *mut _xmlNode;
                            (*node).content = (*ent).content;
                        }
                        if last.is_null() {
                            head = node;
                        } else {
                            (*last).next = node;
                            (*node).prev = last;
                        }
                        last = node;
                    }
                    xmlFreeImpl(name as *mut c_void);
                }
                unsafe {
                    cur = cur.add(1);
                }
                remaining -= 1;
                q = cur;
            }
            if charval != 0 {
                let charval = if charval >= 0x110000 { 0xFFFD } else { charval };
                utf8_encode_char(&mut buf, charval);
            }
        } else {
            unsafe {
                cur = cur.add(1);
            }
            remaining -= 1;
        }
    }

    /* handle the last piece of text */
    if cur != q {
        unsafe {
            buf.extend_from_slice(slice::from_raw_parts(q, cur.offset_from(q) as usize));
        }
    }

    if !buf.is_empty() {
        buf.push(0); /* NUL-terminate for the text-node dup */
        let node = new_doc_text(doc, buf.as_ptr() as *const xmlChar);
        buf.pop();
        if node.is_null() {
            free_node_list(head);
            return -1;
        }
        (*node).parent = attr;
        if last.is_null() {
            head = node;
        } else {
            (*last).next = node;
            (*node).prev = last;
        }
        last = node;
    } else if head.is_null() {
        head = new_doc_text(doc, b"" as *const u8 as *const xmlChar);
        if head.is_null() {
            return -1;
        }
        (*head).parent = attr;
        last = head;
    }

    if !attr.is_null() {
        (*attr).children = head;
        (*attr).last = last;
    }
    if !list_ptr.is_null() {
        *list_ptr = head;
    }
    0
}

/// Build a node list (text and entity reference nodes) from an attribute
/// value (upstream tree.c `xmlStringLenGetNodeList`).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNode *xmlStringLenGetNodeList(const xmlDoc *doc,
///                                  const xmlChar *value, int len);
/// ```
///
/// Returns the head of a linked list of `XML_TEXT_NODE` /
/// `XML_ENTITY_REF_NODE` nodes, or NULL for a NULL/empty `value` or on
/// allocation failure. A negative `len` means the value is NUL-terminated.
/// Predefined entity references are expanded into text; other declared
/// entities produce entity reference nodes (whose content is parsed into
/// the entity declaration's children); undeclared references produce
/// entity reference nodes without content, as upstream.
///
/// # SAFETY
///
/// - `doc` must be a valid `xmlDoc*` or NULL.
/// - `value` must be a valid NUL-terminated string of at least `len` bytes
///   or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlStringLenGetNodeList(
    doc: *const _xmlDoc,
    value: *const xmlChar,
    len: c_int,
) -> *mut _xmlNode {
    let max_size: usize = if len < 0 { usize::MAX } else { len as usize };
    let mut ret: *mut _xmlNode = ptr::null_mut();
    unsafe {
        node_parse_att_value(doc, ptr::null_mut(), value, max_size, &mut ret);
    }
    ret
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlUTF8* family (xmlstring.h)
// ═══════════════════════════════════════════════════════════════════════════════

/// Compare two UTF-8 characters (upstream xmlstring.c `xmlUTF8Charcmp`).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlUTF8Charcmp(const xmlChar *utf1, const xmlChar *utf2);
/// ```
///
/// Returns the result of comparing the first `xmlUTF8Size(utf1)` bytes
/// (like `xmlStrncmp`); NULL `utf1` sorts before non-NULL, both NULL are
/// equal.
///
/// # SAFETY
///
/// - `utf1` must be a valid pointer into a UTF-8 string or NULL.
/// - `utf2` must be a valid pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlUTF8Charcmp(utf1: *const xmlChar, utf2: *const xmlChar) -> c_int {
    if utf1.is_null() {
        return if utf2.is_null() { 0 } else { -1 };
    }
    unsafe { xml_strncmp(utf1, utf2, utf8_size(utf1)) }
}

/// Byte size of the first `len` UTF-8 characters (upstream xmlstring.c
/// `xmlUTF8Strsize`).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlUTF8Strsize(const xmlChar *utf, int len);
/// ```
///
/// Returns 0 for NULL input, `len <= 0` or at the end of the string.
/// The behaviour is not guaranteed for invalid UTF-8 (as upstream).
///
/// # SAFETY
///
/// - `utf` must be a valid NUL-terminated byte string or NULL.
#[no_mangle]
pub const unsafe extern "C" fn xmlUTF8Strsize(utf: *const xmlChar, len: c_int) -> c_int {
    unsafe { utf8_strsize(utf, len) }
}

/// Duplicate the first `len` UTF-8 characters of `utf` (upstream
/// xmlstring.c `xmlUTF8Strndup`).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlUTF8Strndup(const xmlChar *utf, int len);
/// ```
///
/// Returns a freshly allocated NUL-terminated string (caller frees with
/// `xmlFree`), or NULL when `utf` is NULL, `len < 0` or allocation fails.
///
/// # SAFETY
///
/// - `utf` must be a valid NUL-terminated byte string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlUTF8Strndup(utf: *const xmlChar, len: c_int) -> *mut xmlChar {
    if utf.is_null() || len < 0 {
        return ptr::null_mut();
    }
    let i = unsafe { utf8_strsize(utf, len) };
    let ret = unsafe { xmlMallocImpl(i as usize + 1) as *mut xmlChar };
    if ret.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        ptr::copy_nonoverlapping(utf, ret, i as usize);
        *ret.add(i as usize) = 0;
    }
    ret
}

/// Pointer to the UTF-8 character at character position `pos` (upstream
/// xmlstring.c `xmlUTF8Strpos`).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// const xmlChar *xmlUTF8Strpos(const xmlChar *utf, int pos);
/// ```
///
/// Returns NULL when `utf` is NULL, `pos < 0`, the position is past the
/// end, or the input is not well-formed UTF-8.
///
/// # SAFETY
///
/// - `utf` must be a valid NUL-terminated byte string or NULL.
#[no_mangle]
pub const unsafe extern "C" fn xmlUTF8Strpos(utf: *const xmlChar, pos: c_int) -> *const xmlChar {
    if utf.is_null() || pos < 0 {
        return ptr::null();
    }
    unsafe {
        let mut p = utf;
        let mut n = pos;
        while n > 0 {
            let ch = *p;
            p = p.add(1);
            if ch == 0 {
                return ptr::null();
            }
            if (ch & 0x80) != 0 {
                /* if not simple ascii, verify proper format */
                if (ch & 0xc0) != 0xc0 {
                    return ptr::null();
                }
                /* skip over the remaining bytes for this char */
                let mut m = ch;
                loop {
                    m <<= 1;
                    if (m & 0x80) == 0 {
                        break;
                    }
                    let cont = *p;
                    p = p.add(1);
                    if (cont & 0xc0) != 0x80 {
                        return ptr::null();
                    }
                }
            }
            n -= 1;
        }
        p
    }
}

/// Relative character position of the UTF-8 character `utfchar` within
/// `utf` (upstream xmlstring.c `xmlUTF8Strloc`).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlUTF8Strloc(const xmlChar *utf, const xmlChar *utfchar);
/// ```
///
/// Returns the character offset (0-based) of the first occurrence, or -1
/// when not found / arguments are NULL / the input is not well-formed
/// UTF-8.
///
/// # SAFETY
///
/// - `utf` and `utfchar` must be valid NUL-terminated byte strings or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlUTF8Strloc(utf: *const xmlChar, utfchar: *const xmlChar) -> c_int {
    if utf.is_null() || utfchar.is_null() {
        return -1;
    }
    unsafe {
        let size = utf8_strsize(utfchar, 1);
        let mut p = utf;
        let mut i: usize = 0;
        loop {
            let ch = *p;
            if ch == 0 {
                break;
            }
            if xml_strncmp(p, utfchar, size) == 0 {
                return if i > c_int::MAX as usize {
                    0
                } else {
                    i as c_int
                };
            }
            p = p.add(1);
            if (ch & 0x80) != 0 {
                /* if not simple ascii, verify proper format */
                if (ch & 0xc0) != 0xc0 {
                    return -1;
                }
                /* skip over the remaining bytes for this char */
                let mut m = ch;
                loop {
                    m <<= 1;
                    if (m & 0x80) == 0 {
                        break;
                    }
                    if (*p & 0xc0) != 0x80 {
                        return -1;
                    }
                    p = p.add(1);
                }
            }
            i += 1;
        }
    }
    -1
}

/// Extract a substring by UTF-8 character positions (upstream xmlstring.c
/// `xmlUTF8Strsub`).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlUTF8Strsub(const xmlChar *utf, int start, int len);
/// ```
///
/// Returns a freshly allocated NUL-terminated string (caller frees with
/// `xmlFree`), or NULL when `utf` is NULL, `start < 0`, `len < 0`, the
/// start index is past the end, or allocation fails. If `len` is too
/// large, the result is truncated.
///
/// # SAFETY
///
/// - `utf` must be a valid NUL-terminated byte string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlUTF8Strsub(
    utf: *const xmlChar,
    start: c_int,
    len: c_int,
) -> *mut xmlChar {
    if utf.is_null() || start < 0 || len < 0 {
        return ptr::null_mut();
    }
    unsafe {
        let mut p = utf;
        for _ in 0..start {
            let mut ch = *p;
            p = p.add(1);
            if ch == 0 {
                return ptr::null_mut();
            }
            /* skip over the remaining bytes for this char */
            if (ch & 0x80) != 0 {
                ch <<= 1;
                while (ch & 0x80) != 0 {
                    if *p == 0 {
                        return ptr::null_mut();
                    }
                    p = p.add(1);
                    ch <<= 1;
                }
            }
        }
        xmlUTF8Strndup(p, len)
    }
}
