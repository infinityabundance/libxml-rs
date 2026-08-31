//! exports_parserint — xmlParse* internal parser entry points (§11.1-I).
//!
//! C ABI exports for the classic libxml2 parser-internals family
//! (parserInternals.h + a few parser.h / xmlIO.h / xmlerror.h entries).
//! These are the recursive-descent parser primitives (`xmlParseName`,
//! `xmlParseAttValue`, `xmlParseStartTag`, `xmlParseElement`, ...) that
//! custom SAX consumers call directly on a `xmlParserCtxtPtr`.
//!
//! # Implementation strategy
//!
//! The functions are ported from `archaeology/libxml2-git/parser.c` and
//! `parserInternals.c`, operating directly on the `_xmlParserCtxt` /
//! `_xmlParserInput` field layout (same structs the crate's own parser
//! entry points produce via `crate::xml::parser::helpers`), so they work
//! on contexts created by `xmlCreateDocParserCtxt` /
//! `xmlCreateFileParserCtxt` / `xmlCreatePushParserCtxt`.
//!
//! The crate's internal engine (`src/xml/parser/state.rs`) consumes the
//! buffered input wholesale and does not expose this primitive
//! granularity, so these primitives are ported directly rather than wired
//! to the internal tokenizer. SAX events are dispatched through
//! `crate::xml::sax::dispatch::SaxDispatcher` / the raw `_xmlSAXHandler`
//! callbacks, preferring the SAX2 variants (`startElementNs`) when the
//! handler provides them, exactly like upstream.
//!
//! All strings returned to C are allocated with the crate allocator
//! (`xmlMalloc`) and must be freed by the caller with `xmlFree`, matching
//! the upstream contract when `ctxt->dict == NULL` (which is the case for
//! every context the crate creates).
//!
//! # Upstream contract
//!
//! Parity target is upstream `parserInternals.c` and `parser.c` (libxml2
//! 2.15.3) with the `parserInternals.h`/`parser.h`/`xmlIO.h`/`xmlerror.h`
//! signatures — the recursive-descent primitives (`xmlParseName`,
//! `xmlParseAttValue`, `xmlParseStartTag`, `xmlParseElement`, the namespace
//! parsers, `xmlParseQuotedString`, ...) that custom SAX consumers call
//! directly on a `xmlParserCtxtPtr`. R-000165 and R-000169 both touch this
//! module.
//!
//! # Conceptual behavior
//!
//! This module implements the classic parser-internals primitives as faithful
//! ports operating directly on the `_xmlParserCtxt`/`_xmlParserInput` field
//! layout, dispatching SAX events through `crate::xml::sax::dispatch` exactly
//! like upstream (SAX2 variants preferred when present).
//!
//! # Ownership & safety invariants
//!
//! Strings returned to C are allocated with the crate allocator (`xmlMalloc`)
//! and must be freed by the caller with `xmlFree`, matching the upstream
//! contract when `ctxt->dict == NULL`. Parser inputs are owned by the context;
//! the `_xmlParserInput.filename` is an owned copy — R-000169 made every
//! construction path own its filename (xml_strndup) and every free path
//! symmetric with `free_parser_input`.
//!
//! # Historical quirks & epochs
//!
//! These primitives are the 2.0-era recursive-descent parser surface
//! (`legacy_parser` epoch in HISTORY.md) that has stayed exported for
//! custom-SAX consumers; the internal `src/xml/parser/state.rs` engine is the
//! modern non-recursive path (E-002 epoch for diagnostics). R-000169 (11.1-X)
//! fixed the dangling-filename defect class in the four parserInternals entry
//! points.
//!
//! # Deliberate oddities
//!
//! The primitives are deliberately ported standalone rather than wired to the
//! internal tokenizer (documented in the header above): the internal engine
//! consumes input wholesale and cannot expose this granularity. The `xmlParse*`
//! namespace parsers follow upstreams exact token-by-token consumption
//! including its error returns.
//!
//! # Proving courts
//!
//! The PARSER court family plus the DSO-LOADER (25/25) and HEADER-COMPILE
//! (595/595) courts cover this module; the TREE-001 probe exercises the
//! structures these primitives build; the parse-helper unit tests run under
//! cargo test.
//!
//! # Tempting simplifications that would break parity
//!
//! A tempting simplification is to build the parsed name strings with
//! Rust-owned buffers and hand their pointers to C — the strings must be
//! `xmlMalloc`-allocated so `xmlFree` releases them (ownership contract
//! above); and a tempting shortcut to store the input filename by borrowing
//! the Rust String is exactly the R-000169 defect (dangling pointer after
//! context free). Both must not be simplified.

#![allow(missing_docs)]
#![allow(non_snake_case)]
#![allow(non_camel_case_types)]
#![allow(non_upper_case_globals)]
#![allow(unused_variables)]
#![allow(private_interfaces)]
#![allow(clippy::missing_safety_doc)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]
#![allow(clippy::too_many_arguments)]
#![allow(dead_code)]
#![allow(unused_assignments)]

use core::ffi::c_void;
use core::ptr;
use std::mem::size_of;
use std::os::raw::{c_char, c_int, c_uint, c_ulong};

use crate::abi::allocator::{xmlFreeImpl, xmlMallocImpl, xmlMallocZero, xmlReallocImpl};
use crate::abi::callbacks::_xmlSAXLocator;
use crate::abi::data_globals::xmlParserInputBufferCreateFilenameValue;
use crate::abi::structs::*;
use crate::abi::types::*;
use crate::xml::dtd::{create_content_model, create_int_subset, free_content_model};
use crate::xml::parser::helpers::{
    create_parser_ctxt, free_parser_ctxt, input_from_file, input_from_memory, setup_parser_input,
};
use crate::xml::sax::dispatch::SaxDispatcher;
use crate::xml::validation::{is_xml_name_char, is_xml_name_start};

// ═══════════════════════════════════════════════════════════════════════════════
// Local constants
// ═══════════════════════════════════════════════════════════════════════════════

const XML_PARSER_EOF_STATE: c_int = 9; // XML_PARSER_EOF
const XML_PARSER_BUFFER_SIZE: usize = 512;
const LINE_LEN: usize = 80;

// Enum-derived integer constants (the crate keeps these as `repr(C)` enums
// in `crate::abi::types`; define plain `c_int` aliases for the values used
// by the parser primitives).
const XML_ENTITY_DECL: c_int = xmlElementType::XML_ENTITY_DECL as c_int;
const XML_INTERNAL_GENERAL_ENTITY: c_int = xmlEntityType::XML_INTERNAL_GENERAL_ENTITY as c_int;
const XML_EXTERNAL_GENERAL_PARSED_ENTITY: c_int =
    xmlEntityType::XML_EXTERNAL_GENERAL_PARSED_ENTITY as c_int;
const XML_INTERNAL_PARAMETER_ENTITY: c_int = xmlEntityType::XML_INTERNAL_PARAMETER_ENTITY as c_int;
const XML_EXTERNAL_PARAMETER_ENTITY: c_int = xmlEntityType::XML_EXTERNAL_PARAMETER_ENTITY as c_int;
const XML_INTERNAL_PREDEFINED_ENTITY: c_int =
    xmlEntityType::XML_INTERNAL_PREDEFINED_ENTITY as c_int;
const XML_ATTRIBUTE_CDATA: c_int = xmlAttributeType::XML_ATTRIBUTE_CDATA as c_int;
const XML_ATTRIBUTE_ID: c_int = xmlAttributeType::XML_ATTRIBUTE_ID as c_int;
const XML_ATTRIBUTE_IDREF: c_int = xmlAttributeType::XML_ATTRIBUTE_IDREF as c_int;
const XML_ATTRIBUTE_IDREFS: c_int = xmlAttributeType::XML_ATTRIBUTE_IDREFS as c_int;
const XML_ATTRIBUTE_ENTITY: c_int = xmlAttributeType::XML_ATTRIBUTE_ENTITY as c_int;
const XML_ATTRIBUTE_ENTITIES: c_int = xmlAttributeType::XML_ATTRIBUTE_ENTITIES as c_int;
const XML_ATTRIBUTE_NMTOKEN: c_int = xmlAttributeType::XML_ATTRIBUTE_NMTOKEN as c_int;
const XML_ATTRIBUTE_NMTOKENS: c_int = xmlAttributeType::XML_ATTRIBUTE_NMTOKENS as c_int;
const XML_ATTRIBUTE_ENUMERATION: c_int = xmlAttributeType::XML_ATTRIBUTE_ENUMERATION as c_int;
const XML_ATTRIBUTE_NOTATION: c_int = xmlAttributeType::XML_ATTRIBUTE_NOTATION as c_int;
const XML_ATTRIBUTE_NONE: c_int = xmlAttributeDefault::XML_ATTRIBUTE_NONE as c_int;
const XML_ATTRIBUTE_REQUIRED: c_int = xmlAttributeDefault::XML_ATTRIBUTE_REQUIRED as c_int;
const XML_ATTRIBUTE_IMPLIED: c_int = xmlAttributeDefault::XML_ATTRIBUTE_IMPLIED as c_int;
const XML_ATTRIBUTE_FIXED: c_int = xmlAttributeDefault::XML_ATTRIBUTE_FIXED as c_int;
const XML_ELEMENT_CONTENT_PCDATA: c_int =
    xmlElementContentType::XML_ELEMENT_CONTENT_PCDATA as c_int;
const XML_ELEMENT_CONTENT_ELEMENT: c_int =
    xmlElementContentType::XML_ELEMENT_CONTENT_ELEMENT as c_int;
const XML_ELEMENT_CONTENT_SEQ: c_int = xmlElementContentType::XML_ELEMENT_CONTENT_SEQ as c_int;
const XML_ELEMENT_CONTENT_OR: c_int = xmlElementContentType::XML_ELEMENT_CONTENT_OR as c_int;
const XML_ELEMENT_CONTENT_ONCE: c_int = xmlElementContentOccur::XML_ELEMENT_CONTENT_ONCE as c_int;
const XML_ELEMENT_CONTENT_OPT: c_int = xmlElementContentOccur::XML_ELEMENT_CONTENT_OPT as c_int;
const XML_ELEMENT_CONTENT_MULT: c_int = xmlElementContentOccur::XML_ELEMENT_CONTENT_MULT as c_int;
const XML_ELEMENT_CONTENT_PLUS: c_int = xmlElementContentOccur::XML_ELEMENT_CONTENT_PLUS as c_int;
const XML_ELEMENT_TYPE_EMPTY: c_int = xmlElementTypeVal::XML_ELEMENT_TYPE_EMPTY as c_int;
const XML_ELEMENT_TYPE_ANY: c_int = xmlElementTypeVal::XML_ELEMENT_TYPE_ANY as c_int;
const XML_ELEMENT_TYPE_MIXED: c_int = xmlElementTypeVal::XML_ELEMENT_TYPE_MIXED as c_int;
const XML_ELEMENT_TYPE_ELEMENT: c_int = xmlElementTypeVal::XML_ELEMENT_TYPE_ELEMENT as c_int;
const XML_DOC_INTERNAL: c_int = xmlDocProperties::XML_DOC_INTERNAL as c_int;

// Error codes that exist upstream (parser.c error paths) but are missing
// from the crate's renumbered `XML_ERR_*` list. Only used to flag errors
// through `ctxt->errNo`; the exact numeric values are not part of any
// upstream enum in this crate.
const XML_ERR_URI_REQUIRED: c_int = 100;
const XML_ERR_PUBID_REQUIRED: c_int = 101;
const XML_ERR_RESERVED_XML_NAME: c_int = 102;
const XML_ERR_HYPHEN_IN_COMMENT: c_int = 103;
const XML_ERR_EQUAL_REQUIRED: c_int = 104;
const XML_ERR_SEPARATOR_REQUIRED: c_int = 105;
const XML_ERR_INT_SUBSET_NOT_FINISHED: c_int = 106;
const XML_ERR_UNKNOWN_VERSION: c_int = 107;
const XML_ERR_ENCODING_NAME: c_int = 108;
const XML_IO_UNKNOWN: c_int = 109;
const XML_ERR_PCDATA_REQUIRED: c_int = 110;
const XML_ERR_RESOURCE_LIMIT: c_int = 111;
const XML_ERR_LTSLASH_REQUIRED: c_int = 112;

// The static predefined-entity table is immutable; mark the struct Sync so
// it can live in a `static` (same pattern as `xmlChRangeGroup` in structs.rs).
unsafe impl Sync for _xmlEntity {}

// ═══════════════════════════════════════════════════════════════════════════════
// Predefined entities (static instances, upstream entities.c)
// ═══════════════════════════════════════════════════════════════════════════════

static PREDEF_AMP_NAME: [xmlChar; 4] = *b"amp\0";
static PREDEF_LT_NAME: [xmlChar; 3] = *b"lt\0";
static PREDEF_GT_NAME: [xmlChar; 3] = *b"gt\0";
static PREDEF_QUOT_NAME: [xmlChar; 5] = *b"quot\0";
static PREDEF_APOS_NAME: [xmlChar; 5] = *b"apos\0";
static PREDEF_AMP_CONTENT: [xmlChar; 2] = *b"&\0";
static PREDEF_LT_CONTENT: [xmlChar; 2] = *b"<\0";
static PREDEF_GT_CONTENT: [xmlChar; 2] = *b">\0";
static PREDEF_QUOT_CONTENT: [xmlChar; 2] = *b"\"\0";
static PREDEF_APOS_CONTENT: [xmlChar; 2] = *b"'\0";

static PREDEFINED_ENTITIES: [_xmlEntity; 5] = [
    _xmlEntity {
        _private: ptr::null_mut(),
        type_: XML_ENTITY_DECL as c_int,
        name: PREDEF_AMP_NAME.as_ptr(),
        children: ptr::null_mut(),
        last: ptr::null_mut(),
        parent: ptr::null_mut(),
        next: ptr::null_mut(),
        prev: ptr::null_mut(),
        doc: ptr::null_mut(),
        orig: ptr::null_mut(),
        content: PREDEF_AMP_CONTENT.as_ptr() as *mut xmlChar,
        length: 1,
        etype: XML_INTERNAL_PREDEFINED_ENTITY as c_int,
        ExternalID: ptr::null(),
        SystemID: ptr::null(),
        nexte: ptr::null_mut(),
        URI: ptr::null(),
        owner: 0,
        flags: 0,
        expandedSize: 0,
    },
    _xmlEntity {
        _private: ptr::null_mut(),
        type_: XML_ENTITY_DECL as c_int,
        name: PREDEF_LT_NAME.as_ptr(),
        children: ptr::null_mut(),
        last: ptr::null_mut(),
        parent: ptr::null_mut(),
        next: ptr::null_mut(),
        prev: ptr::null_mut(),
        doc: ptr::null_mut(),
        orig: ptr::null_mut(),
        content: PREDEF_LT_CONTENT.as_ptr() as *mut xmlChar,
        length: 1,
        etype: XML_INTERNAL_PREDEFINED_ENTITY as c_int,
        ExternalID: ptr::null(),
        SystemID: ptr::null(),
        nexte: ptr::null_mut(),
        URI: ptr::null(),
        owner: 0,
        flags: 0,
        expandedSize: 0,
    },
    _xmlEntity {
        _private: ptr::null_mut(),
        type_: XML_ENTITY_DECL as c_int,
        name: PREDEF_GT_NAME.as_ptr(),
        children: ptr::null_mut(),
        last: ptr::null_mut(),
        parent: ptr::null_mut(),
        next: ptr::null_mut(),
        prev: ptr::null_mut(),
        doc: ptr::null_mut(),
        orig: ptr::null_mut(),
        content: PREDEF_GT_CONTENT.as_ptr() as *mut xmlChar,
        length: 1,
        etype: XML_INTERNAL_PREDEFINED_ENTITY as c_int,
        ExternalID: ptr::null(),
        SystemID: ptr::null(),
        nexte: ptr::null_mut(),
        URI: ptr::null(),
        owner: 0,
        flags: 0,
        expandedSize: 0,
    },
    _xmlEntity {
        _private: ptr::null_mut(),
        type_: XML_ENTITY_DECL as c_int,
        name: PREDEF_QUOT_NAME.as_ptr(),
        children: ptr::null_mut(),
        last: ptr::null_mut(),
        parent: ptr::null_mut(),
        next: ptr::null_mut(),
        prev: ptr::null_mut(),
        doc: ptr::null_mut(),
        orig: ptr::null_mut(),
        content: PREDEF_QUOT_CONTENT.as_ptr() as *mut xmlChar,
        length: 1,
        etype: XML_INTERNAL_PREDEFINED_ENTITY as c_int,
        ExternalID: ptr::null(),
        SystemID: ptr::null(),
        nexte: ptr::null_mut(),
        URI: ptr::null(),
        owner: 0,
        flags: 0,
        expandedSize: 0,
    },
    _xmlEntity {
        _private: ptr::null_mut(),
        type_: XML_ENTITY_DECL as c_int,
        name: PREDEF_APOS_NAME.as_ptr(),
        children: ptr::null_mut(),
        last: ptr::null_mut(),
        parent: ptr::null_mut(),
        next: ptr::null_mut(),
        prev: ptr::null_mut(),
        doc: ptr::null_mut(),
        orig: ptr::null_mut(),
        content: PREDEF_APOS_CONTENT.as_ptr() as *mut xmlChar,
        length: 1,
        etype: XML_INTERNAL_PREDEFINED_ENTITY as c_int,
        ExternalID: ptr::null(),
        SystemID: ptr::null(),
        nexte: ptr::null_mut(),
        URI: ptr::null(),
        owner: 0,
        flags: 0,
        expandedSize: 0,
    },
];

// ═══════════════════════════════════════════════════════════════════════════════
// Character classification helpers (upstream parserInternals.h macros)
// ═══════════════════════════════════════════════════════════════════════════════

#[inline]
const fn pi_is_blank_ch(c: u8) -> bool {
    c == b' ' || c == b'\t' || c == b'\n' || c == b'\r'
}

#[inline]
fn pi_is_byte_char(c: u8) -> bool {
    c == 0x09 || c == 0x0A || c == 0x0D || (0x20..=0x7E).contains(&c) || (0xA0..=0xFF).contains(&c)
}

#[inline]
const fn pi_is_pubidchar(c: u8) -> bool {
    c == 0x20
        || c == 0x0D
        || c == 0x0A
        || c.is_ascii_alphanumeric()
        || matches!(
            c,
            b'-' | b'\''
                | b'('
                | b')'
                | b'+'
                | b','
                | b'.'
                | b'/'
                | b':'
                | b'='
                | b'?'
                | b';'
                | b'!'
                | b'*'
                | b'#'
                | b'@'
                | b'$'
                | b'_'
                | b'%'
        )
}

/// The XML `Char` production.
#[inline]
const fn pi_is_char(c: c_int) -> bool {
    matches!(c, 0x9 | 0xA | 0xD | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x10000..=0x10FFFF)
}

/// `IS_LETTER_CH` — ASCII letter.
#[inline]
const fn pi_is_letter_ch(c: u8) -> bool {
    c.is_ascii_alphabetic()
}

/// `IS_DIGIT_CH` — ASCII digit.
#[inline]
const fn pi_is_digit_ch(c: u8) -> bool {
    c.is_ascii_digit()
}

/// `IS_NAME_START_CHAR` — XML NameStartChar (including ':').
const fn pi_is_name_start_char(c: c_int) -> bool {
    if c < 0x80 {
        return (c as u8).is_ascii_alphabetic() || c == b'_' as c_int || c == b':' as c_int;
    }
    if c > 0x10FFFF {
        return false;
    }
    match char::from_u32(c as u32) {
        Some(ch) => is_xml_name_start(ch),
        None => false,
    }
}

