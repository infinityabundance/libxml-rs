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
//!
//! # Upstream contract
//!
//! Mirrors the input-stream layer of upstream parserInternals.c and xmlIO.c
//! (SRC-LIBXML2-2.15.0, oracle tree `oracle/historical/src/libxml2-2.15.0/`):
//! `_xmlParserInput` / `_xmlParserInputBuffer` construction, BOM detection,
//! encoding sniffing, line/column tracking and input stacking. Parity target:
//! the system libxml2 2.15.3 oracle.
//!
//! # Conceptual behavior
//!
//! Provides safe-Rust abstractions over XML input sources: memory buffers,
//! files and custom I/O callbacks, with character-level position tracking, BOM
//! detection, encoding detection from the XML declaration, and the
//! entity-expansion input stack. The safe types are NOT `#[repr(C)]`; C ABI
//! structs are only populated at the FFI boundary.
//!
//! # Ownership & safety invariants
//!
//! Ownership: the InputBuffer owns its byte storage; InputStack owns its
//! buffers in LIFO order (entity expansion pushes/pops); `_xmlParserInput`
//! pointers borrow the buffer. SAFETY: line/col/byte positions are computed in
//! safe code; the only raw pointers are the populated C structs handed to the
//! parser. Filenames flowing into C structs are owned dupes (R-000169).
//!
//! # Historical quirks & epochs
//!
//! Epoch facts: the modern era (2.10+, atlas/HISTORY.md 1.8) moved toward a
//! built-in UTF-8/UTF-16 converter (inferred); the push-parser chunk
//! semantics date from the 2.6 validation era; the 11.1-M error rework
//! (R-000163) pinned columns to byte-based `input->col` semantics.
//!
//! # Deliberate oddities
//!
//! Deliberate oddities: encoding names are normalized to lowercase with
//! utf-16 defaulting to LE when there is no BOM; unknown encodings degrade to
//! `Encoding::Other` rather than failing at load time (the encoding module
//! owns the unsupported-encoding error, R-000157).
//!
//! # Proving courts
//!
//! Exercised by the PARSER court family, ERROR-001 (filename/line/column
//! windows), TREE-001 (input filename fingerprinting) and `cargo test --lib`.
//! Receipts under courts/receipts/phase-11.
//!
//! # Tempting simplifications that would break parity
//!
//! Reading the whole input into a single Vec would break the push parser and
//! the entity-expansion input stack (xmlParseChunk / xmlCtxtParseEntity
//! semantics). Do not pre-decode the stream at load time — the parser must see
//! raw bytes so diagnostics and re-encoded input behave like upstream.

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
    /// EBCDIC code page 037 (first-4-byte pattern detected: 4C 6F A7 94).
    Ebcdic,
    /// UCS-4 (UTF-32) little-endian (pattern: 3C 00 00 00).
    Ucs4Le,
    /// UCS-4 (UTF-32) big-endian (pattern: 00 00 00 3C).
    Ucs4Be,
    /// Other encoding (name stored for reference).
    Other(String),
}

