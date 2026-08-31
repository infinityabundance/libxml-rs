//! Parser input buffer management — safe Rust wrapper (§19.3, §20.4).
//!
//! This module provides internal safe-Rust abstractions over XML input sources:
//! memory buffers, files, and custom I/O callbacks. It handles character-level
//! reading with position tracking (line/column/byte-offset), BOM detection,
//! encoding detection from XML declarations, and entity-expansion input stacks.
//!
//! # Architecture
//!
//! ```text
//! C ABI (_xmlParserInput, _xmlParserInputBuffer)
//!         ↕  populate / read from
//! InputBuffer (safe internal representation)
//!         ↕  stack management
//! InputStack (entity expansion nesting)
//! ```
//!
//! The safe types (`InputBuffer`, `InputStack`, `InputSource`) are NOT `#[repr(C)]`;
//! they are implementation details. C ABI structs are only populated when crossing
//! the FFI boundary.

#![allow(dead_code)]

use crate::abi::callbacks::{xmlInputCloseCallback, xmlInputReadCallback};
use crate::abi::structs::{_xmlParserInput, _xmlParserInputBuffer};
use crate::abi::types::xmlCharEncoding;
use std::fs;
use std::io::Read;
use std::os::raw::{c_char, c_int, c_ulong, c_void};
use std::path::Path;

// ═══════════════════════════════════════════════════════════════════════════════
// Encoding
// ═══════════════════════════════════════════════════════════════════════════════

/// Internal encoding representation (not `#[repr(C)]`).
///
/// Maps to [`xmlCharEncoding`] for FFI conversion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Encoding {
    /// No encoding detected / unknown.
    None,
    /// UTF-8.
    Utf8,
    /// UTF-16 little-endian.
    Utf16Le,
    /// UTF-16 big-endian.
    Utf16Be,
    /// US-ASCII.
    Ascii,
    /// ISO-8859-1 (Latin-1).
    Iso8859_1,
    /// Other encoding (name stored for reference).
    Other(String),
}

impl Encoding {
    /// Convert to the C ABI [`xmlCharEncoding`] value.
    pub(crate) fn to_xml_char_encoding(&self) -> xmlCharEncoding {
        match self {
            Self::None => xmlCharEncoding::XML_CHAR_ENCODING_NONE,
            Self::Utf8 => xmlCharEncoding::XML_CHAR_ENCODING_UTF8,
            Self::Utf16Le => xmlCharEncoding::XML_CHAR_ENCODING_UTF16LE,
            Self::Utf16Be => xmlCharEncoding::XML_CHAR_ENCODING_UTF16BE,
            Self::Ascii => xmlCharEncoding::XML_CHAR_ENCODING_ASCII,
            Self::Iso8859_1 => xmlCharEncoding::XML_CHAR_ENCODING_8859_1,
            Self::Other(_) => xmlCharEncoding::XML_CHAR_ENCODING_ERROR,
        }
    }

