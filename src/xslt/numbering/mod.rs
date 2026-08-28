//! XSLT numbering (§33, §85 Phase 8).
//!
//! `<xsl:number>` computes and formats numbers for output. Supported
//! formats include decimal ("1"), alphabetic ("a", "A"), and roman
//! ("i", "I") sequences.
//!
//! # UPSTREAM-PARITY
//!
//! Upstream libxslt (numbers.c) implements XSLT 1.0 §7.7 number
//! formatting. Format tokens:
//!
//! - `1` — decimal digits (1, 2, 3, ...)
//! - `01`, `001` — zero-padded decimal
//! - `a` — lowercase alphabetic (a, b, c, ...)
//! - `A` — uppercase alphabetic (A, B, C, ...)
//! - `i` — lowercase roman (i, ii, iii, ...)
//! - `I` — uppercase roman (I, II, III, ...)
//!
//! Multiple tokens separated by punctuation (e.g. "1.1") produce
//! hierarchical numbers (e.g. "1.2.3").

use crate::abi::types::*;
use std::ptr;

/// Format a number according to the format string.
///
/// Returns a heap-allocated NUL-terminated string, or NULL on error.
/// The caller frees with `libc::free`.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltNumberFormat(xsltTransformContextPtr ctxt, xsltNumberDataPtr data,
///                       xmlNodePtr node);
/// ```
///
/// Simplified single-number formatting used by the transform engine.
pub unsafe fn xsltFormatNumber(number: f64, format: *const xmlChar) -> *mut xmlChar {
    if format.is_null() {
        return alloc_result(&format_decimal(number, 1));
    }
    let bytes =
        core::slice::from_raw_parts(format, libc::strlen(format as *const libc::c_char) as usize);
    if bytes.is_empty() {
        return alloc_result(&format_decimal(number, 1));
    }
    // Determine the format token from the first character.
    let token = bytes[0];
    let mut token_len = 1usize;
    // Count the full token run (consecutive same characters or digits).
    match token {
        b'a' | b'A' | b'i' | b'I' => {
            while token_len < bytes.len() && bytes[token_len] == token {
                token_len += 1;
            }
        }
        b'0' | b'1'..=b'9' => {
            while token_len < bytes.len() && bytes[token_len].is_ascii_digit() {
                token_len += 1;
            }
        }
        _ => {}
    }
    let result: Vec<u8> = match token {
        b'a' => format_alphabetic(number, false),
        b'A' => format_alphabetic(number, true),
        b'i' => format_roman(number, false),
        b'I' => format_roman(number, true),
        b'0' | b'1'..=b'9' => {
            // The format token is a run of digit characters; the number of
            // characters determines the minimum output width (XSLT 1.0
            // §7.7.1: zero-padded decimal). E.g. "1" → width 1, "01" →
            // width 2, "001" → width 3.
            format_decimal(number, token_len)
        }
        _ => format_decimal(number, 1),
    };
    // Append any literal suffix after the token.
    let mut out = result;
    let mut idx = token_len;
    while idx < bytes.len() {
        out.push(bytes[idx]);
        idx += 1;
    }
    alloc_result(&out)
}

/// Allocate a NUL-terminated heap string from a byte buffer.
unsafe fn alloc_result(bytes: &[u8]) -> *mut xmlChar {
    let p = libc::malloc(bytes.len() + 1) as *mut xmlChar;
    if p.is_null() {
        return ptr::null_mut();
    }
    if !bytes.is_empty() {
        libc::memcpy(
            p as *mut libc::c_void,
            bytes.as_ptr() as *const libc::c_void,
            bytes.len(),
        );
    }
    *p.add(bytes.len()) = 0;
    p
}

/// Format a number as decimal digits with the given minimum width.
unsafe fn format_decimal(number: f64, min_width: usize) -> Vec<u8> {
    let n = number.round();
    let mut s = if n < 0.0 {
        format!("{}", (n.abs() as u64))
    } else {
        format!("{}", (n as u64))
    };
    while s.len() < min_width {
        s.insert(0, '0');
    }
    if n < 0.0 {
        s.insert(0, '-');
    }
    s.into_bytes()
}

