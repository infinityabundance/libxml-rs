//! XSLT numbering (§33, §85 Phase 8).
//!
//! # Upstream contract
//!
//! Parity target: upstream libxslt `numbers.c` (1.1.45;
//! `SRC-LIBXSLT-1.1.42-NUMBERS-C` under oracle/historical/src). Subsystem
//! census: xslt-numbering. The observable surface is `xsltNumberFormat`/
//! `xsltNumberFormatInsert` (xsl:number), `xsltFormatNumberConversion` /
//! `xsltFormatNumberFunction` (format-number), and the default decimal
//! format (xslt.c `xsltNewDecimalFormat`).
//!
//! # Conceptual behavior
//!
//! `xsl:number` computes a level/from/count sequence and formats it with
//! the token grammar above (XSLT 1.0 §7.7). `format-number()` implements
//! the JDK 1.1 DecimalFormat picture language: `#`/`0` digit positions,
//! grouping separators, decimal point, percent/permille multipliers, the
//! sub-picture separator `;` for negative patterns, and the single-quote
//! literal escape character. The port keeps upstream quirks: the default
//! decimal format when no xsl:decimal-format matches, the
//! `is_negative_pattern` field set but never read, and the UTF-8 character
//! comparisons (`xsltUTF8Charcmp`).
//!
//! # Ownership & safety invariants
//!
//! The picture parsers borrow the input strings and the `_xsltDecimalFormat`
//! (owned by the stylesheet chain); output buffers are caller-owned
//! (`xsltFormatNumberConversion` writes into a caller buffer, matching
//! the upstream `caller frees` contract). The default character tables
//! (`DEF_*`) are static; nothing here allocates beyond the output buffer.
//! `fmt_char` resolves a format field or its default — never dereferences
//! a NULL `fmt`.
//!
//! # Historical quirks & epochs
//!
//! E-008 (atlas/SEMANTIC_EPOCHS.md): xsltproc `num` output is
//! byte-identical from libxslt 1.1.26 (2009) through 1.1.45. R-000166
//! (11.1-P) found format-number(1234567.891, `#,##0.00`) empty and
//! value-of at full double precision; the canonical numbers.c port
//! (R-000163) closed both, pinned by CLI-XSLTPROC-0014/0015/0017 and
//! test_xml_number_to_string_parity_cases.
//!
//! # Deliberate oddities
//!
//! - The `is_negative_pattern` info field is set but not read by
//!   `xsltFormatNumberConversion` — kept for fidelity to upstream
//!   `_xsltFormatNumberInfo` (annotated in the struct).
//! - Percent/permille handling keeps upstream multiplier semantics rather
//!   than the simpler single-percent behavior.
//!
//! # Proving courts
//!
//! CLI-XSLTPROC-0014, CLI-XSLTPROC-0015, CLI-XSLTPROC-0017 (format-number
//! corpus), XSLT-001 (xsltFormatNumberConversion via the xslt-family
//! probe), test_xml_number_to_string_parity_cases, and the in-crate
//! `cargo test` suites.
//!
//! # Tempting simplifications that would break parity
//!
//! - Replacing the picture parser with a formatting crate breaks the
//!   JDK 1.1 sub-picture, grouping, and percent semantics the oracle
//!   reproduces (R-000166).
//! - Using Rust `{:.*}` precision for value-of breaks the full-precision
//!   epoch output (R-000166: `1234567.891000000061467` vs `1234567.891`).
//! - Dropping the `;` negative sub-picture breaks negative format-number
//!   output.
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
//!
//! This module also hosts the faithful port of upstream `format-number()`
//! machinery (numbers.c 1.1.45): `xsltFormatNumberConversion` and the
//! picture-parsing helpers it uses. Every ABI entry point that formats a
//! number with a decimal-format picture (`xsltFormatNumberConversion`,
//! `xsltFormatNumberFunction`, the internal XSLT evaluator
//! `format-number()`) delegates here so there is exactly one
//! implementation of the JDK 1.1 DecimalFormat algorithm.

use crate::abi::structs::{_xsltDecimalFormat, _xsltStylesheet};
use crate::abi::types::*;
use libc::{c_char, c_int};
use std::ptr;

/// The quote character that escapes literals in a decimal-format picture
/// (upstream numbers.c SYMBOL_QUOTE).
const SYMBOL_QUOTE: u8 = b'\'';

/// Format information accumulated while parsing a decimal-format picture
/// (upstream `_xsltFormatNumberInfo`).
#[derive(Default, Clone, Copy)]
struct FormatNumberInfo {
    integer_hash: i32,
    integer_digits: i32,
    frac_digits: i32,
    frac_hash: i32,
    group: i32,
    multiplier: i32,
    add_decimal: bool,
    is_multiplier_set: bool,
    /// Mirrors upstream `_xsltFormatNumberInfo.is_negative_pattern` (set but
    /// not read by `xsltFormatNumberConversion`; kept for fidelity).
    #[allow(dead_code)]
    is_negative_pattern: bool,
}