    /// Parse an encoding name from an XML declaration.
    fn from_name(name: &str) -> Self {
        match name.trim().to_ascii_lowercase().as_str() {
            "utf-8" | "utf8" => Encoding::Utf8,
            "utf-16" | "utf16" => Encoding::Utf16Le, // default LE when no BOM
            "utf-16le" | "utf16le" => Encoding::Utf16Le,
            "utf-16be" | "utf16be" => Encoding::Utf16Be,
            "us-ascii" | "ascii" => Encoding::Ascii,
            "iso-8859-1" | "iso8859-1" | "latin1" | "latin-1" => Encoding::Iso8859_1,
            other => Encoding::Other(other.to_string()),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// InputSource
// ═══════════════════════════════════════════════════════════════════════════════

/// Internal abstraction over an XML input source.
///
/// This is NOT a `#[repr(C)]` type; it is a safe Rust implementation detail.
#[derive(Debug)]
pub(crate) enum InputSource {
    /// Data from an in-memory byte slice (copied into owned storage).
    Memory(Vec<u8>),
    /// Data from a file on disk.
    File {
        /// The file path.
        path: String,
        /// The open file handle.
        file: fs::File,
    },
    /// Data from custom I/O callbacks.
    Callback {
        /// Read callback — reads bytes into a buffer.
        read: xmlInputReadCallback,
        /// Close callback — called when the input is closed.
        close: xmlInputCloseCallback,
        /// Opaque context pointer passed to both callbacks.
        ctx: *mut c_void,
    },
}

impl InputSource {
    /// Read raw bytes from the source into a `Vec<u8>`.
    ///
    /// For memory sources this is a clone of the underlying data.
    /// For file sources this reads the entire file.
    /// For callback sources this reads incrementally until EOF.
    fn read_all(&mut self) -> Result<Vec<u8>, InputError> {
        match self {
            InputSource::Memory(data) => Ok(data.clone()),
            InputSource::File { file, .. } => {
                let mut buf = Vec::new();
                file.read_to_end(&mut buf)
                    .map_err(|e| InputError::Io(e.to_string()))?;
                Ok(buf)
            }
            InputSource::Callback { read, ctx, .. } => {
                let mut buf = Vec::new();
                let mut tmp = [0u8; 4096];
                loop {
                    // SAFETY: The caller guarantees that `read` is a valid function pointer
                    // and `ctx` is a valid pointer. The callback writes up to `tmp.len()`
                    // bytes into `tmp`. We trust the callback to not overrun the buffer.
                    let n =
                        unsafe { read(*ctx, tmp.as_mut_ptr() as *mut c_char, tmp.len() as c_int) };
                    if n < 0 {
                        return Err(InputError::Callback("read callback returned error".into()));
                    }
                    if n == 0 {
                        break; // EOF
                    }
                    buf.extend_from_slice(&tmp[..n as usize]);
                }
                Ok(buf)
            }
        }
    }

    /// Read a chunk of bytes into a buffer. Returns the number of bytes read.
    fn read_chunk(&mut self, buf: &mut [u8]) -> Result<usize, InputError> {
        match self {
            InputSource::Memory(_data) => {
                // Memory reads are handled by the buffer directly.
                Ok(0)
            }
            InputSource::File { file, .. } => {
                file.read(buf).map_err(|e| InputError::Io(e.to_string()))
            }
            InputSource::Callback { read, ctx, .. } => {
                // SAFETY: Same as in `read_all`.
                let n = unsafe { read(*ctx, buf.as_mut_ptr() as *mut c_char, buf.len() as c_int) };
                if n < 0 {
                    Err(InputError::Callback("read callback returned error".into()))
                } else {
                    Ok(n as usize)
                }
            }
        }
    }

    /// Get the filename/URI if available.
    fn filename(&self) -> Option<&str> {
        match self {
            InputSource::File { path, .. } => Some(path.as_str()),
            _ => None,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// InputError
// ═══════════════════════════════════════════════════════════════════════════════

/// Errors that can occur during input operations.
#[derive(Debug)]
pub(crate) enum InputError {
    /// I/O error (file not found, permission denied, etc.).
    Io(String),
    /// Callback returned an error.
    Callback(String),
    /// Invalid UTF-8 sequence encountered.
    InvalidUtf8,
    /// Unexpected end of input.
    UnexpectedEof,
    /// Empty input.
    EmptyInput,
}

impl std::fmt::Display for InputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InputError::Io(msg) => write!(f, "I/O error: {msg}"),
            InputError::Callback(msg) => write!(f, "callback error: {msg}"),
            InputError::InvalidUtf8 => write!(f, "invalid UTF-8 sequence"),
            InputError::UnexpectedEof => write!(f, "unexpected end of input"),
            InputError::EmptyInput => write!(f, "empty input"),
        }
    }
}

impl std::error::Error for InputError {}

// ═══════════════════════════════════════════════════════════════════════════════
// InputBuffer
// ═══════════════════════════════════════════════════════════════════════════════

/// Internal safe representation of an XML input source with position tracking.
///
/// This type is NOT `#[repr(C)]`. It is an internal implementation detail that
/// wraps raw input sources and provides character-level reading, line/column
/// tracking, BOM detection, and encoding detection.
///
/// # Position tracking
///
/// - Line numbers are 1-based (first line is line 1).
/// - Column numbers are 1-based (first column is col 1).
/// - Byte offset is 0-based from the start of the input.
/// - Both `\n` (LF) and `\r` (CR) increment the line counter.
/// - `\r\n` (CRLF) is treated as a single line break.
pub(crate) struct InputBuffer {
    /// The raw input source.
    source: InputSource,
    /// The complete buffered data (after any encoding conversion to UTF-8).
    data: Vec<u8>,
    /// Current byte position in `data`.
    pos: usize,
    /// Current line number (1-based).
    line: usize,
    /// Current column number (1-based).
    col: usize,
    /// Detected character encoding.
    encoding: Encoding,
    /// Filename or URI, if known.
    filename: Option<String>,
    /// Whether the BOM has been consumed.
    bom_consumed: bool,
}

impl std::fmt::Debug for InputBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InputBuffer")
            .field("source", &self.source)
            .field("pos", &self.pos)
            .field("line", &self.line)
            .field("col", &self.col)
            .field("encoding", &self.encoding)
            .field("filename", &self.filename)
            .field("len", &self.data.len())
            .field("bom_consumed", &self.bom_consumed)
            .finish()
    }
}

impl InputBuffer {
    // ── Constructors ───────────────────────────────────────────────────────

    /// Create an `InputBuffer` from an in-memory byte slice.
    ///
    /// The bytes are copied into an owned buffer. BOM and encoding detection
    /// are performed during construction.
    pub fn from_memory(buf: &[u8], uri: Option<&str>) -> Self {
        let data = buf.to_vec();
        let filename = uri.map(|s| s.to_string());
        let mut ib = InputBuffer {
            source: InputSource::Memory(data.clone()),
            data,
            pos: 0,
            line: 1,
            col: 1,
            encoding: Encoding::None,
            filename,
            bom_consumed: false,
        };
        ib.detect_bom_and_encoding();
        ib
    }

    /// Create an `InputBuffer` from a file on disk.
    ///
    /// Returns `Err` if the file cannot be opened or read.
    pub fn from_file(path: &str) -> Result<Self, InputError> {
        let p = Path::new(path);
        let file = fs::File::open(p).map_err(|e| InputError::Io(e.to_string()))?;
        let filename = Some(path.to_string());

        let mut source = InputSource::File {
            path: path.to_string(),
            file,
        };
        let data = source.read_all()?;

        let mut ib = InputBuffer {
            source,
            data,
            pos: 0,
            line: 1,
            col: 1,
            encoding: Encoding::None,
            filename,
            bom_consumed: false,
        };
        ib.detect_bom_and_encoding();
        Ok(ib)
    }

    /// Create an `InputBuffer` from custom I/O callbacks.
    ///
    /// The callbacks are used to read all available data from the source.
    pub fn from_callback(
        read: xmlInputReadCallback,
        close: xmlInputCloseCallback,
        ctx: *mut c_void,
    ) -> Result<Self, InputError> {
        let mut source = InputSource::Callback { read, close, ctx };
        let data = source.read_all()?;

        let mut ib = InputBuffer {
            source,
            data,
            pos: 0,
            line: 1,
            col: 1,
            encoding: Encoding::None,
            filename: None,
            bom_consumed: false,
        };
        ib.detect_bom_and_encoding();
        Ok(ib)
    }

    // ── BOM and encoding detection ─────────────────────────────────────────

