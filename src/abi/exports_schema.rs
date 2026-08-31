//! exports_schema — C ABI exports for the XML Schema family.
//!
//! Implements the public ABI of the upstream headers `xmlschemas.h`,
//! `xmlschemastypes.h` and `schematron.h` (family closure 11.1-I).
//!
//! The internal XSD engine lives in `src/xml/schemas` (`XsdSchema`,
//! `xsd_parse`, `xsd_validate`, `xsd_validate_datatype`). It powers
//! `xmllint --schema` and is oracle-verified, so every function here that
//! needs real schema behavior is wrapped around that engine. Datatype-value
//! machinery (`xmlSchemaVal`, `xmlSchemaFacet`, built-in `xmlSchemaType`
//! descriptors) does not exist in the internal engine and is provided here
//! as small `repr(C)` structs allocated through the crate allocator.
//!
//! # UPSTREAM-PARITY
//!
//! All 57 functions below follow the exact oracle signatures. Behavior notes:
//!
//! - Parser/validation context *state* (error callbacks, options, filename,
//!   locator) is stored in side registries keyed by context address, because
//!   the internal engine's `xmlSchemaNewParserCtxt`/`xmlSchemaNewValidCtxt`
//!   (defined in `src/xml/schemas/mod.rs`) box the schema/validation structs
//!   directly and cannot be extended without breaking that module's layout.
//! - `xmlSchemaNewDocParserCtxt` boxes a parsed `XsdSchema`, matching the
//!   internal engine's convention that `xmlSchemaParse` returns its context
//!   as the schema pointer (`schema == pctxt`).
//! - Functions requiring streaming/SAX interception the internal DOM engine
//!   cannot provide (`xmlSchemaSAXPlug`, `xmlSchemaValidateStream` without a
//!   readable input buffer) are simplified and documented inline.

#![allow(
    missing_docs,
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals
)]
#![allow(clippy::missing_safety_doc)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]
#![allow(missing_debug_implementations)]

use core::ffi::c_void;
use core::ptr;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_ulong};

use crate::abi::allocator::{xmlFreeImpl, xmlMemStrdupImpl};
use crate::abi::callbacks::{xmlStructuredErrorFunc, xmlValidityErrorFunc, xmlValidityWarningFunc};
use crate::abi::structs::{_xmlDoc, _xmlError, _xmlNode, _xmlParserInputBuffer, _xmlSAXHandler};
use crate::abi::types::{xmlChar, xmlCharEncoding, xmlErrorLevel, XML_FROM_SCHEMASV};
use crate::xml::schemas::{
    xsd_parse, xsd_validate, xsd_validate_datatype, xsd_validate_facet, XsdDatatypeKind, XsdSchema,
    XsdValidCtxt,
};
use crate::xml::schematron::schematron_parse;

// ═══════════════════════════════════════════════════════════════════════════════
// Opaque upstream types
// ═══════════════════════════════════════════════════════════════════════════════

// These mirror the forward-declared opaque structs in the upstream headers
// (`typedef struct _xmlSchema xmlSchema;` etc.). They are only ever used
// through pointers, so uninhabited enums are ABI-identical to the opaque
// C structs.

pub enum xmlSchema {}
pub enum xmlSchemaParserCtxt {}
pub enum xmlSchemaValidCtxt {}
pub enum xmlSchemaVal {}
pub enum xmlSchemaFacet {}
pub enum xmlSchemaType {}
pub enum xmlSchemaWildcard {}
pub enum xmlSchemaSAXPlugStruct {}
pub enum xmlSchematronParserCtxt {}
pub enum xmlSchematronValidCtxt {}

/// `int (*xmlSchemaValidityLocatorFunc)(void *ctx, const char **file, unsigned long *line);`
/// (xmlschemas.h)
pub type xmlSchemaValidityLocatorFunc =
    unsafe extern "C" fn(ctx: *mut c_void, file: *mut *const c_char, line: *mut c_ulong) -> c_int;

// ═══════════════════════════════════════════════════════════════════════════════
// Internal representations (allocated with Box / the crate allocator)
// ═══════════════════════════════════════════════════════════════════════════════

/// An `xmlSchemaVal` — a typed simple value.
///
/// `value`/`ns` are NUL-terminated heap strings (xmlMalloc'd). List values
/// (NMTOKENS, IDREFS, ENTITIES) are chained through `next`.
#[repr(C)]
struct XsdVal {
    val_type: c_int,     // xmlSchemaValType
    value: *mut xmlChar, // canonical/lexical value
    ns: *mut xmlChar,    // namespace URI for QName/NOTATION
    next: *mut XsdVal,   // next item for list values
}

/// An `xmlSchemaFacet` — a facet declaration.
#[repr(C)]
struct XsdFacet {
    facet_type: c_int, // xmlSchemaFacetType (1000..1011)
    value: *mut xmlChar,
    next: *mut XsdFacet,
}

/// A built-in `xmlSchemaType` descriptor (static, registry-owned).
#[repr(C)]
struct XsdType {
    val_type: c_int,         // xmlSchemaValType
    item_type: *mut XsdType, // item type for the built-in list types
    name: *const c_char,     // static type name ("string", "int", ...)
}

/// State kept for a parser context (side registry).
#[derive(Default, Clone, Copy)]
struct ParserState {
    err: Option<xmlValidityErrorFunc>,
    warn: Option<xmlValidityWarningFunc>,
    ctx: usize,
    serror: Option<xmlStructuredErrorFunc>,
    sctx: usize,
    resource_loader: Option<crate::abi::callbacks::xmlResourceLoader>,
    resource_ctxt: usize,
}

/// State kept for a validation context (side registry).
#[derive(Default, Clone, Copy)]
struct ValidState {
    err: Option<xmlValidityErrorFunc>,
    warn: Option<xmlValidityWarningFunc>,
    ctx: usize,
    serror: Option<xmlStructuredErrorFunc>,
    sctx: usize,
    options: c_int,
    filename: usize, // raw const char* (upstream stores the pointer, not a copy)
    locator: Option<xmlSchemaValidityLocatorFunc>,
    locator_ctx: usize,
}

/// State kept for a Schematron validation context (side registry).
#[derive(Default, Clone, Copy)]
struct SchematronValidState {
    serror: Option<xmlStructuredErrorFunc>,
    sctx: usize,
}

/// The SAX plug allocated by `xmlSchemaSAXPlug`.
#[repr(C)]
struct XsdSaxPlug {
    sax: *const _xmlSAXHandler,
    user_data: *mut c_void,
}

static PARSER_STATES: Lazy<Mutex<HashMap<usize, ParserState>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static VALID_STATES: Lazy<Mutex<HashMap<usize, ValidState>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static SCHEMATRON_VALID_STATES: Lazy<Mutex<HashMap<usize, SchematronValidState>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static TYPE_REGISTRY: Lazy<Mutex<HashMap<c_int, usize>>> = Lazy::new(|| Mutex::new(HashMap::new()));

// ═══════════════════════════════════════════════════════════════════════════════
// Constants (upstream values)
// ═══════════════════════════════════════════════════════════════════════════════

// xmlSchemaValType (schemasInternals.h)
const VAL_UNKNOWN: c_int = 0;
const VAL_STRING: c_int = 1;
const VAL_NORMSTRING: c_int = 2;
const VAL_DECIMAL: c_int = 3;
const VAL_TIME: c_int = 4;
const VAL_GDAY: c_int = 5;
const VAL_GMONTH: c_int = 6;
const VAL_GMONTHDAY: c_int = 7;
const VAL_GYEAR: c_int = 8;
const VAL_GYEARMONTH: c_int = 9;
const VAL_DATE: c_int = 10;
const VAL_DATETIME: c_int = 11;
const VAL_DURATION: c_int = 12;
const VAL_FLOAT: c_int = 13;
const VAL_DOUBLE: c_int = 14;
const VAL_BOOLEAN: c_int = 15;
const VAL_TOKEN: c_int = 16;
const VAL_LANGUAGE: c_int = 17;
const VAL_NMTOKEN: c_int = 18;
const VAL_NMTOKENS: c_int = 19;
const VAL_NAME: c_int = 20;
const VAL_QNAME: c_int = 21;
const VAL_NCNAME: c_int = 22;
const VAL_ID: c_int = 23;
const VAL_IDREF: c_int = 24;
const VAL_IDREFS: c_int = 25;
const VAL_ENTITY: c_int = 26;
const VAL_ENTITIES: c_int = 27;
const VAL_NOTATION: c_int = 28;
const VAL_ANYURI: c_int = 29;
const VAL_INTEGER: c_int = 30;
const VAL_NPINTEGER: c_int = 31;
const VAL_NINTEGER: c_int = 32;
const VAL_NNINTEGER: c_int = 33;
const VAL_PINTEGER: c_int = 34;
const VAL_INT: c_int = 35;
const VAL_UINT: c_int = 36;
const VAL_LONG: c_int = 37;
const VAL_ULONG: c_int = 38;
const VAL_SHORT: c_int = 39;
const VAL_USHORT: c_int = 40;
const VAL_BYTE: c_int = 41;
const VAL_UBYTE: c_int = 42;
const VAL_HEXBINARY: c_int = 43;
const VAL_BASE64BINARY: c_int = 44;
const VAL_ANYTYPE: c_int = 45;
const VAL_ANYSIMPLETYPE: c_int = 46;

// xmlSchemaFacetType (schemasInternals.h)
const FACET_MININCLUSIVE: c_int = 1000;
const FACET_MINEXCLUSIVE: c_int = 1001;
const FACET_MAXINCLUSIVE: c_int = 1002;
const FACET_MAXEXCLUSIVE: c_int = 1003;
const FACET_TOTALDIGITS: c_int = 1004;
const FACET_FRACTIONDIGITS: c_int = 1005;
const FACET_PATTERN: c_int = 1006;
const FACET_ENUMERATION: c_int = 1007;
const FACET_WHITESPACE: c_int = 1008;
const FACET_LENGTH: c_int = 1009;
const FACET_MAXLENGTH: c_int = 1010;
const FACET_MINLENGTH: c_int = 1011;

// xmlSchemaWhitespaceValueType (xmlschemastypes.h)
const WS_PRESERVE: c_int = 1;
const WS_REPLACE: c_int = 2;
const WS_COLLAPSE: c_int = 3;

// xmlSchemaValidOptions (xmlschemas.h)
const VAL_VC_I_CREATE: c_int = 1 << 0;
const VAL_XSI_ASSEMBLE: c_int = 1 << 1;
const VAL_OPTIONS_MASK: c_int = VAL_VC_I_CREATE | VAL_XSI_ASSEMBLE;

/// The W3C XML Schema namespace.
const XSD_NS: &[u8] = b"http://www.w3.org/2001/XMLSchema";

// ═══════════════════════════════════════════════════════════════════════════════
// Small helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Read a NUL-terminated C string as UTF-8 lossy.
///
/// # SAFETY
///
/// - `s` must be a valid NUL-terminated C string or NULL.
unsafe fn cstr_to_str(s: *const c_char) -> Option<String> {
    if s.is_null() {
        return None;
    }
    // SAFETY: Caller guarantees a valid NUL-terminated C string.
    let c = unsafe { CStr::from_ptr(s) };
    Some(String::from_utf8_lossy(c.to_bytes()).to_string())
}

/// Compare a C string against a byte slice (no trailing NUL in the slice).
///
/// # SAFETY
///
/// - `s` must be a valid NUL-terminated C string or NULL.
unsafe fn cstr_eq(s: *const c_char, bytes: &[u8]) -> bool {
    if s.is_null() {
        return false;
    }
    // SAFETY: Caller guarantees a valid NUL-terminated C string.
    unsafe {
        let mut i = 0usize;
        while i < bytes.len() {
            if *s.add(i) as u8 != bytes[i] {
                return false;
            }
            i += 1;
        }
        *s.add(i) == 0
    }
}

/// Duplicate a C string into heap memory owned by the caller.
///
/// Returns NULL when `s` is NULL.
unsafe fn dup_cstr(s: *const c_char) -> *mut c_char {
    if s.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: xmlMemStrdup requires a valid C string; caller guarantees it.
    unsafe { xmlMemStrdupImpl(s) as *mut c_char }
}

fn is_whitespace(c: char) -> bool {
    c == ' ' || c == '\t' || c == '\n' || c == '\r'
}

/// Replace whitespace characters with spaces (upstream `xmlSchemaWhiteSpaceReplace`).
fn whitespace_replace(s: &str) -> String {
    s.chars()
        .map(|c| if is_whitespace(c) { ' ' } else { c })
        .collect()
}