/// NUL-terminated default decimal-format characters (upstream
/// `xsltNewDecimalFormat`, xslt.c). These are the fallback values used when
/// `self` is NULL or a field is NULL — the same characters upstream's
/// default `xsltDecimalFormat` carries.
#[allow(dead_code)]
static DEF_DIGIT: [u8; 2] = *b"#\0";
static DEF_PATTERN_SEP: [u8; 2] = *b";\0";
static DEF_MINUS: [u8; 2] = *b"-\0";
static DEF_INFINITY: [u8; 9] = *b"Infinity\0";
static DEF_NAN: [u8; 4] = *b"NaN\0";
static DEF_DECIMAL_POINT: [u8; 2] = *b".\0";
static DEF_GROUPING: [u8; 2] = *b",\0";
static DEF_PERCENT: [u8; 2] = *b"%\0";
static DEF_PERMILLE: [u8; 4] = [0xE2, 0x80, 0xB0, 0]; // U+2030 ‰
static DEF_ZERO_DIGIT: [u8; 2] = *b"0\0";
static DEF_DIGIT_CHAR: [u8; 2] = *b"#\0";

/// Resolve a decimal-format character as a NUL-terminated string: the
/// format's field if non-NULL (with a non-NULL `self`), else the default.
unsafe fn fmt_char(
    fmt: *mut _xsltDecimalFormat,
    get: unsafe fn(*mut _xsltDecimalFormat) -> *mut xmlChar,
    default: &'static [u8],
) -> *const xmlChar {
    unsafe {
        if fmt.is_null() {
            return default.as_ptr() as *const xmlChar;
        }
        let p = get(fmt);
        if p.is_null() {
            default.as_ptr() as *const xmlChar
        } else {
            p
        }
    }
}

/// Compare the UTF-8 character at `cur` with the first UTF-8 character of
/// the NUL-terminated string `s` (upstream `xsltUTF8Charcmp`).
unsafe fn utf8_char_cmp(cur: *const xmlChar, s: *const xmlChar) -> bool {
    unsafe {
        if cur.is_null() || s.is_null() {
            return false;
        }
        let n = crate::abi::exports_string::xmlUTF8Strsize(cur, 1);
        if n < 1 {
            return false;
        }
        libc::strncmp(cur as *const c_char, s as *const c_char, n as usize) == 0
    }
}

/// Whether the UTF-8 character at `cur` is a picture "special" character
/// (upstream IS_SPECIAL: zeroDigit, digit, decimalPoint, grouping,
/// patternSeparator).
unsafe fn is_special(
    _fmt: *mut _xsltDecimalFormat,
    cur: *const xmlChar,
    decimal_point: *const xmlChar,
    grouping: *const xmlChar,
    zero_digit: *const xmlChar,
    digit: *const xmlChar,
    pattern_sep: *const xmlChar,
) -> bool {
    unsafe {
        utf8_char_cmp(cur, zero_digit)
            || utf8_char_cmp(cur, digit)
            || utf8_char_cmp(cur, decimal_point)
            || utf8_char_cmp(cur, grouping)
            || utf8_char_cmp(cur, pattern_sep)
    }
}

/// Process the prefix/suffix of a decimal-format picture (upstream
/// `xsltFormatNumberPreSuffix`, numbers.c); returns the length in **bytes**
/// (excluding quote characters) or -1 on error. `format` is advanced past
/// the consumed characters.
#[allow(clippy::too_many_arguments)]
unsafe fn pre_suffix(
    fmt: *mut _xsltDecimalFormat,
    format: &mut *const xmlChar,
    info: &mut FormatNumberInfo,
    decimal_point: *const xmlChar,
    grouping: *const xmlChar,
    zero_digit: *const xmlChar,
    digit: *const xmlChar,
    pattern_sep: *const xmlChar,
    percent: *const xmlChar,
    permille: *const xmlChar,
) -> c_int {
    unsafe {
        let mut count: c_int = 0;
        loop {
            if **format == 0 {
                return count;
            }
            // An escaped character (quoted) is counted but not interpreted.
            if **format == SYMBOL_QUOTE {
                *format = format.add(1);
                if **format == 0 {
                    return -1;
                }
            } else if is_special(
                fmt,
                *format,
                decimal_point,
                grouping,
                zero_digit,
                digit,
                pattern_sep,
            ) {
                return count;
            } else if utf8_char_cmp(*format, percent) {
                if info.is_multiplier_set {
                    return -1;
                }
                info.multiplier = 100;
                info.is_multiplier_set = true;
            } else if utf8_char_cmp(*format, permille) {
                if info.is_multiplier_set {
                    return -1;
                }
                info.multiplier = 1000;
                info.is_multiplier_set = true;
            }
            let len = crate::abi::exports_string::xmlUTF8Strsize(*format, 1);
            if len < 1 {
                return -1;
            }
            count += len;
            *format = format.add(len as usize);
        }
    }
}

/// Encode a Unicode code point as UTF-8 (upstream `xsltCopyCharMultiByte`).
/// Returns the number of bytes written (0 for invalid code points).
const unsafe fn encode_utf8(out: &mut [u8], val: u32) -> usize {
    {
        if val < 0x80 {
            out[0] = val as u8;
            1
        } else if val < 0x800 {
            out[0] = ((val >> 6) | 0xC0) as u8;
            out[1] = ((val & 0x3F) | 0x80) as u8;
            2
        } else if val < 0x10000 {
            out[0] = ((val >> 12) | 0xE0) as u8;
            out[1] = (((val >> 6) & 0x3F) | 0x80) as u8;
            out[2] = ((val & 0x3F) | 0x80) as u8;
            3
        } else if val < 0x110000 {
            out[0] = ((val >> 18) | 0xF0) as u8;
            out[1] = (((val >> 12) & 0x3F) | 0x80) as u8;
            out[2] = (((val >> 6) & 0x3F) | 0x80) as u8;
            out[3] = ((val & 0x3F) | 0x80) as u8;
            4
        } else {
            0
        }
    }
}