impl Encoding {
    /// Convert to the C ABI [`xmlCharEncoding`] value.
    pub(crate) const fn to_xml_char_encoding(&self) -> xmlCharEncoding {
        match self {
            Self::None => xmlCharEncoding::XML_CHAR_ENCODING_NONE,
            Self::Utf8 => xmlCharEncoding::XML_CHAR_ENCODING_UTF8,
            Self::Utf16Le => xmlCharEncoding::XML_CHAR_ENCODING_UTF16LE,
            Self::Utf16Be => xmlCharEncoding::XML_CHAR_ENCODING_UTF16BE,
            Self::Ascii => xmlCharEncoding::XML_CHAR_ENCODING_ASCII,
            Self::Iso8859_1 => xmlCharEncoding::XML_CHAR_ENCODING_8859_1,
            Self::Ebcdic => xmlCharEncoding::XML_CHAR_ENCODING_EBCDIC,
            Self::Ucs4Le => xmlCharEncoding::XML_CHAR_ENCODING_UCS4LE,
            Self::Ucs4Be => xmlCharEncoding::XML_CHAR_ENCODING_UCS4BE,
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
    const fn filename(&self) -> Option<&str> {
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
    /// Whether the buffered `data` has already been transcoded to UTF-8
    /// (UTF-16 BOM decode or a native non-UTF-8 encoding declared in the XML
    /// declaration, e.g. ISO-8859-1). While false, incremental `push_bytes`
    /// calls keep re-running detection so a declaration that only becomes
    /// visible once the accumulated input grows is still honored (KEY-1:
    /// BOM-less declared-encoding inputs, xslt.xml `encoding="iso-8859-1"`).
    converted_to_utf8: bool,
    /// Set when the input starts with `<?xml` whose `?>` has not been seen
    /// yet — the declaration may complete on a later push call.
    decl_pending: bool,
    /// The source (file / callback) failed to produce data: upstream
    /// raises an I/O error on the first grow instead of parsing empty
    /// content (HOSTILE-CALLBACKS C4).
    io_failed: bool,
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
            .field("converted_to_utf8", &self.converted_to_utf8)
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
            converted_to_utf8: false,
            decl_pending: false,
            io_failed: false,
        };
        ib.detect_bom_and_encoding();
        ib
    }

    /// Append raw bytes to the buffered input (push-parser mode). Upstream
    /// `xmlParseChunk` grows the parser input's base with each chunk; the
    /// candidate accumulates into the stashed buffer and parses on the
    /// terminating call (Phase-12 EXTERNAL-CONSUMERS court: parse4.c).
    pub fn push_bytes(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let first_real_bytes = self.data.is_empty() && self.pos == 0 && !self.bom_consumed;
        // Once the accumulated buffer has been transcoded to UTF-8, the raw
        // tail still arrives in the declared (source) encoding. Convert just
        // the new tail per the source encoding (KEY-1): single-byte legacy
        // encodings (ISO-8859-x, windows-1252 …) and the other
        // registry-served declared encodings are byte-wise/self-synchronizing
        // enough to convert tail-chunk-wise like upstream's incremental
        // encoder. For any other latched encoding, keep appending raw like
        // upstream's buffered input does before the parser's own encoder
        // processes it.
        if self.converted_to_utf8 && !matches!(self.encoding, Encoding::Utf8 | Encoding::Ascii) {
            if let Some(src_name) = self.legacy_source_encoding_name() {
                if let InputSource::Memory(d) = &mut self.source {
                    d.extend_from_slice(bytes);
                }
                let tail = crate::xml::encoding::decode_whole_buffer_declared(&src_name, bytes);
                if let Ok(conv) = tail {
                    self.data.extend_from_slice(&conv);
                    return;
                }
                // Undecodable tail: append raw; the tokenizer reports the
                // invalid-character error.
                self.data.extend_from_slice(bytes);
                return;
            }
        }
        self.data.extend_from_slice(bytes);
        if let InputSource::Memory(d) = &mut self.source {
            d.extend_from_slice(bytes);
        }
        // Re-run detection when the first real bytes arrive (the buffer was
        // constructed empty) or when an in-progress `<?xml` declaration may
        // have just completed on this push (KEY-1: a BOM-less declared
        // encoding such as `encoding="iso-8859-1"` only becomes visible once
        // enough of the stream has accumulated).
        if first_real_bytes || (self.decl_pending && !self.converted_to_utf8) {
            self.detect_bom_and_encoding();
        }
    }

    /// Produce an independent copy of this buffer at its current state, so
    /// the same accumulated input can be parsed more than once (incremental
    /// push probe/delivery: `helpers::parse_chunk` runs a silent completeness
    /// probe and, when the accumulated input is a complete document, a
    /// completing parse over an identical buffer).
    pub(crate) fn duplicate_for_reparse(&self) -> InputBuffer {
        InputBuffer {
            source: InputSource::Memory(self.data.clone()),
            data: self.data.clone(),
            pos: self.pos,
            line: self.line,
            col: self.col,
            encoding: self.encoding.clone(),
            filename: self.filename.clone(),
            bom_consumed: self.bom_consumed,
            converted_to_utf8: self.converted_to_utf8,
            decl_pending: self.decl_pending,
            io_failed: self.io_failed,
        }
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
            converted_to_utf8: false,
            decl_pending: false,
            io_failed: false,
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
            converted_to_utf8: false,
            decl_pending: false,
            io_failed: false,
        };
        ib.detect_bom_and_encoding();
        Ok(ib)
    }