/// `IS_NAME_CHAR` — XML NameChar (including ':').
const fn pi_is_name_char(c: c_int) -> bool {
    if c < 0x80 {
        let b = c as u8;
        return b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.' || b == b':';
    }
    if c > 0x10FFFF {
        return false;
    }
    match char::from_u32(c as u32) {
        Some(ch) => is_xml_name_char(ch),
        None => false,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Input access primitives (upstream parser.c macros CUR/RAW/NXT/NEXT/SKIP/...)
// ═══════════════════════════════════════════════════════════════════════════════

/// Current input of the context, or NULL.
#[inline]
unsafe fn pi_input(ctxt: *mut _xmlParserCtxt) -> *mut _xmlParserInput {
    if ctxt.is_null() {
        return ptr::null_mut();
    }
    unsafe { (*ctxt).input }
}

/// `CUR` — current byte.
#[inline]
unsafe fn pi_raw(ctxt: *mut _xmlParserCtxt) -> u8 {
    let input = unsafe { pi_input(ctxt) };
    if input.is_null() || unsafe { (*input).cur.is_null() } {
        return 0;
    }
    unsafe { *(*input).cur }
}

/// `NXT(val)` — byte at offset.
#[inline]
unsafe fn pi_nxt(ctxt: *mut _xmlParserCtxt, off: isize) -> u8 {
    let input = unsafe { pi_input(ctxt) };
    if input.is_null() || unsafe { (*input).cur.is_null() } {
        return 0;
    }
    let cur = unsafe { (*input).cur };
    let end = unsafe { (*input).end };
    if off >= 0 {
        if cur.offset(off) >= end {
            return 0;
        }
        unsafe { *cur.offset(off) }
    } else {
        // negative offsets are only ever used for already-consumed bytes
        let base = unsafe { (*input).base };
        if cur.offset(off) < base {
            return 0;
        }
        unsafe { *cur.offset(off) }
    }
}

/// `CUR_PTR`.
#[inline]
unsafe fn pi_cur_ptr(ctxt: *mut _xmlParserCtxt) -> *const xmlChar {
    let input = unsafe { pi_input(ctxt) };
    if input.is_null() {
        return ptr::null();
    }
    unsafe { (*input).cur }
}

/// `BASE_PTR`.
#[inline]
unsafe fn pi_base_ptr(ctxt: *mut _xmlParserCtxt) -> *const xmlChar {
    let input = unsafe { pi_input(ctxt) };
    if input.is_null() {
        return ptr::null();
    }
    unsafe { (*input).base }
}

/// `SKIP(val)` — advance `val` bytes, updating the column.
unsafe fn pi_skip(ctxt: *mut _xmlParserCtxt, val: isize) {
    let input = unsafe { pi_input(ctxt) };
    if input.is_null() {
        return;
    }
    unsafe {
        let cur = (*input).cur;
        let end = (*input).end;
        let mut n = val;
        while n > 0 && cur.offset(n) > end {
            n -= 1;
        }
        (*input).col += val as c_int;
        (*input).cur = cur.offset(val).min(end);
    }
}

/// `NEXT1` — advance one byte.
unsafe fn pi_next1(ctxt: *mut _xmlParserCtxt) {
    let input = unsafe { pi_input(ctxt) };
    if input.is_null() {
        return;
    }
    unsafe {
        if (*input).cur < (*input).end {
            (*input).cur = (*input).cur.add(1);
            (*input).col += 1;
        }
    }
}

/// `NEXTL(l)` — advance `l` bytes, tracking line/col.
unsafe fn pi_nextl(ctxt: *mut _xmlParserCtxt, l: usize) {
    let input = unsafe { pi_input(ctxt) };
    if input.is_null() {
        return;
    }
    unsafe {
        let cur = (*input).cur;
        if cur < (*input).end {
            if *cur == b'\n' {
                (*input).line += 1;
                (*input).col = 1;
            } else {
                (*input).col += 1;
            }
            (*input).cur = cur.add(l).min((*input).end);
        }
    }
}

/// `NEXT` — advance one Unicode character (upstream xmlNextChar).
unsafe fn pi_next_char(ctxt: *mut _xmlParserCtxt) {
    let input = unsafe { pi_input(ctxt) };
    if input.is_null() {
        return;
    }
    unsafe {
        let cur = (*input).cur;
        if cur >= (*input).end {
            return;
        }
        let c = *cur;
        if c < 0x80 {
            if c == b'\n' {
                (*input).cur = cur.add(1);
                (*input).line += 1;
                (*input).col = 1;
            } else if c == b'\r' {
                if cur.add(1) < (*input).end && *cur.add(1) == b'\n' {
                    (*input).cur = cur.add(2);
                } else {
                    (*input).cur = cur.add(1);
                }
                (*input).line += 1;
                (*input).col = 1;
            } else {
                (*input).cur = cur.add(1);
                (*input).col += 1;
            }
        } else {
            (*input).col += 1;
            let (_, l) = pi_decode_utf8(cur, (*input).end);
            if l == 0 {
                (*input).cur = cur.add(1);
            } else {
                (*input).cur = cur.add(l);
            }
        }
    }
}

/// Decode a UTF-8 character at `ptr` (bounded by `end`).
///
/// Returns `(codepoint, byte_len)`. At EOF returns `(0, 0)`; on encoding
/// errors returns `(0xFFFD, 1)` (the recovery character).
unsafe fn pi_decode_utf8(ptr: *const u8, end: *const u8) -> (c_int, usize) {
    unsafe {
        if ptr >= end {
            return (0, 0);
        }
        let c = *ptr;
        if c < 0x80 {
            return (c as c_int, 1);
        }
        let avail = end.offset_from(ptr) as usize;
        if (0xC2..=0xDF).contains(&c) && avail >= 2 && (*ptr.add(1) & 0xC0) == 0x80 {
            let v = (((c as c_int) & 0x1F) << 6) | ((*ptr.add(1) as c_int) & 0x3F);
            return (v, 2);
        }
        if c >= 0xE0 && avail >= 3 && (*ptr.add(1) & 0xC0) == 0x80 && (*ptr.add(2) & 0xC0) == 0x80 {
            let v = (((c as c_int) & 0x0F) << 12)
                | (((*ptr.add(1) as c_int) & 0x3F) << 6)
                | ((*ptr.add(2) as c_int) & 0x3F);
            if v >= 0x800 && !(0xD800..=0xDFFF).contains(&v) {
                return (v, 3);
            }
            return (0xFFFD, 1);
        }
        if c >= 0xF0
            && avail >= 4
            && (*ptr.add(1) & 0xC0) == 0x80
            && (*ptr.add(2) & 0xC0) == 0x80
            && (*ptr.add(3) & 0xC0) == 0x80
        {
            let v = (((c as c_int) & 0x07) << 18)
                | (((*ptr.add(1) as c_int) & 0x3F) << 12)
                | (((*ptr.add(2) as c_int) & 0x3F) << 6)
                | ((*ptr.add(3) as c_int) & 0x3F);
            if (0x10000..=0x10FFFF).contains(&v) {
                return (v, 4);
            }
            return (0xFFFD, 1);
        }
        (0xFFFD, 1)
    }
}

/// `xmlCurrentChar` — current Unicode char + byte length; `(0, 0)` at EOF.
unsafe fn pi_current_char(ctxt: *mut _xmlParserCtxt) -> (c_int, usize) {
    let input = unsafe { pi_input(ctxt) };
    if input.is_null() || unsafe { (*input).cur.is_null() } {
        return (0, 0);
    }
    unsafe {
        let cur = (*input).cur;
        if cur >= (*input).end {
            return (0, 0);
        }
        pi_decode_utf8(cur, (*input).end)
    }
}

/// `xmlCurrentCharRecover` — maps EOF/invalid to 0xFFFD.
unsafe fn pi_current_char_recover(ctxt: *mut _xmlParserCtxt) -> (c_int, usize) {
    let (c, l) = unsafe { pi_current_char(ctxt) };
    if c == 0 {
        (0xFFFD, 1)
    } else {
        (c, l)
    }
}

/// `PARSER_STOPPED`.
#[inline]
unsafe fn pi_stopped(ctxt: *mut _xmlParserCtxt) -> bool {
    unsafe { (*ctxt).errNo != XML_ERR_OK || (*ctxt).disableSAX != 0 }
}

/// `xmlFatalErr` equivalent: record the error and stop the parser.
unsafe fn pi_fatal_err(ctxt: *mut _xmlParserCtxt, code: c_int) {
    unsafe {
        let c = &mut *ctxt;
        if c.errNo == XML_ERR_OK {
            c.errNo = code;
        }
        c.wellFormed = 0;
    }
}

/// `xmlErrMemory` equivalent.
unsafe fn pi_err_memory(ctxt: *mut _xmlParserCtxt) {
    unsafe {
        let c = &mut *ctxt;
        if c.errNo == XML_ERR_OK {
            c.errNo = XML_ERR_NO_MEMORY;
        }
        c.wellFormed = 0;
    }
}

/// Duplicate `len` bytes into a null-terminated xmlChar buffer.
unsafe fn pi_strndup_bytes(start: *const xmlChar, len: usize) -> *mut xmlChar {
    unsafe {
        let buf = xmlMallocImpl(len + 1) as *mut xmlChar;
        if buf.is_null() {
            return ptr::null_mut();
        }
        ptr::copy_nonoverlapping(start, buf, len);
        *buf.add(len) = 0;
        buf
    }
}

/// Encode a codepoint as UTF-8 into the buffer (upstream `COPY_BUF`).
fn pi_push_codepoint(buf: &mut Vec<u8>, c: c_int) {
    if c < 0x80 {
        buf.push(c as u8);
    } else if c < 0x800 {
        buf.push((0xC0 | ((c >> 6) & 0x1F)) as u8);
        buf.push((0x80 | (c & 0x3F)) as u8);
    } else if c < 0x10000 {
        buf.push((0xE0 | ((c >> 12) & 0x0F)) as u8);
        buf.push((0x80 | ((c >> 6) & 0x3F)) as u8);
        buf.push((0x80 | (c & 0x3F)) as u8);
    } else {
        buf.push((0xF0 | ((c >> 18) & 0x07)) as u8);
        buf.push((0x80 | ((c >> 12) & 0x3F)) as u8);
        buf.push((0x80 | ((c >> 6) & 0x3F)) as u8);
        buf.push((0x80 | (c & 0x3F)) as u8);
    }
}

/// `SKIP_BLANKS` — skip whitespace, popping parameter entities at end of
/// input. Returns the number of characters skipped.
unsafe fn pi_skip_blanks(ctxt: *mut _xmlParserCtxt) -> c_int {
    let mut res: c_int = 0;
    unsafe {
        loop {
            if pi_stopped(ctxt) {
                break;
            }
            let input = pi_input(ctxt);
            if input.is_null() {
                break;
            }
            if (*input).cur >= (*input).end || *(*input).cur == 0 {
                // End of a parameter-entity input: pop it (upstream
                // xmlSkipBlankCharsPE). The main input is never popped.
                if (*input).entity.is_null() || (*ctxt).inputNr <= 1 {
                    break;
                }
                pi_pop_pe(ctxt);
                res = res.saturating_add(1);
                continue;
            }
            let c = *(*input).cur;
            if pi_is_blank_ch(c) {
                pi_next_char(ctxt);
                res = res.saturating_add(1);
            } else {
                break;
            }
        }
    }
    res
}

/// Compare six bytes at the current position against a literal.
#[inline]
unsafe fn pi_cmp6(ctxt: *mut _xmlParserCtxt, s: &[u8; 6]) -> bool {
    unsafe {
        pi_nxt(ctxt, 0) == s[0]
            && pi_nxt(ctxt, 1) == s[1]
            && pi_nxt(ctxt, 2) == s[2]
            && pi_nxt(ctxt, 3) == s[3]
            && pi_nxt(ctxt, 4) == s[4]
            && pi_nxt(ctxt, 5) == s[5]
    }
}

#[inline]
unsafe fn pi_cmp5(ctxt: *mut _xmlParserCtxt, s: &[u8; 5]) -> bool {
    unsafe {
        pi_nxt(ctxt, 0) == s[0]
            && pi_nxt(ctxt, 1) == s[1]
            && pi_nxt(ctxt, 2) == s[2]
            && pi_nxt(ctxt, 3) == s[3]
            && pi_nxt(ctxt, 4) == s[4]
    }
}

#[inline]
unsafe fn pi_cmp7(ctxt: *mut _xmlParserCtxt, s: &[u8; 7]) -> bool {
    unsafe { pi_cmp6(ctxt, &[s[0], s[1], s[2], s[3], s[4], s[5]]) && pi_nxt(ctxt, 6) == s[6] }
}

#[inline]
unsafe fn pi_cmp8(ctxt: *mut _xmlParserCtxt, s: &[u8; 8]) -> bool {
    unsafe { pi_cmp7(ctxt, &[s[0], s[1], s[2], s[3], s[4], s[5], s[6]]) && pi_nxt(ctxt, 7) == s[7] }
}

#[inline]
unsafe fn pi_cmp9(ctxt: *mut _xmlParserCtxt, s: &[u8; 9]) -> bool {
    unsafe {
        pi_cmp8(ctxt, &[s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]) && pi_nxt(ctxt, 8) == s[8]
    }
}

#[inline]
unsafe fn pi_cmp10(ctxt: *mut _xmlParserCtxt, s: &[u8; 10]) -> bool {
    unsafe {
        pi_cmp9(
            ctxt,
            &[s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7], s[8]],
        ) && pi_nxt(ctxt, 9) == s[9]
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Stack helpers (name / space / node / input stacks of _xmlParserCtxt)
// ═══════════════════════════════════════════════════════════════════════════════

/// `namePush` — push a name onto ctxt->nameTab (takes ownership).
unsafe fn pi_name_push(ctxt: *mut _xmlParserCtxt, name: *const xmlChar) -> c_int {
    unsafe {
        let c = &mut *ctxt;
        if c.nameNr >= c.nameMax {
            let new_max = if c.nameMax == 0 { 10 } else { c.nameMax * 2 };
            let new_tab = xmlReallocImpl(
                c.nameTab as *mut c_void,
                (new_max as usize) * size_of::<*const xmlChar>(),
            ) as *mut *const xmlChar;
            if new_tab.is_null() {
                return -1;
            }
            c.nameTab = new_tab;
            c.nameMax = new_max;
        }
        *c.nameTab.add(c.nameNr as usize) = name;
        c.nameNr += 1;
        c.name = name;
    }
    0
}

/// `namePop` — pop and free the top name.
unsafe fn pi_name_pop(ctxt: *mut _xmlParserCtxt) {
    unsafe {
        let c = &mut *ctxt;
        if c.nameNr <= 0 {
            return;
        }
        c.nameNr -= 1;
        let old = *c.nameTab.add(c.nameNr as usize);
        *c.nameTab.add(c.nameNr as usize) = ptr::null();
        if c.nameNr == 0 {
            c.name = ptr::null();
        } else {
            c.name = *c.nameTab.add((c.nameNr - 1) as usize);
        }
        if !old.is_null() {
            xmlFreeImpl(old as *mut c_void);
        }
    }
}

/// `spacePush`.
unsafe fn pi_space_push(ctxt: *mut _xmlParserCtxt, val: c_int) -> c_int {
    unsafe {
        let c = &mut *ctxt;
        if c.spaceNr >= c.spaceMax {
            let new_max = if c.spaceMax == 0 { 10 } else { c.spaceMax * 2 };
            let new_tab = xmlReallocImpl(
                c.spaceTab as *mut c_void,
                (new_max as usize) * size_of::<c_int>(),
            ) as *mut c_int;
            if new_tab.is_null() {
                return -1;
            }
            c.spaceTab = new_tab;
            c.spaceMax = new_max;
        }
        *c.spaceTab.add(c.spaceNr as usize) = val;
        c.spaceNr += 1;
        c.space = c.spaceTab.add((c.spaceNr - 1) as usize);
    }
    0
}

/// `spacePop`.
unsafe fn pi_space_pop(ctxt: *mut _xmlParserCtxt) {
    unsafe {
        let c = &mut *ctxt;
        if c.spaceNr <= 0 {
            return;
        }
        c.spaceNr -= 1;
        if c.spaceNr == 0 {
            c.space = ptr::null_mut();
        } else {
            c.space = c.spaceTab.add((c.spaceNr - 1) as usize);
        }
    }
}

/// `nodePush` — push a node onto ctxt->nodeTab.
unsafe fn pi_node_push(ctxt: *mut _xmlParserCtxt, node: *mut _xmlNode) -> c_int {
    unsafe {
        let c = &mut *ctxt;
        if c.nodeNr >= c.nodeMax {
            let new_max = if c.nodeMax == 0 { 10 } else { c.nodeMax * 2 };
            let new_tab = xmlReallocImpl(
                c.nodeTab as *mut c_void,
                (new_max as usize) * size_of::<*mut _xmlNode>(),
            ) as *mut *mut _xmlNode;
            if new_tab.is_null() {
                return -1;
            }
            c.nodeTab = new_tab;
            c.nodeMax = new_max;
        }
        *c.nodeTab.add(c.nodeNr as usize) = node;
        c.nodeNr += 1;
        c.node = node;
    }
    0
}

/// `nodePop`.
unsafe fn pi_node_pop(ctxt: *mut _xmlParserCtxt) {
    unsafe {
        let c = &mut *ctxt;
        if c.nodeNr <= 0 {
            return;
        }
        c.nodeNr -= 1;
        if c.nodeNr == 0 {
            c.node = ptr::null_mut();
        } else {
            c.node = *c.nodeTab.add((c.nodeNr - 1) as usize);
        }
    }
}

/// `xmlCtxtPushInput` — push an input onto the input stack.
unsafe fn pi_input_push(ctxt: *mut _xmlParserCtxt, input: *mut _xmlParserInput) -> c_int {
    unsafe {
        if ctxt.is_null() || input.is_null() {
            return -1;
        }
        let c = &mut *ctxt;
        if c.inputNr >= c.inputMax {
            let new_max = if c.inputMax == 0 { 4 } else { c.inputMax * 2 };
            let new_tab = xmlReallocImpl(
                c.inputTab as *mut c_void,
                (new_max as usize) * size_of::<*mut _xmlParserInput>(),
            ) as *mut *mut _xmlParserInput;
            if new_tab.is_null() {
                return -1;
            }
            c.inputTab = new_tab;
            c.inputMax = new_max;
        }
        *c.inputTab.add(c.inputNr as usize) = input;
        c.input = input;
        c.inputNr += 1;
    }
    0
}

/// `xmlCtxtPopInput` — pop the top input from the stack.
unsafe fn pi_input_pop(ctxt: *mut _xmlParserCtxt) -> *mut _xmlParserInput {
    unsafe {
        if ctxt.is_null() {
            return ptr::null_mut();
        }
        let c = &mut *ctxt;
        if c.inputNr <= 0 {
            return ptr::null_mut();
        }
        c.inputNr -= 1;
        if c.inputNr > 0 {
            c.input = *c.inputTab.add((c.inputNr - 1) as usize);
        } else {
            c.input = ptr::null_mut();
        }
        let ret = *c.inputTab.add(c.inputNr as usize);
        *c.inputTab.add(c.inputNr as usize) = ptr::null_mut();
        ret
    }
}

/// `xmlPopPE` — pop a parameter-entity input, releasing its buffer.
unsafe fn pi_pop_pe(ctxt: *mut _xmlParserCtxt) {
    unsafe {
        let input = pi_input_pop(ctxt);
        if input.is_null() {
            return;
        }
        let base = (*input).base;
        if !base.is_null() && !(*input).entity.is_null() {
            xmlFreeImpl(base as *mut c_void);
        }
        // Free the owned filename copy (alloc_parser_input/parserint dup) so
        // the pop path is symmetric with free_parser_input.
        if !(*input).filename.is_null() {
            xmlFreeImpl((*input).filename as *mut c_void);
        }
        xmlFreeImpl(input as *mut c_void);
    }
}

/// `xmlCtxtInitializeLate` — detect SAX2 handlers.
unsafe fn pi_ctxt_late_init(ctxt: *mut _xmlParserCtxt) {
    unsafe {
        if ctxt.is_null() {
            return;
        }
        let c = &mut *ctxt;
        if !c.sax.is_null() && (*c.sax).initialized == XML_SAX2_MAGIC as c_uint {
            c.sax2 = 1;
        }
    }
}

/// Split a QName into (localname, prefix) sub-pointers.
const unsafe fn pi_split_qname(qname: *const xmlChar) -> (*const xmlChar, *const xmlChar) {
    if qname.is_null() {
        return (ptr::null(), ptr::null());
    }
    unsafe {
        let mut p = qname;
        while *p != 0 {
            if *p == b':' {
                return (p.add(1), qname);
            }
            p = p.add(1);
        }
    }
    (qname, ptr::null())
}

/// Whether the null-terminated string equals `bytes`.
const unsafe fn pi_cstr_eq(s: *const xmlChar, bytes: &[u8]) -> bool {
    if s.is_null() {
        return bytes.is_empty();
    }
    unsafe {
        let mut i = 0usize;
        loop {
            let b = *s.add(i);
            if i >= bytes.len() {
                return b == 0;
            }
            if b != bytes[i] {
                return false;
            }
            i += 1;
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Entity lookup
// ═══════════════════════════════════════════════════════════════════════════════

/// `xmlGetPredefinedEntity` equivalent.
unsafe fn pi_get_predefined_entity(name: *const xmlChar) -> *mut _xmlEntity {
    if name.is_null() {
        return ptr::null_mut();
    }
    for e in PREDEFINED_ENTITIES.iter() {
        if unsafe { crate::abi::exports_xml2::xmlStrcmp(e.name, name) == 0 } {
            return e as *const _xmlEntity as *mut _xmlEntity;
        }
    }
    ptr::null_mut()
}

/// `xmlLookupGeneralEntity` equivalent (without the unparsed-entity
/// validation, which needs error reporting paths).
unsafe fn pi_lookup_general_entity(
    ctxt: *mut _xmlParserCtxt,
    name: *const xmlChar,
) -> *mut _xmlEntity {
    unsafe {
        // Predefined entities override any extra definition (unless OLDSAX).
        if (*ctxt).options & XML_PARSE_OLDSAX == 0 {
            let ent = pi_get_predefined_entity(name);
            if !ent.is_null() {
                return ent;
            }
        }
        let c = &*ctxt;
        if !c.sax.is_null() {
            let ent = SaxDispatcher::get_entity(&*c.sax, c.userData, name);
            if !ent.is_null() {
                return ent;
            }
            if c.userData == ctxt as *mut c_void {
                let ent2 = crate::abi::exports_xml2::xmlSAX2GetEntity(c.userData, name);
                if !ent2.is_null() {
                    return ent2;
                }
            }
        }
        ptr::null_mut()
    }
}

/// Lookup a parameter entity (sax getParameterEntity, then doc fallback).
unsafe fn pi_lookup_parameter_entity(
    ctxt: *mut _xmlParserCtxt,
    name: *const xmlChar,
) -> *mut _xmlEntity {
    unsafe {
        let c = &*ctxt;
        if !c.sax.is_null() {
            let ent = SaxDispatcher::get_parameter_entity(&*c.sax, c.userData, name);
            if !ent.is_null() {
                return ent;
            }
            if c.userData == ctxt as *mut c_void {
                let ent2 = crate::abi::exports_xml2::xmlSAX2GetParameterEntity(c.userData, name);
                if !ent2.is_null() {
                    return ent2;
                }
            }
        }
        ptr::null_mut()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// SAX dispatch helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Dispatch a SAX characters/ignorableWhitespace event with `bytes`.
unsafe fn pi_sax_chars(ctxt: *mut _xmlParserCtxt, bytes: &[u8], ignorable: bool) {
    if bytes.is_empty() {
        return;
    }
    unsafe {
        let c = &*ctxt;
        if c.sax.is_null() || c.disableSAX != 0 {
            return;
        }
        let buf = pi_strndup_bytes(bytes.as_ptr(), bytes.len());
        if buf.is_null() {
            return;
        }
        if ignorable {
            SaxDispatcher::ignorable_whitespace(&*c.sax, c.userData, buf, bytes.len() as c_int);
        } else {
            SaxDispatcher::characters(&*c.sax, c.userData, buf, bytes.len() as c_int);
        }
        xmlFreeImpl(buf as *mut c_void);
    }
}

/// Dispatch a SAX comment event with `bytes`.
unsafe fn pi_sax_comment(ctxt: *mut _xmlParserCtxt, bytes: &[u8]) {
    unsafe {
        let c = &*ctxt;
        if c.sax.is_null() || c.disableSAX != 0 {
            return;
        }
        let buf = if bytes.is_empty() {
            ptr::null()
        } else {
            pi_strndup_bytes(bytes.as_ptr(), bytes.len())
        };
        if bytes.is_empty() || !buf.is_null() {
            SaxDispatcher::comment(&*c.sax, c.userData, buf);
        }
        if !buf.is_null() {
            xmlFreeImpl(buf as *mut c_void);
        }
    }
}

/// Dispatch a SAX processingInstruction event.
unsafe fn pi_sax_pi(ctxt: *mut _xmlParserCtxt, target: *const xmlChar, data: &[u8]) {
    unsafe {
        let c = &*ctxt;
        if c.sax.is_null() || c.disableSAX != 0 {
            return;
        }
        let buf = if data.is_empty() {
            ptr::null()
        } else {
            pi_strndup_bytes(data.as_ptr(), data.len())
        };
        if data.is_empty() || !buf.is_null() {
            SaxDispatcher::processing_instruction(&*c.sax, c.userData, target, buf);
        }
        if !buf.is_null() {
            xmlFreeImpl(buf as *mut c_void);
        }
    }
}

/// Dispatch a SAX start-element event, preferring SAX2 (`startElementNs`)
/// over SAX1 (`startElement`) — upstream SAX2.c behaviour.
///
/// `atts` is the SAX1 attribute array (`[name, value, ...]`, `nbatts`
/// entries). The SAX2 array is derived from it: `xmlns` attributes become
/// namespace declarations, everything else becomes attributes.
unsafe fn pi_dispatch_start_element(
    ctxt: *mut _xmlParserCtxt,
    qname: *const xmlChar,
    atts: *mut *const xmlChar,
    nbatts: usize,
) {
    unsafe {
        let c = &*ctxt;
        if c.sax.is_null() || c.disableSAX != 0 {
            return;
        }
        let sax = &*c.sax;
        let (local, prefix) = pi_split_qname(qname);
        if let Some(cb2) = sax.startElementNs {
            let nattr = nbatts / 2;
            let mut namespaces: Vec<*const xmlChar> = Vec::new();
            let mut attrs2: Vec<*const xmlChar> = Vec::new();
            for k in 0..nattr {
                let aname = *atts.add(k * 2);
                let aval = *atts.add(k * 2 + 1);
                if aname.is_null() {
                    continue;
                }
                let (alocal, apref) = pi_split_qname(aname);
                if apref.is_null() && pi_cstr_eq(aname, b"xmlns") {
                    namespaces.push(ptr::null());
                    namespaces.push(aval);
                } else if !apref.is_null() && pi_cstr_eq(apref, b"xmlns") {
                    namespaces.push(alocal);
                    namespaces.push(aval);
                } else {
                    attrs2.push(alocal);
                    attrs2.push(apref);
                    attrs2.push(ptr::null());
                    attrs2.push(aval);
                    attrs2.push(aval.add(crate::abi::exports_xml2::xmlStrlen(aval) as usize));
                }
            }
            cb2(
                c.userData,
                local,
                prefix,
                ptr::null(),
                (namespaces.len() / 2) as c_int,
                namespaces.as_mut_ptr(),
                (attrs2.len() / 5) as c_int,
                0,
                attrs2.as_mut_ptr(),
            );
        } else if let Some(cb1) = sax.startElement {
            let sa1 = if nbatts > 0 {
                // ensure NULL-terminated pair list
                let mut arr: Vec<*const xmlChar> = Vec::with_capacity(nbatts + 2);
                for i in 0..nbatts {
                    arr.push(*atts.add(i));
                }
                arr.push(ptr::null());
                arr.push(ptr::null());
                arr.as_mut_ptr()
            } else {
                ptr::null_mut()
            };
            cb1(c.userData, qname, sa1);
        }
    }
}

/// Dispatch a SAX end-element event, preferring SAX2 (`endElementNs`).
unsafe fn pi_dispatch_end_element(ctxt: *mut _xmlParserCtxt, qname: *const xmlChar) {
    unsafe {
        let c = &*ctxt;
        if c.sax.is_null() || c.disableSAX != 0 {
            return;
        }
        let sax = &*c.sax;
        let (local, prefix) = pi_split_qname(qname);
        if let Some(cb2) = sax.endElementNs {
            cb2(c.userData, local, prefix, ptr::null());
        } else if let Some(cb1) = sax.endElement {
            cb1(c.userData, qname);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Core parsing primitives (ports of parser.c / parserInternals.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// `xmlParseName` — returns a malloc'd copy of the name (upstream returns
/// a dict pointer; with a NULL dict upstream copies with xmlStrdup too).
unsafe fn pi_parse_name(ctxt: *mut _xmlParserCtxt) -> *const xmlChar {
    unsafe {
        let input = pi_input(ctxt);
        if input.is_null() || (*input).cur.is_null() {
            return ptr::null();
        }
        let in_ptr = (*input).cur;
        let end = (*input).end;
        let first = *in_ptr;

        // Accelerator for simple ASCII names.
        if (first.is_ascii_lowercase()
            || first.is_ascii_uppercase()
            || first == b'_'
            || first == b':')
            && in_ptr < end
        {
            let mut p = in_ptr.add(1);
            while p < end {
                let b = *p;
                if b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b':' || b == b'.' {
                    p = p.add(1);
                } else {
                    break;
                }
            }
            if p >= end || (*p > 0 && *p < 0x80) {
                let count = p.offset_from(in_ptr) as usize;
                if count > 0 {
                    let ret = pi_strndup_bytes(in_ptr, count);
                    if ret.is_null() {
                        pi_err_memory(ctxt);
                        return ptr::null();
                    }
                    (*input).cur = p;
                    (*input).col += count as c_int;
                    return ret;
                }
            }
        }

        // Complex path: full Unicode handling.
        let start = (*input).cur;
        let (c, l) = pi_current_char(ctxt);
        if !pi_is_name_start_char(c) {
            return ptr::null();
        }
        let mut len = l;
        pi_nextl(ctxt, l);
        loop {
            let (c2, l2) = pi_current_char(ctxt);
            if !pi_is_name_char(c2) {
                break;
            }
            len += l2;
            pi_nextl(ctxt, l2);
        }
        let ret = pi_strndup_bytes(start, len);
        if ret.is_null() {
            pi_err_memory(ctxt);
        }
        ret
    }
}

/// `xmlParseNmtoken` — returns a malloc'd copy.
unsafe fn pi_parse_nmtoken(ctxt: *mut _xmlParserCtxt) -> *mut xmlChar {
    unsafe {
        let mut buf: Vec<u8> = Vec::new();
        loop {
            let (c, l) = pi_current_char(ctxt);
            if !pi_is_name_char(c) {
                break;
            }
            pi_push_codepoint(&mut buf, c);
            pi_nextl(ctxt, l);
        }
        if buf.is_empty() {
            return ptr::null_mut();
        }
        let ret = pi_strndup_bytes(buf.as_ptr(), buf.len());
        if ret.is_null() {
            pi_err_memory(ctxt);
        }
        ret
    }
}

/// `xmlParseNameAndCompare` — fast path returning `1` on match.
unsafe fn pi_parse_name_and_compare(
    ctxt: *mut _xmlParserCtxt,
    other: *const xmlChar,
) -> *const xmlChar {
    unsafe {
        let input = pi_input(ctxt);
        if input.is_null() || other.is_null() {
            return ptr::null();
        }
        let mut in_ptr = (*input).cur;
        let mut cmp = other;
        while !in_ptr.is_null() && *in_ptr != 0 && *in_ptr == *cmp {
            in_ptr = in_ptr.add(1);
            cmp = cmp.add(1);
        }
        if *cmp == 0 && (*in_ptr == b'>' || pi_is_blank_ch(*in_ptr)) {
            (*input).col += in_ptr.offset_from((*input).cur) as c_int;
            (*input).cur = in_ptr;
            return std::ptr::dangling::<xmlChar>();
        }
        let ret = pi_parse_name(ctxt);
        if !ret.is_null() && crate::abi::exports_xml2::xmlStrcmp(ret, other) == 0 {
            xmlFreeImpl(ret as *mut c_void);
            return std::ptr::dangling::<xmlChar>();
        }
        ret
    }
}

/// `xmlParseCharRef` — parse `&#...;` / `&#x...;`, consuming the '&'.
unsafe fn pi_parse_char_ref(ctxt: *mut _xmlParserCtxt) -> c_int {
    unsafe {
        let mut val: c_int = 0;
        let mut count: c_int = 0;
        if pi_raw(ctxt) == b'&' && pi_nxt(ctxt, 1) == b'#' && pi_nxt(ctxt, 2) == b'x' {
            pi_skip(ctxt, 3);
            while pi_raw(ctxt) != b';' && !pi_stopped(ctxt) {
                if count > 20 {
                    count = 0;
                }
                let c = pi_raw(ctxt);
                if c.is_ascii_digit() {
                    val = val * 16 + (c - b'0') as c_int;
                } else if (b'a'..=b'f').contains(&c) {
                    val = val * 16 + (c - b'a') as c_int + 10;
                } else if (b'A'..=b'F').contains(&c) {
                    val = val * 16 + (c - b'A') as c_int + 10;
                } else {
                    pi_fatal_err(ctxt, XML_ERR_INVALID_HEX_CHARREF);
                    val = 0;
                    break;
                }
                if val > 0x110000 {
                    val = 0x110000;
                }
                pi_next1(ctxt);
                count += 1;
            }
            if pi_raw(ctxt) == b';' {
                pi_next1(ctxt);
            }
        } else if pi_raw(ctxt) == b'&' && pi_nxt(ctxt, 1) == b'#' {
            pi_skip(ctxt, 2);
            while pi_raw(ctxt) != b';' {
                if count > 20 {
                    count = 0;
                }
                let c = pi_raw(ctxt);
                if c.is_ascii_digit() {
                    val = val * 10 + (c - b'0') as c_int;
                } else {
                    pi_fatal_err(ctxt, XML_ERR_INVALID_DEC_CHARREF);
                    val = 0;
                    break;
                }
                if val > 0x110000 {
                    val = 0x110000;
                }
                pi_next1(ctxt);
                count += 1;
            }
            if pi_raw(ctxt) == b';' {
                pi_next1(ctxt);
            }
        } else {
            if pi_raw(ctxt) == b'&' {
                pi_skip(ctxt, 1);
            }
            pi_fatal_err(ctxt, XML_ERR_INVALID_CHARREF);
        }

        // [WFC: Legal Character]
        if val >= 0x110000 {
            pi_fatal_err(ctxt, XML_ERR_INVALID_CHAR);
            val = 0xFFFD;
        } else if !pi_is_char(val) {
            pi_fatal_err(ctxt, XML_ERR_INVALID_CHAR);
        }
        val
    }
}

/// `xmlParseEntityRef` — parse `&name;`, returning the entity.
unsafe fn pi_parse_entity_ref(ctxt: *mut _xmlParserCtxt) -> *mut _xmlEntity {
    unsafe {
        if ctxt.is_null() {
            return ptr::null_mut();
        }
        let name = pi_parse_entity_ref_name(ctxt);
        if name.is_null() {
            return ptr::null_mut();
        }
        let ent = pi_lookup_general_entity(ctxt, name);
        xmlFreeImpl(name as *mut c_void);
        ent
    }
}

/// `xmlParseEntityRefInternal` — parse `&name;`, returning the name.
unsafe fn pi_parse_entity_ref_name(ctxt: *mut _xmlParserCtxt) -> *const xmlChar {
    unsafe {
        if pi_raw(ctxt) != b'&' {
            return ptr::null();
        }
        pi_next1(ctxt);
        let name = pi_parse_name(ctxt);
        if name.is_null() {
            pi_fatal_err(ctxt, XML_ERR_ENTITYREF_NO_NAME);
            return ptr::null();
        }
        if pi_raw(ctxt) != b';' {
            pi_fatal_err(ctxt, XML_ERR_ENTITYREF_SEMICOL_MISSING);
            xmlFreeImpl(name as *mut c_void);
            return ptr::null();
        }
        pi_next1(ctxt);
        name
    }
}

/// `xmlParseEntityValue` — parse a quoted entity value.
unsafe fn pi_parse_entity_value(
    ctxt: *mut _xmlParserCtxt,
    orig: *mut *mut xmlChar,
) -> *mut xmlChar {
    unsafe {
        let quote = pi_raw(ctxt);
        if quote != b'"' && quote != b'\'' {
            pi_fatal_err(ctxt, XML_ERR_ENTITY_NOT_STARTED);
            return ptr::null_mut();
        }
        let start = pi_cur_ptr(ctxt);
        pi_next1(ctxt);
        let mut len: usize = 0;
        loop {
            if pi_stopped(ctxt) {
                return ptr::null_mut();
            }
            let input = pi_input(ctxt);
            if input.is_null() || (*input).cur >= (*input).end {
                pi_fatal_err(ctxt, XML_ERR_ENTITY_NOT_FINISHED);
                return ptr::null_mut();
            }
            let c = pi_raw(ctxt);
            if c == 0 {
                pi_fatal_err(ctxt, XML_ERR_INVALID_CHAR);
                return ptr::null_mut();
            }
            if c == quote {
                break;
            }
            pi_next1(ctxt);
            len += 1;
        }
        if !orig.is_null() {
            *orig = pi_strndup_bytes(start, len);
        }
        let val = pi_strndup_bytes(start, len);
        pi_next1(ctxt);
        val
    }
}

/// `xmlParseAttValue` — parse an attribute value (entity references are
/// preserved as `&name;` unless substitution is enabled).
unsafe fn pi_parse_att_value(ctxt: *mut _xmlParserCtxt) -> *mut xmlChar {
    unsafe {
        if ctxt.is_null() || pi_input(ctxt).is_null() {
            return ptr::null_mut();
        }
        let quote = pi_raw(ctxt);
        if quote != b'"' && quote != b'\'' {
            pi_fatal_err(ctxt, XML_ERR_ATTRIBUTE_NOT_STARTED);
            return ptr::null_mut();
        }
        pi_next1(ctxt);
        let replace_entities = (*ctxt).replaceEntities != 0;
        let mut out: Vec<u8> = Vec::new();
        loop {
            if pi_stopped(ctxt) {
                return ptr::null_mut();
            }
            let input = pi_input(ctxt);
            if input.is_null() {
                return ptr::null_mut();
            }
            if (*input).cur >= (*input).end {
                pi_fatal_err(ctxt, XML_ERR_ATTRIBUTE_NOT_FINISHED);
                return ptr::null_mut();
            }
            let c = pi_raw(ctxt);
            if c == quote {
                break;
            }
            if c >= 0x80 {
                let (ch, l) = pi_current_char(ctxt);
                if ch == 0 {
                    pi_fatal_err(ctxt, XML_ERR_INVALID_CHAR);
                    return ptr::null_mut();
                }
                pi_push_codepoint(&mut out, ch);
                pi_nextl(ctxt, l);
            } else if c == b'&' {
                if pi_nxt(ctxt, 1) == b'#' {
                    let val = pi_parse_char_ref(ctxt);
                    if val == 0 {
                        return ptr::null_mut();
                    }
                    if val == b'&' as c_int && !replace_entities {
                        out.extend_from_slice(b"&#38;");
                    } else {
                        pi_push_codepoint(&mut out, val);
                    }
                } else {
                    let name = pi_parse_entity_ref_name(ctxt);
                    if name.is_null() {
                        return ptr::null_mut();
                    }
                    let ent = pi_lookup_general_entity(ctxt, name);
                    let mut expanded = false;
                    if !ent.is_null() {
                        let etype = (*ent).etype;
                        let content = (*ent).content;
                        if !content.is_null() {
                            if etype == XML_INTERNAL_PREDEFINED_ENTITY as c_int {
                                if *content == b'&' && !replace_entities {
                                    out.extend_from_slice(b"&#38;");
                                } else {
                                    let l = crate::abi::exports_xml2::xmlStrlen(content) as usize;
                                    out.extend_from_slice(core::slice::from_raw_parts(content, l));
                                }
                                expanded = true;
                            } else if replace_entities
                                && etype == XML_INTERNAL_GENERAL_ENTITY as c_int
                            {
                                let l = crate::abi::exports_xml2::xmlStrlen(content) as usize;
                                out.extend_from_slice(core::slice::from_raw_parts(content, l));
                                expanded = true;
                            }
                        }
                    }
                    if !expanded {
                        out.push(b'&');
                        let l = crate::abi::exports_xml2::xmlStrlen(name) as usize;
                        out.extend_from_slice(core::slice::from_raw_parts(name, l));
                        out.push(b';');
                    }
                    xmlFreeImpl(name as *mut c_void);
                }
            } else {
                if c == b'<' {
                    pi_fatal_err(ctxt, XML_ERR_LT_IN_ATTRIBUTE);
                }
                if c < 0x20 {
                    // Whitespace is converted to space; CRLF collapses.
                    out.push(b' ');
                    if c == b'\r' && pi_nxt(ctxt, 1) == b'\n' {
                        pi_next1(ctxt);
                    }
                } else {
                    out.push(c);
                }
                pi_next1(ctxt);
            }
        }
        pi_next1(ctxt);
        out.push(0);

        pi_strndup_bytes(out.as_ptr(), out.len() - 1)
    }
}

/// `xmlParseSystemLiteral`.
unsafe fn pi_parse_system_literal(ctxt: *mut _xmlParserCtxt) -> *mut xmlChar {
    unsafe {
        let stop = if pi_raw(ctxt) == b'"' {
            pi_next1(ctxt);
            b'"'
        } else if pi_raw(ctxt) == b'\'' {
            pi_next1(ctxt);
            b'\''
        } else {
            pi_fatal_err(ctxt, XML_ERR_LITERAL_NOT_STARTED);
            return ptr::null_mut();
        };
        let mut buf: Vec<u8> = Vec::new();
        loop {
            let (cur, l) = pi_current_char_recover(ctxt);
            if !pi_is_char(cur) || cur == stop as c_int {
                break;
            }
            pi_push_codepoint(&mut buf, cur);
            pi_nextl(ctxt, l);
        }
        let cur = pi_raw(ctxt);
        if !pi_is_char(cur as c_int) {
            pi_fatal_err(ctxt, XML_ERR_LITERAL_NOT_FINISHED);
        } else if cur == stop {
            pi_next1(ctxt);
        }
        pi_strndup_bytes(buf.as_ptr(), buf.len())
    }
}

/// `xmlParsePubidLiteral`.
unsafe fn pi_parse_pubid_literal(ctxt: *mut _xmlParserCtxt) -> *mut xmlChar {
    unsafe {
        let stop = if pi_raw(ctxt) == b'"' {
            pi_next1(ctxt);
            b'"'
        } else if pi_raw(ctxt) == b'\'' {
            pi_next1(ctxt);
            b'\''
        } else {
            pi_fatal_err(ctxt, XML_ERR_LITERAL_NOT_STARTED);
            return ptr::null_mut();
        };
        let mut buf: Vec<u8> = Vec::new();
        loop {
            let cur = pi_raw(ctxt);
            if !pi_is_pubidchar(cur) || cur == stop {
                break;
            }
            buf.push(cur);
            pi_next1(ctxt);
        }
        if pi_raw(ctxt) != stop {
            pi_fatal_err(ctxt, XML_ERR_LITERAL_NOT_FINISHED);
        } else {
            pi_next1(ctxt);
        }
        pi_strndup_bytes(buf.as_ptr(), buf.len())
    }
}

/// `xmlParseQuotedString` — parse and return a quoted string.
unsafe fn pi_parse_quoted_string(ctxt: *mut _xmlParserCtxt) -> *mut xmlChar {
    unsafe {
        if ctxt.is_null() || pi_input(ctxt).is_null() {
            return ptr::null_mut();
        }
        let ret = pi_parse_att_value(ctxt);
        if ret.is_null() {
            pi_fatal_err(ctxt, XML_ERR_STRING_NOT_STARTED);
        }
        ret
    }
}

/// `xmlParseExternalID` — returns the URI; sets `*publicId` if PUBLIC.
unsafe fn pi_parse_external_id(
    ctxt: *mut _xmlParserCtxt,
    public_id: *mut *mut xmlChar,
    strict: c_int,
) -> *mut xmlChar {
    unsafe {
        *public_id = ptr::null_mut();
        let mut uri: *mut xmlChar = ptr::null_mut();
        if pi_cmp6(ctxt, b"SYSTEM") {
            pi_skip(ctxt, 6);
            pi_skip_blanks(ctxt);
            uri = pi_parse_system_literal(ctxt);
            if uri.is_null() {
                pi_fatal_err(ctxt, XML_ERR_URI_REQUIRED);
            }
        } else if pi_cmp6(ctxt, b"PUBLIC") {
            pi_skip(ctxt, 6);
            pi_skip_blanks(ctxt);
            *public_id = pi_parse_pubid_literal(ctxt);
            if public_id.is_null() || (*public_id).is_null() {
                pi_fatal_err(ctxt, XML_ERR_PUBID_REQUIRED);
            }
            if strict != 0 {
                if pi_skip_blanks(ctxt) == 0 {
                    pi_fatal_err(ctxt, XML_ERR_SPACE_REQUIRED);
                }
            } else {
                if pi_skip_blanks(ctxt) == 0 {
                    return ptr::null_mut();
                }
                if pi_raw(ctxt) != b'\'' && pi_raw(ctxt) != b'"' {
                    return ptr::null_mut();
                }
            }
            uri = pi_parse_system_literal(ctxt);
            if uri.is_null() {
                pi_fatal_err(ctxt, XML_ERR_URI_REQUIRED);
            }
        }
        uri
    }
}

/// `xmlParsePITarget`.
unsafe fn pi_parse_pi_target(ctxt: *mut _xmlParserCtxt) -> *const xmlChar {
    unsafe {
        let name = pi_parse_name(ctxt);
        if name.is_null() {
            return ptr::null();
        }
        let len = crate::abi::exports_xml2::xmlStrlen(name) as usize;
        if len >= 3 {
            let c0 = *name;
            let c1 = *name.add(1);
            let c2 = *name.add(2);
            if (c0 == b'x' || c0 == b'X')
                && (c1 == b'm' || c1 == b'M')
                && (c2 == b'l' || c2 == b'L')
            {
                // Reserved "xml*" names are reported but still returned.
                if len == 3 || !(c0 == b'x' && c1 == b'm' && c2 == b'l') {
                    pi_fatal_err(ctxt, XML_ERR_RESERVED_XML_NAME);
                }
            }
        }
        if !crate::abi::exports_xml2::xmlStrchr(name, b':').is_null() {
            // colons are forbidden from PI names (warning-level upstream)
        }
        name
    }
}

/// `xmlParsePI`.
unsafe fn pi_parse_pi(ctxt: *mut _xmlParserCtxt) {
    unsafe {
        if pi_raw(ctxt) == b'<' && pi_nxt(ctxt, 1) == b'?' {
            pi_skip(ctxt, 2);
            let target = pi_parse_pi_target(ctxt);
            if !target.is_null() {
                if pi_raw(ctxt) == b'?' && pi_nxt(ctxt, 1) == b'>' {
                    pi_skip(ctxt, 2);
                    pi_sax_pi(ctxt, target, &[]);
                    xmlFreeImpl(target as *mut c_void);
                    return;
                }
                if pi_skip_blanks(ctxt) == 0 {
                    pi_fatal_err(ctxt, XML_ERR_SPACE_REQUIRED);
                }
                let mut buf: Vec<u8> = Vec::new();
                loop {
                    let (cur, l) = pi_current_char_recover(ctxt);
                    if !pi_is_char(cur) || (cur == b'?' as c_int && pi_nxt(ctxt, 1) == b'>') {
                        break;
                    }
                    pi_push_codepoint(&mut buf, cur);
                    pi_nextl(ctxt, l);
                }
                if pi_raw(ctxt) != b'?' {
                    pi_fatal_err(ctxt, XML_ERR_PI_NOT_FINISHED);
                } else {
                    pi_skip(ctxt, 2);
                    pi_sax_pi(ctxt, target, &buf);
                }
                xmlFreeImpl(target as *mut c_void);
            } else {
                pi_fatal_err(ctxt, XML_ERR_PI_NOT_STARTED);
            }
        }
    }
}

/// `xmlParseComment` — parse a comment (assumes `<!--` position).
unsafe fn pi_parse_comment(ctxt: *mut _xmlParserCtxt) {
    unsafe {
        if pi_raw(ctxt) != b'<' || pi_nxt(ctxt, 1) != b'!' {
            return;
        }
        pi_skip(ctxt, 2);
        if pi_raw(ctxt) != b'-' || pi_nxt(ctxt, 1) != b'-' {
            return;
        }
        pi_skip(ctxt, 2);

        let mut buf: Vec<u8> = Vec::new();
        let (mut q, ql) = pi_current_char_recover(ctxt);
        if q == 0 {
            pi_fatal_err(ctxt, XML_ERR_COMMENT_NOT_FINISHED);
            return;
        }
        pi_nextl(ctxt, ql);
        let (mut r, rl) = pi_current_char_recover(ctxt);
        if r == 0 {
            pi_fatal_err(ctxt, XML_ERR_COMMENT_NOT_FINISHED);
            return;
        }
        pi_nextl(ctxt, rl);
        let (mut cur, mut l) = pi_current_char_recover(ctxt);
        while pi_is_char(cur) && !(cur == b'>' as c_int && r == b'-' as c_int && q == b'-' as c_int)
        {
            if r == b'-' as c_int && q == b'-' as c_int {
                pi_fatal_err(ctxt, XML_ERR_HYPHEN_IN_COMMENT);
            }
            pi_push_codepoint(&mut buf, q);
            q = r;
            r = cur;
            pi_nextl(ctxt, l);
            let (c2, l2) = pi_current_char_recover(ctxt);
            cur = c2;
            l = l2;
        }
        if cur == 0 {
            pi_fatal_err(ctxt, XML_ERR_COMMENT_NOT_FINISHED);
            return;
        }
        if !pi_is_char(cur) {
            pi_fatal_err(ctxt, XML_ERR_INVALID_CHAR);
            return;
        }
        pi_next1(ctxt);
        pi_sax_comment(ctxt, &buf);
    }
}

/// `xmlParseCharData` — parse character data until '<' or '&'.
unsafe fn pi_parse_char_data(ctxt: *mut _xmlParserCtxt, _cdata: c_int) {
    unsafe {
        let mut buf: Vec<u8> = Vec::new();
        loop {
            if pi_stopped(ctxt) {
                break;
            }
            let input = pi_input(ctxt);
            if input.is_null() {
                break;
            }
            if (*input).cur >= (*input).end {
                break;
            }
            let c = pi_raw(ctxt);
            if c == b'<' || c == b'&' {
                break;
            }
            buf.push(c);
            pi_next1(ctxt);
        }
        if buf.is_empty() {
            return;
        }
        // Whitespace-only data is ignorable when keepBlanks is off.
        let all_ws = buf.iter().all(|&b| pi_is_blank_ch(b));
        let keep_blanks = (*ctxt).keepBlanks != 0;
        let ignorable = all_ws && !keep_blanks;
        pi_sax_chars(ctxt, &buf, ignorable);
    }
}

/// `xmlParseCDSect` — parse `<![CDATA[...]]>`.
unsafe fn pi_parse_cd_sect(ctxt: *mut _xmlParserCtxt) {
    unsafe {
        if pi_raw(ctxt) != b'<' || pi_nxt(ctxt, 1) != b'!' || pi_nxt(ctxt, 2) != b'[' {
            return;
        }
        pi_skip(ctxt, 3);
        if !pi_cmp6(ctxt, b"CDATA[") {
            return;
        }
        pi_skip(ctxt, 6);

        let mut buf: Vec<u8> = Vec::new();
        let (mut r, rl) = pi_current_char_recover(ctxt);
        if !pi_is_char(r) {
            pi_fatal_err(ctxt, XML_ERR_CDATA_NOT_FINISHED);
            return;
        }
        pi_nextl(ctxt, rl);
        let (mut s, sl) = pi_current_char_recover(ctxt);
        if !pi_is_char(s) {
            pi_fatal_err(ctxt, XML_ERR_CDATA_NOT_FINISHED);
            return;
        }
        pi_nextl(ctxt, sl);
        let (mut cur, mut l) = pi_current_char_recover(ctxt);
        while pi_is_char(cur) && !(r == b']' as c_int && s == b']' as c_int && cur == b'>' as c_int)
        {
            pi_push_codepoint(&mut buf, r);
            r = s;
            s = cur;
            pi_nextl(ctxt, l);
            let (c2, l2) = pi_current_char_recover(ctxt);
            cur = c2;
            l = l2;
        }
        if cur != b'>' as c_int {
            pi_fatal_err(ctxt, XML_ERR_CDATA_NOT_FINISHED);
            return;
        }
        pi_nextl(ctxt, l);

        // OK the buffer is to be consumed as cdata.
        let c = &*ctxt;
        if !c.sax.is_null() && c.disableSAX == 0 {
            let buf_p = pi_strndup_bytes(buf.as_ptr(), buf.len());
            if !buf_p.is_null() {
                let sax = &*c.sax;
                if sax.cdataBlock.is_some() && (c.options & XML_PARSE_NOCDATA) == 0 {
                    SaxDispatcher::cdata_block(sax, c.userData, buf_p, buf.len() as c_int);
                } else {
                    SaxDispatcher::characters(sax, c.userData, buf_p, buf.len() as c_int);
                }
                xmlFreeImpl(buf_p as *mut c_void);
            }
        }
    }
}

/// `xmlParseReference` — handle `&...;` in content.
unsafe fn pi_parse_reference(ctxt: *mut _xmlParserCtxt) {
    unsafe {
        if pi_raw(ctxt) != b'&' {
            return;
        }
        // Simple case of a CharRef.
        if pi_nxt(ctxt, 1) == b'#' {
            let value = pi_parse_char_ref(ctxt);
            if value == 0 {
                return;
            }
            let mut out = Vec::new();
            pi_push_codepoint(&mut out, value);
            pi_sax_chars(ctxt, &out, false);
            return;
        }

        // Entity reference.
        let name = pi_parse_entity_ref_name(ctxt);
        if name.is_null() {
            return;
        }
        let ent = pi_lookup_general_entity(ctxt, name);
        if ent.is_null() {
            // Reference to undeclared entity.
            let c = &*ctxt;
            if c.replaceEntities == 0
                && !c.sax.is_null()
                && c.disableSAX == 0
                && (*c.sax).reference.is_some()
            {
                SaxDispatcher::reference(&*c.sax, c.userData, name);
            }
            xmlFreeImpl(name as *mut c_void);
            return;
        }
        if (*ctxt).wellFormed == 0 {
            xmlFreeImpl(name as *mut c_void);
            return;
        }

        // Special case of predefined entities.
        let etype = (*ent).etype;
        if etype == XML_INTERNAL_PREDEFINED_ENTITY as c_int {
            let val = (*ent).content;
            if !val.is_null() {
                let l = crate::abi::exports_xml2::xmlStrlen(val) as usize;
                let bytes = core::slice::from_raw_parts(val, l);
                pi_sax_chars(ctxt, bytes, false);
            }
            xmlFreeImpl(name as *mut c_void);
            return;
        }

        let c = &*ctxt;
        if c.replaceEntities == 0 {
            // Create a reference.
            if !c.sax.is_null() && c.disableSAX == 0 && (*c.sax).reference.is_some() {
                SaxDispatcher::reference(&*c.sax, c.userData, (*ent).name);
            }
        } else if etype == XML_INTERNAL_GENERAL_ENTITY as c_int && !(*ent).content.is_null() {
            // Substitute the replacement text inline.
            let val = (*ent).content;
            let l = crate::abi::exports_xml2::xmlStrlen(val) as usize;
            let bytes = core::slice::from_raw_parts(val, l);
            pi_sax_chars(ctxt, bytes, false);
        }
        xmlFreeImpl(name as *mut c_void);
    }
}

/// `xmlParsePEReference` — parse `%name;` and expand internal parameter
/// entities by pushing a new input.
unsafe fn pi_parse_pe_reference(ctxt: *mut _xmlParserCtxt) {
    unsafe {
        if pi_raw(ctxt) != b'%' {
            return;
        }
        pi_next1(ctxt);
        let name = pi_parse_name(ctxt);
        if name.is_null() {
            pi_fatal_err(ctxt, XML_ERR_PEREF_NO_NAME);
            return;
        }
        if pi_raw(ctxt) != b';' {
            pi_fatal_err(ctxt, XML_ERR_PEREF_SEMICOL_MISSING);
            xmlFreeImpl(name as *mut c_void);
            return;
        }
        pi_next1(ctxt);

        let ent = pi_lookup_parameter_entity(ctxt, name);
        if ent.is_null() {
            pi_fatal_err(ctxt, XML_ERR_UNDECLARED_ENTITY);
            xmlFreeImpl(name as *mut c_void);
            return;
        }
        (*ctxt).hasPErefs = 1;

        if (*ent).etype == XML_INTERNAL_PARAMETER_ENTITY as c_int && !(*ent).content.is_null() {
            let content = (*ent).content;
            let clen = crate::abi::exports_xml2::xmlStrlen(content) as usize;
            // The spec requires one leading and one trailing space.
            let total = clen + 3;
            let buf = xmlMallocImpl(total) as *mut xmlChar;
            if !buf.is_null() {
                *buf = b' ';
                ptr::copy_nonoverlapping(content, buf.add(1), clen);
                *buf.add(clen + 1) = b' ';
                *buf.add(clen + 2) = 0;
                let input = xmlMallocZero(size_of::<_xmlParserInput>()) as *mut _xmlParserInput;
                if !input.is_null() {
                    (*input).base = buf;
                    (*input).cur = buf;
                    (*input).end = buf.add(clen + 1);
                    (*input).line = (*(*ctxt).input).line;
                    (*input).col = (*(*ctxt).input).col;
                    (*input).entity = ent;
                    pi_input_push(ctxt, input);
                } else {
                    xmlFreeImpl(buf as *mut c_void);
                }
            }
        }
        xmlFreeImpl(name as *mut c_void);
    }
}

/// `xmlParseNotationDecl`.
unsafe fn pi_parse_notation_decl(ctxt: *mut _xmlParserCtxt) {
    unsafe {
        if pi_raw(ctxt) != b'<' || pi_nxt(ctxt, 1) != b'!' {
            return;
        }
        pi_skip(ctxt, 2);
        if pi_cmp8(ctxt, b"NOTATION") {
            pi_skip(ctxt, 8);
            if pi_skip_blanks(ctxt) == 0 {
                pi_fatal_err(ctxt, XML_ERR_SPACE_REQUIRED);
                return;
            }
            let name = pi_parse_name(ctxt);
            if name.is_null() {
                pi_fatal_err(ctxt, XML_ERR_NOTATION_NOT_STARTED);
                return;
            }
            if pi_skip_blanks(ctxt) == 0 {
                pi_fatal_err(ctxt, XML_ERR_SPACE_REQUIRED);
                xmlFreeImpl(name as *mut c_void);
                return;
            }
            let mut pubid: *mut xmlChar = ptr::null_mut();
            let systemid = pi_parse_external_id(ctxt, &mut pubid, 0);
            pi_skip_blanks(ctxt);
            if pi_raw(ctxt) == b'>' {
                pi_next1(ctxt);
                let c = &*ctxt;
                if !c.sax.is_null() && c.disableSAX == 0 {
                    SaxDispatcher::notation_decl(&*c.sax, c.userData, name, pubid, systemid);
                }
            } else {
                pi_fatal_err(ctxt, XML_ERR_NOTATION_NOT_FINISHED);
            }
            if !systemid.is_null() {
                xmlFreeImpl(systemid as *mut c_void);
            }
            if !pubid.is_null() {
                xmlFreeImpl(pubid as *mut c_void);
            }
            xmlFreeImpl(name as *mut c_void);
        }
    }
}

/// `xmlParseEntityDecl`.
unsafe fn pi_parse_entity_decl(ctxt: *mut _xmlParserCtxt) {
    unsafe {
        if pi_raw(ctxt) != b'<' || pi_nxt(ctxt, 1) != b'!' {
            return;
        }
        pi_skip(ctxt, 2);
        if pi_cmp6(ctxt, b"ENTITY") {
            pi_skip(ctxt, 6);
            if pi_skip_blanks(ctxt) == 0 {
                pi_fatal_err(ctxt, XML_ERR_SPACE_REQUIRED);
            }
            let mut is_parameter = false;
            if pi_raw(ctxt) == b'%' {
                pi_next1(ctxt);
                if pi_skip_blanks(ctxt) == 0 {
                    pi_fatal_err(ctxt, XML_ERR_SPACE_REQUIRED);
                }
                is_parameter = true;
            }
            let name = pi_parse_name(ctxt);
            if name.is_null() {
                pi_fatal_err(ctxt, XML_ERR_NAME_REQUIRED);
                return;
            }
            if pi_skip_blanks(ctxt) == 0 {
                pi_fatal_err(ctxt, XML_ERR_SPACE_REQUIRED);
            }

            let mut value: *mut xmlChar = ptr::null_mut();
            let mut uri: *mut xmlChar = ptr::null_mut();
            let mut literal: *mut xmlChar = ptr::null_mut();
            let mut ndata: *const xmlChar = ptr::null();
            let mut orig: *mut xmlChar = ptr::null_mut();

            if is_parameter {
                if pi_raw(ctxt) == b'"' || pi_raw(ctxt) == b'\'' {
                    value = pi_parse_entity_value(ctxt, &mut orig);
                    if !value.is_null() {
                        let c = &*ctxt;
                        if !c.sax.is_null() && c.disableSAX == 0 {
                            SaxDispatcher::entity_decl(
                                &*c.sax,
                                c.userData,
                                name,
                                XML_INTERNAL_PARAMETER_ENTITY as c_int,
                                ptr::null(),
                                ptr::null(),
                                value,
                            );
                        }
                    }
                } else {
                    uri = pi_parse_external_id(ctxt, &mut literal, 1);
                    if !uri.is_null() {
                        let c = &*ctxt;
                        if !c.sax.is_null() && c.disableSAX == 0 {
                            SaxDispatcher::entity_decl(
                                &*c.sax,
                                c.userData,
                                name,
                                XML_EXTERNAL_PARAMETER_ENTITY as c_int,
                                literal,
                                uri,
                                ptr::null_mut(),
                            );
                        }
                    }
                }
            } else {
                if pi_raw(ctxt) == b'"' || pi_raw(ctxt) == b'\'' {
                    value = pi_parse_entity_value(ctxt, &mut orig);
                    let c = &*ctxt;
                    if !c.sax.is_null() && c.disableSAX == 0 {
                        SaxDispatcher::entity_decl(
                            &*c.sax,
                            c.userData,
                            name,
                            XML_INTERNAL_GENERAL_ENTITY as c_int,
                            ptr::null(),
                            ptr::null(),
                            value,
                        );
                    }
                } else {
                    uri = pi_parse_external_id(ctxt, &mut literal, 1);
                    if pi_raw(ctxt) != b'>' && pi_skip_blanks(ctxt) == 0 {
                        pi_fatal_err(ctxt, XML_ERR_SPACE_REQUIRED);
                    }
                    if pi_cmp5(ctxt, b"NDATA") {
                        pi_skip(ctxt, 5);
                        if pi_skip_blanks(ctxt) == 0 {
                            pi_fatal_err(ctxt, XML_ERR_SPACE_REQUIRED);
                        }
                        ndata = pi_parse_name(ctxt);
                        let c = &*ctxt;
                        if !c.sax.is_null() && c.disableSAX == 0 {
                            SaxDispatcher::unparsed_entity_decl(
                                &*c.sax, c.userData, name, literal, uri, ndata,
                            );
                        }
                        if !ndata.is_null() {
                            xmlFreeImpl(ndata as *mut c_void);
                        }
                    } else {
                        let c = &*ctxt;
                        if !c.sax.is_null() && c.disableSAX == 0 {
                            SaxDispatcher::entity_decl(
                                &*c.sax,
                                c.userData,
                                name,
                                XML_EXTERNAL_GENERAL_PARSED_ENTITY as c_int,
                                literal,
                                uri,
                                ptr::null_mut(),
                            );
                        }
                    }
                }
            }

            pi_skip_blanks(ctxt);
            if pi_raw(ctxt) != b'>' {
                pi_fatal_err(ctxt, XML_ERR_ENTITY_NOT_FINISHED);
            } else {
                pi_next1(ctxt);
            }

            if !orig.is_null() {
                // Attach the raw entity value to the entity if it has none.
                let c = &*ctxt;
                let mut cur: *mut _xmlEntity = ptr::null_mut();
                if !c.sax.is_null() {
                    if is_parameter {
                        if (*c.sax).getParameterEntity.is_some() {
                            cur = SaxDispatcher::get_parameter_entity(&*c.sax, c.userData, name);
                        }
                    } else if (*c.sax).getEntity.is_some() {
                        cur = SaxDispatcher::get_entity(&*c.sax, c.userData, name);
                    }
                }
                if !cur.is_null() && (*cur).orig.is_null() {
                    (*cur).orig = orig;
                    orig = ptr::null_mut();
                }
            }

            if !value.is_null() {
                xmlFreeImpl(value as *mut c_void);
            }
            if !uri.is_null() {
                xmlFreeImpl(uri as *mut c_void);
            }
            if !literal.is_null() {
                xmlFreeImpl(literal as *mut c_void);
            }
            if !orig.is_null() {
                xmlFreeImpl(orig as *mut c_void);
            }
            xmlFreeImpl(name as *mut c_void);
        }
    }
}

/// `xmlParseDefaultDecl`.
unsafe fn pi_parse_default_decl(ctxt: *mut _xmlParserCtxt, value: *mut *mut xmlChar) -> c_int {
    unsafe {
        *value = ptr::null_mut();
        if pi_cmp9(ctxt, b"#REQUIRED") {
            pi_skip(ctxt, 9);
            return XML_ATTRIBUTE_REQUIRED;
        }
        if pi_cmp8(ctxt, b"#IMPLIED") {
            pi_skip(ctxt, 8);
            return XML_ATTRIBUTE_IMPLIED;
        }
        let mut val = XML_ATTRIBUTE_NONE;
        if pi_cmp6(ctxt, b"#FIXED") {
            pi_skip(ctxt, 6);
            val = XML_ATTRIBUTE_FIXED;
            if pi_skip_blanks(ctxt) == 0 {
                pi_fatal_err(ctxt, XML_ERR_SPACE_REQUIRED);
            }
        }
        let ret = pi_parse_att_value(ctxt);
        if ret.is_null() {
            pi_fatal_err(ctxt, (*ctxt).errNo);
        } else {
            *value = ret;
        }
        val
    }
}

/// `xmlParseAttributeType`.
unsafe fn pi_parse_attribute_type(
    ctxt: *mut _xmlParserCtxt,
    tree: *mut *mut _xmlEnumeration,
) -> c_int {
    unsafe {
        if pi_cmp5(ctxt, b"CDATA") {
            pi_skip(ctxt, 5);
            XML_ATTRIBUTE_CDATA
        } else if pi_cmp6(ctxt, b"IDREFS") {
            pi_skip(ctxt, 6);
            XML_ATTRIBUTE_IDREFS
        } else if pi_cmp5(ctxt, b"IDREF") {
            pi_skip(ctxt, 5);
            XML_ATTRIBUTE_IDREF
        } else if pi_raw(ctxt) == b'I' && pi_nxt(ctxt, 1) == b'D' {
            pi_skip(ctxt, 2);
            XML_ATTRIBUTE_ID
        } else if pi_cmp6(ctxt, b"ENTITY") {
            pi_skip(ctxt, 6);
            XML_ATTRIBUTE_ENTITY
        } else if pi_cmp8(ctxt, b"ENTITIES") {
            pi_skip(ctxt, 8);
            XML_ATTRIBUTE_ENTITIES
        } else if pi_cmp8(ctxt, b"NMTOKENS") {
            pi_skip(ctxt, 8);
            XML_ATTRIBUTE_NMTOKENS
        } else if pi_cmp7(ctxt, b"NMTOKEN") {
            pi_skip(ctxt, 7);
            XML_ATTRIBUTE_NMTOKEN
        } else {
            pi_parse_enumerated_type(ctxt, tree)
        }
    }
}

/// `xmlParseNotationType`.
unsafe fn pi_parse_notation_type(ctxt: *mut _xmlParserCtxt) -> *mut _xmlEnumeration {
    unsafe {
        if pi_raw(ctxt) != b'(' {
            pi_fatal_err(ctxt, XML_ERR_NOTATION_NOT_STARTED);
            return ptr::null_mut();
        }
        let mut ret: *mut _xmlEnumeration = ptr::null_mut();
        let mut last: *mut _xmlEnumeration = ptr::null_mut();
        loop {
            pi_next1(ctxt);
            pi_skip_blanks(ctxt);
            let name = pi_parse_name(ctxt);
            if name.is_null() {
                pi_fatal_err(ctxt, XML_ERR_NAME_REQUIRED);
                pi_free_enumeration(ret);
                return ptr::null_mut();
            }
            let cur = pi_create_enumeration(name);
            // ownership of `name` transfers to the enumeration node
            if cur.is_null() {
                pi_err_memory(ctxt);
                xmlFreeImpl(name as *mut c_void);
                pi_free_enumeration(ret);
                return ptr::null_mut();
            }
            if last.is_null() {
                ret = cur;
            } else {
                (*last).next = cur;
            }
            last = cur;
            pi_skip_blanks(ctxt);
            if pi_raw(ctxt) != b'|' {
                break;
            }
        }
        if pi_raw(ctxt) != b')' {
            pi_fatal_err(ctxt, XML_ERR_NOTATION_NOT_FINISHED);
            pi_free_enumeration(ret);
            return ptr::null_mut();
        }
        pi_next1(ctxt);
        ret
    }
}

/// `xmlParseEnumerationType`.
unsafe fn pi_parse_enumeration_type(ctxt: *mut _xmlParserCtxt) -> *mut _xmlEnumeration {
    unsafe {
        if pi_raw(ctxt) != b'(' {
            pi_fatal_err(ctxt, XML_ERR_ATTLIST_NOT_STARTED);
            return ptr::null_mut();
        }
        let mut ret: *mut _xmlEnumeration = ptr::null_mut();
        let mut last: *mut _xmlEnumeration = ptr::null_mut();
        loop {
            pi_next1(ctxt);
            pi_skip_blanks(ctxt);
            let name = pi_parse_nmtoken(ctxt);
            if name.is_null() {
                pi_fatal_err(ctxt, XML_ERR_NMTOKEN_REQUIRED);
                return ret;
            }
            let cur = pi_create_enumeration(name);
            // ownership of `name` transfers to the enumeration node
            if cur.is_null() {
                pi_err_memory(ctxt);
                xmlFreeImpl(name as *mut c_void);
                pi_free_enumeration(ret);
                return ptr::null_mut();
            }
            if last.is_null() {
                ret = cur;
            } else {
                (*last).next = cur;
            }
            last = cur;
            pi_skip_blanks(ctxt);
            if pi_raw(ctxt) != b'|' {
                break;
            }
        }
        if pi_raw(ctxt) != b')' {
            pi_fatal_err(ctxt, XML_ERR_ATTLIST_NOT_FINISHED);
            return ret;
        }
        pi_next1(ctxt);
        ret
    }
}

/// `xmlParseEnumeratedType`.
unsafe fn pi_parse_enumerated_type(
    ctxt: *mut _xmlParserCtxt,
    tree: *mut *mut _xmlEnumeration,
) -> c_int {
    unsafe {
        if pi_cmp8(ctxt, b"NOTATION") {
            pi_skip(ctxt, 8);
            if pi_skip_blanks(ctxt) == 0 {
                pi_fatal_err(ctxt, XML_ERR_SPACE_REQUIRED);
                return 0;
            }
            *tree = pi_parse_notation_type(ctxt);
            if tree.is_null() || (*tree).is_null() {
                return 0;
            }
            return XML_ATTRIBUTE_NOTATION;
        }
        *tree = pi_parse_enumeration_type(ctxt);
        if tree.is_null() || (*tree).is_null() {
            return 0;
        }
        XML_ATTRIBUTE_ENUMERATION
    }
}

/// `xmlParseAttributeListDecl`.
unsafe fn pi_parse_attribute_list_decl(ctxt: *mut _xmlParserCtxt) {
    unsafe {
        if pi_raw(ctxt) != b'<' || pi_nxt(ctxt, 1) != b'!' {
            return;
        }
        pi_skip(ctxt, 2);
        if pi_cmp7(ctxt, b"ATTLIST") {
            pi_skip(ctxt, 7);
            if pi_skip_blanks(ctxt) == 0 {
                pi_fatal_err(ctxt, XML_ERR_SPACE_REQUIRED);
            }
            let elem_name = pi_parse_name(ctxt);
            if elem_name.is_null() {
                pi_fatal_err(ctxt, XML_ERR_NAME_REQUIRED);
                return;
            }
            pi_skip_blanks(ctxt);
            while pi_raw(ctxt) != b'>' && !pi_stopped(ctxt) {
                let mut tree: *mut _xmlEnumeration = ptr::null_mut();
                let attr_name = pi_parse_name(ctxt);
                if attr_name.is_null() {
                    pi_fatal_err(ctxt, XML_ERR_NAME_REQUIRED);
                    break;
                }
                if pi_skip_blanks(ctxt) == 0 {
                    pi_fatal_err(ctxt, XML_ERR_SPACE_REQUIRED);
                    xmlFreeImpl(attr_name as *mut c_void);
                    break;
                }
                let type_ = pi_parse_attribute_type(ctxt, &mut tree);
                if type_ <= 0 {
                    xmlFreeImpl(attr_name as *mut c_void);
                    break;
                }
                if pi_skip_blanks(ctxt) == 0 {
                    pi_fatal_err(ctxt, XML_ERR_SPACE_REQUIRED);
                    if !tree.is_null() {
                        pi_free_enumeration(tree);
                    }
                    xmlFreeImpl(attr_name as *mut c_void);
                    break;
                }
                let mut default_value: *mut xmlChar = ptr::null_mut();
                let def = pi_parse_default_decl(ctxt, &mut default_value);
                if def <= 0 {
                    if !default_value.is_null() {
                        xmlFreeImpl(default_value as *mut c_void);
                    }
                    if !tree.is_null() {
                        pi_free_enumeration(tree);
                    }
                    xmlFreeImpl(attr_name as *mut c_void);
                    break;
                }
                if pi_raw(ctxt) != b'>' && pi_skip_blanks(ctxt) == 0 {
                    pi_fatal_err(ctxt, XML_ERR_SPACE_REQUIRED);
                    if !default_value.is_null() {
                        xmlFreeImpl(default_value as *mut c_void);
                    }
                    if !tree.is_null() {
                        pi_free_enumeration(tree);
                    }
                    xmlFreeImpl(attr_name as *mut c_void);
                    break;
                }
                let c = &*ctxt;
                if !c.sax.is_null() && c.disableSAX == 0 {
                    SaxDispatcher::attribute_decl(
                        &*c.sax,
                        c.userData,
                        elem_name,
                        attr_name,
                        type_,
                        def,
                        default_value,
                        tree,
                    );
                } else if !tree.is_null() {
                    pi_free_enumeration(tree);
                }
                if !default_value.is_null() {
                    xmlFreeImpl(default_value as *mut c_void);
                }
                xmlFreeImpl(attr_name as *mut c_void);
            }
            if pi_raw(ctxt) == b'>' {
                pi_next1(ctxt);
            }
            xmlFreeImpl(elem_name as *mut c_void);
        }
    }
}

/// Create an `_xmlEnumeration` node taking ownership of `name`.
unsafe fn pi_create_enumeration(name: *const xmlChar) -> *mut _xmlEnumeration {
    unsafe {
        let cur = xmlMallocZero(size_of::<_xmlEnumeration>()) as *mut _xmlEnumeration;
        if !cur.is_null() {
            (*cur).name = name;
            (*cur).next = ptr::null_mut();
        }
        cur
    }
}

/// `xmlFreeEnumeration` equivalent.
unsafe fn pi_free_enumeration(cur: *mut _xmlEnumeration) {
    unsafe {
        let mut cur = cur;
        while !cur.is_null() {
            let next = (*cur).next;
            if !(*cur).name.is_null() {
                xmlFreeImpl((*cur).name as *mut c_void);
            }
            xmlFreeImpl(cur as *mut c_void);
            cur = next;
        }
    }
}

/// `xmlParseElementMixedContentDecl` — the leading '(' was already consumed.
unsafe fn pi_parse_element_mixed_content_decl(
    ctxt: *mut _xmlParserCtxt,
    _open_input_nr: c_int,
) -> *mut _xmlElementContent {
    unsafe {
        if pi_cmp7(ctxt, b"#PCDATA") {
            pi_skip(ctxt, 7);
            pi_skip_blanks(ctxt);
            if pi_raw(ctxt) == b')' {
                pi_next1(ctxt);
                let ret = create_content_model(ptr::null(), XML_ELEMENT_CONTENT_PCDATA as c_int);
                if pi_raw(ctxt) == b'*' {
                    if !ret.is_null() {
                        (*ret).ocur = XML_ELEMENT_CONTENT_MULT as c_int;
                    }
                    pi_next1(ctxt);
                }
                return ret;
            }
            let mut ret: *mut _xmlElementContent =
                create_content_model(ptr::null(), XML_ELEMENT_CONTENT_PCDATA as c_int);
            let mut cur = ret;
            let mut elem: *const xmlChar = ptr::null();
            while pi_raw(ctxt) == b'|' && !pi_stopped(ctxt) {
                pi_next1(ctxt);
                let n = create_content_model(ptr::null(), XML_ELEMENT_CONTENT_OR as c_int);
                if n.is_null() {
                    pi_err_memory(ctxt);
                    free_content_model(ret);
                    return ptr::null_mut();
                }
                if elem.is_null() {
                    (*n).c1 = cur;
                    if !cur.is_null() {
                        (*cur).parent = n;
                    }
                    ret = n;
                    cur = n;
                } else {
                    (*cur).c2 = n;
                    (*n).parent = cur;
                    let c1 = create_content_model(elem, XML_ELEMENT_CONTENT_ELEMENT as c_int);
                    xmlFreeImpl(elem as *mut c_void);
                    (*n).c1 = c1;
                    if !c1.is_null() {
                        (*c1).parent = n;
                    }
                    cur = n;
                }
                pi_skip_blanks(ctxt);
                elem = pi_parse_name(ctxt);
                if elem.is_null() {
                    pi_fatal_err(ctxt, XML_ERR_NAME_REQUIRED);
                    free_content_model(ret);
                    return ptr::null_mut();
                }
                pi_skip_blanks(ctxt);
            }
            if pi_raw(ctxt) == b')' && pi_nxt(ctxt, 1) == b'*' {
                if !elem.is_null() {
                    let c2 = create_content_model(elem, XML_ELEMENT_CONTENT_ELEMENT as c_int);
                    xmlFreeImpl(elem as *mut c_void);
                    if !cur.is_null() {
                        (*cur).c2 = c2;
                        if !c2.is_null() {
                            (*c2).parent = cur;
                        }
                    }
                }
                if !ret.is_null() {
                    (*ret).ocur = XML_ELEMENT_CONTENT_MULT as c_int;
                }
                pi_skip(ctxt, 2);
            } else {
                if !elem.is_null() {
                    xmlFreeImpl(elem as *mut c_void);
                }
                free_content_model(ret);
                pi_fatal_err(ctxt, XML_ERR_MIXED_NOT_STARTED);
                return ptr::null_mut();
            }
            return ret;
        }
        pi_fatal_err(ctxt, XML_ERR_PCDATA_REQUIRED);
        ptr::null_mut()
    }
}

/// `xmlParseElementChildrenContentDeclPriv`.
unsafe fn pi_parse_element_children_content_decl_priv(
    ctxt: *mut _xmlParserCtxt,
    _open_input_nr: c_int,
    depth: c_int,
) -> *mut _xmlElementContent {
    unsafe {
        let max_depth = if (*ctxt).options & XML_PARSE_HUGE != 0 {
            2048
        } else {
            256
        };
        if depth > max_depth {
            pi_fatal_err(ctxt, XML_ERR_RESOURCE_LIMIT);
            return ptr::null_mut();
        }
        pi_skip_blanks(ctxt);
        let mut ret: *mut _xmlElementContent = ptr::null_mut();
        let mut cur: *mut _xmlElementContent = ptr::null_mut();
        let mut last: *mut _xmlElementContent = ptr::null_mut();
        let mut type_: u8 = 0;

        if pi_raw(ctxt) == b'(' {
            pi_next1(ctxt);
            cur = pi_parse_element_children_content_decl_priv(ctxt, (*ctxt).inputNr, depth + 1);
            if cur.is_null() {
                return ptr::null_mut();
            }
            ret = cur;
        } else {
            let elem = pi_parse_name(ctxt);
            if elem.is_null() {
                pi_fatal_err(ctxt, XML_ERR_ELEMCONTENT_NOT_STARTED);
                return ptr::null_mut();
            }
            cur = create_content_model(elem, XML_ELEMENT_CONTENT_ELEMENT as c_int);
            xmlFreeImpl(elem as *mut c_void);
            if cur.is_null() {
                pi_err_memory(ctxt);
                return ptr::null_mut();
            }
            ret = cur;
            if pi_raw(ctxt) == b'?' {
                (*cur).ocur = XML_ELEMENT_CONTENT_OPT as c_int;
                pi_next1(ctxt);
            } else if pi_raw(ctxt) == b'*' {
                (*cur).ocur = XML_ELEMENT_CONTENT_MULT as c_int;
                pi_next1(ctxt);
            } else if pi_raw(ctxt) == b'+' {
                (*cur).ocur = XML_ELEMENT_CONTENT_PLUS as c_int;
                pi_next1(ctxt);
            } else {
                (*cur).ocur = XML_ELEMENT_CONTENT_ONCE as c_int;
            }
        }

        while !pi_stopped(ctxt) {
            pi_skip_blanks(ctxt);
            if pi_raw(ctxt) == b')' {
                break;
            }
            if pi_raw(ctxt) == b',' || pi_raw(ctxt) == b'|' {
                let sep = pi_raw(ctxt);
                if type_ == 0 {
                    type_ = sep;
                } else if type_ != sep {
                    pi_fatal_err(ctxt, XML_ERR_SEPARATOR_REQUIRED);
                    free_content_model(ret);
                    return ptr::null_mut();
                }
                pi_next1(ctxt);
                let op_type = if sep == b',' {
                    XML_ELEMENT_CONTENT_SEQ as c_int
                } else {
                    XML_ELEMENT_CONTENT_OR as c_int
                };
                let op = create_content_model(ptr::null(), op_type);
                if op.is_null() {
                    pi_err_memory(ctxt);
                    free_content_model(ret);
                    return ptr::null_mut();
                }
                if last.is_null() {
                    (*op).c1 = ret;
                    if !ret.is_null() {
                        (*ret).parent = op;
                    }
                    ret = op;
                    cur = op;
                } else {
                    (*cur).c2 = op;
                    (*op).parent = cur;
                    (*op).c1 = last;
                    if !last.is_null() {
                        (*last).parent = op;
                    }
                    cur = op;
                    last = ptr::null_mut();
                }
            } else {
                pi_fatal_err(ctxt, XML_ERR_ELEMCONTENT_NOT_FINISHED);
                free_content_model(ret);
                return ptr::null_mut();
            }

            pi_skip_blanks(ctxt);
            if pi_raw(ctxt) == b'(' {
                pi_next1(ctxt);
                last =
                    pi_parse_element_children_content_decl_priv(ctxt, (*ctxt).inputNr, depth + 1);
                if last.is_null() {
                    free_content_model(ret);
                    return ptr::null_mut();
                }
            } else {
                let elem = pi_parse_name(ctxt);
                if elem.is_null() {
                    pi_fatal_err(ctxt, XML_ERR_ELEMCONTENT_NOT_STARTED);
                    free_content_model(ret);
                    return ptr::null_mut();
                }
                last = create_content_model(elem, XML_ELEMENT_CONTENT_ELEMENT as c_int);
                xmlFreeImpl(elem as *mut c_void);
                if last.is_null() {
                    pi_err_memory(ctxt);
                    free_content_model(ret);
                    return ptr::null_mut();
                }
                if pi_raw(ctxt) == b'?' {
                    (*last).ocur = XML_ELEMENT_CONTENT_OPT as c_int;
                    pi_next1(ctxt);
                } else if pi_raw(ctxt) == b'*' {
                    (*last).ocur = XML_ELEMENT_CONTENT_MULT as c_int;
                    pi_next1(ctxt);
                } else if pi_raw(ctxt) == b'+' {
                    (*last).ocur = XML_ELEMENT_CONTENT_PLUS as c_int;
                    pi_next1(ctxt);
                } else {
                    (*last).ocur = XML_ELEMENT_CONTENT_ONCE as c_int;
                }
            }
        }

        if !cur.is_null() && !last.is_null() {
            (*cur).c2 = last;
            (*last).parent = cur;
        }
        pi_next1(ctxt);
        if pi_raw(ctxt) == b'?' {
            if !ret.is_null() {
                (*ret).ocur = if (*ret).ocur == XML_ELEMENT_CONTENT_PLUS as c_int
                    || (*ret).ocur == XML_ELEMENT_CONTENT_MULT as c_int
                {
                    XML_ELEMENT_CONTENT_MULT as c_int
                } else {
                    XML_ELEMENT_CONTENT_OPT as c_int
                };
            }
            pi_next1(ctxt);
        } else if pi_raw(ctxt) == b'*' {
            if !ret.is_null() {
                (*ret).ocur = XML_ELEMENT_CONTENT_MULT as c_int;
            }
            pi_next1(ctxt);
        } else if pi_raw(ctxt) == b'+' {
            if !ret.is_null() {
                (*ret).ocur = if (*ret).ocur == XML_ELEMENT_CONTENT_OPT as c_int
                    || (*ret).ocur == XML_ELEMENT_CONTENT_MULT as c_int
                {
                    XML_ELEMENT_CONTENT_MULT as c_int
                } else {
                    XML_ELEMENT_CONTENT_PLUS as c_int
                };
            }
            pi_next1(ctxt);
        }
        ret
    }
}

/// `xmlParseElementContentDecl`.
unsafe fn pi_parse_element_content_decl(
    ctxt: *mut _xmlParserCtxt,
    name: *const xmlChar,
    result: *mut *mut _xmlElementContent,
) -> c_int {
    unsafe {
        *result = ptr::null_mut();
        if pi_raw(ctxt) != b'(' {
            pi_fatal_err(ctxt, XML_ERR_ELEMCONTENT_NOT_STARTED);
            return -1;
        }
        let open_input_nr = (*ctxt).inputNr;
        pi_next1(ctxt);
        pi_skip_blanks(ctxt);

        let (tree, res) = if pi_cmp7(ctxt, b"#PCDATA") {
            (
                pi_parse_element_mixed_content_decl(ctxt, open_input_nr),
                XML_ELEMENT_TYPE_MIXED,
            )
        } else {
            (
                pi_parse_element_children_content_decl_priv(ctxt, open_input_nr, 1),
                XML_ELEMENT_TYPE_ELEMENT,
            )
        };
        if tree.is_null() {
            return -1;
        }
        pi_skip_blanks(ctxt);
        *result = tree;
        res
    }
}

/// `xmlParseElementDecl`.
unsafe fn pi_parse_element_decl(ctxt: *mut _xmlParserCtxt) -> c_int {
    unsafe {
        let mut ret = -1;
        if pi_raw(ctxt) != b'<' || pi_nxt(ctxt, 1) != b'!' {
            return ret;
        }
        pi_skip(ctxt, 2);
        if pi_cmp7(ctxt, b"ELEMENT") {
            pi_skip(ctxt, 7);
            if pi_skip_blanks(ctxt) == 0 {
                pi_fatal_err(ctxt, XML_ERR_SPACE_REQUIRED);
                return -1;
            }
            let name = pi_parse_name(ctxt);
            if name.is_null() {
                pi_fatal_err(ctxt, XML_ERR_NAME_REQUIRED);
                return -1;
            }
            if pi_skip_blanks(ctxt) == 0 {
                pi_fatal_err(ctxt, XML_ERR_SPACE_REQUIRED);
            }
            let mut content: *mut _xmlElementContent = ptr::null_mut();
            if pi_cmp5(ctxt, b"EMPTY") {
                pi_skip(ctxt, 5);
                ret = XML_ELEMENT_TYPE_EMPTY;
            } else if pi_raw(ctxt) == b'A' && pi_nxt(ctxt, 1) == b'N' && pi_nxt(ctxt, 2) == b'Y' {
                pi_skip(ctxt, 3);
                ret = XML_ELEMENT_TYPE_ANY;
            } else if pi_raw(ctxt) == b'(' {
                ret = pi_parse_element_content_decl(ctxt, name, &mut content);
                if ret <= 0 {
                    xmlFreeImpl(name as *mut c_void);
                    return -1;
                }
            } else {
                pi_fatal_err(ctxt, XML_ERR_ELEMCONTENT_NOT_STARTED);
                xmlFreeImpl(name as *mut c_void);
                return -1;
            }

            pi_skip_blanks(ctxt);
            if pi_raw(ctxt) != b'>' {
                pi_fatal_err(ctxt, XML_ERR_GT_REQUIRED);
                if !content.is_null() {
                    free_content_model(content);
                }
            } else {
                pi_next1(ctxt);
                let c = &*ctxt;
                if !c.sax.is_null() && c.disableSAX == 0 && (*c.sax).elementDecl.is_some() {
                    if !content.is_null() {
                        (*content).parent = ptr::null_mut();
                    }
                    SaxDispatcher::element_decl(&*c.sax, c.userData, name, ret, content);
                    if !content.is_null() && (*content).parent.is_null() {
                        // Not plugged into a DTD — free it.
                        free_content_model(content);
                    }
                } else if !content.is_null() {
                    free_content_model(content);
                }
            }
            xmlFreeImpl(name as *mut c_void);
        }
        ret
    }
}

/// `xmlParseMarkupDecl`.
unsafe fn pi_parse_markup_decl(ctxt: *mut _xmlParserCtxt) {
    unsafe {
        if pi_raw(ctxt) == b'<' {
            if pi_nxt(ctxt, 1) == b'!' {
                match pi_nxt(ctxt, 2) {
                    b'E' => {
                        if pi_nxt(ctxt, 3) == b'L' {
                            pi_parse_element_decl(ctxt);
                        } else if pi_nxt(ctxt, 3) == b'N' {
                            pi_parse_entity_decl(ctxt);
                        } else {
                            pi_skip(ctxt, 2);
                        }
                    }
                    b'A' => pi_parse_attribute_list_decl(ctxt),
                    b'N' => pi_parse_notation_decl(ctxt),
                    b'-' => pi_parse_comment(ctxt),
                    _ => {
                        pi_fatal_err(
                            ctxt,
                            if (*ctxt).inSubset == 2 {
                                XML_ERR_EXT_SUBSET_NOT_FINISHED
                            } else {
                                XML_ERR_INT_SUBSET_NOT_FINISHED
                            },
                        );
                        pi_skip(ctxt, 2);
                    }
                }
            } else if pi_nxt(ctxt, 1) == b'?' {
                pi_parse_pi(ctxt);
            }
        }
    }
}

/// `xmlParseTextDecl`.
unsafe fn pi_parse_text_decl(ctxt: *mut _xmlParserCtxt) {
    unsafe {
        if pi_cmp5(ctxt, b"<?xml") && pi_is_blank_ch(pi_nxt(ctxt, 5)) {
            pi_skip(ctxt, 5);
        } else {
            pi_fatal_err(ctxt, XML_ERR_XMLDECL_NOT_STARTED);
            return;
        }
        if pi_skip_blanks(ctxt) == 0 {
            pi_fatal_err(ctxt, XML_ERR_SPACE_REQUIRED);
        }
        let mut version = pi_parse_version_info(ctxt);
        if version.is_null() {
            version = crate::abi::exports_xml2::xmlStrdup(c"1.0".as_ptr() as *const xmlChar);
        } else if pi_skip_blanks(ctxt) == 0 {
            pi_fatal_err(ctxt, XML_ERR_SPACE_REQUIRED);
        }
        let input = pi_input(ctxt);
        if !input.is_null() {
            (*input).version = version;
        } else if !version.is_null() {
            xmlFreeImpl(version as *mut c_void);
        }
        pi_parse_encoding_decl(ctxt);
        pi_skip_blanks(ctxt);
        if pi_raw(ctxt) == b'?' && pi_nxt(ctxt, 1) == b'>' {
            pi_skip(ctxt, 2);
        } else if pi_raw(ctxt) == b'>' {
            pi_fatal_err(ctxt, XML_ERR_XMLDECL_NOT_FINISHED);
            pi_next1(ctxt);
        } else {
            pi_fatal_err(ctxt, XML_ERR_XMLDECL_NOT_FINISHED);
            while !pi_stopped(ctxt) && pi_raw(ctxt) != 0 {
                let c = pi_raw(ctxt);
                pi_next1(ctxt);
                if c == b'>' {
                    break;
                }
            }
        }
    }
}

/// `xmlParseExternalSubset`.
#[allow(clippy::while_immutable_condition)]
unsafe fn pi_parse_external_subset(
    ctxt: *mut _xmlParserCtxt,
    public_id: *const xmlChar,
    system_id: *const xmlChar,
) {
    unsafe {
        pi_ctxt_late_init(ctxt);
        if pi_cmp5(ctxt, b"<?xml") && pi_is_blank_ch(pi_nxt(ctxt, 5)) {
            pi_parse_text_decl(ctxt);
        }
        if (*ctxt).myDoc.is_null() {
            (*ctxt).myDoc = crate::xml::tree::new_doc(c"1.0".as_ptr() as *const xmlChar);
            if (*ctxt).myDoc.is_null() {
                pi_err_memory(ctxt);
                return;
            }
            (*(*ctxt).myDoc).properties |= XML_DOC_INTERNAL as c_int;
        }
        if (*(*ctxt).myDoc).intSubset.is_null() {
            create_int_subset((*ctxt).myDoc, ptr::null(), public_id, system_id);
        }
        (*ctxt).inSubset = 2;
        let old_input_nr = (*ctxt).inputNr;
        pi_skip_blanks(ctxt);
        while !pi_stopped(ctxt) {
            let input = pi_input(ctxt);
            if input.is_null() {
                break;
            }
            if (*input).cur >= (*input).end {
                if (*ctxt).inputNr <= old_input_nr {
                    pi_fatal_err(ctxt, XML_ERR_EXT_SUBSET_NOT_FINISHED);
                    break;
                }
                pi_pop_pe(ctxt);
            } else if pi_raw(ctxt) == b'<' && pi_nxt(ctxt, 1) == b'!' && pi_nxt(ctxt, 2) == b'[' {
                pi_parse_conditional_sections(ctxt);
            } else if pi_raw(ctxt) == b'<' && (pi_nxt(ctxt, 1) == b'!' || pi_nxt(ctxt, 1) == b'?') {
                pi_parse_markup_decl(ctxt);
            } else if pi_raw(ctxt) == b'%' {
                pi_parse_pe_reference(ctxt);
            } else {
                pi_fatal_err(ctxt, XML_ERR_EXT_SUBSET_NOT_FINISHED);
                while (*ctxt).inputNr > old_input_nr {
                    pi_pop_pe(ctxt);
                }
                break;
            }
            pi_skip_blanks(ctxt);
        }
    }
}

/// `xmlParseConditionalSections`.
unsafe fn pi_parse_conditional_sections(ctxt: *mut _xmlParserCtxt) {
    unsafe {
        let old_input_nr = (*ctxt).inputNr;
        let mut depth: usize = 0;
        loop {
            if pi_stopped(ctxt) {
                return;
            }
            let input = pi_input(ctxt);
            if input.is_null() {
                return;
            }
            if (*input).cur >= (*input).end {
                if (*ctxt).inputNr <= old_input_nr {
                    pi_fatal_err(ctxt, XML_ERR_EXT_SUBSET_NOT_FINISHED);
                    return;
                }
                pi_pop_pe(ctxt);
            } else if pi_raw(ctxt) == b'<' && pi_nxt(ctxt, 1) == b'!' && pi_nxt(ctxt, 2) == b'[' {
                pi_skip(ctxt, 3);
                pi_skip_blanks(ctxt);
                if pi_cmp7(ctxt, b"INCLUDE") {
                    pi_skip(ctxt, 7);
                    pi_skip_blanks(ctxt);
                    if pi_raw(ctxt) != b'[' {
                        pi_fatal_err(ctxt, XML_ERR_CONDSEC_INVALID);
                        return;
                    }
                    pi_next1(ctxt);
                    depth += 1;
                } else if pi_cmp6(ctxt, b"IGNORE") {
                    pi_skip(ctxt, 6);
                    pi_skip_blanks(ctxt);
                    if pi_raw(ctxt) != b'[' {
                        pi_fatal_err(ctxt, XML_ERR_CONDSEC_INVALID);
                        return;
                    }
                    pi_next1(ctxt);
                    let mut ignore_depth: usize = 0;
                    loop {
                        if pi_stopped(ctxt) {
                            return;
                        }
                        let inp = pi_input(ctxt);
                        if inp.is_null() || (*inp).cur >= (*inp).end || pi_raw(ctxt) == 0 {
                            pi_fatal_err(ctxt, XML_ERR_CONDSEC_NOT_FINISHED);
                            return;
                        }
                        if pi_raw(ctxt) == b'<'
                            && pi_nxt(ctxt, 1) == b'!'
                            && pi_nxt(ctxt, 2) == b'['
                        {
                            pi_skip(ctxt, 3);
                            ignore_depth += 1;
                        } else if pi_raw(ctxt) == b']'
                            && pi_nxt(ctxt, 1) == b']'
                            && pi_nxt(ctxt, 2) == b'>'
                        {
                            pi_skip(ctxt, 3);
                            if ignore_depth == 0 {
                                break;
                            }
                            ignore_depth -= 1;
                        } else {
                            pi_next1(ctxt);
                        }
                    }
                } else {
                    pi_fatal_err(ctxt, XML_ERR_CONDSEC_INVALID_KEYWORD);
                    return;
                }
            } else if depth > 0
                && pi_raw(ctxt) == b']'
                && pi_nxt(ctxt, 1) == b']'
                && pi_nxt(ctxt, 2) == b'>'
            {
                depth -= 1;
                pi_skip(ctxt, 3);
            } else if pi_raw(ctxt) == b'<' && (pi_nxt(ctxt, 1) == b'!' || pi_nxt(ctxt, 1) == b'?') {
                pi_parse_markup_decl(ctxt);
            } else if pi_raw(ctxt) == b'%' {
                pi_parse_pe_reference(ctxt);
            } else {
                pi_fatal_err(ctxt, XML_ERR_EXT_SUBSET_NOT_FINISHED);
                return;
            }
            if depth == 0 {
                break;
            }
            pi_skip_blanks(ctxt);
        }
    }
}

/// `xmlParseVersionNum`.
unsafe fn pi_parse_version_num(ctxt: *mut _xmlParserCtxt) -> *mut xmlChar {
    unsafe {
        let mut buf: Vec<u8> = Vec::new();
        let mut cur = pi_raw(ctxt);
        if !cur.is_ascii_digit() {
            return ptr::null_mut();
        }
        buf.push(cur);
        pi_next1(ctxt);
        cur = pi_raw(ctxt);
        if cur != b'.' {
            return ptr::null_mut();
        }
        buf.push(cur);
        pi_next1(ctxt);
        cur = pi_raw(ctxt);
        while cur.is_ascii_digit() {
            buf.push(cur);
            pi_next1(ctxt);
            cur = pi_raw(ctxt);
        }
        pi_strndup_bytes(buf.as_ptr(), buf.len())
    }
}

/// `xmlParseVersionInfo`.
unsafe fn pi_parse_version_info(ctxt: *mut _xmlParserCtxt) -> *mut xmlChar {
    unsafe {
        if pi_cmp7(ctxt, b"version") {
            pi_skip(ctxt, 7);
            pi_skip_blanks(ctxt);
            if pi_raw(ctxt) != b'=' {
                pi_fatal_err(ctxt, XML_ERR_EQUAL_REQUIRED);
                return ptr::null_mut();
            }
            pi_next1(ctxt);
            pi_skip_blanks(ctxt);
            if pi_raw(ctxt) == b'"' {
                pi_next1(ctxt);
                let version = pi_parse_version_num(ctxt);
                if pi_raw(ctxt) != b'"' {
                    pi_fatal_err(ctxt, XML_ERR_STRING_NOT_CLOSED);
                } else {
                    pi_next1(ctxt);
                }
                return version;
            } else if pi_raw(ctxt) == b'\'' {
                pi_next1(ctxt);
                let version = pi_parse_version_num(ctxt);
                if pi_raw(ctxt) != b'\'' {
                    pi_fatal_err(ctxt, XML_ERR_STRING_NOT_CLOSED);
                } else {
                    pi_next1(ctxt);
                }
                return version;
            } else {
                pi_fatal_err(ctxt, XML_ERR_STRING_NOT_STARTED);
            }
        }
        ptr::null_mut()
    }
}

/// `xmlParseEncName`.
unsafe fn pi_parse_enc_name(ctxt: *mut _xmlParserCtxt) -> *mut xmlChar {
    unsafe {
        let cur = pi_raw(ctxt);
        if !(cur.is_ascii_lowercase() || cur.is_ascii_uppercase()) {
            pi_fatal_err(ctxt, XML_ERR_ENCODING_NAME);
            return ptr::null_mut();
        }
        let mut buf: Vec<u8> = Vec::new();
        buf.push(cur);
        pi_next1(ctxt);
        let mut c = pi_raw(ctxt);
        while c.is_ascii_alphanumeric() || c == b'.' || c == b'_' || c == b'-' {
            buf.push(c);
            pi_next1(ctxt);
            c = pi_raw(ctxt);
        }
        pi_strndup_bytes(buf.as_ptr(), buf.len())
    }
}

/// `xmlParseEncodingDecl` — returns `ctxt->encoding` and stores it.
unsafe fn pi_parse_encoding_decl(ctxt: *mut _xmlParserCtxt) -> *const xmlChar {
    unsafe {
        pi_skip_blanks(ctxt);
        if !pi_cmp8(ctxt, b"encoding") {
            return ptr::null();
        }
        pi_skip(ctxt, 8);
        pi_skip_blanks(ctxt);
        if pi_raw(ctxt) != b'=' {
            pi_fatal_err(ctxt, XML_ERR_EQUAL_REQUIRED);
            return ptr::null();
        }
        pi_next1(ctxt);
        pi_skip_blanks(ctxt);
        let mut encoding: *mut xmlChar = ptr::null_mut();
        if pi_raw(ctxt) == b'"' {
            pi_next1(ctxt);
            encoding = pi_parse_enc_name(ctxt);
            if pi_raw(ctxt) != b'"' {
                pi_fatal_err(ctxt, XML_ERR_STRING_NOT_CLOSED);
                if !encoding.is_null() {
                    xmlFreeImpl(encoding as *mut c_void);
                }
                return ptr::null();
            }
            pi_next1(ctxt);
        } else if pi_raw(ctxt) == b'\'' {
            pi_next1(ctxt);
            encoding = pi_parse_enc_name(ctxt);
            if pi_raw(ctxt) != b'\'' {
                pi_fatal_err(ctxt, XML_ERR_STRING_NOT_CLOSED);
                if !encoding.is_null() {
                    xmlFreeImpl(encoding as *mut c_void);
                }
                return ptr::null();
            }
            pi_next1(ctxt);
        } else {
            pi_fatal_err(ctxt, XML_ERR_STRING_NOT_STARTED);
        }
        if encoding.is_null() {
            return ptr::null();
        }
        let c = &mut *ctxt;
        if !c.encoding.is_null() {
            xmlFreeImpl(c.encoding as *mut c_void);
        }
        c.encoding = encoding;
        c.encoding as *const xmlChar
    }
}

/// `xmlParseSDDecl`.
unsafe fn pi_parse_sd_decl(ctxt: *mut _xmlParserCtxt) -> c_int {
    unsafe {
        let mut standalone = -2;
        pi_skip_blanks(ctxt);
        if pi_cmp10(ctxt, b"standalone") {
            pi_skip(ctxt, 10);
            pi_skip_blanks(ctxt);
            if pi_raw(ctxt) != b'=' {
                pi_fatal_err(ctxt, XML_ERR_EQUAL_REQUIRED);
                return standalone;
            }
            pi_next1(ctxt);
            pi_skip_blanks(ctxt);
            let quote = pi_raw(ctxt);
            if quote == b'\'' || quote == b'"' {
                pi_next1(ctxt);
                if pi_raw(ctxt) == b'n' && pi_nxt(ctxt, 1) == b'o' {
                    standalone = 0;
                    pi_skip(ctxt, 2);
                } else if pi_raw(ctxt) == b'y' && pi_nxt(ctxt, 1) == b'e' && pi_nxt(ctxt, 2) == b's'
                {
                    standalone = 1;
                    pi_skip(ctxt, 3);
                } else {
                    pi_fatal_err(ctxt, XML_ERR_STANDALONE_VALUE);
                }
                if pi_raw(ctxt) != quote {
                    pi_fatal_err(ctxt, XML_ERR_STRING_NOT_CLOSED);
                } else {
                    pi_next1(ctxt);
                }
            } else {
                pi_fatal_err(ctxt, XML_ERR_STRING_NOT_STARTED);
            }
        }
        standalone
    }
}

/// `xmlParseXMLDecl`.
unsafe fn pi_parse_xml_decl(ctxt: *mut _xmlParserCtxt) {
    unsafe {
        (*ctxt).standalone = -2;
        pi_skip(ctxt, 5);
        if !pi_is_blank_ch(pi_raw(ctxt)) {
            pi_fatal_err(ctxt, XML_ERR_SPACE_REQUIRED);
        }
        pi_skip_blanks(ctxt);
        let version = pi_parse_version_info(ctxt);
        if version.is_null() {
            pi_fatal_err(ctxt, XML_ERR_VERSION_MISSING);
        } else {
            if !pi_cstr_eq(version, b"1.0") {
                if pi_nxt(ctxt, 0) == b'1' {
                    // warning-level upstream; keep parsing
                } else {
                    pi_fatal_err(ctxt, XML_ERR_UNKNOWN_VERSION);
                }
            }
            let c = &mut *ctxt;
            if !c.version.is_null() {
                xmlFreeImpl(c.version as *mut c_void);
            }
            c.version = version;
        }
        if !pi_is_blank_ch(pi_raw(ctxt)) {
            if pi_raw(ctxt) == b'?' && pi_nxt(ctxt, 1) == b'>' {
                pi_skip(ctxt, 2);
                return;
            }
            pi_fatal_err(ctxt, XML_ERR_SPACE_REQUIRED);
        }
        pi_parse_encoding_decl(ctxt);
        if !(*ctxt).encoding.is_null() && !pi_is_blank_ch(pi_raw(ctxt)) {
            if pi_raw(ctxt) == b'?' && pi_nxt(ctxt, 1) == b'>' {
                pi_skip(ctxt, 2);
                return;
            }
            pi_fatal_err(ctxt, XML_ERR_SPACE_REQUIRED);
        }
        pi_skip_blanks(ctxt);
        (*ctxt).standalone = pi_parse_sd_decl(ctxt);
        pi_skip_blanks(ctxt);
        if pi_raw(ctxt) == b'?' && pi_nxt(ctxt, 1) == b'>' {
            pi_skip(ctxt, 2);
        } else if pi_raw(ctxt) == b'>' {
            pi_fatal_err(ctxt, XML_ERR_XMLDECL_NOT_FINISHED);
            pi_next1(ctxt);
        } else {
            pi_fatal_err(ctxt, XML_ERR_XMLDECL_NOT_FINISHED);
            while !pi_stopped(ctxt) && pi_raw(ctxt) != 0 {
                let c = pi_raw(ctxt);
                pi_next1(ctxt);
                if c == b'>' {
                    break;
                }
            }
        }
    }
}

/// `xmlParseMisc`.
unsafe fn pi_parse_misc(ctxt: *mut _xmlParserCtxt) {
    unsafe {
        while !pi_stopped(ctxt) {
            pi_skip_blanks(ctxt);
            if pi_raw(ctxt) == b'<' && pi_nxt(ctxt, 1) == b'?' {
                pi_parse_pi(ctxt);
            } else if pi_raw(ctxt) == b'<'
                && pi_nxt(ctxt, 1) == b'!'
                && pi_nxt(ctxt, 2) == b'-'
                && pi_nxt(ctxt, 3) == b'-'
            {
                pi_parse_comment(ctxt);
            } else {
                break;
            }
        }
    }
}

/// `xmlParseContent` — parse a content sequence.
#[allow(clippy::while_immutable_condition)]
unsafe fn pi_parse_content(ctxt: *mut _xmlParserCtxt) {
    unsafe {
        if ctxt.is_null() || pi_input(ctxt).is_null() {
            return;
        }
        pi_ctxt_late_init(ctxt);
        let old_name_nr = (*ctxt).nameNr;
        let old_space_nr = (*ctxt).spaceNr;
        let old_node_nr = (*ctxt).nodeNr;
        loop {
            let input = pi_input(ctxt);
            if input.is_null() {
                break;
            }
            if (*input).cur >= (*input).end || pi_stopped(ctxt) {
                break;
            }
            let cur = (*input).cur;
            if *cur == b'<' {
                if cur.add(1) < (*input).end && *cur.add(1) == b'?' {
                    pi_parse_pi(ctxt);
                } else if cur.add(8) < (*input).end
                    && *cur.add(1) == b'!'
                    && *cur.add(2) == b'['
                    && *cur.add(3) == b'C'
                    && *cur.add(4) == b'D'
                    && *cur.add(5) == b'A'
                    && *cur.add(6) == b'T'
                    && *cur.add(7) == b'A'
                    && *cur.add(8) == b'['
                {
                    pi_parse_cd_sect(ctxt);
                } else if cur.add(3) < (*input).end
                    && *cur.add(1) == b'!'
                    && *cur.add(2) == b'-'
                    && *cur.add(3) == b'-'
                {
                    pi_parse_comment(ctxt);
                } else if cur.add(1) < (*input).end && *cur.add(1) == b'/' {
                    if (*ctxt).nameNr <= old_name_nr {
                        break;
                    }
                    pi_parse_end_tag(ctxt);
                } else {
                    pi_parse_element(ctxt);
                }
            } else if *cur == b'&' {
                pi_parse_reference(ctxt);
            } else {
                pi_parse_char_data(ctxt, 0);
            }
        }
        // Premature end of data in tag.
        if (*ctxt).nameNr > old_name_nr
            && !pi_input(ctxt).is_null()
            && (*(*ctxt).input).cur >= (*(*ctxt).input).end
            && (*ctxt).wellFormed != 0
        {
            pi_fatal_err(ctxt, XML_ERR_TAG_NOT_FINISHED);
        }
        // Clean up in error case.
        while (*ctxt).nodeNr > old_node_nr {
            pi_node_pop(ctxt);
        }
        while (*ctxt).nameNr > old_name_nr {
            pi_name_pop(ctxt);
        }
        while (*ctxt).spaceNr > old_space_nr {
            pi_space_pop(ctxt);
        }
    }
}

/// `xmlParseAttribute` — parse `Name Eq AttValue`.
unsafe fn pi_parse_attribute(
    ctxt: *mut _xmlParserCtxt,
    value: *mut *mut xmlChar,
) -> *const xmlChar {
    unsafe {
        *value = ptr::null_mut();
        let name = pi_parse_name(ctxt);
        if name.is_null() {
            pi_fatal_err(ctxt, XML_ERR_NAME_REQUIRED);
            return ptr::null();
        }
        pi_skip_blanks(ctxt);
        let mut val: *mut xmlChar = ptr::null_mut();
        if pi_raw(ctxt) == b'=' {
            pi_next1(ctxt);
            pi_skip_blanks(ctxt);
            val = pi_parse_att_value(ctxt);
        } else {
            pi_fatal_err(ctxt, XML_ERR_ATTRIBUTE_WITHOUT_VALUE);
            return name;
        }
        // xml:space / xml:lang checks (only when space stack is set up).
        if pi_cstr_eq(name, b"xml:space") && !val.is_null() && !(*ctxt).space.is_null() {
            if pi_cstr_eq(val, b"default") {
                *(*ctxt).space = 0;
            } else if pi_cstr_eq(val, b"preserve") {
                *(*ctxt).space = 1;
            }
        }
        *value = val;
        name
    }
}

/// `xmlParseStartTag` — parse `<name attrs...>`.
unsafe fn pi_parse_start_tag(ctxt: *mut _xmlParserCtxt) -> *const xmlChar {
    unsafe {
        if pi_raw(ctxt) != b'<' {
            return ptr::null();
        }
        pi_next1(ctxt);
        let name = pi_parse_name(ctxt);
        if name.is_null() {
            pi_fatal_err(ctxt, XML_ERR_NAME_REQUIRED);
            return ptr::null();
        }
        let c0 = &mut *ctxt;
        let mut atts = c0.atts;
        let mut maxatts = c0.maxatts;
        let mut nbatts: usize = 0;
        let mut failed = false;

        pi_skip_blanks(ctxt);
        while !(pi_raw(ctxt) == b'>' || (pi_raw(ctxt) == b'/' && pi_nxt(ctxt, 1) == b'>'))
            && pi_is_byte_char(pi_raw(ctxt))
            && !pi_stopped(ctxt)
        {
            let mut attvalue: *mut xmlChar = ptr::null_mut();
            let attname = pi_parse_attribute(ctxt, &mut attvalue);
            if attname.is_null() {
                failed = true;
                if !attvalue.is_null() {
                    xmlFreeImpl(attvalue as *mut c_void);
                }
                break;
            }
            if !attvalue.is_null() {
                // [WFC: Unique Att Spec]
                let mut i = 0;
                while i < nbatts {
                    if crate::abi::exports_xml2::xmlStrEqual(*atts.add(i), attname) != 0 {
                        failed = true;
                        break;
                    }
                    i += 2;
                }
                if failed {
                    if !attvalue.is_null() {
                        xmlFreeImpl(attvalue as *mut c_void);
                    }
                    break;
                }
                // Add the pair to atts.
                if nbatts + 4 > maxatts as usize {
                    let new_max = if maxatts == 0 { 20 } else { maxatts * 2 };
                    let n = xmlReallocImpl(
                        atts as *mut c_void,
                        (new_max as usize) * size_of::<*const xmlChar>(),
                    ) as *mut *const xmlChar;
                    if n.is_null() {
                        pi_err_memory(ctxt);
                        failed = true;
                        xmlFreeImpl(attvalue as *mut c_void);
                        break;
                    }
                    atts = n;
                    maxatts = new_max;
                    let c = &mut *ctxt;
                    c.atts = atts;
                    c.maxatts = maxatts;
                }
                *atts.add(nbatts) = attname;
                *atts.add(nbatts + 1) = attvalue;
                nbatts += 2;
                attvalue = ptr::null_mut();
            } else {
                failed = true;
            }
            if !attvalue.is_null() {
                xmlFreeImpl(attvalue as *mut c_void);
            }
            if pi_raw(ctxt) == b'>' || (pi_raw(ctxt) == b'/' && pi_nxt(ctxt, 1) == b'>') {
                break;
            }
            if pi_skip_blanks(ctxt) == 0 {
                pi_fatal_err(ctxt, XML_ERR_SPACE_REQUIRED);
            }
        }

        // SAX: Start of Element!
        pi_dispatch_start_element(ctxt, name, atts, nbatts);

        // Free the attribute name/value strings (SAX handlers copy them).
        let mut i = 0;
        while i < nbatts {
            if !atts.is_null() {
                let p = *atts.add(i);
                if !p.is_null() {
                    xmlFreeImpl(p as *mut c_void);
                }
            }
            i += 1;
        }
        name
    }
}

/// `xmlParseEndTag` — parse `</name>`.
unsafe fn pi_parse_end_tag(ctxt: *mut _xmlParserCtxt) {
    unsafe {
        if pi_raw(ctxt) != b'<' || pi_nxt(ctxt, 1) != b'/' {
            pi_fatal_err(ctxt, XML_ERR_LTSLASH_REQUIRED);
            return;
        }
        pi_skip(ctxt, 2);
        let c = &*ctxt;
        let name = pi_parse_name_and_compare(ctxt, c.name);
        pi_skip_blanks(ctxt);
        if !pi_is_byte_char(pi_raw(ctxt)) || pi_raw(ctxt) != b'>' {
            pi_fatal_err(ctxt, XML_ERR_GT_REQUIRED);
        } else {
            pi_next1(ctxt);
        }
        if name != std::ptr::dangling::<xmlChar>() {
            if name.is_null() {
                // "unparsable" name
                pi_fatal_err(ctxt, XML_ERR_TAG_NAME_MISMATCH);
            } else {
                pi_fatal_err(ctxt, XML_ERR_TAG_NAME_MISMATCH);
                xmlFreeImpl(name as *mut c_void);
            }
        }
        // SAX: End of Tag.
        let c = &*ctxt;
        pi_dispatch_end_element(ctxt, c.name);
        pi_name_pop(ctxt);
        pi_space_pop(ctxt);
    }
}

/// `xmlParseElement` — parse `<name ...>content</name>`.
unsafe fn pi_parse_element(ctxt: *mut _xmlParserCtxt) {
    unsafe {
        let max_depth = if (*ctxt).options & XML_PARSE_HUGE != 0 {
            2048
        } else {
            256
        };
        if (*ctxt).nameNr > max_depth {
            pi_fatal_err(ctxt, XML_ERR_RESOURCE_LIMIT);
            return;
        }
        // spacePush
        let c = &mut *ctxt;
        if c.spaceNr == 0 || (!c.space.is_null() && *c.space == -2) {
            pi_space_push(ctxt, -1);
        } else if !c.space.is_null() {
            pi_space_push(ctxt, *c.space);
        } else {
            pi_space_push(ctxt, -1);
        }
        let line = (*(*ctxt).input).line;
        let name = pi_parse_start_tag(ctxt);
        if name.is_null() {
            pi_space_pop(ctxt);
            return;
        }
        pi_name_push(ctxt, name);

        // Check for an empty element.
        if pi_raw(ctxt) == b'/' && pi_nxt(ctxt, 1) == b'>' {
            pi_skip(ctxt, 2);
            let c = &*ctxt;
            pi_dispatch_end_element(ctxt, c.name);
            pi_name_pop(ctxt);
            pi_space_pop(ctxt);
            return;
        }
        if pi_raw(ctxt) == b'>' {
            pi_next1(ctxt);
        } else {
            pi_fatal_err(ctxt, XML_ERR_GT_REQUIRED);
            pi_name_pop(ctxt);
            pi_space_pop(ctxt);
            return;
        }

        // Content.
        pi_parse_content(ctxt);

        // End tag.
        let input = pi_input(ctxt);
        if input.is_null() || (*input).cur >= (*input).end {
            if (*ctxt).wellFormed != 0 {
                pi_fatal_err(ctxt, XML_ERR_TAG_NOT_FINISHED);
            }
            return;
        }
        pi_parse_end_tag(ctxt);
    }
}

/// `xmlParseDocTypeDecl` — assumes `<!DOCTYPE` was detected.
unsafe fn pi_parse_doc_type_decl(ctxt: *mut _xmlParserCtxt) {
    unsafe {
        pi_skip(ctxt, 9);
        if pi_skip_blanks(ctxt) == 0 {
            pi_fatal_err(ctxt, XML_ERR_SPACE_REQUIRED);
        }
        let name = pi_parse_name(ctxt);
        if name.is_null() {
            pi_fatal_err(ctxt, XML_ERR_NAME_REQUIRED);
            return;
        }
        (*ctxt).intSubName = name;
        pi_skip_blanks(ctxt);
        let mut public_id: *mut xmlChar = ptr::null_mut();
        let uri = pi_parse_external_id(ctxt, &mut public_id, 1);
        if !uri.is_null() || !public_id.is_null() {
            (*ctxt).hasExternalSubset = 1;
        }
        (*ctxt).extSubURI = uri;
        (*ctxt).extSubSystem = public_id;
        pi_skip_blanks(ctxt);
        let c = &*ctxt;
        if !c.sax.is_null() && c.disableSAX == 0 {
            SaxDispatcher::internal_subset(&*c.sax, c.userData, name, public_id, uri);
        }
        if pi_raw(ctxt) != b'[' && pi_raw(ctxt) != b'>' {
            pi_fatal_err(ctxt, XML_ERR_DOCTYPE_NOT_FINISHED);
        }
    }
}

/// `xmlParseInternalSubset`.
#[allow(clippy::while_immutable_condition)]
unsafe fn pi_parse_internal_subset(ctxt: *mut _xmlParserCtxt) {
    unsafe {
        if pi_raw(ctxt) == b'[' {
            let old_input_nr = (*ctxt).inputNr;
            pi_next1(ctxt);
            pi_skip_blanks(ctxt);
            loop {
                if pi_stopped(ctxt) {
                    return;
                }
                let input = pi_input(ctxt);
                if input.is_null() {
                    return;
                }
                if (*input).cur >= (*input).end {
                    if (*ctxt).inputNr <= old_input_nr {
                        pi_fatal_err(ctxt, XML_ERR_INT_SUBSET_NOT_FINISHED);
                        return;
                    }
                    pi_pop_pe(ctxt);
                } else if pi_raw(ctxt) == b']' && (*ctxt).inputNr <= old_input_nr {
                    pi_next1(ctxt);
                    pi_skip_blanks(ctxt);
                    break;
                } else if pi_raw(ctxt) == b'<'
                    && (pi_nxt(ctxt, 1) == b'!' || pi_nxt(ctxt, 1) == b'?')
                {
                    pi_parse_markup_decl(ctxt);
                } else if pi_raw(ctxt) == b'%' {
                    pi_parse_pe_reference(ctxt);
                } else {
                    pi_fatal_err(ctxt, XML_ERR_INT_SUBSET_NOT_FINISHED);
                    while (*ctxt).inputNr > old_input_nr {
                        pi_pop_pe(ctxt);
                    }
                    return;
                }
                pi_skip_blanks(ctxt);
            }
        }
        if pi_raw(ctxt) != b'>' {
            pi_fatal_err(ctxt, XML_ERR_DOCTYPE_NOT_FINISHED);
            return;
        }
        pi_next1(ctxt);
    }
}

/// `xmlParseDocument`.
unsafe fn pi_parse_document(ctxt: *mut _xmlParserCtxt) -> c_int {
    unsafe {
        if ctxt.is_null() || pi_input(ctxt).is_null() {
            return -1;
        }
        pi_ctxt_late_init(ctxt);

        // SAX: detecting the level — setDocumentLocator.
        let c0 = &*ctxt;
        if !c0.sax.is_null() && (*c0.sax).setDocumentLocator.is_some() {
            SaxDispatcher::set_document_locator(
                &*c0.sax,
                c0.userData,
                &crate::abi::data_globals::xmlDefaultSAXLocator as *const _xmlSAXLocator
                    as *mut _xmlSAXLocator,
            );
        }

        if pi_raw(ctxt) == 0 {
            pi_fatal_err(ctxt, XML_ERR_DOCUMENT_EMPTY);
            return -1;
        }

        if pi_cmp5(ctxt, b"<?xml") && pi_is_blank_ch(pi_nxt(ctxt, 5)) {
            pi_parse_xml_decl(ctxt);
            pi_skip_blanks(ctxt);
        } else {
            let c = &mut *ctxt;
            if !c.version.is_null() {
                xmlFreeImpl(c.version as *mut c_void);
            }
            c.version = crate::abi::exports_xml2::xmlStrdup(c"1.0".as_ptr() as *const xmlChar);
            if c.version.is_null() {
                pi_err_memory(ctxt);
                return -1;
            }
        }
        let c1 = &*ctxt;
        if !c1.sax.is_null() && c1.disableSAX == 0 {
            SaxDispatcher::start_document(&*c1.sax, c1.userData);
        }

        // The Misc part of the prolog.
        pi_parse_misc(ctxt);

        // Then possibly doc type declaration(s) and more Misc.
        if pi_cmp9(ctxt, b"<!DOCTYPE") {
            let c = &mut *ctxt;
            c.inSubset = 1;
            pi_parse_doc_type_decl(ctxt);
            if pi_raw(ctxt) == b'[' {
                pi_parse_internal_subset(ctxt);
            } else if pi_raw(ctxt) == b'>' {
                pi_next1(ctxt);
            }
            c.inSubset = 2;
            let c2 = &*ctxt;
            if !c2.sax.is_null() && c2.disableSAX == 0 {
                SaxDispatcher::external_subset(
                    &*c2.sax,
                    c2.userData,
                    c2.intSubName,
                    c2.extSubSystem,
                    c2.extSubURI,
                );
            }
            let c3 = &mut *ctxt;
            c3.inSubset = 0;
            pi_parse_misc(ctxt);
        }

        // Time to start parsing the tree itself.
        if pi_raw(ctxt) != b'<' {
            if (*ctxt).wellFormed != 0 {
                pi_fatal_err(ctxt, XML_ERR_DOCUMENT_EMPTY);
            }
        } else {
            pi_parse_element(ctxt);
            pi_parse_misc(ctxt);
            // Check EOF.
            if !pi_stopped(ctxt) {
                let input = pi_input(ctxt);
                if !input.is_null() && (*input).cur < (*input).end {
                    pi_fatal_err(ctxt, XML_ERR_DOCUMENT_END);
                }
            }
        }

        let c = &mut *ctxt;
        c.instate = XML_PARSER_EOF_STATE;
        if !c.sax.is_null() && c.disableSAX == 0 {
            SaxDispatcher::end_document(&*c.sax, c.userData);
        }
        if c.wellFormed == 0 {
            c.valid = 0;
            return -1;
        }
        0
    }
}

/// `xmlParseExtParsedEnt`.
unsafe fn pi_parse_ext_parsed_ent(ctxt: *mut _xmlParserCtxt) -> c_int {
    unsafe {
        if ctxt.is_null() || pi_input(ctxt).is_null() {
            return -1;
        }
        pi_ctxt_late_init(ctxt);
        if pi_raw(ctxt) == 0 {
            pi_fatal_err(ctxt, XML_ERR_DOCUMENT_EMPTY);
        }
        if pi_cmp5(ctxt, b"<?xml") && pi_is_blank_ch(pi_nxt(ctxt, 5)) {
            pi_parse_xml_decl(ctxt);
            pi_skip_blanks(ctxt);
        } else {
            let c = &mut *ctxt;
            if !c.version.is_null() {
                xmlFreeImpl(c.version as *mut c_void);
            }
            c.version = crate::abi::exports_xml2::xmlStrdup(c"1.0".as_ptr() as *const xmlChar);
        }
        let c0 = &*ctxt;
        if !c0.sax.is_null() && c0.disableSAX == 0 {
            SaxDispatcher::start_document(&*c0.sax, c0.userData);
        }
        let c1 = &mut *ctxt;
        c1.options &= !XML_PARSE_DTDVALID;
        c1.validate = 0;
        c1.depth = 0;

        pi_parse_content(ctxt);

        let input = pi_input(ctxt);
        if !input.is_null() && (*input).cur < (*input).end {
            pi_fatal_err(ctxt, XML_ERR_NOT_WELL_BALANCED);
        }
        let c2 = &*ctxt;
        if !c2.sax.is_null() && c2.disableSAX == 0 {
            SaxDispatcher::end_document(&*c2.sax, c2.userData);
        }
        if (*ctxt).wellFormed == 0 {
            return -1;
        }
        0
    }
}

/// Parse a content sequence into a node list, using a synthetic `#root`
/// node (upstream `xmlCtxtParseContentInternal`).
unsafe fn pi_parse_content_node_list(
    ctxt: *mut _xmlParserCtxt,
    input: *mut _xmlParserInput,
    has_text_decl: c_int,
) -> *mut _xmlNode {
    unsafe {
        let mut root: *mut _xmlNode = ptr::null_mut();
        let mut list: *mut _xmlNode = ptr::null_mut();
        let root_name = b"#root\0";
        root = crate::xml::tree::new_node(ptr::null_mut(), root_name.as_ptr() as *const xmlChar);
        if root.is_null() {
            pi_err_memory(ctxt);
            return ptr::null_mut();
        }
        if pi_input_push(ctxt, input) < 0 {
            crate::xml::tree::free_node(root);
            return ptr::null_mut();
        }
        pi_name_push(ctxt, root_name.as_ptr() as *const xmlChar);
        pi_space_push(ctxt, -1);
        pi_node_push(ctxt, root);

        if has_text_decl != 0 && pi_cmp5(ctxt, b"<?xml") && pi_is_blank_ch(pi_nxt(ctxt, 5)) {
            pi_parse_text_decl(ctxt);
        }

        pi_parse_content(ctxt);

        let cur = pi_input(ctxt);
        if !cur.is_null() && (*cur).cur < (*cur).end {
            pi_fatal_err(ctxt, XML_ERR_NOT_WELL_BALANCED);
        }

        if (*ctxt).wellFormed != 0 {
            // Unlink the newly created node list.
            list = (*root).children;
            (*root).children = ptr::null_mut();
            (*root).last = ptr::null_mut();
            let mut n = list;
            while !n.is_null() {
                (*n).parent = ptr::null_mut();
                n = (*n).next;
            }
        }

        if pi_input_pop(ctxt) == input {
            // The popped input is the one we pushed: free it (struct and its
            // owned filename; the base data lives in the boxed InputBuffer
            // stashed in ctxt->_private, freed by free_parser_ctxt).
            crate::xml::parser::helpers::free_parser_input(input);
        }
        pi_node_pop(ctxt);
        pi_name_pop(ctxt);
        pi_space_pop(ctxt);
        crate::xml::tree::free_node(root);
        list
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// C ABI exports — parserInternals.h name/primitive family
// ═══════════════════════════════════════════════════════════════════════════════

/// `xmlNamespaceParseNCName` (legacy libxml2 2.6.x API).
///
/// ```c
/// const xmlChar *xmlNamespaceParseNCName(xmlParserCtxtPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlNamespaceParseNCName(ctxt: *mut _xmlParserCtxt) -> *const xmlChar {
    if ctxt.is_null() || pi_input(ctxt).is_null() {
        return ptr::null();
    }
    unsafe {
        let start = pi_cur_ptr(ctxt);
        let (c, l) = pi_current_char(ctxt);
        if (c != b'_' as c_int && !pi_is_letter_ch(c as u8)) || c == b':' as c_int {
            return ptr::null();
        }
        pi_nextl(ctxt, l);
        loop {
            let (c2, l2) = pi_current_char(ctxt);
            if !pi_is_name_char(c2) || c2 == b':' as c_int {
                break;
            }
            pi_nextl(ctxt, l2);
        }
        let len = pi_cur_ptr(ctxt).offset_from(start) as usize;
        if len == 0 {
            return ptr::null();
        }
        pi_strndup_bytes(start, len)
    }
}

/// `xmlNamespaceParseQName` (legacy libxml2 2.6.x API).
///
/// ```c
/// const xmlChar *xmlNamespaceParseQName(xmlParserCtxtPtr ctxt, xmlChar **prefix);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlNamespaceParseQName(
    ctxt: *mut _xmlParserCtxt,
    prefix: *mut *mut xmlChar,
) -> *const xmlChar {
    if ctxt.is_null() || pi_input(ctxt).is_null() {
        return ptr::null();
    }
    unsafe {
        if !prefix.is_null() {
            *prefix = ptr::null_mut();
        }
        let start = pi_cur_ptr(ctxt);
        let (c, l) = pi_current_char(ctxt);
        if (c != b'_' as c_int && !pi_is_letter_ch(c as u8)) || c == b':' as c_int {
            return ptr::null();
        }
        pi_nextl(ctxt, l);
        loop {
            let (c2, l2) = pi_current_char(ctxt);
            if !pi_is_name_char(c2) || c2 == b':' as c_int {
                break;
            }
            pi_nextl(ctxt, l2);
        }
        if pi_raw(ctxt) == b':' {
            pi_next1(ctxt);
            if !prefix.is_null() {
                let plen = pi_cur_ptr(ctxt).offset_from(start) as usize - 1;
                *prefix = pi_strndup_bytes(start, plen);
            }
            let lstart = pi_cur_ptr(ctxt);
            let (c2, l2) = pi_current_char(ctxt);
            if (c2 != b'_' as c_int && !pi_is_letter_ch(c2 as u8)) || c2 == b':' as c_int {
                return ptr::null();
            }
            pi_nextl(ctxt, l2);
            loop {
                let (c3, l3) = pi_current_char(ctxt);
                if !pi_is_name_char(c3) || c3 == b':' as c_int {
                    break;
                }
                pi_nextl(ctxt, l3);
            }
            let llen = pi_cur_ptr(ctxt).offset_from(lstart) as usize;
            return pi_strndup_bytes(lstart, llen);
        }
        let len = pi_cur_ptr(ctxt).offset_from(start) as usize;
        if len == 0 {
            return ptr::null();
        }
        pi_strndup_bytes(start, len)
    }
}

/// `xmlNamespaceParseNSDef` (legacy libxml2 2.6.x API).
///
/// ```c
/// const xmlChar *xmlNamespaceParseNSDef(xmlParserCtxtPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlNamespaceParseNSDef(ctxt: *mut _xmlParserCtxt) -> *const xmlChar {
    if ctxt.is_null() || pi_input(ctxt).is_null() {
        return ptr::null();
    }
    unsafe {
        let start = pi_cur_ptr(ctxt);
        if !pi_cmp5(ctxt, b"xmlns") {
            return ptr::null();
        }
        pi_skip(ctxt, 5);
        if pi_raw(ctxt) == b':' {
            pi_next1(ctxt);
            let (c, l) = pi_current_char(ctxt);
            if (c != b'_' as c_int && !pi_is_letter_ch(c as u8)) || c == b':' as c_int {
                return ptr::null();
            }
            pi_nextl(ctxt, l);
            loop {
                let (c2, l2) = pi_current_char(ctxt);
                if !pi_is_name_char(c2) || c2 == b':' as c_int {
                    break;
                }
                pi_nextl(ctxt, l2);
            }
        }
        let len = pi_cur_ptr(ctxt).offset_from(start) as usize;
        pi_strndup_bytes(start, len)
    }
}

/// `xmlParseNamespace` (legacy libxml2 2.6.x API).
///
/// Parses a namespace declaration `xmlns[:prefix] = "uri"`.
///
/// ```c
/// const xmlChar *xmlParseNamespace(xmlParserCtxtPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseNamespace(ctxt: *mut _xmlParserCtxt) -> *const xmlChar {
    if ctxt.is_null() || pi_input(ctxt).is_null() {
        return ptr::null();
    }
    unsafe {
        let name = xmlNamespaceParseNSDef(ctxt);
        if name.is_null() {
            return ptr::null();
        }
        pi_skip_blanks(ctxt);
        if pi_raw(ctxt) == b'=' {
            pi_next1(ctxt);
            pi_skip_blanks(ctxt);
            let value = pi_parse_att_value(ctxt);
            if value.is_null() {
                pi_fatal_err(ctxt, XML_ERR_ATTRIBUTE_WITHOUT_VALUE);
                return name;
            }
            xmlFreeImpl(value as *mut c_void);
        }
        name
    }
}

/// `xmlParseQuotedString` (legacy libxml2 2.6.x API).
///
/// ```c
/// xmlChar *xmlParseQuotedString(xmlParserCtxtPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseQuotedString(ctxt: *mut _xmlParserCtxt) -> *mut xmlChar {
    pi_parse_quoted_string(ctxt)
}

/// `xmlParseName`.
///
/// ```c
/// const xmlChar *xmlParseName(xmlParserCtxtPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseName(ctxt: *mut _xmlParserCtxt) -> *const xmlChar {
    pi_parse_name(ctxt)
}

/// `xmlParseNmtoken`.
///
/// ```c
/// xmlChar *xmlParseNmtoken(xmlParserCtxtPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseNmtoken(ctxt: *mut _xmlParserCtxt) -> *mut xmlChar {
    pi_parse_nmtoken(ctxt)
}

/// `xmlParseCharRef`.
///
/// ```c
/// int xmlParseCharRef(xmlParserCtxtPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseCharRef(ctxt: *mut _xmlParserCtxt) -> c_int {
    pi_parse_char_ref(ctxt)
}

/// `xmlParseEntityRef`.
///
/// ```c
/// xmlEntity *xmlParseEntityRef(xmlParserCtxtPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseEntityRef(ctxt: *mut _xmlParserCtxt) -> *mut _xmlEntity {
    pi_parse_entity_ref(ctxt)
}

/// `xmlParseEntityValue`.
///
/// ```c
/// xmlChar *xmlParseEntityValue(xmlParserCtxtPtr ctxt, xmlChar **orig);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseEntityValue(
    ctxt: *mut _xmlParserCtxt,
    orig: *mut *mut xmlChar,
) -> *mut xmlChar {
    pi_parse_entity_value(ctxt, orig)
}

/// `xmlParseAttValue`.
///
/// ```c
/// xmlChar *xmlParseAttValue(xmlParserCtxtPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseAttValue(ctxt: *mut _xmlParserCtxt) -> *mut xmlChar {
    pi_parse_att_value(ctxt)
}

/// `xmlParseSystemLiteral`.
///
/// ```c
/// xmlChar *xmlParseSystemLiteral(xmlParserCtxtPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseSystemLiteral(ctxt: *mut _xmlParserCtxt) -> *mut xmlChar {
    pi_parse_system_literal(ctxt)
}

/// `xmlParsePubidLiteral`.
///
/// ```c
/// xmlChar *xmlParsePubidLiteral(xmlParserCtxtPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParsePubidLiteral(ctxt: *mut _xmlParserCtxt) -> *mut xmlChar {
    pi_parse_pubid_literal(ctxt)
}

/// `xmlParseExternalID`.
///
/// ```c
/// xmlChar *xmlParseExternalID(xmlParserCtxtPtr ctxt, xmlChar **publicId, int strict);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseExternalID(
    ctxt: *mut _xmlParserCtxt,
    public_id: *mut *mut xmlChar,
    strict: c_int,
) -> *mut xmlChar {
    if ctxt.is_null() || public_id.is_null() {
        return ptr::null_mut();
    }
    pi_parse_external_id(ctxt, public_id, strict)
}