/// Decode the first UTF-8 character at `p` (upstream `xsltGetUTF8Char`);
/// returns the code point, or -1 on error. `*len` receives the byte length
/// of the character.
unsafe fn get_utf8_char(p: *const xmlChar, len: &mut c_int) -> c_int {
    unsafe { crate::abi::exports_misc::xmlGetUTF8Char(p, len as *mut c_int) }
}

/// Append the decimal rendering of `number` (upstream
/// `xsltNumberFormatDecimal`, numbers.c). `digit_zero` is the first byte of
/// the zero-digit character; `grouping_char` is the grouping character's
/// code point (0 disables grouping). Digits are rendered as
/// `digit_zero + (int)fmod(number, 10)` encoded as UTF-8, exactly as
/// upstream does (so a single-byte zero-digit above 0x7F yields the
/// same-value code point, and a multi-byte zero-digit follows upstream's
/// byte-arithmetic behavior).
unsafe fn number_format_decimal(
    out: &mut Vec<u8>,
    mut number: f64,
    digit_zero: u32,
    width: c_int,
    digits_per_group: c_int,
    grouping_char: u32,
) {
    unsafe {
        let mut stack: Vec<u8> = Vec::new();
        let mut gbuf = [0u8; 4];
        let g_len = if grouping_char != 0 {
            encode_utf8(&mut gbuf, grouping_char)
        } else {
            0
        };
        let mut i: c_int = 0;
        loop {
            if i >= width && number.abs() < 1.0 {
                break;
            }
            if i > 0
                && grouping_char != 0
                && digits_per_group > 0
                && (i % digits_per_group) == 0
                && g_len > 0
            {
                stack.extend_from_slice(&gbuf[..g_len]);
            }
            let val = digit_zero as i32 + (number % 10.0) as i32;
            let mut cb = [0u8; 4];
            let clen = encode_utf8(&mut cb, val as u32);
            if clen > 0 {
                stack.extend_from_slice(&cb[..clen]);
            }
            number /= 10.0;
            i += 1;
        }
        stack.reverse();
        out.extend_from_slice(&stack);
    }
}

/// Emit a transform error message through the XSLT error machinery with the
/// same observable output upstream produces (message without a trailing
/// newline; the error channel appends one).
unsafe fn emit_transform_error(msg: &[u8]) {
    {
        let mut buf = msg.to_vec();
        buf.push(0);
        crate::xslt::errors::xsltTransformError(
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            buf.as_ptr() as *const c_char,
        );
    }
}