/// Format a number as alphabetic (a, b, ..., z, aa, ab, ...).
unsafe fn format_alphabetic(number: f64, upper: bool) -> Vec<u8> {
    let mut n = number.round() as u64;
    let base = if upper { b'A' } else { b'a' };
    if n == 0 {
        return Vec::new();
    }
    // Bijective base-26: 1 → a, 26 → z, 27 → aa.
    let mut out: Vec<u8> = Vec::new();
    while n > 0 {
        let rem = ((n - 1) % 26) as u8;
        out.push(base + rem);
        n = (n - 1) / 26;
    }
    out.reverse();
    out
}

/// Format a number as Roman numerals.
unsafe fn format_roman(number: f64, upper: bool) -> Vec<u8> {
    let mut n = number.round() as u64;
    if n == 0 || n > 4999 {
        return format_decimal(number, 1);
    }
    let vals: [(u64, &[u8]); 13] = [
        (1000, b"M"),
        (900, b"CM"),
        (500, b"D"),
        (400, b"CD"),
        (100, b"C"),
        (90, b"XC"),
        (50, b"L"),
        (40, b"XL"),
        (10, b"X"),
        (9, b"IX"),
        (5, b"V"),
        (4, b"IV"),
        (1, b"I"),
    ];
    let mut out: Vec<u8> = Vec::new();
    for (val, sym) in vals.iter() {
        while n >= *val {
            out.extend_from_slice(sym);
            n -= *val;
        }
    }
    if upper {
        out.iter_mut().for_each(|b| *b = b.to_ascii_uppercase());
    } else {
        out.iter_mut().for_each(|b| *b = b.to_ascii_lowercase());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::ptr;

    fn free_str(p: *mut xmlChar) {
        if !p.is_null() {
            unsafe { libc::free(p as *mut libc::c_void) };
        }
    }

    fn to_string(p: *mut xmlChar) -> String {
        if p.is_null() {
            return String::new();
        }
        unsafe {
            let bytes =
                core::slice::from_raw_parts(p, libc::strlen(p as *const libc::c_char) as usize);
            String::from_utf8_lossy(bytes).into_owned()
        }
    }

    #[test]
    fn test_format_decimal() {
        unsafe {
            let s = xsltFormatNumber(42.0, b"1\0".as_ptr() as *const xmlChar);
            assert_eq!(to_string(s), "42");
            free_str(s);
        }
    }

    #[test]
    fn test_format_padded() {
        unsafe {
            let s = xsltFormatNumber(7.0, b"01\0".as_ptr() as *const xmlChar);
            assert_eq!(to_string(s), "07");
            free_str(s);
        }
    }

    #[test]
    fn test_format_alphabetic() {
        unsafe {
            let s = xsltFormatNumber(1.0, b"a\0".as_ptr() as *const xmlChar);
            assert_eq!(to_string(s), "a");
            free_str(s);
            let s = xsltFormatNumber(27.0, b"a\0".as_ptr() as *const xmlChar);
            assert_eq!(to_string(s), "aa");
            free_str(s);
            let s = xsltFormatNumber(1.0, b"A\0".as_ptr() as *const xmlChar);
            assert_eq!(to_string(s), "A");
            free_str(s);
        }
    }

    #[test]
    fn test_format_roman() {
        unsafe {
            let s = xsltFormatNumber(4.0, b"i\0".as_ptr() as *const xmlChar);
            assert_eq!(to_string(s), "iv");
            free_str(s);
            let s = xsltFormatNumber(9.0, b"I\0".as_ptr() as *const xmlChar);
            assert_eq!(to_string(s), "IX");
            free_str(s);
            let s = xsltFormatNumber(1999.0, b"I\0".as_ptr() as *const xmlChar);
            assert_eq!(to_string(s), "MCMXCIX");
            free_str(s);
        }
    }

    #[test]
    fn test_format_null() {
        unsafe {
            let s = xsltFormatNumber(5.0, ptr::null());
            assert_eq!(to_string(s), "5");
            free_str(s);
        }
    }
}
