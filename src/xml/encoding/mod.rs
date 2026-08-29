//! Character encoding handling (§22, §85 Phase 4).
//!
//! Encoding detection, XML declaration encoding, BOM behavior, UTF-8/UTF-16
//! validity, legacy encodings, conversion errors, output conversion,
//! serializer fallback, custom encoding handlers.
//!
//! # Architecture
//!
//! ```text
//! ABI exports (exports_xml2.rs)  ←  pub(crate) functions in this module
//!                                           ↕
//!                           Encoding handler registry (global RwLock)
//!                                           ↕
//!              Built-in handlers: UTF-8, UTF-16LE, UTF-16BE, Latin-1, ASCII
//! ```
//!
//! The internal encoding is always UTF-8. All conversions go to/from UTF-8.
//! The handler registry stores `_xmlCharEncodingHandler` structs that contain
//! function pointers for input (→UTF-8) and output (UTF-8→) conversion.

#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_uchar, c_uint, c_void};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};

use once_cell::sync::Lazy;
use parking_lot::RwLock;

use crate::abi::allocator::{xmlFree, xmlMalloc, xmlRealloc};
use crate::abi::callbacks::{xmlCharEncodingInputFunc, xmlCharEncodingOutputFunc};
use crate::abi::constants::XML_MAX_ENCODING_NAME_LEN;
use crate::abi::structs::{
    _xmlBuffer, _xmlCharEncodingHandler, EncodingInputUnion, EncodingOutputUnion,
};
use crate::abi::types::{xmlChar, xmlCharEncoding, xmlCharPtr};

// ── Constants ──────────────────────────────────────────────────────────────

/// Maximum bytes needed per character for any supported encoding.
const MAX_CHAR_BYTES: usize = 6;

/// UTF-8 BOM bytes.
const UTF8_BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

/// UTF-16LE BOM bytes.
const UTF16LE_BOM: [u8; 2] = [0xFF, 0xFE];

/// UTF-16BE BOM bytes.
const UTF16BE_BOM: [u8; 2] = [0xFE, 0xFF];

// ── Global handler registry ────────────────────────────────────────────────

/// A raw pointer wrapper that implements `Send` and `Sync`.
///
/// This is safe because all access to the global handler registry is
/// serialized through the `RwLock`, and handlers are only accessed from
/// trusted internal code.
#[derive(Clone, Copy)]
struct HandlerPtr(*mut _xmlCharEncodingHandler);

unsafe impl Send for HandlerPtr {}
unsafe impl Sync for HandlerPtr {}

/// Global list of registered encoding handlers, protected by a read-write lock.
static ENCODING_HANDLERS: Lazy<RwLock<Vec<HandlerPtr>>> = Lazy::new(|| RwLock::new(Vec::new()));

/// Whether the built-in encoding handlers have been initialized.
static ENCODING_INITIALIZED: AtomicBool = AtomicBool::new(false);

// ═══════════════════════════════════════════════════════════════════════════════
// 1. Encoding detection
// ═══════════════════════════════════════════════════════════════════════════════

/// Determine encoding from BOM bytes.
///
/// Returns `XML_CHAR_ENCODING_NONE` if no BOM is present, or if `data` is empty.
/// Otherwise returns the matching encoding enum value.
pub(crate) fn detect_encoding_from_bom(data: &[u8]) -> xmlCharEncoding {
    if data.len() >= 3 && data[0..3] == UTF8_BOM {
        xmlCharEncoding::XML_CHAR_ENCODING_UTF8
    } else if data.len() >= 2 && data[0..2] == UTF16LE_BOM {
        xmlCharEncoding::XML_CHAR_ENCODING_UTF16LE
    } else if data.len() >= 2 && data[0..2] == UTF16BE_BOM {
        xmlCharEncoding::XML_CHAR_ENCODING_UTF16BE
    } else {
        xmlCharEncoding::XML_CHAR_ENCODING_NONE
    }
}

/// Determine encoding from an XML declaration's `encoding` attribute.
///
/// Scans for `<?xml ... encoding="..." ?>` and returns the encoding name
/// as a byte vector (lowercased), or `None` if not found.
pub(crate) fn detect_encoding_from_declaration(data: &[u8]) -> Option<Vec<u8>> {
    // Look for "<?xml" at the start (possibly after BOM)
    let start = if data.len() >= 3 && data[0..3] == UTF8_BOM {
        3
    } else if data.len() >= 2 && (data[0..2] == UTF16LE_BOM || data[0..2] == UTF16BE_BOM) {
        // For UTF-16, we can't easily scan the bytes; skip
        return None;
    } else {
        0
    };

    let remaining = &data[start..];

    // Must start with "<?xml"
    if remaining.len() < 5 || !remaining[0..5].eq_ignore_ascii_case(b"<?xml") {
        return None;
    }

    // Find the end of the PI: "?>"
    let pi_end = remaining.windows(2).position(|w| w == b"?>")?;
    let decl_content = &remaining[5..pi_end];

    // Look for "encoding" attribute
    let decl_str = core::str::from_utf8(decl_content).ok()?;
    let lower = decl_str.to_ascii_lowercase();

    // Find "encoding" keyword
    let enc_pos = lower.find("encoding")?;

    // After "encoding", expect optional whitespace and '='
    let after_enc = &decl_content[enc_pos + 8..];
    let after_enc_str = core::str::from_utf8(after_enc).ok()?;
    let after_enc_trimmed = after_enc_str.trim_start();

    if !after_enc_trimmed.starts_with('=') {
        return None;
    }

    let after_eq = after_enc_trimmed[1..].trim_start();

    // Expect quote character
    let quote = after_eq.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }

    // Find matching closing quote
    let value_end = after_eq[1..].find(quote)?;
    let encoding_value = &after_eq[1..=value_end];

    Some(encoding_value.to_ascii_lowercase().as_bytes().to_vec())
}

/// Parse an encoding name string to an `xmlCharEncoding` enum value.
///
/// Matching is case-insensitive. Common aliases are recognized.
/// Returns `XML_CHAR_ENCODING_ERROR` if the name is not recognized.
pub(crate) fn encoding_from_name(name: &[u8]) -> xmlCharEncoding {
    let s = core::str::from_utf8(name).unwrap_or("");
    let s = s.trim().to_ascii_lowercase();

    match s.as_str() {
        // UTF-8
        "utf-8" | "utf8" => xmlCharEncoding::XML_CHAR_ENCODING_UTF8,

        // UTF-16
        "utf-16" | "utf-16le" | "utf16le" => xmlCharEncoding::XML_CHAR_ENCODING_UTF16LE,
        "utf-16be" | "utf16be" => xmlCharEncoding::XML_CHAR_ENCODING_UTF16BE,

        // ISO-8859 variants
        "iso-8859-1" | "iso_8859-1" | "latin1" | "latin-1" | "l1" | "cp819" | "ibm819"
        | "iso-ir-100" | "iso_8859-1:1987" => xmlCharEncoding::XML_CHAR_ENCODING_8859_1,
        "iso-8859-2" | "iso_8859-2" | "latin2" | "latin-2" | "l2" => {
            xmlCharEncoding::XML_CHAR_ENCODING_8859_2
        }
        "iso-8859-3" | "iso_8859-3" | "latin3" | "latin-3" | "l3" => {
            xmlCharEncoding::XML_CHAR_ENCODING_8859_3
        }
        "iso-8859-4" | "iso_8859-4" | "latin4" | "latin-4" | "l4" => {
            xmlCharEncoding::XML_CHAR_ENCODING_8859_4
        }
        "iso-8859-5" | "iso_8859-5" | "cyrillic" => xmlCharEncoding::XML_CHAR_ENCODING_8859_5,
        "iso-8859-6" | "iso_8859-6" | "arabic" => xmlCharEncoding::XML_CHAR_ENCODING_8859_6,
        "iso-8859-7" | "iso_8859-7" | "greek" => xmlCharEncoding::XML_CHAR_ENCODING_8859_7,
        "iso-8859-8" | "iso_8859-8" | "hebrew" => xmlCharEncoding::XML_CHAR_ENCODING_8859_8,
        "iso-8859-9" | "iso_8859-9" | "latin5" | "latin-5" | "l5" | "turkish" => {
            xmlCharEncoding::XML_CHAR_ENCODING_8859_9
        }

        // ASCII
        "ascii" | "us-ascii" | "us" | "ansi_x3.4-1968" | "ansi_x3.4-1986" | "iso-ir-6"
        | "iso_646.irv:1991" | "cp367" | "ibm367" => xmlCharEncoding::XML_CHAR_ENCODING_ASCII,

        // East Asian
        "iso-2022-jp" | "iso2022-jp" => xmlCharEncoding::XML_CHAR_ENCODING_2022_JP,
        "shift_jis" | "shift-jis" | "sjis" | "cp932" => {
            xmlCharEncoding::XML_CHAR_ENCODING_SHIFT_JIS
        }
        "euc-jp" | "eucjp" => xmlCharEncoding::XML_CHAR_ENCODING_EUC_JP,

        // UCS/Unicode variants
        "ucs-4" | "ucs4" => xmlCharEncoding::XML_CHAR_ENCODING_UCS4LE,
        "ucs-4le" | "ucs4le" => xmlCharEncoding::XML_CHAR_ENCODING_UCS4LE,
        "ucs-4be" | "ucs4be" => xmlCharEncoding::XML_CHAR_ENCODING_UCS4BE,
        "ucs-2" | "ucs2" => xmlCharEncoding::XML_CHAR_ENCODING_UCS2,

        // EBCDIC
        "ebcdic" | "cp037" | "ibm037" => xmlCharEncoding::XML_CHAR_ENCODING_EBCDIC,

        _ => xmlCharEncoding::XML_CHAR_ENCODING_ERROR,
    }
}