    /// Detect BOM and encoding from the beginning of the data.
    ///
    /// This checks:
    /// 1. UTF-8 BOM (`EF BB BF`)
    /// 2. UTF-16 LE BOM (`FF FE`)
    /// 3. UTF-16 BE BOM (`FE FF`)
    /// 4. XML declaration (`<?xml encoding="..."?>`)
    ///
    /// The BOM is consumed (position advanced past it) so that subsequent
    /// reads start after the BOM.
    fn detect_bom_and_encoding(&mut self) {
        if self.data.is_empty() {
            self.encoding = Encoding::Utf8;
            return;
        }

        // Check for UTF-8 BOM: EF BB BF
        if self.data.len() >= 3
            && self.data[0] == 0xEF
            && self.data[1] == 0xBB
            && self.data[2] == 0xBF
        {
            self.encoding = Encoding::Utf8;
            self.pos = 3;
            self.col = 4; // BOM occupies columns 1-3
            self.bom_consumed = true;
            // After consuming BOM, check for XML declaration
            self.detect_encoding_from_xml_declaration();
            return;
        }

        // Check for UTF-16 LE BOM: FF FE
        if self.data.len() >= 2 && self.data[0] == 0xFF && self.data[1] == 0xFE {
            self.encoding = Encoding::Utf16Le;
            self.pos = 2;
            self.col = 3;
            self.bom_consumed = true;
            // We don't try to decode UTF-16 yet; mark and continue
            return;
        }

        // Check for UTF-16 BE BOM: FE FF
        if self.data.len() >= 2 && self.data[0] == 0xFE && self.data[1] == 0xFF {
            self.encoding = Encoding::Utf16Be;
            self.pos = 2;
            self.col = 3;
            self.bom_consumed = true;
            return;
        }

        // No BOM found. Default to UTF-8 and check for XML declaration.
        self.encoding = Encoding::Utf8;
        self.detect_encoding_from_xml_declaration();
    }

    /// Try to detect encoding from an XML declaration at the start of the input.
    ///
    /// Looks for `<?xml ... encoding="..."?>` or `<?xml ... encoding='...'?>`
    /// after any BOM has been consumed.
    fn detect_encoding_from_xml_declaration(&mut self) {
        let remaining = &self.data[self.pos..];

        // Must start with "<?xml"
        if remaining.len() < 5 {
            return;
        }
        if &remaining[..5] != b"<?xml" {
            return;
        }

        // Find the end of the PI: "?>"
        let pi_end = remaining.windows(2).position(|w| w == b"?>");
        let pi_end = match pi_end {
            Some(e) => e + 2,
            None => return, // Malformed PI, ignore
        };

        // Look for 'encoding' in the PI
        let pi_content = &remaining[..pi_end];
        let pi_str = match std::str::from_utf8(pi_content) {
            Ok(s) => s,
            Err(_) => return,
        };

        // Find encoding="..." or encoding='...'
        if let Some(enc) = Self::extract_encoding_from_pi(pi_str) {
            self.encoding = Encoding::from_name(&enc);
        }
    }

    /// Extract the encoding name from an XML processing instruction.
    ///
    /// Handles both single and double quotes around the encoding value.
    fn extract_encoding_from_pi(pi: &str) -> Option<String> {
        // Find "encoding" keyword
        let pi_lower = pi.to_ascii_lowercase();
        let kw_pos = pi_lower.find("encoding")?;

        let after_kw = &pi[kw_pos + 8..]; // skip past "encoding"
        let after_kw = after_kw.trim_start();

        // Must be followed by '='
        if !after_kw.starts_with('=') {
            return None;
        }
        let after_eq = after_kw[1..].trim_start();

        // Check for quote character
        let quote = after_eq.chars().next()?;
        if quote != '"' && quote != '\'' {
            return None;
        }

        // Find the closing quote
        let value_start = 1; // skip opening quote
        let value_end = after_eq[value_start..].find(quote)? + value_start;

        Some(after_eq[value_start..value_end].to_string())
    }

    // ── Character reading ──────────────────────────────────────────────────

    /// Read and return the next UTF-8 character, advancing the position.
    ///
    /// Returns `None` at EOF.
    ///
    /// Updates line/column tracking:
    /// - `\n` (LF) increments line, resets col.
    /// - `\r` (CR) increments line, resets col.
    /// - `\r\n` (CRLF) counts as one line break.
    /// - Tab advances col by 1 (libxml2 behavior).
    pub fn read_char(&mut self) -> Option<char> {
        let c = self.peek_char_inner()?;
        self.advance_past_char(c);
        Some(c)
    }

    /// Return the next UTF-8 character without advancing.
    ///
    /// Returns `None` at EOF.
    pub fn peek_char(&self) -> Option<char> {
        self.peek_char_inner()
    }

    /// Peek the raw next byte without decoding (None at EOF). Used to
    /// detect invalid UTF-8 for upstream-compatible encoding errors.
    pub fn peek_raw(&self) -> Option<u8> {
        if self.pos >= self.data.len() {
            None
        } else {
            Some(self.data[self.pos])
        }
    }

    /// Skip `n` raw bytes without decoding (used to step past invalid
    /// UTF-8 bytes; those are never line breaks, so each skipped byte
    /// advances the column by 1 like upstream `NEXTL(1)`).
    pub fn skip_raw_bytes(&mut self, n: usize) {
        for _ in 0..n {
            if self.pos >= self.data.len() {
                break;
            }
            self.pos += 1;
            self.col += 1;
        }
    }

    /// Internal peek implementation.
    fn peek_char_inner(&self) -> Option<char> {
        if self.pos >= self.data.len() {
            return None;
        }

        let remaining = &self.data[self.pos..];
        Self::decode_utf8_char(remaining)
    }

