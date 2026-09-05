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
//!
//! # Upstream contract
//!
//! Mirrors upstream `encoding.c` / `encoding.h`
//! (`SRC-LIBXML2-2.15.0-ENCODING-C`, parity target libxml2 2.15.3 oracle).
//! ABI surface: `xmlLookupCharEncodingHandler`, `xmlGetCharEncodingHandler`,
//! `xmlOpenCharEncodingHandler`, `xmlCreateCharEncodingHandler`,
//! `xmlCharEncInput`/`xmlCharEncOutput` and the `_xmlCharEncodingHandler`
//! C-layout struct (R-000129 fixed the Rust mirror from 48 to the upstream
//! 56 bytes).
//!
//! # Conceptual behavior
//!
//! Detection runs BOM first, then the XML declaration, then registry lookup.
//! The registry mirrors upstream `defaultHandlers[32]` plus the extra-handler
//! table (`globalHandlers`, encoding.c): named handlers are registered under
//! their canonical lowercased alias and found by `xmlFindCharEncodingHandler`
//! via `find_encoding_handler`.
//!
//! # Ownership & safety invariants
//!
//! Handlers are allocated with xmlMalloc and owned by the registry; `xmlFree` releases
//! them at teardown. Registry access is serialized by an RwLock; that
//! serialization is exactly what makes the raw `HandlerPtr` Send+Sync
//! SAFETY sound (documented on the wrapper). Names from
//! `xmlGetCharEncodingName`/alias tables are borrowed statics — the caller
//! never frees them.
//!
//! # Historical quirks & epochs
//!
//! R-000157 (OPEN, UNRESOLVED): the crate ships no
//! iconv/ICU backend, so the iconv/ICU-only encodings (UCS-4LE/BE, EBCDIC,
//! UCS-2, ISO-8859-2..16, ISO-2022-JP, Shift_JIS, EUC-JP, windows-1252)
//! report XML_ERR_UNSUPPORTED_ENCODING (32) where the 2.15.3 oracle (built
//! with Iconv+ICU enabled) returns
//! a converter, while the native set (UTF-8, UTF-16LE/BE, UTF-16,
//! ISO-8859-1, US-ASCII) and all error paths are byte-identical. This is a
//! REAL current executed-platform difference, so the residual is UNRESOLVED
//! (11.1-Z.1) — closure requires implementing an iconv/ICU backend, a future
//! implementation work item, not a waiver. Upstream
//! itself removed the libiconv dependence where possible in the 2.10+ era
//! (HISTORY.md §1.8), which is the epoch this module targets.
//!
//! # Deliberate oddities
//!
//! The bounded native set is a deliberate divergence, not a stub: the
//! missing encodings are absent because no converter exists, and every
//! error path matches the oracle. `xmlLookupCharEncodingHandler` returns
//! XML_ERR_OK with a NULL handler for UTF-8/NONE exactly like upstream
//! encoding.c (`/* Return NULL handler for UTF-8 */`). R-000157 is tracked
//! UNRESOLVED: adding an iconv/ICU backend would close the gap for the
//! encodings the executed oracle serves.
//!
//! # Proving courts
//!
//! ENCODING-001 (`courts/suites/data-abi/encoding-family-probe.c`) compiles
//! one C probe against the oracle DSO and the candidate and requires
//! byte-identical stdout across the native set and all error paths.
//!
//! # Tempting simplifications that would break parity
//!
//! Do not collapse the registry to a fixed match statement: custom handlers
//! added through `xmlAddCharEncodingHandler` must stay discoverable by later
//! lookups. Do not fabricate handlers for the iconv-only encodings — that
//! would fake a converter that does not exist and break the R-000157
//! UNRESOLVED record (the honest path is a real iconv/ICU backend). Do not
//! touch the struct layout: R-000129
//! proved a 48-byte mirror breaks the C ABI.

#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_uchar, c_uint, c_void};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};

use once_cell::sync::Lazy;
use parking_lot::RwLock;

use crate::abi::allocator::{xmlFreeImpl, xmlMallocImpl, xmlReallocImpl};
use crate::abi::callbacks::{
    xmlCharEncConvCtxtDtor, xmlCharEncConvFunc, xmlCharEncConvImpl, xmlCharEncodingInputFunc,
    xmlCharEncodingOutputFunc,
};
use crate::abi::structs::{
    _xmlBuffer, _xmlCharEncodingHandler, EncodingInputUnion, EncodingOutputUnion,
};
use crate::abi::types::{xmlChar, xmlCharEncoding};

// ── Constants ──────────────────────────────────────────────────────────────

/// Maximum bytes needed per character for any supported encoding.
#[allow(dead_code)]
const MAX_CHAR_BYTES: usize = 6;

/// UTF-8 BOM bytes.
#[allow(dead_code)]
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
///
/// UPSTREAM-PARITY: this is the Rust mirror of encoding.c `globalHandlers`
/// (the table behind `xmlFindExtraHandler`); `xmlAddCharEncodingHandler`
/// appends here and lookups scan it after the built-in `defaultHandlers[32]`
/// set (R-000157: only the native subset is backed by real converters).
static ENCODING_HANDLERS: Lazy<RwLock<Vec<HandlerPtr>>> = Lazy::new(|| RwLock::new(Vec::new()));

/// Whether the built-in encoding handlers have been initialized.
static ENCODING_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Serializes first-time handler registration (see init_encodings).
static ENCODING_INIT_MUTEX: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

// ═══════════════════════════════════════════════════════════════════════════════
// 1. Encoding detection
// ═══════════════════════════════════════════════════════════════════════════════

/// Determine encoding from BOM bytes.
///
/// Returns `XML_CHAR_ENCODING_NONE` if no BOM is present, or if `data` is empty.
/// Otherwise returns the matching encoding enum value.
#[allow(dead_code)]
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
#[allow(dead_code)]
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
pub(crate) const fn encoding_name(enc: xmlCharEncoding) -> Option<&'static [u8]> {
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
        xmlCharEncoding::XML_CHAR_ENCODING_ASCII => Some(b"US-ASCII" as &[u8]),
        _ => None,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 2. UTF-8 validation
// ═══════════════════════════════════════════════════════════════════════════════