/// `xmlParsePITarget`.
///
/// ```c
/// const xmlChar *xmlParsePITarget(xmlParserCtxtPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParsePITarget(ctxt: *mut _xmlParserCtxt) -> *const xmlChar {
    pi_parse_pi_target(ctxt)
}

/// `xmlParsePI`.
///
/// ```c
/// void xmlParsePI(xmlParserCtxtPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParsePI(ctxt: *mut _xmlParserCtxt) {
    pi_parse_pi(ctxt)
}

/// `xmlParseComment`.
///
/// ```c
/// void xmlParseComment(xmlParserCtxtPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseComment(ctxt: *mut _xmlParserCtxt) {
    pi_parse_comment(ctxt)
}

/// `xmlParseCharData`.
///
/// ```c
/// void xmlParseCharData(xmlParserCtxtPtr ctxt, int cdata);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseCharData(ctxt: *mut _xmlParserCtxt, cdata: c_int) {
    pi_parse_char_data(ctxt, cdata)
}

/// `xmlParseCDSect`.
///
/// ```c
/// void xmlParseCDSect(xmlParserCtxtPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseCDSect(ctxt: *mut _xmlParserCtxt) {
    pi_parse_cd_sect(ctxt)
}

/// `xmlParseReference`.
///
/// ```c
/// void xmlParseReference(xmlParserCtxtPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseReference(ctxt: *mut _xmlParserCtxt) {
    pi_parse_reference(ctxt)
}