/// Collapse whitespace: trim and replace internal runs with single spaces
/// (upstream `xmlSchemaCollapseString`).
fn whitespace_collapse(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_run = false;
    for c in s.chars() {
        if is_whitespace(c) {
            in_run = true;
        } else {
            if in_run && !out.is_empty() {
                out.push(' ');
            }
            in_run = false;
            out.push(c);
        }
    }
    out
}

/// Apply one of the `xmlSchemaWhitespaceValueType` transformations.
fn apply_ws(s: &str, ws: c_int) -> String {
    match ws {
        WS_REPLACE => whitespace_replace(s),
        WS_COLLAPSE => whitespace_collapse(s),
        _ => s.to_string(),
    }
}

/// Map an `xmlSchemaValType` to the internal `XsdDatatypeKind`.
fn val_type_to_kind(val_type: c_int) -> Option<XsdDatatypeKind> {
    Some(match val_type {
        VAL_STRING | VAL_ANYTYPE | VAL_ANYSIMPLETYPE => XsdDatatypeKind::String,
        VAL_NORMSTRING => XsdDatatypeKind::NormalizedString,
        VAL_DECIMAL => XsdDatatypeKind::Decimal,
        VAL_TIME => XsdDatatypeKind::Time,
        VAL_GDAY => XsdDatatypeKind::GDay,
        VAL_GMONTH => XsdDatatypeKind::GMonth,
        VAL_GMONTHDAY => XsdDatatypeKind::GMonthDay,
        VAL_GYEAR => XsdDatatypeKind::GYear,
        VAL_GYEARMONTH => XsdDatatypeKind::GYearMonth,
        VAL_DATE => XsdDatatypeKind::Date,
        VAL_DATETIME => XsdDatatypeKind::DateTime,
        VAL_DURATION => XsdDatatypeKind::Duration,
        VAL_FLOAT => XsdDatatypeKind::Float,
        VAL_DOUBLE => XsdDatatypeKind::Double,
        VAL_BOOLEAN => XsdDatatypeKind::Boolean,
        VAL_TOKEN => XsdDatatypeKind::Token,
        VAL_LANGUAGE => XsdDatatypeKind::Language,
        VAL_NMTOKEN => XsdDatatypeKind::Nmtoken,
        VAL_NMTOKENS => XsdDatatypeKind::Nmtokens,
        VAL_NAME => XsdDatatypeKind::Name,
        VAL_QNAME => XsdDatatypeKind::QName,
        VAL_NCNAME => XsdDatatypeKind::NCName,
        VAL_ID => XsdDatatypeKind::Id,
        VAL_IDREF => XsdDatatypeKind::Idref,
        VAL_IDREFS => XsdDatatypeKind::Idrefs,
        VAL_ENTITY => XsdDatatypeKind::Entity,
        VAL_ENTITIES => XsdDatatypeKind::Entities,
        VAL_NOTATION => XsdDatatypeKind::Notation,
        VAL_ANYURI => XsdDatatypeKind::AnyURI,
        VAL_INTEGER => XsdDatatypeKind::Integer,
        VAL_NPINTEGER => XsdDatatypeKind::NonPositiveInteger,
        VAL_NINTEGER => XsdDatatypeKind::NegativeInteger,
        VAL_NNINTEGER => XsdDatatypeKind::NonNegativeInteger,
        VAL_PINTEGER => XsdDatatypeKind::PositiveInteger,
        VAL_INT => XsdDatatypeKind::Int,
        VAL_UINT => XsdDatatypeKind::UnsignedInt,
        VAL_LONG => XsdDatatypeKind::Long,
        VAL_ULONG => XsdDatatypeKind::UnsignedLong,
        VAL_SHORT => XsdDatatypeKind::Short,
        VAL_USHORT => XsdDatatypeKind::UnsignedShort,
        VAL_BYTE => XsdDatatypeKind::Byte,
        VAL_UBYTE => XsdDatatypeKind::UnsignedByte,
        VAL_HEXBINARY => XsdDatatypeKind::HexBinary,
        VAL_BASE64BINARY => XsdDatatypeKind::Base64Binary,
        _ => return None,
    })
}

/// Map an `xmlSchemaFacetType` to the internal facet kind.
fn facet_type_to_kind(facet_type: c_int) -> Option<XsdDatatypeKind> {
    Some(match facet_type {
        FACET_MININCLUSIVE => XsdDatatypeKind::FacetMinInclusive,
        FACET_MINEXCLUSIVE => XsdDatatypeKind::FacetMinExclusive,
        FACET_MAXINCLUSIVE => XsdDatatypeKind::FacetMaxInclusive,
        FACET_MAXEXCLUSIVE => XsdDatatypeKind::FacetMaxExclusive,
        FACET_TOTALDIGITS => XsdDatatypeKind::FacetTotalDigits,
        FACET_FRACTIONDIGITS => XsdDatatypeKind::FacetFractionDigits,
        FACET_PATTERN => XsdDatatypeKind::FacetPattern,
        FACET_ENUMERATION => XsdDatatypeKind::FacetEnumeration,
        FACET_WHITESPACE => XsdDatatypeKind::FacetWhiteSpace,
        FACET_LENGTH => XsdDatatypeKind::FacetLength,
        FACET_MAXLENGTH => XsdDatatypeKind::FacetMaxLength,
        FACET_MINLENGTH => XsdDatatypeKind::FacetMinLength,
        _ => return None,
    })
}

/// NUL-terminated static name for a built-in `xmlSchemaValType`.
fn type_name(val_type: c_int) -> Option<&'static [u8]> {
    Some(match val_type {
        VAL_STRING => b"string\0",
        VAL_NORMSTRING => b"normalizedString\0",
        VAL_DECIMAL => b"decimal\0",
        VAL_TIME => b"time\0",
        VAL_GDAY => b"gDay\0",
        VAL_GMONTH => b"gMonth\0",
        VAL_GMONTHDAY => b"gMonthDay\0",
        VAL_GYEAR => b"gYear\0",
        VAL_GYEARMONTH => b"gYearMonth\0",
        VAL_DATE => b"date\0",
        VAL_DATETIME => b"dateTime\0",
        VAL_DURATION => b"duration\0",
        VAL_FLOAT => b"float\0",
        VAL_DOUBLE => b"double\0",
        VAL_BOOLEAN => b"boolean\0",
        VAL_TOKEN => b"token\0",
        VAL_LANGUAGE => b"language\0",
        VAL_NMTOKEN => b"NMTOKEN\0",
        VAL_NMTOKENS => b"NMTOKENS\0",
        VAL_NAME => b"Name\0",
        VAL_QNAME => b"QName\0",
        VAL_NCNAME => b"NCName\0",
        VAL_ID => b"ID\0",
        VAL_IDREF => b"IDREF\0",
        VAL_IDREFS => b"IDREFS\0",
        VAL_ENTITY => b"ENTITY\0",
        VAL_ENTITIES => b"ENTITIES\0",
        VAL_NOTATION => b"NOTATION\0",
        VAL_ANYURI => b"anyURI\0",
        VAL_INTEGER => b"integer\0",
        VAL_NPINTEGER => b"nonPositiveInteger\0",
        VAL_NINTEGER => b"negativeInteger\0",
        VAL_NNINTEGER => b"nonNegativeInteger\0",
        VAL_PINTEGER => b"positiveInteger\0",
        VAL_INT => b"int\0",
        VAL_UINT => b"unsignedInt\0",
        VAL_LONG => b"long\0",
        VAL_ULONG => b"unsignedLong\0",
        VAL_SHORT => b"short\0",
        VAL_USHORT => b"unsignedShort\0",
        VAL_BYTE => b"byte\0",
        VAL_UBYTE => b"unsignedByte\0",
        VAL_HEXBINARY => b"hexBinary\0",
        VAL_BASE64BINARY => b"base64Binary\0",
        VAL_ANYTYPE => b"anyType\0",
        VAL_ANYSIMPLETYPE => b"anySimpleType\0",
        _ => return None,
    })
}

/// Whether an `xmlSchemaValType` is numeric (decimal / integer family / float / double).
fn is_numeric_type(val_type: c_int) -> bool {
    matches!(
        val_type,
        VAL_DECIMAL
            | VAL_FLOAT
            | VAL_DOUBLE
            | VAL_INTEGER
            | VAL_NPINTEGER
            | VAL_NINTEGER
            | VAL_NNINTEGER
            | VAL_PINTEGER
            | VAL_INT
            | VAL_UINT
            | VAL_LONG
            | VAL_ULONG
            | VAL_SHORT
            | VAL_USHORT
            | VAL_BYTE
            | VAL_UBYTE
    )
}

/// Whether an `xmlSchemaValType` is a built-in list type.
fn is_list_type(val_type: c_int) -> bool {
    matches!(val_type, VAL_NMTOKENS | VAL_IDREFS | VAL_ENTITIES)
}

/// The item type of a built-in list type (itself otherwise).
fn list_item_type(val_type: c_int) -> c_int {
    match val_type {
        VAL_NMTOKENS => VAL_NMTOKEN,
        VAL_IDREFS => VAL_IDREF,
        VAL_ENTITIES => VAL_ENTITY,
        other => other,
    }
}

/// Canonicalize a decimal string (no leading/trailing zeros, "-" only for
/// negative non-zero values).
fn canonical_decimal(s: &str) -> String {
    let mut s = s.trim();
    let neg = s.starts_with('-');
    if s.starts_with('+') || s.starts_with('-') {
        s = &s[1..];
    }
    let (int_part, frac_part) = match s.find('.') {
        Some(i) => (&s[..i], &s[i + 1..]),
        None => (s, ""),
    };
    let int_trimmed = int_part.trim_start_matches('0');
    let int_trimmed = if int_trimmed.is_empty() {
        "0"
    } else {
        int_trimmed
    };
    let frac_trimmed = frac_part.trim_end_matches('0');
    let mut out = String::new();
    if neg && !(int_trimmed == "0" && frac_trimmed.is_empty()) {
        out.push('-');
    }
    out.push_str(int_trimmed);
    if !frac_trimmed.is_empty() {
        out.push('.');
        out.push_str(frac_trimmed);
    }
    out
}

// ═══════════════════════════════════════════════════════════════════════════════
// Value allocation / ownership helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Create a value node. `value`/`ns` are duplicated into heap memory.
unsafe fn new_val(val_type: c_int, value: *const c_char, ns: *const c_char) -> *mut XsdVal {
    // SAFETY: dup_cstr requires valid C strings or NULL; caller guarantees it.
    unsafe {
        Box::into_raw(Box::new(XsdVal {
            val_type,
            value: dup_cstr(value) as *mut xmlChar,
            ns: dup_cstr(ns) as *mut xmlChar,
            next: ptr::null_mut(),
        }))
    }
}

/// Free a value chain (values + their strings).
unsafe fn free_val_chain(mut cur: *mut XsdVal) {
    while !cur.is_null() {
        let next = unsafe { (*cur).next };
        // SAFETY: value/ns were xmlMalloc'd by new_val; cur is a Box we own.
        unsafe {
            if !(*cur).value.is_null() {
                xmlFreeImpl((*cur).value as *mut c_void);
            }
            if !(*cur).ns.is_null() {
                xmlFreeImpl((*cur).ns as *mut c_void);
            }
            drop(Box::from_raw(cur));
        }
        cur = next;
    }
}

/// Create a value from a Rust string, duplicating into heap memory.
fn val_from_str(val_type: c_int, value: &str) -> *mut XsdVal {
    if let Ok(c) = CString::new(value) {
        // SAFETY: c is a valid C string for the duration of the call.
        unsafe { new_val(val_type, c.as_ptr(), ptr::null()) }
    } else {
        ptr::null_mut()
    }
}

/// Create a (possibly list) value from a C string. List types are split on
/// whitespace into an item chain, matching upstream's list-value layout.
unsafe fn new_string_value(val_type: c_int, value: *const c_char) -> *mut XsdVal {
    if value.is_null() {
        return ptr::null_mut();
    }
    let s = unsafe { cstr_to_str(value).unwrap_or_default() };
    if is_list_type(val_type) {
        let item_type = list_item_type(val_type);
        let tokens: Vec<&str> = s.split_whitespace().collect();
        if tokens.is_empty() {
            return ptr::null_mut();
        }
        let mut head: *mut XsdVal = ptr::null_mut();
        let mut tail: *mut XsdVal = ptr::null_mut();
        for tok in tokens {
            let item = val_from_str(item_type, tok);
            if item.is_null() {
                continue;
            }
            if head.is_null() {
                head = item;
            } else {
                // SAFETY: tail is a valid node from this loop.
                unsafe { (*tail).next = item };
            }
            tail = item;
        }
        head
    } else {
        val_from_str(val_type, &s)
    }
}