/// Get the canonical name for an encoding as a byte slice.
///
/// Returns `None` for `XML_CHAR_ENCODING_ERROR` and `XML_CHAR_ENCODING_NONE`.
pub(crate) fn encoding_name(enc: xmlCharEncoding) -> Option<&'static [u8]> {
    match enc {
        xmlCharEncoding::XML_CHAR_ENCODING_UTF8 => Some(b"UTF-8" as &[u8]),
        xmlCharEncoding::XML_CHAR_ENCODING_UTF16LE => Some(b"UTF-16LE" as &[u8]),
        xmlCharEncoding::XML_CHAR_ENCODING_UTF16BE => Some(b"UTF-16BE" as &[u8]),
        xmlCharEncoding::XML_CHAR_ENCODING_UCS4LE => Some(b"UCS-4LE" as &[u8]),
        xmlCharEncoding::XML_CHAR_ENCODING_UCS4BE => Some(b"UCS-4BE" as &[u8]),
        xmlCharEncoding::XML_CHAR_ENCODING_EBCDIC => Some(b"EBCDIC" as &[u8]),
        xmlCharEncoding::XML_CHAR_ENCODING_UCS4_2143 => Some(b"UCS-4-2143" as &[u8]),
        xmlCharEncoding::XML_CHAR_ENCODING_UCS4_3412 => Some(b"UCS-4-3412" as &[u8]),
        xmlCharEncoding::XML_CHAR_ENCODING_UCS2 => Some(b"UCS-2" as &[u8]),
        xmlCharEncoding::XML_CHAR_ENCODING_8859_1 => Some(b"ISO-8859-1" as &[u8]),
        xmlCharEncoding::XML_CHAR_ENCODING_8859_2 => Some(b"ISO-8859-2" as &[u8]),
        xmlCharEncoding::XML_CHAR_ENCODING_8859_3 => Some(b"ISO-8859-3" as &[u8]),
        xmlCharEncoding::XML_CHAR_ENCODING_8859_4 => Some(b"ISO-8859-4" as &[u8]),
        xmlCharEncoding::XML_CHAR_ENCODING_8859_5 => Some(b"ISO-8859-5" as &[u8]),
        xmlCharEncoding::XML_CHAR_ENCODING_8859_6 => Some(b"ISO-8859-6" as &[u8]),
        xmlCharEncoding::XML_CHAR_ENCODING_8859_7 => Some(b"ISO-8859-7" as &[u8]),
        xmlCharEncoding::XML_CHAR_ENCODING_8859_8 => Some(b"ISO-8859-8" as &[u8]),
        xmlCharEncoding::XML_CHAR_ENCODING_8859_9 => Some(b"ISO-8859-9" as &[u8]),
        xmlCharEncoding::XML_CHAR_ENCODING_2022_JP => Some(b"ISO-2022-JP" as &[u8]),
        xmlCharEncoding::XML_CHAR_ENCODING_SHIFT_JIS => Some(b"SHIFT_JIS" as &[u8]),
        xmlCharEncoding::XML_CHAR_ENCODING_EUC_JP => Some(b"EUC-JP" as &[u8]),
        xmlCharEncoding::XML_CHAR_ENCODING_ASCII => Some(b"ASCII" as &[u8]),
        _ => None,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 2. UTF-8 validation
// ═══════════════════════════════════════════════════════════════════════════════

/// Check if a byte sequence is valid UTF-8.
///
/// Returns `true` if the entire slice is valid UTF-8, `false` otherwise.
pub(crate) fn utf8_valid(data: &[u8]) -> bool {
    core::str::from_utf8(data).is_ok()
}

/// Check if a Unicode codepoint is a valid XML character.
///
/// Per XML 1.0 (Fifth Edition) §2.2, the valid character ranges are:
/// - `#x9` (tab)
/// - `#xA` (LF)
/// - `#xD` (CR)
/// - `#x20` – `#xD7FF`
/// - `#xE000` – `#xFFFD`
/// - `#x10000` – `#x10FFFF`
///
/// Excludes surrogate halves (`#xD800` – `#xDFFF`) and `#xFFFE`/`#xFFFF`.
pub(crate) fn is_valid_xml_char(cp: u32) -> bool {
    match cp {
        0x9 | 0xA | 0xD => true,
        0x20..=0xD7FF => true,
        0xE000..=0xFFFD => true,
        0x10000..=0x10FFFF => true,
        _ => false,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 3. UTF-16 handling
// ═══════════════════════════════════════════════════════════════════════════════

/// Decode a single UTF-16LE code unit from two bytes.
#[inline]
fn read_utf16le_unit(data: &[u8]) -> Option<u16> {
    if data.len() < 2 {
        return None;
    }
    Some(u16::from_le_bytes([data[0], data[1]]))
}

/// Decode a single UTF-16BE code unit from two bytes.
#[inline]
fn read_utf16be_unit(data: &[u8]) -> Option<u16> {
    if data.len() < 2 {
        return None;
    }
    Some(u16::from_be_bytes([data[0], data[1]]))
}

/// Encode a Unicode codepoint as UTF-8 bytes.
///
/// Returns the number of bytes written (1–4), or 0 if the codepoint is invalid.
fn encode_codepoint_to_utf8(cp: u32, out: &mut [u8]) -> usize {
    if cp < 0x80 {
        if !out.is_empty() {
            out[0] = cp as u8;
        }
        1
    } else if cp < 0x800 {
        if out.len() < 2 {
            return 0;
        }
        out[0] = 0xC0 | ((cp >> 6) as u8);
        out[1] = 0x80 | (cp as u8 & 0x3F);
        2
    } else if cp < 0x10000 {
        if out.len() < 3 {
            return 0;
        }
        out[0] = 0xE0 | ((cp >> 12) as u8);
        out[1] = 0x80 | ((cp >> 6) as u8 & 0x3F);
        out[2] = 0x80 | (cp as u8 & 0x3F);
        3
    } else if cp < 0x110000 {
        if out.len() < 4 {
            return 0;
        }
        out[0] = 0xF0 | ((cp >> 18) as u8);
        out[1] = 0x80 | ((cp >> 12) as u8 & 0x3F);
        out[2] = 0x80 | ((cp >> 6) as u8 & 0x3F);
        out[3] = 0x80 | (cp as u8 & 0x3F);
        4
    } else {
        0
    }
}

/// Convert UTF-16LE bytes to UTF-8.
///
/// Returns `Ok(converted_bytes)` on success, or `Err(())` on invalid input
/// (e.g., unpaired surrogates, truncated data).
pub(crate) fn utf16le_to_utf8(data: &[u8]) -> Result<Vec<u8>, ()> {
    if data.is_empty() {
        return Ok(Vec::new());
    }

    // Skip BOM if present
    let offset = if data.len() >= 2 && data[0..2] == UTF16LE_BOM {
        2
    } else {
        0
    };

    let mut result = Vec::with_capacity(data.len() / 2 + data.len() / 4);
    let mut i = offset;

    while i < data.len() {
        let unit = read_utf16le_unit(&data[i..]).ok_or(())?;
        i += 2;

        if unit >= 0xD800 && unit <= 0xDBFF {
            // High surrogate: expect a low surrogate
            let low = read_utf16le_unit(&data[i..]).ok_or(())?;
            i += 2;

            if !(0xDC00..=0xDFFF).contains(&low) {
                return Err(());
            }

            let cp = 0x10000 + ((unit as u32 - 0xD800) << 10) + (low as u32 - 0xDC00);
            let mut buf = [0u8; 4];
            let n = encode_codepoint_to_utf8(cp, &mut buf);
            if n == 0 {
                return Err(());
            }
            result.extend_from_slice(&buf[..n]);
        } else if unit >= 0xDC00 && unit <= 0xDFFF {
            // Unexpected low surrogate
            return Err(());
        } else {
            let cp = unit as u32;
            let mut buf = [0u8; 4];
            let n = encode_codepoint_to_utf8(cp, &mut buf);
            result.extend_from_slice(&buf[..n]);
        }
    }

    Ok(result)
}

/// Convert UTF-16BE bytes to UTF-8.
///
/// Returns `Ok(converted_bytes)` on success, or `Err(())` on invalid input.
pub(crate) fn utf16be_to_utf8(data: &[u8]) -> Result<Vec<u8>, ()> {
    if data.is_empty() {
        return Ok(Vec::new());
    }

    // Skip BOM if present
    let offset = if data.len() >= 2 && data[0..2] == UTF16BE_BOM {
        2
    } else {
        0
    };

    let mut result = Vec::with_capacity(data.len() / 2 + data.len() / 4);
    let mut i = offset;

    while i < data.len() {
        let unit = read_utf16be_unit(&data[i..]).ok_or(())?;
        i += 2;

        if unit >= 0xD800 && unit <= 0xDBFF {
            // High surrogate: expect a low surrogate
            let low = read_utf16be_unit(&data[i..]).ok_or(())?;
            i += 2;

            if !(0xDC00..=0xDFFF).contains(&low) {
                return Err(());
            }

            let cp = 0x10000 + ((unit as u32 - 0xD800) << 10) + (low as u32 - 0xDC00);
            let mut buf = [0u8; 4];
            let n = encode_codepoint_to_utf8(cp, &mut buf);
            if n == 0 {
                return Err(());
            }
            result.extend_from_slice(&buf[..n]);
        } else if unit >= 0xDC00 && unit <= 0xDFFF {
            // Unexpected low surrogate
            return Err(());
        } else {
            let cp = unit as u32;
            let mut buf = [0u8; 4];
            let n = encode_codepoint_to_utf8(cp, &mut buf);
            result.extend_from_slice(&buf[..n]);
        }
    }

    Ok(result)
}

/// Encode a Unicode codepoint as UTF-16LE bytes.
///
/// Returns the number of bytes written (2 or 4), or 0 if the codepoint is invalid.
fn encode_codepoint_to_utf16le(cp: u32, out: &mut [u8]) -> usize {
    if cp < 0x10000 {
        if out.len() < 2 {
            return 0;
        }
        let u = cp as u16;
        out[..2].copy_from_slice(&u.to_le_bytes());
        2
    } else if cp < 0x110000 {
        if out.len() < 4 {
            return 0;
        }
        let cp = cp - 0x10000;
        let high = 0xD800 | ((cp >> 10) as u16);
        let low = 0xDC00 | (cp as u16 & 0x3FF);
        out[..2].copy_from_slice(&high.to_le_bytes());
        out[2..4].copy_from_slice(&low.to_le_bytes());
        4
    } else {
        0
    }
}

/// Convert UTF-8 bytes to UTF-16LE.
///
/// Returns `Ok(converted_bytes)` on success, or `Err(())` on invalid UTF-8 input.
pub(crate) fn utf8_to_utf16le(data: &[u8]) -> Result<Vec<u8>, ()> {
    let s = core::str::from_utf8(data).map_err(|_| ())?;
    let mut result = Vec::with_capacity(data.len() * 2);

    for ch in s.chars() {
        let cp = ch as u32;
        let mut buf = [0u8; 4];
        let n = encode_codepoint_to_utf16le(cp, &mut buf);
        if n == 0 {
            return Err(());
        }
        result.extend_from_slice(&buf[..n]);
    }

    Ok(result)
}

// ═══════════════════════════════════════════════════════════════════════════════
// 4. ISO-8859-1 (Latin-1) handling
// ═══════════════════════════════════════════════════════════════════════════════

/// Convert Latin-1 (ISO-8859-1) bytes to UTF-8.
///
/// Latin-1 maps codepoints 0x00–0xFF directly to Unicode codepoints U+0000–U+00FF.
/// Each input byte produces either 1 or 2 UTF-8 bytes.
pub(crate) fn latin1_to_utf8(data: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(data.len() * 2);

    for &byte in data {
        let cp = byte as u32;
        let mut buf = [0u8; 2];
        let n = encode_codepoint_to_utf8(cp, &mut buf);
        result.extend_from_slice(&buf[..n]);
    }

    result
}

/// Convert UTF-8 bytes to Latin-1 (ISO-8859-1).
///
/// Returns `Err(())` if the input is not valid UTF-8 or contains codepoints
/// outside the Latin-1 range (U+0000–U+00FF).
pub(crate) fn utf8_to_latin1(data: &[u8]) -> Result<Vec<u8>, ()> {
    let s = core::str::from_utf8(data).map_err(|_| ())?;
    let mut result = Vec::with_capacity(data.len());

    for ch in s.chars() {
        let cp = ch as u32;
        if cp > 0xFF {
            return Err(());
        }
        result.push(cp as u8);
    }

    Ok(result)
}

// ═══════════════════════════════════════════════════════════════════════════════
// 5. Encoding handler registry
// ═══════════════════════════════════════════════════════════════════════════════

/// Initialize the built-in encoding handlers.
///
/// This function registers handlers for:
/// - UTF-8 (identity/no conversion)
/// - UTF-16LE
/// - UTF-16BE
/// - ISO-8859-1 (Latin-1)
/// - ASCII
///
/// Safe to call multiple times — only the first call has an effect.
pub(crate) fn init_encodings() {
    if ENCODING_INITIALIZED.swap(true, Ordering::SeqCst) {
        return;
    }

    register_builtin_handlers();
}

/// Register all built-in encoding handlers.
fn register_builtin_handlers() {
    // UTF-8 (identity handler — no conversion needed)
    register_handler(
        b"UTF-8\0",
        xmlCharEncoding::XML_CHAR_ENCODING_UTF8,
        xmlCharEncoding::XML_CHAR_ENCODING_UTF8,
        Some(utf8_input_func as xmlCharEncodingInputFunc),
        Some(utf8_output_func as xmlCharEncodingOutputFunc),
    );

    // UTF-16LE
    register_handler(
        b"UTF-16LE\0",
        xmlCharEncoding::XML_CHAR_ENCODING_UTF16LE,
        xmlCharEncoding::XML_CHAR_ENCODING_UTF16LE,
        Some(utf16le_input_func as xmlCharEncodingInputFunc),
        Some(utf16le_output_func as xmlCharEncodingOutputFunc),
    );

    // UTF-16BE
    register_handler(
        b"UTF-16BE\0",
        xmlCharEncoding::XML_CHAR_ENCODING_UTF16BE,
        xmlCharEncoding::XML_CHAR_ENCODING_UTF16BE,
        Some(utf16be_input_func as xmlCharEncodingInputFunc),
        Some(utf16be_output_func as xmlCharEncodingOutputFunc),
    );

    // ISO-8859-1 (Latin-1)
    register_handler(
        b"ISO-8859-1\0",
        xmlCharEncoding::XML_CHAR_ENCODING_8859_1,
        xmlCharEncoding::XML_CHAR_ENCODING_8859_1,
        Some(latin1_input_func as xmlCharEncodingInputFunc),
        Some(latin1_output_func as xmlCharEncodingOutputFunc),
    );

    // ASCII
    register_handler(
        b"ASCII\0",
        xmlCharEncoding::XML_CHAR_ENCODING_ASCII,
        xmlCharEncoding::XML_CHAR_ENCODING_ASCII,
        Some(ascii_input_func as xmlCharEncodingInputFunc),
        Some(ascii_output_func as xmlCharEncodingOutputFunc),
    );
}

/// Helper to create and register an encoding handler.
fn register_handler(
    name_bytes: &[u8],
    input_enc: xmlCharEncoding,
    output_enc: xmlCharEncoding,
    input_func: Option<xmlCharEncodingInputFunc>,
    output_func: Option<xmlCharEncodingOutputFunc>,
) {
    let name_raw =
        unsafe { crate::abi::allocator::xmlMemStrdup(name_bytes.as_ptr() as *const c_char) };
    if name_raw.is_null() {
        return;
    }

    let handler =
        unsafe { xmlMalloc(size_of::<_xmlCharEncodingHandler>()) } as *mut _xmlCharEncodingHandler;

    if handler.is_null() {
        unsafe { xmlFree(name_raw) };
        return;
    }

    unsafe {
        ptr::write(
            handler,
            _xmlCharEncodingHandler {
                name: name_raw as *mut c_char,
                input: EncodingInputUnion {
                    legacyFunc: input_func,
                },
                output: EncodingOutputUnion {
                    legacyFunc: output_func,
                },
                inputCtxt: ptr::null_mut(),
                outputCtxt: ptr::null_mut(),
                ctxtDtor: None,
                flags: 0,
            },
        );
    }

    add_encoding_handler(handler);
}

/// Clean up encoding handlers.
///
/// Frees all registered handlers and resets the registry.
pub(crate) fn cleanup_encodings() {
    let mut handlers = ENCODING_HANDLERS.write();
    for &handler in handlers.iter() {
        let ptr = handler.0;
        if !ptr.is_null() {
            unsafe {
                if !(*ptr).name.is_null() {
                    xmlFree((*ptr).name as *mut c_void);
                }
                xmlFree(ptr as *mut c_void);
            }
        }
    }
    handlers.clear();
    ENCODING_INITIALIZED.store(false, Ordering::SeqCst);
}

/// Find an encoding handler by name.
///
/// Searches the global handler registry for a handler whose name matches
/// (case-insensitive). Returns a pointer to the handler, or `ptr::null_mut()`
/// if not found.
pub(crate) fn find_encoding_handler(name: *const xmlChar) -> *mut _xmlCharEncodingHandler {
    if name.is_null() {
        return ptr::null_mut();
    }

    let name_str = unsafe {
        match CStr::from_ptr(name as *const c_char).to_bytes() {
            b"" => return ptr::null_mut(),
            s => s,
        }
    };

    let handlers = ENCODING_HANDLERS.read();
    for &handler in handlers.iter() {
        let ptr = handler.0;
        if ptr.is_null() {
            continue;
        }
        let h_name = unsafe {
            if (*ptr).name.is_null() {
                continue;
            }
            CStr::from_ptr((*ptr).name).to_bytes()
        };

        if name_str.eq_ignore_ascii_case(h_name) {
            return ptr;
        }
    }

    ptr::null_mut()
}

/// Add an encoding handler to the registry.
///
/// Returns 0 on success, -1 on failure (e.g., null pointer).
pub(crate) fn add_encoding_handler(handler: *mut _xmlCharEncodingHandler) -> c_int {
    if handler.is_null() {
        return -1;
    }

    let mut handlers = ENCODING_HANDLERS.write();
    handlers.push(HandlerPtr(handler));
    0
}

// ═══════════════════════════════════════════════════════════════════════════════
// 6. Encoding conversion functions
// ═══════════════════════════════════════════════════════════════════════════════

/// Input conversion: convert from handler's input encoding to UTF-8.
///
/// Calls the handler's `input.legacyFunc` callback. Returns bytes written or -1 on error.
pub(crate) fn char_enc_in_func(
    handler: *mut _xmlCharEncodingHandler,
    out: &mut [u8],
    in_data: &[u8],
) -> c_int {
    if handler.is_null() {
        return -1;
    }

    let h = unsafe { &*handler };
    let input_func = unsafe { h.input.legacyFunc };
    let input_func = match input_func {
        Some(f) => f,
        None => return -1,
    };

    let mut outlen = out.len() as c_int;
    let mut inlen = in_data.len() as c_int;

    unsafe { input_func(out.as_mut_ptr(), &mut outlen, in_data.as_ptr(), &mut inlen) }
}

/// Output conversion: convert from UTF-8 to handler's output encoding.
///
/// Calls the handler's `output.legacyFunc` callback. Returns bytes written or -1 on error.
pub(crate) fn char_enc_out_func(
    handler: *mut _xmlCharEncodingHandler,
    out: &mut [u8],
    in_data: &[u8],
) -> c_int {
    if handler.is_null() {
        return -1;
    }

    let h = unsafe { &*handler };
    let output_func = unsafe { h.output.legacyFunc };
    let output_func = match output_func {
        Some(f) => f,
        None => return -1,
    };

    let mut outlen = out.len() as c_int;
    let mut inlen = in_data.len() as c_int;

    unsafe { output_func(out.as_mut_ptr(), &mut outlen, in_data.as_ptr(), &mut inlen) }
}

/// Full input conversion (`xmlCharEncInFunc` equivalent).
///
/// Reads from the input `_xmlBuffer`, converts via the handler's `input.legacyFunc`,
/// and appends the result to the output `_xmlBuffer`.
///
/// Returns the number of bytes written to the output buffer, or -1 on error.
pub(crate) fn char_enc_in(
    handler: *mut _xmlCharEncodingHandler,
    out: *mut _xmlBuffer,
    in_: *mut _xmlBuffer,
) -> c_int {
    if handler.is_null() || out.is_null() || in_.is_null() {
        return -1;
    }

    let h = unsafe { &*handler };
    let input_func = unsafe { h.input.legacyFunc };
    let input_func = match input_func {
        Some(f) => f,
        None => return -1,
    };

    let in_buf = unsafe { &*in_ };
    let out_buf = unsafe { &mut *out };

    if in_buf.content.is_null() || in_buf.use_ == 0 {
        return 0;
    }

    let in_data = unsafe { core::slice::from_raw_parts(in_buf.content, in_buf.use_ as usize) };

    // Allocate an output buffer. A good heuristic is 2x input for UTF-16→UTF-8.
    let out_capacity = (in_buf.use_ as usize).saturating_mul(3).max(256);
    let mut out_vec = vec![0u8; out_capacity];
    let mut out_len = out_capacity as c_int;
    let mut in_len = in_buf.use_ as c_int;

    let ret = unsafe {
        input_func(
            out_vec.as_mut_ptr(),
            &mut out_len,
            in_data.as_ptr(),
            &mut in_len,
        )
    };

    if ret < 0 {
        return -1;
    }

    let written = ret as usize;

    // Append to output buffer
    append_to_xml_buffer(out_buf, &out_vec[..written]);

    written as c_int
}

/// Full output conversion (`xmlCharEncOutFunc` equivalent).
///
/// Reads from the input `_xmlBuffer` (UTF-8), converts via the handler's
/// `output.legacyFunc`, and appends the result to the output `_xmlBuffer`.
///
/// Returns the number of bytes written to the output buffer, or -1 on error.
pub(crate) fn char_enc_out(
    handler: *mut _xmlCharEncodingHandler,
    out: *mut _xmlBuffer,
    in_: *mut _xmlBuffer,
) -> c_int {
    if handler.is_null() || out.is_null() || in_.is_null() {
        return -1;
    }

    let h = unsafe { &*handler };
    let output_func = unsafe { h.output.legacyFunc };
    let output_func = match output_func {
        Some(f) => f,
        None => return -1,
    };

    let in_buf = unsafe { &*in_ };
    let out_buf = unsafe { &mut *out };

    if in_buf.content.is_null() || in_buf.use_ == 0 {
        return 0;
    }

    let in_data = unsafe { core::slice::from_raw_parts(in_buf.content, in_buf.use_ as usize) };

    let out_capacity = (in_buf.use_ as usize).saturating_mul(3).max(256);
    let mut out_vec = vec![0u8; out_capacity];
    let mut out_len = out_capacity as c_int;
    let mut in_len = in_buf.use_ as c_int;

    let ret = unsafe {
        output_func(
            out_vec.as_mut_ptr(),
            &mut out_len,
            in_data.as_ptr(),
            &mut in_len,
        )
    };

    if ret < 0 {
        return -1;
    }

    let written = ret as usize;

    // Append to output buffer
    append_to_xml_buffer(out_buf, &out_vec[..written]);

    written as c_int
}

/// Append bytes to an `_xmlBuffer`, reallocating if needed.
fn append_to_xml_buffer(buf: &mut _xmlBuffer, data: &[u8]) {
    if data.is_empty() {
        return;
    }

    let new_use = (buf.use_ as usize).saturating_add(data.len());
    if new_use > buf.size as usize {
        // Grow buffer: double or fit, whichever is larger
        let new_size = (buf.size as usize).saturating_mul(2).max(new_use).max(256);
        let new_content =
            unsafe { xmlRealloc(buf.content as *mut c_void, new_size) as *mut xmlChar };
        if new_content.is_null() {
            return; // Allocation failure — silently skip
        }
        buf.content = new_content;
        buf.size = new_size as c_uint;
    }

    unsafe {
        ptr::copy_nonoverlapping(
            data.as_ptr(),
            buf.content.add(buf.use_ as usize),
            data.len(),
        );
    }
    buf.use_ = new_use as c_uint;
}

// ═══════════════════════════════════════════════════════════════════════════════
// 7. Built-in encoding handler callbacks (extern "C")
// ═══════════════════════════════════════════════════════════════════════════════

// ── UTF-8 (identity) ──────────────────────────────────────────────────────

/// UTF-8 input function: identity (input is already UTF-8).
///
/// Simply copies bytes from input to output, up to the available space.
unsafe extern "C" fn utf8_input_func(
    out: *mut c_uchar,
    outlen: *mut c_int,
    in_: *const c_uchar,
    inlen: *mut c_int,
) -> c_int {
    let avail_out = *outlen as usize;
    let avail_in = *inlen as usize;
    let to_copy = avail_out.min(avail_in);

    if to_copy > 0 {
        ptr::copy_nonoverlapping(in_, out, to_copy);
    }

    *outlen = to_copy as c_int;
    *inlen = to_copy as c_int;
    to_copy as c_int
}

/// UTF-8 output function: identity (output is already UTF-8).
unsafe extern "C" fn utf8_output_func(
    out: *mut c_uchar,
    outlen: *mut c_int,
    in_: *const c_uchar,
    inlen: *mut c_int,
) -> c_int {
    utf8_input_func(out, outlen, in_, inlen)
}

// ── UTF-16LE ──────────────────────────────────────────────────────────────

/// UTF-16LE input function: convert UTF-16LE to UTF-8.
unsafe extern "C" fn utf16le_input_func(
    out: *mut c_uchar,
    outlen: *mut c_int,
    in_: *const c_uchar,
    inlen: *mut c_int,
) -> c_int {
    if out.is_null() || outlen.is_null() || in_.is_null() || inlen.is_null() {
        return -1;
    }

    let avail_in = *inlen as usize;
    let avail_out = *outlen as usize;

    if avail_in == 0 || avail_out == 0 {
        *outlen = 0;
        *inlen = 0;
        return 0;
    }

    let in_data = core::slice::from_raw_parts(in_, avail_in);
    let out_slice = core::slice::from_raw_parts_mut(out, avail_out);

    // Use the safe wrapper
    let result = match utf16le_to_utf8(in_data) {
        Ok(v) => v,
        Err(()) => return -1,
    };

    let written = result.len().min(avail_out);
    if written > 0 {
        ptr::copy_nonoverlapping(result.as_ptr(), out, written);
    }

    *outlen = written as c_int;
    *inlen = avail_in as c_int; // All input consumed
    written as c_int
}

/// UTF-16LE output function: convert UTF-8 to UTF-16LE.
unsafe extern "C" fn utf16le_output_func(
    out: *mut c_uchar,
    outlen: *mut c_int,
    in_: *const c_uchar,
    inlen: *mut c_int,
) -> c_int {
    if out.is_null() || outlen.is_null() || in_.is_null() || inlen.is_null() {
        return -1;
    }

    let avail_in = *inlen as usize;
    let avail_out = *outlen as usize;

    if avail_in == 0 || avail_out == 0 {
        *outlen = 0;
        *inlen = 0;
        return 0;
    }

    let in_data = core::slice::from_raw_parts(in_, avail_in);
    let out_slice = core::slice::from_raw_parts_mut(out, avail_out);

    let result = match utf8_to_utf16le(in_data) {
        Ok(v) => v,
        Err(()) => return -1,
    };

    let written = result.len().min(avail_out);
    if written > 0 {
        ptr::copy_nonoverlapping(result.as_ptr(), out, written);
    }

    *outlen = written as c_int;
    *inlen = avail_in as c_int;
    written as c_int
}

// ── UTF-16BE ──────────────────────────────────────────────────────────────

/// UTF-16BE input function: convert UTF-16BE to UTF-8.
unsafe extern "C" fn utf16be_input_func(
    out: *mut c_uchar,
    outlen: *mut c_int,
    in_: *const c_uchar,
    inlen: *mut c_int,
) -> c_int {
    if out.is_null() || outlen.is_null() || in_.is_null() || inlen.is_null() {
        return -1;
    }

    let avail_in = *inlen as usize;
    let avail_out = *outlen as usize;

    if avail_in == 0 || avail_out == 0 {
        *outlen = 0;
        *inlen = 0;
        return 0;
    }

    let in_data = core::slice::from_raw_parts(in_, avail_in);
    let out_slice = core::slice::from_raw_parts_mut(out, avail_out);

    let result = match utf16be_to_utf8(in_data) {
        Ok(v) => v,
        Err(()) => return -1,
    };

    let written = result.len().min(avail_out);
    if written > 0 {
        ptr::copy_nonoverlapping(result.as_ptr(), out, written);
    }

    *outlen = written as c_int;
    *inlen = avail_in as c_int;
    written as c_int
}

/// UTF-16BE output function: convert UTF-8 to UTF-16BE.
unsafe extern "C" fn utf16be_output_func(
    out: *mut c_uchar,
    outlen: *mut c_int,
    in_: *const c_uchar,
    inlen: *mut c_int,
) -> c_int {
    if out.is_null() || outlen.is_null() || in_.is_null() || inlen.is_null() {
        return -1;
    }

    let avail_in = *inlen as usize;
    let avail_out = *outlen as usize;

    if avail_in == 0 || avail_out == 0 {
        *outlen = 0;
        *inlen = 0;
        return 0;
    }

    let in_data = core::slice::from_raw_parts(in_, avail_in);

    // First convert to UTF-16LE, then swap bytes
    let le_result = match utf8_to_utf16le(in_data) {
        Ok(v) => v,
        Err(()) => return -1,
    };

    // Swap byte pairs to get UTF-16BE
    let mut result = le_result;
    for chunk in result.chunks_exact_mut(2) {
        chunk.swap(0, 1);
    }

    let written = result.len().min(avail_out);
    if written > 0 {
        ptr::copy_nonoverlapping(result.as_ptr(), out, written);
    }

    *outlen = written as c_int;
    *inlen = avail_in as c_int;
    written as c_int
}

// ── ISO-8859-1 (Latin-1) ─────────────────────────────────────────────────

/// Latin-1 input function: convert ISO-8859-1 to UTF-8.
unsafe extern "C" fn latin1_input_func(
    out: *mut c_uchar,
    outlen: *mut c_int,
    in_: *const c_uchar,
    inlen: *mut c_int,
) -> c_int {
    if out.is_null() || outlen.is_null() || in_.is_null() || inlen.is_null() {
        return -1;
    }

    let avail_in = *inlen as usize;
    let avail_out = *outlen as usize;

    if avail_in == 0 || avail_out == 0 {
        *outlen = 0;
        *inlen = 0;
        return 0;
    }

    let in_data = core::slice::from_raw_parts(in_, avail_in);
    let out_slice = core::slice::from_raw_parts_mut(out, avail_out);

    let mut in_pos = 0;
    let mut out_pos = 0;

    while in_pos < avail_in && out_pos < avail_out {
        let byte = in_data[in_pos];
        in_pos += 1;

        if byte < 0x80 {
            // Single byte UTF-8
            if out_pos < avail_out {
                out_slice[out_pos] = byte;
                out_pos += 1;
            } else {
                break;
            }
        } else {
            // Two byte UTF-8: 0xC0 | (byte >> 6), 0x80 | (byte & 0x3F)
            // For byte 0x80-0xFF, the encoding is 0xC2-0xC3 followed by continuation
            if out_pos + 1 < avail_out {
                out_slice[out_pos] = 0xC2 | (byte >> 6);
                out_slice[out_pos + 1] = 0x80 | (byte & 0x3F);
                out_pos += 2;
            } else {
                break;
            }
        }
    }

    *outlen = out_pos as c_int;
    *inlen = in_pos as c_int;
    out_pos as c_int
}

/// Latin-1 output function: convert UTF-8 to ISO-8859-1.
unsafe extern "C" fn latin1_output_func(
    out: *mut c_uchar,
    outlen: *mut c_int,
    in_: *const c_uchar,
    inlen: *mut c_int,
) -> c_int {
    if out.is_null() || outlen.is_null() || in_.is_null() || inlen.is_null() {
        return -1;
    }

    let avail_in = *inlen as usize;
    let avail_out = *outlen as usize;

    if avail_in == 0 || avail_out == 0 {
        *outlen = 0;
        *inlen = 0;
        return 0;
    }

    let in_data = core::slice::from_raw_parts(in_, avail_in);
    let out_slice = core::slice::from_raw_parts_mut(out, avail_out);

    let mut in_pos = 0;
    let mut out_pos = 0;

    while in_pos < avail_in && out_pos < avail_out {
        let byte = in_data[in_pos];
        in_pos += 1;

        if byte < 0x80 {
            // ASCII — direct mapping
            out_slice[out_pos] = byte;
            out_pos += 1;
        } else if byte >= 0xC2 && byte <= 0xC3 {
            // Two-byte UTF-8 for codepoints U+0080–U+00FF
            if in_pos < avail_in {
                let second = in_data[in_pos];
                in_pos += 1;
                if second & 0xC0 != 0x80 {
                    return -1; // Invalid continuation byte
                }
                let cp = ((byte as u32 & 0x1F) << 6) | (second as u32 & 0x3F);
                if cp > 0xFF {
                    return -1; // Outside Latin-1 range
                }
                out_slice[out_pos] = cp as u8;
                out_pos += 1;
            } else {
                return -1; // Truncated
            }
        } else if byte >= 0x80 && byte <= 0xBF {
            // Unexpected continuation byte
            return -1;
        } else {
            // Multi-byte sequence for codepoints > U+00FF
            // Skip the rest of the sequence and return error
            return -1;
        }
    }

    *outlen = out_pos as c_int;
    *inlen = in_pos as c_int;
    out_pos as c_int
}

// ── ASCII ─────────────────────────────────────────────────────────────────

/// ASCII input function: verify and pass through ASCII data to UTF-8.
unsafe extern "C" fn ascii_input_func(
    out: *mut c_uchar,
    outlen: *mut c_int,
    in_: *const c_uchar,
    inlen: *mut c_int,
) -> c_int {
    if out.is_null() || outlen.is_null() || in_.is_null() || inlen.is_null() {
        return -1;
    }

    let avail_in = *inlen as usize;
    let avail_out = *outlen as usize;

    if avail_in == 0 || avail_out == 0 {
        *outlen = 0;
        *inlen = 0;
        return 0;
    }

    let in_data = core::slice::from_raw_parts(in_, avail_in);
    let out_slice = core::slice::from_raw_parts_mut(out, avail_out);

    let mut pos = 0;
    while pos < avail_in && pos < avail_out {
        let byte = in_data[pos];
        if byte > 0x7F {
            return -1; // Not valid ASCII
        }
        out_slice[pos] = byte;
        pos += 1;
    }

    *outlen = pos as c_int;
    *inlen = pos as c_int;
    pos as c_int
}

/// ASCII output function: verify and pass through UTF-8 data that is ASCII.
unsafe extern "C" fn ascii_output_func(
    out: *mut c_uchar,
    outlen: *mut c_int,
    in_: *const c_uchar,
    inlen: *mut c_int,
) -> c_int {
    // For output, ASCII handler requires that input is already ASCII
    ascii_input_func(out, outlen, in_, inlen)
}

// ═══════════════════════════════════════════════════════════════════════════════
// 8. ABI export functions (called from exports_xml2.rs)
// ═══════════════════════════════════════════════════════════════════════════════

/// `xmlFindCharEncodingHandler` implementation.
///
/// Finds an encoding handler by name. Returns a pointer to the handler,
/// or `ptr::null_mut()` if not found.
pub(crate) fn xmlFindCharEncodingHandler(name: *const c_char) -> *mut _xmlCharEncodingHandler {
    if name.is_null() {
        return ptr::null_mut();
    }
    find_encoding_handler(name as *const xmlChar)
}

/// `xmlGetCharEncodingName` implementation.
///
/// Returns the canonical name for an encoding, or `ptr::null()` if unknown.
pub(crate) fn xmlGetCharEncodingName(enc: xmlCharEncoding) -> *const c_char {
    // Return null-terminated C strings using static CStr literals
    match enc {
        xmlCharEncoding::XML_CHAR_ENCODING_UTF8 => c"UTF-8".as_ptr(),
        xmlCharEncoding::XML_CHAR_ENCODING_UTF16LE => c"UTF-16LE".as_ptr(),
        xmlCharEncoding::XML_CHAR_ENCODING_UTF16BE => c"UTF-16BE".as_ptr(),
        xmlCharEncoding::XML_CHAR_ENCODING_UCS4LE => c"UCS-4LE".as_ptr(),
        xmlCharEncoding::XML_CHAR_ENCODING_UCS4BE => c"UCS-4BE".as_ptr(),
        xmlCharEncoding::XML_CHAR_ENCODING_EBCDIC => c"EBCDIC".as_ptr(),
        xmlCharEncoding::XML_CHAR_ENCODING_UCS4_2143 => c"UCS-4-2143".as_ptr(),
        xmlCharEncoding::XML_CHAR_ENCODING_UCS4_3412 => c"UCS-4-3412".as_ptr(),
        xmlCharEncoding::XML_CHAR_ENCODING_UCS2 => c"UCS-2".as_ptr(),
        xmlCharEncoding::XML_CHAR_ENCODING_8859_1 => c"ISO-8859-1".as_ptr(),
        xmlCharEncoding::XML_CHAR_ENCODING_8859_2 => c"ISO-8859-2".as_ptr(),
        xmlCharEncoding::XML_CHAR_ENCODING_8859_3 => c"ISO-8859-3".as_ptr(),
        xmlCharEncoding::XML_CHAR_ENCODING_8859_4 => c"ISO-8859-4".as_ptr(),
        xmlCharEncoding::XML_CHAR_ENCODING_8859_5 => c"ISO-8859-5".as_ptr(),
        xmlCharEncoding::XML_CHAR_ENCODING_8859_6 => c"ISO-8859-6".as_ptr(),
        xmlCharEncoding::XML_CHAR_ENCODING_8859_7 => c"ISO-8859-7".as_ptr(),
        xmlCharEncoding::XML_CHAR_ENCODING_8859_8 => c"ISO-8859-8".as_ptr(),
        xmlCharEncoding::XML_CHAR_ENCODING_8859_9 => c"ISO-8859-9".as_ptr(),
        xmlCharEncoding::XML_CHAR_ENCODING_2022_JP => c"ISO-2022-JP".as_ptr(),
        xmlCharEncoding::XML_CHAR_ENCODING_SHIFT_JIS => c"SHIFT_JIS".as_ptr(),
        xmlCharEncoding::XML_CHAR_ENCODING_EUC_JP => c"EUC-JP".as_ptr(),
        xmlCharEncoding::XML_CHAR_ENCODING_ASCII => c"ASCII".as_ptr(),
        _ => ptr::null(),
    }
}

/// `xmlParseCharEncoding` implementation.
///
/// Parses an encoding name string to an `xmlCharEncoding` enum value,
/// returned as `c_int`.
pub(crate) fn xmlParseCharEncoding(name: *const c_char) -> c_int {
    if name.is_null() {
        return xmlCharEncoding::XML_CHAR_ENCODING_NONE as c_int;
    }
    let bytes = unsafe { CStr::from_ptr(name).to_bytes() };
    encoding_from_name(bytes) as c_int
}

/// `xmlCharEncInFunc` implementation.
///
/// Converts the input buffer's encoding to UTF-8 using the given handler.
pub(crate) fn xmlCharEncInFunc(
    handler: *mut _xmlCharEncodingHandler,
    out: *mut _xmlBuffer,
    in_: *mut _xmlBuffer,
) -> c_int {
    char_enc_in(handler, out, in_)
}

/// `xmlCharEncOutFunc` implementation.
///
/// Converts the input buffer from UTF-8 to the handler's output encoding.
pub(crate) fn xmlCharEncOutFunc(
    handler: *mut _xmlCharEncodingHandler,
    out: *mut _xmlBuffer,
    in_: *mut _xmlBuffer,
) -> c_int {
    char_enc_out(handler, out, in_)
}

/// `xmlNewCharEncodingHandler` implementation.
///
/// Creates a new encoding handler with the given name and conversion functions.
/// The name string is duplicated. Returns a pointer to the new handler,
/// or `ptr::null_mut()` on allocation failure.
pub(crate) fn xmlNewCharEncodingHandler(
    name: *const c_char,
    input: xmlCharEncodingInputFunc,
    output: xmlCharEncodingOutputFunc,
) -> *mut _xmlCharEncodingHandler {
    if name.is_null() {
        return ptr::null_mut();
    }

    let name_raw = unsafe { crate::abi::allocator::xmlMemStrdup(name) };
    if name_raw.is_null() {
        return ptr::null_mut();
    }

    let handler =
        unsafe { xmlMalloc(size_of::<_xmlCharEncodingHandler>()) } as *mut _xmlCharEncodingHandler;

    if handler.is_null() {
        unsafe { xmlFree(name_raw) };
        return ptr::null_mut();
    }

    unsafe {
        ptr::write(
            handler,
            _xmlCharEncodingHandler {
                name: name_raw as *mut c_char,
                input: EncodingInputUnion {
                    legacyFunc: Some(input),
                },
                output: EncodingOutputUnion {
                    legacyFunc: Some(output),
                },
                inputCtxt: ptr::null_mut(),
                outputCtxt: ptr::null_mut(),
                ctxtDtor: None,
                flags: 0,
            },
        );
    }

    handler
}

/// `xmlDelEncodingHandler` implementation.
///
/// Frees an encoding handler previously created with `xmlNewCharEncodingHandler`.
pub(crate) fn xmlDelEncodingHandler(handler: *mut _xmlCharEncodingHandler) {
    if handler.is_null() {
        return;
    }

    // Remove from registry if present
    {
        let mut handlers = ENCODING_HANDLERS.write();
        handlers.retain(|&h| h.0 != handler);
    }

    unsafe {
        if !(*handler).name.is_null() {
            xmlFree((*handler).name as *mut c_void);
        }
        xmlFree(handler as *mut c_void);
    }
}

/// `xmlInitCharEncodingHandlers` implementation.
pub(crate) fn xmlInitCharEncodingHandlers() {
    init_encodings();
}

/// `xmlCleanupCharEncodingHandlers` implementation.
pub(crate) fn xmlCleanupCharEncodingHandlers() {
    cleanup_encodings();
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ── BOM detection ──────────────────────────────────────────────────────

    #[test]
    fn test_detect_bom_utf8() {
        let data = [0xEF, 0xBB, 0xBF, b'<', b'?', b'x', b'm', b'l'];
        assert_eq!(
            detect_encoding_from_bom(&data),
            xmlCharEncoding::XML_CHAR_ENCODING_UTF8
        );
    }

    #[test]
    fn test_detect_bom_utf16le() {
        let data = [0xFF, 0xFE, 0x00, 0x01];
        assert_eq!(
            detect_encoding_from_bom(&data),
            xmlCharEncoding::XML_CHAR_ENCODING_UTF16LE
        );
    }

    #[test]
    fn test_detect_bom_utf16be() {
        let data = [0xFE, 0xFF, 0x00, 0x01];
        assert_eq!(
            detect_encoding_from_bom(&data),
            xmlCharEncoding::XML_CHAR_ENCODING_UTF16BE
        );
    }

    #[test]
    fn test_detect_bom_none() {
        let data = b"<xml>";
        assert_eq!(
            detect_encoding_from_bom(data),
            xmlCharEncoding::XML_CHAR_ENCODING_NONE
        );
    }

    #[test]
    fn test_detect_bom_empty() {
        assert_eq!(
            detect_encoding_from_bom(b""),
            xmlCharEncoding::XML_CHAR_ENCODING_NONE
        );
    }

    // ── Encoding from declaration ──────────────────────────────────────────

    #[test]
    fn test_detect_encoding_declaration_utf8() {
        let data = b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>";
        let result = detect_encoding_from_declaration(data);
        assert_eq!(result, Some(b"utf-8".to_vec()));
    }

    #[test]
    fn test_detect_encoding_declaration_iso() {
        let data = b"<?xml version='1.0' encoding='ISO-8859-1'?>";
        let result = detect_encoding_from_declaration(data);
        assert_eq!(result, Some(b"iso-8859-1".to_vec()));
    }

    #[test]
    fn test_detect_encoding_declaration_none() {
        let data = b"<?xml version=\"1.0\"?>";
        let result = detect_encoding_from_declaration(data);
        assert!(result.is_none());
    }

    #[test]
    fn test_detect_encoding_declaration_no_xml() {
        let data = b"<root>";
        let result = detect_encoding_from_declaration(data);
        assert!(result.is_none());
    }

    #[test]
    fn test_detect_encoding_declaration_with_bom() {
        let mut data = vec![0xEF, 0xBB, 0xBF];
        data.extend_from_slice(b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
        let result = detect_encoding_from_declaration(&data);
        assert_eq!(result, Some(b"utf-8".to_vec()));
    }

    // ── Encoding from name ─────────────────────────────────────────────────

    #[test]
    fn test_encoding_from_name_utf8() {
        assert_eq!(
            encoding_from_name(b"UTF-8"),
            xmlCharEncoding::XML_CHAR_ENCODING_UTF8
        );
        assert_eq!(
            encoding_from_name(b"utf8"),
            xmlCharEncoding::XML_CHAR_ENCODING_UTF8
        );
    }

    #[test]
    fn test_encoding_from_name_utf16() {
        assert_eq!(
            encoding_from_name(b"UTF-16LE"),
            xmlCharEncoding::XML_CHAR_ENCODING_UTF16LE
        );
        assert_eq!(
            encoding_from_name(b"UTF-16BE"),
            xmlCharEncoding::XML_CHAR_ENCODING_UTF16BE
        );
        assert_eq!(
            encoding_from_name(b"utf-16"),
            xmlCharEncoding::XML_CHAR_ENCODING_UTF16LE
        );
    }

    #[test]
    fn test_encoding_from_name_latin1() {
        assert_eq!(
            encoding_from_name(b"ISO-8859-1"),
            xmlCharEncoding::XML_CHAR_ENCODING_8859_1
        );
        assert_eq!(
            encoding_from_name(b"Latin1"),
            xmlCharEncoding::XML_CHAR_ENCODING_8859_1
        );
    }

    #[test]
    fn test_encoding_from_name_ascii() {
        assert_eq!(
            encoding_from_name(b"ASCII"),
            xmlCharEncoding::XML_CHAR_ENCODING_ASCII
        );
        assert_eq!(
            encoding_from_name(b"US-ASCII"),
            xmlCharEncoding::XML_CHAR_ENCODING_ASCII
        );
    }

    #[test]
    fn test_encoding_from_name_error() {
        assert_eq!(
            encoding_from_name(b"invalid-encoding"),
            xmlCharEncoding::XML_CHAR_ENCODING_ERROR
        );
    }

    #[test]
    fn test_encoding_from_name_empty() {
        assert_eq!(
            encoding_from_name(b""),
            xmlCharEncoding::XML_CHAR_ENCODING_ERROR
        );
    }

    // ── Encoding name ──────────────────────────────────────────────────────

    #[test]
    fn test_encoding_name_utf8() {
        assert_eq!(
            encoding_name(xmlCharEncoding::XML_CHAR_ENCODING_UTF8),
            Some(b"UTF-8" as &[u8])
        );
    }

    #[test]
    fn test_encoding_name_utf16le() {
        assert_eq!(
            encoding_name(xmlCharEncoding::XML_CHAR_ENCODING_UTF16LE),
            Some(b"UTF-16LE" as &[u8])
        );
    }

    #[test]
    fn test_encoding_name_none() {
        assert!(encoding_name(xmlCharEncoding::XML_CHAR_ENCODING_NONE).is_none());
    }

    #[test]
    fn test_encoding_name_error() {
        assert!(encoding_name(xmlCharEncoding::XML_CHAR_ENCODING_ERROR).is_none());
    }

    // ── UTF-8 validation ───────────────────────────────────────────────────

    #[test]
    fn test_utf8_valid_ascii() {
        assert!(utf8_valid(b"hello world"));
    }

    #[test]
    fn test_utf8_valid_multi_byte() {
        assert!(utf8_valid("héllo wörld 🌍".as_bytes()));
    }

    #[test]
    fn test_utf8_valid_empty() {
        assert!(utf8_valid(b""));
    }

    #[test]
    fn test_utf8_invalid() {
        assert!(!utf8_valid(&[0xFF, 0xFE, 0x00]));
    }

    // ── XML char validation ────────────────────────────────────────────────

    #[test]
    fn test_valid_xml_chars() {
        assert!(is_valid_xml_char(0x9)); // Tab
        assert!(is_valid_xml_char(0xA)); // LF
        assert!(is_valid_xml_char(0xD)); // CR
        assert!(is_valid_xml_char(0x20)); // Space
        assert!(is_valid_xml_char(0x41)); // 'A'
        assert!(is_valid_xml_char(0xD7FF));
        assert!(is_valid_xml_char(0xE000));
        assert!(is_valid_xml_char(0xFFFD));
        assert!(is_valid_xml_char(0x10000));
        assert!(is_valid_xml_char(0x10FFFF));
    }

    #[test]
    fn test_invalid_xml_chars() {
        assert!(!is_valid_xml_char(0x00));
        assert!(!is_valid_xml_char(0x08));
        assert!(!is_valid_xml_char(0x0B));
        assert!(!is_valid_xml_char(0x0C));
        assert!(!is_valid_xml_char(0x0E));
        assert!(!is_valid_xml_char(0x1F));
        assert!(!is_valid_xml_char(0xD800)); // Surrogate
        assert!(!is_valid_xml_char(0xDFFF)); // Surrogate
        assert!(!is_valid_xml_char(0xFFFE));
        assert!(!is_valid_xml_char(0xFFFF));
        assert!(!is_valid_xml_char(0x110000));
    }

    // ── UTF-16LE to UTF-8 ──────────────────────────────────────────────────

    #[test]
    fn test_utf16le_to_utf8_ascii() {
        // "AB" in UTF-16LE
        let data = [b'A', 0x00, b'B', 0x00];
        let result = utf16le_to_utf8(&data).unwrap();
        assert_eq!(result, b"AB");
    }

    #[test]
    fn test_utf16le_to_utf8_bom() {
        let mut data = vec![0xFF, 0xFE]; // BOM
        data.extend_from_slice(&[b'A', 0x00, b'B', 0x00]);
        let result = utf16le_to_utf8(&data).unwrap();
        assert_eq!(result, b"AB");
    }

    #[test]
    fn test_utf16le_to_utf8_bmp() {
        // U+00E9 (é) in UTF-16LE = 0xE9 0x00
        let data = [0xE9, 0x00];
        let result = utf16le_to_utf8(&data).unwrap();
        assert_eq!(result, "é".as_bytes());
    }

    #[test]
    fn test_utf16le_to_utf8_supplementary() {
        // U+1F600 (😀) in UTF-16LE = 0x3D 0xD8 0x00 0xDE
        let data = [0x3D, 0xD8, 0x00, 0xDE];
        let result = utf16le_to_utf8(&data).unwrap();
        assert_eq!(result, "😀".as_bytes());
    }

    #[test]
    fn test_utf16le_to_utf8_unpaired_surrogate() {
        let data = [0x00, 0xD8]; // High surrogate without low
        assert!(utf16le_to_utf8(&data).is_err());
    }

    #[test]
    fn test_utf16le_to_utf8_truncated() {
        let data = [0x00]; // Odd length
        assert!(utf16le_to_utf8(&data).is_err());
    }

    #[test]
    fn test_utf16le_to_utf8_empty() {
        let result = utf16le_to_utf8(b"").unwrap();
        assert!(result.is_empty());
    }

    // ── UTF-16BE to UTF-8 ──────────────────────────────────────────────────

    #[test]
    fn test_utf16be_to_utf8_ascii() {
        let data = [0x00, b'A', 0x00, b'B'];
        let result = utf16be_to_utf8(&data).unwrap();
        assert_eq!(result, b"AB");
    }

    #[test]
    fn test_utf16be_to_utf8_bom() {
        let mut data = vec![0xFE, 0xFF]; // BOM
        data.extend_from_slice(&[0x00, b'A', 0x00, b'B']);
        let result = utf16be_to_utf8(&data).unwrap();
        assert_eq!(result, b"AB");
    }

    #[test]
    fn test_utf16be_to_utf8_supplementary() {
        // U+1F600 (😀) in UTF-16BE = 0xD8 0x3D 0xDE 0x00
        let data = [0xD8, 0x3D, 0xDE, 0x00];
        let result = utf16be_to_utf8(&data).unwrap();
        assert_eq!(result, "😀".as_bytes());
    }

    #[test]
    fn test_utf16be_to_utf8_empty() {
        let result = utf16be_to_utf8(b"").unwrap();
        assert!(result.is_empty());
    }

    // ── UTF-8 to UTF-16LE ──────────────────────────────────────────────────

    #[test]
    fn test_utf8_to_utf16le_ascii() {
        let result = utf8_to_utf16le(b"AB").unwrap();
        assert_eq!(result, [b'A', 0x00, b'B', 0x00]);
    }

    #[test]
    fn test_utf8_to_utf16le_bmp() {
        let result = utf8_to_utf16le("é".as_bytes()).unwrap();
        assert_eq!(result, [0xE9, 0x00]);
    }

    #[test]
    fn test_utf8_to_utf16le_supplementary() {
        let result = utf8_to_utf16le("😀".as_bytes()).unwrap();
        assert_eq!(result, [0x3D, 0xD8, 0x00, 0xDE]);
    }

    #[test]
    fn test_utf8_to_utf16le_invalid_utf8() {
        assert!(utf8_to_utf16le(&[0xFF]).is_err());
    }

    #[test]
    fn test_utf8_to_utf16le_empty() {
        let result = utf8_to_utf16le(b"").unwrap();
        assert!(result.is_empty());
    }

    // ── Latin-1 to UTF-8 ───────────────────────────────────────────────────

    #[test]
    fn test_latin1_to_utf8_ascii() {
        let result = latin1_to_utf8(b"ABC");
        assert_eq!(result, b"ABC");
    }

    #[test]
    fn test_latin1_to_utf8_accented() {
        // 0xE9 = é in Latin-1
        let result = latin1_to_utf8(&[0xE9]);
        assert_eq!(result, "é".as_bytes());
    }

    #[test]
    fn test_latin1_to_utf8_all_255() {
        let result = latin1_to_utf8(&[0xFF]);
        // U+00FF = ÿ, UTF-8: 0xC3 0xBF
        assert_eq!(result, [0xC3, 0xBF]);
    }

    #[test]
    fn test_latin1_to_utf8_empty() {
        let result = latin1_to_utf8(b"");
        assert!(result.is_empty());
    }

    #[test]
    fn test_latin1_to_utf8_mixed() {
        let result = latin1_to_utf8(b"caf\xE9");
        assert_eq!(result, "café".as_bytes());
    }

    // ── UTF-8 to Latin-1 ───────────────────────────────────────────────────

    #[test]
    fn test_utf8_to_latin1_ascii() {
        let result = utf8_to_latin1(b"ABC").unwrap();
        assert_eq!(result, b"ABC");
    }

    #[test]
    fn test_utf8_to_latin1_accented() {
        let result = utf8_to_latin1("é".as_bytes()).unwrap();
        assert_eq!(result, [0xE9]);
    }

    #[test]
    fn test_utf8_to_latin1_out_of_range() {
        assert!(utf8_to_latin1("€".as_bytes()).is_err()); // U+20AC not in Latin-1
    }

    #[test]
    fn test_utf8_to_latin1_invalid_utf8() {
        assert!(utf8_to_latin1(&[0xFF]).is_err());
    }

    #[test]
    fn test_utf8_to_latin1_empty() {
        let result = utf8_to_latin1(b"").unwrap();
        assert!(result.is_empty());
    }

    // ── Encoding handler registry ──────────────────────────────────────────

    #[test]
    fn test_init_and_find_encodings() {
        init_encodings();

        let utf8_name: *const xmlChar = b"UTF-8\0".as_ptr();
        assert!(!find_encoding_handler(utf8_name).is_null());

        let utf16le_name: *const xmlChar = b"UTF-16LE\0".as_ptr();
        assert!(!find_encoding_handler(utf16le_name).is_null());

        let utf16be_name: *const xmlChar = b"UTF-16BE\0".as_ptr();
        assert!(!find_encoding_handler(utf16be_name).is_null());

        let latin1_name: *const xmlChar = b"ISO-8859-1\0".as_ptr();
        assert!(!find_encoding_handler(latin1_name).is_null());

        let ascii_name: *const xmlChar = b"ASCII\0".as_ptr();
        assert!(!find_encoding_handler(ascii_name).is_null());

        // Case insensitive
        let lower_name: *const xmlChar = b"utf-8\0".as_ptr();
        assert!(!find_encoding_handler(lower_name).is_null());
    }

    #[test]
    fn test_find_encoding_handler_not_found() {
        let name: *const xmlChar = b"NONEXISTENT\0".as_ptr();
        assert!(find_encoding_handler(name).is_null());
    }

    #[test]
    fn test_find_encoding_handler_null() {
        assert!(find_encoding_handler(ptr::null()).is_null());
    }

    #[test]
    fn test_add_encoding_handler() {
        let handler = unsafe {
            xmlMalloc(size_of::<_xmlCharEncodingHandler>()) as *mut _xmlCharEncodingHandler
        };
        assert!(!handler.is_null());

        let name =
            unsafe { crate::abi::allocator::xmlMemStrdup(b"TEST-ENC\0".as_ptr() as *const c_char) };
        unsafe {
            ptr::write(
                handler,
                _xmlCharEncodingHandler {
                    name: name as *mut c_char,
                    input: EncodingInputUnion { legacyFunc: None },
                    output: EncodingOutputUnion { legacyFunc: None },
                    inputCtxt: ptr::null_mut(),
                    outputCtxt: ptr::null_mut(),
                    ctxtDtor: None,
                    flags: 0,
                },
            );
        }

        assert_eq!(add_encoding_handler(handler), 0);

        let found = find_encoding_handler(b"TEST-ENC\0".as_ptr() as *const xmlChar);
        assert_eq!(found, handler);

        // Remove from registry before freeing to avoid dangling pointers
        {
            let mut handlers = ENCODING_HANDLERS.write();
            handlers.retain(|&h| h.0 != handler);
        }

        unsafe {
            xmlFree(name as *mut c_void);
            xmlFree(handler as *mut c_void);
        }
    }

    // ── Conversion round-trips ─────────────────────────────────────────────

    #[test]
    fn test_utf16le_roundtrip() {
        let original = b"Hello, World! UTF-16LE test: \xC3\xA9\xF0\x9F\x98\x80";
        let utf16 = utf8_to_utf16le(original).unwrap();
        let back = utf16le_to_utf8(&utf16).unwrap();
        assert_eq!(original.to_vec(), back);
    }

    #[test]
    fn test_utf16be_roundtrip() {
        let original = b"Hello, World! UTF-16BE test: \xC3\xA9\xF0\x9F\x98\x80";
        let utf16le = utf8_to_utf16le(original).unwrap();
        // Convert LE to BE by swapping bytes
        let mut utf16be = utf16le.clone();
        for chunk in utf16be.chunks_exact_mut(2) {
            chunk.swap(0, 1);
        }
        let back = utf16be_to_utf8(&utf16be).unwrap();
        assert_eq!(original.to_vec(), back);
    }

    #[test]
    fn test_latin1_roundtrip() {
        let original: Vec<u8> = (0x00..=0xFF).collect();
        let utf8 = latin1_to_utf8(&original);
        let back = utf8_to_latin1(&utf8).unwrap();
        assert_eq!(original, back);
    }

    // ── Built-in handler callbacks ─────────────────────────────────────────

    #[test]
    fn test_utf8_handler_identity() {
        let input = b"Hello, UTF-8!";
        let mut output = [0u8; 64];
        let mut outlen = output.len() as c_int;
        let mut inlen = input.len() as c_int;

        let ret = unsafe {
            utf8_input_func(output.as_mut_ptr(), &mut outlen, input.as_ptr(), &mut inlen)
        };

        assert_eq!(ret, input.len() as c_int);
        assert_eq!(&output[..ret as usize], input);
        assert_eq!(inlen, input.len() as c_int);
    }

    #[test]
    fn test_utf16le_handler_roundtrip() {
        init_encodings();

        let original = b"Hello UTF-16LE!";
        let mut utf16_buf = [0u8; 128];
        let mut outlen = utf16_buf.len() as c_int;
        let mut inlen = original.len() as c_int;

        let written = unsafe {
            utf16le_output_func(
                utf16_buf.as_mut_ptr(),
                &mut outlen,
                original.as_ptr(),
                &mut inlen,
            )
        };
        assert!(written > 0);

        // Now decode back
        let mut decoded = [0u8; 128];
        let mut outlen2 = decoded.len() as c_int;
        let mut inlen2 = written;

        let written2 = unsafe {
            utf16le_input_func(
                decoded.as_mut_ptr(),
                &mut outlen2,
                utf16_buf.as_ptr(),
                &mut inlen2,
            )
        };
        assert_eq!(written2 as usize, original.len());
        assert_eq!(&decoded[..written2 as usize], original);
    }

    // ── xmlBuffer operations ───────────────────────────────────────────────

    #[test]
    fn test_append_to_xml_buffer() {
        unsafe {
            let content = xmlMalloc(64) as *mut xmlChar;
            assert!(!content.is_null());

            let mut buf = _xmlBuffer {
                content,
                use_: 0,
                size: 64,
                alloc: 0,
                contentIO: ptr::null_mut(),
            };

            append_to_xml_buffer(&mut buf, b"Hello");
            assert_eq!(buf.use_, 5);
            let slice = core::slice::from_raw_parts(buf.content, 5);
            assert_eq!(slice, b"Hello");

            append_to_xml_buffer(&mut buf, b" World");
            assert_eq!(buf.use_, 11);
            let slice = core::slice::from_raw_parts(buf.content, 11);
            assert_eq!(slice, b"Hello World");

            xmlFree(buf.content as *mut c_void);
        }
    }

    // ── ABI export functions ───────────────────────────────────────────────

    #[test]
    fn test_xml_parse_char_encoding() {
        let name = b"UTF-8\0".as_ptr() as *const c_char;
        assert_eq!(
            xmlParseCharEncoding(name),
            xmlCharEncoding::XML_CHAR_ENCODING_UTF8 as c_int
        );

        let name = b"ISO-8859-1\0".as_ptr() as *const c_char;
        assert_eq!(
            xmlParseCharEncoding(name),
            xmlCharEncoding::XML_CHAR_ENCODING_8859_1 as c_int
        );

        assert_eq!(
            xmlParseCharEncoding(ptr::null()),
            xmlCharEncoding::XML_CHAR_ENCODING_NONE as c_int
        );
    }

    #[test]
    fn test_xml_new_and_del_encoding_handler() {
        let name = b"TestEnc\0".as_ptr() as *const c_char;
        let handler = xmlNewCharEncodingHandler(
            name,
            utf8_input_func as xmlCharEncodingInputFunc,
            utf8_output_func as xmlCharEncodingOutputFunc,
        );
        assert!(!handler.is_null());

        unsafe {
            assert!(!(*handler).name.is_null());
            let cstr = CStr::from_ptr((*handler).name);
            assert_eq!(cstr.to_bytes(), b"TestEnc");
        }

        xmlDelEncodingHandler(handler);
    }

    #[test]
    fn test_xml_init_and_cleanup() {
        xmlInitCharEncodingHandlers();

        let name: *const xmlChar = b"UTF-8\0".as_ptr();
        assert!(!find_encoding_handler(name).is_null());

        xmlCleanupCharEncodingHandlers();
        // After cleanup, handlers should be empty
    }
}