/// Check if a byte sequence is valid UTF-8.
///
/// Returns `true` if the entire slice is valid UTF-8, `false` otherwise.
#[allow(dead_code)]
pub(crate) const fn utf8_valid(data: &[u8]) -> bool {
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
#[allow(dead_code)]
pub(crate) const fn is_valid_xml_char(cp: u32) -> bool {
    matches!(
        cp,
        0x9 | 0xA | 0xD | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x10000..=0x10FFFF
    )
}

// ═══════════════════════════════════════════════════════════════════════════════
// 3. UTF-16 handling
// ═══════════════════════════════════════════════════════════════════════════════

/// Decode a single UTF-16LE code unit from two bytes.
#[inline]
const fn read_utf16le_unit(data: &[u8]) -> Option<u16> {
    if data.len() < 2 {
        return None;
    }
    Some(u16::from_le_bytes([data[0], data[1]]))
}

/// Decode a single UTF-16BE code unit from two bytes.
#[inline]
const fn read_utf16be_unit(data: &[u8]) -> Option<u16> {
    if data.len() < 2 {
        return None;
    }
    Some(u16::from_be_bytes([data[0], data[1]]))
}

/// Encode a Unicode codepoint as UTF-8 bytes.
///
/// Returns the number of bytes written (1–4), or 0 if the codepoint is invalid.
const fn encode_codepoint_to_utf8(cp: u32, out: &mut [u8]) -> usize {
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

        if (0xD800..=0xDBFF).contains(&unit) {
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
        } else if (0xDC00..=0xDFFF).contains(&unit) {
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

        if (0xD800..=0xDBFF).contains(&unit) {
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
        } else if (0xDC00..=0xDFFF).contains(&unit) {
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
#[allow(dead_code)]
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
    if ENCODING_INITIALIZED.load(Ordering::SeqCst) {
        return;
    }
    // Serialize first-time registration: without the mutex, a second thread
    // can observe ENCODING_INITIALIZED == true and look up handlers while
    // the first thread is still registering them (race found by the parallel
    // test suite: xml::io test_output_buffer_with_encoding intermittently
    // failed to find the Latin-1 handler).
    let _guard = ENCODING_INIT_MUTEX.lock();
    if ENCODING_INITIALIZED.load(Ordering::SeqCst) {
        return;
    }
    register_builtin_handlers();
    ENCODING_INITIALIZED.store(true, Ordering::SeqCst);
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

    // Windows-1252 (CP1252): served by iconv on the oracle; native converter
    // here (R-000157 partial closure). Registered under both the canonical
    // spelling and the cp1252 alias; lookup is case-insensitive so
    // "Windows-1252" (the Dom\XMLDocument overrideEncoding the PHP court
    // passes verbatim) resolves to the same entry.
    register_handler(
        b"windows-1252\0",
        xmlCharEncoding::XML_CHAR_ENCODING_ERROR,
        xmlCharEncoding::XML_CHAR_ENCODING_ERROR,
        Some(cp1252_input_func as xmlCharEncodingInputFunc),
        Some(cp1252_output_func as xmlCharEncodingOutputFunc),
    );
    register_handler(
        b"cp1252\0",
        xmlCharEncoding::XML_CHAR_ENCODING_ERROR,
        xmlCharEncoding::XML_CHAR_ENCODING_ERROR,
        Some(cp1252_input_func as xmlCharEncodingInputFunc),
        Some(cp1252_output_func as xmlCharEncodingOutputFunc),
    );

    // ASCII — upstream's static default handler (defaultHandlers[22]) is named
    // "US-ASCII"; the name "ASCII" is registered as a second entry so name-based
    // lookups (xmlFindCharEncodingHandler, the saver path) accept both spellings
    // exactly like upstream's xmlParseCharEncodingInternal mapping.
    register_handler(
        b"US-ASCII\0",
        xmlCharEncoding::XML_CHAR_ENCODING_ASCII,
        xmlCharEncoding::XML_CHAR_ENCODING_ASCII,
        Some(ascii_input_func as xmlCharEncodingInputFunc),
        Some(ascii_output_func as xmlCharEncodingOutputFunc),
    );
    register_handler(
        b"ASCII\0",
        xmlCharEncoding::XML_CHAR_ENCODING_ASCII,
        xmlCharEncoding::XML_CHAR_ENCODING_ASCII,
        Some(ascii_input_func as xmlCharEncodingInputFunc),
        Some(ascii_output_func as xmlCharEncodingOutputFunc),
    );

    // UTF-16 (default handler for enc == XML_CHAR_ENCODING_UTF16 == 23): the
    // upstream converter is UTF16LEToUTF8/UTF8ToUTF16 (the latter emits the LE
    // BOM on its init call). Our converter pair is the UTF-16LE pair; the BOM
    // init protocol is not emitted (documented divergence, conversion only).
    register_handler(
        b"UTF-16\0",
        xmlCharEncoding::XML_CHAR_ENCODING_UTF16LE,
        xmlCharEncoding::XML_CHAR_ENCODING_UTF16LE,
        Some(utf16le_input_func as xmlCharEncodingInputFunc),
        Some(utf16le_output_func as xmlCharEncodingOutputFunc),
    );

    // Shift_JIS + EUC-JP — encoding_rs-backed converters (R-000157 closure
    // slice, Phase 14.27). Upstream serves these through iconv on the
    // executed oracle (2.15.3, Iconv+ICU); the crate ships no iconv/ICU
    // backend, so the converters are implemented natively over WHATWG
    // Shift_JIS (a CP932-compatible superset) / EUC-JP, which byte-match
    // glibc iconv on the shared JIS X 0208 repertoire the php suite and
    // byte-parity probes exercise (the WHATWG/CP932 extension differences
    // are residualized in R-000157). Registered under the canonical
    // spellings + the aliases upstream's name path accepts (registry lookup
    // is case-insensitive).
    register_handler(
        b"SHIFT_JIS\0",
        xmlCharEncoding::XML_CHAR_ENCODING_SHIFT_JIS,
        xmlCharEncoding::XML_CHAR_ENCODING_SHIFT_JIS,
        Some(shift_jis_input_func as xmlCharEncodingInputFunc),
        Some(shift_jis_output_func as xmlCharEncodingOutputFunc),
    );
    register_handler(
        b"SJIS\0",
        xmlCharEncoding::XML_CHAR_ENCODING_SHIFT_JIS,
        xmlCharEncoding::XML_CHAR_ENCODING_SHIFT_JIS,
        Some(shift_jis_input_func as xmlCharEncodingInputFunc),
        Some(shift_jis_output_func as xmlCharEncodingOutputFunc),
    );
    register_handler(
        b"CP932\0",
        xmlCharEncoding::XML_CHAR_ENCODING_SHIFT_JIS,
        xmlCharEncoding::XML_CHAR_ENCODING_SHIFT_JIS,
        Some(shift_jis_input_func as xmlCharEncodingInputFunc),
        Some(shift_jis_output_func as xmlCharEncodingOutputFunc),
    );
    register_handler(
        b"EUC-JP\0",
        xmlCharEncoding::XML_CHAR_ENCODING_EUC_JP,
        xmlCharEncoding::XML_CHAR_ENCODING_EUC_JP,
        Some(euc_jp_input_func as xmlCharEncodingInputFunc),
        Some(euc_jp_output_func as xmlCharEncodingOutputFunc),
    );
    register_handler(
        b"EUCJP\0",
        xmlCharEncoding::XML_CHAR_ENCODING_EUC_JP,
        xmlCharEncoding::XML_CHAR_ENCODING_EUC_JP,
        Some(euc_jp_input_func as xmlCharEncodingInputFunc),
        Some(euc_jp_output_func as xmlCharEncodingOutputFunc),
    );

    // ISO-8859-2..16 (R-000157 remainder, Phase 14.29): encoding_rs-backed
    // single-byte converters (upstream serves these via iconv on the
    // executed oracle). ISO-8859-11 == TIS-620 == the WHATWG windows-874
    // single-byte set on the shared repertoire; both spellings are
    // registered. The canonical names are the registered keys; lookups are
    // case-insensitive.
    macro_rules! register_iso8859 {
        ($name:literal, $enc:expr, $input:ident, $output:ident) => {
            register_handler(
                concat!($name, "\0").as_bytes(),
                xmlCharEncoding::XML_CHAR_ENCODING_ERROR,
                xmlCharEncoding::XML_CHAR_ENCODING_ERROR,
                Some($input as xmlCharEncodingInputFunc),
                Some($output as xmlCharEncodingOutputFunc),
            );
            let _ = $enc; // (encoding identity carried by the func pair)
        };
    }
    register_iso8859!(
        "ISO-8859-2",
        encoding_rs::ISO_8859_2,
        iso_8859_2_input_func,
        iso_8859_2_output_func
    );
    register_iso8859!(
        "ISO-8859-3",
        encoding_rs::ISO_8859_3,
        iso_8859_3_input_func,
        iso_8859_3_output_func
    );
    register_iso8859!(
        "ISO-8859-4",
        encoding_rs::ISO_8859_4,
        iso_8859_4_input_func,
        iso_8859_4_output_func
    );
    register_iso8859!(
        "ISO-8859-5",
        encoding_rs::ISO_8859_5,
        iso_8859_5_input_func,
        iso_8859_5_output_func
    );
    register_iso8859!(
        "ISO-8859-6",
        encoding_rs::ISO_8859_6,
        iso_8859_6_input_func,
        iso_8859_6_output_func
    );
    register_iso8859!(
        "ISO-8859-7",
        encoding_rs::ISO_8859_7,
        iso_8859_7_input_func,
        iso_8859_7_output_func
    );
    register_iso8859!(
        "ISO-8859-8",
        encoding_rs::ISO_8859_8,
        iso_8859_8_input_func,
        iso_8859_8_output_func
    );
    register_iso8859!(
        "ISO-8859-9",
        encoding_rs::WINDOWS_1254,
        iso_8859_9_input_func,
        iso_8859_9_output_func
    );
    register_iso8859!(
        "ISO-8859-10",
        encoding_rs::ISO_8859_10,
        iso_8859_10_input_func,
        iso_8859_10_output_func
    );
    register_iso8859!(
        "ISO-8859-11",
        encoding_rs::WINDOWS_874,
        iso_8859_11_input_func,
        iso_8859_11_output_func
    );
    register_iso8859!(
        "windows-874",
        encoding_rs::WINDOWS_874,
        iso_8859_11_input_func,
        iso_8859_11_output_func
    );
    register_iso8859!(
        "ISO-8859-13",
        encoding_rs::ISO_8859_13,
        iso_8859_13_input_func,
        iso_8859_13_output_func
    );
    register_iso8859!(
        "ISO-8859-14",
        encoding_rs::ISO_8859_14,
        iso_8859_14_input_func,
        iso_8859_14_output_func
    );
    register_iso8859!(
        "ISO-8859-15",
        encoding_rs::ISO_8859_15,
        iso_8859_15_input_func,
        iso_8859_15_output_func
    );
    register_iso8859!(
        "ISO-8859-16",
        encoding_rs::ISO_8859_16,
        iso_8859_16_input_func,
        iso_8859_16_output_func
    );

    // ISO-2022-JP (stateful escape-sequence encoding; encoding_rs keeps the
    // JIS X 0208 / ASCII escape state inside each conversion call).
    register_handler(
        b"ISO-2022-JP\0",
        xmlCharEncoding::XML_CHAR_ENCODING_2022_JP,
        xmlCharEncoding::XML_CHAR_ENCODING_2022_JP,
        Some(iso_2022_jp_input_func as xmlCharEncodingInputFunc),
        Some(iso_2022_jp_output_func as xmlCharEncodingOutputFunc),
    );

    // UCS-2 (2-byte big-endian units; the glibc iconv "UCS-2" the oracle
    // serves) and UCS-4LE/BE (4-byte units). Native fixed-width codecs.
    register_handler(
        b"UCS-2\0",
        xmlCharEncoding::XML_CHAR_ENCODING_UCS2,
        xmlCharEncoding::XML_CHAR_ENCODING_UCS2,
        Some(ucs2_input_func as xmlCharEncodingInputFunc),
        Some(ucs2_output_func as xmlCharEncodingOutputFunc),
    );
    register_handler(
        b"UCS-4LE\0",
        xmlCharEncoding::XML_CHAR_ENCODING_UCS4LE,
        xmlCharEncoding::XML_CHAR_ENCODING_UCS4LE,
        Some(ucs4le_input_func as xmlCharEncodingInputFunc),
        Some(ucs4le_output_func as xmlCharEncodingOutputFunc),
    );
    register_handler(
        b"UCS-4BE\0",
        xmlCharEncoding::XML_CHAR_ENCODING_UCS4BE,
        xmlCharEncoding::XML_CHAR_ENCODING_UCS4BE,
        Some(ucs4be_input_func as xmlCharEncodingInputFunc),
        Some(ucs4be_output_func as xmlCharEncodingOutputFunc),
    );
    register_handler(
        b"UCS-4\0",
        xmlCharEncoding::XML_CHAR_ENCODING_UCS4LE,
        xmlCharEncoding::XML_CHAR_ENCODING_UCS4LE,
        Some(ucs4le_input_func as xmlCharEncodingInputFunc),
        Some(ucs4le_output_func as xmlCharEncodingOutputFunc),
    );

    // EBCDIC code page 037 (the glibc iconv "IBM037"/"EBCDIC-US" the
    // oracle serves). Native 037 table.
    register_handler(
        b"IBM037\0",
        xmlCharEncoding::XML_CHAR_ENCODING_EBCDIC,
        xmlCharEncoding::XML_CHAR_ENCODING_EBCDIC,
        Some(ebcdic_input_func as xmlCharEncodingInputFunc),
        Some(ebcdic_output_func as xmlCharEncodingOutputFunc),
    );
    register_handler(
        b"EBCDIC-US\0",
        xmlCharEncoding::XML_CHAR_ENCODING_EBCDIC,
        xmlCharEncoding::XML_CHAR_ENCODING_EBCDIC,
        Some(ebcdic_input_func as xmlCharEncodingInputFunc),
        Some(ebcdic_output_func as xmlCharEncodingOutputFunc),
    );
    register_handler(
        b"EBCDIC\0",
        xmlCharEncoding::XML_CHAR_ENCODING_EBCDIC,
        xmlCharEncoding::XML_CHAR_ENCODING_EBCDIC,
        Some(ebcdic_input_func as xmlCharEncodingInputFunc),
        Some(ebcdic_output_func as xmlCharEncodingOutputFunc),
    );
}

/// Helper to create and register an encoding handler.
///
/// # Safety
///
/// - `name_bytes` must be a valid byte slice containing a NUL terminator;
///   `xmlMemStrdupImpl` scans it as a C string.
/// - The `xmlMallocImpl` result is NULL-checked before `ptr::write`
///   initializes the handler; the written handler is inserted into the
///   global registry, which keeps it alive for the process lifetime.
fn register_handler(
    name_bytes: &[u8],
    _input_enc: xmlCharEncoding,
    _output_enc: xmlCharEncoding,
    input_func: Option<xmlCharEncodingInputFunc>,
    output_func: Option<xmlCharEncodingOutputFunc>,
) {
    let name_raw =
        unsafe { crate::abi::allocator::xmlMemStrdupImpl(name_bytes.as_ptr() as *const c_char) };
    if name_raw.is_null() {
        return;
    }

    let handler = unsafe { xmlMallocImpl(size_of::<_xmlCharEncodingHandler>()) }
        as *mut _xmlCharEncodingHandler;

    if handler.is_null() {
        unsafe { xmlFreeImpl(name_raw) };
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
///
/// # Safety
///
/// - Every registered handler pointer must be NULL or a valid
///   heap-allocated `_xmlCharEncodingHandler` whose `name` is NULL or a
///   heap-allocated NUL-terminated string; each allocation is freed exactly
///   once and must not be freed elsewhere.
pub(crate) fn cleanup_encodings() {
    let mut handlers = ENCODING_HANDLERS.write();
    for &handler in handlers.iter() {
        let ptr = handler.0;
        if !ptr.is_null() {
            unsafe {
                if !(*ptr).name.is_null() {
                    xmlFreeImpl((*ptr).name as *mut c_void);
                }
                xmlFreeImpl(ptr as *mut c_void);
            }
        }
    }
    handlers.clear();
    // Re-allow registration on the next init_encodings()/cleanup round-trip so
    // a caller that cleans up and then (re)initializes in another thread does
    // not observe a stale "already initialized" registry that stays empty.
    // (ENCODING_INITIALIZED/ENCODING_INIT_MUTEX are separate statics.)
    drop(handlers);
    ENCODING_INITIALIZED.store(false, Ordering::SeqCst);
}

/// Find an encoding handler by name.
///
/// Searches the global handler registry for a handler whose name matches
/// (case-insensitive). Returns a pointer to the handler, or `ptr::null_mut()`
/// if not found.
///
/// # Safety
///
/// - `name` must be NULL or a valid pointer to a NUL-terminated string.
/// - Each registry entry must be NULL or a valid `_xmlCharEncodingHandler`
///   whose `name` is NULL or a valid NUL-terminated string.
pub(crate) fn find_encoding_handler(name: *const xmlChar) -> *mut _xmlCharEncodingHandler {
    if name.is_null() {
        return ptr::null_mut();
    }

    /* The upstream default-handler table is static and always present; the
     * candidate's registry is populated lazily, so ensure it is initialized
     * before any name-based lookup. Idempotent. */
    init_encodings();

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

/// Build an owned, caller-freed copy of a registered encoding handler.
///
/// Upstream's `xmlFindCharEncodingHandler` hands the caller a handler it owns
/// and is expected to release with `xmlCharEncCloseFunc` after use (Phase 14
/// PHP court: `dom_document_encoding_write` finds a handler then closes it for
/// every write). Returning the persistent registry pointer directly would let
/// the exported close free the registry entry out from under later lookups
/// (a use-after-free seen as `DOMDocument::$encoding = 'UTF-16'` corrupting the
/// handler registry and crashing the next `find_encoding_handler`).
///
/// The copy duplicates the name with `xmlMemStrdupImpl` so the caller may free
/// it; the conversion unions and context pointers are shared with the original
/// registry entry. All built-in registry handlers the find path serves are
/// stateless (`ctxtDtor` is None, contexts NULL), so `xmlCharEncCloseFunc` on
/// the copy only releases the duplicated name and the struct.
///
/// Returns the new handler or `ptr::null_mut()` when `src` is NULL/alloc fails.
pub(crate) fn clone_encoding_handler_for_find(
    src: *mut _xmlCharEncodingHandler,
) -> *mut _xmlCharEncodingHandler {
    if src.is_null() {
        return ptr::null_mut();
    }
    let name_raw = unsafe {
        let nm = (*src).name;
        if nm.is_null() {
            ptr::null_mut()
        } else {
            crate::abi::allocator::xmlMemStrdupImpl(nm)
        }
    };
    let handler = unsafe { xmlMallocImpl(size_of::<_xmlCharEncodingHandler>()) }
        as *mut _xmlCharEncodingHandler;
    if handler.is_null() {
        if !name_raw.is_null() {
            unsafe { crate::abi::allocator::xmlFreeImpl(name_raw) };
        }
        return ptr::null_mut();
    }
    unsafe {
        ptr::write(
            handler,
            _xmlCharEncodingHandler {
                name: name_raw as *mut c_char,
                input: ptr::read(&(*src).input),
                output: ptr::read(&(*src).output),
                inputCtxt: (*src).inputCtxt,
                outputCtxt: (*src).outputCtxt,
                ctxtDtor: (*src).ctxtDtor,
                flags: (*src).flags,
            },
        );
    }
    handler
}

/// Upstream handler `flags` marker: the handler lives for the process lifetime
/// (upstream `encoding.c` `{"UTF-8", ... , XML_HANDLER_STATIC}`) and
/// `xmlCharEncCloseFunc` must therefore not release it.
pub(crate) const XML_HANDLER_STATIC: c_int = 0x01;

/// ABI `xmlFindCharEncodingHandler` mirror (upstream libxml2 2.15 encoding.c
/// `xmlFindCharEncodingHandler`).
///
/// Upstream returns an OWNED handler the caller releases with
/// `xmlCharEncCloseFunc`, except for UTF-8/UTF8 where it returns the static
/// `defaultHandlers[XML_CHAR_ENCODING_UTF8]` (has `XML_HANDLER_STATIC`, so
/// `xmlCharEncCloseFunc` is a no-op). Phase 14 PHP court:
/// `dom_document_encoding_write` finds a handler for every `$dom->encoding=
/// write and closes it — so returning the persistent registry pointer for a
/// non-UTF-8 encoding let the caller's close free the registry entry (the
/// use-after-free behind `DOMDocument::$encoding = 'UTF-16'` crashing the next
/// `find_encoding_handler`).
///
/// Returns an owned heap copy for non-UTF-8 encodings, the flagged-static
/// registry UTF-8 handler for UTF-8, or `ptr::null_mut()` when `name` is NULL
/// or no handler is registered.
pub(crate) fn xmlFindCharEncodingHandler_owned(
    name: *const xmlChar,
) -> *mut _xmlCharEncodingHandler {
    if name.is_null() {
        return ptr::null_mut();
    }
    let name_bytes = unsafe {
        let len = libc::strlen(name as *const c_char);
        core::slice::from_raw_parts(name as *const u8, len)
    };

    // UTF-8 / UTF8 special case (upstream returns the static handler).
    if encoding_from_name(name_bytes) == xmlCharEncoding::XML_CHAR_ENCODING_UTF8 {
        let utf8 = find_encoding_handler(c"UTF-8".as_ptr() as *const xmlChar);
        if utf8.is_null() {
            return ptr::null_mut();
        }
        // Flag it static so xmlCharEncCloseFunc does not free the registry entry.
        unsafe {
            (*utf8).flags |= XML_HANDLER_STATIC;
        }
        return utf8;
    }

    // Non-UTF-8: resolve the registry entry, preferring a canonical lookup when
    // the raw spelling is not itself a registered key (mirrors the canonical
    // re-lookup upstream performs in xmlCreateCharEncodingHandler).
    let mut entry = find_encoding_handler(name as *const xmlChar);
    if entry.is_null() {
        if let Some(canon) = encoding_name(encoding_from_name(name_bytes)) {
            entry = find_encoding_handler(canon.as_ptr() as *const xmlChar);
        }
    }
    // Return an OWNED copy of the registry entry (never the entry itself), so
    // the caller's xmlCharEncCloseFunc releases only the copy.
    clone_encoding_handler_for_find(entry)
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
///
/// # Safety
///
/// - `handler` must be NULL or a valid pointer to an initialized
///   `_xmlCharEncodingHandler`; the stored `input.legacyFunc` callback, when
///   present, must be a valid function pointer.
/// - `out` must be a valid mutable byte slice and `in_data` a valid byte
///   slice; both stay valid for the duration of the callback.
#[allow(dead_code)]
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
///
/// # Safety
///
/// - `handler` must be NULL or a valid pointer to an initialized
///   `_xmlCharEncodingHandler`; the stored `output.legacyFunc` callback,
///   when present, must be a valid function pointer.
/// - `out` must be a valid mutable byte slice and `in_data` a valid byte
///   slice; both stay valid for the duration of the callback.
#[allow(dead_code)]
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
///
/// # Safety
///
/// - `handler` must be NULL or a valid `_xmlCharEncodingHandler` whose
///   `input.legacyFunc` callback is a valid function pointer.
/// - `in_` and `out` must be NULL or valid `_xmlBuffer` pointers; `in_`'s
///   `content` must be NULL or point to `use_` readable bytes, and `out`
///   must stay valid while `append_to_xml_buffer` may reallocate its
///   `content`.
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
///
/// # Safety
///
/// - `handler` must be NULL or a valid `_xmlCharEncodingHandler` whose
///   `output.legacyFunc` callback is a valid function pointer.
/// - `in_` and `out` must be NULL or valid `_xmlBuffer` pointers; `in_`'s
///   `content` must be NULL or point to `use_` readable bytes, and `out`
///   must stay valid while `append_to_xml_buffer` may reallocate its
///   `content`.
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

    let mut in_data = unsafe { core::slice::from_raw_parts(in_buf.content, in_buf.use_ as usize) };

    // UPSTREAM-PARITY (encoding.c xmlCharEncOutput): the output conversion
    // runs in a loop; when the converter reports an INPUT error (a character
    // not representable in the output encoding — the ASCII handler stops at
    // the first byte >= 0x80), the offending UTF-8 character is decoded and
    // replaced by a DECIMAL character reference (&#NNN;), then conversion
    // continues. This is how libxml2 serializes non-ASCII text into an
    // ASCII output buffer (lxml's default `tostring` encoding, which
    // produces `&#195;&#169;` for the mojibake case).
    const ENC_INPUT_ERROR: c_int = -2;
    let mut total_written: usize = 0;
    loop {
        // Scratch is >= 5x the input so even the widest native codec
        // (UCS-4: 4 bytes per ASCII input byte) can never exhaust it
        // mid-buffer; unmappable characters stop with ENC_INPUT_ERROR and
        // are substituted here without expansion through the func.
        let out_capacity = (in_data.len().saturating_mul(5)).max(64) + 16;
        let mut out_vec = vec![0u8; out_capacity];
        let mut out_len = out_capacity as c_int;
        let mut in_len = in_data.len() as c_int;
        let ret = unsafe {
            output_func(
                out_vec.as_mut_ptr(),
                &mut out_len,
                in_data.as_ptr(),
                &mut in_len,
            )
        };
        let written = out_len.max(0) as usize;
        if written > 0 {
            append_to_xml_buffer(out_buf, &out_vec[..written]);
            total_written += written;
        }
        let consumed = in_len.max(0) as usize;
        if ret == ENC_INPUT_ERROR && consumed < in_data.len() {
            // Decode the UTF-8 character at the offending position and emit
            // a decimal character reference (upstream xmlSerializeDecCharRef).
            let mut clen: c_int = 4;
            let cp = unsafe {
                crate::abi::exports_misc::xmlGetUTF8Char(in_data[consumed..].as_ptr(), &mut clen)
            };
            if cp <= 0 || clen <= 0 || (consumed + clen as usize) > in_data.len() {
                return -1;
            }
            let ref_str = format!("&#{};", cp);
            append_to_xml_buffer(out_buf, ref_str.as_bytes());
            total_written += ref_str.len();
            in_data = &in_data[consumed + clen as usize..];
            if in_data.is_empty() {
                break;
            }
            continue;
        }
        if ret < 0 {
            return -1;
        }
        break;
    }

    total_written as c_int
}

// ═══════════════════════════════════════════════════════════════════════════════
// 6b. Whole-buffer declared-encoding decode (parser input layer; R-000157)
// ═══════════════════════════════════════════════════════════════════════════════

/// Decode a whole raw byte buffer to UTF-8 through the registry handler for
/// a declared encoding NAME, canonicalizing alias spellings exactly like
/// `xmlFindCharEncodingHandler_owned`. Used by the parser input layer for
/// BOM-less inputs whose XML declaration names a legacy encoding (and for
/// the pattern-detected UCS-4/EBCDIC family, whose canonical names are
/// passed directly). Returns `Err(())` when the name has no handler or the
/// bytes are not decodable (iconv EILSEQ semantics — the caller falls back
/// to the tokenizer's invalid-character diagnostics).
pub(crate) fn decode_whole_buffer_declared(name: &[u8], data: &[u8]) -> Result<Vec<u8>, ()> {
    if data.is_empty() {
        return Ok(Vec::new());
    }
    let Ok(cname) = std::ffi::CString::new(name) else {
        return Err(());
    };
    let mut handler = find_encoding_handler(cname.as_ptr() as *const xmlChar);
    if handler.is_null() {
        // Canonical re-lookup for alias spellings (upstream
        // xmlFindCharEncodingHandler: latin2 -> ISO-8859-2, sjis ->
        // SHIFT_JIS, ...).
        if let Some(canon) = encoding_name(encoding_from_name(name)) {
            if let Ok(canon_c) = std::ffi::CString::new(canon) {
                handler = find_encoding_handler(canon_c.as_ptr() as *const xmlChar);
            }
        }
    }
    if handler.is_null() {
        return Err(());
    }
    decode_bytes_with_handler(handler, data)
}

/// Drive a registry handler's `input.legacyFunc` over a whole byte buffer,
/// growing the output as needed. Each input func converts complete source
/// characters; source encodings expand at most ~3x into UTF-8, so the
/// initial 3x+16 scratch completes valid input in one call and the growth
/// branch only guards pathological (near-invalid) content.
pub(crate) fn decode_bytes_with_handler(
    handler: *mut _xmlCharEncodingHandler,
    data: &[u8],
) -> Result<Vec<u8>, ()> {
    if handler.is_null() || data.is_empty() {
        return Ok(Vec::new());
    }
    let input_func = unsafe { (*handler).input.legacyFunc };
    let Some(input_func) = input_func else {
        return Err(());
    };
    let mut out: Vec<u8> = vec![0u8; data.len().saturating_mul(3) + 16];
    let mut in_pos: usize = 0;
    let mut written_total: usize = 0;
    loop {
        let mut out_len = (out.len() - written_total) as c_int;
        let mut in_len = (data.len() - in_pos) as c_int;
        // SAFETY: `out[written_total..]` and `data[in_pos..]` are valid
        // writable/readable slices for the call; the func respects the
        // length pointers (house func contract).
        let ret = unsafe {
            input_func(
                out[written_total..].as_mut_ptr(),
                &mut out_len,
                data[in_pos..].as_ptr(),
                &mut in_len,
            )
        };
        let written = out_len.max(0) as usize;
        let consumed = in_len.max(0) as usize;
        written_total += written;
        in_pos += consumed;
        if ret < 0 {
            return Err(());
        }
        if in_pos >= data.len() {
            break;
        }
        if written == 0 {
            // No progress with input left: undecodable tail.
            return Err(());
        }
        out.resize(out.len().saturating_mul(2).max(written_total + 64), 0);
    }
    out.truncate(written_total);
    Ok(out)
}

/// Append bytes to an `_xmlBuffer`, reallocating if needed.
///
/// - `buf` must be a valid `_xmlBuffer` whose `content` is NULL or points to
///   `size` allocated bytes; `buf.content` may be replaced by a fresh
///   `xmlReallocImpl` allocation when it must grow.
/// - `data` must be a valid byte slice; after the call, `buf.content` holds
///   `use_` initialized bytes.
fn append_to_xml_buffer(buf: &mut _xmlBuffer, data: &[u8]) {
    if data.is_empty() {
        return;
    }

    let new_use = (buf.use_ as usize).saturating_add(data.len());
    if new_use > buf.size as usize {
        // Grow buffer: double or fit, whichever is larger
        let new_size = (buf.size as usize).saturating_mul(2).max(new_use).max(256);
        let new_content =
            unsafe { xmlReallocImpl(buf.content as *mut c_void, new_size) as *mut xmlChar };
        if new_content.is_null() {
            return; // Allocation failure — silently skip
        }
        buf.content = new_content;
        // UPSTREAM-PARITY (io/mod.rs buf_add realloc paths): when the buffer
        // grows, contentIO tracks the CURRENT allocation base — buf_free
        // frees contentIO, so a stale contentIO (the pre-realloc block) would
        // cause a double-free on buffers that grew through this conversion
        // path (nokogiri HTML4/HTML5 UTF-8 serialization).
        buf.contentIO = new_content;
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
    let _out_slice = core::slice::from_raw_parts_mut(out, avail_out);

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
    let _out_slice = core::slice::from_raw_parts_mut(out, avail_out);

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
    let _out_slice = core::slice::from_raw_parts_mut(out, avail_out);

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
    for chunk in result.as_chunks_mut::<2>().0 {
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
        } else if (0xC2..=0xC3).contains(&byte) {
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
        } else if (0x80..=0xBF).contains(&byte) {
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

// ── Windows-1252 (CP1252) ────────────────────────────────────────────────

/// Windows-1252 mapping for bytes 0x80..=0xFF (WHATWG windows-1252 == glibc
/// iconv CP1252). Bytes 0x81, 0x8D, 0x8F, 0x90, 0x9D are UNDEFINED in the
/// encoding (iconv raises EILSEQ on them). 0x00..=0x7F are ASCII and 0xA0..=
/// 0xFF are the Latin-1 supplement, so only 0x80..=0x9F need the table below
/// (indexed by `byte - 0x80`, U+FFFF = undefined).
///
/// R-000157 closure (partial): the oracle serves windows-1252 through iconv;
/// the candidate now ships a native converter for this single-byte set.
const CP1252_C1: [u16; 32] = [
    0x20AC, 0xFFFF, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, // 80..87
    0x02C6, 0x2030, 0x0160, 0x2039, 0x0152, 0xFFFF, 0x017D, 0xFFFF, // 88..8F
    0xFFFF, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, // 90..97
    0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0xFFFF, 0x017E, 0x0178, // 98..9F
];

/// Map a Windows-1252 byte to its Unicode codepoint; `None` for the five
/// undefined C1 bytes.
#[allow(dead_code)]
pub(crate) const fn cp1252_byte_to_cp(byte: u8) -> Option<u32> {
    match byte {
        0x00..=0x7F => Some(byte as u32),
        0x80..=0x9F => {
            let cp = CP1252_C1[(byte - 0x80) as usize];
            if cp == 0xFFFF {
                None
            } else {
                Some(cp as u32)
            }
        }
        _ => Some(byte as u32), // 0xA0..=0xFF = Latin-1 supplement
    }
}

/// Map a Unicode codepoint back to its Windows-1252 byte; `None` when the
/// codepoint is not representable in windows-1252.
#[allow(dead_code)]
pub(crate) const fn cp_to_cp1252_byte(cp: u32) -> Option<u8> {
    if cp < 0x80 || (cp >= 0xA0 && cp <= 0xFF) {
        Some(cp as u8)
    } else if cp >= 0x80 && cp <= 0x9F {
        // Reverse scan of the C1 table (32 entries; called per character on
        // output conversion only).
        let mut i = 0;
        while i < 32 {
            if CP1252_C1[i] == cp as u16 {
                return Some(0x80 + i as u8);
            }
            i += 1;
        }
        None
    } else {
        None
    }
}

/// Convert a single UTF-8 character starting at `data[in_pos]` to its
/// codepoint. Returns `(cp, bytes_consumed)` or `None` on invalid UTF-8.
fn decode_utf8_char(data: &[u8], in_pos: usize) -> Option<(u32, usize)> {
    let b0 = *data.get(in_pos)?;
    if b0 < 0x80 {
        return Some((u32::from(b0), 1));
    }
    let (len, cp0) = match b0 {
        0xC2..=0xDF => (2, u32::from(b0 & 0x1F)),
        0xE0..=0xEF => (3, u32::from(b0 & 0x0F)),
        0xF0..=0xF4 => (4, u32::from(b0 & 0x07)),
        _ => return None,
    };
    if in_pos + len > data.len() {
        return None;
    }
    let mut cp = cp0;
    for k in 1..len {
        let b = data[in_pos + k];
        if b & 0xC0 != 0x80 {
            return None;
        }
        cp = (cp << 6) | u32::from(b & 0x3F);
    }
    Some((cp, len))
}

/// Convert a whole CP1252 byte slice to UTF-8.
///
/// Returns `Err(())` when a byte has no windows-1252 mapping (the five
/// undefined C1 bytes 0x81/0x8D/0x8F/0x90/0x9D — iconv raises EILSEQ).
pub(crate) fn cp1252_to_utf8(data: &[u8]) -> Result<Vec<u8>, ()> {
    let mut result = Vec::with_capacity(data.len() * 2);
    for &byte in data {
        let cp = match cp1252_byte_to_cp(byte) {
            None => return Err(()),
            Some(cp) => cp,
        };
        let mut buf = [0u8; 4];
        let n = encode_codepoint_to_utf8(cp, &mut buf);
        result.extend_from_slice(&buf[..n]);
    }
    Ok(result)
}

/// Convert UTF-8 bytes to CP1252 (used by whole-buffer output paths).
///
/// Returns `Err(())` on invalid UTF-8 or an unrepresentable codepoint.
#[allow(dead_code)]
pub(crate) fn utf8_to_cp1252(data: &[u8]) -> Result<Vec<u8>, ()> {
    let mut result = Vec::with_capacity(data.len());
    let mut pos = 0;
    while pos < data.len() {
        let (cp, consumed) = match decode_utf8_char(data, pos) {
            None => return Err(()),
            Some(v) => v,
        };
        let byte = match cp_to_cp1252_byte(cp) {
            None => return Err(()),
            Some(b) => b,
        };
        result.push(byte);
        pos += consumed;
    }
    Ok(result)
}

/// Windows-1252 input function: convert CP1252 bytes to UTF-8.
unsafe extern "C" fn cp1252_input_func(
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
        let cp = match cp1252_byte_to_cp(byte) {
            // Undefined byte (0x81/0x8D/0x8F/0x90/0x9D): EILSEQ like iconv.
            None => {
                *outlen = out_pos as c_int;
                *inlen = in_pos as c_int;
                return -1;
            }
            Some(cp) => cp,
        };
        let mut buf = [0u8; 4];
        let n = encode_codepoint_to_utf8(cp, &mut buf);
        if out_pos + n > avail_out {
            break;
        }
        out_slice[out_pos..out_pos + n].copy_from_slice(&buf[..n]);
        out_pos += n;
        in_pos += 1;
    }

    *outlen = out_pos as c_int;
    *inlen = in_pos as c_int;
    out_pos as c_int
}

/// Windows-1252 output function: convert UTF-8 to CP1252 bytes.
unsafe extern "C" fn cp1252_output_func(
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
        let (cp, consumed) = match decode_utf8_char(in_data, in_pos) {
            None => {
                *outlen = out_pos as c_int;
                *inlen = in_pos as c_int;
                return -1;
            }
            Some(v) => v,
        };
        let byte = match cp_to_cp1252_byte(cp) {
            None => {
                // Not representable in windows-1252: EILSEQ like iconv.
                *outlen = out_pos as c_int;
                *inlen = in_pos as c_int;
                return -1;
            }
            Some(b) => b,
        };
        out_slice[out_pos] = byte;
        out_pos += 1;
        in_pos += consumed;
    }

    *outlen = out_pos as c_int;
    *inlen = in_pos as c_int;
    out_pos as c_int
}

// ── ASCII ─────────────────────────────────────────────────────────────────

/// ASCII input function: verify and pass through ASCII data to UTF-8.
///
/// Returns the number of bytes written, `-1` on invalid arguments, or
/// `-2` (the candidate's input-error code) when a byte >= 0x80 is reached
/// — in that case `*inlen`/`*outlen` hold the bytes consumed/written before
/// the offending character, so the output converter (`char_enc_out`) can
/// decode the UTF-8 character and replace it with a decimal character
/// reference (upstream `asciiToAscii` returns XML_ENC_ERR_INPUT).
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
            // Not valid ASCII: report how much was consumed so the caller
            // can substitute a character reference and retry.
            *outlen = pos as c_int;
            *inlen = pos as c_int;
            return -2;
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

// ── Shift_JIS / EUC-JP (encoding_rs-backed; R-000157 closure slice) ────────

/// Module-level input-error code: a converter reports the character at
/// `*inlen` as unrepresentable and `char_enc_out` substitutes the upstream
/// decimal character reference (&#NNN;) before retrying (encoding.c
/// xmlCharEncOutput XML_ENC_ERR_INPUT path).
const ENC_INPUT_ERROR: c_int = -2;

/// Shared output conversion for the encoding_rs-backed East-Asian handlers
/// (UTF-8 → `target`). House func contract (see cp1252): complete UTF-8
/// characters are converted while output space lasts; the first character
/// `target` cannot represent stops the conversion and is reported with the
/// -2 input-error convention (so `char_enc_out` emits the decimal character
/// reference and retries); invalid UTF-8 (or an incomplete trailing
/// sequence) reports -1 with the bytes before the error in `*inlen`. No
/// charref expansion happens inside the func, and Shift_JIS/EUC-JP output is
/// at most 1:1 with the UTF-8 input on the representable repertoire, so the
/// caller's >= 3x scratch can never overflow.
unsafe fn enc_rs_output(
    target: &'static encoding_rs::Encoding,
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

    // Convert only complete UTF-8 characters. On invalid bytes (or an
    // incomplete trailing sequence) the valid prefix is converted and the
    // error is reported at the first offending byte (upstream iconv EILSEQ;
    // the xmlCharEncOutput error path decodes the UTF-8 character there).
    let (s, error_at) = match core::str::from_utf8(in_data) {
        Ok(s) => (s, None),
        Err(e) => {
            let valid = e.valid_up_to();
            if valid == 0 {
                *outlen = 0;
                *inlen = 0;
                return -1;
            }
            // SAFETY: `valid` is a UTF-8 boundary (from_utf8 guarantees the
            // valid prefix ends on a character boundary).
            (
                unsafe { core::str::from_utf8_unchecked(&in_data[..valid]) },
                Some(valid),
            )
        }
    };

    let mut encoder = target.new_encoder();
    let mut in_pos: usize = 0;
    let mut out_pos: usize = 0;
    while in_pos < s.len() && out_pos < avail_out {
        let dst = &mut out_slice[out_pos..];
        let (res, read, written) =
            encoder.encode_from_utf8_without_replacement(&s[in_pos..], dst, true);
        out_pos += written;
        in_pos += read;
        match res {
            encoding_rs::EncoderResult::InputEmpty => break,
            encoding_rs::EncoderResult::OutputFull => {
                // Output exhausted: report the partial conversion (with the
                // caller's >= 3x scratch this is unreachable for these
                // encodings on complete input).
                break;
            }
            encoding_rs::EncoderResult::Unmappable(c) => {
                // The encoder consumed the unrepresentable character `c`
                // (its UTF-8 bytes are the last len_utf8() bytes of the
                // consumed prefix), so rewind *inlen to point AT it:
                // char_enc_out substitutes the decimal character reference
                // for the character there and retries the remainder.
                *outlen = out_pos as c_int;
                *inlen = (in_pos - c.len_utf8()) as c_int;
                return ENC_INPUT_ERROR;
            }
        }
    }

    if let Some(err) = error_at {
        if in_pos == s.len() {
            // The whole convertible prefix was converted; report the UTF-8
            // error at the offending byte (the trailing partial is not
            // converted).
            *outlen = out_pos as c_int;
            *inlen = err as c_int;
            return -1;
        }
    }
    *outlen = out_pos as c_int;
    *inlen = in_pos as c_int;
    out_pos as c_int
}

/// Shared input conversion for the encoding_rs-backed East-Asian handlers
/// (`source` → UTF-8). Converts complete characters while output space
/// lasts; an undefined byte or an incomplete trailing sequence reports -1
/// with the bytes before the error in `*inlen` (iconv EILSEQ semantics —
/// deterministic and loop-free for the caller).
unsafe fn enc_rs_input(
    source: &'static encoding_rs::Encoding,
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

    let mut decoder = source.new_decoder_without_bom_handling();
    let mut in_pos: usize = 0;
    let mut out_pos: usize = 0;
    while in_pos < avail_in && out_pos < avail_out {
        let (res, read, written) = decoder.decode_to_utf8_without_replacement(
            &in_data[in_pos..],
            &mut out_slice[out_pos..],
            true,
        );
        out_pos += written;
        in_pos += read;
        match res {
            encoding_rs::DecoderResult::InputEmpty => break,
            encoding_rs::DecoderResult::OutputFull => break,
            encoding_rs::DecoderResult::Malformed(..) => {
                // Undefined byte or incomplete tail: hard error after the
                // complete prefix (iconv EILSEQ).
                *outlen = out_pos as c_int;
                *inlen = in_pos as c_int;
                return -1;
            }
        }
    }

    *outlen = out_pos as c_int;
    *inlen = in_pos as c_int;
    out_pos as c_int
}

/// Shift_JIS input function (CP932-compatible WHATWG Shift_JIS → UTF-8).
unsafe extern "C" fn shift_jis_input_func(
    out: *mut c_uchar,
    outlen: *mut c_int,
    in_: *const c_uchar,
    inlen: *mut c_int,
) -> c_int {
    enc_rs_input(encoding_rs::SHIFT_JIS, out, outlen, in_, inlen)
}

/// Shift_JIS output function (UTF-8 → CP932-compatible WHATWG Shift_JIS).
unsafe extern "C" fn shift_jis_output_func(
    out: *mut c_uchar,
    outlen: *mut c_int,
    in_: *const c_uchar,
    inlen: *mut c_int,
) -> c_int {
    enc_rs_output(encoding_rs::SHIFT_JIS, out, outlen, in_, inlen)
}

/// EUC-JP input function (EUC-JP → UTF-8).
unsafe extern "C" fn euc_jp_input_func(
    out: *mut c_uchar,
    outlen: *mut c_int,
    in_: *const c_uchar,
    inlen: *mut c_int,
) -> c_int {
    enc_rs_input(encoding_rs::EUC_JP, out, outlen, in_, inlen)
}

/// EUC-JP output function (UTF-8 → EUC-JP).
unsafe extern "C" fn euc_jp_output_func(
    out: *mut c_uchar,
    outlen: *mut c_int,
    in_: *const c_uchar,
    inlen: *mut c_int,
) -> c_int {
    enc_rs_output(encoding_rs::EUC_JP, out, outlen, in_, inlen)
}

// ── ISO-8859-2..11 / 13..16 + ISO-2022-JP (encoding_rs-backed, R-000157) ──

/// Generate the input/output func pair for an encoding_rs single-byte or
/// stateful legacy encoding served by name (upstream: iconv).
macro_rules! define_enc_rs_codec {
    ($input_fn:ident, $output_fn:ident, $enc:expr) => {
        #[allow(dead_code)]
        unsafe extern "C" fn $input_fn(
            out: *mut c_uchar,
            outlen: *mut c_int,
            in_: *const c_uchar,
            inlen: *mut c_int,
        ) -> c_int {
            enc_rs_input($enc, out, outlen, in_, inlen)
        }
        #[allow(dead_code)]
        unsafe extern "C" fn $output_fn(
            out: *mut c_uchar,
            outlen: *mut c_int,
            in_: *const c_uchar,
            inlen: *mut c_int,
        ) -> c_int {
            enc_rs_output($enc, out, outlen, in_, inlen)
        }
    };
}

define_enc_rs_codec!(
    iso_8859_2_input_func,
    iso_8859_2_output_func,
    encoding_rs::ISO_8859_2
);
define_enc_rs_codec!(
    iso_8859_3_input_func,
    iso_8859_3_output_func,
    encoding_rs::ISO_8859_3
);
define_enc_rs_codec!(
    iso_8859_4_input_func,
    iso_8859_4_output_func,
    encoding_rs::ISO_8859_4
);
define_enc_rs_codec!(
    iso_8859_5_input_func,
    iso_8859_5_output_func,
    encoding_rs::ISO_8859_5
);
define_enc_rs_codec!(
    iso_8859_6_input_func,
    iso_8859_6_output_func,
    encoding_rs::ISO_8859_6
);
define_enc_rs_codec!(
    iso_8859_7_input_func,
    iso_8859_7_output_func,
    encoding_rs::ISO_8859_7
);
define_enc_rs_codec!(
    iso_8859_8_input_func,
    iso_8859_8_output_func,
    encoding_rs::ISO_8859_8
);
define_enc_rs_codec!(
    iso_8859_9_input_func,
    iso_8859_9_output_func,
    encoding_rs::WINDOWS_1254
);
define_enc_rs_codec!(
    iso_8859_10_input_func,
    iso_8859_10_output_func,
    encoding_rs::ISO_8859_10
);
define_enc_rs_codec!(
    iso_8859_11_input_func,
    iso_8859_11_output_func,
    encoding_rs::WINDOWS_874
);
define_enc_rs_codec!(
    iso_8859_13_input_func,
    iso_8859_13_output_func,
    encoding_rs::ISO_8859_13
);
define_enc_rs_codec!(
    iso_8859_14_input_func,
    iso_8859_14_output_func,
    encoding_rs::ISO_8859_14
);
define_enc_rs_codec!(
    iso_8859_15_input_func,
    iso_8859_15_output_func,
    encoding_rs::ISO_8859_15
);
define_enc_rs_codec!(
    iso_8859_16_input_func,
    iso_8859_16_output_func,
    encoding_rs::ISO_8859_16
);
define_enc_rs_codec!(
    iso_2022_jp_input_func,
    iso_2022_jp_output_func,
    encoding_rs::ISO_2022_JP
);

// ── UCS-2 / UCS-4 / EBCDIC (native codecs; R-000157 remainder) ────────────

/// Shared fixed-width-input converter: 2/4-byte big/little-endian code
/// units → UTF-8. `width` is 2 (UCS-2) or 4 (UCS-4). Undefined code units
/// (surrogates for UCS-2, > U+10FFFF for UCS-4) report -1 after the complete
/// prefix (iconv EILSEQ); an incomplete trailing unit stops cleanly with the
/// complete prefix consumed (iconv EINVAL semantics — the caller owns the
/// tail).
unsafe fn fixed_width_input(
    le: bool,
    width: usize,
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
    let mut in_pos = 0usize;
    let mut out_pos = 0usize;
    while in_pos + width <= avail_in {
        let mut unit: u32 = 0;
        for k in 0..width {
            let b = in_data[in_pos + k] as u32;
            unit = if le {
                unit | (b << (8 * k))
            } else {
                (unit << 8) | b
            };
        }
        if unit > 0x10FFFF || (0xD800..=0xDFFF).contains(&unit) {
            // Undefined code unit: hard error after the complete prefix.
            *outlen = out_pos as c_int;
            *inlen = in_pos as c_int;
            return -1;
        }
        let mut buf = [0u8; 4];
        // SAFETY: unit <= 0x10FFFF and not a surrogate, so char::from_u32
        // succeeds.
        let ch = unsafe { char::from_u32_unchecked(unit) };
        let n = ch.encode_utf8(&mut buf).len();
        if out_pos + n > avail_out {
            break;
        }
        out_slice[out_pos..out_pos + n].copy_from_slice(&buf[..n]);
        out_pos += n;
        in_pos += width;
    }
    *outlen = out_pos as c_int;
    *inlen = in_pos as c_int;
    out_pos as c_int
}

/// Shared fixed-width OUTPUT converter: UTF-8 → 2/4-byte big/little-endian
/// code units. A code point that does not fit the width (astral under UCS-2)
/// stops with the -2 input-error convention (charref substitution).
unsafe fn fixed_width_output(
    le: bool,
    width: usize,
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
    let mut in_pos = 0usize;
    let mut out_pos = 0usize;
    while in_pos < avail_in {
        let (cp, consumed) = match decode_utf8_char(in_data, in_pos) {
            None => {
                // Invalid UTF-8 (or an incomplete trailing sequence): hard
                // error after the complete prefix (iconv EILSEQ/EINVAL).
                *outlen = out_pos as c_int;
                *inlen = in_pos as c_int;
                return -1;
            }
            Some(v) => v,
        };
        let max_cp = if width == 2 { 0xFFFF } else { 0x10FFFF };
        if cp > max_cp {
            // Not representable at this width (astral under UCS-2): stop
            // BEFORE it — char_enc_out substitutes the decimal charref.
            *outlen = out_pos as c_int;
            *inlen = in_pos as c_int;
            return ENC_INPUT_ERROR;
        }
        if out_pos + width > avail_out {
            break;
        }
        for k in 0..width {
            let shift = 8 * if le { k } else { width - 1 - k };
            out_slice[out_pos + k] = ((cp >> shift) & 0xFF) as u8;
        }
        out_pos += width;
        in_pos += consumed;
    }
    *outlen = out_pos as c_int;
    *inlen = in_pos as c_int;
    out_pos as c_int
}

/// UCS-2 input (2-byte units → UTF-8). glibc iconv "UCS-2" uses the host
/// byte order (little-endian on the executed x86-64 oracle), so the codec
/// is little-endian to match.
unsafe extern "C" fn ucs2_input_func(
    out: *mut c_uchar,
    outlen: *mut c_int,
    in_: *const c_uchar,
    inlen: *mut c_int,
) -> c_int {
    fixed_width_input(true, 2, out, outlen, in_, inlen)
}

/// UCS-2 output (UTF-8 → 2-byte little-endian units).
unsafe extern "C" fn ucs2_output_func(
    out: *mut c_uchar,
    outlen: *mut c_int,
    in_: *const c_uchar,
    inlen: *mut c_int,
) -> c_int {
    fixed_width_output(true, 2, out, outlen, in_, inlen)
}

/// UCS-4LE input (4-byte little-endian units → UTF-8).
unsafe extern "C" fn ucs4le_input_func(
    out: *mut c_uchar,
    outlen: *mut c_int,
    in_: *const c_uchar,
    inlen: *mut c_int,
) -> c_int {
    fixed_width_input(true, 4, out, outlen, in_, inlen)
}

/// UCS-4LE output (UTF-8 → 4-byte little-endian units).
unsafe extern "C" fn ucs4le_output_func(
    out: *mut c_uchar,
    outlen: *mut c_int,
    in_: *const c_uchar,
    inlen: *mut c_int,
) -> c_int {
    fixed_width_output(true, 4, out, outlen, in_, inlen)
}

/// UCS-4BE input (4-byte big-endian units → UTF-8).
unsafe extern "C" fn ucs4be_input_func(
    out: *mut c_uchar,
    outlen: *mut c_int,
    in_: *const c_uchar,
    inlen: *mut c_int,
) -> c_int {
    fixed_width_input(false, 4, out, outlen, in_, inlen)
}

/// UCS-4BE output (UTF-8 → 4-byte big-endian units).
unsafe extern "C" fn ucs4be_output_func(
    out: *mut c_uchar,
    outlen: *mut c_int,
    in_: *const c_uchar,
    inlen: *mut c_int,
) -> c_int {
    fixed_width_output(false, 4, out, outlen, in_, inlen)
}

/// EBCDIC code page 037 → Unicode (derived from the oracle container's glibc
/// iconv IBM037 table: byte i maps to EBCDIC037_TO_UNICODE[i]; the mapping is
/// a bijection onto U+0000..U+00FF).
const EBCDIC037_TO_UNICODE: [u16; 256] = [
    0x0000, 0x0001, 0x0002, 0x0003, 0x009C, 0x0009, 0x0086, 0x007F, 0x0097, 0x008D, 0x008E, 0x000B,
    0x000C, 0x000D, 0x000E, 0x000F, 0x0010, 0x0011, 0x0012, 0x0013, 0x009D, 0x0085, 0x0008, 0x0087,
    0x0018, 0x0019, 0x0092, 0x008F, 0x001C, 0x001D, 0x001E, 0x001F, 0x0080, 0x0081, 0x0082, 0x0083,
    0x0084, 0x000A, 0x0017, 0x001B, 0x0088, 0x0089, 0x008A, 0x008B, 0x008C, 0x0005, 0x0006, 0x0007,
    0x0090, 0x0091, 0x0016, 0x0093, 0x0094, 0x0095, 0x0096, 0x0004, 0x0098, 0x0099, 0x009A, 0x009B,
    0x0014, 0x0015, 0x009E, 0x001A, 0x0020, 0x00A0, 0x00E2, 0x00E4, 0x00E0, 0x00E1, 0x00E3, 0x00E5,
    0x00E7, 0x00F1, 0x00A2, 0x002E, 0x003C, 0x0028, 0x002B, 0x007C, 0x0026, 0x00E9, 0x00EA, 0x00EB,
    0x00E8, 0x00ED, 0x00EE, 0x00EF, 0x00EC, 0x00DF, 0x0021, 0x0024, 0x002A, 0x0029, 0x003B, 0x00AC,
    0x002D, 0x002F, 0x00C2, 0x00C4, 0x00C0, 0x00C1, 0x00C3, 0x00C5, 0x00C7, 0x00D1, 0x00A6, 0x002C,
    0x0025, 0x005F, 0x003E, 0x003F, 0x00F8, 0x00C9, 0x00CA, 0x00CB, 0x00C8, 0x00CD, 0x00CE, 0x00CF,
    0x00CC, 0x0060, 0x003A, 0x0023, 0x0040, 0x0027, 0x003D, 0x0022, 0x00D8, 0x0061, 0x0062, 0x0063,
    0x0064, 0x0065, 0x0066, 0x0067, 0x0068, 0x0069, 0x00AB, 0x00BB, 0x00F0, 0x00FD, 0x00FE, 0x00B1,
    0x00B0, 0x006A, 0x006B, 0x006C, 0x006D, 0x006E, 0x006F, 0x0070, 0x0071, 0x0072, 0x00AA, 0x00BA,
    0x00E6, 0x00B8, 0x00C6, 0x00A4, 0x00B5, 0x007E, 0x0073, 0x0074, 0x0075, 0x0076, 0x0077, 0x0078,
    0x0079, 0x007A, 0x00A1, 0x00BF, 0x00D0, 0x00DD, 0x00DE, 0x00AE, 0x005E, 0x00A3, 0x00A5, 0x00B7,
    0x00A9, 0x00A7, 0x00B6, 0x00BC, 0x00BD, 0x00BE, 0x005B, 0x005D, 0x00AF, 0x00A8, 0x00B4, 0x00D7,
    0x007B, 0x0041, 0x0042, 0x0043, 0x0044, 0x0045, 0x0046, 0x0047, 0x0048, 0x0049, 0x00AD, 0x00F4,
    0x00F6, 0x00F2, 0x00F3, 0x00F5, 0x007D, 0x004A, 0x004B, 0x004C, 0x004D, 0x004E, 0x004F, 0x0050,
    0x0051, 0x0052, 0x00B9, 0x00FB, 0x00FC, 0x00F9, 0x00FA, 0x00FF, 0x005C, 0x00F7, 0x0053, 0x0054,
    0x0055, 0x0056, 0x0057, 0x0058, 0x0059, 0x005A, 0x00B2, 0x00D4, 0x00D6, 0x00D2, 0x00D3, 0x00D5,
    0x0030, 0x0031, 0x0032, 0x0033, 0x0034, 0x0035, 0x0036, 0x0037, 0x0038, 0x0039, 0x00B3, 0x00DB,
    0x00DC, 0x00D9, 0x00DA, 0x009F,
];

/// Reverse lookup: Unicode code point → EBCDIC 037 byte. The forward table
/// is a bijection onto U+0000..U+00FF, so any cp <= 0xFF resolves.
const fn ebcdic037_cp_to_byte(cp: u32) -> Option<u8> {
    if cp > 0xFF {
        return None;
    }
    let mut i = 0;
    while i < 256 {
        if EBCDIC037_TO_UNICODE[i] as u32 == cp {
            return Some(i as u8);
        }
        i += 1;
    }
    None
}

/// EBCDIC (IBM037) input: bytes → UTF-8 via the 037 table.
unsafe extern "C" fn ebcdic_input_func(
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
    let mut in_pos = 0usize;
    let mut out_pos = 0usize;
    while in_pos < avail_in {
        let cp = u32::from(EBCDIC037_TO_UNICODE[in_data[in_pos] as usize]);
        let mut buf = [0u8; 2];
        let ch = unsafe { char::from_u32_unchecked(cp) };
        let n = ch.encode_utf8(&mut buf).len();
        if out_pos + n > avail_out {
            break;
        }
        out_slice[out_pos..out_pos + n].copy_from_slice(&buf[..n]);
        out_pos += n;
        in_pos += 1;
    }
    *outlen = out_pos as c_int;
    *inlen = in_pos as c_int;
    out_pos as c_int
}

/// EBCDIC (IBM037) output: UTF-8 → 037 bytes. Unmappable code points
/// (> U+00FF) stop with the -2 input-error convention (charref).
unsafe extern "C" fn ebcdic_output_func(
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
    let mut in_pos = 0usize;
    let mut out_pos = 0usize;
    while in_pos < avail_in {
        let (cp, consumed) = match decode_utf8_char(in_data, in_pos) {
            None => {
                *outlen = out_pos as c_int;
                *inlen = in_pos as c_int;
                return -1;
            }
            Some(v) => v,
        };
        match ebcdic037_cp_to_byte(cp) {
            None => {
                *outlen = out_pos as c_int;
                *inlen = in_pos as c_int;
                return ENC_INPUT_ERROR;
            }
            Some(byte) => {
                if out_pos + 1 > avail_out {
                    break;
                }
                out_slice[out_pos] = byte;
                out_pos += 1;
            }
        }
        in_pos += consumed;
    }
    *outlen = out_pos as c_int;
    *inlen = in_pos as c_int;
    out_pos as c_int
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
pub(crate) const fn xmlGetCharEncodingName(enc: xmlCharEncoding) -> *const c_char {
    // Return null-terminated C strings using static CStr literals.
    // Mirrors upstream 2.15 xmlGetCharEncodingName: the UTF-16/UCS-4 pairs
    // return the W3C canonical names before the defaultHandlers table.
    match enc {
        xmlCharEncoding::XML_CHAR_ENCODING_UTF8 => c"UTF-8".as_ptr(),
        xmlCharEncoding::XML_CHAR_ENCODING_UTF16LE | xmlCharEncoding::XML_CHAR_ENCODING_UTF16BE => {
            c"UTF-16".as_ptr()
        }
        xmlCharEncoding::XML_CHAR_ENCODING_UCS4LE | xmlCharEncoding::XML_CHAR_ENCODING_UCS4BE => {
            c"UCS-4".as_ptr()
        }
        xmlCharEncoding::XML_CHAR_ENCODING_EBCDIC => c"IBM037".as_ptr(),
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
        xmlCharEncoding::XML_CHAR_ENCODING_SHIFT_JIS => c"Shift_JIS".as_ptr(),
        xmlCharEncoding::XML_CHAR_ENCODING_EUC_JP => c"EUC-JP".as_ptr(),
        // upstream defaultHandlers[22].name
        xmlCharEncoding::XML_CHAR_ENCODING_ASCII => c"US-ASCII".as_ptr(),
        _ => ptr::null(),
    }
}

/// `xmlParseCharEncoding` implementation.
///
/// Parses an encoding name string to an `xmlCharEncoding` enum value,
/// returned as `c_int`.
///
/// # Safety
///
/// - `name` must be NULL or a valid pointer to a NUL-terminated string.
pub(crate) fn xmlParseCharEncoding(name: *const c_char) -> c_int {
    if name.is_null() {
        return xmlCharEncoding::XML_CHAR_ENCODING_NONE as c_int;
    }
    let bytes = unsafe { CStr::from_ptr(name).to_bytes() };
    encoding_from_name(bytes) as c_int
}

// ── Encoding aliases (upstream encoding.c xmlAddEncodingAlias etc.) ──────────
//
// A global alias table maps alias names to canonical encoding names.
// Upstream keeps a static hash of aliases; the candidate uses a
// process-lifetime RwLock<HashMap>. Thread-safe; matches upstream's
// observable contract (add/del/get by name).

static ENCODING_ALIASES: std::sync::OnceLock<
    parking_lot::RwLock<std::collections::HashMap<Vec<u8>, Vec<u8>>>,
> = std::sync::OnceLock::new();

fn encoding_aliases() -> &'static parking_lot::RwLock<std::collections::HashMap<Vec<u8>, Vec<u8>>> {
    ENCODING_ALIASES.get_or_init(|| parking_lot::RwLock::new(std::collections::HashMap::new()))
}

/// `xmlAddEncodingAlias` implementation: register `alias` for `name`.
/// Returns 0 on success, -1 on error (NULL arguments).
///
/// # Safety
///
/// - `name` and `alias` must be NULL or valid pointers to NUL-terminated
///   strings; both are copied before insertion into the alias table.
pub(crate) fn add_encoding_alias(name: *const c_char, alias: *const c_char) -> c_int {
    if name.is_null() || alias.is_null() {
        return -1;
    }
    let n = unsafe { CStr::from_ptr(name).to_bytes().to_vec() };
    let a = unsafe { CStr::from_ptr(alias).to_bytes().to_vec() };
    encoding_aliases().write().insert(a, n);
    0
}

/// `xmlDelEncodingAlias` implementation: remove `alias`.
/// Returns 0 on success, -1 if the alias does not exist.
///
/// # Safety
///
/// - `alias` must be NULL or a valid pointer to a NUL-terminated string.
pub(crate) fn del_encoding_alias(alias: *const c_char) -> c_int {
    if alias.is_null() {
        return -1;
    }
    let a = unsafe { CStr::from_ptr(alias).to_bytes().to_vec() };
    if encoding_aliases().write().remove(&a).is_some() {
        0
    } else {
        -1
    }
}

/// `xmlGetEncodingAlias` implementation: return the canonical name for
/// `alias`, or NULL when not registered.
///
/// # Safety
///
/// - `alias` must be NULL or a valid pointer to a NUL-terminated string.
/// - The returned pointer is a leaked, process-lifetime NUL-terminated
///   string, or NULL; the caller must not free it.
pub(crate) fn get_encoding_alias(alias: *const c_char) -> *const c_char {
    if alias.is_null() {
        return ptr::null();
    }
    let a = unsafe { CStr::from_ptr(alias).to_bytes().to_vec() };
    let guard = encoding_aliases().read();
    match guard.get(&a) {
        Some(v) => {
            // leak the canonical name: upstream returns a pointer valid for
            // the process lifetime (the alias hash owns the strings)
            let leaked: &'static [u8] = Box::leak(v.clone().into_boxed_slice());
            leaked.as_ptr() as *const c_char
        }
        None => ptr::null(),
    }
}

/// `xmlCleanupEncodingAliases` implementation: drop all aliases.
pub(crate) fn cleanup_encoding_aliases() {
    encoding_aliases().write().clear();
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
///
/// # Safety
///
/// - `name` must be NULL or a valid pointer to a NUL-terminated string that
///   stays valid until it is duplicated.
/// - `input` and `output` must be valid function pointers matching the
///   callback ABI; on success the returned handler owns a duplicated name
///   and must be released with `xmlDelEncodingHandler`.
pub(crate) fn xmlNewCharEncodingHandler(
    name: *const c_char,
    input: xmlCharEncodingInputFunc,
    output: xmlCharEncodingOutputFunc,
) -> *mut _xmlCharEncodingHandler {
    if name.is_null() {
        return ptr::null_mut();
    }

    let name_raw = unsafe { crate::abi::allocator::xmlMemStrdupImpl(name) };
    if name_raw.is_null() {
        return ptr::null_mut();
    }

    let handler = unsafe { xmlMallocImpl(size_of::<_xmlCharEncodingHandler>()) }
        as *mut _xmlCharEncodingHandler;

    if handler.is_null() {
        unsafe { xmlFreeImpl(name_raw) };
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
///
/// # Safety
///
/// - `handler` must be NULL or a valid heap-allocated
///   `_xmlCharEncodingHandler` whose `name` is NULL or a heap-allocated
///   NUL-terminated string; both allocations are freed exactly once, and the
///   handler must have been removed from the registry.
#[allow(dead_code)]
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
            xmlFreeImpl((*handler).name as *mut c_void);
        }
        xmlFreeImpl(handler as *mut c_void);
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
// 7. Handler lookup / creation (upstream 2.13.0+ encoding.c)
// ═══════════════════════════════════════════════════════════════════════════════
//
// Upstream keeps a static `defaultHandlers[32]` table indexed by xmlCharEncoding
// plus iconv/ICU fallbacks. The candidate ships no iconv/ICU, so encodings whose
// upstream default handler carries a real converter (UTF-8, UTF-16LE, UTF-16BE,
// UTF-16, ISO-8859-1, US-ASCII) resolve to the registered built-in handlers;
// every other encoding reports XML_ERR_UNSUPPORTED_ENCODING exactly where
// upstream would fall through to iconv/ICU.

/// `xmlLookupCharEncodingHandler` implementation (upstream encoding.c).
///
/// Mirrors the upstream control flow:
///  - `out == NULL`                     → XML_ERR_ARGUMENT (115)
///  - `enc <= 0 || enc >= 32`           → XML_ERR_UNSUPPORTED_ENCODING (32)
///  - UTF-8                             → XML_ERR_OK, `*out` stays NULL
///  - native built-in encoding          → XML_ERR_OK, `*out` = static handler
///  - iconv/ICU-only encoding           → XML_ERR_UNSUPPORTED_ENCODING
///
/// The returned handler is a static registry entry and must NOT be freed.
///
/// # Safety
///
/// - `out` must be a valid pointer to a `*mut c_void` out-parameter; it is
///   written with NULL or a pointer to a static registry handler that the
///   caller must not free.
pub(crate) fn xmlLookupCharEncodingHandler(enc: c_int, out: *mut *mut c_void) -> c_int {
    if out.is_null() {
        return crate::abi::types::XML_ERR_ARGUMENT;
    }
    unsafe {
        *out = ptr::null_mut();
    }
    if enc <= 0 || enc >= 32 {
        return crate::abi::types::XML_ERR_UNSUPPORTED_ENCODING;
    }
    /* Return NULL handler for UTF-8 */
    if enc == xmlCharEncoding::XML_CHAR_ENCODING_UTF8 as c_int {
        return crate::abi::types::XML_ERR_OK;
    }
    let canonical: &[u8] = match enc {
        /* XML_CHAR_ENCODING_UTF16LE */
        2 => b"UTF-16LE\0",
        /* XML_CHAR_ENCODING_UTF16BE */
        3 => b"UTF-16BE\0",
        /* XML_CHAR_ENCODING_8859_1 */
        10 => b"ISO-8859-1\0",
        /* XML_CHAR_ENCODING_ASCII */
        22 => b"US-ASCII\0",
        /* XML_CHAR_ENCODING_UTF16 (not in the local enum) */
        23 => b"UTF-16\0",
        _ => return crate::abi::types::XML_ERR_UNSUPPORTED_ENCODING,
    };
    let h = find_encoding_handler(canonical.as_ptr() as *const xmlChar);
    if h.is_null() {
        return crate::abi::types::XML_ERR_UNSUPPORTED_ENCODING;
    }
    unsafe {
        *out = h as *mut c_void;
    }
    crate::abi::types::XML_ERR_OK
}

/// `xmlGetCharEncodingHandler` implementation (deprecated upstream wrapper).
pub(crate) fn xmlGetCharEncodingHandler(enc: c_int) -> *mut c_void {
    let mut ret: *mut c_void = ptr::null_mut();
    let _rc = xmlLookupCharEncodingHandler(enc, &mut ret);
    ret
}

/// `xmlCreateCharEncodingHandler` implementation (upstream 2.14.0+ encoding.c).
///
/// Flags: XML_ENC_INPUT = 1, XML_ENC_OUTPUT = 2, XML_ENC_HTML = 4.
/// Unlike upstream, no iconv/ICU backend exists, so encodings without a native
/// converter fall through to `find_extra_handler` (custom impl / deprecated
/// global registry) and otherwise report XML_ERR_UNSUPPORTED_ENCODING.
///
/// # Safety
///
/// - `out` must be a valid pointer to a `*mut c_void` out-parameter; it is
///   written with NULL or a heap-allocated handler copy the caller owns.
/// - `name` must be NULL or a valid pointer to a NUL-terminated string.
/// - `implCtxt` is an opaque context forwarded to `find_extra_handler` and
///   must be valid for the callback that consumes it.
pub(crate) fn xmlCreateCharEncodingHandler(
    name: *const c_char,
    flags: c_int,
    impl_: Option<xmlCharEncConvImpl>,
    implCtxt: *mut c_void,
    out: *mut *mut c_void,
) -> c_int {
    if out.is_null() {
        return crate::abi::types::XML_ERR_ARGUMENT;
    }
    unsafe {
        *out = ptr::null_mut();
    }
    if name.is_null() || flags == 0 {
        return crate::abi::types::XML_ERR_ARGUMENT;
    }
    let norig = unsafe { CStr::from_ptr(name).to_bytes() };

    /* Alias resolution (upstream xmlGetEncodingAlias). */
    let mut eff: &[u8] = norig;
    let alias = get_encoding_alias(name);
    if !alias.is_null() {
        eff = unsafe { CStr::from_ptr(alias).to_bytes() };
    }

    let enc = encoding_from_name(eff);

    /* Return NULL handler for UTF-8 */
    if enc == xmlCharEncoding::XML_CHAR_ENCODING_UTF8 {
        return crate::abi::types::XML_ERR_OK;
    }

    let canonical: &[u8] = match enc {
        xmlCharEncoding::XML_CHAR_ENCODING_UTF16LE => b"UTF-16LE\0",
        xmlCharEncoding::XML_CHAR_ENCODING_UTF16BE => b"UTF-16BE\0",
        xmlCharEncoding::XML_CHAR_ENCODING_8859_1 => b"ISO-8859-1\0",
        xmlCharEncoding::XML_CHAR_ENCODING_ASCII => b"US-ASCII\0",
        _ => {
            return find_extra_handler(norig, eff, flags, impl_, implCtxt, out);
        }
    };
    let h = find_encoding_handler(canonical.as_ptr() as *const xmlChar);
    if h.is_null() {
        return find_extra_handler(norig, eff, flags, impl_, implCtxt, out);
    }
    unsafe {
        let src = &*h;
        let has_in = (flags & 1) == 0 || !src.input.legacyFunc.is_none();
        let has_out = (flags & 2) == 0 || !src.output.legacyFunc.is_none();
        if !has_in || !has_out {
            return find_extra_handler(norig, eff, flags, impl_, implCtxt, out);
        }
        /*
         * Return a copy of the handler with the original name (upstream
         * "Return a copy of the handler with the original name").
         */
        let copy =
            xmlMallocImpl(size_of::<_xmlCharEncodingHandler>()) as *mut _xmlCharEncodingHandler;
        if copy.is_null() {
            return crate::abi::types::XML_ERR_NO_MEMORY;
        }
        let name_copy = crate::abi::allocator::xmlMemStrdupImpl(name) as *mut c_char;
        if name_copy.is_null() {
            xmlFreeImpl(copy as *mut c_void);
            return crate::abi::types::XML_ERR_NO_MEMORY;
        }
        ptr::write(
            copy,
            _xmlCharEncodingHandler {
                name: name_copy,
                input: EncodingInputUnion {
                    legacyFunc: src.input.legacyFunc,
                },
                output: EncodingOutputUnion {
                    legacyFunc: src.output.legacyFunc,
                },
                inputCtxt: src.inputCtxt,
                outputCtxt: src.outputCtxt,
                ctxtDtor: src.ctxtDtor,
                flags: src.flags,
            },
        );
        *out = copy as *mut c_void;
    }
    crate::abi::types::XML_ERR_OK
}

/// Fallback path of `xmlCreateCharEncodingHandler` (upstream `xmlFindExtraHandler`).
///
/// Tries the caller-supplied custom implementation first, then the deprecated
/// global handler registry. iconv/ICU do not exist in the candidate, so the
/// final result is XML_ERR_UNSUPPORTED_ENCODING.
///
/// # Safety
///
/// - `norig` and `name` must be valid byte slices; NUL-terminated copies are
///   built from them for lookups and callbacks.
/// - `out` must be a valid out-parameter; it is written with NULL or a
///   registry handler pointer that must not be freed.
/// - `implCtxt` must be a valid context for the custom `impl_` callback when
///   one is supplied.
fn find_extra_handler(
    norig: &[u8],
    name: &[u8],
    flags: c_int,
    impl_: Option<xmlCharEncConvImpl>,
    implCtxt: *mut c_void,
    out: *mut *mut c_void,
) -> c_int {
    /* Custom implementation before deprecated global handlers. */
    if let Some(f) = impl_ {
        let mut n = norig.to_vec();
        n.push(0);
        let rc = unsafe {
            f(
                implCtxt,
                n.as_ptr() as *const c_char,
                flags,
                out as *mut *mut crate::abi::structs::_xmlCharEncodingHandler,
            )
        };
        return rc;
    }
    /* Deprecated global handlers registry (xmlRegisterCharEncodingHandler). */
    let mut n = name.to_vec();
    n.push(0);
    let h = find_encoding_handler(n.as_ptr() as *const xmlChar);
    if !h.is_null() {
        unsafe {
            let src = &*h;
            let has_in = (flags & 1) == 0 || !src.input.legacyFunc.is_none();
            let has_out = (flags & 2) == 0 || !src.output.legacyFunc.is_none();
            if has_in && has_out {
                *out = h as *mut c_void;
                return crate::abi::types::XML_ERR_OK;
            }
        }
    }
    crate::abi::types::XML_ERR_UNSUPPORTED_ENCODING
}

/// `xmlOpenCharEncodingHandler` implementation (upstream encoding.c).
pub(crate) fn xmlOpenCharEncodingHandler(
    name: *const c_char,
    output: c_int,
    out: *mut *mut c_void,
) -> c_int {
    /* XML_ENC_OUTPUT if output else XML_ENC_INPUT */
    let flags: c_int = if output != 0 { 2 } else { 1 };
    xmlCreateCharEncodingHandler(name, flags, None, ptr::null_mut(), out)
}

/// `xmlCharEncNewCustomHandler` implementation (upstream 2.15.0+ encoding.c).
///
/// Creates a handler backed by modern `xmlCharEncConvFunc` callbacks (with
/// per-direction contexts and a context destructor). The handler must be
/// released with `xmlCharEncCloseFunc`.
///
/// # Safety
///
/// - `out` must be a valid pointer to a `*mut c_void` out-parameter.
/// - `name` must be NULL or a valid pointer to a NUL-terminated string.
/// - `input` and `output` must be valid `xmlCharEncConvFunc` callbacks;
///   `inputCtxt` and `outputCtxt` are opaque contexts consumed by them and
///   by `ctxtDtor`, which is invoked on each non-NULL context when
///   allocation fails (and later by `xmlCharEncCloseFunc`).
pub(crate) fn xmlCharEncNewCustomHandler(
    name: *const c_char,
    input: xmlCharEncConvFunc,
    output: xmlCharEncConvFunc,
    ctxtDtor: Option<xmlCharEncConvCtxtDtor>,
    inputCtxt: *mut c_void,
    outputCtxt: *mut c_void,
    out: *mut *mut c_void,
) -> c_int {
    if out.is_null() {
        return crate::abi::types::XML_ERR_ARGUMENT;
    }
    let handler = unsafe { xmlMallocImpl(size_of::<_xmlCharEncodingHandler>()) }
        as *mut _xmlCharEncodingHandler;
    if handler.is_null() {
        unsafe {
            if let Some(d) = ctxtDtor {
                if !inputCtxt.is_null() {
                    d(inputCtxt);
                }
                if !outputCtxt.is_null() {
                    d(outputCtxt);
                }
            }
        }
        return crate::abi::types::XML_ERR_NO_MEMORY;
    }
    let name_copy = if name.is_null() {
        ptr::null_mut()
    } else {
        let nc = unsafe { crate::abi::allocator::xmlMemStrdupImpl(name) } as *mut c_char;
        if nc.is_null() {
            unsafe { xmlFreeImpl(handler as *mut c_void) };
            unsafe {
                if let Some(d) = ctxtDtor {
                    if !inputCtxt.is_null() {
                        d(inputCtxt);
                    }
                    if !outputCtxt.is_null() {
                        d(outputCtxt);
                    }
                }
            }
            return crate::abi::types::XML_ERR_NO_MEMORY;
        }
        nc
    };
    unsafe {
        ptr::write(
            handler,
            _xmlCharEncodingHandler {
                name: name_copy,
                input: EncodingInputUnion { func: Some(input) },
                output: EncodingOutputUnion { func: Some(output) },
                inputCtxt,
                outputCtxt,
                ctxtDtor,
                flags: 0,
            },
        );
        *out = handler as *mut c_void;
    }
    crate::abi::types::XML_ERR_OK
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

        let utf8_name: *const xmlChar = c"UTF-8".as_ptr() as *const xmlChar;
        assert!(!find_encoding_handler(utf8_name).is_null());

        let utf16le_name: *const xmlChar = c"UTF-16LE".as_ptr() as *const xmlChar;
        assert!(!find_encoding_handler(utf16le_name).is_null());

        let utf16be_name: *const xmlChar = c"UTF-16BE".as_ptr() as *const xmlChar;
        assert!(!find_encoding_handler(utf16be_name).is_null());

        let latin1_name: *const xmlChar = c"ISO-8859-1".as_ptr() as *const xmlChar;
        assert!(!find_encoding_handler(latin1_name).is_null());

        let ascii_name: *const xmlChar = c"ASCII".as_ptr() as *const xmlChar;
        assert!(!find_encoding_handler(ascii_name).is_null());

        // Case insensitive
        let lower_name: *const xmlChar = c"utf-8".as_ptr() as *const xmlChar;
        assert!(!find_encoding_handler(lower_name).is_null());
    }

    /// Phase 14 PHP court regression: the ABI `xmlFindCharEncodingHandler`
    /// hands the caller an OWNED handler that it may release with
    /// `xmlCharEncCloseFunc` — except UTF-8, where upstream returns a static
    /// handler that close must not release (so the registry is never freed
    /// out from under subsequent lookups). Closing a returned non-UTF-8
    /// handler must not free the persistent registry entry.
    ///
    /// # Safety
    ///
    /// - The handler returned by `xmlFindCharEncodingHandler_owned` is owned by
    ///   the caller and released here with the allocator, mirroring the export
    ///   `xmlCharEncCloseFunc` (which, for these stateless built-in handlers,
    ///   frees `name` and the struct without invoking any context destructor).
    #[test]
    fn test_find_owned_close_keeps_registry_intact() {
        init_encodings();
        let name: *const xmlChar = c"ISO-8859-1".as_ptr() as *const xmlChar;

        // The registry entry is a long-lived borrow.
        let registry = find_encoding_handler(name);
        assert!(!registry.is_null());
        // First retrieval returns an OWNED copy, distinct from the registry entry.
        let h1 = xmlFindCharEncodingHandler_owned(name);
        assert!(!h1.is_null());
        assert_ne!(h1 as *const c_void, registry as *const c_void);

        // Closing h1 (simulate xmlCharEncCloseFunc on a non-static handler):
        // frees its name + struct but NOT the registry entry.
        unsafe {
            if !(*h1).name.is_null() {
                crate::abi::allocator::xmlFreeImpl((*h1).name as *mut c_void);
            }
            xmlFreeImpl(h1 as *mut c_void);
        }

        // The registry entry must survive the close of a previous result with
        // its name intact (the PHP `$dom->encoding='UTF-16'` crash was the
        // registry entry itself being freed by this very close, so the next
        // lookup returned freed memory).
        let registry2 = find_encoding_handler(name);
        assert_eq!(registry2 as *const c_void, registry as *const c_void);
        assert!(!unsafe { (*registry2).name }.is_null());
        let reg_name = unsafe { CStr::from_ptr((*registry2).name as *const c_char) };
        assert_eq!(reg_name.to_bytes(), b"ISO-8859-1");

        // A second owned retrieval still works and is usable.
        let h2 = xmlFindCharEncodingHandler_owned(name);
        assert!(!h2.is_null());
        assert_ne!(h2 as *const c_void, registry as *const c_void);
        unsafe {
            if !(*h2).name.is_null() {
                crate::abi::allocator::xmlFreeImpl((*h2).name as *mut c_void);
            }
            xmlFreeImpl(h2 as *mut c_void);
        }
    }

    /// Phase 14 PHP court regression (UTF-8 subset): retrieval for UTF-8/UTF8
    /// returns the persistent static handler, and referencing it from a second
    /// caller must yield the same live pointer (the registry entry is never
    /// freed by a close — `xmlCharEncCloseFunc` on XML_HANDLER_STATIC is a
    /// no-op).
    #[test]
    fn test_find_owned_utf8_static_and_persistent() {
        init_encodings();
        let name: *const xmlChar = c"UTF-8".as_ptr() as *const xmlChar;
        let u1 = xmlFindCharEncodingHandler_owned(name);
        assert!(!u1.is_null());
        // Static: close must not release it, so a second find returns the same
        // live registry handler.
        let u2 = xmlFindCharEncodingHandler_owned(c"utf8".as_ptr() as *const xmlChar);
        assert_eq!(u1, u2);
        assert_eq!(
            unsafe { (*u1).flags } & XML_HANDLER_STATIC,
            XML_HANDLER_STATIC
        );
    }

    #[test]
    fn test_find_encoding_handler_not_found() {
        let name: *const xmlChar = c"NONEXISTENT".as_ptr() as *const xmlChar;
        assert!(find_encoding_handler(name).is_null());
    }

    #[test]
    fn test_find_encoding_handler_null() {
        assert!(find_encoding_handler(ptr::null()).is_null());
    }

    /// Verify registering a handler in the global registry and looking it
    /// up.
    ///
    /// # Safety
    ///
    /// - The `xmlMallocImpl` and `xmlMemStrdupImpl` results are NULL-checked
    ///   before `ptr::write` initializes the handler; the handler is removed
    ///   from the registry before its allocations are freed exactly once.
    #[test]
    fn test_add_encoding_handler() {
        let handler = unsafe {
            xmlMallocImpl(size_of::<_xmlCharEncodingHandler>()) as *mut _xmlCharEncodingHandler
        };
        assert!(!handler.is_null());

        let name = unsafe {
            crate::abi::allocator::xmlMemStrdupImpl(c"TEST-ENC".as_ptr() as *const c_char)
        };
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

        let found = find_encoding_handler(c"TEST-ENC".as_ptr() as *const xmlChar);
        assert_eq!(found, handler);

        // Remove from registry before freeing to avoid dangling pointers
        {
            let mut handlers = ENCODING_HANDLERS.write();
            handlers.retain(|&h| h.0 != handler);
        }

        unsafe {
            xmlFreeImpl(name as *mut c_void);
            xmlFreeImpl(handler as *mut c_void);
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
        for chunk in utf16be.as_chunks_mut::<2>().0 {
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

    /// Verify the UTF-8 identity callback copies bytes up to the smaller
    /// length.
    ///
    /// # Safety
    ///
    /// - `output` is a valid mutable 64-byte buffer and `input` a valid byte
    ///   slice; the callback writes at most the minimum of the two lengths.
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

    /// Verify a UTF-16LE output/input callback round-trip.
    ///
    /// # Safety
    ///
    /// - The `utf16_buf` and `decoded` arrays are valid buffers of the given
    ///   lengths, and the input slices are valid; the callbacks write only
    ///   up to the advertised output length.
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

    /// Verify `append_to_xml_buffer` grows the buffer and copies bytes.
    ///
    /// # Safety
    ///
    /// - `content` is a valid 64-byte allocation owned by the test and freed
    ///   exactly once with `xmlFreeImpl`; `buf` keeps consistent `use_` and
    ///   `size` fields while `append_to_xml_buffer` may reallocate `content`.
    #[test]
    fn test_append_to_xml_buffer() {
        unsafe {
            let content = xmlMallocImpl(64) as *mut xmlChar;
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

            xmlFreeImpl(buf.content as *mut c_void);
        }
    }

    // ── ABI export functions ───────────────────────────────────────────────

    #[test]
    fn test_xml_parse_char_encoding() {
        let name = c"UTF-8".as_ptr() as *const c_char;
        assert_eq!(
            xmlParseCharEncoding(name),
            xmlCharEncoding::XML_CHAR_ENCODING_UTF8 as c_int
        );

        let name = c"ISO-8859-1".as_ptr() as *const c_char;
        assert_eq!(
            xmlParseCharEncoding(name),
            xmlCharEncoding::XML_CHAR_ENCODING_8859_1 as c_int
        );

        assert_eq!(
            xmlParseCharEncoding(ptr::null()),
            xmlCharEncoding::XML_CHAR_ENCODING_NONE as c_int
        );
    }

    /// Verify `xmlNewCharEncodingHandler` and `xmlDelEncodingHandler`
    /// round-trip.
    ///
    /// # Safety
    ///
    /// - `name` is a valid NUL-terminated string; the returned handler is
    ///   non-NULL, its `name` field is a valid NUL-terminated string, and it
    ///   is freed exactly once by `xmlDelEncodingHandler`.
    #[test]
    fn test_xml_new_and_del_encoding_handler() {
        let name = c"TestEnc".as_ptr() as *const c_char;
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

        let name: *const xmlChar = c"UTF-8".as_ptr() as *const xmlChar;
        assert!(!find_encoding_handler(name).is_null());

        xmlCleanupCharEncodingHandlers();
        // After cleanup, handlers should be empty
    }

    // ── Shift_JIS / EUC-JP (encoding_rs-backed, R-000157 slice) ────────────

    /// Drive a legacy func on whole buffers.
    fn call_func(
        func: unsafe extern "C" fn(*mut c_uchar, *mut c_int, *const c_uchar, *mut c_int) -> c_int,
        input: &[u8],
    ) -> (c_int, Vec<u8>, usize) {
        let mut out = vec![0u8; input.len() * 6 + 64];
        let mut outlen = out.len() as c_int;
        let mut inlen = input.len() as c_int;
        let rc = unsafe { func(out.as_mut_ptr(), &mut outlen, input.as_ptr(), &mut inlen) };
        out.truncate(outlen.max(0) as usize);
        (rc, out, inlen.max(0) as usize)
    }

    #[test]
    fn test_shift_jis_output_roundtrip() {
        // ぁ (U+3041 → 0x82 0x9F), 漢 (U+6F22 → 0x8A 0xBF), half-width ｱ
        // (U+FF71 → 0xB1): byte-exact vs the oracle's iconv output.
        let (rc, out, consumed) = call_func(shift_jis_output_func, "ぁ漢ｱ".as_bytes());
        assert!(rc >= 0);
        assert_eq!(out, [0x82, 0x9F, 0x8A, 0xBF, 0xB1]);
        assert_eq!(consumed, "ぁ漢ｱ".len());

        let (rc, back, _) = call_func(shift_jis_input_func, &out);
        assert!(rc >= 0);
        assert_eq!(back, "ぁ漢ｱ".as_bytes());
    }

    #[test]
    fn test_shift_jis_output_unmappable_reports_input_error() {
        // U+1F600 is outside Shift_JIS: the func stops BEFORE it with the
        // -2 input-error convention (char_enc_out substitutes the decimal
        // character reference, exactly like the oracle iconv EILSEQ path).
        let (rc, out, consumed) = call_func(shift_jis_output_func, "A😀B".as_bytes());
        assert_eq!(rc, ENC_INPUT_ERROR);
        assert_eq!(out, b"A");
        assert_eq!(consumed, 1); // *inlen points AT the emoji

        // Whole-buffer conversion through char_enc_out emits the charref and
        // continues: &#128512; (decimal), matching xmlSerializeDecCharRef.
        let handler = find_encoding_handler(c"SHIFT_JIS".as_ptr() as *const xmlChar);
        assert!(!handler.is_null());
        let in_buf = crate::xml::io::buf_create(64);
        let src = "A\u{1F600}B".as_bytes();
        assert!(
            crate::xml::io::buf_add(in_buf, src.as_ptr() as *const xmlChar, src.len() as c_int)
                >= 0
        );
        let out_buf = crate::xml::io::buf_create(64);
        let n = char_enc_out(handler, out_buf, in_buf);
        assert!(n >= 0);
        let bytes =
            unsafe { core::slice::from_raw_parts((*out_buf).content, (*out_buf).use_ as usize) };
        assert_eq!(bytes, b"A&#128512;B");
        crate::xml::io::buf_free(in_buf);
        crate::xml::io::buf_free(out_buf);
    }

    #[test]
    fn test_euc_jp_output_roundtrip() {
        // ぁ (U+3041 → 0xA4 0xA1), 漢 (U+6F22 → 0xB4 0xC1), ｱ (U+FF71 →
        // 0x8E 0xB1) — oracle iconv byte-exact.
        let (rc, out, consumed) = call_func(euc_jp_output_func, "ぁ漢ｱ".as_bytes());
        assert!(rc >= 0);
        assert_eq!(out, [0xA4, 0xA1, 0xB4, 0xC1, 0x8E, 0xB1]);
        assert_eq!(consumed, "ぁ漢ｱ".len());

        let (rc, back, _) = call_func(euc_jp_input_func, &out);
        assert!(rc >= 0);
        assert_eq!(back, "ぁ漢ｱ".as_bytes());
    }

    #[test]
    fn test_east_asian_handlers_registered_and_findable() {
        for name in [
            c"SHIFT_JIS".as_ptr(),
            c"Shift_JIS".as_ptr(),
            c"SJIS".as_ptr(),
            c"CP932".as_ptr(),
            c"EUC-JP".as_ptr(),
            c"euc-jp".as_ptr(),
        ] {
            assert!(
                !find_encoding_handler(name as *const xmlChar).is_null(),
                "handler not found for {name:?}"
            );
        }
    }

    #[test]
    fn test_shift_jis_output_invalid_utf8_errors() {
        let (rc, out, consumed) = call_func(shift_jis_output_func, b"A\xFFB");
        assert_eq!(rc, -1);
        assert_eq!(out, b"A");
        assert_eq!(consumed, 1);
    }

    // ── R-000157 remainder codecs (UCS-4/UCS-2/EBCDIC/ISO-8859-x) ─────────

    #[test]
    fn test_ucs4le_output_matches_utf32le() {
        // あ U+3042 → 42 30 00 00 little-endian; 中 U+4E2D → 2D 4E 00 00.
        let (rc, out, consumed) = call_func(ucs4le_output_func, "Aあ中".as_bytes());
        assert!(rc >= 0);
        assert_eq!(out, [0x41, 0, 0, 0, 0x42, 0x30, 0, 0, 0x2D, 0x4E, 0, 0]);
        assert_eq!(consumed, "Aあ中".len());
        let (rc, back, _) = call_func(ucs4le_input_func, &out);
        assert!(rc >= 0);
        assert_eq!(back, "Aあ中".as_bytes());
    }

    #[test]
    fn test_ucs4be_output_matches_utf32be() {
        let (rc, out, _) = call_func(ucs4be_output_func, "Aあ".as_bytes());
        assert!(rc >= 0);
        assert_eq!(out, [0, 0, 0, 0x41, 0, 0, 0x30, 0x42]);
        let (rc, back, _) = call_func(ucs4be_input_func, &out);
        assert!(rc >= 0);
        assert_eq!(back, "Aあ".as_bytes());
    }

    #[test]
    fn test_ucs2_output_astral_is_unmappable() {
        // glibc "UCS-2" on x86 is little-endian; astral chars are unmappable
        // (the -2 input-error convention -> decimal charref).
        let (rc, out, consumed) = call_func(ucs2_output_func, "A😀".as_bytes());
        assert_eq!(rc, ENC_INPUT_ERROR);
        assert_eq!(out, [0x41, 0]);
        assert_eq!(consumed, 1);
    }

    #[test]
    fn test_ebcdic037_bijection() {
        // cp037: space 0x40, 'A' 0xC1, '0' 0xF0.
        let (rc, out, _) = call_func(ebcdic_output_func, b"A 0");
        assert!(rc >= 0);
        assert_eq!(out, [0xC1, 0x40, 0xF0]);
        let (rc, back, _) = call_func(ebcdic_input_func, &out);
        assert!(rc >= 0);
        assert_eq!(back, b"A 0");
        // Every byte is defined and the mapping is a bijection onto
        // U+0000..U+00FF (derived from the oracle glibc iconv IBM037).
        for (b, cp) in EBCDIC037_TO_UNICODE.iter().enumerate() {
            assert_eq!(ebcdic037_cp_to_byte(u32::from(*cp)), Some(b as u8));
        }
        assert!(ebcdic037_cp_to_byte(0x100).is_none());
    }

    #[test]
    fn test_iso_8859_2_output_roundtrip() {
        // ą U+0105 → 0xB1, ć U+0107 → 0xE6, ę U+0119 → 0xEA (ISO-8859-2).
        let (rc, out, _) = call_func(iso_8859_2_output_func, "Aąćę".as_bytes());
        assert!(rc >= 0);
        assert_eq!(out, [0x41, 0xB1, 0xE6, 0xEA]);
        let (rc, back, _) = call_func(iso_8859_2_input_func, &out);
        assert!(rc >= 0);
        assert_eq!(back, "Aąćę".as_bytes());
    }

    #[test]
    fn test_decode_whole_buffer_declared_dispatch() {
        // Whole-buffer decode via the registry (parser input layer path).
        let iso2 = [0x41u8, 0xB1, 0xE6, 0xEA];
        assert_eq!(
            decode_whole_buffer_declared(b"ISO-8859-2", &iso2).unwrap(),
            "Aąćę".as_bytes()
        );
        // Alias spelling resolves through the canonical re-lookup.
        assert_eq!(
            decode_whole_buffer_declared(b"latin2", &iso2).unwrap(),
            "Aąćę".as_bytes()
        );
        // Unknown names error (no handler).
        assert!(decode_whole_buffer_declared(b"no-such-encoding", b"abc").is_err());
    }

    #[test]
    fn test_iso_2022_jp_output_uses_escape_sequences() {
        // A → ASCII; あ U+3042 → ESC $ B + 0x24 0x22; back to ASCII ESC ( B.
        let (rc, out, consumed) = call_func(iso_2022_jp_output_func, "AあB".as_bytes());
        assert!(rc >= 0);
        assert_eq!(out, b"A\x1B$B$\"\x1B(BB");
        assert_eq!(consumed, "AあB".len());
        let (rc, back, _) = call_func(iso_2022_jp_input_func, &out);
        assert!(rc >= 0);
        assert_eq!(back, "AあB".as_bytes());
    }
}
