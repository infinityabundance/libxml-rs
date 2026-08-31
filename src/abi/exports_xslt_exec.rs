//! C ABI exports for libxslt.so.1 — the "exec" family (§16, Phase 8).
//!
//! This module implements the element-instruction *execution* entry points
//! of the libxslt 1.1.45 C ABI: the `xslt*` runtime handlers for
//! `xsl:attribute`, `xsl:element`, `xsl:text`, `xsl:comment`,
//! `xsl:processing-instruction`, `xsl:copy`, `xsl:copy-of`, `xsl:value-of`,
//! `xsl:number`, `xsl:choose`, `xsl:if`, `xsl:for-each`, `xsl:sort`,
//! `xsl:message`, the multi-document extension elements (`xsltDocumentElem`)
//! and the numeric-formatting / sorting machinery:
//!
//! - Numbering (numbers.c): `xsltNumberFormat`, `xsltFormatNumberConversion`,
//!   `xsltDecimalFormatGetByName`, `xsltDecimalFormatGetByQName`
//! - Sorting (xsltutils.c): `xsltDefaultSortFunction`, `xsltDoSortFunction`,
//!   `xsltDocumentSortFunction`, `xsltComputeSortResult`, `xsltSetSortFunc`,
//!   `xsltSetCtxtSortFunc`
//! - Debug helper (extra.c): `xsltDebug`
//!
//! Where the native-Rust XSLT engine in `src/xslt/*` already implements the
//! upstream behaviour (the engine is oracle-tested through the xsltproc
//! CLI), the exports below are wired to it; the rest are faithful ports of
//! the upstream C sources in `archaeology/libxslt-git/libxslt/`.

#![allow(non_snake_case)]
#![allow(unused_variables)]

use core::ptr;
use std::os::raw::{c_char, c_double, c_int, c_uint, c_void};

use crate::abi::allocator::{xmlFreeImpl, xmlMallocImpl};
use crate::abi::exports_buffer::xmlBufferCCat;
use crate::abi::exports_html::{htmlNewDoc, htmlNewDocNoDtD};
use crate::abi::exports_string::{xmlStrstr, xmlUTF8Strloc, xmlUTF8Strpos, xmlUTF8Strsize};
use crate::abi::exports_uri::xmlBuildURI;
use crate::abi::exports_xml2::{
    xmlBufferAdd, xmlBufferCat, xmlBufferContent, xmlBufferCreate, xmlBufferFree,
    xmlCreateIntSubset, xmlDocGetRootElement, xmlNewComment, xmlNewDoc, xmlStrEqual, xmlStrcat,
    xmlStrcmp, xmlStrdup, xmlStrlen, xmlStrncmp, xmlStrndup, xmlXPathCastStringToNumber,
    xmlXPathCastToString, xmlXPathCmpNodes, xmlXPathEvalExpression, xmlXPathFreeObject,
    xmlXPathNodeSetCreate,
};
use crate::abi::exports_xslt_apply::_xsltCompMatch;
use crate::abi::exports_xslt_avt::{
    xsltEvalAttrValueTemplate, xsltGetQNameURI, xsltGetUTF8Char as xmlGetUTF8Char,
};
use crate::abi::structs::*;
use crate::abi::types::xmlElementType::*;
use crate::abi::types::*;
use crate::xml::uri::xmlURIEscapeStr;
use crate::xml::xpath::exports::{xmlXPathIsInf, xmlXPathIsNaN};

/// The XSLT namespace URI (NUL-terminated; upstream `XSLT_NAMESPACE`).
const XSLT_NAMESPACE_URI: &[u8] = b"http://www.w3.org/1999/XSL/Transform\0";

/// `xsltOutputType::XSLT_OUTPUT_XML` (upstream xsltInternals.h, enum at 0).
const XSLT_OUTPUT_XML: c_int = 0;
/// `xsltOutputType::XSLT_OUTPUT_HTML`.
const XSLT_OUTPUT_HTML: c_int = 1;
/// `xsltOutputType::XSLT_OUTPUT_TEXT`.
const XSLT_OUTPUT_TEXT: c_int = 2;

/// `xmlCharEncoding::XML_CHAR_ENCODING_UTF8` (upstream encoding.h).
const XML_CHAR_ENCODING_UTF8: c_int = 3;

/// Extension-element namespaces handled by `xsltDocumentElem`.
const XSLT_SAXON_NAMESPACE: &[u8] = b"http://icl.com/saxon\0";
const XSLT_XALAN_NAMESPACE: &[u8] = b"http://xml.apache.org/xalan\0";

/// `XSLT_MAX_SORT` (upstream xsltInternals.h): max xsl:sort instructions.
const XSLT_MAX_SORT: c_int = 15;

/// `XSLT_FUNC_*` enum values used by `xsltDebug` (upstream xsltInternals.h).
const XSLT_FUNC_PARAM: c_int = 19;
const XSLT_FUNC_VARIABLE: c_int = 20;

/// Signature of the sort function used during sorting (upstream
/// `xsltSortFunc` in xsltInternals.h):
///
/// ```c
/// typedef void (*xsltSortFunc)(xsltTransformContextPtr ctxt,
///                              xmlNodePtr *sorts, int nbsorts);
/// ```
///
/// The handler is exposed as `Option<xsltSortFunc>` so that a NULL C
/// function pointer can be represented (the `Option` is null-pointer
/// optimized and ABI-identical to the raw function pointer).
pub type xsltSortFunc = unsafe extern "C" fn(*mut _xsltTransformContext, *mut *mut _xmlNode, c_int);

/// The global sort function (upstream `static xsltSortFunction`,
/// xsltutils.c) — `None` stands for the default handler.
static mut XSLT_SORT_FUNCTION: Option<xsltSortFunc> = None;

// ═══════════════════════════════════════════════════════════════════════════════
// Common helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Emit a message through the global `xsltGenericError` handler (falling
/// back to stderr), mirroring upstream `xsltGenericError(...)` calls with a
/// literal message.
unsafe fn emit_generic_error(msg: &[u8]) {
    let handler = crate::abi::data_globals::xsltGenericError;
    if let Some(f) = handler {
        f(
            crate::abi::data_globals::xsltGenericErrorContext,
            msg.as_ptr() as *const c_char,
        );
    } else {
        let _ = libc::write(2, msg.as_ptr() as *const c_void, msg.len());
    }
}

/// Compare two NUL-terminated xmlChar strings for equality.
unsafe fn xml_chars_equal(a: *const xmlChar, b: *const xmlChar) -> bool {
    if a.is_null() || b.is_null() {
        return false;
    }
    libc::strcmp(a as *const libc::c_char, b as *const libc::c_char) == 0
}

/// Read one UTF-8 char from a NUL-terminated string (upstream
/// `xsltGetUTF8CharZ`, xsltutils.c).  Returns the char value or -1 in case
/// of error and updates `len` with the number of bytes used.
unsafe fn xslt_get_utf8_char_z(utf: *const xmlChar, len: *mut c_int) -> c_int {
    if utf.is_null() || len.is_null() {
        if !len.is_null() {
            *len = 0;
        }
        return -1;
    }
    let mut c: u32 = *utf as u32;
    if c & 0x80 != 0 {
        if (*(utf.add(1)) & 0xc0) != 0x80 {
            *len = 0;
            return -1;
        }
        if (c & 0xe0) == 0xe0 {
            if (*(utf.add(2)) & 0xc0) != 0x80 {
                *len = 0;
                return -1;
            }
            if (c & 0xf0) == 0xf0 {
                if (c & 0xf8) != 0xf0 || (*(utf.add(3)) & 0xc0) != 0x80 {
                    *len = 0;
                    return -1;
                }
                *len = 4;
                /* 4-byte code */
                c = (((*utf & 0x7) as u32) << 18)
                    | (((*utf.add(1) & 0x3f) as u32) << 12)
                    | (((*utf.add(2) & 0x3f) as u32) << 6)
                    | ((*(utf.add(3)) & 0x3f) as u32);
            } else {
                /* 3-byte code */
                *len = 3;
                c = (((*utf & 0xf) as u32) << 12)
                    | (((*utf.add(1) & 0x3f) as u32) << 6)
                    | ((*(utf.add(2)) & 0x3f) as u32);
            }
        } else {
            /* 2-byte code */
            *len = 2;
            c = (((*utf & 0x1f) as u32) << 6) | ((*(utf.add(1)) & 0x3f) as u32);
        }
    } else {
        /* 1-byte code */
        *len = 1;
    }
    c as c_int
}

/// `xsltIsDigitZero` (numbers.c): is `ch` a Unicode digit-zero character?
unsafe fn xslt_is_digit_zero(ch: c_int) -> c_int {
    match ch {
        0x0030 | 0x0660 | 0x06f0 | 0x0966 | 0x09e6 | 0x0a66 | 0x0ae6 | 0x0b66 | 0x0c66 | 0x0ce6
        | 0x0d66 | 0x0e50 | 0x0ed0 | 0x0f20 => 1,
        _ => 0,
    }
}

/// `IS_DIGIT_ONE(x)` = `xsltIsDigitZero(x - 1)` (numbers.c).
unsafe fn xslt_is_digit_one(ch: c_int) -> c_int {
    xslt_is_digit_zero(ch - 1)
}

/// `xsltIsLetterDigit` (numbers.c): `xmlIsBaseCharQ || xmlIsIdeographicQ ||
/// xmlIsDigitQ`.
unsafe fn xslt_is_letter_digit(val: c_int) -> bool {
    if crate::xml::chvalid::xmlIsLetter(val) != 0 {
        return true;
    }
    crate::xml::chvalid::xmlIsDigit(val as c_uint) != 0
}

/// Test a node against a compiled `xsltCompMatch` (upstream
/// `xsltTestCompMatchList`), delegating to the engine's pattern matcher.
unsafe fn test_comp_match(
    context: *mut _xsltTransformContext,
    node: *mut _xmlNode,
    pat: *mut _xsltCompMatch,
) -> c_int {
    if pat.is_null() {
        return 0;
    }
    crate::xslt::patterns::xsltTestPattern(
        context,
        (*pat).pattern as *mut crate::xslt::patterns::_xsltPattern,
        node,
    )
}