    /// Decode a single UTF-8 character from the beginning of a byte slice.
    ///
    /// Returns `None` if the slice is empty or starts with an invalid sequence.
    fn decode_utf8_char(bytes: &[u8]) -> Option<char> {
        if bytes.is_empty() {
            return None;
        }

        let byte = bytes[0];
        let (code_point, _len) = if byte & 0x80 == 0 {
            // 1-byte sequence: 0xxxxxxx
            (u32::from(byte), 1)
        } else if byte & 0xE0 == 0xC0 {
            // 2-byte sequence: 110xxxxx 10xxxxxx
            if bytes.len() < 2 {
                return None;
            }
            let b1 = u32::from(bytes[1]);
            if b1 & 0xC0 != 0x80 {
                return None;
            }
            ((u32::from(byte & 0x1F) << 6) | (b1 & 0x3F), 2)
        } else if byte & 0xF0 == 0xE0 {
            // 3-byte sequence: 1110xxxx 10xxxxxx 10xxxxxx
            if bytes.len() < 3 {
                return None;
            }
            let b1 = u32::from(bytes[1]);
            let b2 = u32::from(bytes[2]);
            if b1 & 0xC0 != 0x80 || b2 & 0xC0 != 0x80 {
                return None;
            }
            (
                (u32::from(byte & 0x0F) << 12) | ((b1 & 0x3F) << 6) | (b2 & 0x3F),
                3,
            )
        } else if byte & 0xF8 == 0xF0 {
            // 4-byte sequence: 11110xxx 10xxxxxx 10xxxxxx 10xxxxxx
            if bytes.len() < 4 {
                return None;
            }
            let b1 = u32::from(bytes[1]);
            let b2 = u32::from(bytes[2]);
            let b3 = u32::from(bytes[3]);
            if b1 & 0xC0 != 0x80 || b2 & 0xC0 != 0x80 || b3 & 0xC0 != 0x80 {
                return None;
            }
            (
                (u32::from(byte & 0x07) << 18)
                    | ((b1 & 0x3F) << 12)
                    | ((b2 & 0x3F) << 6)
                    | (b3 & 0x3F),
                4,
            )
        } else {
            // Invalid leading byte
            return None;
        };

        char::from_u32(code_point)
    }

    /// Get the byte length of the UTF-8 character starting at the current position.
    fn char_len(&self) -> usize {
        if self.pos >= self.data.len() {
            return 0;
        }
        Self::utf8_char_len(self.data[self.pos])
    }

    /// Determine the byte length of a UTF-8 character from its leading byte.
    fn utf8_char_len(leading: u8) -> usize {
        if leading & 0x80 == 0 {
            1
        } else if leading & 0xE0 == 0xC0 {
            2
        } else if leading & 0xF0 == 0xE0 {
            3
        } else if leading & 0xF8 == 0xF0 {
            4
        } else {
            // Invalid leading byte; treat as 1 byte to avoid stalling
            1
        }
    }

    /// Advance position past a character, updating line/col tracking.
    fn advance_past_char(&mut self, c: char) {
        let byte_len = self.char_len();
        let old_pos = self.pos;
        self.pos += byte_len;

        // Track the bytes we're advancing over for \r\n detection
        if byte_len == 1 && self.data[old_pos] == b'\n' {
            // LF: new line
            self.line += 1;
            self.col = 1;
        } else if byte_len == 1 && self.data[old_pos] == b'\r' {
            // CR: check for CRLF
            if self.pos < self.data.len() && self.data[self.pos] == b'\n' {
                // CRLF: consume the LF too and count as one line break
                self.pos += 1;
            }
            self.line += 1;
            self.col = 1;
        } else if c == '\t' {
            // Tab: advance col (libxml2 treats tab as single column)
            self.col += 1;
        } else {
            // Regular character
            self.col += 1;
        }
    }

    // ── Bulk reading ───────────────────────────────────────────────────────

    /// Read a string of up to `max_chars` characters.
    ///
    /// Returns the string read, which may be shorter than `max_chars` if EOF
    /// is encountered. Line/column tracking is updated for each character.
    pub fn read_string(&mut self, max_chars: usize) -> String {
        let mut s = String::with_capacity(max_chars.min(256));
        for _ in 0..max_chars {
            match self.read_char() {
                Some(c) => s.push(c),
                None => break,
            }
        }
        s
    }

    /// Read all remaining characters as a string.
    pub fn read_all_chars(&mut self) -> String {
        let mut s = String::new();
        while let Some(c) = self.read_char() {
            s.push(c);
        }
        s
    }

    // ── Skip ───────────────────────────────────────────────────────────────

    /// Skip `n` bytes forward (not characters).
    ///
    /// This is a byte-level skip that does not attempt character decoding.
    /// Line/column counts are updated based on the bytes skipped.
    ///
    /// Returns the number of bytes actually skipped (may be less than `n`
    /// if EOF is reached).
    pub fn skip(&mut self, n: usize) -> usize {
        let end = self.pos.saturating_add(n).min(self.data.len());
        let skipped = end - self.pos;

        // Update line/col based on bytes in the skipped range
        for &byte in &self.data[self.pos..end] {
            match byte {
                b'\n' => {
                    self.line += 1;
                    self.col = 1;
                }
                b'\r' => {
                    self.line += 1;
                    self.col = 1;
                }
                _ => {
                    self.col += 1;
                }
            }
        }

        self.pos = end;
        skipped
    }

    // ── Position queries ───────────────────────────────────────────────────

    /// Return the current position as `(line, col, byte_offset)`.
    ///
    /// Line and column are 1-based. Byte offset is 0-based.
    pub fn pos(&self) -> (usize, usize, usize) {
        (self.line, self.col, self.pos)
    }

    /// Check if the input has been fully consumed.
    pub fn is_eof(&self) -> bool {
        self.pos >= self.data.len()
    }

    /// Return the remaining unconsumed bytes.
    pub fn remaining(&self) -> &[u8] {
        &self.data[self.pos..]
    }

    /// Return the bytes consumed so far.
    pub fn consumed(&self) -> &[u8] {
        &self.data[..self.pos]
    }

    /// Return the total length of the buffered data in bytes.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Return the filename or URI, if known.
    pub fn filename(&self) -> Option<&str> {
        self.filename.as_deref()
    }

    /// Return the detected encoding.
    pub fn encoding(&self) -> &Encoding {
        &self.encoding
    }