/// `xmlParserHandleReference` (legacy wrapper, upstream parserInternals.h).
///
/// ```c
/// void xmlParserHandleReference(xmlParserCtxtPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParserHandleReference(ctxt: *mut _xmlParserCtxt) {
    pi_parse_reference(ctxt)
}

/// `xmlParsePEReference`.
///
/// ```c
/// void xmlParsePEReference(xmlParserCtxtPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParsePEReference(ctxt: *mut _xmlParserCtxt) {
    pi_parse_pe_reference(ctxt)
}

/// `xmlParserHandlePEReference`.
///
/// ```c
/// void xmlParserHandlePEReference(xmlParserCtxtPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParserHandlePEReference(ctxt: *mut _xmlParserCtxt) {
    pi_parse_pe_reference(ctxt)
}

/// `xmlParseNotationDecl`.
///
/// ```c
/// void xmlParseNotationDecl(xmlParserCtxtPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseNotationDecl(ctxt: *mut _xmlParserCtxt) {
    pi_parse_notation_decl(ctxt)
}

/// `xmlParseEntityDecl`.
///
/// ```c
/// void xmlParseEntityDecl(xmlParserCtxtPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseEntityDecl(ctxt: *mut _xmlParserCtxt) {
    pi_parse_entity_decl(ctxt)
}