    /// Create an `InputBuffer` whose source failed to produce data (read
    /// callback returned an error). The parser raises an I/O error at the
    /// first grow — mirroring upstream — instead of reporting an empty
    /// document.
    pub const fn failed_source() -> Self {
        InputBuffer {
            source: InputSource::Memory(Vec::new()),
            data: Vec::new(),
            pos: 0,
            line: 1,
            col: 1,
            encoding: Encoding::Utf8,
            filename: None,
            bom_consumed: false,
            converted_to_utf8: false,
            decl_pending: false,
            io_failed: true,
        }
    }

    /// Whether a UTF-8 BOM (`EF BB BF`) was consumed and retained at the
    /// start of the buffer (its bytes occupy offsets 0..3). UTF-16 BOMs are
    /// stripped during conversion, so they report 0.
    pub(crate) const fn bom_bytes_consumed(&self) -> usize {
        if self.bom_consumed {
            3
        } else {
            0
        }
    }

    /// Whether the underlying source failed (read callback returned an
    /// error).
    pub const fn has_source_error(&self) -> bool {
        self.io_failed
    }

    /// Set the input's filename/URI. Upstream stores the base URL as the
    /// input filename (xmlCtxtNewInputFromMemory/FromIO), which feeds the
    /// `file:line:` error prefix (HOSTILE-CALLBACKS C3/C4).
    pub fn with_filename(mut self, name: &str) -> Self {
        self.filename = Some(name.to_string());
        self
    }

    // ── BOM and encoding detection ─────────────────────────────────────────

    /// Detect BOM and encoding from the beginning of the data.
    ///
    /// Upstream performs the same sniffing inside its parserInternals.c/xmlIO.c
    /// input-switch paths (the encoding switch machinery of the parser): UTF-8
    /// BOM, UTF-16 LE/BE BOMs, then the XML declaration; the BOM must be
    /// consumed before any character is reported so line/column counts match
    /// the oracle.
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

        // Check for UTF-32 LE BOM: FF FE 00 00 (must precede the UTF-16LE
        // check — its first two bytes are the UTF-16LE BOM).
        if self.data.len() >= 4
            && self.data[0] == 0xFF
            && self.data[1] == 0xFE
            && self.data[2] == 0x00
            && self.data[3] == 0x00
        {
            self.encoding = Encoding::Ucs4Le;
            self.pos = 4;
            self.col = 5;
            self.bom_consumed = true;
            self.convert_declared_native_encoding();
            return;
        }

        // Check for UTF-32 BE BOM: 00 00 FE FF
        if self.data.len() >= 4
            && self.data[0] == 0x00
            && self.data[1] == 0x00
            && self.data[2] == 0xFE
            && self.data[3] == 0xFF
        {
            self.encoding = Encoding::Ucs4Be;
            self.pos = 4;
            self.col = 5;
            self.bom_consumed = true;
            self.convert_declared_native_encoding();
            return;
        }

        // Check for UTF-16 LE BOM: FF FE
        if self.data.len() >= 2 && self.data[0] == 0xFF && self.data[1] == 0xFE {
            self.encoding = Encoding::Utf16Le;
            self.pos = 2;
            self.col = 3;
            self.bom_consumed = true;
            self.convert_detected_utf16();
            return;
        }

        // Check for UTF-16 BE BOM: FE FF
        if self.data.len() >= 2 && self.data[0] == 0xFE && self.data[1] == 0xFF {
            self.encoding = Encoding::Utf16Be;
            self.pos = 2;
            self.col = 3;
            self.bom_consumed = true;
            self.convert_detected_utf16();
            return;
        }