    /// Return whether a BOM was detected and consumed.
    pub fn bom_was_consumed(&self) -> bool {
        self.bom_consumed
    }

    // ── C ABI integration ──────────────────────────────────────────────────

    /// Populate a `_xmlParserInput` struct with the current state of this buffer.
    ///
    /// The caller must ensure the raw pointers remain valid for the lifetime
    /// of the `_xmlParserInput` struct. This typically means the `InputBuffer`
    /// must not be dropped or reborrowed while the C struct is in use.
    ///
    /// # Safety
    ///
    /// The caller must ensure that:
    /// - The returned `_xmlParserInput` is not used after the `InputBuffer` is
    ///   dropped or mutably borrowed.
    /// - The `base`, `cur`, and `end` pointers point into stable storage.
    pub unsafe fn populate_parser_input(&self, input: &mut _xmlParserInput) {
        let data_ptr = self.data.as_ptr();
        let base = data_ptr as *const crate::abi::types::xmlChar;
        let cur = unsafe { data_ptr.add(self.pos) as *const crate::abi::types::xmlChar };
        let end = unsafe { data_ptr.add(self.data.len()) as *const crate::abi::types::xmlChar };

        input.base = base;
        input.cur = cur;
        input.end = end;
        input.line = self.line as c_int;
        input.col = self.col as c_int;
        input.length = self.data.len() as c_int;
        input.consumed = self.pos as c_ulong;
        input.filename = self
            .filename
            .as_ref()
            .map(|s| s.as_ptr() as *const c_char)
            .unwrap_or(std::ptr::null());
    }

    /// Like [`populate_parser_input`](Self::populate_parser_input) but leaves
    /// `filename` untouched: the caller owns a duplicated C string instead of
    /// borrowing the Rust-side filename (which the parser moves/drops).
    pub unsafe fn populate_parser_input_without_filename(&self, input: &mut _xmlParserInput) {
        let data_ptr = self.data.as_ptr();
        let base = data_ptr as *const crate::abi::types::xmlChar;
        let cur = unsafe { data_ptr.add(self.pos) as *const crate::abi::types::xmlChar };
        let end = unsafe { data_ptr.add(self.data.len()) as *const crate::abi::types::xmlChar };

        input.base = base;
        input.cur = cur;
        input.end = end;
        input.line = self.line as c_int;
        input.col = self.col as c_int;
        input.length = self.data.len() as c_int;
        input.consumed = self.pos as c_ulong;
    }

    /// Create a `_xmlParserInputBuffer` from this buffer's source.
    ///
    /// # Safety
    ///
    /// The caller must ensure the callback function pointers are valid.
    pub unsafe fn populate_parser_input_buffer(&self, buf: &mut _xmlParserInputBuffer) {
        match &self.source {
            InputSource::Callback { read, close, ctx } => {
                buf.readcallback = Some(*read);
                buf.closecallback = Some(*close);
                buf.context = *ctx;
            }
            _ => {
                buf.readcallback = None;
                buf.closecallback = None;
                buf.context = std::ptr::null_mut();
            }
        }
        buf.encoder = std::ptr::null_mut();
        buf.buffer = std::ptr::null_mut();
        buf.raw = std::ptr::null_mut();
        buf.compressed = 0;
        buf.error = 0;
        buf.rawconsumed = 0;
    }

    // ── Resetting ──────────────────────────────────────────────────────────

    /// Reset the buffer to the beginning of the input.
    ///
    /// This allows re-parsing the same input from the start.
    pub fn reset(&mut self) {
        self.pos = 0;
        self.line = 1;
        self.col = 1;
        self.bom_consumed = false;
        self.detect_bom_and_encoding();
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// InputStack
// ═══════════════════════════════════════════════════════════════════════════════

/// A stack of input buffers, used for entity expansion.
///
/// When the parser encounters an entity reference, it pushes a new `InputBuffer`
/// onto the stack containing the entity's replacement text. When the entity's
/// content is fully consumed, the stack is popped to resume parsing the
/// original input.
///
/// # Invariants
///
/// - The stack always has at least one entry (the base input).
/// - `current` always indexes a valid entry in `inputs`.
pub(crate) struct InputStack {
    /// The stack of input buffers.
    inputs: Vec<InputBuffer>,
    /// Index of the current (top) input.
    current: usize,
}

impl std::fmt::Debug for InputStack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InputStack")
            .field("depth", &self.inputs.len())
            .field("current", &self.current)
            .finish()
    }
}

impl InputStack {
    /// Create a new input stack with the given base input.
    pub fn new(base: InputBuffer) -> Self {
        InputStack {
            inputs: vec![base],
            current: 0,
        }
    }

    /// Push a new input onto the stack.
    ///
    /// This is used when entering an entity expansion.
    pub fn push(&mut self, input: InputBuffer) {
        self.inputs.push(input);
        self.current = self.inputs.len() - 1;
    }

    /// Pop the current input from the stack.
    ///
    /// Returns the popped input, or `None` if the stack would become empty
    /// (i.e., if there is only one input remaining).
    pub fn pop(&mut self) -> Option<InputBuffer> {
        if self.inputs.len() <= 1 {
            // Cannot pop the base input
            return None;
        }
        let popped = self.inputs.pop();
        self.current = self.inputs.len() - 1;
        popped
    }

    /// Get a mutable reference to the current (top) input buffer.
    pub fn current(&mut self) -> &mut InputBuffer {
        // `current` always indexes a valid entry.
        &mut self.inputs[self.current]
    }

    /// Get a shared reference to the current (top) input buffer.
    pub fn current_ref(&self) -> &InputBuffer {
        &self.inputs[self.current]
    }

    /// Get the current position across the entire stack.
    ///
    /// Returns `(line, col, byte_offset)` for the current input.
    pub fn current_pos(&self) -> (usize, usize, usize) {
        self.inputs[self.current].pos()
    }