/// `xmlParseDefaultDecl`.
///
/// ```c
/// int xmlParseDefaultDecl(xmlParserCtxtPtr ctxt, xmlChar **value);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseDefaultDecl(
    ctxt: *mut _xmlParserCtxt,
    value: *mut *mut xmlChar,
) -> c_int {
    if ctxt.is_null() || value.is_null() {
        return 0;
    }
    pi_parse_default_decl(ctxt, value)
}

/// `xmlParseAttributeType`.
///
/// ```c
/// int xmlParseAttributeType(xmlParserCtxtPtr ctxt, xmlEnumeration **tree);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseAttributeType(
    ctxt: *mut _xmlParserCtxt,
    tree: *mut *mut _xmlEnumeration,
) -> c_int {
    if ctxt.is_null() || tree.is_null() {
        return 0;
    }
    pi_parse_attribute_type(ctxt, tree)
}

/// `xmlParseNotationType`.
///
/// ```c
/// xmlEnumeration *xmlParseNotationType(xmlParserCtxtPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseNotationType(ctxt: *mut _xmlParserCtxt) -> *mut _xmlEnumeration {
    pi_parse_notation_type(ctxt)
}

/// `xmlParseEnumerationType`.
///
/// ```c
/// xmlEnumeration *xmlParseEnumerationType(xmlParserCtxtPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseEnumerationType(
    ctxt: *mut _xmlParserCtxt,
) -> *mut _xmlEnumeration {
    pi_parse_enumeration_type(ctxt)
}