/// Build the built-in type descriptor for `val_type` (registry-owned, leaked).
fn builtin_type(val_type: c_int) -> *mut XsdType {
    if val_type <= VAL_UNKNOWN || val_type > VAL_ANYSIMPLETYPE {
        return ptr::null_mut();
    }
    {
        let reg = TYPE_REGISTRY.lock();
        if let Some(addr) = reg.get(&val_type) {
            return *addr as *mut XsdType;
        }
    }
    // Resolve the item type outside the lock to avoid re-entrancy.
    let item = if is_list_type(val_type) {
        builtin_type(list_item_type(val_type))
    } else {
        ptr::null_mut()
    };
    let name = type_name(val_type)
        .map(|b| b.as_ptr() as *const c_char)
        .unwrap_or(ptr::null());
    let t = Box::into_raw(Box::new(XsdType {
        val_type,
        item_type: item,
        name,
    }));
    let mut reg = TYPE_REGISTRY.lock();
    if let Some(existing) = reg.get(&val_type) {
        // Another thread won the race; drop our duplicate.
        // SAFETY: t is a Box we own and nobody else can see yet.
        unsafe { drop(Box::from_raw(t)) };
        return *existing as *mut XsdType;
    }
    reg.insert(val_type, t as usize);
    t
}

// ═══════════════════════════════════════════════════════════════════════════════
// Error dispatch helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Dispatch accumulated validation errors to the callbacks registered on a
/// validation context (both the plain and the structured handler).
///
/// # SAFETY
///
/// - `ctxt_addr` must be the address of an `XsdValidCtxt` that a caller
///   registered state for; otherwise this is a no-op.
unsafe fn dispatch_valid_errors(ctxt_addr: usize, errors: &[String]) {
    let state = {
        let guard = VALID_STATES.lock();
        guard.get(&ctxt_addr).copied()
    };
    let Some(state) = state else { return };
    if state.err.is_none() && state.serror.is_none() {
        return;
    }
    for msg in errors {
        let Ok(cmsg) = CString::new(msg.as_str()) else {
            continue;
        };
        if let Some(err) = state.err {
            // SAFETY: The caller supplied this callback in xmlSchemaSetValidErrors.
            unsafe { err(state.ctx as *mut c_void, cmsg.as_ptr()) };
        }
        if let Some(serror) = state.serror {
            // SAFETY: The caller supplied this callback in
            // xmlSchemaSetValidStructuredErrors.
            let mut e: _xmlError = unsafe { std::mem::zeroed() };
            e.domain = XML_FROM_SCHEMASV;
            e.code = 0;
            e.message = cmsg.as_ptr() as *mut c_char;
            e.level = xmlErrorLevel::XML_ERR_ERROR as c_int;
            e.file = state.filename as *mut c_char;
            e.line = 0;
            unsafe { serror(state.sctx as *mut c_void, &e) };
        }
    }
}

/// Dispatch a single error message through a parser context's callbacks.
///
/// # SAFETY
///
/// - `ctxt_addr` must be the address of a parser context that a caller
///   registered state for; otherwise this is a no-op.
unsafe fn dispatch_parser_error(ctxt_addr: usize, msg: &str) {
    let state = {
        let guard = PARSER_STATES.lock();
        guard.get(&ctxt_addr).copied()
    };
    let Some(state) = state else { return };
    if state.err.is_none() && state.serror.is_none() {
        return;
    }
    let Ok(cmsg) = CString::new(msg) else { return };
    if let Some(err) = state.err {
        // SAFETY: Caller-supplied callback from xmlSchemaSetParserErrors.
        unsafe { err(state.ctx as *mut c_void, cmsg.as_ptr()) };
    }
    if let Some(serror) = state.serror {
        // SAFETY: Caller-supplied callback from xmlSchemaSetParserStructuredErrors.
        let mut e: _xmlError = unsafe { std::mem::zeroed() };
        e.domain = XML_FROM_SCHEMASV;
        e.code = 0;
        e.message = cmsg.as_ptr() as *mut c_char;
        e.level = xmlErrorLevel::XML_ERR_ERROR as c_int;
        unsafe { serror(state.sctx as *mut c_void, &e) };
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Document serialization helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Serialize a document to a Rust string.
///
/// # SAFETY
///
/// - `doc` must be a valid `_xmlDoc` pointer.
unsafe fn doc_to_string(doc: *mut _xmlDoc) -> Option<String> {
    if doc.is_null() {
        return None;
    }
    let mut mem: *mut xmlChar = ptr::null_mut();
    let mut size: c_int = 0;
    // SAFETY: doc is valid; mem/size are writable locals.
    unsafe { crate::xml::tree::xmlDocDumpFormatMemory(doc, &mut mem, &mut size, 0) };
    if mem.is_null() {
        return None;
    }
    // SAFETY: mem/size describe the dumped buffer.
    let slice = unsafe { std::slice::from_raw_parts(mem as *const u8, size as usize) };
    let out = String::from_utf8_lossy(slice).to_string();
    // SAFETY: mem was allocated by the dumper; xmlFree is the matching free.
    unsafe { xmlFreeImpl(mem as *mut c_void) };
    Some(out)
}

/// Serialize a single element (subtree) to a Rust string.
///
/// # SAFETY
///
/// - `node` must be a valid `_xmlNode` pointer.
unsafe fn node_to_string(node: *mut _xmlNode) -> Option<String> {
    if node.is_null() {
        return None;
    }
    // xmlBufferCreate returns a valid buffer or NULL.
    let buf = crate::abi::exports_xml2::xmlBufferCreate();
    if buf.is_null() {
        return None;
    }
    // SAFETY: node and buf are valid; node->doc may be NULL, which the dumper tolerates.
    unsafe { crate::xml::tree::xmlNodeDump(buf, (*node).doc, node, 0, 0) };
    // buf is valid; content/length are readable fields.
    let content = crate::abi::exports_xml2::xmlBufferContent(buf);
    let len = crate::abi::exports_xml2::xmlBufferLength(buf);
    if content.is_null() || len <= 0 {
        // buf was created by xmlBufferCreate; xmlBufferFree matches.
        crate::abi::exports_xml2::xmlBufferFree(buf);
        return None;
    }
    // SAFETY: content/len describe the dumped element bytes.
    let slice = unsafe { std::slice::from_raw_parts(content as *const u8, len as usize) };
    let out = String::from_utf8_lossy(slice).to_string();
    // buf was created by xmlBufferCreate; xmlBufferFree matches.
    crate::abi::exports_xml2::xmlBufferFree(buf);
    Some(out)
}

/// Reset the accumulated error state of a validation context.
///
/// # SAFETY
///
/// - `ctxt` must be a valid `XsdValidCtxt` pointer.
unsafe fn reset_valid_ctxt(ctxt: *mut xmlSchemaValidCtxt) {
    // SAFETY: ctxt is the internal XsdValidCtxt box.
    let vc = unsafe { &mut *(ctxt as *mut XsdValidCtxt) };
    vc.errors.clear();
    vc.nb_errors = 0;
}

/// Parse an XML string into a doc and validate it with the internal engine,
/// dispatching any errors through the context's callbacks.
///
/// Returns the number of validation errors, or -1 on internal error.
///
/// # SAFETY
///
/// - `ctxt` must be a valid `XsdValidCtxt` (as created by the internal
///   `xmlSchemaNewValidCtxt`).
unsafe fn validate_doc_string(ctxt: *mut xmlSchemaValidCtxt, xml: &str) -> c_int {
    if ctxt.is_null() {
        return -1;
    }
    // Reset so xmlSchemaIsValid reflects this run even when the internal
    // engine only records errors on failure.
    unsafe { reset_valid_ctxt(ctxt) };
    // SAFETY: xmlReadMemory reads `xml` for `len` bytes; the pointer is valid.
    let doc = unsafe {
        crate::abi::exports_xml2::xmlReadMemory(
            xml.as_ptr() as *const c_char,
            xml.len() as c_int,
            b"doc.xml\0".as_ptr() as *const c_char,
            ptr::null(),
            0,
        )
    };
    if doc.is_null() {
        // Mirror upstream xmlSchemaValidateFile: report and bail out with -1.
        unsafe {
            dispatch_valid_errors(ctxt as usize, &["Document is not well-formed".to_string()]);
        }
        return -1;
    }
    // SAFETY: ctxt is a valid XsdValidCtxt; doc is a valid _xmlDoc.
    let ret = unsafe { crate::xml::schemas::xmlSchemaValidateDoc(ctxt as *mut c_void, doc) };
    // SAFETY: doc was created by xmlReadMemory; xmlFreeDoc matches.
    unsafe { crate::abi::exports_xml2::xmlFreeDoc(doc) };
    if ret != 0 {
        // SAFETY: ctxt is the internal XsdValidCtxt whose errors were filled
        // by xmlSchemaValidateDoc on failure.
        let errors = unsafe { (*(ctxt as *mut XsdValidCtxt)).errors.clone() };
        unsafe { dispatch_valid_errors(ctxt as usize, &errors) };
    }
    ret
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlschemastypes.h — initialization / built-in types
// ═══════════════════════════════════════════════════════════════════════════════

/// Initialize the XML Schema datatype machinery.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlSchemaInitTypes(void);
/// ```
///
/// Returns 0 on success. The internal engine initializes lazily; this
/// eagerly creates the built-in type descriptors so subsequent
/// `xmlSchemaGetBuiltInType` calls never fail.
#[no_mangle]
pub extern "C" fn xmlSchemaInitTypes() -> c_int {
    // Force creation of the full built-in type table.
    for t in VAL_STRING..=VAL_ANYSIMPLETYPE {
        builtin_type(t);
    }
    0
}

/// Clean up the XML Schema datatype machinery.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlSchemaCleanupTypes(void);
/// ```
///
/// Upstream frees its static tables at shutdown. The built-in descriptors
/// here are intentionally leaked statics (their addresses are handed out to
/// callers and must remain valid), so this is a no-op for ABI compatibility.
#[no_mangle]
pub extern "C" fn xmlSchemaCleanupTypes() {
    // No-op: built-in type descriptors are process-lifetime statics.
}

/// Look up a predefined (built-in) type by name and namespace.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlSchemaType * xmlSchemaGetPredefinedType(const xmlChar *name, const xmlChar *ns);
/// ```
///
/// Matches upstream: both `name` and `ns` must be non-NULL and match the
/// built-in type's name and the XML Schema namespace
/// (`http://www.w3.org/2001/XMLSchema`).
///
/// # SAFETY
///
/// - `name`/`ns` must be valid NUL-terminated C strings or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaGetPredefinedType(
    name: *const xmlChar,
    ns: *const xmlChar,
) -> *mut xmlSchemaType {
    if name.is_null() || ns.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: Caller guarantees valid C strings.
    let ns_str = unsafe { cstr_to_str(ns as *const c_char) }.unwrap_or_default();
    if ns_str.as_bytes() != XSD_NS {
        return ptr::null_mut();
    }
    for val_type in VAL_STRING..=VAL_ANYSIMPLETYPE {
        // SAFETY: type_name returns static NUL-terminated bytes.
        if let Some(bytes) = type_name(val_type) {
            // SAFETY: caller guarantees a valid C string.
            if unsafe { cstr_eq(name as *const c_char, &bytes[..bytes.len() - 1]) } {
                return builtin_type(val_type) as *mut xmlSchemaType;
            }
        }
    }
    ptr::null_mut()
}

/// Get the built-in type descriptor for an `xmlSchemaValType`.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlSchemaType * xmlSchemaGetBuiltInType(xmlSchemaValType type);
/// ```
///
/// Returns NULL for `XML_SCHEMAS_UNKNOWN` and out-of-range values.
#[no_mangle]
pub extern "C" fn xmlSchemaGetBuiltInType(type_: c_int) -> *mut xmlSchemaType {
    builtin_type(type_) as *mut xmlSchemaType
}

/// For a built-in list type, return its item type.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlSchemaType * xmlSchemaGetBuiltInListSimpleTypeItemType(xmlSchemaType *type);
/// ```
///
/// Returns NULL for non-list types. Implemented via the built-in descriptor
/// table (NMTOKENS→NMTOKEN, IDREFS→IDREF, ENTITIES→ENTITY).
///
/// # SAFETY
///
/// - `type_` must be a descriptor returned by `xmlSchemaGetBuiltInType`/
///   `xmlSchemaGetPredefinedType`, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaGetBuiltInListSimpleTypeItemType(
    type_: *mut xmlSchemaType,
) -> *mut xmlSchemaType {
    if type_.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: type_ is one of our descriptor pointers.
    unsafe { (*(type_ as *mut XsdType)).item_type as *mut xmlSchemaType }
}

/// Whether `facetType` is a valid facet for the built-in type `type_`.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlSchemaIsBuiltInTypeFacet(xmlSchemaType *type, int facetType);
/// ```
///
/// Returns 1 if valid, 0 otherwise. The check is coarse compared to
/// upstream's per-type tables: string-family types accept the length/pattern/
/// enumeration/whitespace facets, numeric types accept the bound/digit facets,
/// and all types accept pattern/enumeration/whitespace.
///
/// # SAFETY
///
/// - `type_` must be a descriptor returned by `xmlSchemaGetBuiltInType`/
///   `xmlSchemaGetPredefinedType`, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaIsBuiltInTypeFacet(
    type_: *mut xmlSchemaType,
    facetType: c_int,
) -> c_int {
    if type_.is_null() {
        return 0;
    }
    if facetType < FACET_MININCLUSIVE || facetType > FACET_MINLENGTH {
        return 0;
    }
    // SAFETY: type_ is one of our descriptor pointers.
    let val_type = unsafe { (*(type_ as *mut XsdType)).val_type };
    let length_facets = matches!(
        facetType,
        FACET_LENGTH
            | FACET_MINLENGTH
            | FACET_MAXLENGTH
            | FACET_PATTERN
            | FACET_ENUMERATION
            | FACET_WHITESPACE
    );
    let numeric_facets = matches!(
        facetType,
        FACET_MININCLUSIVE
            | FACET_MINEXCLUSIVE
            | FACET_MAXINCLUSIVE
            | FACET_MAXEXCLUSIVE
            | FACET_TOTALDIGITS
            | FACET_FRACTIONDIGITS
            | FACET_PATTERN
            | FACET_ENUMERATION
            | FACET_WHITESPACE
    );
    let universal = matches!(
        facetType,
        FACET_PATTERN | FACET_ENUMERATION | FACET_WHITESPACE
    );
    if is_numeric_type(val_type) {
        if numeric_facets {
            1
        } else {
            0
        }
    } else if matches!(val_type, VAL_STRING | VAL_NORMSTRING | VAL_TOKEN) || is_list_type(val_type)
    {
        if length_facets {
            1
        } else {
            0
        }
    } else if universal {
        1
    } else {
        0
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlschemastypes.h — string whitespace helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Replace whitespace characters with spaces and return a new string.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar * xmlSchemaWhiteSpaceReplace(const xmlChar *value);
/// ```
///
/// The result is xmlMalloc'd; the caller must free it with `xmlFree`.
///
/// # SAFETY
///
/// - `value` must be a valid NUL-terminated C string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaWhiteSpaceReplace(value: *const xmlChar) -> *mut xmlChar {
    if value.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: Caller guarantees a valid C string.
    let s = unsafe { cstr_to_str(value as *const c_char) }.unwrap_or_default();
    let out = whitespace_replace(&s);
    if let Ok(c) = CString::new(out) {
        // SAFETY: dup_cstr copies the string into xmlMalloc'd memory.
        unsafe { dup_cstr(c.as_ptr()) as *mut xmlChar }
    } else {
        ptr::null_mut()
    }
}

/// Collapse whitespace and return a new string.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar * xmlSchemaCollapseString(const xmlChar *value);
/// ```
///
/// The result is xmlMalloc'd; the caller must free it with `xmlFree`.
///
/// # SAFETY
///
/// - `value` must be a valid NUL-terminated C string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaCollapseString(value: *const xmlChar) -> *mut xmlChar {
    if value.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: Caller guarantees a valid C string.
    let s = unsafe { cstr_to_str(value as *const c_char) }.unwrap_or_default();
    let out = whitespace_collapse(&s);
    if let Ok(c) = CString::new(out) {
        // SAFETY: dup_cstr copies the string into xmlMalloc'd memory.
        unsafe { dup_cstr(c.as_ptr()) as *mut xmlChar }
    } else {
        ptr::null_mut()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlschemastypes.h — value objects
// ═══════════════════════════════════════════════════════════════════════════════

/// Create a simple typed value.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlSchemaVal * xmlSchemaNewStringValue(xmlSchemaValType type, const xmlChar *value);
/// ```
///
/// For the built-in list types (NMTOKENS, IDREFS, ENTITIES) the value is
/// split on whitespace into an item chain, matching upstream's layout.
///
/// # SAFETY
///
/// - `value` must be a valid NUL-terminated C string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaNewStringValue(
    type_: c_int,
    value: *const xmlChar,
) -> *mut xmlSchemaVal {
    // SAFETY: Delegates to new_string_value with the same contract.
    unsafe { new_string_value(type_, value as *const c_char) as *mut xmlSchemaVal }
}

/// Create a NOTATION value.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlSchemaVal * xmlSchemaNewNOTATIONValue(const xmlChar *name, const xmlChar *ns);
/// ```
///
/// # SAFETY
///
/// - `name`/`ns` must be valid NUL-terminated C strings or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaNewNOTATIONValue(
    name: *const xmlChar,
    ns: *const xmlChar,
) -> *mut xmlSchemaVal {
    // SAFETY: Delegates to new_val with the same contract.
    unsafe {
        new_val(VAL_NOTATION, name as *const c_char, ns as *const c_char) as *mut xmlSchemaVal
    }
}