/// Format a number with an XSLT decimal-format picture. Faithful port of
/// upstream `xsltFormatNumberConversion` (numbers.c 1.1.45): the positive
/// subpattern is parsed (prefix, integer `#`/`0` run, fraction `0`/`#`
/// run, suffix), the negative subpattern (after `patternSeparator`) is
/// applied for negative numbers, the multiplier from `%`/`permille` is
/// applied, the number is rounded to the fraction width and the result is
/// assembled with the decimal format's characters (grouping, decimal point,
/// minus sign, infinity, NaN).
///
/// `self_` may be NULL: the standard default decimal format is substituted
/// (upstream's own `xsltNewDecimalFormat` defaults).
///
/// On success `*result` receives an xmlMalloc'd NUL-terminated string
/// (caller frees with `xmlFree`); returns XPATH_EXPRESSION_OK.
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
/// - `format` must be a valid pointer or NULL (a NULL format is treated as
///   an empty format, matching `xmlStrlen(NULL) == 0` upstream).
/// - `result` must be a valid out-pointer.
pub(crate) unsafe fn xslt_format_number_conversion(
    self_: *mut _xsltDecimalFormat,
    format: *const xmlChar,
    number: f64,
    result: *mut *mut xmlChar,
) -> c_int {
    unsafe {
        let status: c_int = XPATH_EXPRESSION_OK as c_int;
        let fmt = self_;
        *result = ptr::null_mut();

        let format_len = if format.is_null() {
            0
        } else {
            libc::strlen(format as *const libc::c_char) as usize
        };
        if format_len == 0 {
            emit_transform_error(b"xsltFormatNumberConversion : Invalid format (0-length)\n");
        }
        if number.is_nan() {
            let no_number = fmt_char(fmt, |f| (*f).noNumber, &DEF_NAN);
            *result = crate::xml::string::xml_strdup(no_number);
            return status;
        }

        let zero_digit = fmt_char(fmt, |f| (*f).zeroDigit, &DEF_ZERO_DIGIT);
        let digit = fmt_char(fmt, |f| (*f).digit, &DEF_DIGIT_CHAR);
        let decimal_point = fmt_char(fmt, |f| (*f).decimalPoint, &DEF_DECIMAL_POINT);
        let grouping = fmt_char(fmt, |f| (*f).grouping, &DEF_GROUPING);
        let pattern_sep = fmt_char(fmt, |f| (*f).patternSeparator, &DEF_PATTERN_SEP);
        let minus_sign = fmt_char(fmt, |f| (*f).minusSign, &DEF_MINUS);
        let percent = fmt_char(fmt, |f| (*f).percent, &DEF_PERCENT);
        let permille = fmt_char(fmt, |f| (*f).permille, &DEF_PERMILLE);

        let mut info = FormatNumberInfo {
            group: -1,
            multiplier: 1,
            ..Default::default()
        };

        let mut the_format: *const xmlChar = format;
        let mut found_error: bool = false;
        let mut default_sign: bool = false;
        let mut prefix: *const xmlChar = the_format;
        let mut prefix_length: c_int;
        let mut suffix: *const xmlChar = ptr::null();
        let mut suffix_length: c_int = 0;
        let mut nprefix: *const xmlChar = ptr::null();
        let mut nsuffix: *const xmlChar = ptr::null();
        let mut nprefix_length: c_int = 0;
        let mut nsuffix_length: c_int = 0;
        let mut len: c_int = 0;
        let mut delayed_multiplier: c_int = 0;

        // +ve pattern: prefix.
        prefix_length = pre_suffix(
            fmt,
            &mut the_format,
            &mut info,
            decimal_point,
            grouping,
            zero_digit,
            digit,
            pattern_sep,
            percent,
            permille,
        );
        if prefix_length < 0 {
            found_error = true;
            // goto OUTPUT_NUMBER
        }

        let self_grouping_len = if fmt.is_null() {
            1
        } else if grouping.is_null() {
            0
        } else {
            libc::strlen(grouping as *const c_char) as usize
        };

        if !found_error {
            // Number part: digits, grouping, percent/permille.
            while *the_format != 0
                && !utf8_char_cmp(the_format, decimal_point)
                && !utf8_char_cmp(the_format, pattern_sep)
            {
                if delayed_multiplier != 0 {
                    info.multiplier = delayed_multiplier;
                    info.is_multiplier_set = true;
                    delayed_multiplier = 0;
                }
                if utf8_char_cmp(the_format, digit) {
                    if info.integer_digits > 0 {
                        found_error = true;
                        break;
                    }
                    info.integer_hash += 1;
                    if info.group >= 0 {
                        info.group += 1;
                    }
                } else if utf8_char_cmp(the_format, zero_digit) {
                    info.integer_digits += 1;
                    if info.group >= 0 {
                        info.group += 1;
                    }
                } else if self_grouping_len > 0
                    && libc::strncmp(
                        the_format as *const c_char,
                        grouping as *const c_char,
                        self_grouping_len,
                    ) == 0
                {
                    // Reset group count (the grouping separator may be
                    // multi-byte; consume its whole length).
                    info.group = 0;
                    the_format = the_format.add(self_grouping_len);
                    continue;
                } else if utf8_char_cmp(the_format, percent) {
                    if info.is_multiplier_set {
                        found_error = true;
                        break;
                    }
                    delayed_multiplier = 100;
                } else if utf8_char_cmp(the_format, permille) {
                    if info.is_multiplier_set {
                        found_error = true;
                        break;
                    }
                    delayed_multiplier = 1000;
                } else {
                    break;
                }
                len = crate::abi::exports_string::xmlUTF8Strsize(the_format, 1);
                if len < 1 {
                    found_error = true;
                    break;
                }
                the_format = the_format.add(len as usize);
            }
        }

        // Fraction part.
        if !found_error && *the_format != 0 && utf8_char_cmp(the_format, decimal_point) {
            info.add_decimal = true;
            len = crate::abi::exports_string::xmlUTF8Strsize(the_format, 1);
            if len < 1 {
                found_error = true;
            } else {
                the_format = the_format.add(len as usize);
                while *the_format != 0 {
                    if utf8_char_cmp(the_format, zero_digit) {
                        if info.frac_hash != 0 {
                            found_error = true;
                            break;
                        }
                        info.frac_digits += 1;
                    } else if utf8_char_cmp(the_format, digit) {
                        info.frac_hash += 1;
                    } else if utf8_char_cmp(the_format, percent) {
                        if info.is_multiplier_set {
                            found_error = true;
                            break;
                        }
                        delayed_multiplier = 100;
                        len = crate::abi::exports_string::xmlUTF8Strsize(the_format, 1);
                        if len < 1 {
                            found_error = true;
                            break;
                        }
                        the_format = the_format.add(len as usize);
                        continue;
                    } else if utf8_char_cmp(the_format, permille) {
                        if info.is_multiplier_set {
                            found_error = true;
                            break;
                        }
                        delayed_multiplier = 1000;
                        len = crate::abi::exports_string::xmlUTF8Strsize(the_format, 1);
                        if len < 1 {
                            found_error = true;
                            break;
                        }
                        the_format = the_format.add(len as usize);
                        continue;
                    } else if !utf8_char_cmp(the_format, grouping) {
                        break;
                    }
                    len = crate::abi::exports_string::xmlUTF8Strsize(the_format, 1);
                    if len < 1 {
                        found_error = true;
                        break;
                    }
                    the_format = the_format.add(len as usize);
                    if delayed_multiplier != 0 {
                        info.multiplier = delayed_multiplier;
                        delayed_multiplier = 0;
                        info.is_multiplier_set = true;
                    }
                }
            }
        }

        // A trailing multiplier after the number part belongs to the suffix.
        if delayed_multiplier != 0 {
            the_format = the_format.sub(len as usize);
            delayed_multiplier = 0;
        }

        // +ve suffix.
        if !found_error {
            suffix = the_format;
            suffix_length = pre_suffix(
                fmt,
                &mut the_format,
                &mut info,
                decimal_point,
                grouping,
                zero_digit,
                digit,
                pattern_sep,
                percent,
                permille,
            );
            if suffix_length < 0 || (*the_format != 0 && !utf8_char_cmp(the_format, pattern_sep)) {
                found_error = true;
            }
        }

        // Negative number: -ve pattern.
        if !found_error && number < 0.0 {
            // `j` is the number of UTF-8 chars before the separator, not
            // the number of bytes (upstream bug 151975).
            let j = crate::abi::exports_string::xmlUTF8Strloc(format, pattern_sep);
            if j < 0 {
                // No -ve pattern present, so use default signing.
                default_sign = true;
            } else {
                // Skip over the pattern separator (accounting for UTF-8).
                the_format = crate::abi::exports_string::xmlUTF8Strpos(format, j + 1);
                // Flag changes interpretation of percent/permille in the
                // -ve pattern.
                info.is_negative_pattern = true;
                info.is_multiplier_set = false;

                // First do the -ve prefix.
                nprefix = the_format;
                nprefix_length = pre_suffix(
                    fmt,
                    &mut the_format,
                    &mut info,
                    decimal_point,
                    grouping,
                    zero_digit,
                    digit,
                    pattern_sep,
                    percent,
                    permille,
                );
                if nprefix_length < 0 {
                    found_error = true;
                }
                let mut neg_mult: c_int = 0;
                if !found_error {
                    while *the_format != 0 {
                        if utf8_char_cmp(the_format, percent) || utf8_char_cmp(the_format, permille)
                        {
                            if info.is_multiplier_set {
                                found_error = true;
                                break;
                            }
                            info.is_multiplier_set = true;
                            neg_mult = 1;
                        } else if is_special(
                            fmt,
                            the_format,
                            decimal_point,
                            grouping,
                            zero_digit,
                            digit,
                            pattern_sep,
                        ) {
                            neg_mult = 0;
                        } else {
                            break;
                        }
                        len = crate::abi::exports_string::xmlUTF8Strsize(the_format, 1);
                        if len < 1 {
                            found_error = true;
                            break;
                        }
                        the_format = the_format.add(len as usize);
                    }
                    if neg_mult != 0 {
                        info.is_multiplier_set = false;
                        the_format = the_format.sub(len as usize);
                    }
                    // Finally do the -ve suffix.
                    if *the_format != 0 {
                        nsuffix = the_format;
                        nsuffix_length = pre_suffix(
                            fmt,
                            &mut the_format,
                            &mut info,
                            decimal_point,
                            grouping,
                            zero_digit,
                            digit,
                            pattern_sep,
                            percent,
                            permille,
                        );
                        if nsuffix_length < 0 {
                            found_error = true;
                        }
                    } else {
                        nsuffix_length = 0;
                    }
                    if !found_error && *the_format != 0 {
                        found_error = true;
                    }
                    // Java peculiarity: if the -ve prefix/suffix equals the
                    // +ve ones, discard it and use the default.
                    if (nprefix_length != prefix_length)
                        || (nsuffix_length != suffix_length)
                        || (nprefix_length > 0
                            && prefix_length > 0
                            && libc::strncmp(
                                nprefix as *const c_char,
                                prefix as *const c_char,
                                prefix_length as usize,
                            ) != 0)
                        || (nsuffix_length > 0
                            && suffix_length > 0
                            && libc::strncmp(
                                nsuffix as *const c_char,
                                suffix as *const c_char,
                                suffix_length as usize,
                            ) != 0)
                    {
                        prefix = nprefix;
                        prefix_length = nprefix_length;
                        suffix = nsuffix;
                        suffix_length = nsuffix_length;
                    }
                }
            }
        }

        // OUTPUT_NUMBER:
        if found_error {
            // Upstream expands %s with the (possibly NULL) format string;
            // glibc prints "(null)" for a NULL %s argument.
            let shown = if format.is_null() {
                "(null)".to_string()
            } else {
                let bytes =
                    core::slice::from_raw_parts(format, libc::strlen(format as *const c_char));
                String::from_utf8_lossy(bytes).into_owned()
            };
            let msg = format!(
                "xsltFormatNumberConversion : error in format string '{}', using default\n",
                shown
            );
            emit_transform_error(msg.as_bytes());
            default_sign = number < 0.0;
            prefix_length = 0;
            suffix_length = 0;
            info.integer_hash = 0;
            info.integer_digits = 1;
            info.frac_digits = 1;
            info.frac_hash = 4;
            info.group = -1;
            info.multiplier = 1;
            info.add_decimal = true;
        }

        // Apply the multiplier.
        let scaled = number * info.multiplier as f64;
        match is_inf(scaled) {
            -1 => {
                // Upstream `case -1` assigns the minus sign then falls
                // through to `case 1`, concatenating the infinity string.
                let ms = fmt_char(fmt, |f| (*f).minusSign, &DEF_MINUS);
                let inf = fmt_char(fmt, |f| (*f).infinity, &DEF_INFINITY);
                let mut joined = unsafe_bytes(ms).to_vec();
                joined.extend_from_slice(unsafe_bytes(inf));
                *result = xml_strdup_joined(&joined);
                return status;
            }
            1 => {
                let inf = fmt_char(fmt, |f| (*f).infinity, &DEF_INFINITY);
                *result = crate::xml::string::xml_strdup(inf);
                return status;
            }
            _ => {}
        }

        let mut out: Vec<u8> = Vec::new();

        // Default sign first (only the first character of minus-sign is
        // emitted, as upstream adds xmlUTF8Strsize(minusSign, 1) bytes).
        if default_sign {
            let l = crate::abi::exports_string::xmlUTF8Strsize(minus_sign, 1);
            out.extend_from_slice(core::slice::from_raw_parts(minus_sign, l.max(1) as usize));
        }

        // Prefix (quote characters are stripped).
        let mut j: c_int = 0;
        let mut pc = prefix;
        while j < prefix_length {
            if *pc == SYMBOL_QUOTE {
                pc = pc.add(1);
            }
            let l = crate::abi::exports_string::xmlUTF8Strsize(pc, 1);
            if l < 1 {
                break;
            }
            out.extend_from_slice(core::slice::from_raw_parts(pc, l as usize));
            pc = pc.add(l as usize);
            j += l;
        }

        // Round to frac_digits + frac_hash digits.
        let mut num = scaled.abs();
        let mut exp10 = info.frac_digits + info.frac_hash;
        if exp10 > 308 {
            if info.frac_digits > 308 {
                info.frac_digits = 308;
                info.frac_hash = 0;
            } else {
                info.frac_hash = 308 - info.frac_digits;
            }
            exp10 = 308;
        }
        let scale = 10f64.powi(exp10);
        num += 0.5 / scale;
        num -= num % (1.0 / scale);

        // Integer part.
        let zero_byte = *zero_digit as u32;
        if !grouping.is_null() && *grouping != 0 {
            let mut glen: c_int = libc::strlen(grouping as *const c_char) as c_int;
            let gchar = get_utf8_char(grouping, &mut glen) as u32;
            number_format_decimal(
                &mut out,
                num.floor(),
                zero_byte,
                info.integer_digits,
                info.group,
                gchar,
            );
        } else {
            number_format_decimal(
                &mut out,
                num.floor(),
                zero_byte,
                info.integer_digits,
                info.group,
                ',' as u32,
            );
        }

        // Java quirk: '.#' acts like '.0'.
        if info.integer_digits + info.integer_hash + info.frac_digits == 0 && info.frac_hash > 0 {
            info.frac_digits += 1;
            info.frac_hash -= 1;
        }

        // Leading zero if the integer part is empty.
        if num.floor() == 0.0 && info.integer_digits + info.frac_digits == 0 {
            let l = crate::abi::exports_string::xmlUTF8Strsize(zero_digit, 1);
            out.extend_from_slice(core::slice::from_raw_parts(zero_digit, l.max(1) as usize));
        }

        // Fraction part.
        if info.frac_digits + info.frac_hash == 0 {
            if info.add_decimal {
                let l = crate::abi::exports_string::xmlUTF8Strsize(decimal_point, 1);
                out.extend_from_slice(core::slice::from_raw_parts(
                    decimal_point,
                    l.max(1) as usize,
                ));
            }
        } else {
            let frac = num - num.floor();
            if frac != 0.0 || info.frac_digits != 0 {
                let l = crate::abi::exports_string::xmlUTF8Strsize(decimal_point, 1);
                out.extend_from_slice(core::slice::from_raw_parts(
                    decimal_point,
                    l.max(1) as usize,
                ));
                let mut fnum = (scale * frac + 0.5).floor();
                let mut jj = info.frac_hash;
                while jj > 0 {
                    if fnum % 10.0 >= 1.0 {
                        break;
                    }
                    fnum /= 10.0;
                    jj -= 1;
                }
                number_format_decimal(
                    &mut out,
                    fnum.floor(),
                    zero_byte,
                    info.frac_digits + jj,
                    0,
                    0,
                );
            }
        }

        // Suffix (quote characters are stripped).
        let mut k: c_int = 0;
        let mut sc = suffix;
        while k < suffix_length {
            if *sc == SYMBOL_QUOTE {
                sc = sc.add(1);
            }
            let l = crate::abi::exports_string::xmlUTF8Strsize(sc, 1);
            if l < 1 {
                break;
            }
            out.extend_from_slice(core::slice::from_raw_parts(sc, l as usize));
            sc = sc.add(l as usize);
            k += l;
        }

        out.push(0);
        let mem = crate::abi::allocator::xmlMallocImpl(out.len()) as *mut xmlChar;
        if mem.is_null() {
            return -1;
        }
        ptr::copy_nonoverlapping(out.as_ptr(), mem, out.len());
        *result = mem;
        status
    }
}