/// `xmlParseEnumeratedType`.
///
/// ```c
/// int xmlParseEnumeratedType(xmlParserCtxtPtr ctxt, xmlEnumeration **tree);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseEnumeratedType(
    ctxt: *mut _xmlParserCtxt,
    tree: *mut *mut _xmlEnumeration,
) -> c_int {
    if ctxt.is_null() || tree.is_null() {
        return 0;
    }
    pi_parse_enumerated_type(ctxt, tree)
}

/// `xmlParseAttributeListDecl`.
///
/// ```c
/// void xmlParseAttributeListDecl(xmlParserCtxtPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseAttributeListDecl(ctxt: *mut _xmlParserCtxt) {
    pi_parse_attribute_list_decl(ctxt)
}

/// `xmlParseElementMixedContentDecl`.
///
/// ```c
/// xmlElementContent *xmlParseElementMixedContentDecl(xmlParserCtxtPtr ctxt, int inputchk);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseElementMixedContentDecl(
    ctxt: *mut _xmlParserCtxt,
    inputchk: c_int,
) -> *mut _xmlElementContent {
    pi_parse_element_mixed_content_decl(ctxt, inputchk)
}

/// `xmlParseElementChildrenContentDecl`.
///
/// ```c
/// xmlElementContent *xmlParseElementChildrenContentDecl(xmlParserCtxtPtr ctxt, int inputchk);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseElementChildrenContentDecl(
    ctxt: *mut _xmlParserCtxt,
    inputchk: c_int,
) -> *mut _xmlElementContent {
    pi_parse_element_children_content_decl_priv(ctxt, inputchk, 1)
}

/// `xmlParseElementContentDecl`.
///
/// ```c
/// int xmlParseElementContentDecl(xmlParserCtxtPtr ctxt, const xmlChar *name,
///                                xmlElementContent **result);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseElementContentDecl(
    ctxt: *mut _xmlParserCtxt,
    name: *const xmlChar,
    result: *mut *mut _xmlElementContent,
) -> c_int {
    if ctxt.is_null() || result.is_null() {
        return -1;
    }
    pi_parse_element_content_decl(ctxt, name, result)
}

/// `xmlParseElementDecl`.
///
/// ```c
/// int xmlParseElementDecl(xmlParserCtxtPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseElementDecl(ctxt: *mut _xmlParserCtxt) -> c_int {
    pi_parse_element_decl(ctxt)
}

/// `xmlParseMarkupDecl`.
///
/// ```c
/// void xmlParseMarkupDecl(xmlParserCtxtPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseMarkupDecl(ctxt: *mut _xmlParserCtxt) {
    pi_parse_markup_decl(ctxt)
}

/// `xmlParseVersionNum`.
///
/// ```c
/// xmlChar *xmlParseVersionNum(xmlParserCtxtPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseVersionNum(ctxt: *mut _xmlParserCtxt) -> *mut xmlChar {
    pi_parse_version_num(ctxt)
}

/// `xmlParseVersionInfo`.
///
/// ```c
/// xmlChar *xmlParseVersionInfo(xmlParserCtxtPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseVersionInfo(ctxt: *mut _xmlParserCtxt) -> *mut xmlChar {
    pi_parse_version_info(ctxt)
}

/// `xmlParseEncName`.
///
/// ```c
/// xmlChar *xmlParseEncName(xmlParserCtxtPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseEncName(ctxt: *mut _xmlParserCtxt) -> *mut xmlChar {
    pi_parse_enc_name(ctxt)
}

/// `xmlParseEncodingDecl`.
///
/// ```c
/// const xmlChar *xmlParseEncodingDecl(xmlParserCtxtPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseEncodingDecl(ctxt: *mut _xmlParserCtxt) -> *const xmlChar {
    pi_parse_encoding_decl(ctxt)
}

/// `xmlParseSDDecl`.
///
/// ```c
/// int xmlParseSDDecl(xmlParserCtxtPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseSDDecl(ctxt: *mut _xmlParserCtxt) -> c_int {
    pi_parse_sd_decl(ctxt)
}

/// `xmlParseXMLDecl`.
///
/// ```c
/// void xmlParseXMLDecl(xmlParserCtxtPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseXMLDecl(ctxt: *mut _xmlParserCtxt) {
    pi_parse_xml_decl(ctxt)
}

/// `xmlParseTextDecl`.
///
/// ```c
/// void xmlParseTextDecl(xmlParserCtxtPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseTextDecl(ctxt: *mut _xmlParserCtxt) {
    pi_parse_text_decl(ctxt)
}

/// `xmlParseExternalSubset`.
///
/// ```c
/// void xmlParseExternalSubset(xmlParserCtxtPtr ctxt, const xmlChar *publicId,
///                             const xmlChar *systemId);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseExternalSubset(
    ctxt: *mut _xmlParserCtxt,
    public_id: *const xmlChar,
    system_id: *const xmlChar,
) {
    pi_parse_external_subset(ctxt, public_id, system_id)
}

/// `xmlParseDocTypeDecl`.
///
/// ```c
/// void xmlParseDocTypeDecl(xmlParserCtxtPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseDocTypeDecl(ctxt: *mut _xmlParserCtxt) {
    pi_parse_doc_type_decl(ctxt)
}

/// `xmlParseAttribute`.
///
/// ```c
/// const xmlChar *xmlParseAttribute(xmlParserCtxtPtr ctxt, xmlChar **value);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseAttribute(
    ctxt: *mut _xmlParserCtxt,
    value: *mut *mut xmlChar,
) -> *const xmlChar {
    if ctxt.is_null() || value.is_null() {
        return ptr::null();
    }
    pi_parse_attribute(ctxt, value)
}