/// Create a QName value.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlSchemaVal * xmlSchemaNewQNameValue(const xmlChar *namespaceName, const xmlChar *localName);
/// ```
///
/// # SAFETY
///
/// - `namespaceName`/`localName` must be valid NUL-terminated C strings or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaNewQNameValue(
    namespaceName: *const xmlChar,
    localName: *const xmlChar,
) -> *mut xmlSchemaVal {
    // SAFETY: Delegates to new_val with the same contract.
    unsafe {
        new_val(
            VAL_QNAME,
            localName as *const c_char,
            namespaceName as *const c_char,
        ) as *mut xmlSchemaVal
    }
}

/// Deep-copy a value (including list chains).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlSchemaVal * xmlSchemaCopyValue(xmlSchemaVal *val);
/// ```
///
/// # SAFETY
///
/// - `val` must be a value created by this module or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaCopyValue(val: *mut xmlSchemaVal) -> *mut xmlSchemaVal {
    if val.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: val is one of our XsdVal nodes.
    let mut src = val as *mut XsdVal;
    let mut head: *mut XsdVal = ptr::null_mut();
    let mut tail: *mut XsdVal = ptr::null_mut();
    while !src.is_null() {
        // SAFETY: each node is a valid XsdVal owned by this module.
        let (vtype, vvalue, vns) = unsafe { ((*src).val_type, (*src).value, (*src).ns) };
        let node = unsafe { new_val(vtype, vvalue as *const c_char, vns as *const c_char) };
        if node.is_null() {
            // Out of memory: free the partial chain.
            unsafe { free_val_chain(head) };
            return ptr::null_mut();
        }
        if head.is_null() {
            head = node;
        } else {
            // SAFETY: tail is a valid node from this loop.
            unsafe { (*tail).next = node };
        }
        tail = node;
        // SAFETY: src is a valid node from the chain.
        src = unsafe { (*src).next };
    }
    head as *mut xmlSchemaVal
}

/// Free a value (and any list items).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlSchemaFreeValue(xmlSchemaVal *val);
/// ```
///
/// # SAFETY
///
/// - `val` must be a value created by this module (or NULL).
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaFreeValue(val: *mut xmlSchemaVal) {
    if val.is_null() {
        return;
    }
    // SAFETY: val is one of our XsdVal chains.
    unsafe { free_val_chain(val as *mut XsdVal) };
}

/// Get the `xmlSchemaValType` of a value.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlSchemaValType xmlSchemaGetValType(xmlSchemaVal *val);
/// ```
///
/// # SAFETY
///
/// - `val` must be a value created by this module or NULL (returns 0).
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaGetValType(val: *mut xmlSchemaVal) -> c_int {
    if val.is_null() {
        return VAL_UNKNOWN;
    }
    // SAFETY: val is one of our XsdVal nodes.
    unsafe { (*(val as *mut XsdVal)).val_type }
}

/// Return the next item of a list value.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlSchemaVal * xmlSchemaValueGetNext(xmlSchemaVal *cur);
/// ```
///
/// # SAFETY
///
/// - `cur` must be a value created by this module or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaValueGetNext(cur: *mut xmlSchemaVal) -> *mut xmlSchemaVal {
    if cur.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: cur is one of our XsdVal nodes.
    unsafe { (*(cur as *mut XsdVal)).next as *mut xmlSchemaVal }
}

/// Return the string representation of a value.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// const xmlChar * xmlSchemaValueGetAsString(xmlSchemaVal *val);
/// ```
///
/// The returned pointer is owned by the value and stays valid until the
/// value is freed. Returns NULL for a NULL argument.
///
/// # SAFETY
///
/// - `val` must be a value created by this module or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaValueGetAsString(val: *mut xmlSchemaVal) -> *const xmlChar {
    if val.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: val is one of our XsdVal nodes; value is a valid C string or NULL.
    unsafe { (*(val as *mut XsdVal)).value as *const xmlChar }
}

/// Return a boolean value as 1/0; -1 if the value is not boolean.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlSchemaValueGetAsBoolean(xmlSchemaVal *val);
/// ```
///
/// # SAFETY
///
/// - `val` must be a value created by this module or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaValueGetAsBoolean(val: *mut xmlSchemaVal) -> c_int {
    if val.is_null() {
        return -1;
    }
    // SAFETY: val is one of our XsdVal nodes.
    let v = unsafe { &*(val as *mut XsdVal) };
    if v.val_type != VAL_BOOLEAN {
        return -1;
    }
    // SAFETY: v.value is a valid C string or NULL.
    let s = unsafe { cstr_to_str(v.value as *const c_char) }.unwrap_or_default();
    match s.as_str() {
        "true" | "1" => 1,
        "false" | "0" => 0,
        _ => -1,
    }
}