        // No BOM found. Sniff upstream xmlDetectCharEncoding's first-4-byte
        // patterns for the non-ASCII-compatible encodings whose XML
        // declaration cannot be read as ASCII/UTF-8: UCS-4 LE/BE (the `<?`
        // code units interleaved with NULs), EBCDIC 037 (`<?xm` as 4C 6F A7
        // 94) and BOM-less UTF-16 (`<\0?\0` / `\0<\0?` — upstream also
        // auto-recognizes those). Each switches to the matching whole-buffer
        // decoder; the parser then reads the converted UTF-8 (the
        // declaration inside is never re-scanned, exactly like upstream's
        // xmlSwitchEncoding).
        if self.data.len() >= 4 {
            let d0 = self.data[0];
            let d1 = self.data[1];
            let d2 = self.data[2];
            let d3 = self.data[3];
            if d0 == 0x3C && d1 == 0x00 && d2 == 0x00 && d3 == 0x00 {
                self.encoding = Encoding::Ucs4Le;
                self.convert_declared_native_encoding();
                return;
            }
            if d0 == 0x00 && d1 == 0x00 && d2 == 0x00 && d3 == 0x3C {
                self.encoding = Encoding::Ucs4Be;
                self.convert_declared_native_encoding();
                return;
            }
            if d0 == 0x4C && d1 == 0x6F && d2 == 0xA7 && d3 == 0x94 {
                self.encoding = Encoding::Ebcdic;
                self.convert_declared_native_encoding();
                return;
            }
            if d0 == 0x3C && d1 == 0x00 && d2 == 0x3F && d3 == 0x00 {
                self.encoding = Encoding::Utf16Le;
                self.convert_detected_utf16();
                return;
            }
            if d0 == 0x00 && d1 == 0x3C && d2 == 0x00 && d3 == 0x3F {
                self.encoding = Encoding::Utf16Be;
                self.convert_detected_utf16();
                return;
            }
        }