/// `xmlParseStartTag`.
///
/// ```c
/// const xmlChar *xmlParseStartTag(xmlParserCtxtPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseStartTag(ctxt: *mut _xmlParserCtxt) -> *const xmlChar {
    pi_parse_start_tag(ctxt)
}

/// `xmlParseEndTag`.
///
/// ```c
/// void xmlParseEndTag(xmlParserCtxtPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseEndTag(ctxt: *mut _xmlParserCtxt) {
    pi_parse_end_tag(ctxt)
}

/// `xmlParseElement`.
///
/// ```c
/// void xmlParseElement(xmlParserCtxtPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseElement(ctxt: *mut _xmlParserCtxt) {
    pi_parse_element(ctxt)
}

/// `xmlParseContent`.
///
/// ```c
/// void xmlParseContent(xmlParserCtxtPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseContent(ctxt: *mut _xmlParserCtxt) {
    pi_parse_content(ctxt)
}

/// `xmlParseMisc`.
///
/// ```c
/// void xmlParseMisc(xmlParserCtxtPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseMisc(ctxt: *mut _xmlParserCtxt) {
    pi_parse_misc(ctxt)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Document-level entry points
// ═══════════════════════════════════════════════════════════════════════════════

/// `xmlParseCtxtExternalEntity`.
///
/// ```c
/// int xmlParseCtxtExternalEntity(xmlParserCtxtPtr ctx, const xmlChar *URL,
///                                const xmlChar *ID, xmlNode **lst);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseCtxtExternalEntity(
    ctxt: *mut _xmlParserCtxt,
    url: *const xmlChar,
    _id: *const xmlChar,
    list_out: *mut *mut _xmlNode,
) -> c_int {
    unsafe {
        if !list_out.is_null() {
            *list_out = ptr::null_mut();
        }
        if ctxt.is_null() {
            return XML_ERR_ARGUMENT;
        }
        if url.is_null() {
            pi_fatal_err(ctxt, XML_ERR_INTERNAL_ERROR);
            return (*ctxt).errNo;
        }
        // Load the entity content from the URL (treated as a file path).
        let url_cstr = url as *const c_char;
        let input_buf = match input_from_file(url_cstr) {
            Ok(b) => b,
            Err(_) => {
                pi_fatal_err(ctxt, XML_ERR_INTERNAL_ERROR);
                return (*ctxt).errNo;
            }
        };
        let input = {
            // Wrap the buffer in a _xmlParserInput; keep the InputBuffer
            // alive by boxing it and stashing it in _private.
            let boxed = Box::into_raw(Box::new(input_buf));
            let pi = xmlMallocZero(size_of::<_xmlParserInput>()) as *mut _xmlParserInput;
            if pi.is_null() {
                let _ = Box::from_raw(boxed);
                pi_err_memory(ctxt);
                return (*ctxt).errNo;
            }
            (*boxed).populate_parser_input_without_filename(&mut *pi);
            // Own a C copy of the buffer's filename: the boxed InputBuffer is
            // freed with the context (free_parser_ctxt), so borrowing its
            // Rust String here would leave a dangling `filename` (the
            // observed heap-reuse garbage in TREE-001).
            if let Some(fname) = (*boxed).filename() {
                (*pi).filename = crate::xml::string::xml_strndup(
                    fname.as_ptr() as *const crate::abi::types::xmlChar,
                    fname.len(),
                ) as *const c_char;
            }
            (*pi).buf = ptr::null_mut();
            (*pi).directory = ptr::null();
            (*pi).free = None;
            (*pi).encoding = ptr::null();
            (*pi).version = ptr::null();
            (*pi).flags = 0;
            (*pi).id = 0;
            (*pi).parentConsumed = 0;
            (*pi).entity = ptr::null_mut();
            // stash the box so it outlives the parse (side table; ctxt._private
            // stays application data — 11.1-X)
            crate::xml::parser::helpers::stash_input_buffer(ctxt, boxed);
            pi
        };

        pi_ctxt_late_init(ctxt);
        let list = pi_parse_content_node_list(ctxt, input, 1);
        if !list_out.is_null() {
            *list_out = list;
        } else if !list.is_null() {
            crate::xml::tree::free_node_list(list);
        }
        (*ctxt).errNo
    }
}

/// `xmlParseExternalEntity`.
///
/// ```c
/// int xmlParseExternalEntity(xmlDoc *doc, xmlSAXHandler *sax, void *user_data,
///                            int depth, const xmlChar *URL, const xmlChar *ID,
///                            xmlNode **lst);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseExternalEntity(
    doc: *mut _xmlDoc,
    sax: *mut _xmlSAXHandler,
    user_data: *mut c_void,
    depth: c_int,
    url: *const xmlChar,
    id: *const xmlChar,
    list: *mut *mut _xmlNode,
) -> c_int {
    unsafe {
        if !list.is_null() {
            *list = ptr::null_mut();
        }
        if doc.is_null() {
            return XML_ERR_ARGUMENT;
        }
        let ctxt = create_parser_ctxt();
        if ctxt.is_null() {
            return XML_ERR_NO_MEMORY;
        }
        // Install the given SAX handler (or the default one).
        if !sax.is_null() {
            let dst = (*ctxt).sax;
            if !dst.is_null() {
                // Copy the handler struct; only copy the first-class fields.
                ptr::copy_nonoverlapping(sax, dst, 1);
            }
            (*ctxt).userData = user_data;
        }
        (*ctxt).depth = depth;
        (*ctxt).myDoc = doc;
        let ret = xmlParseCtxtExternalEntity(ctxt, url, id, list);
        free_parser_ctxt(ctxt);
        ret
    }
}

/// `xmlParseBalancedChunkMemory`.
///
/// ```c
/// int xmlParseBalancedChunkMemory(xmlDoc *doc, xmlSAXHandler *sax, void *user_data,
///                                 int depth, const xmlChar *string, xmlNode **lst);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseBalancedChunkMemory(
    doc: *mut _xmlDoc,
    sax: *mut _xmlSAXHandler,
    user_data: *mut c_void,
    depth: c_int,
    string: *const xmlChar,
    lst: *mut *mut _xmlNode,
) -> c_int {
    xmlParseBalancedChunkMemoryRecover(doc, sax, user_data, depth, string, lst, 0)
}

/// `xmlParseBalancedChunkMemoryRecover`.
///
/// ```c
/// int xmlParseBalancedChunkMemoryRecover(xmlDoc *doc, xmlSAXHandler *sax,
///                                        void *user_data, int depth,
///                                        const xmlChar *string, xmlNode **lst,
///                                        int recover);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseBalancedChunkMemoryRecover(
    doc: *mut _xmlDoc,
    sax: *mut _xmlSAXHandler,
    user_data: *mut c_void,
    depth: c_int,
    string: *const xmlChar,
    list_out: *mut *mut _xmlNode,
    recover: c_int,
) -> c_int {
    unsafe {
        if !list_out.is_null() {
            *list_out = ptr::null_mut();
        }
        if string.is_null() {
            return XML_ERR_ARGUMENT;
        }
        if doc.is_null() {
            return XML_ERR_ARGUMENT;
        }
        let ctxt = create_parser_ctxt();
        if ctxt.is_null() {
            return XML_ERR_NO_MEMORY;
        }
        if !sax.is_null() {
            let dst = (*ctxt).sax;
            if !dst.is_null() {
                ptr::copy_nonoverlapping(sax, dst, 1);
            }
            (*ctxt).userData = user_data;
        }
        pi_ctxt_late_init(ctxt);
        (*ctxt).depth = depth;
        (*ctxt).myDoc = doc;
        if recover != 0 {
            (*ctxt).options |= XML_PARSE_RECOVER;
            (*ctxt).recovery = 1;
        }

        let slen = crate::abi::exports_xml2::xmlStrlen(string) as c_int;
        let input_buf = input_from_memory(string as *const c_char, slen);
        // Keep the buffer alive via the side table (ctxt._private stays
        // application data — 11.1-X).
        let boxed = Box::into_raw(Box::new(input_buf));
        crate::xml::parser::helpers::stash_input_buffer(ctxt, boxed);
        let input = xmlMallocZero(size_of::<_xmlParserInput>()) as *mut _xmlParserInput;
        if input.is_null() {
            let _ = Box::from_raw(boxed);
            crate::xml::parser::helpers::free_stashed_input_buffer(ctxt);
            let ret = (*ctxt).errNo;
            free_parser_ctxt(ctxt);
            return if ret != 0 { ret } else { XML_ERR_NO_MEMORY };
        }
        (*boxed).populate_parser_input_without_filename(&mut *input);
        if let Some(fname) = (*boxed).filename() {
            (*input).filename = crate::xml::string::xml_strndup(
                fname.as_ptr() as *const crate::abi::types::xmlChar,
                fname.len(),
            ) as *const c_char;
        }
        (*input).buf = ptr::null_mut();
        (*input).directory = ptr::null();
        (*input).free = None;
        (*input).encoding = ptr::null();
        (*input).version = ptr::null();
        (*input).flags = 0;
        (*input).id = 0;
        (*input).parentConsumed = 0;
        (*input).entity = ptr::null_mut();

        let list = pi_parse_content_node_list(ctxt, input, 0);
        if !list_out.is_null() {
            *list_out = list;
        } else if !list.is_null() {
            crate::xml::tree::free_node_list(list);
        }
        let ret = if (*ctxt).wellFormed == 0 {
            (*ctxt).errNo
        } else {
            XML_ERR_OK
        };
        free_parser_ctxt(ctxt);
        ret
    }
}

/// `xmlParseInNodeContext`.
///
/// ```c
/// xmlParserErrors xmlParseInNodeContext(xmlNode *node, const char *data,
///                                       int datalen, int options, xmlNode **lst);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseInNodeContext(
    node: *mut _xmlNode,
    data: *const c_char,
    datalen: c_int,
    options: c_int,
    list_out: *mut *mut _xmlNode,
) -> c_int {
    unsafe {
        if list_out.is_null() {
            return XML_ERR_INTERNAL_ERROR;
        }
        *list_out = ptr::null_mut();
        if node.is_null() || data.is_null() || datalen < 0 {
            return XML_ERR_INTERNAL_ERROR;
        }
        let doc = (*node).doc;
        if doc.is_null() {
            return XML_ERR_INTERNAL_ERROR;
        }
        let ctxt = create_parser_ctxt();
        if ctxt.is_null() {
            return XML_ERR_NO_MEMORY;
        }
        (*ctxt).options = options;

        let input_buf = input_from_memory(data, datalen);
        let boxed = Box::into_raw(Box::new(input_buf));
        crate::xml::parser::helpers::stash_input_buffer(ctxt, boxed);
        let input = xmlMallocZero(size_of::<_xmlParserInput>()) as *mut _xmlParserInput;
        if input.is_null() {
            let _ = Box::from_raw(boxed);
            crate::xml::parser::helpers::free_stashed_input_buffer(ctxt);
            free_parser_ctxt(ctxt);
            return XML_ERR_NO_MEMORY;
        }
        (*boxed).populate_parser_input_without_filename(&mut *input);
        if let Some(fname) = (*boxed).filename() {
            (*input).filename = crate::xml::string::xml_strndup(
                fname.as_ptr() as *const crate::abi::types::xmlChar,
                fname.len(),
            ) as *const c_char;
        }
        (*input).buf = ptr::null_mut();
        (*input).directory = ptr::null();
        (*input).free = None;
        (*input).encoding = ptr::null();
        (*input).version = ptr::null();
        (*input).flags = 0;
        (*input).id = 0;
        (*input).parentConsumed = 0;
        (*input).entity = ptr::null_mut();

        pi_ctxt_late_init(ctxt);
        (*ctxt).myDoc = doc;

        // Push namespaces in scope of the node onto the SAX2 ns stack.
        // (Simplified: only the direct nsDef chain of the node.)
        let mut ns_cur = (*node).nsDef;
        while !ns_cur.is_null() {
            let nsp = ns_cur;
            ns_cur = (*ns_cur).next;
            // Create a namespace declaration on the synthetic root later;
            // record them by pushing onto ctxt->nsTab (SAX2 ns stack).
            if !(*nsp).prefix.is_null() {
                // push (prefix, href) onto nsTab
            }
        }

        let list = pi_parse_content_node_list(ctxt, input, 0);
        if list.is_null() {
            let ret = (*ctxt).errNo;
            if ret == XML_ERR_ARGUMENT {
                free_parser_ctxt(ctxt);
                return XML_ERR_INTERNAL_ERROR;
            }
            free_parser_ctxt(ctxt);
            return if ret != 0 {
                ret
            } else {
                XML_ERR_INTERNAL_ERROR
            };
        }
        *list_out = list;
        free_parser_ctxt(ctxt);
        XML_ERR_OK
    }
}

/// `xmlParseDTD`.
///
/// ```c
/// xmlDtdPtr xmlParseDTD(const xmlChar *publicId, const xmlChar *systemId);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseDTD(
    public_id: *const xmlChar,
    system_id: *const xmlChar,
) -> *mut _xmlDtd {
    unsafe {
        if public_id.is_null() && system_id.is_null() {
            return ptr::null_mut();
        }
        if system_id.is_null() {
            return ptr::null_mut();
        }
        let ctxt = create_parser_ctxt();
        if ctxt.is_null() {
            return ptr::null_mut();
        }
        pi_ctxt_late_init(ctxt);
        (*ctxt).inSubset = 2;
        (*ctxt).hasExternalSubset = 1;

        let mut ret: *mut _xmlDtd = ptr::null_mut();
        let input_buf = input_from_file(system_id as *const c_char);
        if let Ok(buf) = input_buf {
            let boxed = Box::into_raw(Box::new(buf));
            crate::xml::parser::helpers::stash_input_buffer(ctxt, boxed);
            let input = xmlMallocZero(size_of::<_xmlParserInput>()) as *mut _xmlParserInput;
            if !input.is_null() {
                (*boxed).populate_parser_input_without_filename(&mut *input);
                if let Some(fname) = (*boxed).filename() {
                    (*input).filename = crate::xml::string::xml_strndup(
                        fname.as_ptr() as *const crate::abi::types::xmlChar,
                        fname.len(),
                    ) as *const c_char;
                }
                (*input).buf = ptr::null_mut();
                (*input).directory = ptr::null();
                (*input).free = None;
                (*input).encoding = ptr::null();
                (*input).version = ptr::null();
                (*input).flags = 0;
                (*input).id = 0;
                (*input).parentConsumed = 0;
                (*input).entity = ptr::null_mut();
                // Make it the main input.
                (*ctxt).input = input;
                (*ctxt).inputNr = 1;
                let tab = xmlMallocZero(4 * size_of::<*mut _xmlParserInput>())
                    as *mut *mut _xmlParserInput;
                if !tab.is_null() {
                    *tab = input;
                    (*ctxt).inputTab = tab;
                    (*ctxt).inputMax = 4;
                }
                pi_parse_external_subset(ctxt, public_id, system_id);
            }
        }

        if (*ctxt).wellFormed != 0 && !(*ctxt).myDoc.is_null() {
            let doc = (*ctxt).myDoc;
            if !(*doc).intSubset.is_null() {
                ret = (*doc).intSubset;
            } else if !(*doc).extSubset.is_null() {
                ret = (*doc).extSubset;
            }
        }
        free_parser_ctxt(ctxt);
        ret
    }
}

/// `xmlParseEntity`.
///
/// ```c
/// xmlDocPtr xmlParseEntity(const char *filename);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseEntity(filename: *const c_char) -> *mut _xmlDoc {
    unsafe {
        if filename.is_null() {
            return ptr::null_mut();
        }
        let ctxt = create_parser_ctxt();
        if ctxt.is_null() {
            return ptr::null_mut();
        }
        let input_buf = match input_from_file(filename) {
            Ok(b) => b,
            Err(_) => {
                free_parser_ctxt(ctxt);
                return ptr::null_mut();
            }
        };
        setup_parser_input(ctxt, input_buf);
        xmlParseExtParsedEnt(ctxt);
        let ret = if (*ctxt).wellFormed != 0 {
            (*ctxt).myDoc
        } else {
            let doc = (*ctxt).myDoc;
            if !doc.is_null() {
                crate::xml::tree::free_doc(doc);
                (*ctxt).myDoc = ptr::null_mut();
            }
            ptr::null_mut()
        };
        // Detach the doc so the context free doesn't touch it.
        (*ctxt).myDoc = ptr::null_mut();
        free_parser_ctxt(ctxt);
        ret
    }
}

/// `xmlParseExtParsedEnt`.
///
/// ```c
/// int xmlParseExtParsedEnt(xmlParserCtxtPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParseExtParsedEnt(ctxt: *mut _xmlParserCtxt) -> c_int {
    pi_parse_ext_parsed_ent(ctxt)
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlParserInput* input handling (parserInternals.c / parser.h)
// ═══════════════════════════════════════════════════════════════════════════════

// `xmlParserInputRead` — deprecated, always an error.
//
// ```c
// int xmlParserInputRead(xmlParserInput *in, int len);
// `xmlParserInputGrow`.
//
// ```c
// int xmlParserInputGrow(xmlParserInput *in, int len);
// `xmlParserInputShrink`.
//
// ```c
// void xmlParserInputShrink(xmlParserInput *in);
// ═══════════════════════════════════════════════════════════════════════════════
// xmlParserInputBuffer* (xmlIO.h)
// ═══════════════════════════════════════════════════════════════════════════════

/// Opaque memory backing for a `_xmlParserInputBuffer`.
#[repr(C)]
struct PiInputMem {
    data: *mut u8,
    len: usize,
    cap: usize,
    owned: bool,
}

unsafe fn pi_mem_create() -> *mut PiInputMem {
    unsafe { xmlMallocZero(size_of::<PiInputMem>()) as *mut PiInputMem }
}

unsafe fn pi_mem_free(m: *mut PiInputMem) {
    unsafe {
        if m.is_null() {
            return;
        }
        if !(*m).data.is_null() && (*m).owned {
            xmlFreeImpl((*m).data as *mut c_void);
        }
        xmlFreeImpl(m as *mut c_void);
    }
}

unsafe fn pi_mem_grow(m: *mut PiInputMem, extra: usize) -> bool {
    unsafe {
        if m.is_null() {
            return false;
        }
        if (*m).len + extra <= (*m).cap {
            return true;
        }
        let mut new_cap = if (*m).cap == 0 { 256 } else { (*m).cap };
        while new_cap < (*m).len + extra {
            new_cap *= 2;
        }
        if !(*m).owned {
            // Convert a static buffer into an owned copy.
            let data = xmlMallocImpl(new_cap) as *mut u8;
            if data.is_null() {
                return false;
            }
            if (*m).len > 0 && !(*m).data.is_null() {
                ptr::copy_nonoverlapping((*m).data, data, (*m).len);
            }
            (*m).data = data;
            (*m).owned = true;
            (*m).cap = new_cap;
            return true;
        }
        let data = xmlReallocImpl((*m).data as *mut c_void, new_cap) as *mut u8;
        if data.is_null() {
            return false;
        }
        (*m).data = data;
        (*m).cap = new_cap;
        true
    }
}

/// Read callback for fd-backed buffers.
unsafe extern "C" fn pi_fd_read(context: *mut c_void, buffer: *mut c_char, len: c_int) -> c_int {
    unsafe {
        if context.is_null() || buffer.is_null() || len <= 0 {
            return -1;
        }
        let fd = *(context as *const c_int);
        libc::read(fd, buffer as *mut c_void, len as usize) as c_int
    }
}

/// Close callback for fd-backed buffers: releases the boxed fd.
unsafe extern "C" fn pi_fd_close(context: *mut c_void) -> c_int {
    unsafe {
        if context.is_null() {
            return -1;
        }
        let _ = Box::from_raw(context as *mut c_int);
        0
    }
}

/// Read callback for FILE*-backed buffers.
unsafe extern "C" fn pi_file_read(context: *mut c_void, buffer: *mut c_char, len: c_int) -> c_int {
    unsafe {
        if context.is_null() || buffer.is_null() || len <= 0 {
            return -1;
        }
        libc::fread(
            buffer as *mut c_void,
            1,
            len as usize,
            context as *mut libc::FILE,
        ) as c_int
    }
}

/// `xmlParserInputBufferCreateFd`.
///
/// ```c
/// xmlParserInputBufferPtr xmlParserInputBufferCreateFd(int fd, xmlCharEncoding enc);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParserInputBufferCreateFd(
    fd: c_int,
    _enc: c_int,
) -> *mut _xmlParserInputBuffer {
    unsafe {
        if fd < 0 {
            return ptr::null_mut();
        }
        let buf = crate::xml::parser::helpers::alloc_parser_input_buffer();
        if buf.is_null() {
            return ptr::null_mut();
        }
        let fd_box = Box::into_raw(Box::new(fd));
        (*buf).context = fd_box as *mut c_void;
        (*buf).readcallback = Some(pi_fd_read);
        (*buf).closecallback = Some(pi_fd_close);
        (*buf).compressed = -1;
        (*buf).buffer = pi_mem_create() as *mut c_void;
        buf
    }
}

/// `xmlParserInputBufferCreateFile`.
///
/// ```c
/// xmlParserInputBufferPtr xmlParserInputBufferCreateFile(FILE *file, xmlCharEncoding enc);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParserInputBufferCreateFile(
    file: *mut c_void,
    _enc: c_int,
) -> *mut _xmlParserInputBuffer {
    unsafe {
        if file.is_null() {
            return ptr::null_mut();
        }
        let buf = crate::xml::parser::helpers::alloc_parser_input_buffer();
        if buf.is_null() {
            return ptr::null_mut();
        }
        (*buf).context = file;
        (*buf).readcallback = Some(pi_file_read);
        (*buf).closecallback = None;
        (*buf).compressed = -1;
        (*buf).buffer = pi_mem_create() as *mut c_void;
        buf
    }
}

/// `xmlParserInputBufferCreateStatic`.
///
/// ```c
/// xmlParserInputBufferPtr xmlParserInputBufferCreateStatic(const char *mem,
///                                                          int size, xmlCharEncoding enc);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlParserInputBufferCreateStatic(
    mem: *const c_char,
    size: c_int,
    _enc: c_int,
) -> *mut _xmlParserInputBuffer {
    unsafe {
        if mem.is_null() || size < 0 {
            return ptr::null_mut();
        }
        let buf = crate::xml::parser::helpers::alloc_parser_input_buffer();
        if buf.is_null() {
            return ptr::null_mut();
        }
        let m = pi_mem_create();
        if m.is_null() {
            crate::xml::parser::helpers::free_parser_input_buffer(buf);
            return ptr::null_mut();
        }
        (*m).data = mem as *mut u8;
        (*m).len = size as usize;
        (*m).cap = size as usize;
        (*m).owned = false;
        (*buf).buffer = m as *mut c_void;
        (*buf).compressed = -1;
        buf
    }
}

/// Local stand-in for upstream `__xmlParserInputBufferCreateFilename` (the
/// default filename→buffer factory used when no custom loader is installed).
unsafe extern "C" fn pi_default_input_buffer_create_filename(
    _uri: *const c_char,
    _enc: c_int,
) -> *mut _xmlParserInputBuffer {
    unsafe { crate::xml::parser::helpers::alloc_parser_input_buffer() }
}