/// Append `cur` to the list value `prev` (which may itself be a chain).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlSchemaValueAppend(xmlSchemaVal *prev, xmlSchemaVal *cur);
/// ```
///
/// Returns 0 on success, -1 on error (NULL argument).
///
/// # SAFETY
///
/// - `prev`/`cur` must be values created by this module.
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaValueAppend(
    prev: *mut xmlSchemaVal,
    cur: *mut xmlSchemaVal,
) -> c_int {
    if prev.is_null() || cur.is_null() {
        return -1;
    }
    // SAFETY: prev is one of our XsdVal chains.
    let mut tail = prev as *mut XsdVal;
    // SAFETY: walking the chain; all nodes are ours.
    while !unsafe { (*tail).next }.is_null() {
        tail = unsafe { (*tail).next };
    }
    // SAFETY: tail is the last node of our chain; cur is ours.
    unsafe { (*tail).next = cur as *mut XsdVal };
    0
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlschemastypes.h — canonical values
// ═══════════════════════════════════════════════════════════════════════════════

/// Compute the canonical representation of a value.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlSchemaGetCanonValue(xmlSchemaVal *val, const xmlChar **retValue);
/// ```
///
/// Returns 0 on success and stores an xmlMalloc'd string in `*retValue`
/// (caller frees with `xmlFree`); -1 on error.
///
/// # SAFETY
///
/// - `val` must be a value created by this module; `retValue` must be a
///   writable non-NULL pointer.
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaGetCanonValue(
    val: *mut xmlSchemaVal,
    retValue: *mut *const xmlChar,
) -> c_int {
    // SAFETY: Delegates with the same contract.
    unsafe { xmlSchemaGetCanonValueWhtsp(val, retValue, WS_COLLAPSE) }
}

/// Compute the canonical representation of a value, applying a whitespace
/// transformation first.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlSchemaGetCanonValueWhtsp(xmlSchemaVal *val, const xmlChar **retValue,
///                                 xmlSchemaWhitespaceValueType ws);
/// ```
///
/// Returns 0 on success and stores an xmlMalloc'd string in `*retValue`
/// (caller frees with `xmlFree`); -1 on error.
///
/// # SAFETY
///
/// - `val` must be a value created by this module; `retValue` must be a
///   writable non-NULL pointer.
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaGetCanonValueWhtsp(
    val: *mut xmlSchemaVal,
    retValue: *mut *const xmlChar,
    ws: c_int,
) -> c_int {
    if val.is_null() || retValue.is_null() {
        return -1;
    }
    // SAFETY: val is one of our XsdVal nodes.
    let v = unsafe { &*(val as *mut XsdVal) };
    // List values canonicalize to their items joined by single spaces.
    if v.next.is_null() && !is_list_type(v.val_type) {
        let s = unsafe { cstr_to_str(v.value as *const c_char) }.unwrap_or_default();
        let norm = apply_ws(&s, ws);
        let canon = match v.val_type {
            t if is_numeric_type(t) => {
                if matches!(t, VAL_FLOAT | VAL_DOUBLE) {
                    match norm.as_str() {
                        "NaN" => "NaN".to_string(),
                        "INF" => "INF".to_string(),
                        "-INF" => "-INF".to_string(),
                        _ => match norm.parse::<f64>() {
                            Ok(f) if f.is_nan() => "NaN".to_string(),
                            Ok(f) if f.is_infinite() && f.is_sign_negative() => "-INF".to_string(),
                            Ok(f) if f.is_infinite() => "INF".to_string(),
                            Ok(f) => f.to_string(),
                            Err(_) => canonical_decimal(&norm),
                        },
                    }
                } else if matches!(t, VAL_DECIMAL) {
                    canonical_decimal(&norm)
                } else {
                    // Integer family: parse as i128 when possible.
                    match norm.parse::<i128>() {
                        Ok(i) => i.to_string(),
                        Err(_) => canonical_decimal(&norm),
                    }
                }
            }
            VAL_BOOLEAN => match norm.as_str() {
                "true" | "1" => "true".to_string(),
                "false" | "0" => "false".to_string(),
                _ => return -1,
            },
            _ => norm,
        };
        let Ok(c) = CString::new(canon) else {
            return -1;
        };
        // SAFETY: dup_cstr copies into xmlMalloc'd memory; caller frees.
        let out = unsafe { dup_cstr(c.as_ptr()) } as *const xmlChar;
        if out.is_null() {
            return -1;
        }
        // SAFETY: retValue is caller-guaranteed writable.
        unsafe { *retValue = out };
        0
    } else {
        // List value: canonicalize each item and join with single spaces.
        let mut parts: Vec<String> = Vec::new();
        let mut cur = val as *mut XsdVal;
        while !cur.is_null() {
            // SAFETY: cur is a node of our chain.
            let s = unsafe { cstr_to_str((*cur).value as *const c_char) }.unwrap_or_default();
            parts.push(apply_ws(&s, ws));
            cur = unsafe { (*cur).next };
        }
        let Ok(c) = CString::new(parts.join(" ")) else {
            return -1;
        };
        // SAFETY: dup_cstr copies into xmlMalloc'd memory; caller frees.
        let out = unsafe { dup_cstr(c.as_ptr()) } as *const xmlChar;
        if out.is_null() {
            return -1;
        }
        // SAFETY: retValue is caller-guaranteed writable.
        unsafe { *retValue = out };
        0
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlschemastypes.h — facets
// ═══════════════════════════════════════════════════════════════════════════════

/// Create an empty facet.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlSchemaFacet * xmlSchemaNewFacet(void);
/// ```
///
/// The facet starts with type 0 and a NULL value; the value can be set
/// through `xmlSchemaGetFacetValueAsULong` consumers via the internal
/// representation. Returns NULL on allocation failure.
#[no_mangle]
pub extern "C" fn xmlSchemaNewFacet() -> *mut xmlSchemaFacet {
    // SAFETY: Box allocation is infallible modulo OOM abort.
    Box::into_raw(Box::new(XsdFacet {
        facet_type: 0,
        value: ptr::null_mut(),
        next: ptr::null_mut(),
    })) as *mut xmlSchemaFacet
}

/// Free a facet.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlSchemaFreeFacet(xmlSchemaFacet *facet);
/// ```
///
/// # SAFETY
///
/// - `facet` must be a facet created by this module or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaFreeFacet(facet: *mut xmlSchemaFacet) {
    if facet.is_null() {
        return;
    }
    // SAFETY: facet is one of our XsdFacet boxes.
    let f = unsafe { Box::from_raw(facet as *mut XsdFacet) };
    if !f.value.is_null() {
        // SAFETY: value was xmlMalloc'd by this module.
        unsafe { xmlFreeImpl(f.value as *mut c_void) };
    }
    drop(f);
}

/// Parse a facet's value as an unsigned long.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// unsigned long xmlSchemaGetFacetValueAsULong(xmlSchemaFacet *facet);
/// ```
///
/// Returns 0 when the facet is NULL or the value is not numeric.
///
/// # SAFETY
///
/// - `facet` must be a facet created by this module or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaGetFacetValueAsULong(facet: *mut xmlSchemaFacet) -> c_ulong {
    if facet.is_null() {
        return 0;
    }
    // SAFETY: facet is one of our XsdFacet boxes; value is a C string or NULL.
    let s = unsafe { cstr_to_str((*(facet as *mut XsdFacet)).value as *const c_char) }
        .unwrap_or_default();
    s.trim().parse::<u64>().unwrap_or(0) as c_ulong
}

/// Check a facet for validity against a type.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlSchemaCheckFacet(xmlSchemaFacet *facet, xmlSchemaType *typeDecl,
///                         xmlSchemaParserCtxt *ctxt, const xmlChar *name);
/// ```
///
/// Returns 0 if the facet is acceptable, -1 otherwise (reporting through the
/// parser context's error callbacks). Simplified: verifies the facet type is
/// known and valid for `typeDecl`; deeper value-level checks (upstream's
/// full facet construction) are handled by `xmlSchemaValidateFacet` at
/// validation time.
///
/// # SAFETY
///
/// - `facet`/`typeDecl`/`ctxt` must be objects created by this crate or NULL;
///   `name` must be a valid C string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaCheckFacet(
    facet: *mut xmlSchemaFacet,
    typeDecl: *mut xmlSchemaType,
    ctxt: *mut xmlSchemaParserCtxt,
    name: *const xmlChar,
) -> c_int {
    if facet.is_null() {
        return -1;
    }
    // SAFETY: facet is one of our XsdFacet boxes.
    let facet_type = unsafe { (*(facet as *mut XsdFacet)).facet_type };
    if facet_type < FACET_MININCLUSIVE || facet_type > FACET_MINLENGTH {
        unsafe {
            dispatch_parser_error(ctxt as usize, "Invalid facet type");
        }
        return -1;
    }
    if !typeDecl.is_null() {
        let valid = xmlSchemaIsBuiltInTypeFacet(typeDecl, facet_type);
        if valid == 0 {
            let label = unsafe { cstr_to_str(name as *const c_char) }
                .unwrap_or_else(|| "facet".to_string());
            unsafe {
                dispatch_parser_error(
                    ctxt as usize,
                    &format!("Facet '{}' is not valid for this type", label),
                );
            }
            return -1;
        }
    }
    0
}

/// Validate a value against a facet of a base type.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlSchemaValidateFacet(xmlSchemaType *base, xmlSchemaFacet *facet,
///                            const xmlChar *value, xmlSchemaVal *val);
/// ```
///
/// Returns 0 if the value satisfies the facet, -1 otherwise. Implemented via
/// the internal engine's `xsd_validate_facet`.
///
/// # SAFETY
///
/// - `base`/`facet`/`val` must be objects created by this crate or NULL;
///   `value` must be a valid C string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaValidateFacet(
    base: *mut xmlSchemaType,
    facet: *mut xmlSchemaFacet,
    value: *const xmlChar,
    val: *mut xmlSchemaVal,
) -> c_int {
    if facet.is_null() {
        return -1;
    }
    // SAFETY: facet is one of our boxes.
    let facet_type = unsafe { (*(facet as *mut XsdFacet)).facet_type };
    let Some(facet_kind) = facet_type_to_kind(facet_type) else {
        return -1;
    };
    // SAFETY: facet value is a C string or NULL.
    let facet_value = unsafe { cstr_to_str((*(facet as *mut XsdFacet)).value as *const c_char) }
        .unwrap_or_default();
    // Determine the base kind: from `base`, falling back to `val`, then string.
    let base_type = if !base.is_null() {
        // SAFETY: base is one of our descriptors.
        unsafe { (*(base as *mut XsdType)).val_type }
    } else if !val.is_null() {
        // SAFETY: val is one of our nodes.
        unsafe { (*(val as *mut XsdVal)).val_type }
    } else {
        VAL_STRING
    };
    let Some(kind) = val_type_to_kind(base_type) else {
        return -1;
    };
    // SAFETY: value is a caller-guaranteed C string or NULL.
    let raw = unsafe { cstr_to_str(value as *const c_char) }.unwrap_or_default();
    let norm = whitespace_collapse(&raw);
    if xsd_validate_facet(&kind, &norm, &facet_kind, &facet_value) {
        0
    } else {
        -1
    }
}

/// Validate a value against a facet with explicit whitespace handling.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlSchemaValidateFacetWhtsp(xmlSchemaFacet *facet,
///                                 xmlSchemaWhitespaceValueType fws,
///                                 xmlSchemaValType valType,
///                                 const xmlChar *value, xmlSchemaVal *val,
///                                 xmlSchemaWhitespaceValueType ws);
/// ```
///
/// Returns 0 if the value satisfies the facet, -1 otherwise.
///
/// # SAFETY
///
/// - `facet`/`val` must be objects created by this crate or NULL; `value`
///   must be a valid C string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaValidateFacetWhtsp(
    facet: *mut xmlSchemaFacet,
    _fws: c_int,
    valType: c_int,
    value: *const xmlChar,
    val: *mut xmlSchemaVal,
    ws: c_int,
) -> c_int {
    if facet.is_null() {
        return -1;
    }
    // SAFETY: facet is one of our boxes.
    let facet_type = unsafe { (*(facet as *mut XsdFacet)).facet_type };
    let Some(facet_kind) = facet_type_to_kind(facet_type) else {
        return -1;
    };
    // SAFETY: facet value is a C string or NULL.
    let facet_value = unsafe { cstr_to_str((*(facet as *mut XsdFacet)).value as *const c_char) }
        .unwrap_or_default();
    let kind = if val.is_null() {
        val_type_to_kind(valType)
    } else {
        // SAFETY: val is one of our nodes.
        val_type_to_kind(unsafe { (*(val as *mut XsdVal)).val_type })
    };
    let Some(kind) = kind else {
        return -1;
    };
    // SAFETY: value is a caller-guaranteed C string or NULL.
    let raw = unsafe { cstr_to_str(value as *const c_char) }.unwrap_or_default();
    let norm = apply_ws(&raw, ws);
    if xsd_validate_facet(&kind, &norm, &facet_kind, &facet_value) {
        0
    } else {
        -1
    }
}

/// Validate a length facet against a value, reporting the value's length.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlSchemaValidateLengthFacet(xmlSchemaType *type, xmlSchemaFacet *facet,
///                                  const xmlChar *value, xmlSchemaVal *val,
///                                  unsigned long *length);
/// ```
///
/// Returns 0 if the facet holds, -1 otherwise. `*length` receives the
/// value's length (characters for string types, items for list types).
///
/// # SAFETY
///
/// - `type`/`facet`/`val` must be objects created by this crate or NULL;
///   `value` must be a valid C string or NULL; `length` must be writable.
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaValidateLengthFacet(
    type_: *mut xmlSchemaType,
    facet: *mut xmlSchemaFacet,
    value: *const xmlChar,
    val: *mut xmlSchemaVal,
    length: *mut c_ulong,
) -> c_int {
    let val_type = if !type_.is_null() {
        // SAFETY: type_ is one of our descriptors.
        unsafe { (*(type_ as *mut XsdType)).val_type }
    } else if !val.is_null() {
        // SAFETY: val is one of our nodes.
        unsafe { (*(val as *mut XsdVal)).val_type }
    } else {
        VAL_STRING
    };
    // SAFETY: Delegates with the same contract.
    unsafe { xmlSchemaValidateLengthFacetWhtsp(facet, val_type, value, val, length, WS_PRESERVE) }
}