    /// Returns the depth of the stack (number of nested inputs).
    ///
    /// A stack with only the base input has depth 1.
    pub fn depth(&self) -> usize {
        self.inputs.len()
    }

    /// Check if all inputs on the stack are at EOF.
    ///
    /// Exhausted pushed inputs are popped automatically.
    pub fn is_eof(&mut self) -> bool {
        self.pop_exhausted();
        self.inputs[self.current].is_eof()
    }

    /// Read the next character from the current input.
    ///
    /// Exhausted pushed inputs are popped automatically so the stack
    /// behaves as a single logical input (used for entity expansion).
    pub fn read_char(&mut self) -> Option<char> {
        self.pop_exhausted();
        self.inputs[self.current].read_char()
    }

    /// Peek at the next character from the current input without advancing.
    ///
    /// Exhausted pushed inputs are popped automatically.
    pub fn peek_char(&mut self) -> Option<char> {
        self.pop_exhausted();
        self.inputs[self.current].peek_char()
    }

    /// Peek the raw next byte of the current input without decoding.
    pub fn peek_raw(&mut self) -> Option<u8> {
        self.pop_exhausted();
        self.inputs[self.current].peek_raw()
    }

    /// Skip `n` raw bytes of the current input (invalid UTF-8 handling).
    pub fn skip_raw_bytes(&mut self, n: usize) {
        self.pop_exhausted();
        self.inputs[self.current].skip_raw_bytes(n);
    }