        // No BOM or pattern found. Default to UTF-8 and check for XML declaration.
        self.encoding = Encoding::Utf8;
        self.detect_encoding_from_xml_declaration();
        // The XML declaration may name a native non-UTF-8 encoding (e.g.
        // `encoding="iso-8859-1"` on a BOM-less stream). Transcode the
        // buffered bytes to UTF-8 so the parser never sees raw non-UTF-8
        // bytes (KEY-1: upstream `xmlSwitchEncoding` after xmlParseXMLDecl;
        // without this the tokenizer raises "Invalid bytes in character
        // encoding" on every valid Latin-1 byte >= 0x80).
        self.convert_declared_native_encoding();
    }

    /// Transcode `data` to UTF-8 when the XML declaration named an encoding
    /// the crate has a converter for, or the first bytes pattern-detected a
    /// non-ASCII-compatible encoding (UCS-4/EBCDIC). ISO-8859-1 is a
    /// byte-wise mapping (every byte 0x80..=0xFF becomes a two-byte UTF-8
    /// sequence, all ASCII stays identical — including the declaration
    /// itself), so the whole buffered stream converts safely regardless of
    /// how much has arrived. Every other registry-served legacy encoding
    /// (ISO-8859-2..16, windows-1252, Shift_JIS, EUC-JP, ISO-2022-JP, UCS-2,
    /// UCS-4LE/BE, EBCDIC …) is decoded whole-buffer through its registered
    /// input handler the same way (R-000157 input side, Phase 14.29).
    /// Unknown encodings are left untouched so the existing
    /// unsupported-encoding handling applies unchanged.
    fn convert_declared_native_encoding(&mut self) {
        if self.converted_to_utf8 {
            return;
        }
        match &self.encoding {
            Encoding::Iso8859_1 => {
                let raw = std::mem::take(&mut self.data);
                self.data = crate::xml::encoding::latin1_to_utf8(&raw);
                self.converted_to_utf8 = true;
            }
            Encoding::Ascii => {
                // US-ASCII is a strict UTF-8 subset: no transcode, but latch so
                // incremental pushes stop re-detecting.
                self.converted_to_utf8 = true;
            }
            Encoding::Other(name) => self.convert_via_registry(&name.clone().into_bytes()),
            Encoding::Ebcdic => self.convert_via_registry(b"IBM037"),
            Encoding::Ucs4Le => self.convert_via_registry(b"UCS-4LE"),
            Encoding::Ucs4Be => self.convert_via_registry(b"UCS-4BE"),
            _ => {}
        }
    }

    /// Whole-buffer decode of the raw input through the registry handler for
    /// `name`. On success the converted UTF-8 replaces `data` and the
    /// position resets; on failure the raw bytes stay and the tokenizer
    /// reports the invalid-character error like upstream (mirrors the
    /// UTF-16 `convert_detected_utf16` failure handling). The `encoding`
    /// field is deliberately NOT reset: incremental `push_bytes` tails still
    /// arrive in the source encoding and convert per-tail (KEY-1).
    fn convert_via_registry(&mut self, name: &[u8]) {
        if self.converted_to_utf8 || self.data.is_empty() {
            return;
        }
        match crate::xml::encoding::decode_whole_buffer_declared(name, &self.data) {
            Ok(conv) => {
                self.data = conv;
                self.pos = 0;
                self.col = 1;
                self.bom_consumed = false;
                self.converted_to_utf8 = true;
            }
            Err(()) => {
                self.encoding = Encoding::Utf8;
                self.pos = 0;
                self.col = 1;
                self.bom_consumed = false;
                self.converted_to_utf8 = false;
            }
        }
    }

    /// Convert a BOM-detected UTF-16 input buffer to UTF-8 in place.
    ///
    /// Upstream's input-switch machinery (`xmlSwitchEncoding`) installs the
    /// UTF-16 decoder as soon as the BOM is seen, so the parser never
    /// observes the raw 16-bit code units. The candidate's `InputBuffer`
    /// buffers the raw bytes, so the conversion happens here: the whole
    /// buffer (including the BOM, which the converters skip) is decoded to
    /// UTF-8 and the position resets to the start of the converted stream.
    fn convert_detected_utf16(&mut self) {
        let raw = self.data.clone();
        let converted = match self.encoding {
            Encoding::Utf16Le => crate::xml::encoding::utf16le_to_utf8(&raw),
            Encoding::Utf16Be => crate::xml::encoding::utf16be_to_utf8(&raw),
            _ => return,
        };
        match converted {
            Ok(conv) => {
                self.data = conv;
                self.pos = 0;
                self.col = 1;
                self.bom_consumed = false;
                self.converted_to_utf8 = true;
            }
            Err(()) => {
                // Undecodable input: keep the raw bytes; the tokenizer will
                // report the invalid-character error like upstream.
                self.encoding = Encoding::Utf8;
                self.pos = 0;
                self.col = 1;
                self.bom_consumed = false;
                self.converted_to_utf8 = false;
            }
        }
    }

    /// Apply a caller-supplied whole-buffer encoding override (upstream
    /// `xmlSwitchToEncoding` on a memory parser input, PHP's
    /// `overrideEncoding` path which switches before `xmlParseDocument`).
    ///
    /// Only encodings with a native whole-buffer converter are handled;
    /// returns `false` when the override cannot be applied (name unknown or
    /// the stream already converted, e.g. a BOM-decoded UTF-16 input where
    /// the raw bytes are gone). The converted stream replaces `data` and the
    /// position resets so the caller can repopulate the `_xmlParserInput`.
    pub(crate) fn apply_name_encoding_override(&mut self, name: &[u8]) -> bool {
        if self.converted_to_utf8 {
            // Raw bytes already transcoded (BOM UTF-16 / declared Latin-1):
            // re-decoding the UTF-8 stream under the override would corrupt
            // it, and upstream's switch happened before any conversion.
            return false;
        }
        let lower = String::from_utf8_lossy(name).to_ascii_lowercase();
        let converted: Option<Vec<u8>> = match lower.as_str() {
            "windows-1252" | "cp1252" => crate::xml::encoding::cp1252_to_utf8(&self.data).ok(),
            "iso-8859-1" | "iso8859-1" | "latin1" | "latin-1" => {
                Some(crate::xml::encoding::latin1_to_utf8(&self.data))
            }
            "us-ascii" | "ascii" => Some(self.data.clone()),
            _ => None,
        };
        match converted {
            Some(conv) => {
                self.data = conv;
                self.pos = 0;
                self.col = 1;
                self.line = 1;
                self.bom_consumed = false;
                self.converted_to_utf8 = true;
                self.encoding = Encoding::Utf8;
                true
            }
            None => false,
        }
    }

    /// Apply an EXPLICIT caller-supplied input encoding (the `encoding`
    /// argument of xmlCtxtReadMemory/ReadDoc and friends, upstream
    /// `xmlCtxtNewInputFromMemory` -> `xmlSwitchEncoding` before the parse).
    /// Any encodable multi-byte or legacy encoding converts the whole raw
    /// buffer to UTF-8 up front (UTF-16LE/BE, UCS-4LE/BE, Latin-1 and the
    /// other registry-served encodings — lxml feeds PEP-393 KIND-2/4 python
    /// strings this way). Returns false when no conversion applies (the raw
    /// bytes stay; BOM/declaration detection then decides as usual).
    pub(crate) fn apply_explicit_input_encoding(&mut self, name: &[u8]) -> bool {
        if self.converted_to_utf8 || self.data.is_empty() {
            return false;
        }
        let enc = crate::xml::encoding::encoding_from_name(name);
        let converted: Option<Vec<u8>> = match enc {
            crate::abi::types::xmlCharEncoding::XML_CHAR_ENCODING_UTF16LE => {
                crate::xml::encoding::utf16le_to_utf8(&self.data).ok()
            }
            crate::abi::types::xmlCharEncoding::XML_CHAR_ENCODING_UTF16BE => {
                crate::xml::encoding::utf16be_to_utf8(&self.data).ok()
            }
            crate::abi::types::xmlCharEncoding::XML_CHAR_ENCODING_UCS4LE => {
                crate::xml::encoding::ucs4le_to_utf8(&self.data).ok()
            }
            crate::abi::types::xmlCharEncoding::XML_CHAR_ENCODING_UCS4BE => {
                crate::xml::encoding::ucs4be_to_utf8(&self.data).ok()
            }
            crate::abi::types::xmlCharEncoding::XML_CHAR_ENCODING_8859_1 => {
                Some(crate::xml::encoding::latin1_to_utf8(&self.data))
            }
            crate::abi::types::xmlCharEncoding::XML_CHAR_ENCODING_ASCII
            | crate::abi::types::xmlCharEncoding::XML_CHAR_ENCODING_UTF8 => Some(self.data.clone()),
            // Other registry-served encodings (ISO-8859-2..16, Shift_JIS,
            // EUC-JP, ISO-2022-JP, UCS-2, EBCDIC ...): whole-buffer decode
            // through the registered input handler (R-000157).
            crate::abi::types::xmlCharEncoding::XML_CHAR_ENCODING_ERROR => None,
            _ => {
                if let Some(canon) = crate::xml::encoding::encoding_name(enc) {
                    crate::xml::encoding::decode_whole_buffer_declared(canon, &self.data).ok()
                } else {
                    None
                }
            }
        };
        match converted {
            Some(conv) => {
                self.data = conv;
                self.pos = 0;
                self.col = 1;
                self.line = 1;
                self.bom_consumed = false;
                self.converted_to_utf8 = true;
                self.encoding = Encoding::Utf8;
                true
            }
            None => false,
        }
    }

    /// The registry name of the source encoding whose raw bytes still
    /// arrive incrementally after a whole-buffer conversion (KEY-1 tail
    /// path). UTF-16 (BOM-switched) and UTF-8/ASCII return None — their
    /// tails are handled by the existing raw-append paths.
    fn legacy_source_encoding_name(&self) -> Option<Vec<u8>> {
        match &self.encoding {
            Encoding::Iso8859_1 => Some(b"ISO-8859-1".to_vec()),
            Encoding::Ebcdic => Some(b"IBM037".to_vec()),
            Encoding::Ucs4Le => Some(b"UCS-4LE".to_vec()),
            Encoding::Ucs4Be => Some(b"UCS-4BE".to_vec()),
            Encoding::Other(name) => Some(name.clone().into_bytes()),
            _ => None,
        }
    }

    /// Try to detect encoding from an XML declaration at the start of the input.
    ///
    /// Looks for `<?xml ... encoding="..."?>` or `<?xml ... encoding='...'?>`
    /// after any BOM has been consumed.
    fn detect_encoding_from_xml_declaration(&mut self) {
        let remaining = &self.data[self.pos..];

        // Must start with "<?xml"
        if remaining.len() < 5 {
            // Fewer than 5 bytes: a `<?xml` prefix may still be arriving on a
            // later push call — but only when the stream actually started
            // with `<?xml` so far. If fewer bytes than `<?xml` are present we
            // cannot tell yet whether a declaration is coming; optimistically
            // stay pending only when what we have is a prefix of `<?xml`.
            self.decl_pending = remaining == b"<"
                || remaining == b"<?"
                || remaining == b"<?x"
                || remaining == b"<?xm";
            return;
        }
        if &remaining[..5] != b"<?xml" {
            // The document does not start with an XML declaration; a later
            // push can never produce one (the declaration must be at offset 0).
            self.decl_pending = false;
            return;
        }

        // Find the end of the PI: ">"
        let pi_end = remaining.windows(2).position(|w| w == b"?>");
        let pi_end = match pi_end {
            Some(e) => e + 2,
            None => {
                // `<?xml` declaration truncated by the end of the available
                // input: it may complete on a later push call (KEY-1).
                self.decl_pending = true;
                return;
            }
        };
        self.decl_pending = false;

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
    const fn utf8_char_len(leading: u8) -> usize {
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
    pub const fn pos(&self) -> (usize, usize, usize) {
        (self.line, self.col, self.pos)
    }

    /// Check if the input has been fully consumed.
    pub const fn is_eof(&self) -> bool {
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
    pub const fn len(&self) -> usize {
        self.data.len()
    }

    /// Return the filename or URI, if known.
    pub fn filename(&self) -> Option<&str> {
        self.filename.as_deref()
    }

    /// Return the detected encoding.
    pub const fn encoding(&self) -> &Encoding {
        &self.encoding
    }

    /// Return whether a BOM was detected and consumed.
    pub const fn bom_was_consumed(&self) -> bool {
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
    ///
    /// # Safety
    ///
    /// - `self` must be a valid `InputBuffer` and `input` a valid
    ///   `_xmlParserInput`; the `base`/`cur`/`end` pointers written into
    ///   `input` borrow `self.data`, so the buffer must stay alive and not
    ///   be mutated or reallocated while the parser input is in use.
    pub const unsafe fn populate_parser_input_without_filename(&self, input: &mut _xmlParserInput) {
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

    /// Byte offset (in the base input) where the document content logically
    /// begins: 0 normally, or 3 when a UTF-8 BOM was consumed and retained in
    /// the buffer. The XML declaration is "at the start of the document" when
    /// its token begins at this offset (KEY-3: xmlParseDocument runs the
    /// declaration check at the logical start, after the input layer stripped
    /// any BOM).
    pub(crate) fn doc_start_offset(&self) -> usize {
        self.inputs[0].bom_bytes_consumed()
    }

    /// Whether the parser is reading the base document input (no entity
    /// expansion on the stack).
    pub(crate) const fn at_base_input(&self) -> bool {
        self.current == 0
    }

    /// Resolve the error location the way upstream `xmlCtxtVErr` does
    /// (parserInternals.c 2.15): use the current input's filename/line/col,
    /// but when the current input has no filename and the stack is nested
    /// (`inputNr > 1`), fall back to the PARENT input's filename/line/col —
    /// entity-content errors are attributed to the referencing document
    /// (HOSTILE-CALLBACKS C1/C2).
    ///
    /// Returns `(filename, line, col)`.
    pub fn error_context(&self) -> (Option<&str>, usize, usize) {
        let cur = &self.inputs[self.current];
        if cur.filename().is_none() && self.current > 0 {
            let parent = &self.inputs[self.current - 1];
            let (pl, pc, _) = parent.pos();
            // UPSTREAM-PARITY: a frozen (suspended) input's `col` lags the
            // next-char position by the raw `NEXT` macro consumes (e.g. the
            // trailing `;` of an entity reference — parserInternals.c
            // xmlParseEntityRef ends with `NEXT` without the xmlCurrentChar
            // col++). The oracle reports the last col-tracked char, so the
            // candidate's 1-based next-char column is one ahead; clamp at 1
            // (a newline consume resets col to 1 in both models).
            (parent.filename(), pl, pc.saturating_sub(1).max(1))
        } else {
            let (l, c, _) = cur.pos();
            (cur.filename(), l, c)
        }
    }

    /// Returns the depth of the stack (number of nested inputs).
    ///
    /// A stack with only the base input has depth 1.
    pub const fn depth(&self) -> usize {
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
        // UPSTREAM-PARITY: the UTF-16 input is converted to UTF-8 as soon as
        // the BOM is seen (xmlSwitchEncoding), so the buffer starts at the
        // converted stream with the BOM consumed.
        assert_eq!(buf.encoding(), &Encoding::Utf16Le);
        assert_eq!(buf.pos, 0);
        assert_eq!(buf.remaining(), b"hi");
    }

    #[test]
    fn test_utf16be_bom_detection() {
        let data = vec![0xFE, 0xFF, 0x00, b'h', 0x00, b'i'];
        let buf = InputBuffer::from_memory(&data, None);
        assert_eq!(buf.encoding(), &Encoding::Utf16Be);
        assert_eq!(buf.pos, 0);
        assert_eq!(buf.remaining(), b"hi");
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

    // ── KEY-1: BOM-less declared native encoding is transcoded to UTF-8 ──

    /// A BOM-less stream whose XML declaration names `iso-8859-1` must have
    /// its bytes transcoded to UTF-8 (KEY-1): the parser otherwise raises
    /// "Invalid bytes in character encoding" on every Latin-1 byte >= 0x80
    /// (upstream `xmlSwitchEncoding` after `xmlParseXMLDecl`).
    #[test]
    fn test_declared_latin1_bytes_transcoded_to_utf8() {
        // `<?xml version="1.0" encoding="iso-8859-1"?><r>ä</r>` in Latin-1.
        let mut raw = b"<?xml version=\"1.0\" encoding=\"iso-8859-1\"?><r>".to_vec();
        raw.push(0xE4); // 'ä' in ISO-8859-1
        raw.extend_from_slice(b"</r>");
        let mut buf = InputBuffer::from_memory(&raw, None);
        assert_eq!(buf.encoding(), &Encoding::Iso8859_1);
        assert!(buf.converted_to_utf8, "Latin-1 data must be transcoded");
        let all = buf.read_all_chars();
        assert!(
            all.contains('ä'),
            "declared Latin-1 byte must decode to UTF-8 'ä', got {all:?}"
        );
        assert!(
            !all.contains('�'),
            "no U+FFFD replacement may appear (raw byte was misread as UTF-8), got {all:?}"
        );
    }

    /// Incremental push: the declaration only becomes visible once enough of
    /// the stream accumulated; detection must re-run and transcode then.
    #[test]
    fn test_declared_latin1_incremental_push_transcodes() {
        let mut raw = b"<?xml version=\"1.0\" encoding=\"iso-8859-1\"?><r>".to_vec();
        raw.push(0xE4);
        raw.extend_from_slice(b"</r>");
        let mut buf = InputBuffer::from_memory(&[], None);
        // Feed in small chunks so the full `<?xml ... ?>` declaration is not
        // present until the third push (mid-stream completion).
        for chunk in raw.chunks(7) {
            buf.push_bytes(chunk);
        }
        assert_eq!(buf.encoding(), &Encoding::Iso8859_1);
        assert!(buf.converted_to_utf8);
        let all = buf.read_all_chars();
        assert!(
            all.contains('ä'),
            "incremental Latin-1 push must decode to UTF-8 'ä', got {all:?}"
        );
    }

    /// `duplicate_for_reparse` (push probe/delivery) must carry the converted
    /// stream and the latch so a re-parse never re-transcodes or sees raw bytes.
    #[test]
    fn test_duplicate_of_converted_latin1_stays_utf8() {
        let mut raw = b"<?xml version=\"1.0\" encoding=\"iso-8859-1\"?><r>".to_vec();
        raw.push(0xE4);
        raw.extend_from_slice(b"</r>");
        let buf = InputBuffer::from_memory(&raw, None);
        let mut dup = buf.duplicate_for_reparse();
        assert!(dup.converted_to_utf8);
        let all = dup.read_all_chars();
        assert!(
            all.contains('ä'),
            "duplicated converted buffer must still read as UTF-8, got {all:?}"
        );
    }
}