/// Validate a length facet with explicit whitespace handling.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlSchemaValidateLengthFacetWhtsp(xmlSchemaFacet *facet,
///                                       xmlSchemaValType valType,
///                                       const xmlChar *value, xmlSchemaVal *val,
///                                       unsigned long *length,
///                                       xmlSchemaWhitespaceValueType ws);
/// ```
///
/// Returns 0 if the facet holds, -1 otherwise. `*length` receives the
/// value's length (characters for string types, items for list types).
///
/// # SAFETY
///
/// - `facet`/`val` must be objects created by this crate or NULL; `value`
///   must be a valid C string or NULL; `length` must be writable.
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaValidateLengthFacetWhtsp(
    facet: *mut xmlSchemaFacet,
    valType: c_int,
    value: *const xmlChar,
    val: *mut xmlSchemaVal,
    length: *mut c_ulong,
    ws: c_int,
) -> c_int {
    if facet.is_null() {
        return -1;
    }
    // SAFETY: facet is one of our boxes.
    let facet_type = unsafe { (*(facet as *mut XsdFacet)).facet_type };
    let facet_ulong = xmlSchemaGetFacetValueAsULong(facet);
    let len: c_ulong = if is_list_type(valType) {
        if val.is_null() {
            0
        } else {
            // Count items in the chain.
            let mut count: c_ulong = 0;
            let mut cur = val as *mut XsdVal;
            while !cur.is_null() {
                count += 1;
                // SAFETY: cur is a node of our chain.
                cur = unsafe { (*cur).next };
            }
            count
        }
    } else {
        // SAFETY: value is a caller-guaranteed C string or NULL.
        let raw = unsafe { cstr_to_str(value as *const c_char) }.unwrap_or_default();
        let norm = apply_ws(&raw, ws);
        norm.chars().count() as c_ulong
    };
    if !length.is_null() {
        // SAFETY: caller-guaranteed writable.
        unsafe { *length = len as c_ulong };
    }
    let ok = match facet_type {
        FACET_LENGTH => len == facet_ulong,
        FACET_MINLENGTH => len >= facet_ulong,
        FACET_MAXLENGTH => len <= facet_ulong,
        _ => {
            // Not a length facet: upstream reports "not a length facet".
            return -1;
        }
    };
    if ok {
        0
    } else {
        -1
    }
}

/// Validate a list-type facet given the actual number of items.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlSchemaValidateListSimpleTypeFacet(xmlSchemaFacet *facet,
///                                          const xmlChar *value,
///                                          unsigned long actualLen,
///                                          unsigned long *expectedLen);
/// ```
///
/// For length facets, checks `actualLen` against the facet and stores the
/// facet's value in `*expectedLen`. Returns 0 if valid, -1 otherwise.
///
/// # SAFETY
///
/// - `facet` must be a facet created by this module or NULL; `value` must be
///   a valid C string or NULL; `expectedLen` must be writable.
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaValidateListSimpleTypeFacet(
    facet: *mut xmlSchemaFacet,
    _value: *const xmlChar,
    actualLen: c_ulong,
    expectedLen: *mut c_ulong,
) -> c_int {
    if facet.is_null() {
        return -1;
    }
    // SAFETY: facet is one of our boxes.
    let facet_type = unsafe { (*(facet as *mut XsdFacet)).facet_type };
    let facet_ulong = xmlSchemaGetFacetValueAsULong(facet);
    if !expectedLen.is_null() {
        // SAFETY: caller-guaranteed writable.
        unsafe { *expectedLen = facet_ulong };
    }
    let ok = match facet_type {
        FACET_LENGTH => actualLen == facet_ulong,
        FACET_MINLENGTH => actualLen >= facet_ulong,
        FACET_MAXLENGTH => actualLen <= facet_ulong,
        _ => return -1,
    };
    if ok {
        0
    } else {
        -1
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlschemastypes.h — predefined-type validation
// ═══════════════════════════════════════════════════════════════════════════════

/// Validate a value against a predefined (built-in) type.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlSchemaValidatePredefinedType(xmlSchemaType *type, const xmlChar *value,
///                                     xmlSchemaVal **val);
/// ```
///
/// Returns 0 if the value is valid, -1 otherwise. If `val` is non-NULL it
/// receives a newly created value on success (caller frees it).
///
/// # SAFETY
///
/// - `type` must be a built-in descriptor from this crate; `value` must be a
///   valid C string; `val` must be writable or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaValidatePredefinedType(
    type_: *mut xmlSchemaType,
    value: *const xmlChar,
    val: *mut *mut xmlSchemaVal,
) -> c_int {
    // SAFETY: Delegates with the same contract (node is unused here).
    unsafe { xmlSchemaValPredefTypeNode(type_, value, val, ptr::null_mut()) }
}

/// Validate a value against a predefined type, with whitespace normalization.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlSchemaValPredefTypeNode(xmlSchemaType *type, const xmlChar *value,
///                                xmlSchemaVal **val, xmlNode *node);
/// ```
///
/// Returns 0 if the value is valid, -1 otherwise. If `val` is non-NULL it
/// receives a newly created value on success (caller frees it). `node` is
/// accepted for signature parity and used for error reporting (ignored here;
/// the internal engine validates strings).
///
/// # SAFETY
///
/// - `type` must be a built-in descriptor from this crate; `value` must be a
///   valid C string; `val` must be writable or NULL; `node` may be NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaValPredefTypeNode(
    type_: *mut xmlSchemaType,
    value: *const xmlChar,
    val: *mut *mut xmlSchemaVal,
    _node: *mut _xmlNode,
) -> c_int {
    if type_.is_null() || value.is_null() {
        return -1;
    }
    // SAFETY: type_ is one of our descriptors.
    let val_type = unsafe { (*(type_ as *mut XsdType)).val_type };
    let Some(kind) = val_type_to_kind(val_type) else {
        return -1;
    };
    // SAFETY: value is a caller-guaranteed C string.
    let raw = unsafe { cstr_to_str(value as *const c_char) }.unwrap_or_default();
    // Whitespace normalization, as upstream does before validating.
    let norm = whitespace_collapse(&raw);
    if !xsd_validate_datatype(&kind, &norm, &[]) {
        return -1;
    }
    if !val.is_null() {
        // SAFETY: val is caller-guaranteed writable; new_string_value creates
        // a value with the same contract as xmlSchemaNewStringValue.
        *val = unsafe { new_string_value(val_type, value as *const c_char) } as *mut xmlSchemaVal;
    }
    0
}

/// Validate a value against a predefined type, without normalization.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlSchemaValPredefTypeNodeNoNorm(xmlSchemaType *type, const xmlChar *value,
///                                      xmlSchemaVal **val, xmlNode *node);
/// ```
///
/// Same as `xmlSchemaValPredefTypeNode` but the value is validated as-is
/// (no whitespace collapse). Returns 0 if valid, -1 otherwise.
///
/// # SAFETY
///
/// - `type` must be a built-in descriptor from this crate; `value` must be a
///   valid C string; `val` must be writable or NULL; `node` may be NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaValPredefTypeNodeNoNorm(
    type_: *mut xmlSchemaType,
    value: *const xmlChar,
    val: *mut *mut xmlSchemaVal,
    _node: *mut _xmlNode,
) -> c_int {
    if type_.is_null() || value.is_null() {
        return -1;
    }
    // SAFETY: type_ is one of our descriptors.
    let val_type = unsafe { (*(type_ as *mut XsdType)).val_type };
    let Some(kind) = val_type_to_kind(val_type) else {
        return -1;
    };
    // SAFETY: value is a caller-guaranteed C string.
    let raw = unsafe { cstr_to_str(value as *const c_char) }.unwrap_or_default();
    if !xsd_validate_datatype(&kind, &raw, &[]) {
        return -1;
    }
    if !val.is_null() {
        // SAFETY: val is caller-guaranteed writable.
        *val = unsafe { new_string_value(val_type, value as *const c_char) } as *mut xmlSchemaVal;
    }
    0
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlschemastypes.h — value comparison
// ═══════════════════════════════════════════════════════════════════════════════

/// Compare two schema values.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlSchemaCompareValues(xmlSchemaVal *x, xmlSchemaVal *y);
/// ```
///
/// Returns -2 on error, -1 if x < y, 0 if equal, 1 if x > y. Numeric values
/// are compared numerically, booleans as false < true, everything else as
/// (collapsed) strings.
///
/// # SAFETY
///
/// - `x`/`y` must be values created by this module or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaCompareValues(
    x: *mut xmlSchemaVal,
    y: *mut xmlSchemaVal,
) -> c_int {
    // SAFETY: Delegates with the same contract.
    unsafe { xmlSchemaCompareValuesWhtsp(x, WS_COLLAPSE, y, WS_COLLAPSE) }
}