    /// Pop any exhausted pushed inputs so that the current input always has
    /// remaining data (or is the base input).
    fn pop_exhausted(&mut self) {
        while self.inputs.len() > 1 && self.inputs[self.current].is_eof() {
            self.inputs.pop();
            self.current = self.inputs.len() - 1;
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Free-standing helpers for C ABI population
// ═══════════════════════════════════════════════════════════════════════════════

/// Create a `_xmlParserInput` from an `InputBuffer`.
///
/// This allocates a new `_xmlParserInput` on the heap (matching libxml2's
/// allocation pattern). The returned pointer must eventually be freed.
///
/// # Safety
///
/// The returned `_xmlParserInput` contains raw pointers into the `InputBuffer`'s
/// data storage. The caller must ensure the `InputBuffer` outlives the returned
/// struct.
pub(crate) unsafe fn input_buffer_to_parser_input(buf: &InputBuffer) -> *mut _xmlParserInput {
    let input = Box::into_raw(Box::new(_xmlParserInput {
        buf: std::ptr::null_mut(),
        filename: buf
            .filename
            .as_ref()
            .map(|s| s.as_ptr() as *const c_char)
            .unwrap_or(std::ptr::null()),
        directory: std::ptr::null(),
        base: buf.data.as_ptr() as *const crate::abi::types::xmlChar,
        cur: unsafe { buf.data.as_ptr().add(buf.pos) as *const crate::abi::types::xmlChar },
        end: unsafe { buf.data.as_ptr().add(buf.data.len()) as *const crate::abi::types::xmlChar },
        length: buf.data.len() as c_int,
        line: buf.line as c_int,
        col: buf.col as c_int,
        consumed: buf.pos as c_ulong,
        free: None,
        encoding: std::ptr::null(),
        version: std::ptr::null(),
        flags: 0,
        id: 0,
        parentConsumed: 0,
        entity: std::ptr::null_mut(),
    }));
    input
}

/// Create a `_xmlParserInputBuffer` from an `InputBuffer`.
///
/// This allocates a new `_xmlParserInputBuffer` on the heap.
///
/// # Safety
///
/// The caller must ensure callback pointers are valid if the input source
/// uses callbacks.
pub(crate) unsafe fn input_buffer_to_parser_input_buffer(
    buf: &InputBuffer,
) -> *mut _xmlParserInputBuffer {
    let mut raw_buf = Box::new(_xmlParserInputBuffer {
        context: std::ptr::null_mut(),
        readcallback: None,
        closecallback: None,
        encoder: std::ptr::null_mut(),
        buffer: std::ptr::null_mut(),
        raw: std::ptr::null_mut(),
        compressed: 0,
        error: 0,
        rawconsumed: 0,
    });

    unsafe {
        buf.populate_parser_input_buffer(&mut raw_buf);
    }

    Box::into_raw(raw_buf)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ── Basic construction ─────────────────────────────────────────────────

    #[test]
    fn test_from_memory_empty() {
        let buf = InputBuffer::from_memory(b"", None);
        assert!(buf.is_eof());
        assert_eq!(buf.pos(), (1, 1, 0));
        assert_eq!(buf.encoding(), &Encoding::Utf8);
    }

    #[test]
    fn test_from_memory_basic() {
        let buf = InputBuffer::from_memory(b"hello", None);
        assert!(!buf.is_eof());
        assert_eq!(buf.len(), 5);
        assert_eq!(buf.remaining(), b"hello");
        assert!(buf.consumed().is_empty());
    }

    #[test]
    fn test_from_memory_with_uri() {
        let buf = InputBuffer::from_memory(b"test", Some("http://example.com"));
        assert_eq!(buf.filename(), Some("http://example.com"));
    }

    #[test]
    fn test_from_memory_with_encoding_declaration() {
        let buf =
            InputBuffer::from_memory(b"<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?>", None);
        assert_eq!(buf.encoding(), &Encoding::Iso8859_1);
    }

    // ── BOM detection ──────────────────────────────────────────────────────

    #[test]
    fn test_utf8_bom_detection() {
        // UTF-8 BOM: EF BB BF followed by "hello"
        let mut data = vec![0xEF, 0xBB, 0xBF];
        data.extend_from_slice(b"hello");
        let buf = InputBuffer::from_memory(&data, None);
        assert!(buf.bom_was_consumed());
        // BOM consumes 3 bytes
        assert_eq!(buf.pos, 3);
        assert_eq!(buf.col, 4);
        assert_eq!(buf.remaining(), b"hello");
    }

    #[test]
    fn test_utf16le_bom_detection() {
        let data = vec![0xFF, 0xFE, b'h', 0x00, b'i', 0x00];
        let buf = InputBuffer::from_memory(&data, None);
        assert!(buf.bom_was_consumed());
        assert_eq!(buf.encoding(), &Encoding::Utf16Le);
        assert_eq!(buf.pos, 2);
    }

    #[test]
    fn test_utf16be_bom_detection() {
        let data = vec![0xFE, 0xFF, 0x00, b'h', 0x00, b'i'];
        let buf = InputBuffer::from_memory(&data, None);
        assert!(buf.bom_was_consumed());
        assert_eq!(buf.encoding(), &Encoding::Utf16Be);
        assert_eq!(buf.pos, 2);
    }

    #[test]
    fn test_bom_with_encoding_declaration() {
        // BOM + XML declaration with encoding
        let mut data = vec![0xEF, 0xBB, 0xBF];
        data.extend_from_slice(b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
        let buf = InputBuffer::from_memory(&data, None);
        assert!(buf.bom_was_consumed());
        assert_eq!(buf.encoding(), &Encoding::Utf8);
    }

    // ── Encoding detection from XML declaration ────────────────────────────

    #[test]
    fn test_encoding_extraction_from_pi() {
        let pi = r#"<?xml version="1.0" encoding="UTF-8"?>"#;
        assert_eq!(
            InputBuffer::extract_encoding_from_pi(pi),
            Some("UTF-8".to_string())
        );
    }

    #[test]
    fn test_encoding_extraction_single_quotes() {
        let pi = r#"<?xml version='1.0' encoding='ISO-8859-1'?>"#;
        assert_eq!(
            InputBuffer::extract_encoding_from_pi(pi),
            Some("ISO-8859-1".to_string())
        );
    }

    #[test]
    fn test_encoding_extraction_no_encoding() {
        let pi = r#"<?xml version="1.0"?>"#;
        assert_eq!(InputBuffer::extract_encoding_from_pi(pi), None);
    }

    #[test]
    fn test_encoding_from_name() {
        assert_eq!(Encoding::from_name("UTF-8"), Encoding::Utf8);
        assert_eq!(Encoding::from_name("utf8"), Encoding::Utf8);
        assert_eq!(Encoding::from_name("UTF-16"), Encoding::Utf16Le);
        assert_eq!(Encoding::from_name("utf-16le"), Encoding::Utf16Le);
        assert_eq!(Encoding::from_name("UTF-16BE"), Encoding::Utf16Be);
        assert_eq!(Encoding::from_name("ASCII"), Encoding::Ascii);
        assert_eq!(Encoding::from_name("ISO-8859-1"), Encoding::Iso8859_1);
        assert_eq!(
            Encoding::from_name("Shift_JIS"),
            Encoding::Other("shift_jis".to_string())
        );
    }

    // ── Character reading ──────────────────────────────────────────────────

    #[test]
    fn test_read_char_ascii() {
        let mut buf = InputBuffer::from_memory(b"abc", None);
        assert_eq!(buf.read_char(), Some('a'));
        assert_eq!(buf.read_char(), Some('b'));
        assert_eq!(buf.read_char(), Some('c'));
        assert_eq!(buf.read_char(), None);
        assert!(buf.is_eof());
    }

    #[test]
    fn test_read_char_multibyte_utf8() {
        // 2-byte: é (U+00E9) = 0xC3 0xA9
        // 3-byte: € (U+20AC) = 0xE2 0x82 0xAC
        // 4-byte: 𐍈 (U+10348) = 0xF0 0x90 0x8D 0x88
        let data = vec![0xC3, 0xA9, 0xE2, 0x82, 0xAC, 0xF0, 0x90, 0x8D, 0x88];
        let mut buf = InputBuffer::from_memory(&data, None);
        assert_eq!(buf.read_char(), Some('é'));
        assert_eq!(buf.read_char(), Some('€'));
        assert_eq!(buf.read_char(), Some('𐍈'));
        assert_eq!(buf.read_char(), None);
    }

    #[test]
    fn test_peek_char() {
        let mut buf = InputBuffer::from_memory(b"abc", None);
        assert_eq!(buf.peek_char(), Some('a'));
        assert_eq!(buf.peek_char(), Some('a')); // peek doesn't advance
        assert_eq!(buf.read_char(), Some('a'));
        assert_eq!(buf.peek_char(), Some('b'));
    }

    #[test]
    fn test_read_char_tracking() {
        let mut buf = InputBuffer::from_memory(b"a\nb\nc", None);
        assert_eq!(buf.read_char(), Some('a'));
        assert_eq!(buf.pos(), (1, 2, 1));
        assert_eq!(buf.read_char(), Some('\n'));
        assert_eq!(buf.pos(), (2, 1, 2));
        assert_eq!(buf.read_char(), Some('b'));
        assert_eq!(buf.pos(), (2, 2, 3));
        assert_eq!(buf.read_char(), Some('\n'));
        assert_eq!(buf.pos(), (3, 1, 4));
        assert_eq!(buf.read_char(), Some('c'));
        assert_eq!(buf.pos(), (3, 2, 5));
    }

    #[test]
    fn test_crlf_handling() {
        let mut buf = InputBuffer::from_memory(b"a\r\nb", None);
        assert_eq!(buf.read_char(), Some('a'));
        assert_eq!(buf.pos(), (1, 2, 1));
        // \r should trigger newline; CRLF should consume the \n too
        assert_eq!(buf.read_char(), Some('\r'));
        // After \r, we're on line 2, col 1; the \n was consumed
        assert_eq!(buf.pos(), (2, 1, 3));
        assert_eq!(buf.read_char(), Some('b'));
        assert_eq!(buf.pos(), (2, 2, 4));
    }

    // ── Skip ───────────────────────────────────────────────────────────────

    #[test]
    fn test_skip() {
        let mut buf = InputBuffer::from_memory(b"hello world", None);
        assert_eq!(buf.skip(5), 5);
        assert_eq!(buf.remaining(), b" world");
        assert_eq!(buf.pos(), (1, 6, 5));
    }

    #[test]
    fn test_skip_past_end() {
        let mut buf = InputBuffer::from_memory(b"hi", None);
        assert_eq!(buf.skip(100), 2);
        assert!(buf.is_eof());
    }

    // ── Read string ────────────────────────────────────────────────────────

    #[test]
    fn test_read_string() {
        let mut buf = InputBuffer::from_memory(b"hello world", None);
        assert_eq!(buf.read_string(5), "hello");
        assert_eq!(buf.read_string(10), " world");
    }

    #[test]
    fn test_read_all_chars() {
        let mut buf = InputBuffer::from_memory(b"hello", None);
        assert_eq!(buf.read_all_chars(), "hello");
        assert!(buf.is_eof());
    }

    // ── Reset ──────────────────────────────────────────────────────────────

    #[test]
    fn test_reset() {
        let mut buf = InputBuffer::from_memory(b"hello", None);
        assert_eq!(buf.read_char(), Some('h'));
        assert_eq!(buf.read_char(), Some('e'));
        buf.reset();
        assert_eq!(buf.read_char(), Some('h'));
        assert_eq!(buf.pos(), (1, 2, 1));
    }

    // ── File reading ───────────────────────────────────────────────────────

    #[test]
    fn test_from_file_not_found() {
        let result = InputBuffer::from_file("/nonexistent/file.xml");
        assert!(result.is_err());
        match result {
            Err(InputError::Io(_)) => {} // expected
            _ => panic!("expected Io error"),
        }
    }

    // ── Position tracking ──────────────────────────────────────────────────

    #[test]
    fn test_position_tracking() {
        let mut buf = InputBuffer::from_memory(b"line1\nline2\nline3", None);
        // line1
        assert_eq!(buf.pos(), (1, 1, 0));
        assert_eq!(buf.read_string(5), "line1");
        assert_eq!(buf.pos(), (1, 6, 5));
        // \n
        assert_eq!(buf.read_char(), Some('\n'));
        assert_eq!(buf.pos(), (2, 1, 6));
        // line2
        assert_eq!(buf.read_string(5), "line2");
        assert_eq!(buf.pos(), (2, 6, 11));
        // \n
        assert_eq!(buf.read_char(), Some('\n'));
        assert_eq!(buf.pos(), (3, 1, 12));
        // line3
        assert_eq!(buf.read_string(5), "line3");
        assert_eq!(buf.pos(), (3, 6, 17));
        assert!(buf.is_eof());
    }

    // ── InputStack ─────────────────────────────────────────────────────────

    #[test]
    fn test_input_stack_basic() {
        let base = InputBuffer::from_memory(b"base ", None);
        let mut stack = InputStack::new(base);
        assert_eq!(stack.depth(), 1);

        let entity = InputBuffer::from_memory(b"entity", None);
        stack.push(entity);
        assert_eq!(stack.depth(), 2);

        // Read from entity
        assert_eq!(stack.read_char(), Some('e'));
        assert_eq!(stack.current_pos(), (1, 2, 1));

        // Pop back to base
        let popped = stack.pop();
        assert!(popped.is_some());
        assert_eq!(stack.depth(), 1);
        assert_eq!(stack.read_char(), Some('b'));
    }

    #[test]
    fn test_input_stack_no_pop_base() {
        let base = InputBuffer::from_memory(b"base", None);
        let mut stack = InputStack::new(base);
        assert!(stack.pop().is_none());
        assert_eq!(stack.depth(), 1);
    }

    #[test]
    fn test_input_stack_peek() {
        let base = InputBuffer::from_memory(b"abc", None);
        let mut stack = InputStack::new(base);
        assert_eq!(stack.peek_char(), Some('a'));
        assert_eq!(stack.read_char(), Some('a'));
        assert_eq!(stack.peek_char(), Some('b'));
    }

    #[test]
    fn test_input_stack_eof() {
        let base = InputBuffer::from_memory(b"ab", None);
        let mut stack = InputStack::new(base);
        assert!(!stack.is_eof());
        stack.read_char();
        stack.read_char();
        assert!(stack.is_eof());
    }

    // ── UTF-8 decoding edge cases ──────────────────────────────────────────

    #[test]
    fn test_decode_utf8_invalid_continuation() {
        // Invalid: 0xC0 followed by 0x00 (not a continuation byte)
        let data = vec![0xC0, 0x00];
        let mut buf = InputBuffer::from_memory(&data, None);
        // Should return None for the invalid sequence
        assert!(buf.read_char().is_none());
    }

    #[test]
    fn test_decode_utf8_truncated_sequence() {
        // Truncated 2-byte sequence (only leading byte)
        let data = vec![0xC3];
        let mut buf = InputBuffer::from_memory(&data, None);
        assert!(buf.read_char().is_none());
    }

    // ── Encoding detection in XML declaration (no BOM) ────────────────────

    #[test]
    fn test_encoding_detection_utf8_xml_decl() {
        let buf = InputBuffer::from_memory(b"<?xml version='1.0' encoding='UTF-8'?>", None);
        assert_eq!(buf.encoding(), &Encoding::Utf8);
    }

    #[test]
    fn test_encoding_detection_latin1_xml_decl() {
        let buf = InputBuffer::from_memory(b"<?xml version='1.0' encoding='ISO-8859-1'?>", None);
        assert_eq!(buf.encoding(), &Encoding::Iso8859_1);
    }

    #[test]
    fn test_encoding_detection_unknown() {
        let buf = InputBuffer::from_memory(b"<?xml version='1.0' encoding='Shift_JIS'?>", None);
        assert_eq!(buf.encoding(), &Encoding::Other("shift_jis".to_string()));
    }
}