/// Classification of a value as +inf / -inf / finite (upstream
/// `xmlXPathIsInf`).
fn is_inf(val: f64) -> i32 {
    if val.is_infinite() {
        if val > 0.0 {
            1
        } else {
            -1
        }
    } else {
        0
    }
}

/// View a NUL-terminated string as bytes (excluding the terminator).
unsafe fn unsafe_bytes(p: *const xmlChar) -> &'static [u8] {
    unsafe {
        if p.is_null() {
            return &[];
        }
        core::slice::from_raw_parts(p, libc::strlen(p as *const c_char))
    }
}

/// xmlStrdup an already-assembled NUL-terminated byte buffer.
unsafe fn xml_strdup_joined(bytes: &[u8]) -> *mut xmlChar {
    unsafe { crate::xml::string::xml_strdup(bytes.as_ptr() as *const xmlChar) }
}

/// Locate a decimal format by (namespace URI, local name), following
/// upstream xslt.c `xsltDecimalFormatGetByQName`: the default format lives
/// at the chain head and named formats are appended after it, so the named
/// walk starts from `head->next`. With a NULL `name` the default (head) is
/// returned. Imported stylesheets are consulted in order.
///
/// # SAFETY
///
/// - `style` may be NULL; `nsUri`/`name` may be NULL.
pub(crate) unsafe fn decimal_format_by_qname(
    style: *mut _xsltStylesheet,
    ns_uri: *const xmlChar,
    name: *const xmlChar,
) -> *mut _xsltDecimalFormat {
    unsafe {
        if name.is_null() {
            if style.is_null() {
                return ptr::null_mut();
            }
            return (*style).decimalFormat;
        }
        let mut cur_style = style;
        while !cur_style.is_null() {
            if !(*cur_style).decimalFormat.is_null() {
                let mut result = (*(*cur_style).decimalFormat).next;
                while !result.is_null() {
                    if crate::abi::exports_xml2::xmlStrEqual(ns_uri, (*result).nsUri) != 0
                        && crate::abi::exports_xml2::xmlStrEqual(name, (*result).name) != 0
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
}

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
///
/// # SAFETY
///
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

    unsafe fn free_str(p: *mut xmlChar) {
        if !p.is_null() {
            libc::free(p as *mut libc::c_void);
        }
    }

    /// Convert through the canonical entry point with the NULL (default
    /// ASCII) decimal format and return the string.
    unsafe fn to_string(number: f64, format: &[u8]) -> String {
        unsafe {
            let mut fmt = format.to_vec();
            fmt.push(0);
            let mut result: *mut xmlChar = ptr::null_mut();
            let status = xslt_format_number_conversion(
                ptr::null_mut(),
                fmt.as_ptr() as *const xmlChar,
                number,
                &mut result,
            );
            assert_eq!(status, XPATH_EXPRESSION_OK as c_int);
            let s = String::from_utf8_lossy(unsafe_bytes(result)).into_owned();
            free_str(result);
            s
        }
    }

    /// Build a decimal format with the given character overrides (defaults
    /// for everything else). The caller frees with `free_fmt`.
    unsafe fn make_fmt(
        decimal: &[u8],
        grouping: &[u8],
        infinity: &[u8],
        no_number: &[u8],
    ) -> *mut _xsltDecimalFormat {
        unsafe {
            let f = libc::calloc(1, core::mem::size_of::<_xsltDecimalFormat>())
                as *mut _xsltDecimalFormat;
            (*f).digit = crate::xml::string::xml_strdup(c"#".as_ptr() as *const xmlChar);
            (*f).patternSeparator = crate::xml::string::xml_strdup(c";".as_ptr() as *const xmlChar);
            (*f).minusSign = crate::xml::string::xml_strdup(c"-".as_ptr() as *const xmlChar);
            let mut d = decimal.to_vec();
            d.push(0);
            (*f).decimalPoint = crate::xml::string::xml_strdup(d.as_ptr() as *const xmlChar);
            let mut g = grouping.to_vec();
            g.push(0);
            (*f).grouping = crate::xml::string::xml_strdup(g.as_ptr() as *const xmlChar);
            (*f).percent = crate::xml::string::xml_strdup(c"%".as_ptr() as *const xmlChar);
            (*f).permille = crate::xml::string::xml_strdup(c"‰".as_ptr() as *const xmlChar);
            (*f).zeroDigit = crate::xml::string::xml_strdup(c"0".as_ptr() as *const xmlChar);
            let mut inf = infinity.to_vec();
            inf.push(0);
            (*f).infinity = crate::xml::string::xml_strdup(inf.as_ptr() as *const xmlChar);
            let mut nn = no_number.to_vec();
            nn.push(0);
            (*f).noNumber = crate::xml::string::xml_strdup(nn.as_ptr() as *const xmlChar);
            f
        }
    }

    unsafe fn free_fmt(f: *mut _xsltDecimalFormat) {
        unsafe {
            if f.is_null() {
                return;
            }
            libc::free((*f).digit as *mut libc::c_void);
            libc::free((*f).patternSeparator as *mut libc::c_void);
            libc::free((*f).minusSign as *mut libc::c_void);
            libc::free((*f).decimalPoint as *mut libc::c_void);
            libc::free((*f).grouping as *mut libc::c_void);
            libc::free((*f).percent as *mut libc::c_void);
            libc::free((*f).permille as *mut libc::c_void);
            libc::free((*f).zeroDigit as *mut libc::c_void);
            libc::free((*f).infinity as *mut libc::c_void);
            libc::free((*f).noNumber as *mut libc::c_void);
            libc::free(f as *mut libc::c_void);
        }
    }

    /// Convert with a custom decimal format.
    unsafe fn to_string_fmt(fmt: *mut _xsltDecimalFormat, number: f64, format: &[u8]) -> String {
        unsafe {
            let mut f = format.to_vec();
            f.push(0);
            let mut result: *mut xmlChar = ptr::null_mut();
            let status = xslt_format_number_conversion(
                fmt,
                f.as_ptr() as *const xmlChar,
                number,
                &mut result,
            );
            assert_eq!(status, XPATH_EXPRESSION_OK as c_int);
            let s = String::from_utf8_lossy(unsafe_bytes(result)).into_owned();
            free_str(result);
            s
        }
    }

    // ── Differential-verified values (oracle libxslt 1.1.45) ────────────

    #[test]
    fn test_grouping_and_fraction() {
        unsafe {
            assert_eq!(to_string(1234567.891, b"#,##0.00"), "1,234,567.89");
            assert_eq!(to_string(1234.5, b"#,##0.00"), "1,234.50");
            assert_eq!(to_string(1234.5, b"0,000"), "1,235");
            assert_eq!(to_string(123.456, b"0.00#"), "123.456");
        }
    }

    #[test]
    fn test_negative_patterns() {
        unsafe {
            assert_eq!(to_string(-1234.5, b"#,##0.00;(#,##0.00)"), "(1,234.50)");
            assert_eq!(to_string(-1234.5, b"#,##0.00;-#,##0.00"), "-1,234.50");
            // No -ve pattern: default sign.
            assert_eq!(to_string(-42.0, b"0.00"), "-42.00");
            // Identical -ve pattern is discarded and, because upstream's
            // `default_sign = 1` else-branch is commented out (numbers.c),
            // NO minus is emitted for the negative value.
            assert_eq!(to_string(-1234.5, b"0.00;0.00"), "1234.50");
        }
    }

    #[test]
    fn test_multipliers() {
        unsafe {
            assert_eq!(to_string(0.055, b"0.00%"), "5.50%");
            assert_eq!(to_string(0.055, "0.00‰".as_bytes()), "55.00‰");
            assert_eq!(to_string(1234.0, b"0%"), "123400%");
        }
    }

    #[test]
    fn test_nan_and_infinity() {
        unsafe {
            assert_eq!(to_string(f64::NAN, b"0.00"), "NaN");
            assert_eq!(to_string(f64::INFINITY, b"0.00"), "Infinity");
            assert_eq!(to_string(f64::NEG_INFINITY, b"0.00"), "-Infinity");
        }
    }
    #[allow(clippy::approx_constant)]
    #[test]
    fn test_padding_and_java_quirks() {
        unsafe {
            assert_eq!(to_string(42.0, b"0000"), "0042");
            assert_eq!(to_string(2.0, b"00.00"), "02.00");
            // '.#' acts like '.0'.
            assert_eq!(to_string(42.0, b".#"), "42.0");
            assert_eq!(to_string(3.14159, b"#.##"), "3.14");
            assert_eq!(to_string(7.0, b"0.############"), "7");
        }
    }

    #[test]
    fn test_prefix_suffix_quotes() {
        unsafe {
            // Quoted dollar prefix; the quoted special char after the
            // closing quote is counted in the prefix (upstream quirk).
            assert_eq!(to_string(1234.5, b"'$'#,##0.00"), "$#1,234.50");
            assert_eq!(to_string(1234.5, b"#,##0.00 USD"), "1,234.50 USD");
            assert_eq!(to_string(1234.5, b"'#'#,##0.00"), "##1,234.50");
            assert_eq!(to_string(1234.5, b"''#,##0.00"), "'1,234.50");
        }
    }

    #[test]
    fn test_malformed_fallback() {
        unsafe {
            // Malformed pictures fall back to the default format.
            assert_eq!(to_string(1234.5, b"0.##0"), "1234.5");
            assert_eq!(to_string(1234.5, b"0;0;0"), "1235");
            assert_eq!(to_string(1234.5, b"#0#"), "1234.5");
        }
    }

    #[test]
    fn test_tiny_numbers() {
        unsafe {
            assert_eq!(to_string(0.000000001, b"0.000000000"), "0.000000001");
        }
    }

    #[test]
    fn test_custom_decimal_format() {
        unsafe {
            // Euro style: decimal=',', grouping='.'.
            let euro = make_fmt(b",", b".", b"INF", b"NAN");
            // Picture '0.00' under euro: '.' is the grouping char, so the
            // integer part is grouped every two digits.
            assert_eq!(to_string_fmt(euro, 1234567.891, b"0.00"), "1.23.45.68");
            assert_eq!(to_string_fmt(euro, f64::INFINITY, b"0"), "INF");
            assert_eq!(to_string_fmt(euro, f64::NAN, b"0"), "NAN");
            // '#' is not the digit char under euro (it is), but ',' IS the
            // decimal point: '#,##0.00' has a fraction of '0.00' after ','.
            assert_eq!(to_string_fmt(euro, -1234.5, b"#,##0.00"), "-1234,5");
            free_fmt(euro);
        }
    }

    #[test]
    fn test_format_decimal() {
        unsafe {
            assert_eq!(to_string(1234.0, b"#,##0.00"), "1,234.00");
        }
    }

    #[test]
    fn test_format_padded() {
        unsafe {
            assert_eq!(to_string(42.0, b"0000"), "0042");
        }
    }

    #[test]
    fn test_format_alphabetic() {
        unsafe {
            // The xsl:number token formatter (not the picture parser).
            let p = xsltFormatNumber(27.0, c"a".as_ptr() as *const xmlChar);
            assert_eq!(String::from_utf8_lossy(unsafe_bytes(p)), "aa");
            free_str(p);
        }
    }

    #[test]
    fn test_format_roman() {
        unsafe {
            let p = xsltFormatNumber(2024.0, c"I".as_ptr() as *const xmlChar);
            assert_eq!(String::from_utf8_lossy(unsafe_bytes(p)), "MMXXIV");
            free_str(p);
        }
    }

    #[test]
    fn test_format_null() {
        unsafe {
            // NULL format behaves like an empty picture: raw decimal.
            assert_eq!(to_string(7.0, b""), "7");
            assert_eq!(to_string(0.0, b"0"), "0");
        }
    }
}