/// Compare two schema values with explicit whitespace handling.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlSchemaCompareValuesWhtsp(xmlSchemaVal *x,
///                                 xmlSchemaWhitespaceValueType xws,
///                                 xmlSchemaVal *y,
///                                 xmlSchemaWhitespaceValueType yws);
/// ```
///
/// Returns -2 on error, -1 if x < y, 0 if equal, 1 if x > y.
///
/// # SAFETY
///
/// - `x`/`y` must be values created by this module or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaCompareValuesWhtsp(
    x: *mut xmlSchemaVal,
    xws: c_int,
    y: *mut xmlSchemaVal,
    yws: c_int,
) -> c_int {
    if x.is_null() || y.is_null() {
        return -2;
    }
    // SAFETY: x/y are our nodes.
    let (xt, xv, yt, yv) = unsafe {
        (
            (*(x as *mut XsdVal)).val_type,
            cstr_to_str((*(x as *mut XsdVal)).value as *const c_char).unwrap_or_default(),
            (*(y as *mut XsdVal)).val_type,
            cstr_to_str((*(y as *mut XsdVal)).value as *const c_char).unwrap_or_default(),
        )
    };
    let xs = apply_ws(&xv, xws);
    let ys = apply_ws(&yv, yws);
    if is_numeric_type(xt) && is_numeric_type(yt) {
        // Numeric comparison with INF/NaN handling.
        let to_f64 = |s: &str| -> Option<f64> {
            match s {
                "INF" => Some(f64::INFINITY),
                "-INF" => Some(f64::NEG_INFINITY),
                "NaN" => Some(f64::NAN),
                _ => s.parse::<f64>().ok(),
            }
        };
        match (to_f64(&xs), to_f64(&ys)) {
            (Some(a), Some(b)) => {
                if a.is_nan() || b.is_nan() {
                    return -2;
                }
                if a < b {
                    -1
                } else if a > b {
                    1
                } else {
                    0
                }
            }
            _ => -2,
        }
    } else if xt == VAL_BOOLEAN && yt == VAL_BOOLEAN {
        let b = |s: &str| -> Option<i32> {
            match s {
                "true" | "1" => Some(1),
                "false" | "0" => Some(0),
                _ => None,
            }
        };
        match (b(&xs), b(&ys)) {
            (Some(a), Some(c)) => a.cmp(&c) as c_int,
            _ => -2,
        }
    } else {
        match xs.cmp(&ys) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlschemastypes.h — free functions for types/wildcards
// ═══════════════════════════════════════════════════════════════════════════════

/// Free a schema type.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlSchemaFreeType(xmlSchemaType *type);
/// ```
///
/// Built-in type descriptors are process-lifetime statics owned by the
/// registry, so freeing one is a no-op. Foreign (non-registry) pointers are
/// dropped as this crate's descriptor boxes.
///
/// # SAFETY
///
/// - `type_` must be a pointer returned by this crate (or NULL).
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaFreeType(type_: *mut xmlSchemaType) {
    if type_.is_null() {
        return;
    }
    let addr = type_ as usize;
    let registered = {
        let reg = TYPE_REGISTRY.lock();
        reg.values().any(|&v| v == addr)
    };
    if registered {
        // Static descriptor: nothing to free.
        return;
    }
    // SAFETY: The pointer was not registry-owned, so it must be a Box we
    // created (defensive; callers should only pass our descriptors).
    unsafe { drop(Box::from_raw(type_ as *mut XsdType)) };
}

/// Free a schema wildcard.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlSchemaFreeWildcard(xmlSchemaWildcard *wildcard);
/// ```
///
/// This crate never allocates wildcard objects (the internal engine has no
/// wildcard representation), so there is nothing owned to free; kept as a
/// no-op for ABI compatibility.
///
/// # SAFETY
///
/// - `wildcard` must be NULL or a pointer from this crate.
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaFreeWildcard(_wildcard: *mut xmlSchemaWildcard) {
    // No-op: the internal engine does not allocate wildcard objects.
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlschemas.h — parser context
// ═══════════════════════════════════════════════════════════════════════════════

/// Create a schema parser context from an already-parsed document.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlSchemaParserCtxt * xmlSchemaNewDocParserCtxt(xmlDoc *doc);
/// ```
///
/// The document is serialized and compiled through the internal engine
/// (`xsd_parse`). Following the internal engine's convention,
/// `xmlSchemaParse` returns its context as the schema pointer, so the
/// returned context IS the compiled schema box. Returns NULL if `doc` is
/// NULL or the schema fails to compile.
///
/// # SAFETY
///
/// - `doc` must be a valid `_xmlDoc` pointer.
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaNewDocParserCtxt(doc: *mut _xmlDoc) -> *mut xmlSchemaParserCtxt {
    // SAFETY: doc_to_string requires a valid doc.
    let Some(xml) = (unsafe { doc_to_string(doc) }) else {
        return ptr::null_mut();
    };
    match xsd_parse(&xml) {
        Ok(schema) => Box::into_raw(Box::new(schema)) as *mut xmlSchemaParserCtxt,
        Err(_) => ptr::null_mut(),
    }
}

/// Set the error/warning callbacks of a parser context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlSchemaSetParserErrors(xmlSchemaParserCtxt *ctxt,
///                               xmlSchemaValidityErrorFunc err,
///                               xmlSchemaValidityWarningFunc warn, void *ctx);
/// ```
///
/// The callbacks are stored in a side registry keyed by the context address.
///
/// # SAFETY
///
/// - `ctxt` must be a parser context created by this crate; `err`/`warn`
///   must be valid callbacks or NULL; `ctx` may be NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaSetParserErrors(
    ctxt: *mut xmlSchemaParserCtxt,
    err: Option<xmlValidityErrorFunc>,
    warn: Option<xmlValidityWarningFunc>,
    ctx: *mut c_void,
) {
    if ctxt.is_null() {
        return;
    }
    let mut guard = PARSER_STATES.lock();
    let e = guard.entry(ctxt as usize).or_default();
    e.err = err;
    e.warn = warn;
    e.ctx = ctx as usize;
}

/// Set the structured error callback of a parser context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlSchemaSetParserStructuredErrors(xmlSchemaParserCtxt *ctxt,
///                                         xmlStructuredErrorFunc serror, void *ctx);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be a parser context created by this crate; `serror` must be
///   a valid callback or NULL; `ctx` may be NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaSetParserStructuredErrors(
    ctxt: *mut xmlSchemaParserCtxt,
    serror: Option<xmlStructuredErrorFunc>,
    ctx: *mut c_void,
) {
    if ctxt.is_null() {
        return;
    }
    let mut guard = PARSER_STATES.lock();
    let e = guard.entry(ctxt as usize).or_default();
    e.serror = serror;
    e.sctx = ctx as usize;
}

/// Retrieve the error/warning callbacks of a parser context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlSchemaGetParserErrors(xmlSchemaParserCtxt *ctxt,
///                              xmlSchemaValidityErrorFunc *err,
///                              xmlSchemaValidityWarningFunc *warn, void **ctx);
/// ```
///
/// Returns 0 on success, -1 if `ctxt` is NULL. Output parameters may be NULL.
///
/// # SAFETY
///
/// - `ctxt` must be a parser context created by this crate; `err`/`warn`/
///   `ctx` must be writable or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaGetParserErrors(
    ctxt: *mut xmlSchemaParserCtxt,
    err: *mut Option<xmlValidityErrorFunc>,
    warn: *mut Option<xmlValidityWarningFunc>,
    ctx: *mut *mut c_void,
) -> c_int {
    if ctxt.is_null() {
        return -1;
    }
    let state = {
        let guard = PARSER_STATES.lock();
        guard.get(&(ctxt as usize)).copied().unwrap_or_default()
    };
    // SAFETY: output pointers are caller-guaranteed writable when non-NULL.
    unsafe {
        if !err.is_null() {
            *err = state.err;
        }
        if !warn.is_null() {
            *warn = state.warn;
        }
        if !ctx.is_null() {
            *ctx = state.ctx as *mut c_void;
        }
    }
    0
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlschemas.h — validation context
// ═══════════════════════════════════════════════════════════════════════════════

/// Set the error/warning callbacks of a validation context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlSchemaSetValidErrors(xmlSchemaValidCtxt *ctxt,
///                              xmlSchemaValidityErrorFunc err,
///                              xmlSchemaValidityWarningFunc warn, void *ctx);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be a validation context created by this crate; `err`/`warn`
///   must be valid callbacks or NULL; `ctx` may be NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaSetValidErrors(
    ctxt: *mut xmlSchemaValidCtxt,
    err: Option<xmlValidityErrorFunc>,
    warn: Option<xmlValidityWarningFunc>,
    ctx: *mut c_void,
) {
    if ctxt.is_null() {
        return;
    }
    let mut guard = VALID_STATES.lock();
    let e = guard.entry(ctxt as usize).or_default();
    e.err = err;
    e.warn = warn;
    e.ctx = ctx as usize;
}

/// Retrieve the error/warning callbacks of a validation context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlSchemaGetValidErrors(xmlSchemaValidCtxt *ctxt,
///                             xmlSchemaValidityErrorFunc *err,
///                             xmlSchemaValidityWarningFunc *warn, void **ctx);
/// ```
///
/// Returns 0 on success, -1 if `ctxt` is NULL. Output parameters may be NULL.
///
/// # SAFETY
///
/// - `ctxt` must be a validation context created by this crate; `err`/`warn`/
///   `ctx` must be writable or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaGetValidErrors(
    ctxt: *mut xmlSchemaValidCtxt,
    err: *mut Option<xmlValidityErrorFunc>,
    warn: *mut Option<xmlValidityWarningFunc>,
    ctx: *mut *mut c_void,
) -> c_int {
    if ctxt.is_null() {
        return -1;
    }
    let state = {
        let guard = VALID_STATES.lock();
        guard.get(&(ctxt as usize)).copied().unwrap_or_default()
    };
    // SAFETY: output pointers are caller-guaranteed writable when non-NULL.
    unsafe {
        if !err.is_null() {
            *err = state.err;
        }
        if !warn.is_null() {
            *warn = state.warn;
        }
        if !ctx.is_null() {
            *ctx = state.ctx as *mut c_void;
        }
    }
    0
}

/// Set the structured error callback of a validation context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlSchemaSetValidStructuredErrors(xmlSchemaValidCtxt *ctxt,
///                                        xmlStructuredErrorFunc serror, void *ctx);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be a validation context created by this crate; `serror`
///   must be a valid callback or NULL; `ctx` may be NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaSetValidStructuredErrors(
    ctxt: *mut xmlSchemaValidCtxt,
    serror: Option<xmlStructuredErrorFunc>,
    ctx: *mut c_void,
) {
    if ctxt.is_null() {
        return;
    }
    let mut guard = VALID_STATES.lock();
    let e = guard.entry(ctxt as usize).or_default();
    e.serror = serror;
    e.sctx = ctx as usize;
}

/// Set validation options.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlSchemaSetValidOptions(xmlSchemaValidCtxt *ctxt, int options);
/// ```
///
/// Accepts `XML_SCHEMA_VAL_VC_I_CREATE` and `XML_SCHEMA_VAL_XSI_ASSEMBLE`
/// (stored for the context; the internal engine does not branch on them).
/// Returns 0 on success, -1 if `ctxt` is NULL or the options are invalid.
///
/// # SAFETY
///
/// - `ctxt` must be a validation context created by this crate.
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaSetValidOptions(
    ctxt: *mut xmlSchemaValidCtxt,
    options: c_int,
) -> c_int {
    if ctxt.is_null() {
        return -1;
    }
    if options & !VAL_OPTIONS_MASK != 0 {
        return -1;
    }
    VALID_STATES
        .lock()
        .entry(ctxt as usize)
        .or_default()
        .options = options;
    0
}

/// Get the validation options of a context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlSchemaValidCtxtGetOptions(xmlSchemaValidCtxt *ctxt);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be a validation context created by this crate or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaValidCtxtGetOptions(ctxt: *mut xmlSchemaValidCtxt) -> c_int {
    if ctxt.is_null() {
        return 0;
    }
    let guard = VALID_STATES.lock();
    guard.get(&(ctxt as usize)).map(|s| s.options).unwrap_or(0)
}

/// Get the parser context associated with a validation context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlParserCtxt * xmlSchemaValidCtxtGetParserCtxt(xmlSchemaValidCtxt *ctxt);
/// ```
///
/// Upstream returns the parser context the validator was created from during
/// streaming (SAX) validation. The internal engine performs DOM-based
/// validation and never creates an associated parser context, so NULL is
/// returned (upstream also returns NULL when none is associated).
///
/// # SAFETY
///
/// - `ctxt` may be NULL or a validation context created by this crate.
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaValidCtxtGetParserCtxt(
    _ctxt: *mut xmlSchemaValidCtxt,
) -> *mut c_void {
    ptr::null_mut()
}

/// Set the filename reported with validation errors.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlSchemaValidateSetFilename(xmlSchemaValidCtxt *vctxt, const char *filename);
/// ```
///
/// The pointer is stored as-is (upstream does not copy it) and is used as
/// the `file` field of structured errors.
///
/// # SAFETY
///
/// - `vctxt` must be a validation context created by this crate; `filename`
///   must be a valid C string or NULL and stay alive while `vctxt` is used.
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaValidateSetFilename(
    vctxt: *mut xmlSchemaValidCtxt,
    filename: *const c_char,
) {
    if vctxt.is_null() {
        return;
    }
    VALID_STATES
        .lock()
        .entry(vctxt as usize)
        .or_default()
        .filename = filename as usize;
}

/// Set a validity locator callback.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlSchemaValidateSetLocator(xmlSchemaValidCtxt *vctxt,
///                                  xmlSchemaValidityLocatorFunc f, void *ctxt);
/// ```
///
/// The locator is stored; the internal engine reports errors without line
/// information, so the locator is not consulted.
///
/// # SAFETY
///
/// - `vctxt` must be a validation context created by this crate; `f` must be
///   a valid callback or NULL; `ctxt` may be NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaValidateSetLocator(
    vctxt: *mut xmlSchemaValidCtxt,
    f: Option<xmlSchemaValidityLocatorFunc>,
    ctxt: *mut c_void,
) {
    if vctxt.is_null() {
        return;
    }
    let mut guard = VALID_STATES.lock();
    let e = guard.entry(vctxt as usize).or_default();
    e.locator = f;
    e.locator_ctx = ctxt as usize;
}