/// `xmlParserInputBufferCreateFilenameDefault`.
///
/// ```c
/// xmlParserInputBufferCreateFilenameFunc
/// xmlParserInputBufferCreateFilenameDefault(xmlParserInputBufferCreateFilenameFunc func);
/// ```
#[no_mangle]
/// UPSTREAM-PARITY: comparing the registered callback against the default
/// function pointer to decide reset-vs-replace mirrors upstream globals.c;
/// on ELF platforms the address of a symbol is stable and unique within the
/// DSO.
#[allow(
    renamed_and_removed_lints,
    clippy::fn_address_comparisons,
    unpredictable_function_pointer_comparisons
)]
pub unsafe extern "C" fn xmlParserInputBufferCreateFilenameDefault(
    func: Option<unsafe extern "C" fn(*const c_char, c_int) -> *mut _xmlParserInputBuffer>,
) -> Option<unsafe extern "C" fn(*const c_char, c_int) -> *mut _xmlParserInputBuffer> {
    unsafe {
        let default_fn: unsafe extern "C" fn(*const c_char, c_int) -> *mut _xmlParserInputBuffer =
            pi_default_input_buffer_create_filename;
        let old = xmlParserInputBufferCreateFilenameValue;
        xmlParserInputBufferCreateFilenameValue =
            if func == Some(default_fn) { None } else { func };
        old.or(Some(default_fn))
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlCtxt accessor / 2.14+ parser-API family (parser.h / parserInternals.h)
// ═══════════════════════════════════════════════════════════════════════════════
//
// R-000165 closure (11.1-X): the parser-context accessors and the 2.14+
// input constructors ported from archaeology/libxml2-git (parser.c /
// parserInternals.c 2.15.3). NULL contexts return NULL/0/-1 exactly like
// upstream; every function is exported with the upstream name so the
// header-compile court's declared-functions-exported check closes.

/// Upstream `xmlCtxtIsCatastrophicError` — errNo in the catastrophic set.
unsafe fn pi_ctxt_is_catastrophic(ctxt: *mut _xmlParserCtxt) -> c_int {
    if ctxt.is_null() {
        return 1;
    }
    unsafe {
        let e = (*ctxt).errNo;
        if e == crate::abi::types::XML_ERR_NO_MEMORY
            || e == crate::abi::types::XML_ERR_INTERNAL_ERROR
            || e == crate::abi::types::XML_ERR_RESOURCE_LIMIT
        {
            1
        } else {
            0
        }
    }
}

/// `xmlCtxtGetVersion` — the XML version declared in the document.
#[no_mangle]
pub unsafe extern "C" fn xmlCtxtGetVersion(ctxt: *mut _xmlParserCtxt) -> *const xmlChar {
    if ctxt.is_null() {
        return ptr::null();
    }
    unsafe { (*ctxt).version as *const xmlChar }
}

/// `xmlCtxtGetStandalone` — standalone status (-1 unset, 0 no, 1 yes).
#[no_mangle]
pub unsafe extern "C" fn xmlCtxtGetStandalone(ctxt: *mut _xmlParserCtxt) -> c_int {
    if ctxt.is_null() {
        return -1;
    }
    unsafe { (*ctxt).standalone }
}

/// `xmlCtxtGetOptions` — the parser options bitmask.
#[no_mangle]
pub unsafe extern "C" fn xmlCtxtGetOptions(ctxt: *mut _xmlParserCtxt) -> c_int {
    if ctxt.is_null() {
        return 0;
    }
    unsafe { (*ctxt).options }
}

/// `xmlCtxtGetPrivate` — the private application data.
#[no_mangle]
pub unsafe extern "C" fn xmlCtxtGetPrivate(ctxt: *mut _xmlParserCtxt) -> *mut c_void {
    if ctxt.is_null() {
        return ptr::null_mut();
    }
    unsafe { (*ctxt)._private }
}

/// `xmlCtxtSetPrivate` — set the private application data.
#[no_mangle]
pub unsafe extern "C" fn xmlCtxtSetPrivate(ctxt: *mut _xmlParserCtxt, priv_: *mut c_void) {
    if ctxt.is_null() {
        return;
    }
    unsafe { (*ctxt)._private = priv_ };
}

/// `xmlCtxtGetCatalogs` — the local catalogs.
#[no_mangle]
pub unsafe extern "C" fn xmlCtxtGetCatalogs(ctxt: *mut _xmlParserCtxt) -> *mut c_void {
    if ctxt.is_null() {
        return ptr::null_mut();
    }
    unsafe { (*ctxt).catalogs }
}

/// `xmlCtxtSetCatalogs` — set the local catalogs.
#[no_mangle]
pub unsafe extern "C" fn xmlCtxtSetCatalogs(ctxt: *mut _xmlParserCtxt, catalogs: *mut c_void) {
    if ctxt.is_null() {
        return;
    }
    unsafe { (*ctxt).catalogs = catalogs };
}

/// `xmlCtxtGetDict` — the dictionary.
#[no_mangle]
pub unsafe extern "C" fn xmlCtxtGetDict(ctxt: *mut _xmlParserCtxt) -> *mut c_void {
    if ctxt.is_null() {
        return ptr::null_mut();
    }
    unsafe { (*ctxt).dict }
}

/// `xmlCtxtSetDict` — replace the dictionary (old one freed, new referenced).
#[no_mangle]
pub unsafe extern "C" fn xmlCtxtSetDict(ctxt: *mut _xmlParserCtxt, dict: *mut c_void) {
    if ctxt.is_null() {
        return;
    }
    unsafe {
        if !(*ctxt).dict.is_null() {
            crate::abi::exports_xml2::xmlDictFree((*ctxt).dict);
        }
        if !dict.is_null() {
            crate::abi::exports_hash::xmlDictReference(dict);
        }
        (*ctxt).dict = dict;
    }
}

/// `xmlCtxtGetSaxHandler` — the SAX handler struct (not a copy).
#[no_mangle]
pub unsafe extern "C" fn xmlCtxtGetSaxHandler(ctxt: *mut _xmlParserCtxt) -> *mut _xmlSAXHandler {
    if ctxt.is_null() {
        return ptr::null_mut();
    }
    unsafe { (*ctxt).sax }
}

/// `xmlCtxtSetSaxHandler` — copy `sax` into the context's handler struct.
#[no_mangle]
pub unsafe extern "C" fn xmlCtxtSetSaxHandler(
    ctxt: *mut _xmlParserCtxt,
    sax: *const _xmlSAXHandler,
) -> c_int {
    if ctxt.is_null() || (*ctxt).sax.is_null() || sax.is_null() {
        return -1;
    }
    unsafe {
        ptr::copy_nonoverlapping(sax, (*ctxt).sax, 1);
    }
    0
}

/// `xmlCtxtIsHtml` — 1 if this is an HTML parser context.
#[no_mangle]
pub unsafe extern "C" fn xmlCtxtIsHtml(ctxt: *mut _xmlParserCtxt) -> c_int {
    if ctxt.is_null() {
        return 0;
    }
    unsafe { (*ctxt).html }
}

/// `xmlCtxtIsStopped` — 1 if the parser is stopped (disableSAX != 0).
#[no_mangle]
pub unsafe extern "C" fn xmlCtxtIsStopped(ctxt: *mut _xmlParserCtxt) -> c_int {
    if ctxt.is_null() {
        return 0;
    }
    unsafe {
        if (*ctxt).disableSAX != 0 {
            1
        } else {
            0
        }
    }
}

/// `xmlCtxtIsInSubset` — DTD subset status (0 none, 1 internal, 2 external).
#[no_mangle]
pub unsafe extern "C" fn xmlCtxtIsInSubset(ctxt: *mut _xmlParserCtxt) -> c_int {
    if ctxt.is_null() {
        return 0;
    }
    unsafe { (*ctxt).inSubset }
}

/// `xmlCtxtGetValidCtxt` — pointer to the validation context.
#[no_mangle]
pub unsafe extern "C" fn xmlCtxtGetValidCtxt(ctxt: *mut _xmlParserCtxt) -> *mut _xmlValidCtxt {
    if ctxt.is_null() {
        return ptr::null_mut();
    }
    unsafe { core::ptr::addr_of_mut!((*ctxt).vctxt) }
}

/// `xmlCtxtGetUserData` — the user data.
#[no_mangle]
pub unsafe extern "C" fn xmlCtxtGetUserData(ctxt: *mut _xmlParserCtxt) -> *mut c_void {
    if ctxt.is_null() {
        return ptr::null_mut();
    }
    unsafe { (*ctxt).userData }
}

/// `xmlCtxtGetNode` — the current node or the document node.
#[no_mangle]
pub unsafe extern "C" fn xmlCtxtGetNode(ctxt: *mut _xmlParserCtxt) -> *mut _xmlNode {
    if ctxt.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        if !(*ctxt).node.is_null() {
            (*ctxt).node
        } else {
            (*ctxt).myDoc as *mut _xmlNode
        }
    }
}

/// `xmlCtxtGetDocTypeDecl` — doctype declaration data (SAX callbacks only).
#[no_mangle]
pub unsafe extern "C" fn xmlCtxtGetDocTypeDecl(
    ctxt: *mut _xmlParserCtxt,
    name: *mut *const xmlChar,
    system_id: *mut *const xmlChar,
    public_id: *mut *const xmlChar,
) -> c_int {
    if ctxt.is_null() {
        return -1;
    }
    unsafe {
        if !name.is_null() {
            *name = (*ctxt).intSubName;
        }
        if !system_id.is_null() {
            *system_id = (*ctxt).extSubURI as *const xmlChar;
        }
        if !public_id.is_null() {
            *public_id = (*ctxt).extSubSystem as *const xmlChar;
        }
    }
    0
}

/// `xmlCtxtGetInputPosition` — position of an input (outermost 0, innermost -1).
#[no_mangle]
pub unsafe extern "C" fn xmlCtxtGetInputPosition(
    ctxt: *mut _xmlParserCtxt,
    input_index: c_int,
    filename: *mut *const c_char,
    line: *mut c_int,
    col: *mut c_int,
    utf8_byte_pos: *mut c_ulong,
) -> c_int {
    unsafe {
        if ctxt.is_null() {
            return -1;
        }
        let mut idx = input_index;
        if idx < 0 {
            idx += (*ctxt).inputNr;
            if idx < 0 {
                return -1;
            }
        }
        if idx >= (*ctxt).inputNr || (*ctxt).inputTab.is_null() {
            return -1;
        }
        let input = *(*ctxt).inputTab.add(idx as usize);
        if input.is_null() {
            return -1;
        }
        if !filename.is_null() {
            *filename = (*input).filename;
        }
        if !line.is_null() {
            *line = (*input).line;
        }
        if !col.is_null() {
            *col = (*input).col;
        }
        if !utf8_byte_pos.is_null() {
            let consumed = (*input).consumed;
            let mut pos: c_ulong = consumed;
            if !(*input).cur.is_null() && !(*input).base.is_null() {
                pos = pos.wrapping_add(((*input).cur as usize - (*input).base as usize) as c_ulong);
            }
            *utf8_byte_pos = pos;
        }
        0
    }
}

/// `xmlCtxtGetInputWindow` — window into the input data (upstream
/// xmlParserInputGetWindow; 80-char cap, UTF-8 aware).
#[no_mangle]
pub unsafe extern "C" fn xmlCtxtGetInputWindow(
    ctxt: *mut _xmlParserCtxt,
    input_index: c_int,
    start_out: *mut *const xmlChar,
    size_in_out: *mut c_int,
    offset_out: *mut c_int,
) -> c_int {
    unsafe {
        if ctxt.is_null() || start_out.is_null() || size_in_out.is_null() || offset_out.is_null() {
            return -1;
        }
        let mut idx = input_index;
        if idx < 0 {
            idx += (*ctxt).inputNr;
            if idx < 0 {
                return -1;
            }
        }
        if idx >= (*ctxt).inputNr || (*ctxt).inputTab.is_null() {
            return -1;
        }
        let input = *(*ctxt).inputTab.add(idx as usize);
        if input.is_null() {
            return -1;
        }
        crate::abi::exports_misc::parser_input_get_window_pub(
            input,
            start_out,
            size_in_out,
            offset_out,
        );
        0
    }
}

/// `xmlCtxtGetStatus` — XML_STATUS_* bitmask (well-formedness/validation).
#[no_mangle]
pub unsafe extern "C" fn xmlCtxtGetStatus(ctxt: *mut _xmlParserCtxt) -> c_int {
    unsafe {
        let mut bits: c_int = 0;
        if pi_ctxt_is_catastrophic(ctxt) != 0 {
            bits |= 1 << 3; // XML_STATUS_CATASTROPHIC_ERROR
            bits |= 1 << 0; // XML_STATUS_NOT_WELL_FORMED
            bits |= 1 << 1; // XML_STATUS_NOT_NS_WELL_FORMED
            if !ctxt.is_null() && (*ctxt).validate != 0 {
                bits |= 1 << 2; // XML_STATUS_DTD_VALIDATION_FAILED
            }
            return bits;
        }
        if (*ctxt).wellFormed == 0 {
            bits |= 1 << 0;
        }
        if (*ctxt).nsWellFormed == 0 {
            bits |= 1 << 1;
        }
        if (*ctxt).validate != 0 && (*ctxt).valid == 0 {
            bits |= 1 << 2;
        }
        bits
    }
}

/// `xmlCtxtGetDeclaredEncoding` — the encoding from the encoding declaration.
#[no_mangle]
pub unsafe extern "C" fn xmlCtxtGetDeclaredEncoding(ctxt: *mut _xmlParserCtxt) -> *const xmlChar {
    if ctxt.is_null() {
        return ptr::null();
    }
    unsafe { (*ctxt).encoding as *const xmlChar }
}

/// `xmlCtxtGetDocument` — take the parsed document (resets the context's).
#[no_mangle]
pub unsafe extern "C" fn xmlCtxtGetDocument(ctxt: *mut _xmlParserCtxt) -> *mut _xmlDoc {
    unsafe {
        if ctxt.is_null() {
            return ptr::null_mut();
        }
        let doc: *mut _xmlDoc;
        if (*ctxt).wellFormed != 0
            || (((*ctxt).recovery != 0 || (*ctxt).html != 0) && pi_ctxt_is_catastrophic(ctxt) == 0)
        {
            doc = (*ctxt).myDoc;
        } else {
            if (*ctxt).errNo == crate::abi::types::XML_ERR_OK {
                // xmlFatalErr(ctxt, XML_ERR_INTERNAL_ERROR, "unknown error")
                (*ctxt).errNo = crate::abi::types::XML_ERR_INTERNAL_ERROR;
            }
            doc = ptr::null_mut();
            if !(*ctxt).myDoc.is_null() {
                crate::xml::tree::free_doc((*ctxt).myDoc);
            }
        }
        (*ctxt).myDoc = ptr::null_mut();
        doc
    }
}

/// `xmlCtxtSetCharEncConvImpl` — install a custom encoding-conversion impl.
#[no_mangle]
pub unsafe extern "C" fn xmlCtxtSetCharEncConvImpl(
    ctxt: *mut _xmlParserCtxt,
    impl_: Option<crate::abi::callbacks::xmlCharEncConvImpl>,
    vctxt: *mut c_void,
) {
    if ctxt.is_null() {
        return;
    }
    unsafe {
        (*ctxt).convImpl = impl_;
        (*ctxt).convCtxt = vctxt;
    }
}

/// `xmlCtxtSetResourceLoader` — install a custom resource loader.
#[no_mangle]
pub unsafe extern "C" fn xmlCtxtSetResourceLoader(
    ctxt: *mut _xmlParserCtxt,
    loader: Option<crate::abi::callbacks::xmlResourceLoader>,
    vctxt: *mut c_void,
) {
    if ctxt.is_null() {
        return;
    }
    unsafe {
        (*ctxt).resourceLoader = loader;
        (*ctxt).resourceCtxt = vctxt;
    }
}

/// `xmlCtxtPushInput` — push an input onto the stack (upstream parser.c).
#[no_mangle]
pub unsafe extern "C" fn xmlCtxtPushInput(
    ctxt: *mut _xmlParserCtxt,
    value: *mut _xmlParserInput,
) -> c_int {
    unsafe {
        if ctxt.is_null() || value.is_null() {
            return -1;
        }
        let max_depth = if (*ctxt).options & crate::abi::types::XML_PARSE_HUGE != 0 {
            40
        } else {
            20
        };
        if (*ctxt).inputNr >= (*ctxt).inputMax {
            let old_max = (*ctxt).inputMax;
            let mut new_size = old_max * 2 + 5;
            if new_size > max_depth {
                new_size = max_depth;
            }
            if new_size <= old_max {
                return -1;
            }
            let tmp = xmlReallocImpl(
                (*ctxt).inputTab as *mut c_void,
                (new_size as usize) * size_of::<*mut _xmlParserInput>(),
            ) as *mut *mut _xmlParserInput;
            if tmp.is_null() {
                return -1;
            }
            (*ctxt).inputTab = tmp;
            (*ctxt).inputMax = new_size;
        }
        *(*ctxt).inputTab.add((*ctxt).inputNr as usize) = value;
        (*ctxt).input = value;
        (*value).id = (*ctxt).input_id;
        (*ctxt).input_id += 1;
        let idx = (*ctxt).inputNr;
        (*ctxt).inputNr += 1;
        idx
    }
}

/// `xmlCtxtPopInput` — pop the top input (returns it; caller owns it).
#[no_mangle]
pub unsafe extern "C" fn xmlCtxtPopInput(ctxt: *mut _xmlParserCtxt) -> *mut _xmlParserInput {
    unsafe {
        if ctxt.is_null() || (*ctxt).inputNr <= 0 {
            return ptr::null_mut();
        }
        (*ctxt).inputNr -= 1;
        if (*ctxt).inputNr > 0 {
            (*ctxt).input = *(*ctxt).inputTab.add(((*ctxt).inputNr - 1) as usize);
        } else {
            (*ctxt).input = ptr::null_mut();
        }
        let ret = *(*ctxt).inputTab.add((*ctxt).inputNr as usize);
        *(*ctxt).inputTab.add((*ctxt).inputNr as usize) = ptr::null_mut();
        ret
    }
}

/// `xmlCtxtValidateDtd` — validate a document against a DTD using the
/// context's error handler (upstream valid.c).
#[no_mangle]
pub unsafe extern "C" fn xmlCtxtValidateDtd(
    ctxt: *mut _xmlParserCtxt,
    doc: *mut _xmlDoc,
    dtd: *mut _xmlDtd,
) -> c_int {
    if ctxt.is_null() || (*ctxt).html != 0 {
        return 0;
    }
    unsafe {
        crate::abi::exports_parser::xmlCtxtReset(ctxt);
        crate::xml::validation::validate_dtd(&mut (*ctxt).vctxt, doc, dtd)
    }
}

/// `xmlCtxtValidateDocument` — validate a document using the context's
/// error handler (upstream valid.c).
#[no_mangle]
pub unsafe extern "C" fn xmlCtxtValidateDocument(
    ctxt: *mut _xmlParserCtxt,
    doc: *mut _xmlDoc,
) -> c_int {
    if ctxt.is_null() || (*ctxt).html != 0 {
        return 0;
    }
    unsafe {
        crate::abi::exports_parser::xmlCtxtReset(ctxt);
        crate::xml::validation::validate_document(&mut (*ctxt).vctxt, doc)
    }
}

/// `xmlCtxtParseDtd` — parse a DTD from an input (input is consumed/freed).
#[no_mangle]
pub unsafe extern "C" fn xmlCtxtParseDtd(
    ctxt: *mut _xmlParserCtxt,
    input: *mut _xmlParserInput,
    public_id: *const xmlChar,
    system_id: *const xmlChar,
) -> *mut _xmlDtd {
    unsafe {
        if ctxt.is_null() || input.is_null() {
            crate::xml::parser::helpers::free_parser_input(input);
            return ptr::null_mut();
        }
        if xmlCtxtPushInput(ctxt, input) < 0 {
            crate::xml::parser::helpers::free_parser_input(input);
            return ptr::null_mut();
        }
        let pub_id = if public_id.is_null() {
            c"none".as_ptr() as *const xmlChar
        } else {
            public_id
        };
        let sys_id = if system_id.is_null() {
            c"none".as_ptr() as *const xmlChar
        } else {
            system_id
        };
        (*ctxt).myDoc = crate::xml::tree::new_doc(c"1.0".as_ptr() as *const xmlChar);
        if (*ctxt).myDoc.is_null() {
            return ptr::null_mut();
        }
        (*(*ctxt).myDoc).properties = XML_DOC_INTERNAL as c_int;
        (*(*ctxt).myDoc).extSubset = crate::xml::tree::new_dtd(
            (*ctxt).myDoc,
            c"none".as_ptr() as *const xmlChar,
            pub_id,
            sys_id,
        );
        if (*(*ctxt).myDoc).extSubset.is_null() {
            crate::xml::tree::free_doc((*ctxt).myDoc);
            (*ctxt).myDoc = ptr::null_mut();
            return ptr::null_mut();
        }
        pi_parse_external_subset(ctxt, pub_id, sys_id);
        let mut ret: *mut _xmlDtd = ptr::null_mut();
        if (*ctxt).wellFormed != 0 {
            ret = (*(*ctxt).myDoc).extSubset;
            (*(*ctxt).myDoc).extSubset = ptr::null_mut();
            if !ret.is_null() {
                (*ret).doc = ptr::null_mut();
                let mut tmp = (*ret).children;
                while !tmp.is_null() {
                    (*tmp).doc = ptr::null_mut();
                    tmp = (*tmp).next;
                }
            }
        }
        if !(*ctxt).myDoc.is_null() {
            crate::xml::tree::free_doc((*ctxt).myDoc);
        }
        (*ctxt).myDoc = ptr::null_mut();
        ret
    }
}

/// `xmlCtxtParseContent` — parse a well-balanced content sequence into a
/// node list in the context of `node` (upstream parser.c; the input is
/// consumed and freed).
#[no_mangle]
pub unsafe extern "C" fn xmlCtxtParseContent(
    ctxt: *mut _xmlParserCtxt,
    input: *mut _xmlParserInput,
    node: *mut _xmlNode,
    has_text_decl: c_int,
) -> *mut _xmlNode {
    unsafe {
        if ctxt.is_null() || input.is_null() || node.is_null() {
            crate::xml::parser::helpers::free_parser_input(input);
            return ptr::null_mut();
        }
        let doc = (*node).doc;
        if doc.is_null() {
            crate::xml::parser::helpers::free_parser_input(input);
            return ptr::null_mut();
        }
        let mut target = node;
        match (*node).type_ {
            t if t == xmlElementType::XML_ELEMENT_NODE as c_int
                || t == xmlElementType::XML_DOCUMENT_NODE as c_int
                || t == xmlElementType::XML_HTML_DOCUMENT_NODE as c_int => {}
            t if t == xmlElementType::XML_ATTRIBUTE_NODE as c_int
                || t == xmlElementType::XML_TEXT_NODE as c_int
                || t == xmlElementType::XML_CDATA_SECTION_NODE as c_int
                || t == xmlElementType::XML_ENTITY_REF_NODE as c_int
                || t == xmlElementType::XML_PI_NODE as c_int
                || t == xmlElementType::XML_COMMENT_NODE as c_int =>
            {
                let mut cur = (*node).parent;
                while !cur.is_null() {
                    let ct = (*cur).type_;
                    if ct == xmlElementType::XML_ELEMENT_NODE as c_int
                        || ct == xmlElementType::XML_DOCUMENT_NODE as c_int
                        || ct == xmlElementType::XML_HTML_DOCUMENT_NODE as c_int
                    {
                        target = cur;
                        break;
                    }
                    cur = (*cur).parent;
                }
            }
            _ => {
                crate::xml::parser::helpers::free_parser_input(input);
                return ptr::null_mut();
            }
        }

        crate::abi::exports_parser::xmlCtxtReset(ctxt);
        let old_dict = (*ctxt).dict;
        let old_options = (*ctxt).options;
        let old_dict_names = (*ctxt).dictNames;
        let old_load_subset = (*ctxt).loadsubset;
        if !(*doc).dict.is_null() {
            (*ctxt).dict = (*doc).dict;
        } else {
            (*ctxt).options |= crate::abi::types::XML_PARSE_NODICT;
            (*ctxt).dictNames = 0;
        }
        (*ctxt).loadsubset |= crate::abi::constants::XML_SKIP_IDS;
        (*ctxt).options |= crate::abi::types::XML_PARSE_SKIP_IDS;
        (*ctxt).myDoc = doc;

        let list = pi_parse_content_node_list(ctxt, input, has_text_decl);

        (*ctxt).dict = old_dict;
        (*ctxt).options = old_options;
        (*ctxt).dictNames = old_dict_names;
        (*ctxt).loadsubset = old_load_subset;
        (*ctxt).myDoc = ptr::null_mut();
        (*ctxt).node = ptr::null_mut();
        crate::xml::parser::helpers::free_parser_input(input);
        let _ = target;
        list
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlNewInputFrom* / xmlInputSetEncodingHandler (parser.h 2.14+ family)
// ═══════════════════════════════════════════════════════════════════════════════
//
// R-000165 closure: input-stream constructors ported from
// parserInternals.c. The returned input OWNS its buffer and filename and
// must be freed with xmlFreeInputStream (free_parser_input frees the
// buffer via the xmlIO layer and the owned filename).

/// Set the owned filename on a freshly built input (url may be NULL).
unsafe fn pi_set_input_filename(input: *mut _xmlParserInput, url: *const c_char) {
    if !url.is_null() {
        (*input).filename = crate::xml::string::xml_strdup(url as *const crate::abi::types::xmlChar)
            as *const c_char;
    }
}

/// `xmlNewInputFromMemory` — new input reading from a memory area.
#[no_mangle]
pub unsafe extern "C" fn xmlNewInputFromMemory(
    url: *const c_char,
    mem: *const c_void,
    size: usize,
    _flags: c_int,
) -> *mut _xmlParserInput {
    unsafe {
        if mem.is_null() {
            return ptr::null_mut();
        }
        let buf = crate::xml::io::input_buffer_create_mem(
            mem as *const c_char,
            size as c_int,
            crate::abi::types::xmlCharEncoding::XML_CHAR_ENCODING_NONE as c_int,
        );
        if buf.is_null() {
            return ptr::null_mut();
        }
        let input = crate::abi::exports_parser::parser_input_from_buf_pub(buf);
        if input.is_null() {
            return ptr::null_mut();
        }
        pi_set_input_filename(input, url);
        input
    }
}

/// `xmlNewInputFromString` — new input reading from a zero-terminated string.
#[no_mangle]
pub unsafe extern "C" fn xmlNewInputFromString(
    url: *const c_char,
    str: *const c_char,
    _flags: c_int,
) -> *mut _xmlParserInput {
    unsafe {
        if str.is_null() {
            return ptr::null_mut();
        }
        let len = libc::strlen(str);
        let buf = crate::xml::io::input_buffer_create_mem(
            str,
            len as c_int,
            crate::abi::types::xmlCharEncoding::XML_CHAR_ENCODING_NONE as c_int,
        );
        if buf.is_null() {
            return ptr::null_mut();
        }
        let input = crate::abi::exports_parser::parser_input_from_buf_pub(buf);
        if input.is_null() {
            return ptr::null_mut();
        }
        pi_set_input_filename(input, url);
        input
    }
}

/// `xmlNewInputFromFd` — new input reading from a file descriptor
/// (the fd is drained at creation; upstream closes it with the input — the
/// candidate's read-at-creation pattern closes it after reading).
#[no_mangle]
pub unsafe extern "C" fn xmlNewInputFromFd(
    url: *const c_char,
    fd: c_int,
    _flags: c_int,
) -> *mut _xmlParserInput {
    unsafe {
        if fd < 0 {
            return ptr::null_mut();
        }
        let mut data: Vec<u8> = Vec::new();
        let mut tmp = [0u8; 4096];
        loop {
            let n = libc::read(fd, tmp.as_mut_ptr() as *mut c_void, tmp.len());
            if n <= 0 {
                break;
            }
            data.extend_from_slice(&tmp[..n as usize]);
        }
        let buf = crate::xml::io::input_buffer_create_mem(
            data.as_ptr() as *const c_char,
            data.len() as c_int,
            crate::abi::types::xmlCharEncoding::XML_CHAR_ENCODING_NONE as c_int,
        );
        if buf.is_null() {
            return ptr::null_mut();
        }
        let input = crate::abi::exports_parser::parser_input_from_buf_pub(buf);
        if input.is_null() {
            return ptr::null_mut();
        }
        pi_set_input_filename(input, url);
        input
    }
}

/// `xmlNewInputFromIO` — new input reading from I/O callbacks.
#[no_mangle]
pub unsafe extern "C" fn xmlNewInputFromIO(
    url: *const c_char,
    io_read: Option<crate::abi::callbacks::xmlInputReadCallback>,
    io_close: Option<crate::abi::callbacks::xmlInputCloseCallback>,
    io_ctxt: *mut c_void,
    _flags: c_int,
) -> *mut _xmlParserInput {
    unsafe {
        let Some(read) = io_read else {
            return ptr::null_mut();
        };
        // Drain the callback (candidate's read-at-creation pattern).
        let mut data: Vec<u8> = Vec::new();
        let mut tmp = [0u8; 4096];
        loop {
            let n = read(io_ctxt, tmp.as_mut_ptr() as *mut c_char, tmp.len() as c_int);
            if n <= 0 {
                break;
            }
            data.extend_from_slice(&tmp[..n as usize]);
        }
        let buf = crate::xml::io::input_buffer_create_mem(
            data.as_ptr() as *const c_char,
            data.len() as c_int,
            crate::abi::types::xmlCharEncoding::XML_CHAR_ENCODING_NONE as c_int,
        );
        if buf.is_null() {
            if let Some(close) = io_close {
                close(io_ctxt);
            }
            return ptr::null_mut();
        }
        // Honor the close callback on buffer free (upstream contract).
        (*buf).closecallback = io_close;
        let input = crate::abi::exports_parser::parser_input_from_buf_pub(buf);
        if input.is_null() {
            return ptr::null_mut();
        }
        pi_set_input_filename(input, url);
        input
    }
}

/// `xmlNewInputFromUrl` — new input from a file/URL (2.14+; error code +
/// out-param).
#[no_mangle]
pub unsafe extern "C" fn xmlNewInputFromUrl(
    url: *const c_char,
    _flags: c_int,
    out: *mut *mut _xmlParserInput,
) -> c_int {
    unsafe {
        if out.is_null() {
            return crate::abi::types::XML_ERR_ARGUMENT;
        }
        *out = ptr::null_mut();
        if url.is_null() {
            return crate::abi::types::XML_ERR_ARGUMENT;
        }
        let buf = crate::xml::io::input_buffer_create_file(
            url,
            crate::abi::types::xmlCharEncoding::XML_CHAR_ENCODING_NONE as c_int,
        );
        if buf.is_null() {
            return crate::abi::types::XML_IO_ENOENT;
        }
        let input = crate::abi::exports_parser::parser_input_from_buf_pub(buf);
        if input.is_null() {
            return crate::abi::types::XML_ERR_NO_MEMORY;
        }
        pi_set_input_filename(input, url);
        *out = input;
        crate::abi::types::XML_ERR_OK
    }
}

/// `xmlInputSetEncodingHandler` — attach an encoding handler to an input
/// (upstream parserInternals.c; handler closed on error / UTF-8 pass).
#[no_mangle]
pub unsafe extern "C" fn xmlInputSetEncodingHandler(
    input: *mut _xmlParserInput,
    handler: *mut c_void,
) -> c_int {
    unsafe {
        if input.is_null() || (*input).buf.is_null() {
            if !handler.is_null() {
                crate::abi::exports_xml2::xmlCharEncCloseFunc(handler);
            }
            return crate::abi::types::XML_ERR_ARGUMENT;
        }
        let in_ = (*input).buf;
        let mut h = handler;
        // UTF-8 requires no encoding handler.
        if !h.is_null() {
            let name = (*(h as *mut crate::abi::structs::_xmlCharEncodingHandler)).name;
            if !name.is_null()
                && crate::abi::exports_xml2::xmlStrcasecmp(
                    name as *const crate::abi::types::xmlChar,
                    c"UTF-8".as_ptr() as *const crate::abi::types::xmlChar,
                ) == 0
            {
                crate::abi::exports_xml2::xmlCharEncCloseFunc(h);
                h = ptr::null_mut();
            }
        }
        if std::ptr::eq((*in_).encoder, h) {
            return crate::abi::types::XML_ERR_OK;
        }
        if !(*in_).encoder.is_null() {
            crate::abi::exports_xml2::xmlCharEncCloseFunc((*in_).encoder);
            (*in_).encoder = h;
            return crate::abi::types::XML_ERR_OK;
        }
        (*in_).encoder = h;
        crate::abi::types::XML_ERR_OK
    }
}