/// `xsltTestCompMatchCount` (numbers.c): does `node` match the `count`
/// pattern?  With a NULL pattern, match any node with the same type and
/// expanded-name as the current node `cur`.
unsafe fn xslt_test_comp_match_count(
    context: *mut _xsltTransformContext,
    node: *mut _xmlNode,
    count_pat: *mut _xsltCompMatch,
    cur: *mut _xmlNode,
) -> c_int {
    if !count_pat.is_null() {
        return test_comp_match(context, node, count_pat);
    }
    if (*node).type_ != (*cur).type_ {
        return 0;
    }
    if (*node).type_ == XML_NAMESPACE_DECL as c_int {
        return 1;
    }
    if xmlStrEqual((*node).name, (*cur).name) == 0 {
        return 0;
    }
    if (*node).ns == (*cur).ns {
        return 1;
    }
    if (*node).ns.is_null() || (*cur).ns.is_null() {
        return 0;
    }
    xmlStrEqual((*(*node).ns).href, (*(*cur).ns).href)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Number formatting internals (numbers.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// `_xsltNumberData` — upstream numbersInternals.h.  Not present in
/// structs.rs, so it is defined here with the exact upstream layout.
///
/// ```c
/// struct _xsltNumberData {
///     const xmlChar *level;
///     const xmlChar *count;
///     const xmlChar *from;
///     const xmlChar *value;
///     const xmlChar *format;
///     int has_format;
///     int digitsPerGroup;
///     int groupingCharacter;
///     int groupingCharacterLen;
///     xmlDocPtr doc;
///     xmlNodePtr node;
///     struct _xsltCompMatch *countPat;
///     struct _xsltCompMatch *fromPat;
/// };
/// ```
#[repr(C)]
pub struct _xsltNumberData {
    pub level: *const xmlChar,
    pub count: *const xmlChar,
    pub from: *const xmlChar,
    pub value: *const xmlChar,
    pub format: *const xmlChar,
    pub has_format: c_int,
    pub digitsPerGroup: c_int,
    pub groupingCharacter: c_int,
    pub groupingCharacterLen: c_int,
    pub doc: *mut _xmlDoc,
    pub node: *mut _xmlNode,
    pub countPat: *mut _xsltCompMatch,
    pub fromPat: *mut _xsltCompMatch,
}

/// Pointer to an `_xsltNumberData` (`xsltNumberDataPtr`).
pub type xsltNumberDataPtr = *mut _xsltNumberData;

/// `MAX_TOKENS` (numbers.c).
const MAX_TOKENS: usize = 1024;

/// `struct _xsltFormatToken` (numbers.c): one number-format token.
#[repr(C)]
#[derive(Clone, Copy)]
struct xsltFormatToken {
    separator: *mut xmlChar,
    token: c_int,
    width: c_int,
}

/// `struct _xsltFormat` (numbers.c): a tokenized number-format picture.
#[repr(C)]
struct xsltFormat {
    start: *mut xmlChar,
    tokens: [xsltFormatToken; MAX_TOKENS],
    nTokens: c_int,
    end: *mut xmlChar,
}

/// Append a (possibly multibyte) character to an output buffer (upstream
/// `xsltCopyCharMultiByte`, numbers.c).
///
/// Returns the number of xmlChar written.
unsafe fn xslt_copy_char_multi_byte(mut out: *mut xmlChar, val: c_int) -> c_int {
    if out.is_null() || val < 0 {
        return 0;
    }
    if val >= 0x80 {
        let mut savedout = out;
        let mut bits: c_int;
        if val < 0x800 {
            *out = ((val >> 6) | 0xC0) as xmlChar;
            out = out.add(1);
            bits = 0;
        } else if val < 0x10000 {
            *out = ((val >> 12) | 0xE0) as xmlChar;
            out = out.add(1);
            bits = 6;
        } else if val < 0x110000 {
            *out = ((val >> 18) | 0xF0) as xmlChar;
            out = out.add(1);
            bits = 12;
        } else {
            return 0;
        }
        while bits >= 0 {
            *out = (((val >> bits) & 0x3F) | 0x80) as u8;
            out = out.add(1);
            bits -= 6;
        }
        return (out as isize - savedout as isize) as c_int;
    }
    *out = val as xmlChar;
    1
}

/// `xsltNumberFormatDecimal` (numbers.c): emit the decimal digits of
/// `number` (building the string from the back), with `digit_zero` as the
/// zero digit, a minimum `width`, and an optional grouping separator.
unsafe fn xslt_number_format_decimal(
    buffer: *mut _xmlBuffer,
    number: f64,
    digit_zero: c_int,
    width: c_int,
    digits_per_group: c_int,
    grouping_character: c_int,
    grouping_character_len: c_int,
) {
    let mut temp_string: [xmlChar; 500] = [0; 500];
    let base = temp_string.as_mut_ptr();
    let mut pointer = base.add(500).sub(1); /* last char */
    *pointer = 0;
    let mut number = number;
    let mut i: c_int = 0;
    while pointer > base {
        if (i >= width) && number.abs() < 1.0 {
            break;
        }
        if (i > 0)
            && (grouping_character != 0)
            && (digits_per_group > 0)
            && ((i % digits_per_group) == 0)
        {
            if pointer.offset(-(grouping_character_len as isize)) < base {
                i = -1; /* flag error */
                break;
            }
            pointer = pointer.offset(-(grouping_character_len as isize));
            xslt_copy_char_multi_byte(pointer, grouping_character);
        }
        let val = digit_zero + (number).rem_euclid(10.0) as c_int;
        if val < 0x80 {
            /* shortcut if ASCII */
            if pointer <= base {
                /* Check enough room */
                i = -1;
                break;
            }
            pointer = pointer.sub(1);
            *pointer = val as xmlChar;
        } else {
            let mut temp_char: [xmlChar; 6] = [0; 6];
            let len = xslt_copy_char_multi_byte(temp_char.as_mut_ptr(), val);
            if pointer.offset(-(len as isize)) < base {
                i = -1;
                break;
            }
            pointer = pointer.offset(-(len as isize));
            libc::memcpy(
                pointer as *mut libc::c_void,
                temp_char.as_ptr() as *const libc::c_void,
                len as usize,
            );
        }
        number /= 10.0;
        i += 1;
    }
    if i < 0 {
        emit_generic_error(b"xsltNumberFormatDecimal: Internal buffer size exceeded\n\0");
    }
    xmlBufferCat(buffer, pointer);
}

/// `xsltNumberFormatAlpha` (numbers.c): alphabetic numbering (a, b, ..., z,
/// aa, ...).  Numbers below 1 fall back to decimal.
unsafe fn xslt_number_format_alpha(
    data: *mut _xsltNumberData,
    buffer: *mut _xmlBuffer,
    number: f64,
    is_upper: c_int,
) {
    const ALPHA_UPPER: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    const ALPHA_LOWER: &[u8] = b"abcdefghijklmnopqrstuvwxyz";
    let alpha_size: f64 = 26.0;

    if number < 1.0 {
        xslt_number_format_decimal(
            buffer,
            number,
            b'0' as c_int,
            1,
            (*data).digitsPerGroup,
            (*data).groupingCharacter,
            (*data).groupingCharacterLen,
        );
        return;
    }
    /* Build buffer from back */
    let mut temp_string: [u8; 65] = [0; 65];
    let mut pointer = temp_string.as_mut_ptr().add(65);
    pointer = pointer.sub(1);
    *pointer = 0;
    let alpha_list: &[u8] = if is_upper != 0 {
        ALPHA_UPPER
    } else {
        ALPHA_LOWER
    };

    let mut number = number;
    let mut i: c_int = 1;
    while i < temp_string.len() as c_int {
        number -= 1.0;
        pointer = pointer.sub(1);
        *pointer = alpha_list[(number).rem_euclid(alpha_size) as usize];
        number /= alpha_size;
        if number < 1.0 {
            break;
        }
        i += 1;
    }
    xmlBufferCCat(buffer, pointer as *const c_char);
}

/// `xsltNumberFormatRoman` (numbers.c): Roman numeral numbering.  Numbers
/// outside [1, 5000] fall back to decimal.
unsafe fn xslt_number_format_roman(
    data: *mut _xsltNumberData,
    buffer: *mut _xmlBuffer,
    number: f64,
    is_upper: c_int,
) {
    if number < 1.0 || number > 5000.0 {
        xslt_number_format_decimal(
            buffer,
            number,
            b'0' as c_int,
            1,
            (*data).digitsPerGroup,
            (*data).groupingCharacter,
            (*data).groupingCharacterLen,
        );
        return;
    }
    let mut number = number;
    let hi = |s: &[u8]| -> Vec<u8> {
        if is_upper != 0 {
            s.to_vec()
        } else {
            s.to_ascii_lowercase()
        }
    };
    let mut ccat_hi = |s: &[u8], n: &mut f64, dec: f64| {
        xmlBufferCCat(buffer, hi(s).as_ptr() as *const c_char);
        *n -= dec;
    };
    while number >= 1000.0 {
        ccat_hi(b"M", &mut number, 1000.0);
    }
    if number >= 900.0 {
        ccat_hi(b"CM", &mut number, 900.0);
    }
    while number >= 500.0 {
        ccat_hi(b"D", &mut number, 500.0);
    }
    if number >= 400.0 {
        ccat_hi(b"CD", &mut number, 400.0);
    }
    while number >= 100.0 {
        ccat_hi(b"C", &mut number, 100.0);
    }
    if number >= 90.0 {
        ccat_hi(b"XC", &mut number, 90.0);
    }
    while number >= 50.0 {
        ccat_hi(b"L", &mut number, 50.0);
    }
    if number >= 40.0 {
        ccat_hi(b"XL", &mut number, 40.0);
    }
    while number >= 10.0 {
        ccat_hi(b"X", &mut number, 10.0);
    }
    if number >= 9.0 {
        ccat_hi(b"IX", &mut number, 9.0);
    }
    while number >= 5.0 {
        ccat_hi(b"V", &mut number, 5.0);
    }
    if number >= 4.0 {
        ccat_hi(b"IV", &mut number, 4.0);
    }
    while number >= 1.0 {
        ccat_hi(b"I", &mut number, 1.0);
    }
}

/// `xsltNumberFormatTokenize` (numbers.c): split a format picture into
/// separators and number tokens.
unsafe fn xslt_number_format_tokenize(format: *const xmlChar, tokens: *mut xsltFormat) {
    let mut ix: usize = 0;
    let mut len: c_int = 0;

    (*tokens).start = ptr::null_mut();
    (*tokens).tokens[0].separator = ptr::null_mut();
    (*tokens).end = ptr::null_mut();

    /*
     * Insert initial non-alphanumeric token.
     * There is always such a token in the list, even if NULL
     */
    let mut val = xslt_get_utf8_char_z(format.add(ix), &mut len);
    while !xslt_is_letter_digit(val) {
        if *format.add(ix) == 0 {
            break;
        }
        ix += len as usize;
        val = xslt_get_utf8_char_z(format.add(ix), &mut len);
    }
    if ix > 0 {
        (*tokens).start = xmlStrndup(format, ix as c_int);
    }

    (*tokens).nTokens = 0;
    while ((*tokens).nTokens as usize) < MAX_TOKENS {
        if *format.add(ix) == 0 {
            break;
        }
        /*
         * separator has already been parsed (except for the first
         * number) in tokens->end, recover it.
         */
        if (*tokens).nTokens > 0 {
            (*tokens).tokens[(*tokens).nTokens as usize].separator = (*tokens).end;
            (*tokens).end = ptr::null_mut();
        }

        val = xslt_get_utf8_char_z(format.add(ix), &mut len);
        if xslt_is_digit_one(val) != 0 || xslt_is_digit_zero(val) != 0 {
            (*tokens).tokens[(*tokens).nTokens as usize].width = 1;
            while xslt_is_digit_zero(val) != 0 {
                (*tokens).tokens[(*tokens).nTokens as usize].width += 1;
                ix += len as usize;
                val = xslt_get_utf8_char_z(format.add(ix), &mut len);
            }
            if xslt_is_digit_one(val) != 0 {
                (*tokens).tokens[(*tokens).nTokens as usize].token = val - 1;
                ix += len as usize;
                val = xslt_get_utf8_char_z(format.add(ix), &mut len);
            } else {
                (*tokens).tokens[(*tokens).nTokens as usize].token = b'0' as c_int;
                (*tokens).tokens[(*tokens).nTokens as usize].width = 1;
            }
        } else if val == b'A' as c_int
            || val == b'a' as c_int
            || val == b'I' as c_int
            || val == b'i' as c_int
        {
            (*tokens).tokens[(*tokens).nTokens as usize].token = val;
            ix += len as usize;
            val = xslt_get_utf8_char_z(format.add(ix), &mut len);
        } else {
            /* XSLT section 7.7: unsupported token → use "1". */
            (*tokens).tokens[(*tokens).nTokens as usize].token = b'0' as c_int;
            (*tokens).tokens[(*tokens).nTokens as usize].width = 1;
        }
        /*
         * Skip over remaining alphanumeric characters (Letter and Digit
         * classes from XML).
         */
        while xslt_is_letter_digit(val) {
            ix += len as usize;
            val = xslt_get_utf8_char_z(format.add(ix), &mut len);
        }

        /*
         * Insert temporary non-alphanumeric final token.
         */
        let j = ix;
        while !xslt_is_letter_digit(val) {
            if val == 0 {
                break;
            }
            ix += len as usize;
            val = xslt_get_utf8_char_z(format.add(ix), &mut len);
        }
        if ix > j {
            (*tokens).end = xmlStrndup(format.add(j), (ix - j) as c_int);
        }
        (*tokens).nTokens += 1;
    }
}

/// The default format token (upstream `default_token`, numbers.c).
unsafe fn default_token() -> xsltFormatToken {
    static DEFAULT_SEPARATOR: [u8; 2] = *b".\0";
    xsltFormatToken {
        separator: DEFAULT_SEPARATOR.as_ptr() as *mut xmlChar,
        token: b'0' as c_int,
        width: 1,
    }
}

/// `xsltNumberFormatInsertNumbers` (numbers.c): format `numbers_max` numbers
/// with the tokenized picture into `buffer`.
unsafe fn xslt_number_format_insert_numbers(
    data: *mut _xsltNumberData,
    numbers: *mut f64,
    numbers_max: c_int,
    tokens: *mut xsltFormat,
    buffer: *mut _xmlBuffer,
) {
    let mut i: c_int = 0;
    let mut number: f64;
    let mut token: xsltFormatToken;

    /*
     * Handle initial non-alphanumeric token
     */
    if !(*tokens).start.is_null() {
        xmlBufferCat(buffer, (*tokens).start);
    }

    while i < numbers_max {
        /* Insert number */
        number = *numbers.offset((numbers_max - 1 - i) as isize);
        /* Round to nearest like XSLT 2.0 */
        number = (number + 0.5).floor();
        if number < 0.0 {
            crate::xslt::errors::xsltTransformError(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                b"xsl-number : negative value\n\0".as_ptr() as *const c_char,
            );
            /* Recover by treating negative values as zero. */
            number = 0.0;
        }
        if (i as usize) < (*tokens).nTokens as usize {
            /*
             * The "n"th format token will be used to format the "n"th
             * number in the list
             */
            token = (*tokens).tokens[i as usize];
        } else if (*tokens).nTokens > 0 {
            /*
             * If there are more numbers than format tokens, then the
             * last format token will be used to format the remaining
             * numbers.
             */
            token = (*tokens).tokens[((*tokens).nTokens - 1) as usize];
        } else {
            /*
             * If there are no format tokens, then a format token of
             * 1 is used to format all numbers.
             */
            token = default_token();
        }

        /* Print separator, except for the first number */
        if i > 0 {
            if !token.separator.is_null() {
                xmlBufferCat(buffer, token.separator);
            } else {
                xmlBufferCCat(buffer, b".\0".as_ptr() as *const c_char);
            }
        }

        let inf = xmlXPathIsInf(number as c_double);
        if inf == -1 {
            xmlBufferCCat(buffer, b"-Infinity\0".as_ptr() as *const c_char);
        } else if inf == 1 {
            xmlBufferCCat(buffer, b"Infinity\0".as_ptr() as *const c_char);
        } else if xmlXPathIsNaN(number as c_double) != 0 {
            xmlBufferCCat(buffer, b"NaN\0".as_ptr() as *const c_char);
        } else {
            match token.token {
                /* 'A' */
                65 => {
                    xslt_number_format_alpha(data, buffer, number, 1);
                }
                /* 'a' */
                97 => {
                    xslt_number_format_alpha(data, buffer, number, 0);
                }
                /* 'I' */
                73 => {
                    xslt_number_format_roman(data, buffer, number, 1);
                }
                /* 'i' */
                105 => {
                    xslt_number_format_roman(data, buffer, number, 0);
                }
                _ => {
                    if xslt_is_digit_zero(token.token) != 0 {
                        xslt_number_format_decimal(
                            buffer,
                            number,
                            token.token,
                            token.width,
                            (*data).digitsPerGroup,
                            (*data).groupingCharacter,
                            (*data).groupingCharacterLen,
                        );
                    }
                }
            }
        }
        i += 1;
    }

    /*
     * Handle final non-alphanumeric token
     */
    if !(*tokens).end.is_null() {
        xmlBufferCat(buffer, (*tokens).end);
    }
}

/// `xsltNumberFormatGetAnyLevel` (numbers.c): count matches for
/// `level="any"`.
unsafe fn xslt_number_format_get_any_level(
    context: *mut _xsltTransformContext,
    node: *mut _xmlNode,
    count_pat: *mut _xsltCompMatch,
    from_pat: *mut _xsltCompMatch,
    array: *mut f64,
) -> c_int {
    let mut cnt: c_int = 0;
    let mut cur = node;

    while !cur.is_null() {
        /* process current node */
        if xslt_test_comp_match_count(context, cur, count_pat, node) != 0 {
            cnt += 1;
        }
        if !from_pat.is_null() && test_comp_match(context, cur, from_pat) != 0 {
            break;
        }

        /* Skip to next preceding or ancestor */
        let typ = (*cur).type_;
        if typ == XML_DOCUMENT_NODE as c_int || typ == XML_HTML_DOCUMENT_NODE as c_int {
            break;
        }

        if typ == XML_NAMESPACE_DECL as c_int {
            /* The XPath module stores the parent of a namespace node in
             * the ns->next field. */
            cur = (*(cur as *mut _xmlNs)).next as *mut _xmlNode;
        } else if typ == XML_ATTRIBUTE_NODE as c_int {
            cur = (*cur).parent;
        } else {
            while !(*cur).prev.is_null() {
                let pt = (*(*cur).prev).type_;
                if pt != XML_DTD_NODE as c_int
                    && pt != XML_XINCLUDE_START as c_int
                    && pt != XML_XINCLUDE_END as c_int
                {
                    break;
                }
                cur = (*cur).prev;
            }
            if !(*cur).prev.is_null() {
                cur = (*cur).prev;
                while !(*cur).last.is_null() {
                    cur = (*cur).last;
                }
            } else {
                cur = (*cur).parent;
            }
        }
    }

    *array.offset(0) = cnt as f64;
    1
}

/// `xsltNumberFormatGetMultipleLevel` (numbers.c): compute the hierarchical
/// number for `level="single"` / `level="multiple"`.
unsafe fn xslt_number_format_get_multiple_level(
    context: *mut _xsltTransformContext,
    node: *mut _xmlNode,
    count_pat: *mut _xsltCompMatch,
    from_pat: *mut _xsltCompMatch,
    array: *mut f64,
    max: c_int,
) -> c_int {
    let mut amount: c_int = 0;
    let mut ancestor = node;

    while !ancestor.is_null() && (*ancestor).type_ != XML_DOCUMENT_NODE as c_int {
        if !from_pat.is_null() && test_comp_match(context, ancestor, from_pat) != 0 {
            break;
        }

        if xslt_test_comp_match_count(context, ancestor, count_pat, node) != 0 {
            /* count(preceding-sibling::*) */
            let mut cnt: c_int = 1;
            let mut preceding = if (*ancestor).type_ != XML_NAMESPACE_DECL as c_int {
                (*ancestor).prev
            } else {
                ptr::null_mut()
            };
            while !preceding.is_null() {
                if xslt_test_comp_match_count(context, preceding, count_pat, node) != 0 {
                    cnt += 1;
                }
                preceding = (*preceding).prev;
            }
            *array.offset(amount as isize) = cnt as f64;
            amount += 1;
            if amount >= max {
                break;
            }
        }

        if !ancestor.is_null() && (*ancestor).type_ == XML_NAMESPACE_DECL as c_int {
            let ns = ancestor as *mut _xmlNs;
            if !(*ns).next.is_null() && (*(*ns).next).type_ != XML_NAMESPACE_DECL as c_int {
                ancestor = (*ns).next as *mut _xmlNode;
            } else {
                ancestor = ptr::null_mut();
            }
        } else {
            ancestor = (*ancestor).parent;
        }
    }

    amount
}

/// `xsltNumberFormatGetValue` (numbers.c): evaluate the `value` attribute
/// wrapped in `number(...)` against `node`.
unsafe fn xslt_number_format_get_value(
    context: *mut _xsltTransformContext,
    node: *mut _xmlNode,
    value: *const xmlChar,
    number: *mut f64,
) -> c_int {
    let mut amount: c_int = 0;
    let pattern = xmlBufferCreate();
    if !pattern.is_null() {
        xmlBufferCCat(pattern, b"number(\0".as_ptr() as *const c_char);
        xmlBufferCat(pattern, value);
        xmlBufferCCat(pattern, b")\0".as_ptr() as *const c_char);
        let old_node = (*context).node;
        (*context).node = node;
        let obj = crate::xslt::transform::eval_xpath(context, xmlBufferContent(pattern));
        if !obj.is_null() {
            *number = (*obj).floatval;
            amount += 1;
            xmlXPathFreeObject(obj);
        }
        xmlBufferFree(pattern);
        (*context).node = old_node;
    }
    amount
}

// ═══════════════════════════════════════════════════════════════════════════════
// Instruction handlers wired to the engine (transform.c / attributes.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// Execute an `xsl:attribute` instruction.
///
/// Wired to the engine's `process_attribute`, which evaluates the (possibly
/// AVT) `name` attribute, instantiates the content into a temporary
/// fragment and sets the resulting attribute on the current result element
/// (`ctxt->insert`).  The engine reads the current source node from
/// `ctxt->node`, so it is set from `contextNode` first (and restored, since
/// upstream leaves `ctxt->node` untouched).  `castedComp` is ignored — the
/// candidate does not populate `xsltElemPreComp` structs.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void
/// xsltAttribute(xsltTransformContextPtr ctxt, xmlNodePtr contextNode,
///               xmlNodePtr inst, xsltElemPreCompPtr castedComp);
/// ```
///
/// # SAFETY
///
/// - `ctxt`, `contextNode` and `inst` must be valid pointers.
#[no_mangle]
pub unsafe extern "C" fn xsltAttribute(
    ctxt: *mut _xsltTransformContext,
    contextNode: *mut _xmlNode,
    inst: *mut _xmlNode,
    castedComp: *mut c_void,
) {
    let _ = castedComp;
    if ctxt.is_null() || contextNode.is_null() || inst.is_null() {
        return;
    }
    if (*inst).type_ != XML_ELEMENT_NODE as c_int {
        return;
    }
    let old_node = (*ctxt).node;
    (*ctxt).node = contextNode;
    crate::xslt::transform::process_attribute(ctxt, inst);
    (*ctxt).node = old_node;
}

/// Execute an `xsl:element` instruction.
///
/// Wired to the engine's `process_element`, which evaluates the `name` and
/// `namespace` attributes (both AVTs), creates the result element at
/// `ctxt->insert` and instantiates the content inside it.  Upstream returns
/// immediately when there is no insertion point; that guard is kept.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void
/// xsltElement(xsltTransformContextPtr ctxt, xmlNodePtr node,
///             xmlNodePtr inst, xsltElemPreCompPtr castedComp);
/// ```
///
/// # SAFETY
///
/// - `ctxt`, `node` and `inst` must be valid pointers.
#[no_mangle]
pub unsafe extern "C" fn xsltElement(
    ctxt: *mut _xsltTransformContext,
    node: *mut _xmlNode,
    inst: *mut _xmlNode,
    castedComp: *mut c_void,
) {
    let _ = castedComp;
    if ctxt.is_null() || node.is_null() || inst.is_null() {
        return;
    }
    if (*ctxt).insert.is_null() {
        return;
    }
    let old_node = (*ctxt).node;
    (*ctxt).node = node;
    crate::xslt::transform::process_element(ctxt, inst);
    (*ctxt).node = old_node;
}

/// Execute an `xsl:text` instruction.
///
/// Wired to the engine's `process_text`, which copies the text children of
/// the instruction into the result tree.  NOTE: upstream 1.1.45 reads the
/// compiled `disable-output-escaping` flag from `inst->psvi` (set at
/// compile time) and marks non-CDATA text with `xmlStringTextNoenc`; the
/// engine instead reads the `disable-output-escaping` attribute directly
/// from the instruction node and appends ordinary text nodes, so the
/// serialization layer decides the escaping — documented simplification
/// (the engine is the oracle-tested behaviour).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void
/// xsltText(xsltTransformContextPtr ctxt, xmlNodePtr node ATTRIBUTE_UNUSED,
///          xmlNodePtr inst, xsltElemPreCompPtr comp ATTRIBUTE_UNUSED);
/// ```
///
/// # SAFETY
///
/// - `ctxt` and `inst` must be valid pointers.
#[no_mangle]
pub unsafe extern "C" fn xsltText(
    ctxt: *mut _xsltTransformContext,
    node: *mut _xmlNode,
    inst: *mut _xmlNode,
    comp: *mut c_void,
) {
    let _ = (node, comp);
    if ctxt.is_null() || inst.is_null() {
        return;
    }
    if (*inst).children.is_null() {
        return;
    }
    crate::xslt::transform::process_text(ctxt, inst);
}

/// Execute an `xsl:comment` instruction.
///
/// Wired to the engine's `process_comment`, which computes the string value
/// of the content and appends a comment node.  Upstream additionally
/// rejects comments containing `--` or ending in `-` (reporting an error
/// but still emitting the comment) — the engine skips that validation;
/// documented simplification.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void
/// xsltComment(xsltTransformContextPtr ctxt, xmlNodePtr node,
///             xmlNodePtr inst, xsltElemPreCompPtr comp ATTRIBUTE_UNUSED);
/// ```
///
/// # SAFETY
///
/// - `ctxt`, `node` and `inst` must be valid pointers.
#[no_mangle]
pub unsafe extern "C" fn xsltComment(
    ctxt: *mut _xsltTransformContext,
    node: *mut _xmlNode,
    inst: *mut _xmlNode,
    comp: *mut c_void,
) {
    let _ = comp;
    if ctxt.is_null() || node.is_null() || inst.is_null() {
        return;
    }
    let old_node = (*ctxt).node;
    (*ctxt).node = node;
    crate::xslt::transform::process_comment(ctxt, inst);
    (*ctxt).node = old_node;
}

/// Execute an `xsl:processing-instruction` instruction.
///
/// Wired to the engine's `process_pi`, which evaluates the `name` attribute
/// (AVT) and appends a PI node with the string value of the content.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void
/// xsltProcessingInstruction(xsltTransformContextPtr ctxt, xmlNodePtr node,
///                           xmlNodePtr inst, xsltElemPreCompPtr castedComp);
/// ```
///
/// # SAFETY
///
/// - `ctxt`, `node` and `inst` must be valid pointers.
#[no_mangle]
pub unsafe extern "C" fn xsltProcessingInstruction(
    ctxt: *mut _xsltTransformContext,
    node: *mut _xmlNode,
    inst: *mut _xmlNode,
    castedComp: *mut c_void,
) {
    let _ = castedComp;
    if ctxt.is_null() || node.is_null() || inst.is_null() {
        return;
    }
    if (*ctxt).insert.is_null() {
        return;
    }
    let old_node = (*ctxt).node;
    (*ctxt).node = node;
    crate::xslt::transform::process_pi(ctxt, inst);
    (*ctxt).node = old_node;
}

/// Execute an `xsl:copy` instruction.
///
/// Wired to the engine's `process_copy`, which shallow-copies the current
/// node (`ctxt->node`) into the result tree and instantiates the content
/// inside the copy (element case) or at the insertion point otherwise.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void
/// xsltCopy(xsltTransformContextPtr ctxt, xmlNodePtr node,
///          xmlNodePtr inst, xsltElemPreCompPtr castedComp);
/// ```
///
/// # SAFETY
///
/// - `ctxt`, `node` and `inst` must be valid pointers.
#[no_mangle]
pub unsafe extern "C" fn xsltCopy(
    ctxt: *mut _xsltTransformContext,
    node: *mut _xmlNode,
    inst: *mut _xmlNode,
    castedComp: *mut c_void,
) {
    let _ = castedComp;
    if ctxt.is_null() || node.is_null() || inst.is_null() {
        return;
    }
    let old_node = (*ctxt).node;
    (*ctxt).node = node;
    crate::xslt::transform::process_copy(ctxt, inst);
    (*ctxt).node = old_node;
}

/// Execute an `xsl:copy-of` instruction.
///
/// Wired to the engine's `process_copy_of`, which evaluates the `select`
/// expression and deep-copies node-sets / result tree fragments (or inserts
/// the string value for atomic results).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void
/// xsltCopyOf(xsltTransformContextPtr ctxt, xmlNodePtr node,
///            xmlNodePtr inst, xsltElemPreCompPtr castedComp);
/// ```
///
/// # SAFETY
///
/// - `ctxt`, `node` and `inst` must be valid pointers.
#[no_mangle]
pub unsafe extern "C" fn xsltCopyOf(
    ctxt: *mut _xsltTransformContext,
    node: *mut _xmlNode,
    inst: *mut _xmlNode,
    castedComp: *mut c_void,
) {
    let _ = castedComp;
    if ctxt.is_null() || node.is_null() || inst.is_null() {
        return;
    }
    let old_node = (*ctxt).node;
    (*ctxt).node = node;
    crate::xslt::transform::process_copy_of(ctxt, inst);
    (*ctxt).node = old_node;
}

/// Execute an `xsl:value-of` instruction.
///
/// Wired to the engine's `process_value_of`, which evaluates the `select`
/// expression, casts the result to a string and appends a text node.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void
/// xsltValueOf(xsltTransformContextPtr ctxt, xmlNodePtr node,
///             xmlNodePtr inst, xsltElemPreCompPtr castedComp);
/// ```
///
/// # SAFETY
///
/// - `ctxt`, `node` and `inst` must be valid pointers.
#[no_mangle]
pub unsafe extern "C" fn xsltValueOf(
    ctxt: *mut _xsltTransformContext,
    node: *mut _xmlNode,
    inst: *mut _xmlNode,
    castedComp: *mut c_void,
) {
    let _ = castedComp;
    if ctxt.is_null() || node.is_null() || inst.is_null() {
        return;
    }
    let old_node = (*ctxt).node;
    (*ctxt).node = node;
    crate::xslt::transform::process_value_of(ctxt, inst);
    (*ctxt).node = old_node;
}

/// Execute an `xsl:number` instruction.
///
/// Wired to the engine's `process_number`, which evaluates the `value`
/// attribute (or computes a position from the current node) and formats the
/// result with the `format` attribute.  NOTE: upstream computes the number
/// from the compiled `level`/`count`/`from` data in `castedComp->numdata`;
/// the engine performs the simplified level computation (preceding-sibling
/// count + 1) and formats with `xsltFormatNumber` — documented
/// simplification.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void
/// xsltNumber(xsltTransformContextPtr ctxt, xmlNodePtr node,
///            xmlNodePtr inst, xsltElemPreCompPtr castedComp);
/// ```
///
/// # SAFETY
///
/// - `ctxt`, `node` and `inst` must be valid pointers.
#[no_mangle]
pub unsafe extern "C" fn xsltNumber(
    ctxt: *mut _xsltTransformContext,
    node: *mut _xmlNode,
    inst: *mut _xmlNode,
    castedComp: *mut c_void,
) {
    let _ = castedComp;
    if ctxt.is_null() || node.is_null() || inst.is_null() {
        return;
    }
    let old_node = (*ctxt).node;
    (*ctxt).node = node;
    crate::xslt::transform::process_number(ctxt, inst);
    (*ctxt).node = old_node;
}

/// Execute an `xsl:choose` instruction.
///
/// Wired to the engine's `process_choose`, which evaluates the `test`
/// attribute of each `xsl:when` child in turn and instantiates the first
/// matching branch (or the `xsl:otherwise` child).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void
/// xsltChoose(xsltTransformContextPtr ctxt, xmlNodePtr contextNode,
///            xmlNodePtr inst, xsltElemPreCompPtr comp ATTRIBUTE_UNUSED);
/// ```
///
/// # SAFETY
///
/// - `ctxt`, `contextNode` and `inst` must be valid pointers.
#[no_mangle]
pub unsafe extern "C" fn xsltChoose(
    ctxt: *mut _xsltTransformContext,
    contextNode: *mut _xmlNode,
    inst: *mut _xmlNode,
    comp: *mut c_void,
) {
    let _ = comp;
    if ctxt.is_null() || contextNode.is_null() || inst.is_null() {
        return;
    }
    let old_node = (*ctxt).node;
    (*ctxt).node = contextNode;
    crate::xslt::transform::process_choose(ctxt, inst);
    (*ctxt).node = old_node;
}

/// Execute an `xsl:if` instruction.
///
/// Wired to the engine's `process_if`, which evaluates the `test` attribute
/// (XPath 1.0 boolean conversion) and instantiates the content when true.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void
/// xsltIf(xsltTransformContextPtr ctxt, xmlNodePtr contextNode,
///        xmlNodePtr inst, xsltElemPreCompPtr castedComp);
/// ```
///
/// # SAFETY
///
/// - `ctxt`, `contextNode` and `inst` must be valid pointers.
#[no_mangle]
pub unsafe extern "C" fn xsltIf(
    ctxt: *mut _xsltTransformContext,
    contextNode: *mut _xmlNode,
    inst: *mut _xmlNode,
    castedComp: *mut c_void,
) {
    let _ = castedComp;
    if ctxt.is_null() || contextNode.is_null() || inst.is_null() {
        return;
    }
    let old_node = (*ctxt).node;
    (*ctxt).node = contextNode;
    crate::xslt::transform::process_if(ctxt, inst);
    (*ctxt).node = old_node;
}

/// Execute an `xsl:for-each` instruction.
///
/// Wired to the engine's `process_for_each`, which evaluates the `select`
/// expression, optionally sorts the resulting node-set with the engine's
/// `xsltSortNodeSet`, and instantiates the content once per node (with the
/// context position/size maintained).  The engine saves and restores the
/// current node itself; the wrapper additionally restores `ctxt->node` to
/// its pre-call value (upstream leaves it unchanged for the caller).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void
/// xsltForEach(xsltTransformContextPtr ctxt, xmlNodePtr contextNode,
///             xmlNodePtr inst, xsltElemPreCompPtr castedComp);
/// ```
///
/// # SAFETY
///
/// - `ctxt`, `contextNode` and `inst` must be valid pointers.
#[no_mangle]
pub unsafe extern "C" fn xsltForEach(
    ctxt: *mut _xsltTransformContext,
    contextNode: *mut _xmlNode,
    inst: *mut _xmlNode,
    castedComp: *mut c_void,
) {
    let _ = castedComp;
    if ctxt.is_null() || contextNode.is_null() || inst.is_null() {
        return;
    }
    let old_node = (*ctxt).node;
    (*ctxt).node = contextNode;
    crate::xslt::transform::process_for_each(ctxt, inst);
    (*ctxt).node = old_node;
}

/// Process an `xsl:message` instruction.
///
/// Wired to the engine's `process_message`, which writes the string value
/// of the content to stderr and stops the transformation when
/// `terminate="yes"` (the engine flags `XSLT_STATE_ERROR`, which also
/// discards the result — upstream uses `XSLT_STATE_STOPPED`; observable
/// outcome is the same).  NOTE: upstream routes the message through the
/// per-context error handler (`ctxt->error`) with a trailing newline; the
/// engine writes directly to stderr — documented simplification.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void
/// xsltMessage(xsltTransformContextPtr ctxt, xmlNodePtr node, xmlNodePtr inst);
/// ```
///
/// # SAFETY
///
/// - `ctxt` and `inst` must be valid pointers; `node` may be NULL.
#[no_mangle]
pub unsafe extern "C" fn xsltMessage(
    ctxt: *mut _xsltTransformContext,
    node: *mut _xmlNode,
    inst: *mut _xmlNode,
) {
    if ctxt.is_null() || inst.is_null() {
        return;
    }
    let old_node = (*ctxt).node;
    if !node.is_null() {
        (*ctxt).node = node;
    }
    crate::xslt::transform::process_message(ctxt, inst);
    (*ctxt).node = old_node;
}

// ═══════════════════════════════════════════════════════════════════════════════
// Numbering (numbers.c / xslt.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// Format a number according to an `_xsltNumberData` specification
/// (upstream `xsltNumberFormat`, numbers.c).
///
/// Ported faithfully: the format picture is tokenized, the value is
/// computed either from `data->value` (evaluated as `number(...)`) or from
/// the `level`/`count`/`from` patterns (`single`/`multiple`/`any`), and the
/// resulting text node is appended at `ctxt->insert`.  Pattern matching
/// (`countPat`/`fromPat`) delegates to the engine's `xsltTestPattern`.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void
/// xsltNumberFormat(xsltTransformContextPtr ctxt, xsltNumberDataPtr data,
///                  xmlNodePtr node);
/// ```
///
/// # SAFETY
///
/// - `ctxt`, `data` and `node` must be valid pointers.
#[no_mangle]
pub unsafe extern "C" fn xsltNumberFormat(
    ctxt: *mut _xsltTransformContext,
    data: xsltNumberDataPtr,
    node: *mut _xmlNode,
) {
    if ctxt.is_null() || data.is_null() {
        return;
    }
    let mut tokens: xsltFormat = core::mem::zeroed();

    if !(*data).format.is_null() {
        xslt_number_format_tokenize((*data).format, &mut tokens);
    } else {
        /* The format needs to be recomputed each time */
        if (*data).has_format == 0 {
            return;
        }
        let format = xsltEvalAttrValueTemplate(
            ctxt,
            (*data).node,
            b"format\0".as_ptr() as *const xmlChar,
            XSLT_NAMESPACE_URI.as_ptr() as *const xmlChar,
        );
        if format.is_null() {
            return;
        }
        xslt_number_format_tokenize(format, &mut tokens);
        xmlFreeImpl(format as *mut c_void);
    }

    let output = xmlBufferCreate();
    if output.is_null() {
        // Nothing was emitted; still clean up the tokenized picture.
        cleanup_tokens(&mut tokens);
        return;
    }

    /*
     * Evaluate the XPath expression to find the value(s)
     */
    if !(*data).value.is_null() {
        let mut number: f64 = 0.0;
        let amount = xslt_number_format_get_value(ctxt, node, (*data).value, &mut number);
        if amount == 1 {
            xslt_number_format_insert_numbers(data, &mut number, 1, &mut tokens, output);
        }
    } else if !(*data).level.is_null() {
        if xmlStrEqual((*data).level, b"single\0".as_ptr() as *const xmlChar) != 0 {
            let mut number: f64 = 0.0;
            let amount = xslt_number_format_get_multiple_level(
                ctxt,
                node,
                (*data).countPat,
                (*data).fromPat,
                &mut number,
                1,
            );
            if amount == 1 {
                xslt_number_format_insert_numbers(data, &mut number, 1, &mut tokens, output);
            }
        } else if xmlStrEqual((*data).level, b"multiple\0".as_ptr() as *const xmlChar) != 0 {
            let mut numarray: [f64; 1024] = [0.0; 1024];
            let max = (numarray.len()) as c_int;
            let amount = xslt_number_format_get_multiple_level(
                ctxt,
                node,
                (*data).countPat,
                (*data).fromPat,
                numarray.as_mut_ptr(),
                max,
            );
            if amount > 0 {
                xslt_number_format_insert_numbers(
                    data,
                    numarray.as_mut_ptr(),
                    amount,
                    &mut tokens,
                    output,
                );
            }
        } else if xmlStrEqual((*data).level, b"any\0".as_ptr() as *const xmlChar) != 0 {
            let mut number: f64 = 0.0;
            let amount = xslt_number_format_get_any_level(
                ctxt,
                node,
                (*data).countPat,
                (*data).fromPat,
                &mut number,
            );
            if amount > 0 {
                xslt_number_format_insert_numbers(data, &mut number, 1, &mut tokens, output);
            }
        }

        /*
         * Unlike `match` patterns, `count` and `from` patterns can contain
         * variable references, so the pattern match cache is cleared if the
         * "direct" matching algorithm was used (no-op in the candidate).
         */
        if !(*data).countPat.is_null() {
            crate::abi::exports_xslt_apply::xsltCompMatchClearCache(ctxt, (*data).countPat);
        }
        if !(*data).fromPat.is_null() {
            crate::abi::exports_xslt_apply::xsltCompMatchClearCache(ctxt, (*data).fromPat);
        }
    }

    /* Insert number as text node */
    crate::xslt::transform::append_text_node(ctxt, xmlBufferContent(output));

    xmlBufferFree(output);
    cleanup_tokens(&mut tokens);
}

/// Free the allocated parts of a tokenized number-format picture.
unsafe fn cleanup_tokens(tokens: *mut xsltFormat) {
    if !(*tokens).start.is_null() {
        xmlFreeImpl((*tokens).start as *mut c_void);
    }
    if !(*tokens).end.is_null() {
        xmlFreeImpl((*tokens).end as *mut c_void);
    }
    let mut i: c_int = 0;
    while i < (*tokens).nTokens {
        if !(*tokens).tokens[i as usize].separator.is_null() {
            xmlFreeImpl((*tokens).tokens[i as usize].separator as *mut c_void);
        }
        i += 1;
    }
}

/// Implement the JDK 1.1 `DecimalFormat` algorithm used by `format-number()`
/// (upstream `xsltFormatNumberConversion`, numbers.c).
///
/// Ported faithfully from numbers.c 1.1.45: the positive subpattern is
/// parsed (prefix, integer `#`/`0` run, fraction `0`/`#` run, suffix), the
/// negative subpattern (after `patternSeparator`) is applied for negative
/// numbers, the multiplier from `%`/`permille` is applied, the number is
/// rounded to the fraction width and the result is assembled with the
/// decimal format's characters (grouping, decimal point, minus sign,
/// infinity, NaN).
///
/// When `self` is NULL a default ASCII decimal format is substituted (the
/// same character set upstream's default `xsltDecimalFormat` carries), so
/// the NULL case behaves like upstream-with-default instead of crashing.
/// `*result` is allocated with `xmlStrdup` (caller frees with `xmlFree`).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlXPathError
/// xsltFormatNumberConversion(xsltDecimalFormatPtr self, xmlChar *format,
///                            double number, xmlChar **result);
/// ```
///
/// # SAFETY
///
/// - `format` and `result` must be valid pointers; `self` may be NULL.
#[no_mangle]
pub unsafe extern "C" fn xsltFormatNumberConversion(
    self_: *mut _xsltDecimalFormat,
    format: *mut xmlChar,
    number: f64,
    result: *mut *mut xmlChar,
) -> c_int {
    if result.is_null() {
        return XPATH_MEMORY_ERROR as c_int;
    }
    // Canonical implementation lives in src/xslt/numbering/mod.rs; this
    // ABI export delegates so all decimal-format picture handling shares
    // exactly one code path (the port of numbers.c xsltFormatNumberConversion).
    crate::xslt::numbering::xslt_format_number_conversion(self_, format, number, result)
}

/// Find a named decimal format in a stylesheet (upstream
/// `xsltDecimalFormatGetByName`, xslt.c).
///
/// With a NULL `name` the default format (the head of the chain) is
/// returned.  Otherwise the `decimalFormat` chain of the stylesheet and its
/// imports is walked; the first entry with a matching name and a NULL
/// namespace URI wins.  Returns NULL when no match exists.  (The candidate's
/// compiler only creates decimal-format entries for explicit
/// `xsl:decimal-format` elements, so a stylesheet without any may have a
/// NULL chain — the walk handles that by returning NULL, where upstream
/// assumes a default entry exists.)
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xsltDecimalFormatPtr
/// xsltDecimalFormatGetByName(xsltStylesheetPtr style, xmlChar *name);
/// ```
///
/// # SAFETY
///
/// - `style` must be a valid stylesheet; `name` may be NULL.
#[no_mangle]
pub unsafe extern "C" fn xsltDecimalFormatGetByName(
    style: *mut _xsltStylesheet,
    name: *mut xmlChar,
) -> *mut _xsltDecimalFormat {
    if style.is_null() {
        return ptr::null_mut();
    }
    if name.is_null() {
        return (*style).decimalFormat;
    }
    let mut cur_style = style;
    while !cur_style.is_null() {
        if !(*cur_style).decimalFormat.is_null() {
            let mut result = (*(*cur_style).decimalFormat).next;
            while !result.is_null() {
                if (*result).nsUri.is_null()
                    && xmlStrEqual(name as *const xmlChar, (*result).name) != 0
                {
                    return result;
                }
                result = (*result).next;
            }
        }
        cur_style = crate::abi::exports_xslt_apply::xsltNextImport(cur_style);
    }
    ptr::null_mut()
}

/// Find a decimal format by expanded-name (upstream
/// `xsltDecimalFormatGetByQName`, xslt.c).
///
/// Same walk as [`xsltDecimalFormatGetByName`], matching on namespace URI
/// and local name.  With a NULL `name` the default format is returned.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xsltDecimalFormatPtr
/// xsltDecimalFormatGetByQName(xsltStylesheetPtr style, const xmlChar *nsUri,
///                             const xmlChar *name);
/// ```
///
/// # SAFETY
///
/// - `style` must be a valid stylesheet; `nsUri` and `name` may be NULL.
#[no_mangle]
pub unsafe extern "C" fn xsltDecimalFormatGetByQName(
    style: *mut _xsltStylesheet,
    nsUri: *const xmlChar,
    name: *const xmlChar,
) -> *mut _xsltDecimalFormat {
    if style.is_null() {
        return ptr::null_mut();
    }
    if name.is_null() {
        return (*style).decimalFormat;
    }
    let mut cur_style = style;
    while !cur_style.is_null() {
        if !(*cur_style).decimalFormat.is_null() {
            let mut result = (*(*cur_style).decimalFormat).next;
            while !result.is_null() {
                if xmlStrEqual(nsUri, (*result).nsUri) != 0
                    && xmlStrEqual(name, (*result).name) != 0
                {
                    return result;
                }
                result = (*result).next;
            }
        }
        cur_style = crate::abi::exports_xslt_apply::xsltNextImport(cur_style);
    }
    ptr::null_mut()
}

// ═══════════════════════════════════════════════════════════════════════════════
// Sorting (transform.c / xsltutils.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// Process an `xsl:sort` node.
///
/// Upstream 1.1.45 (transform.c) never dispatches `xsl:sort` through the
/// instruction handlers — the sort nodes are collected and handed to
/// [`xsltDoSortFunction`] by `xsl:apply-templates`/`xsl:for-each` — so
/// `xsltSort` is an error stub reporting "improper use".  Ported verbatim.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void
/// xsltSort(xsltTransformContextPtr ctxt, xmlNodePtr node ATTRIBUTE_UNUSED,
///          xmlNodePtr inst, xsltElemPreCompPtr comp);
/// ```
///
/// # SAFETY
///
/// - `ctxt` and `inst` must be valid pointers; `comp` may be NULL.
#[no_mangle]
pub unsafe extern "C" fn xsltSort(
    ctxt: *mut _xsltTransformContext,
    node: *mut _xmlNode,
    inst: *mut _xmlNode,
    comp: *mut c_void,
) {
    let _ = node;
    if ctxt.is_null() || inst.is_null() {
        return;
    }
    if comp.is_null() {
        crate::xslt::errors::xsltTransformError(
            ctxt,
            ptr::null_mut(),
            inst,
            b"xsl:sort : compilation failed\n\0".as_ptr() as *const c_char,
        );
        return;
    }
    crate::xslt::errors::xsltTransformError(
        ctxt,
        ptr::null_mut(),
        inst,
        b"xsl:sort : improper use this should not be reached\n\0".as_ptr() as *const c_char,
    );
}

/// The default sort function: reorder `ctxt->nodeList` in place according
/// to the sort keys of the `sorts` array of `xsl:sort` instruction nodes
/// (upstream `xsltDefaultSortFunction`, xsltutils.c).
///
/// The candidate compiles each `xsl:sort` node on the fly with the engine's
/// `xsltCompileSort` and delegates the actual reordering to the engine's
/// `xsltSortNodeSet` (which implements the same comparison semantics as the
/// upstream shell sort: text/number data-type, ascending/descending order,
/// multi-key tie-breaking).  Simplifications: `data-type`/`order` are read
/// as static attribute values (upstream additionally evaluates them as AVTs
/// when the compiled record marks them dynamic), and locale-aware key
/// generation (`lang`/`case-order`) is not applied.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void
/// xsltDefaultSortFunction(xsltTransformContextPtr ctxt, xmlNodePtr *sorts,
///                         int nbsorts);
/// ```
///
/// # SAFETY
///
/// - `ctxt` and `sorts` must be valid pointers; `sorts` must hold
///   `nbsorts` entries (0 < nbsorts < XSLT_MAX_SORT).
#[no_mangle]
pub unsafe extern "C" fn xsltDefaultSortFunction(
    ctxt: *mut _xsltTransformContext,
    sorts: *mut *mut _xmlNode,
    nbsorts: c_int,
) {
    if ctxt.is_null() || sorts.is_null() || nbsorts <= 0 || nbsorts >= XSLT_MAX_SORT {
        return;
    }
    if (*sorts).is_null() {
        return;
    }
    let list = (*ctxt).nodeList;
    if list.is_null() || (*list).nodeNr <= 1 {
        return; /* nothing to do */
    }

    // Compile the sort key chain (primary = sorts[0]).
    let style = (*ctxt).style;
    let mut chain_head: *mut _xsltSort = ptr::null_mut();
    let mut chain_tail: *mut _xsltSort = ptr::null_mut();
    let mut j: c_int = 0;
    while j < nbsorts {
        let inst = *sorts.offset(j as isize);
        if !inst.is_null() {
            let s = crate::xslt::sorting::xsltCompileSort(style, inst);
            if !s.is_null() {
                if chain_tail.is_null() {
                    chain_head = s;
                    chain_tail = s;
                } else {
                    (*chain_tail).next = s;
                    chain_tail = s;
                }
            }
        }
        j += 1;
    }
    if chain_head.is_null() {
        return; /* sorts[0] failed to compile (upstream: psvi == NULL) */
    }

    crate::xslt::sorting::xsltSortNodeSet(ctxt, list, chain_head);
    crate::xslt::sorting::xsltFreeSortList(chain_head);
}

/// Dispatcher for sorting (upstream `xsltDoSortFunction`, xsltutils.c):
/// call the context-specific sort function if set, otherwise the global
/// sort function (defaulting to [`xsltDefaultSortFunction`]).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void
/// xsltDoSortFunction(xsltTransformContextPtr ctxt, xmlNodePtr *sorts,
///                    int nbsorts);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be a valid pointer; `sorts` may be NULL.
#[no_mangle]
pub unsafe extern "C" fn xsltDoSortFunction(
    ctxt: *mut _xsltTransformContext,
    sorts: *mut *mut _xmlNode,
    nbsorts: c_int,
) {
    // (Upstream dereferences ctxt unconditionally; the null guard is a
    // defensive divergence.)
    if ctxt.is_null() {
        return;
    }
    let sf = (*ctxt).sortfunc;
    if !sf.is_null() {
        let f: xsltSortFunc = core::mem::transmute(sf);
        f(ctxt, sorts, nbsorts);
        return;
    }
    let global = unsafe { XSLT_SORT_FUNCTION };
    if let Some(f) = global {
        f(ctxt, sorts, nbsorts);
    } else {
        xsltDefaultSortFunction(ctxt, sorts, nbsorts);
    }
}

/// Reorder a node-set into document order (upstream
/// `xsltDocumentSortFunction`, xsltutils.c).
///
/// Upstream bubble-sorts by `xmlXPathCmpNodes`, swapping whenever the first
/// node follows the second (`xmlXPathCmpNodes` returns -1 then, using the
/// upstream convention: 1 = node1 precedes node2).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void
/// xsltDocumentSortFunction(xmlNodeSetPtr list);
/// ```
///
/// # SAFETY
///
/// - `list` must be a valid node-set or NULL.
#[no_mangle]
pub unsafe extern "C" fn xsltDocumentSortFunction(list: *mut _xmlNodeSet) {
    if list.is_null() {
        return;
    }
    let len = (*list).nodeNr;
    if len <= 1 {
        return;
    }
    let tab = (*list).nodeTab;
    if tab.is_null() {
        return;
    }
    /* TODO: sort is really not optimized, does it need to be? */
    let mut i: c_int = 0;
    while i < len - 1 {
        let mut j = i + 1;
        while j < len {
            let tst = xmlXPathCmpNodes(*tab.offset(i as isize), *tab.offset(j as isize));
            // -1 ⇒ nodeTab[i] follows nodeTab[j] in document order ⇒ swap
            // (ascending order; upstream xsltutils.c uses `tst == -1`).
            if tst == -1 {
                let node = *tab.offset(i as isize);
                *tab.offset(i as isize) = *tab.offset(j as isize);
                *tab.offset(j as isize) = node;
            }
            j += 1;
        }
        i += 1;
    }
}

/// Compute the sort-key results for the current node-set (upstream
/// `xsltComputeSortResult`, xsltutils.c).
///
/// Evaluates the sort key of the `sort` instruction for every node of
/// `ctxt->nodeList` and returns an array of `len` XPath objects
/// (`XPATH_STRING` for text sorts, `XPATH_NUMBER` for numeric sorts), each
/// carrying its original position in `index`.  The array is allocated with
/// `xmlMalloc`; the caller frees each object with `xmlXPathFreeObject` and
/// the array with `xmlFree`.  Returns NULL on failure (including when the
/// node-set has at most one node, or the sort key has no `select`).
///
/// The candidate compiles the sort key on the fly (`xsltCompileSort`) and
/// evaluates it with the engine's `xsltEvalSortKey` (which evaluates the
/// `select` expression per node with the correct context position/size).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlXPathObjectPtr *
/// xsltComputeSortResult(xsltTransformContextPtr ctxt, xmlNodePtr sort);
/// ```
///
/// # SAFETY
///
/// - `ctxt` and `sort` must be valid pointers.
#[no_mangle]
pub unsafe extern "C" fn xsltComputeSortResult(
    ctxt: *mut _xsltTransformContext,
    sort: *mut _xmlNode,
) -> *mut *mut _xmlXPathObject {
    if ctxt.is_null() || sort.is_null() {
        return ptr::null_mut();
    }
    let style = (*ctxt).style;
    let compiled = crate::xslt::sorting::xsltCompileSort(style, sort);
    if compiled.is_null() {
        emit_generic_error(b"xsl:sort : compilation failed\n\0");
        return ptr::null_mut();
    }
    if (*compiled).select.is_null() {
        crate::xslt::sorting::xsltFreeSortList(compiled);
        return ptr::null_mut();
    }

    let list = (*ctxt).nodeList;
    if list.is_null() || (*list).nodeNr <= 1 {
        crate::xslt::sorting::xsltFreeSortList(compiled);
        return ptr::null_mut();
    }
    let len = (*list).nodeNr;
    let is_number = (*compiled).isText == 0;

    let results = xmlMallocImpl(len as usize * core::mem::size_of::<*mut _xmlXPathObject>())
        as *mut *mut _xmlXPathObject;
    if results.is_null() {
        emit_generic_error(b"xsltComputeSortResult: memory allocation failure\n\0");
        crate::xslt::sorting::xsltFreeSortList(compiled);
        return ptr::null_mut();
    }

    let xpath_ctxt = (*ctxt).xpathCtxt;
    let old_node = (*ctxt).node;
    let (old_pos, old_size) = if xpath_ctxt.is_null() {
        (0, 0)
    } else {
        ((*xpath_ctxt).proximityPosition, (*xpath_ctxt).contextSize)
    };

    let mut i: c_int = 0;
    while i < len {
        let node = *(*list).nodeTab.offset(i as isize);
        (*ctxt).node = node;
        if !xpath_ctxt.is_null() {
            (*xpath_ctxt).node = node;
            (*xpath_ctxt).contextSize = len;
            (*xpath_ctxt).proximityPosition = i + 1;
        }
        let key = crate::xslt::sorting::xsltEvalSortKey(ctxt, node, compiled);
        if key.is_null() {
            (*ctxt).state = crate::xslt::transform::XSLT_STATE_STOPPED;
            *results.offset(i as isize) = ptr::null_mut();
        } else {
            let obj =
                libc::calloc(1, core::mem::size_of::<_xmlXPathObject>()) as *mut _xmlXPathObject;
            if obj.is_null() {
                // Recover: record a NULL slot like upstream does on
                // evaluation failure.
                *results.offset(i as isize) = ptr::null_mut();
                xmlFreeImpl(key as *mut c_void);
            } else if is_number {
                (*obj).type_ = xmlXPathObjectType::XPATH_NUMBER as c_int;
                (*obj).floatval = xmlXPathCastStringToNumber(key);
                (*obj).index = i;
                *results.offset(i as isize) = obj;
                xmlFreeImpl(key as *mut c_void);
            } else {
                (*obj).type_ = xmlXPathObjectType::XPATH_STRING as c_int;
                (*obj).stringval = key;
                (*obj).index = i;
                *results.offset(i as isize) = obj;
            }
        }
        i += 1;
    }

    (*ctxt).node = old_node;
    if !xpath_ctxt.is_null() {
        (*xpath_ctxt).node = old_node;
        (*xpath_ctxt).proximityPosition = old_pos;
        (*xpath_ctxt).contextSize = old_size;
    }
    crate::xslt::sorting::xsltFreeSortList(compiled);
    results
}

/// Set the global sort handler (upstream `xsltSetSortFunc`, xsltutils.c).
///
/// A NULL handler restores the default sort function.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void
/// xsltSetSortFunc(xsltSortFunc handler);
/// ```
///
/// # SAFETY
///
/// - `handler` must be a valid function pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xsltSetSortFunc(handler: Option<xsltSortFunc>) {
    unsafe {
        XSLT_SORT_FUNCTION = if handler.is_some() {
            handler
        } else {
            Some(xsltDefaultSortFunction)
        };
    }
}

/// Set the context-specific sort handler (upstream `xsltSetCtxtSortFunc`,
/// xsltutils.c).  A NULL handler makes [`xsltDoSortFunction`] fall back to
/// the global sort function.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void
/// xsltSetCtxtSortFunc(xsltTransformContextPtr ctxt, xsltSortFunc handler);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be a valid transform context; `handler` may be NULL.
#[no_mangle]
pub unsafe extern "C" fn xsltSetCtxtSortFunc(
    ctxt: *mut _xsltTransformContext,
    handler: Option<xsltSortFunc>,
) {
    if ctxt.is_null() {
        return;
    }
    (*ctxt).sortfunc = match handler {
        Some(f) => f as *const c_void as *mut c_void,
        None => ptr::null_mut(),
    };
}

// ═══════════════════════════════════════════════════════════════════════════════
// Multi-document output (transform.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// Execute the multi-document output extension elements — `saxon:output`,
/// `xalan:write` and `xt:document`/`xsl:document` — (upstream
/// `xsltDocumentElem`, transform.c 1.1.45).
///
/// Ported faithfully: the target filename is computed from the element name
/// (`file`/`href`/`select` attributes, evaluated as AVTs — `select` as an
/// XPath expression), resolved against `ctxt->outputFile` with
/// `xmlBuildURI`, checked against the write security callback, a fresh
/// output stylesheet is created from the `version`/`encoding`/`method`/
/// `doctype-*`/`standalone`/`indent`/`omit-xml-declaration`/
/// `cdata-section-elements`/`append` attributes, the instruction content is
/// instantiated into a new result document, and the document is serialized
/// to the resolved filename.  The context's `output`/`insert`/`type`/
/// `outputFile` are saved and restored around the instantiation.
///
/// Simplifications: the generated-HTML-doctype table (`xsltGetHTMLIDs`,
/// behind `XSLT_GENERATE_HTML_DOCTYPE`) is not reproduced, the
/// `cdata-section-elements` entries are recorded in the output stylesheet's
/// `stripSpaces` hash but are inert in the candidate serializer, and error
/// messages do not interpolate the offending filename (the candidate's
/// non-variadic error API).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void
/// xsltDocumentElem(xsltTransformContextPtr ctxt, xmlNodePtr node,
///                  xmlNodePtr inst, xsltElemPreCompPtr castedComp);
/// ```
///
/// # SAFETY
///
/// - `ctxt`, `node`, `inst` must be valid pointers; `castedComp` may be
///   NULL.
#[no_mangle]
pub unsafe extern "C" fn xsltDocumentElem(
    ctxt: *mut _xsltTransformContext,
    node: *mut _xmlNode,
    inst: *mut _xmlNode,
    castedComp: *mut c_void,
) {
    let _ = castedComp;
    if ctxt.is_null() || node.is_null() || inst.is_null() {
        return;
    }

    let mut ret: c_int;
    let mut URL: *mut xmlChar = ptr::null_mut();
    let mut filename: *mut xmlChar = ptr::null_mut();
    let mut prop: *mut xmlChar;
    let mut style: *mut _xsltStylesheet = ptr::null_mut();
    let mut res: *mut _xmlDoc = ptr::null_mut();
    let mut redirect_write_append: c_int = 0;

    let inst_name = (*inst).name;
    if xmlStrEqual(inst_name, b"output\0".as_ptr() as *const xmlChar) != 0 {
        /*
         * The element "output" is in the namespace XSLT_SAXON_NAMESPACE
         * (http://icl.com/saxon); the @file attribute is in no namespace.
         */
        URL = xsltEvalAttrValueTemplate(
            ctxt,
            inst,
            b"file\0".as_ptr() as *const xmlChar,
            XSLT_SAXON_NAMESPACE.as_ptr() as *const xmlChar,
        );
        if URL.is_null() {
            URL = xsltEvalAttrValueTemplate(
                ctxt,
                inst,
                b"href\0".as_ptr() as *const xmlChar,
                XSLT_SAXON_NAMESPACE.as_ptr() as *const xmlChar,
            );
        }
    } else if xmlStrEqual(inst_name, b"write\0".as_ptr() as *const xmlChar) != 0 {
        URL = xsltEvalAttrValueTemplate(
            ctxt,
            inst,
            b"select\0".as_ptr() as *const xmlChar,
            XSLT_XALAN_NAMESPACE.as_ptr() as *const xmlChar,
        );
        if !URL.is_null() {
            /*
             * Trying to handle bug #59212: the value of the "select"
             * attribute is an XPath expression.
             * (see http://xml.apache.org/xalan-j/extensionslib.html#redirect)
             */
            let obj = crate::xslt::transform::eval_xpath(ctxt, URL);
            let val = if obj.is_null() {
                ptr::null_mut()
            } else {
                let v = xmlXPathCastToString(obj);
                xmlXPathFreeObject(obj);
                v
            };
            xmlFreeImpl(URL as *mut c_void);
            URL = val;
        }
        if URL.is_null() {
            URL = xsltEvalAttrValueTemplate(
                ctxt,
                inst,
                b"file\0".as_ptr() as *const xmlChar,
                XSLT_XALAN_NAMESPACE.as_ptr() as *const xmlChar,
            );
        }
        if URL.is_null() {
            URL = xsltEvalAttrValueTemplate(
                ctxt,
                inst,
                b"href\0".as_ptr() as *const xmlChar,
                XSLT_XALAN_NAMESPACE.as_ptr() as *const xmlChar,
            );
        }
    } else if xmlStrEqual(inst_name, b"document\0".as_ptr() as *const xmlChar) != 0 {
        URL = xsltEvalAttrValueTemplate(
            ctxt,
            inst,
            b"href\0".as_ptr() as *const xmlChar,
            ptr::null(),
        );
    }

    if URL.is_null() {
        crate::xslt::errors::xsltTransformError(
            ctxt,
            ptr::null_mut(),
            inst,
            b"xsltDocumentElem: href/URI-Reference not found\n\0".as_ptr() as *const c_char,
        );
        return;
    }

    /*
     * If the computation failed, it's likely that the URL wasn't escaped.
     */
    filename = xmlBuildURI(URL as *const c_char, (*ctxt).outputFile);
    if filename.is_null() {
        let esc_url = xmlURIEscapeStr(URL, b":/.?,\0".as_ptr() as *const xmlChar);
        if !esc_url.is_null() {
            filename = xmlBuildURI(esc_url as *const c_char, (*ctxt).outputFile);
            xmlFreeImpl(esc_url as *mut c_void);
        }
    }
    if filename.is_null() {
        crate::xslt::errors::xsltTransformError(
            ctxt,
            ptr::null_mut(),
            inst,
            b"xsltDocumentElem: URL computation failed\n\0".as_ptr() as *const c_char,
        );
        xmlFreeImpl(URL as *mut c_void);
        return;
    }

    /*
     * Security checking: can we write to this resource.
     */
    if !(*ctxt).sec.is_null() {
        let check = crate::xslt::security::xsltGetSecurityPrefs(
            (*ctxt).sec,
            crate::xslt::security::XSLT_SECPREF_WRITE_FILE,
        );
        ret = match check {
            Some(f) => f(ctxt as *mut c_void, (*ctxt).sec, filename as *const c_char),
            None => 1,
        };
        if ret <= 0 {
            if ret == 0 {
                crate::xslt::errors::xsltTransformError(
                    ctxt,
                    ptr::null_mut(),
                    inst,
                    b"xsltDocumentElem: write rights denied\n\0".as_ptr() as *const c_char,
                );
            }
            xmlFreeImpl(URL as *mut c_void);
            xmlFreeImpl(filename as *mut c_void);
            return;
        }
    }

    let old_output_file = (*ctxt).outputFile;
    let old_output = (*ctxt).output;
    let old_insert = (*ctxt).insert;
    let old_type = (*ctxt).type_;
    (*ctxt).outputFile = filename as *const c_char;

    style = crate::xslt::stylesheet::xsltStylesheetCreate();
    if style.is_null() {
        crate::xslt::errors::xsltTransformError(
            ctxt,
            ptr::null_mut(),
            inst,
            b"xsltDocumentElem: out of memory\n\0".as_ptr() as *const c_char,
        );
        goto_error(
            ctxt,
            old_output,
            old_insert,
            old_type,
            old_output_file,
            URL,
            filename,
            style,
            res,
        );
        return;
    }

    /*
     * Version described in 1.1 draft allows full parameterization of the
     * output.
     */
    prop = xsltEvalAttrValueTemplate(
        ctxt,
        inst,
        b"version\0".as_ptr() as *const xmlChar,
        ptr::null(),
    );
    if !prop.is_null() {
        if !(*style).version.is_null() {
            xmlFreeImpl((*style).version as *mut c_void);
        }
        (*style).version = prop;
    }
    prop = xsltEvalAttrValueTemplate(
        ctxt,
        inst,
        b"encoding\0".as_ptr() as *const xmlChar,
        ptr::null(),
    );
    if !prop.is_null() {
        if !(*style).encoding.is_null() {
            xmlFreeImpl((*style).encoding as *mut c_void);
        }
        (*style).encoding = prop;
    }
    prop = xsltEvalAttrValueTemplate(
        ctxt,
        inst,
        b"method\0".as_ptr() as *const xmlChar,
        ptr::null(),
    );
    if !prop.is_null() {
        let mut method_name: *mut xmlChar = prop;
        let uri = xsltGetQNameURI(inst, &mut method_name);
        if method_name.is_null() {
            (*style).errors += 1;
        } else if uri.is_null() {
            if xmlStrEqual(method_name, b"xml\0".as_ptr() as *const xmlChar) != 0
                || xmlStrEqual(method_name, b"html\0".as_ptr() as *const xmlChar) != 0
                || xmlStrEqual(method_name, b"text\0".as_ptr() as *const xmlChar) != 0
            {
                (*style).method = method_name;
            } else {
                crate::xslt::errors::xsltTransformError(
                    ctxt,
                    ptr::null_mut(),
                    inst,
                    b"invalid value for method\n\0".as_ptr() as *const c_char,
                );
                (*style).warnings += 1;
                xmlFreeImpl(method_name as *mut c_void);
            }
        } else {
            (*style).method = method_name;
            (*style).methodURI = xmlStrdup(uri);
        }
    }
    prop = xsltEvalAttrValueTemplate(
        ctxt,
        inst,
        b"doctype-system\0".as_ptr() as *const xmlChar,
        ptr::null(),
    );
    if !prop.is_null() {
        if !(*style).doctypeSystem.is_null() {
            xmlFreeImpl((*style).doctypeSystem as *mut c_void);
        }
        (*style).doctypeSystem = prop;
    }
    prop = xsltEvalAttrValueTemplate(
        ctxt,
        inst,
        b"doctype-public\0".as_ptr() as *const xmlChar,
        ptr::null(),
    );
    if !prop.is_null() {
        if !(*style).doctypePublic.is_null() {
            xmlFreeImpl((*style).doctypePublic as *mut c_void);
        }
        (*style).doctypePublic = prop;
    }
    prop = xsltEvalAttrValueTemplate(
        ctxt,
        inst,
        b"standalone\0".as_ptr() as *const xmlChar,
        ptr::null(),
    );
    if !prop.is_null() {
        if xmlStrEqual(prop, b"yes\0".as_ptr() as *const xmlChar) != 0 {
            (*style).standalone = 1;
        } else if xmlStrEqual(prop, b"no\0".as_ptr() as *const xmlChar) != 0 {
            (*style).standalone = 0;
        } else {
            crate::xslt::errors::xsltTransformError(
                ctxt,
                ptr::null_mut(),
                inst,
                b"invalid value for standalone\n\0".as_ptr() as *const c_char,
            );
            (*style).warnings += 1;
        }
        xmlFreeImpl(prop as *mut c_void);
    }
    prop = xsltEvalAttrValueTemplate(
        ctxt,
        inst,
        b"indent\0".as_ptr() as *const xmlChar,
        ptr::null(),
    );
    if !prop.is_null() {
        if xmlStrEqual(prop, b"yes\0".as_ptr() as *const xmlChar) != 0 {
            (*style).indent = 1;
        } else if xmlStrEqual(prop, b"no\0".as_ptr() as *const xmlChar) != 0 {
            (*style).indent = 0;
        } else {
            crate::xslt::errors::xsltTransformError(
                ctxt,
                ptr::null_mut(),
                inst,
                b"invalid value for indent\n\0".as_ptr() as *const c_char,
            );
            (*style).warnings += 1;
        }
        xmlFreeImpl(prop as *mut c_void);
    }
    prop = xsltEvalAttrValueTemplate(
        ctxt,
        inst,
        b"omit-xml-declaration\0".as_ptr() as *const xmlChar,
        ptr::null(),
    );
    if !prop.is_null() {
        if xmlStrEqual(prop, b"yes\0".as_ptr() as *const xmlChar) != 0 {
            (*style).omitXmlDeclaration = 1;
        } else if xmlStrEqual(prop, b"no\0".as_ptr() as *const xmlChar) != 0 {
            (*style).omitXmlDeclaration = 0;
        } else {
            crate::xslt::errors::xsltTransformError(
                ctxt,
                ptr::null_mut(),
                inst,
                b"invalid value for omit-xml-declaration\n\0".as_ptr() as *const c_char,
            );
            (*style).warnings += 1;
        }
        xmlFreeImpl(prop as *mut c_void);
    }

    /*
     * cdata-section-elements: upstream stores "cdata" entries in the
     * output stylesheet's stripSpaces hash.  The candidate serializer does
     * not consult it, but the parsing is kept for fidelity.
     */
    let elements = xsltEvalAttrValueTemplate(
        ctxt,
        inst,
        b"cdata-section-elements\0".as_ptr() as *const xmlChar,
        ptr::null(),
    );
    if !elements.is_null() {
        if (*style).stripSpaces.is_null() {
            (*style).stripSpaces = crate::xml::hash::hash_create(10) as *mut c_void;
        }
        if !(*style).stripSpaces.is_null() {
            let mut element: *mut xmlChar = elements;
            while *element != 0 {
                while crate::xml::chvalid::xmlIsBlank(*element as c_uint) != 0 {
                    element = element.add(1);
                }
                if *element == 0 {
                    break;
                }
                let mut end = element;
                while *end != 0 && crate::xml::chvalid::xmlIsBlank(*end as c_uint) == 0 {
                    end = end.add(1);
                }
                let mut el = xmlStrndup(element, (end as isize - element as isize) as c_int);
                if !el.is_null() {
                    let uri = xsltGetQNameURI(inst, &mut el);
                    crate::xml::hash::hash_add_entry2(
                        (*style).stripSpaces as *mut crate::xml::hash::HashTable,
                        el,
                        uri,
                        b"cdata\0".as_ptr() as *mut c_void,
                    );
                    xmlFreeImpl(el as *mut c_void);
                }
                element = end;
            }
        }
        xmlFreeImpl(elements as *mut c_void);
    }

    /*
     * Create a new document tree and process the element template.
     */
    let method: *const xmlChar = (*style).method;
    let doctype_public: *const xmlChar = (*style).doctypePublic;
    let doctype_system: *const xmlChar = (*style).doctypeSystem;
    let version: *const xmlChar = (*style).version;
    let encoding: *const xmlChar = (*style).encoding;

    if !method.is_null() && xmlStrEqual(method, b"xml\0".as_ptr() as *const xmlChar) == 0 {
        if xmlStrEqual(method, b"html\0".as_ptr() as *const xmlChar) != 0 {
            (*ctxt).type_ = XSLT_OUTPUT_HTML;
            if !doctype_public.is_null() || !doctype_system.is_null() {
                res = htmlNewDoc(doctype_system, doctype_public);
            } else {
                res = htmlNewDocNoDtD(doctype_system, doctype_public);
            }
        } else if xmlStrEqual(method, b"xhtml\0".as_ptr() as *const xmlChar) != 0 {
            crate::xslt::errors::xsltTransformError(
                ctxt,
                ptr::null_mut(),
                inst,
                b"xsltDocumentElem: unsupported method xhtml\n\0".as_ptr() as *const c_char,
            );
            (*ctxt).type_ = XSLT_OUTPUT_HTML;
            res = htmlNewDocNoDtD(doctype_system, doctype_public);
        } else if xmlStrEqual(method, b"text\0".as_ptr() as *const xmlChar) != 0 {
            (*ctxt).type_ = XSLT_OUTPUT_TEXT;
            res = xmlNewDoc(version);
        } else {
            crate::xslt::errors::xsltTransformError(
                ctxt,
                ptr::null_mut(),
                inst,
                b"xsltDocumentElem: unsupported method\n\0".as_ptr() as *const c_char,
            );
            goto_error(
                ctxt,
                old_output,
                old_insert,
                old_type,
                old_output_file,
                URL,
                filename,
                style,
                res,
            );
            return;
        }
    } else {
        (*ctxt).type_ = XSLT_OUTPUT_XML;
        res = xmlNewDoc(version);
    }
    if res.is_null() {
        goto_error(
            ctxt,
            old_output,
            old_insert,
            old_type,
            old_output_file,
            URL,
            filename,
            style,
            res,
        );
        return;
    }
    (*res).charset = XML_CHAR_ENCODING_UTF8;
    if !encoding.is_null() {
        (*res).encoding = xmlStrdup(encoding);
    }
    (*ctxt).output = res;
    (*ctxt).insert = res as *mut _xmlNode;
    (*ctxt).node = node;
    crate::xslt::transform::execute_content(ctxt, (*inst).children);

    /*
     * Do some post processing work depending on the generated output.
     */
    let root = xmlDocGetRootElement(res);
    if !root.is_null() {
        let mut doctype: *const xmlChar = ptr::null();

        if !(*root).ns.is_null() && !(*(*root).ns).prefix.is_null() {
            if !(*ctxt).dict.is_null() {
                /* xmlDictQLookup: intern "prefix:name" in the dict. */
                let plen = crate::xml::string::xml_strlen((*(*root).ns).prefix);
                let nlen = crate::xml::string::xml_strlen((*root).name);
                let mut qn = Vec::with_capacity(plen + 1 + nlen);
                qn.extend_from_slice(core::slice::from_raw_parts(
                    (*(*root).ns).prefix as *const u8,
                    plen,
                ));
                qn.push(b':');
                qn.extend_from_slice(core::slice::from_raw_parts((*root).name as *const u8, nlen));
                qn.push(0);
                doctype = crate::xml::dictionary::dict_lookup(
                    (*ctxt).dict as *mut crate::xml::dictionary::Dict,
                    qn.as_ptr() as *const xmlChar,
                    (plen + 1 + nlen) as c_int,
                );
            }
        }
        if doctype.is_null() {
            doctype = (*root).name;
        }

        /*
         * Apply the default selection of the method.
         */
        if method.is_null()
            && (*root).ns.is_null()
            && libc::strcasecmp(
                (*root).name as *const c_char,
                b"html\0".as_ptr() as *const c_char,
            ) == 0
        {
            let mut tmp = (*res).children;
            while !tmp.is_null() && tmp != root {
                if (*tmp).type_ == XML_ELEMENT_NODE as c_int {
                    break;
                }
                if (*tmp).type_ == XML_TEXT_NODE as c_int
                    && crate::xml::chvalid::xmlIsBlankNode(tmp) == 0
                {
                    break;
                }
                tmp = (*tmp).next;
            }
            if tmp == root {
                (*ctxt).type_ = XSLT_OUTPUT_HTML;
                (*res).type_ = XML_HTML_DOCUMENT_NODE as c_int;
                if !doctype_public.is_null() || !doctype_system.is_null() {
                    (*res).intSubset =
                        xmlCreateIntSubset(res, doctype, doctype_public, doctype_system);
                }
            }
        }
        if (*ctxt).type_ == XSLT_OUTPUT_XML {
            if !doctype_public.is_null() || !doctype_system.is_null() {
                (*res).intSubset = xmlCreateIntSubset(res, doctype, doctype_public, doctype_system);
            }
        }
    }

    /*
     * Calls to redirect:write also take an optional attribute append.
     * append="true|yes" appends to an existing file instead of always
     * opening a new one.
     */
    prop = xsltEvalAttrValueTemplate(
        ctxt,
        inst,
        b"append\0".as_ptr() as *const xmlChar,
        ptr::null(),
    );
    if !prop.is_null() {
        if xmlStrEqual(prop, b"true\0".as_ptr() as *const xmlChar) != 0
            || xmlStrEqual(prop, b"yes\0".as_ptr() as *const xmlChar) != 0
        {
            (*style).omitXmlDeclaration = 1;
            redirect_write_append = 1;
        } else {
            (*style).omitXmlDeclaration = 0;
        }
        xmlFreeImpl(prop as *mut c_void);
    }

    if redirect_write_append != 0 {
        let f = libc::fopen(filename as *const c_char, b"ab\0".as_ptr() as *const c_char);
        if f.is_null() {
            ret = -1;
        } else {
            ret = crate::xslt::serialization::xsltSaveResultToFile(f as *mut c_void, res, style);
            libc::fclose(f);
        }
    } else {
        ret = crate::xslt::serialization::xsltSaveResultToFilename(
            filename as *const c_char,
            res,
            style,
            0,
        );
    }
    if ret < 0 {
        crate::xslt::errors::xsltTransformError(
            ctxt,
            ptr::null_mut(),
            inst,
            b"xsltDocumentElem: unable to save result\n\0".as_ptr() as *const c_char,
        );
    }

    goto_error(
        ctxt,
        old_output,
        old_insert,
        old_type,
        old_output_file,
        URL,
        filename,
        style,
        res,
    );
}

/// Restore the transform context fields saved by `xsltDocumentElem` and
/// release its temporaries (upstream `error:` label).
#[allow(clippy::too_many_arguments)]
unsafe fn goto_error(
    ctxt: *mut _xsltTransformContext,
    old_output: *mut _xmlDoc,
    old_insert: *mut _xmlNode,
    old_type: c_int,
    old_output_file: *const c_char,
    url: *mut xmlChar,
    filename: *mut xmlChar,
    style: *mut _xsltStylesheet,
    res: *mut _xmlDoc,
) {
    (*ctxt).output = old_output;
    (*ctxt).insert = old_insert;
    (*ctxt).type_ = old_type;
    (*ctxt).outputFile = old_output_file;
    if !url.is_null() {
        xmlFreeImpl(url as *mut c_void);
    }
    if !filename.is_null() {
        xmlFreeImpl(filename as *mut c_void);
    }
    if !style.is_null() {
        crate::xslt::stylesheet::xsltFreeStylesheet(style);
    }
    if !res.is_null() {
        crate::xml::tree::free_doc(res);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Debug (extra.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// Process a `libxslt:debug` element (upstream `xsltDebug`, extra.c):
/// dump the last 15 entries of the template stack and the variable stack
/// through the generic error handler.
///
/// Ported faithfully.  The upstream `xmlXPathDebugDumpObject` dump of
/// variable values (guarded by `LIBXML_DEBUG_ENABLED`) is not reproduced —
/// the candidate lacks that debug API; the text output is identical for the
/// common case.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void
/// xsltDebug(xsltTransformContextPtr ctxt, xmlNodePtr node ATTRIBUTE_UNUSED,
///           xmlNodePtr inst ATTRIBUTE_UNUSED,
///           xsltElemPreCompPtr comp ATTRIBUTE_UNUSED);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be a valid transform context.
#[no_mangle]
pub unsafe extern "C" fn xsltDebug(
    ctxt: *mut _xsltTransformContext,
    node: *mut _xmlNode,
    inst: *mut _xmlNode,
    comp: *mut c_void,
) {
    let _ = (node, inst, comp);
    if ctxt.is_null() {
        return;
    }

    let mut i: c_int = 0;
    let mut j = (*ctxt).templNr - 1;
    emit_generic_error(b"Templates:\n\0");
    if !(*ctxt).templTab.is_null() {
        while i < 15 && j >= 0 {
            let msg = format!("#{} ", i);
            emit_generic_error(msg.as_bytes());
            let t = *(*ctxt).templTab.offset(j as isize);
            if !t.is_null() {
                if !(*t).name.is_null() {
                    let m = format!("name {} ", bytes_to_lossy((*t).name));
                    emit_generic_error(m.as_bytes());
                }
                if !(*t).r#match.is_null() {
                    // NOTE: upstream prints "name %s" for the match pattern
                    // too (extra.c quirk).
                    let m = format!("name {} ", bytes_to_lossy((*t).r#match));
                    emit_generic_error(m.as_bytes());
                }
                if !(*t).mode.is_null() {
                    let m = format!("name {} ", bytes_to_lossy((*t).mode));
                    emit_generic_error(m.as_bytes());
                }
            }
            emit_generic_error(b"\n\0");
            i += 1;
            j -= 1;
        }
    }

    emit_generic_error(b"Variables:\n\0");
    i = 0;
    j = (*ctxt).varsNr - 1;
    if !(*ctxt).varsTab.is_null() {
        while i < 15 && j >= 0 {
            let cur = *(*ctxt).varsTab.offset(j as isize);
            if cur.is_null() {
                j -= 1;
                i += 1;
                continue;
            }
            let msg = format!("#{}\n", i);
            emit_generic_error(msg.as_bytes());
            let mut p = cur;
            while !p.is_null() {
                if (*p).comp.is_null() {
                    emit_generic_error(b"corrupted !!!\n\0");
                } else {
                    let typ = *((*p).comp as *const c_int);
                    if typ == XSLT_FUNC_PARAM {
                        emit_generic_error(b"param \0");
                    } else if typ == XSLT_FUNC_VARIABLE {
                        emit_generic_error(b"var \0");
                    }
                }
                if !(*p).name.is_null() {
                    let m = format!("{} ", bytes_to_lossy((*p).name));
                    emit_generic_error(m.as_bytes());
                } else {
                    emit_generic_error(b"noname !!!!\n\0");
                }
                p = (*p).next;
            }
            j -= 1;
            i += 1;
        }
    }
}

/// Render a NUL-terminated xmlChar string as a lossy Rust string (for the
/// debug dumps).
unsafe fn bytes_to_lossy(s: *const xmlChar) -> String {
    if s.is_null() {
        return String::new();
    }
    let len = libc::strlen(s as *const c_char) as usize;
    String::from_utf8_lossy(core::slice::from_raw_parts(s, len)).into_owned()
}