/// Report whether the last validation had no errors.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlSchemaIsValid(xmlSchemaValidCtxt *ctxt);
/// ```
///
/// Returns 1 if the last validation passed, 0 otherwise (and for NULL).
///
/// # SAFETY
///
/// - `ctxt` must be a validation context created by this crate or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaIsValid(ctxt: *mut xmlSchemaValidCtxt) -> c_int {
    if ctxt.is_null() {
        return 0;
    }
    // SAFETY: ctxt is the internal XsdValidCtxt whose nb_errors the internal
    // xmlSchemaValidateDoc updates.
    let nb = unsafe { (*(ctxt as *mut XsdValidCtxt)).nb_errors };
    if nb == 0 {
        1
    } else {
        0
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlschemas.h — validation entry points
// ═══════════════════════════════════════════════════════════════════════════════

/// Validate an XML file against the context's schema.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlSchemaValidateFile(xmlSchemaValidCtxt *ctxt, const char *filename, int options);
/// ```
///
/// Returns the number of validation errors (0 = valid), or -1 on internal
/// error (unreadable/unparseable file). Errors are reported through the
/// context's callbacks. Wraps the internal engine's `xmlSchemaValidateDoc`.
///
/// # SAFETY
///
/// - `ctxt` must be a validation context created by this crate; `filename`
///   must be a valid C string.
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaValidateFile(
    ctxt: *mut xmlSchemaValidCtxt,
    filename: *const c_char,
    options: c_int,
) -> c_int {
    if ctxt.is_null() || filename.is_null() {
        return -1;
    }
    // Reset so xmlSchemaIsValid reflects this run even when the internal
    // engine only records errors on failure.
    unsafe { reset_valid_ctxt(ctxt) };
    // SAFETY: xmlReadFile requires a valid C string; options is forwarded.
    let doc = unsafe { crate::abi::exports_xml2::xmlReadFile(filename, ptr::null(), options) };
    if doc.is_null() {
        unsafe {
            dispatch_valid_errors(ctxt as usize, &["Failed to parse document".to_string()]);
        }
        return -1;
    }
    // SAFETY: ctxt is a valid XsdValidCtxt; doc is a valid _xmlDoc.
    let ret = unsafe { crate::xml::schemas::xmlSchemaValidateDoc(ctxt as *mut c_void, doc) };
    // SAFETY: doc was created by xmlReadFile; xmlFreeDoc matches.
    unsafe { crate::abi::exports_xml2::xmlFreeDoc(doc) };
    if ret != 0 {
        // SAFETY: ctxt is the internal XsdValidCtxt whose errors were filled
        // by xmlSchemaValidateDoc on failure.
        let errors = unsafe { (*(ctxt as *mut XsdValidCtxt)).errors.clone() };
        unsafe { dispatch_valid_errors(ctxt as usize, &errors) };
    }
    ret
}

/// Validate a single element against the context's schema.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlSchemaValidateOneElement(xmlSchemaValidCtxt *ctxt, xmlNode *elem);
/// ```
///
/// The element subtree is serialized and validated through the internal
/// engine (`xsd_validate`), which matches it against the global element
/// declarations of the schema. Returns the number of validation errors
/// (0 = valid), or -1 on internal error.
///
/// # SAFETY
///
/// - `ctxt` must be a validation context created by this crate; `elem` must
///   be a valid `_xmlNode` pointer.
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaValidateOneElement(
    ctxt: *mut xmlSchemaValidCtxt,
    elem: *mut _xmlNode,
) -> c_int {
    if ctxt.is_null() || elem.is_null() {
        return -1;
    }
    // Reset so xmlSchemaIsValid reflects this run even when the internal
    // engine only records errors on failure.
    unsafe { reset_valid_ctxt(ctxt) };
    // SAFETY: ctxt is the internal XsdValidCtxt.
    let valid_ctxt = unsafe { &mut *(ctxt as *mut XsdValidCtxt) };
    let Some(schema) = valid_ctxt.schema.clone() else {
        return -1;
    };
    // SAFETY: node_to_string requires a valid node.
    let Some(xml) = (unsafe { node_to_string(elem) }) else {
        return -1;
    };
    match xsd_validate(&schema, &xml) {
        Ok(()) => 0,
        Err(errors) => {
            // Record the errors on the context so xmlSchemaGetValidErrors /
            // xmlSchemaIsValid see this run's state.
            valid_ctxt.errors = errors.clone();
            valid_ctxt.nb_errors = errors.len() as i32;
            unsafe { dispatch_valid_errors(ctxt as usize, &errors) };
            errors.len() as c_int
        }
    }
}

/// Validate a stream of SAX events / input buffer content.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlSchemaValidateStream(xmlSchemaValidCtxt *ctxt, xmlParserInputBuffer *input,
///                             xmlCharEncoding enc, const xmlSAXHandler *sax,
///                             void *user_data);
/// ```
///
/// The internal engine is DOM-based, so content is read from `input` via its
/// read callback (the only path in the internal engine that carries data)
/// and validated as a document. Returns the number of validation errors
/// (0 = valid), or -1 when no content can be obtained (NULL input, NULL
/// read callback — upstream SAX-push validation is not supported).
///
/// # SAFETY
///
/// - `ctxt` must be a validation context created by this crate; `input`,
///   `sax`, `user_data` must be valid or NULL as documented above.
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaValidateStream(
    ctxt: *mut xmlSchemaValidCtxt,
    input: *mut _xmlParserInputBuffer,
    _enc: xmlCharEncoding,
    _sax: *const _xmlSAXHandler,
    _user_data: *mut c_void,
) -> c_int {
    if ctxt.is_null() || input.is_null() {
        return -1;
    }
    // SAFETY: input is a valid _xmlParserInputBuffer.
    let ib = unsafe { &*input };
    let Some(read) = ib.readcallback else {
        // The internal engine's memory/file input buffers carry no content,
        // so there is nothing to validate.
        return -1;
    };
    let mut content: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    // SAFETY: read is the caller-supplied callback; context/buffer are valid
    // for the duration of each call.
    loop {
        let n = unsafe { read(ib.context, chunk.as_mut_ptr() as *mut c_char, 4096) };
        if n <= 0 {
            break;
        }
        content.extend_from_slice(&chunk[..n as usize]);
    }
    let xml_str = String::from_utf8_lossy(&content).to_string();
    // SAFETY: Delegates to validate_doc_string with the same contract.
    unsafe { validate_doc_string(ctxt, &xml_str) }
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlschemas.h — SAX plug
// ═══════════════════════════════════════════════════════════════════════════════

/// Plug a schema validator into a SAX handler sequence.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlSchemaSAXPlugStruct * xmlSchemaSAXPlug(xmlSchemaValidCtxt *ctxt,
///                                           xmlSAXHandler **sax, void **user_data);
/// ```
///
/// Upstream replaces the caller's SAX handler with the validator's own and
/// returns a plug to restore it later. The internal engine performs DOM
/// validation and cannot intercept SAX events, so the plug is a pass-through:
/// `*sax` and `*user_data` are left untouched and a plug is returned so the
/// call sequence (plug → validate → unplug) still works. Returns NULL when
/// `ctxt` or `sax` is NULL.
///
/// # SAFETY
///
/// - `ctxt` must be a validation context created by this crate; `sax`/
///   `user_data` must be writable or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaSAXPlug(
    ctxt: *mut xmlSchemaValidCtxt,
    sax: *mut *mut _xmlSAXHandler,
    user_data: *mut *mut c_void,
) -> *mut xmlSchemaSAXPlugStruct {
    if ctxt.is_null() || sax.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: sax/user_data are caller-guaranteed writable when non-NULL.
    let original_sax = unsafe {
        if sax.is_null() {
            ptr::null_mut()
        } else {
            *sax
        }
    };
    let original_ud = unsafe {
        if user_data.is_null() {
            ptr::null_mut()
        } else {
            *user_data
        }
    };
    let plug = Box::new(XsdSaxPlug {
        sax: original_sax as *const _xmlSAXHandler,
        user_data: original_ud,
    });
    Box::into_raw(plug) as *mut xmlSchemaSAXPlugStruct
}

/// Unplug a schema validator from a SAX handler sequence.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlSchemaSAXUnplug(xmlSchemaSAXPlugStruct *plug);
/// ```
///
/// Frees the plug. Returns 0 on success, -1 if `plug` is NULL.
///
/// # SAFETY
///
/// - `plug` must be a plug returned by `xmlSchemaSAXPlug` or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaSAXUnplug(plug: *mut xmlSchemaSAXPlugStruct) -> c_int {
    if plug.is_null() {
        return -1;
    }
    // SAFETY: plug is a Box created by xmlSchemaSAXPlug.
    unsafe { drop(Box::from_raw(plug as *mut XsdSaxPlug)) };
    0
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlschemas.h — schema dump
// ═══════════════════════════════════════════════════════════════════════════════

/// Dump a schema to a FILE*.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlSchemaDump(FILE *output, xmlSchema *schema);
/// ```
///
/// Writes a human-readable listing of the compiled schema components
/// (target namespace, form defaults, component tree with types and
/// occurrence bounds) using the internal engine's `XsdSchema` data.
///
/// # SAFETY
///
/// - `output` must be a valid open `FILE*`; `schema` must be a schema
///   pointer produced by this crate's parser (or NULL).
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaDump(output: *mut c_void, schema: *mut xmlSchema) {
    if output.is_null() || schema.is_null() {
        return;
    }
    // SAFETY: schema is the internal boxed XsdSchema.
    let s = unsafe { &*(schema as *const XsdSchema) };
    let mut out = String::new();
    out.push_str("Schema dump\n");
    if let Some(ref tns) = s.target_namespace {
        out.push_str(&format!("  target namespace: {}\n", tns));
    } else {
        out.push_str("  target namespace: (none)\n");
    }
    out.push_str(&format!(
        "  element form default: {}\n",
        s.element_form_default.as_deref().unwrap_or("unqualified")
    ));
    out.push_str(&format!(
        "  attribute form default: {}\n",
        s.attribute_form_default.as_deref().unwrap_or("unqualified")
    ));
    out.push_str("  components:\n");
    for c in &s.components {
        dump_component(&mut out, c, 2);
    }
    // SAFETY: output is a valid FILE*.
    unsafe { libc::fputs(out.as_ptr() as *const c_char, output as *mut libc::FILE) };
}

/// Append a component (and its children) to the dump text.
fn dump_component(out: &mut String, c: &crate::xml::schemas::XsdComponent, depth: usize) {
    let indent = "  ".repeat(depth);
    let ctype = format!("{:?}", c.component_type).to_lowercase();
    let mut line = format!("{}{}", indent, ctype);
    if let Some(ref name) = c.name {
        line.push_str(&format!(" name='{}'", name));
    }
    if let Some(ref dtype) = c.datatype {
        line.push_str(&format!(" type={:?}", dtype));
    }
    if c.min_occurs != 1 || c.max_occurs != 1 {
        line.push_str(&format!(
            " minOccurs={} maxOccurs={}",
            c.min_occurs,
            if c.max_occurs == -1 {
                "unbounded".to_string()
            } else {
                c.max_occurs.to_string()
            }
        ));
    }
    if !c.facets.is_empty() {
        let facets: Vec<String> = c
            .facets
            .iter()
            .map(|(k, v)| format!("{:?}={}", k, v))
            .collect();
        line.push_str(&format!(" facets=[{}]", facets.join(", ")));
    }
    out.push_str(&line);
    out.push('\n');
    for child in &c.children {
        dump_component(out, child, depth + 1);
    }
    for attr in &c.attributes {
        dump_component(out, attr, depth + 1);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// schematron.h
// ═══════════════════════════════════════════════════════════════════════════════

/// Create a Schematron parser context from an already-parsed document.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlSchematronParserCtxt * xmlSchematronNewDocParserCtxt(xmlDoc *doc);
/// ```
///
/// The document is serialized and compiled through the internal Schematron
/// engine (`schematron_parse`). Following that engine's convention,
/// `xmlSchematronParse` returns its context as the schema pointer. Returns
/// NULL if `doc` is NULL or the schema fails to compile.
///
/// # SAFETY
///
/// - `doc` must be a valid `_xmlDoc` pointer.
#[no_mangle]
pub unsafe extern "C" fn xmlSchematronNewDocParserCtxt(
    doc: *mut _xmlDoc,
) -> *mut xmlSchematronParserCtxt {
    // SAFETY: doc_to_string requires a valid doc.
    let Some(xml) = (unsafe { doc_to_string(doc) }) else {
        return ptr::null_mut();
    };
    match schematron_parse(&xml) {
        Ok(schema) => Box::into_raw(Box::new(schema)) as *mut xmlSchematronParserCtxt,
        Err(_) => ptr::null_mut(),
    }
}

/// Set the structured error callback of a Schematron validation context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlSchematronSetValidStructuredErrors(xmlSchematronValidCtxt *ctxt,
///                                            xmlStructuredErrorFunc serror, void *ctx);
/// ```
///
/// The callback is stored in a side registry keyed by the context address.
/// (The internal Schematron engine reports errors through its own channel;
/// the stored callback is not invoked by it.)
///
/// # SAFETY
///
/// - `ctxt` must be a Schematron validation context created by this crate;
///   `serror` must be a valid callback or NULL; `ctx` may be NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlSchematronSetValidStructuredErrors(
    ctxt: *mut xmlSchematronValidCtxt,
    serror: Option<xmlStructuredErrorFunc>,
    ctx: *mut c_void,
) {
    if ctxt.is_null() {
        return;
    }
    let mut guard = SCHEMATRON_VALID_STATES.lock();
    let e = guard.entry(ctxt as usize).or_default();
    e.serror = serror;
    e.sctx = ctx as usize;
}

/// Install a custom resource loader on an XML Schema parser context
/// (upstream xmlschemas.c `xmlSchemaSetResourceLoader`).
///
/// # SAFETY
///
/// - `ctxt` must be a valid parser context pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlSchemaSetResourceLoader(
    ctxt: *mut c_void,
    loader: Option<crate::abi::callbacks::xmlResourceLoader>,
    data: *mut c_void,
) {
    if ctxt.is_null() {
        return;
    }
    let mut map = PARSER_STATES.lock();
    let st = map.entry(ctxt as usize).or_default();
    st.resource_loader = loader;
    st.resource_ctxt = data as usize;
}

/// Install a custom resource loader on an XInclude context
/// (upstream xinclude.c `xmlXIncludeSetResourceLoader`).
///
/// # SAFETY
///
/// - `ctxt` must be a valid XInclude context pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXIncludeSetResourceLoader(
    ctxt: crate::abi::exports_xinclude::xmlXIncludeCtxtPtr,
    loader: Option<crate::abi::callbacks::xmlResourceLoader>,
    data: *mut c_void,
) {
    if ctxt.is_null() {
        return;
    }
    let mut map = PARSER_STATES.lock();
    let st = map.entry(ctxt as usize).or_default();
    st.resource_loader = loader;
    st.resource_ctxt = data as usize;
}
