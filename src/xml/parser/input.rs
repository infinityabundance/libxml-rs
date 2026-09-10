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
    /// UCS-2 (2-byte code units, registry byte order = host/little-endian —
    /// the glibc iconv "UCS-2" the oracle serves). Reached from a declaration
    /// naming `ucs-2`.
    Ucs2Le,
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
            Self::Ucs2Le => xmlCharEncoding::XML_CHAR_ENCODING_UCS2,
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

/// How the source encoding of an [`InputBuffer`] stands (§16.7.8 progressive
/// decoding).
///
/// Upstream decides the source encoding inside the parser state machine: the
/// `XML_PARSER_START` arm of `xmlParseTryOrFinish` refuses to run
/// `xmlDetectEncoding` until four source bytes are available (and, for the
/// EBCDIC signature `4C 6F A7 94`, until 200 are available, or the call is
/// final). Encoding detection is therefore part of the *observable* push
/// semantics, not a load-time detail — the candidate models it explicitly
/// instead of inferring it from assorted booleans.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SourceDecoding {
    /// Non-progressive input (memory/file/callback constructors): detection
    /// ran once over the complete input at construction and never re-runs.
    WholeBuffer,
    /// Progressive (push) input still in upstream `XML_PARSER_START`: the
    /// bytes received so far are held undecoded in
    /// [`InputBuffer::pending_source`] because the encoding cannot be decided
    /// yet. A non-final call with such an input parks exactly like
    /// `xmlParseTryOrFinish`'s `goto done` — nothing is parsed, no event
    /// fires and no error is raised. A terminating call always decides.
    Parked,
    /// Progressive input with a decided source encoding. The decoder is
    /// installed (`encoding` / `converted_to_utf8`); source bytes may still
    /// be held back as an incomplete encoding unit (an odd UTF-16 byte, a
    /// lone high surrogate, a partial UTF-32 unit) until the rest arrives.
    Decided,
}

/// Owned-or-borrowed byte storage of an [`InputBuffer`] (§16.5.2 — borrowed
/// synchronous memory input).
///
/// The ownership model is explicit:
///
/// - [`InputBytes::Owned`] — the buffer owns a private, growable byte
///   vector: push parsing (chunks accumulate across calls), file/IO sources,
///   transcoded input, and probe-reparse duplicates.
/// - [`InputBytes::Borrowed`] — the buffer borrows a caller-owned region for
///   the duration of the synchronous parse that consumes it (`xmlReadMemory`
///   & the other one-call front-ends): zero allocation on the ordinary
///   UTF-8 whole-buffer path. A borrowed buffer must never outlive its
///   parse — every mutation path (`push_bytes`, transcoding, override,
///   reparse) first converts to Owned, and the one-call front-ends free
///   their context (and therefore the buffer) before returning.
///
/// Reads go through [`Deref`](core::ops::Deref), so `len()`/indexing/slicing
/// behave like the former `Vec<u8>` field; only the mutation points are
/// explicit about ownership.
#[derive(Debug)]
enum InputBytes {
    Owned(Vec<u8>),
    /// See [`InputBytes`] docs for the lifetime contract.
    Borrowed(&'static [u8]),
}

impl core::ops::Deref for InputBytes {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        match self {
            InputBytes::Owned(v) => v,
            InputBytes::Borrowed(s) => s,
        }
    }
}

impl InputBytes {
    /// Total byte length (const-friendly, no Deref in const fns).
    const fn data_len(&self) -> usize {
        match self {
            InputBytes::Owned(v) => v.len(),
            InputBytes::Borrowed(s) => s.len(),
        }
    }

    /// Return the bytes as a mutable owned `Vec`, converting a Borrowed
    /// buffer by copying (§16.5.2: copy only when mutation requires it).
    fn make_owned(&mut self) -> &mut Vec<u8> {
        if let InputBytes::Borrowed(s) = self {
            *self = InputBytes::Owned(s.to_vec());
        }
        match self {
            InputBytes::Owned(v) => v,
            InputBytes::Borrowed(_) => unreachable!(),
        }
    }

    /// Take the bytes as an owned `Vec` (Borrowed copies; Owned moves).
    fn take_owned(&mut self) -> Vec<u8> {
        match self {
            InputBytes::Owned(v) => core::mem::take(v),
            InputBytes::Borrowed(s) => s.to_vec(),
        }
    }
}

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
    /// The complete buffered data (after any encoding conversion to UTF-8),
    /// owned or borrowed (§16.5.2 — see [`InputBytes`]).
    data: InputBytes,
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
    /// Progressive source-decoding state (see [`SourceDecoding`]).
    decoding: SourceDecoding,
    /// Source bytes received from the caller that have **not** been
    /// materialized yet: undecoded because the encoding is still undecided
    /// (`SourceDecoding::Parked`), or because they form an incomplete
    /// encoding unit whose remainder has not arrived (UTF-16 half unit / lone
    /// high surrogate, UTF-32 partial unit). This is the decoder's carry: no
    /// chunk boundary is ever visible in the materialized stream, and no byte
    /// is decoded twice.
    pending_source: Vec<u8>,
    /// RAW source bytes received through `push_bytes` — the source-side leg
    /// of the complexity accounting. Materialized/consumed totals live beside
    /// the bytes they describe (`materialized`, `pos`).
    source_received: u64,
    /// UTF-8/internal bytes this buffer has materialized (the unit the
    /// scanner could ever consume). Backing `materialized_bytes()`;
    /// transcoding may expand (ISO-8859-1 `E9` -> `C3 A9`) or, on
    /// re-materialization of the whole buffer, be recomputed.
    materialized: u64,
    /// Absolute offset (into `data`) of the PHYSICAL window's base — the
    /// `_xmlParserInput::base` the ABI exposes. The buffer always keeps the
    /// whole decoded stream; this is metadata describing how much of it a
    /// physical window still covers, so the ABI can expose libxml2's
    /// rebasing behaviour without making that window authoritative for the
    /// parser.
    ///
    /// Mirrors upstream `xmlParserShrink` (parserInternals.c): when
    /// `cur - base > 4096` it keeps the last `LINE_LEN` (80) bytes as error
    /// context, physically removes the rest and adds the removed amount to
    /// `in->consumed`. Upstream then updates the input pointers, so for every
    /// input the invariant
    ///
    /// ```text
    /// consumed + (cur - base) == absolute_position
    /// ```
    ///
    /// holds — which is exactly how `xmlCtxtGetInputPosition` reconstructs the
    /// UTF-8 byte position.
    window_base_abs: usize,
    /// Bytes the DECODER consumed that are not represented in the materialized
    /// stream at all — today, the leading BOM of a fixed-width codec.
    ///
    /// Upstream counts those in `in->consumed` at the moment the encoding is
    /// switched, which is why `shadow-utf16invalid.xml` at b2 reports
    /// `c=0 p=0 abs=0` on the call that delivers only the BOM, and `c=2 p=0
    /// abs=2` on the next call: the two BOM bytes were consumed but never
    /// appear in the converted buffer, so they show up as consumed-only.
    /// `abs` is therefore the CONVERTED position plus this bias, exactly as
    /// `xmlCtxtGetInputPosition` reconstructs it.
    consumed_bias: usize,
    /// A definite invalid encoding unit was found (upstream
    /// `XML_ENC_ERR_INPUT`: an unpaired low surrogate, a high surrogate
    /// followed by a non-low unit, an out-of-range UCS-4 code point). Raised
    /// immediately on the call that finds it, like `xmlParserInputBufferPush`
    /// returning -1 -> `xmlCtxtErrIO`.
    encoding_error: bool,
    /// A TERMINATING call found an incomplete encoding unit still pending
    /// (upstream `xmlParserCheckEOF`'s `xmlCharEncInput(..., flush=1)` ->
    /// `XML_ENC_ERR_INPUT` -> `XML_ERR_INVALID_ENCODING`). Only surfaced when
    /// the document itself parsed cleanly — `xmlParserCheckEOF` returns early
    /// once `errNo` is set, so a malformed-document error wins.
    truncated_source: bool,
    /// Upstream `XML_INPUT_ENCODING_ERROR` (the `input->flags` bit set by
    /// `xmlCurrentChar`'s `encoding_error` arm): this input already reported an
    /// invalid UTF-8 byte sequence once. Only the FIRST malformed sequence in a
    /// character-data run is diagnosed; later ones are silently replaced by
    /// U+FFFD (`raw-invalid-utf8` at b1 shows one error for two bad bytes).
    utf8_error_reported: bool,
    /// Persistent `encoding_rs` decoder for a declared MULTIBYTE or STATEFUL
    /// source encoding (Shift_JIS, EUC-JP, ISO-2022-JP).
    ///
    /// Only those get one: single-byte sets (ISO-8859-x, windows-1252,
    /// EBCDIC) decode correctly tail-by-tail, and the fixed-width codecs
    /// (UTF-16, UCS-2, UCS-4) carry incomplete units in
    /// `pending_source` instead. A reparse duplicate always has `None` (see
    /// `duplicate_for_reparse`).
    registry: Option<RegistrySourceDecoder>,
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
            .field("len", &self.data.data_len())
            .field("bom_consumed", &self.bom_consumed)
            .field("converted_to_utf8", &self.converted_to_utf8)
            .field("decoding", &self.decoding)
            .field("pending_source", &self.pending_source.len())
            .field("registry", &self.registry.as_ref().map(|r| r.name))
            .finish()
    }
}

impl InputBuffer {
    // ── Constructors ───────────────────────────────────────────────────────

    /// Create an `InputBuffer` from an in-memory byte slice.
    ///
    /// The bytes are copied into an owned buffer (§16.5.2: this is the
    /// multi-phase / long-lived path — push parsing, caller-owned contexts,
    /// reader input — where the input must outlive the call). BOM and
    /// encoding detection are performed during construction.
    pub fn from_memory(buf: &[u8], uri: Option<&str>) -> Self {
        let data = buf.to_vec();
        let filename = uri.map(|s| s.to_string());
        let mut ib = InputBuffer {
            // The InputSource::Memory payload is never read back for a memory
            // buffer (read_chunk returns 0 and read_all is only used by the
            // file/callback constructors), so no mirror copy is kept (§16.5.2:
            // the previous `Memory(data.clone())` duplicated the whole input
            // on every memory parse).
            source: InputSource::Memory(Vec::new()),
            data: InputBytes::Owned(data),
            pos: 0,
            line: 1,
            col: 1,
            encoding: Encoding::None,
            filename,
            bom_consumed: false,
            converted_to_utf8: false,
            decl_pending: false,
            io_failed: false,
            decoding: SourceDecoding::WholeBuffer,
            pending_source: Vec::new(),
            source_received: 0,
            materialized: 0,
            consumed_bias: 0,
            window_base_abs: 0,
            encoding_error: false,
            utf8_error_reported: false,
            truncated_source: false,
            registry: None,
        };
        ib.detect_bom_and_encoding();
        ib
    }

    /// Create an `InputBuffer` for a push-parser session
    /// (`xmlCreatePushParserCtxt` / `xmlCtxtResetPush` and the `xmlParseChunk`
    /// fallback for a context with no stashed input).
    ///
    /// Unlike [`from_memory`](Self::from_memory), detection is PROGRESSIVE:
    /// the initial chunk is appended through the push path, so a short initial
    /// chunk parks exactly like upstream's `XML_PARSER_START` (`avail < 4`,
    /// and the EBCDIC signature before 200 bytes) instead of being sniffed as
    /// UTF-8 because the chunk happened to end early.
    pub fn for_push(initial: &[u8], uri: Option<&str>) -> Self {
        let mut ib = InputBuffer {
            source: InputSource::Memory(Vec::new()),
            data: InputBytes::Owned(Vec::new()),
            pos: 0,
            line: 1,
            col: 1,
            encoding: Encoding::None,
            filename: uri.map(|s| s.to_string()),
            bom_consumed: false,
            converted_to_utf8: false,
            decl_pending: false,
            io_failed: false,
            decoding: SourceDecoding::Parked,
            pending_source: Vec::new(),
            source_received: 0,
            materialized: 0,
            consumed_bias: 0,
            window_base_abs: 0,
            encoding_error: false,
            utf8_error_reported: false,
            truncated_source: false,
            registry: None,
        };
        ib.push_bytes_ex(initial, false);
        ib
    }

    /// Create an `InputBuffer` that BORROWS `buf` for the duration of the
    /// parse that consumes it — the §16.5.2 zero-copy path for the ordinary
    /// synchronous whole-buffer front-ends (`xmlReadMemory`, `xmlReadDoc`,
    /// `xmlSAXParseMemory`, …).
    ///
    /// No copy happens unless a later step needs owned bytes (a non-UTF-8
    /// BOM/declaration transcode or a caller encoding override converts to
    /// Owned inside the constructor/override, and push/reparse are never
    /// used by these front-ends).
    ///
    /// # SAFETY
    ///
    /// - `buf` must stay valid and unmoved until the InputBuffer is dropped.
    /// - The caller must guarantee the buffer is consumed within the same
    ///   synchronous parse call (these front-ends create, parse and free the
    ///   parser context inside one exported call, so the boxed buffer — and
    ///   the C-visible `_xmlParserInput` base/cur/end pointers into `buf` —
    ///   never outlive the call). Multi-phase APIs (`xmlCreateDocParserCtxt`,
    ///   `xmlCtxtReadMemory` on a caller-owned context, push parsing, the
    ///   reader) MUST use [`InputBuffer::from_memory`], which copies.
    pub unsafe fn from_memory_borrowed(buf: &[u8], uri: Option<&str>) -> Self {
        // SAFETY: see fn contract — the region outlives this buffer's use.
        let static_buf: &'static [u8] =
            unsafe { core::mem::transmute::<&[u8], &'static [u8]>(buf) };
        let filename = uri.map(|s| s.to_string());
        let mut ib = InputBuffer {
            source: InputSource::Memory(Vec::new()),
            data: InputBytes::Borrowed(static_buf),
            pos: 0,
            line: 1,
            col: 1,
            encoding: Encoding::None,
            filename,
            bom_consumed: false,
            converted_to_utf8: false,
            decl_pending: false,
            io_failed: false,
            decoding: SourceDecoding::WholeBuffer,
            pending_source: Vec::new(),
            source_received: 0,
            materialized: 0,
            consumed_bias: 0,
            window_base_abs: 0,
            encoding_error: false,
            utf8_error_reported: false,
            truncated_source: false,
            registry: None,
        };
        // BOM/declaration detection may transcode (Borrowed → Owned) for
        // non-UTF-8 inputs; the plain UTF-8/ASCII path stays zero-copy.
        ib.detect_bom_and_encoding();
        ib
    }

    /// Append raw bytes to the buffered input (push-parser mode). Upstream
    /// `xmlParseChunk` grows the parser input's base with each chunk; the
    /// candidate accumulates into the stashed buffer and parses on the
    /// terminating call (Phase-12 EXTERNAL-CONSUMERS court: parse4.c).
    ///
    /// This is the non-final entry point; see [`push_bytes_ex`](Self::push_bytes_ex)
    /// for the `terminate` flag, which is what distinguishes "a truncated
    /// encoding unit" (error) from "more input may follow" (suspension).
    pub fn push_bytes(&mut self, bytes: &[u8]) {
        self.push_bytes_ex(bytes, false);
    }

    /// Append raw bytes to the buffered input with the `xmlParseChunk`
    /// `terminate` flag.
    ///
    /// # Progressive source decoding (§16.7.8)
    ///
    /// A push-session buffer decodes its source incrementally, mirroring
    /// upstream's input-buffer encoder (`xmlParserInputBufferPush` ->
    /// `xmlCharEncInput`):
    ///
    /// - While the encoding is undecided, bytes are held in
    ///   [`pending_source`](Self::pending_source) and a non-final call parks at
    ///   the `XML_PARSER_START` gate (`avail < 4`; the EBCDIC signature
    ///   `4C 6F A7 94` waits for 200).
    /// - Once decided, each call decodes the complete encoding units it can and
    ///   keeps the incomplete trailing unit (odd UTF-16 byte, lone high
    ///   surrogate, partial UTF-32 unit) as carry for the next call — never a
    ///   whole-buffer re-decode, which would just move the replay parser's
    ///   O(N²) down one layer.
    /// - A definite invalid unit latches [`encoding_error`](Self::source_encoding_error)
    ///   on any call; an incomplete unit still pending on a TERMINATING call
    ///   latches [`truncated_source`](Self::source_truncated) (upstream
    ///   `xmlParserCheckEOF`'s flush).
    ///
    /// For a non-progressive input (memory/file/callback whole-buffer
    /// constructors) this keeps the historical behavior: the encoding was
    /// decided at construction, and only the declaration re-sniff (KEY-1) may
    /// still transcode.
    pub(crate) fn push_bytes_ex(&mut self, bytes: &[u8], terminate: bool) {
        self.source_received = self.source_received.saturating_add(bytes.len() as u64);
        match self.decoding {
            SourceDecoding::WholeBuffer => {
                if self.data.is_empty() && self.pos == 0 && !self.bom_consumed {
                    // A whole-buffer input being pushed to with nothing yet
                    // buffered (the `xmlParseChunk` fallback for a context
                    // whose input was never set up for push): begin
                    // progressive decoding from scratch.
                    self.decoding = SourceDecoding::Parked;
                    self.push_progressive(bytes, terminate);
                } else {
                    // A push onto an already-materialized whole-buffer input:
                    // the encoding was decided at construction, so continue
                    // incrementally from here (upstream `xmlParseChunk` on a
                    // memory/IO parser context).
                    self.decoding = SourceDecoding::Decided;
                    self.append_decided(bytes, terminate);
                }
            }
            SourceDecoding::Parked => self.push_progressive(bytes, terminate),
            SourceDecoding::Decided => self.append_decided(bytes, terminate),
        }
    }

    /// Hold `bytes` back until the source encoding can be decided; decide now
    /// when this call gives upstream's `XML_PARSER_START` enough evidence.
    fn push_progressive(&mut self, bytes: &[u8], terminate: bool) {
        self.pending_source.extend_from_slice(bytes);
        if self.try_decide(terminate) {
            self.decoding = SourceDecoding::Decided;
        }
    }

    /// Upstream `xmlParseTryOrFinish`'s `XML_PARSER_START` gates:
    ///
    /// ```c
    /// if ((!terminate) && (avail < 4))
    ///     goto done;
    /// if ((CMP4(CUR_PTR, 0x4C, 0x6F, 0xA7, 0x94)) && (!terminate) && (avail < 200))
    ///     goto done;
    /// ```
    ///
    /// `avail` here is the number of undecided source bytes held back.
    fn try_decide(&mut self, terminate: bool) -> bool {
        let avail = self.pending_source.len();
        if !terminate {
            if avail < 4 {
                return false;
            }
            if self.pending_source[..4] == [0x4C, 0x6F, 0xA7, 0x94] && avail < 200 {
                return false;
            }
        }
        self.decide(terminate);
        true
    }

    /// Run upstream `xmlDetectEncoding` over the held source bytes and install
    /// the matching decoder.
    fn decide(&mut self, terminate: bool) {
        let src = core::mem::take(&mut self.pending_source);
        let at = |i: usize| src.get(i).copied().unwrap_or(0);
        let n = src.len();

        // Order mirrors `detect_bom_and_encoding` / upstream xmlDetectEncoding:
        // the 4-byte patterns first (the UTF-32LE BOM begins with the UTF-16LE
        // BOM, so it must be tested before it), then the 2/3-byte BOMs.
        if n >= 4 && at(0) == 0x3C && at(1) == 0x00 && at(2) == 0x00 && at(3) == 0x00 {
            self.install_unit_decoder(src, Encoding::Ucs4Le, 0, terminate);
        } else if n >= 4 && at(0) == 0x00 && at(1) == 0x00 && at(2) == 0x00 && at(3) == 0x3C {
            self.install_unit_decoder(src, Encoding::Ucs4Be, 0, terminate);
        } else if n >= 4 && at(0) == 0x3C && at(1) == 0x00 && at(2) == 0x3F && at(3) == 0x00 {
            self.install_unit_decoder(src, Encoding::Utf16Le, 0, terminate);
        } else if n >= 4 && at(0) == 0x00 && at(1) == 0x3C && at(2) == 0x00 && at(3) == 0x3F {
            self.install_unit_decoder(src, Encoding::Utf16Be, 0, terminate);
        } else if n >= 4 && at(0) == 0x4C && at(1) == 0x6F && at(2) == 0xA7 && at(3) == 0x94 {
            // EBCDIC signature: the whole stream is decoded through the
            // registered IBM037 handler (single-byte, so tail-wise decoding
            // needs no carry).
            self.encoding = Encoding::Ebcdic;
            self.data = InputBytes::Owned(src);
            self.convert_declared_native_encoding(terminate);
            self.materialized = self.data.data_len() as u64;
        } else if n >= 3 && at(0) == 0xEF && at(1) == 0xBB && at(2) == 0xBF {
            self.decide_utf8(src, 3, terminate);
        } else if n >= 2 && at(0) == 0xFF && at(1) == 0xFE {
            // `FF FE 00 00` (the UTF-32LE BOM) deliberately lands here:
            // upstream has no UTF-32 BOM case, so it is a UTF-16LE BOM whose
            // first decoded character is U+0000 (see detect_bom_and_encoding).
            self.install_unit_decoder(src, Encoding::Utf16Le, 2, terminate);
        } else if n >= 2 && at(0) == 0xFE && at(1) == 0xFF {
            self.install_unit_decoder(src, Encoding::Utf16Be, 2, terminate);
        } else {
            self.decide_utf8(src, 0, terminate);
        }
    }

    /// Install `enc` and decode `src[skip..]` as whole code units, keeping an
    /// incomplete trailing unit pending.
    fn install_unit_decoder(&mut self, src: Vec<u8>, enc: Encoding, skip: usize, terminate: bool) {
        self.encoding = enc;
        self.converted_to_utf8 = true;
        self.data = InputBytes::Owned(Vec::new());
        self.materialized = 0;
        // A leading BOM is consumed and not materialized (upstream advances
        // `input->cur` past it before switching the encoder).
        self.pos = 0;
        self.window_base_abs = 0;
        self.consumed_bias = 0;
        self.col = 1;
        self.pending_source = src[skip..].to_vec();
        // The skipped BOM is consumed-but-never-materialized: upstream counts
        // it in `in->consumed` at the encoding switch (see
        // `Self::consumed_bias`).
        self.consumed_bias = skip;
        self.decode_units_tail(terminate);
    }

    /// Materialize a UTF-8 source (optionally after a `skip`-byte BOM) and run
    /// the declaration sniff so a declared legacy encoding still transcodes
    /// (KEY-1). `terminate` is the `xmlParseChunk` final flag: a declared
    /// multibyte/stateful codec must know whether this is the last feed.
    fn decide_utf8(&mut self, src: Vec<u8>, bom: usize, terminate: bool) {
        self.encoding = Encoding::Utf8;
        self.materialized = src.len() as u64;
        self.data = InputBytes::Owned(src);
        if bom > 0 {
            self.pos = bom;
            self.col = bom + 1;
            self.bom_consumed = true;
        }
        self.detect_encoding_from_xml_declaration();
        self.convert_declared_native_encoding(terminate);
    }

    /// Decode as many complete source units as possible from
    /// [`pending_source`](Self::pending_source) into the materialized buffer,
    /// leaving an incomplete trailing unit pending for the next call.
    fn decode_units_tail(&mut self, terminate: bool) {
        let src = core::mem::take(&mut self.pending_source);
        let mut out: Vec<u8> = Vec::with_capacity(src.len());
        let mut consumed = 0usize;
        let definite_error = match self.encoding {
            Encoding::Utf16Le => decode_utf16_units(&src, false, &mut out, &mut consumed),
            Encoding::Utf16Be => decode_utf16_units(&src, true, &mut out, &mut consumed),
            // UCS-2 is 2-byte host-order (little-endian) in the registry;
            // UCS-4 is 4-byte. Both keep a partial trailing unit as carry.
            Encoding::Ucs2Le => decode_fixed_units(&src, false, 2, &mut out, &mut consumed),
            Encoding::Ucs4Le => decode_fixed_units(&src, false, 4, &mut out, &mut consumed),
            Encoding::Ucs4Be => decode_fixed_units(&src, true, 4, &mut out, &mut consumed),
            _ => return,
        };
        if !out.is_empty() {
            self.data.make_owned().extend_from_slice(&out);
            self.materialized = self.materialized.saturating_add(out.len() as u64);
        }
        self.pending_source = src[consumed..].to_vec();
        if definite_error {
            self.encoding_error = true;
        } else if terminate && !self.pending_source.is_empty() {
            // `xmlParserCheckEOF`: a terminating call flushes the encoder and a
            // still-incomplete unit is a truncated sequence.
            self.truncated_source = true;
        }
    }

    /// Append to an input whose source encoding is already decided.
    fn append_decided(&mut self, bytes: &[u8], terminate: bool) {
        match &self.encoding {
            Encoding::Utf16Le
            | Encoding::Utf16Be
            | Encoding::Ucs2Le
            | Encoding::Ucs4Le
            | Encoding::Ucs4Be
                if self.converted_to_utf8 =>
            {
                self.pending_source.extend_from_slice(bytes);
                self.decode_units_tail(terminate);
            }
            Encoding::Other(_) if self.converted_to_utf8 => {
                if let Some(reg) = self.registry.as_mut() {
                    // Multibyte/stateful codec: the SAME decoder instance
                    // continues, so an incomplete unit and (ISO-2022-JP) the
                    // escape state survive the chunk boundary. `terminate` is
                    // passed through as `encoding_rs`'s `last`: only a
                    // terminating feed treats a held incomplete unit as the
                    // encoder flush error.
                    let mut conv = Vec::new();
                    let res = reg.decode(bytes, terminate, &mut conv);
                    if !conv.is_empty() {
                        self.data.make_owned().extend_from_slice(&conv);
                        self.materialized = self.materialized.saturating_add(conv.len() as u64);
                    }
                    match res {
                        // A definite invalid unit: raise on this call, like
                        // `xmlParserInputBufferPush` returning -1.
                        Err(RegistryTail::Invalid) => self.encoding_error = true,
                        // Incomplete unit on a terminating call: the encoder
                        // flush, surfaced only if the document parses cleanly.
                        Err(RegistryTail::Truncated) => self.truncated_source = true,
                        Ok(()) => {}
                    }
                    return;
                }
                if bytes.is_empty() {
                    return;
                }
                // No persistent decoder: a reparse duplicate (never pushed to)
                // or a registry name with no `encoding_rs` codec. Decode the
                // tail through the registry handler as before.
                match self.legacy_source_encoding_name() {
                    Some(src_name) => {
                        match crate::xml::encoding::decode_whole_buffer_declared(&src_name, bytes) {
                            Ok(conv) => {
                                self.data.make_owned().extend_from_slice(&conv);
                                self.materialized =
                                    self.materialized.saturating_add(conv.len() as u64);
                            }
                            // Undecodable tail: append raw; the tokenizer
                            // reports the invalid-character error.
                            Err(()) => self.append_raw(bytes),
                        }
                    }
                    None => self.append_raw(bytes),
                }
            }
            Encoding::Iso8859_1 | Encoding::Ebcdic if self.converted_to_utf8 => {
                if bytes.is_empty() {
                    return;
                }
                // Single-byte sets: every byte is an independent source unit,
                // so a tail-wise decode through the registered handler is
                // exact (no carry, no state).
                match self.legacy_source_encoding_name() {
                    Some(src_name) => {
                        match crate::xml::encoding::decode_whole_buffer_declared(&src_name, bytes) {
                            Ok(conv) => {
                                self.data.make_owned().extend_from_slice(&conv);
                                self.materialized =
                                    self.materialized.saturating_add(conv.len() as u64);
                            }
                            // Undecodable tail: append raw; the tokenizer
                            // reports the invalid-character error.
                            Err(()) => self.append_raw(bytes),
                        }
                    }
                    None => self.append_raw(bytes),
                }
            }
            _ => {
                // UTF-8 family (UTF-8/ASCII, or a declaration that has not
                // named a legacy encoding yet). The raw source bytes are the
                // materialized bytes; only the still-pending `<?xml ...?>'
                // sniff can switch the encoding (KEY-1).
                self.append_raw(bytes);
                if bytes.is_empty() {
                    return;
                }
                if self.decl_pending && !self.converted_to_utf8 {
                    self.detect_encoding_from_xml_declaration();
                    self.convert_declared_native_encoding(terminate);
                    if self.converted_to_utf8 {
                        self.materialized = self.data.data_len() as u64;
                    }
                }
            }
        }
    }

    /// Append raw source bytes to the materialized stream.
    fn append_raw(&mut self, bytes: &[u8]) {
        self.data.make_owned().extend_from_slice(bytes);
        self.materialized = self.materialized.saturating_add(bytes.len() as u64);
    }

    /// Fold any held-back source bytes into the raw buffer, so a caller
    /// encoding override sees the whole stream (upstream switches the encoding
    /// before the parse and before any detection).
    fn absorb_pending_source(&mut self) {
        if self.pending_source.is_empty() {
            return;
        }
        let held = core::mem::take(&mut self.pending_source);
        self.materialized = self.materialized.saturating_add(held.len() as u64);
        self.data.make_owned().extend_from_slice(&held);
        if self.decoding == SourceDecoding::Parked {
            self.decoding = SourceDecoding::Decided;
        }
    }

    // ── Progressive decoding state queries ───────────────────────────────

    /// The progressive source-decoding state.
    pub(crate) const fn source_decoding(&self) -> SourceDecoding {
        self.decoding
    }

    /// Whether this input is parked in upstream `XML_PARSER_START`: a
    /// non-final call with such an input must parse nothing and fire nothing.
    pub(crate) fn source_parked(&self) -> bool {
        self.decoding == SourceDecoding::Parked
    }

    /// Whether the decoder found a definite invalid encoding unit
    /// (`XML_ENC_ERR_INPUT`) — reported on the call that found it.
    pub(crate) const fn source_encoding_error(&self) -> bool {
        self.encoding_error
    }

    /// Whether a terminating call left an incomplete encoding unit pending
    /// (upstream `xmlParserCheckEOF`'s encoder flush).
    pub(crate) const fn source_truncated(&self) -> bool {
        self.truncated_source
    }

    /// Whether this input already reported an invalid UTF-8 byte sequence
    /// (upstream `XML_INPUT_ENCODING_ERROR`).
    pub(crate) const fn utf8_error_reported(&self) -> bool {
        self.utf8_error_reported
    }

    /// Set `XML_INPUT_ENCODING_ERROR`: the FIRST malformed sequence in a
    /// character-data run has been reported.
    pub(crate) const fn set_utf8_error_reported(&mut self) {
        self.utf8_error_reported = true;
    }

    /// RAW source bytes received through `push_bytes`.
    pub(crate) const fn source_bytes_received(&self) -> u64 {
        self.source_received
    }

    /// UTF-8/internal bytes materialized so far — the only unit the scanner
    /// could ever consume.
    pub(crate) const fn materialized_bytes(&self) -> u64 {
        self.materialized
    }

    /// Absolute offset of the physical window's base (see
    /// [`Self::window_base_abs`]).
    pub(crate) const fn window_base_abs(&self) -> usize {
        self.window_base_abs
    }

    /// Bytes consumed by the decoder but absent from the materialized stream
    /// (see [`Self::consumed_bias`]).
    pub(crate) const fn consumed_bias(&self) -> usize {
        self.consumed_bias
    }

    /// Move the cursor to an absolute byte offset with an explicit 1-based
    /// line/column, for raising a diagnostic at its own position.
    ///
    /// The push driver scans a whole construct and then flushes the tokenizer's
    /// queued diagnostics; upstream raises each one inline, so `input->cur` at a
    /// structured-error callback sits at the raise point rather than at the end
    /// of the construct. The caller restores the position afterwards.
    pub(crate) fn set_diagnostic_position(&mut self, byte_pos: usize, line: usize, col: usize) {
        self.pos = byte_pos.min(self.data.data_len());
        self.line = line.max(1);
        self.col = col.max(1);
    }

    /// Move the cursor backward in COLUMN bookkeeping only.
    ///
    /// Upstream's `xmlParseEntityValue` consumes an entity value's OPENING
    /// QUOTE with a raw `CUR_PTR++`, which advances `cur` WITHOUT bumping
    /// `col` (every other consumer goes through NEXT / NEXTL / SKIP, which do).
    /// The push driver's DTD scan is slice-based, so it consumes the subset
    /// uniformly and then subtracts the quotes here to keep `input->col`
    /// byte-identical to the oracle's through a DTD entity value.
    pub(crate) fn adjust_col_back(&mut self, n: usize) {
        self.col = self.col.saturating_sub(n);
    }

    /// The physical window's used length: `cur - base` in ABI terms.
    pub(crate) const fn window_used(&self) -> usize {
        self.pos.saturating_sub(self.window_base_abs)
    }

    /// Perform the upstream-equivalent `xmlParserShrink` step.
    ///
    /// Called where upstream calls it — at the top of a progressive pass — so
    /// the ABI-visible `cur - base` follows the same trajectory as libxml2's.
    /// The Rust stream is NOT truncated: only the window metadata moves, so
    /// the absolute cursor, decoder state and every absolute offset
    /// (`ParkedConstruct::checked`) are untouched. Returns whether it moved.
    pub(crate) fn shrink_window(&mut self) -> bool {
        const SHRINK_THRESHOLD: usize = 4096; // upstream's `cur - base > 4096`
        const LINE_LEN: usize = 80; // upstream LINE_LEN
        let used = self.window_used();
        if used <= SHRINK_THRESHOLD {
            return false;
        }
        // The removed amount is `used - LINE_LEN`, and `consumed` advances by
        // exactly that (the identity is preserved by construction).
        self.window_base_abs = self.pos - LINE_LEN;
        true
    }

    /// The canonical name of the installed persistent registry decoder, if any
    /// (diagnostic / test surface).
    pub(crate) fn registry_kind_name(&self) -> Option<&'static str> {
        self.registry.as_ref().map(|r| r.name)
    }

    /// Source bytes received but not yet materialized (see
    /// [`pending_source`](Self::pending_source)).
    pub(crate) fn pending_source(&self) -> &[u8] {
        &self.pending_source
    }

    /// `(line, col)` at the end of the materialized stream, with upstream's
    /// line-break semantics (`\r\n` counts once). Used to report an error at
    /// the end of the decoded input (the encoder flush position).
    pub(crate) fn end_line_col(&self) -> (usize, usize) {
        let bytes: &[u8] = &self.data;
        let mut line = 1usize;
        let mut col = 1usize;
        let mut i = 0usize;
        while i < bytes.len() {
            match bytes[i] {
                b'\n' => {
                    line += 1;
                    col = 1;
                    i += 1;
                }
                b'\r' => {
                    if i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
                        i += 2;
                    } else {
                        i += 1;
                    }
                    line += 1;
                    col = 1;
                }
                b if b < 0x80 => {
                    col += 1;
                    i += 1;
                }
                b => {
                    col += 1;
                    i += Self::utf8_char_len(b);
                }
            }
        }
        (line, col)
    }

    /// Produce an independent copy of this buffer at its current state, so
    /// the same accumulated input can be parsed more than once (incremental
    /// push probe/delivery: `helpers::parse_chunk` runs a silent completeness
    /// probe and, when the accumulated input is a complete document, a
    /// completing parse over an identical buffer).
    pub(crate) fn duplicate_for_reparse(&self) -> InputBuffer {
        InputBuffer {
            // A reparse duplicate always OWNS its bytes (the original may be
            // a §16.5.2 borrowed buffer whose region is call-scoped).
            source: InputSource::Memory(Vec::new()),
            data: InputBytes::Owned(self.data.to_vec()),
            pos: self.pos,
            line: self.line,
            col: self.col,
            encoding: self.encoding.clone(),
            filename: self.filename.clone(),
            bom_consumed: self.bom_consumed,
            converted_to_utf8: self.converted_to_utf8,
            decl_pending: self.decl_pending,
            io_failed: self.io_failed,
            decoding: self.decoding,
            pending_source: self.pending_source.clone(),
            source_received: self.source_received,
            materialized: self.materialized,
            consumed_bias: self.consumed_bias,
            window_base_abs: self.window_base_abs,
            encoding_error: self.encoding_error,
            utf8_error_reported: self.utf8_error_reported,
            truncated_source: self.truncated_source,
            // A reparse duplicate is a PARSE-ONLY artifact: it exists so the
            // accumulated materialized stream can be parsed again (probe /
            // delivery / terminating pass) and is never pushed to. Its source
            // decoder is therefore not carried across (`encoding_rs::Decoder`
            // is not `Clone`, and its incomplete-unit / shift state is not
            // reconstructible from `pending_source`); if one ever were pushed
            // to, `append_decided` degrades to the per-tail registry decode.
            registry: None,
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
            data: InputBytes::Owned(data),
            pos: 0,
            line: 1,
            col: 1,
            encoding: Encoding::None,
            filename,
            bom_consumed: false,
            converted_to_utf8: false,
            decl_pending: false,
            io_failed: false,
            decoding: SourceDecoding::WholeBuffer,
            pending_source: Vec::new(),
            source_received: 0,
            materialized: 0,
            consumed_bias: 0,
            window_base_abs: 0,
            encoding_error: false,
            utf8_error_reported: false,
            truncated_source: false,
            registry: None,
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
            data: InputBytes::Owned(data),
            pos: 0,
            line: 1,
            col: 1,
            encoding: Encoding::None,
            filename: None,
            bom_consumed: false,
            converted_to_utf8: false,
            decl_pending: false,
            io_failed: false,
            decoding: SourceDecoding::WholeBuffer,
            pending_source: Vec::new(),
            source_received: 0,
            materialized: 0,
            consumed_bias: 0,
            window_base_abs: 0,
            encoding_error: false,
            utf8_error_reported: false,
            truncated_source: false,
            registry: None,
        };
        ib.detect_bom_and_encoding();
        Ok(ib)
    }

    /// A failed input source (I/O error at open time).
    /// Create an `InputBuffer` whose source failed to produce data (read
    /// callback returned an error). The parser raises an I/O error at the
    /// first grow — mirroring upstream — instead of reporting an empty
    /// document.
    pub const fn failed_source() -> Self {
        InputBuffer {
            source: InputSource::Memory(Vec::new()),
            data: InputBytes::Owned(Vec::new()),
            pos: 0,
            line: 1,
            col: 1,
            encoding: Encoding::Utf8,
            filename: None,
            bom_consumed: false,
            converted_to_utf8: false,
            decl_pending: false,
            io_failed: true,
            decoding: SourceDecoding::WholeBuffer,
            pending_source: Vec::new(),
            source_received: 0,
            materialized: 0,
            consumed_bias: 0,
            window_base_abs: 0,
            encoding_error: false,
            utf8_error_reported: false,
            truncated_source: false,
            registry: None,
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

        // NO UTF-32/UCS-4 BOM TEST: neither upstream `xmlDetectEncoding`
        // (parserInternals.c, the parser's own detection) nor
        // `xmlDetectCharEncoding` (encoding.c, appendix F) recognizes
        // `FF FE 00 00` or `00 00 FE FF`. `FF FE 00 00` therefore falls
        // through to the UTF-16LE BOM below (first decoded char U+0000) and
        // `00 00 FE FF` falls through to the default UTF-8 path — both
        // observed in the phase-16.7.8 push court (`enc-utf32le-bom` /
        // `enc-utf32be-bom`: the oracle reports "Start tag expected, '<' not
        // found", rc 4). BOM-LESS UCS-4 is still detected by the `3C 00 00 00`
        // / `00 00 00 3C` patterns below.

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
                self.convert_declared_native_encoding(true);
                return;
            }
            if d0 == 0x00 && d1 == 0x00 && d2 == 0x00 && d3 == 0x3C {
                self.encoding = Encoding::Ucs4Be;
                self.convert_declared_native_encoding(true);
                return;
            }
            if d0 == 0x4C && d1 == 0x6F && d2 == 0xA7 && d3 == 0x94 {
                self.encoding = Encoding::Ebcdic;
                self.convert_declared_native_encoding(true);
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
        //
        // `terminate = true`: a whole-buffer input IS the complete stream, so a
        // declared codec's trailing incomplete unit is a truncation.
        self.convert_declared_native_encoding(true);
    }

    /// Transcode `data` to UTF-8 when the XML declaration named an encoding
    /// the crate has a converter for, or the first bytes pattern-detected a
    /// non-ASCII-compatible encoding (UCS-4/EBCDIC). ISO-8859-1 is a
    /// byte-wise mapping (every byte 0x80..=0xFF becomes a two-byte UTF-8
    /// sequence, all ASCII stays identical — including the declaration
    /// itself), so the whole buffered stream converts safely regardless of
    /// how much has arrived.
    ///
    /// The registry-served encodings are NOT one homogeneous category — see
    /// [`RegistryKind`]: the single-byte sets (ISO-8859-2..16, windows-1252,
    /// EBCDIC) decode whole-buffer through their registered handler because
    /// every byte is an independent source unit; UCS-2/UCS-4 materialize
    /// through the code-unit decoder so a trailing incomplete unit is CARRIED
    /// rather than rejected; and the multibyte/stateful codecs (Shift_JIS,
    /// EUC-JP, ISO-2022-JP) install a PERSISTENT `encoding_rs::Decoder` (a
    /// per-chunk decode would lose an incomplete unit, and for ISO-2022-JP
    /// the escape state itself, which no byte carry can reconstruct).
    ///
    /// Unknown encodings are left untouched so the existing
    /// unsupported-encoding handling applies unchanged.
    fn convert_declared_native_encoding(&mut self, terminate: bool) {
        if self.converted_to_utf8 {
            return;
        }
        match &self.encoding {
            Encoding::Iso8859_1 => {
                // take_owned: an Owned buffer moves its Vec out (no copy); a
                // Borrowed buffer copies once, then becomes Owned (§16.5.2).
                let raw = self.data.take_owned();
                self.data = InputBytes::Owned(crate::xml::encoding::latin1_to_utf8(&raw));
                self.converted_to_utf8 = true;
            }
            Encoding::Ascii => {
                // US-ASCII is a strict UTF-8 subset: no transcode, but latch so
                // incremental pushes stop re-detecting.
                self.converted_to_utf8 = true;
            }
            Encoding::Other(name) => {
                let bytes = name.clone().into_bytes();
                match registry_kind(&bytes) {
                    RegistryKind::Streaming(enc, canon) => {
                        self.install_registry_decoder(enc, canon, terminate)
                    }
                    RegistryKind::FixedWidth(fixed) => {
                        self.convert_declared_units(fixed, terminate)
                    }
                    // Byte-wise decodes per tail exactly, and an UNCLASSIFIED
                    // codec keeps that same behavior (see RegistryKind) — the
                    // exhaustiveness test is what forbids the latter for the
                    // current registry.
                    RegistryKind::ByteWise | RegistryKind::Unclassified => {
                        self.convert_via_registry(&bytes)
                    }
                }
            }
            Encoding::Ebcdic => self.convert_via_registry(b"IBM037"),
            Encoding::Ucs4Le => self.convert_via_registry(b"UCS-4LE"),
            Encoding::Ucs4Be => self.convert_via_registry(b"UCS-4BE"),
            Encoding::Ucs2Le => self.convert_declared_units(Encoding::Ucs2Le, terminate),
            // A DECLARED UTF-16 (the BOM/pattern paths return early above,
            // with `converted_to_utf8` already set) has fixed-width units too.
            Encoding::Utf16Le => self.convert_declared_units(Encoding::Utf16Le, terminate),
            Encoding::Utf16Be => self.convert_declared_units(Encoding::Utf16Be, terminate),
            _ => {}
        }
    }

    /// Materialize a declared fixed-width source encoding (UCS-2, UCS-4 or a
    /// declared UTF-16) through the code-unit decoder, so a trailing incomplete
    /// unit is held in `pending_source` for the next call instead of being
    /// rejected by a whole-buffer registry decode (`fixed_width_input` reports
    /// the complete prefix and leaves the tail, which
    /// `decode_whole_buffer_declared` turns into a hard `Err`).
    ///
    /// The WHOLE held buffer is decoded, starting at byte zero — INCLUDING the
    /// declaration that named the encoding. That looks wrong (the declaration
    /// was read as ASCII and is not in the declared encoding), and for a
    /// unit-aligned codec it genuinely mangles the declaration; but it is what
    /// upstream does: `xmlSwitchEncoding` converts the input buffer, and the
    /// oracle's own result for a declared UCS-2/UTF-16 stream in an ASCII
    /// declaration is a REJECTION (phase-16.7.8 court `enc-ucs2*`:
    /// `XML_ERR_SPACE_REQUIRED` 65 + `XML_ERR_'?>' expected` 57, wellFormed 0).
    /// For the ASCII-compatible declared codecs (Shift_JIS, EUC-JP,
    /// ISO-2022-JP, the ISO-8859 family) this is a no-op, which is why they
    /// are unaffected.
    fn convert_declared_units(&mut self, canonical: Encoding, terminate: bool) {
        if self.converted_to_utf8 || self.data.is_empty() {
            return;
        }
        let raw = self.data.take_owned();
        self.encoding = canonical;
        self.converted_to_utf8 = true;
        self.data = InputBytes::Owned(Vec::new());
        self.materialized = 0;
        self.pos = 0;
        self.window_base_abs = 0;
        self.consumed_bias = 0;
        self.col = 1;
        self.bom_consumed = false;
        self.pending_source = raw;
        // A trailing partial unit is carried on a non-final feed (a progressive
        // stream may still complete it) and is the encoder flush error on a
        // terminating one — exactly as in the BOM/pattern-detected fixed-width
        // paths.
        self.decode_units_tail(terminate);
    }

    /// Install a persistent `encoding_rs` decoder for a declared multibyte or
    /// stateful encoding, materializing the source bytes held so far (the
    /// single O(N) conversion at decision time; every later chunk decodes
    /// incrementally through the same decoder instance).
    ///
    /// `terminate` is the deciding call's `xmlParseChunk` final flag: a
    /// declared codec that is still holding an incomplete unit when the first
    /// (and only) feed is final is the encoder flush error, exactly as on the
    /// BOM/pattern-detected paths.
    fn install_registry_decoder(
        &mut self,
        enc: &'static encoding_rs::Encoding,
        name: &'static str,
        terminate: bool,
    ) {
        if self.converted_to_utf8 {
            return;
        }
        let raw = self.data.take_owned();
        let mut decoder = RegistrySourceDecoder::new(enc, name);
        let mut out = Vec::with_capacity(raw.len() + 8);
        match decoder.decode(&raw, terminate, &mut out) {
            Ok(()) => {
                self.data = InputBytes::Owned(out);
                self.materialized = self.data.data_len() as u64;
                self.pos = 0;
                self.window_base_abs = 0;
                self.consumed_bias = 0;
                self.col = 1;
                self.bom_consumed = false;
                self.converted_to_utf8 = true;
                self.registry = Some(decoder);
            }
            Err(tail) => {
                if self.decoding == SourceDecoding::WholeBuffer {
                    // Whole-buffer front-end (`xmlReadMemory` & friends): keep
                    // the raw bytes and leave the encoding as UTF-8 so the
                    // TOKENIZER reports the invalid byte ("Invalid bytes in
                    // character encoding") — the same shape as
                    // `convert_via_registry`'s failure path. These callers
                    // never consult the progressive encoder latches.
                    self.data = InputBytes::Owned(raw);
                    self.materialized = self.data.data_len() as u64;
                    self.encoding = Encoding::Utf8;
                    self.pos = 0;
                    self.window_base_abs = 0;
                    self.consumed_bias = 0;
                    self.col = 1;
                    self.bom_consumed = false;
                    self.converted_to_utf8 = false;
                    self.registry = None;
                } else {
                    // Progressive feed: the prefix BEFORE the offending unit
                    // decoded, and upstream's failed `xmlParserInputBufferPush`
                    // means the parser does not run on this call at all — so
                    // materialize the decoded prefix and latch the encoder
                    // error, which `parse_chunk` turns into
                    // XML_ERR_INVALID_ENCODING (or defers to
                    // `xmlParserCheckEOF` when the feed was final).
                    self.data = InputBytes::Owned(out);
                    self.materialized = self.data.data_len() as u64;
                    self.pos = 0;
                    self.window_base_abs = 0;
                    self.consumed_bias = 0;
                    self.col = 1;
                    self.bom_consumed = false;
                    self.converted_to_utf8 = true;
                    self.registry = Some(decoder);
                    match tail {
                        RegistryTail::Invalid => self.encoding_error = true,
                        RegistryTail::Truncated => self.truncated_source = true,
                    }
                }
            }
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
                // The converted stream is always owned (§16.5.2).
                self.data = InputBytes::Owned(conv);
                self.pos = 0;
                self.window_base_abs = 0;
                self.consumed_bias = 0;
                self.col = 1;
                self.bom_consumed = false;
                self.converted_to_utf8 = true;
            }
            Err(()) => {
                self.encoding = Encoding::Utf8;
                self.pos = 0;
                self.window_base_abs = 0;
                self.consumed_bias = 0;
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
        // Read-only decode source; the buffer itself stays Borrowed on
        // failure and becomes Owned(conv) on success (§16.5.2).
        let raw = self.data.to_vec();
        let converted = match self.encoding {
            Encoding::Utf16Le => crate::xml::encoding::utf16le_to_utf8(&raw),
            Encoding::Utf16Be => crate::xml::encoding::utf16be_to_utf8(&raw),
            _ => return,
        };
        match converted {
            Ok(conv) => {
                self.data = InputBytes::Owned(conv);
                self.pos = 0;
                self.window_base_abs = 0;
                self.consumed_bias = 0;
                self.col = 1;
                self.bom_consumed = false;
                self.converted_to_utf8 = true;
            }
            Err(()) => {
                // Undecodable input: keep the raw bytes; the tokenizer will
                // report the invalid-character error like upstream.
                self.encoding = Encoding::Utf8;
                self.pos = 0;
                self.window_base_abs = 0;
                self.consumed_bias = 0;
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
        // A caller override is upstream `xmlSwitchEncoding` before the parse:
        // it precedes any detection, so held-back (undecided) source bytes
        // must be visible to it.
        self.absorb_pending_source();
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
            "us-ascii" | "ascii" => Some(self.data.to_vec()),
            _ => None,
        };
        match converted {
            Some(conv) => {
                self.data = InputBytes::Owned(conv);
                self.pos = 0;
                self.window_base_abs = 0;
                self.consumed_bias = 0;
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
        // See apply_name_encoding_override: an explicit encoding precedes all
        // detection, so undecided source bytes must be visible here.
        self.absorb_pending_source();
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
            | crate::abi::types::xmlCharEncoding::XML_CHAR_ENCODING_UTF8 => {
                Some(self.data.to_vec())
            }
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
                self.data = InputBytes::Owned(conv);
                self.pos = 0;
                self.window_base_abs = 0;
                self.consumed_bias = 0;
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
        self.advance_past_char();
        Some(c)
    }

    /// §16.5.5 decode-once: advance past the character at the current
    /// position WITHOUT decoding it. The caller must have just peeked the
    /// same character via [`peek_char`](Self::peek_char) with no intervening
    /// mutation, so the full UTF-8 decode has already happened once — this
    /// consume only needs the leading-byte length (position/line/col
    /// semantics are byte-for-byte identical to `read_char`). Hot scanner
    /// loops that peek-then-consume per character (text runs, names,
    /// whitespace, attribute values) must use this instead of a second
    /// `read_char` decode.
    pub fn consume_peeked(&mut self) {
        debug_assert!(self.pos < self.data.len(), "consume_peeked after EOF");
        self.advance_past_char();
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

    /// §16.5.6: advance `n` bytes that the caller has bulk-scanned and
    /// proven free of line breaks (`\n`/`\r` — a printable-ASCII text run),
    /// bumping the column once per byte (each byte is one character here).
    /// The caller guarantees `self.pos + n <= self.data.len()`.
    pub(crate) fn skip_linebreak_free(&mut self, n: usize) {
        debug_assert!(self.pos + n <= self.data.len());
        self.pos += n;
        self.col += n;
    }

    /// §16.6 scalar engine: consume a run of ASCII whitespace (space, tab,
    /// CR, LF, form feed) with line/column semantics IDENTICAL to upstream
    /// `SKIP_BLANKS` / `xmlSkipBlankChars` — only a literal LF starts a new
    /// line; EVERY other blank (including a lone CR, which upstream never
    /// converts here) advances the column by one. A CRLF pair therefore still
    /// ends up as one line break (CR: col++, LF: line++), but a lone CR does
    /// NOT: `text-cr-chunkend` drains a trailing `\r` in EPILOG with
    /// `col = 6, line = 3`, not `col = 1, line = 4`. Returns the bytes
    /// consumed (0 when the current byte is not whitespace). Stops at the
    /// first non-whitespace byte or EOF; never decodes.
    pub(crate) fn skip_ascii_whitespace(&mut self) -> usize {
        let start = self.pos;
        while self.pos < self.data.len() {
            match self.data[self.pos] {
                b'\n' => {
                    self.pos += 1;
                    self.line += 1;
                    self.col = 1;
                }
                b' ' | b'\t' | b'\r' | 0x0C => {
                    self.pos += 1;
                    self.col += 1;
                }
                _ => break,
            }
        }
        self.pos - start
    }

    /// Internal peek implementation.
    fn peek_char_inner(&self) -> Option<char> {
        if self.pos >= self.data.len() {
            return None;
        }

        // UPSTREAM-PARITY (parserInternals.c xmlCurrentChar, XML spec 2.11
        // End-of-Line Handling): "the literal two-character sequence #xD#xA
        // or a standalone literal #xD ... must be passed to the application
        // as the single character #xA." libxml2 performs the substitution at
        // character-decode time (xmlCurrentChar's `c == '\r'` arm, which also
        // consumes the LF of a CRLF pair via its `cur++` side effect), so
        // EVERY parser consumer that reads decoded characters — text runs,
        // CDATA sections, comments, PI data, attribute values, entity
        // re-parses — observes `\n` for a source `\r` (with or without a
        // following `\n`). The candidate mirrors the substitution here, at
        // the same decode layer; position advancement (advance_past_char) is
        // source-byte driven and already consumes the CRLF pair, matching
        // xmlCurrentChar's side effect + NEXTL. Raw-byte reads
        // (peek_raw/skip_raw_bytes) and source windows are unaffected — they
        // intentionally see the true source bytes.
        let b = self.data[self.pos];
        if b == b'\r' {
            return Some('\n');
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

    /// Advance position past the character at the current position, updating
    /// line/col tracking. Source-byte driven: the tab/regular-character
    /// branches both advance the column by 1, so the decoded character is
    /// never needed here (§16.5.5 — `read_char`/`consume_peeked` decode
    /// exactly once, up front).
    fn advance_past_char(&mut self) {
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
        } else {
            // Tab, regular character, multi-byte character: one column.
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
        self.pos >= self.data.data_len()
    }

    /// Return the remaining unconsumed bytes.
    pub fn remaining(&self) -> &[u8] {
        &self.data[self.pos..]
    }

    /// Return the bytes consumed so far.
    pub fn consumed(&self) -> &[u8] {
        &self.data[..self.pos]
    }

    /// TEST-ONLY "the prefix is dead" oracle.
    ///
    /// Overwrite the already-consumed prefix `[0, pos)` with NUL, a byte that
    /// cannot appear in a well-formed XML document. A driver that is genuinely
    /// forward-only never reads those bytes again, so its observable output is
    /// unchanged; a driver that restarts from byte zero (the O(N²) replay
    /// failure mode) immediately parses garbage instead. Used by the
    /// persistent-driver court, which poisons the prefix after every call.
    #[cfg(test)]
    pub(crate) fn poison_consumed(&mut self) {
        let pos = self.pos;
        let data = self.data.make_owned();
        for b in data[..pos].iter_mut() {
            *b = 0;
        }
    }

    /// Return the raw source bytes in `[start, end)` (§16.5.3 byte model:
    /// DOCTYPE-body capture transports unnormalized source bytes to
    /// `parse_dtd`). `start`/`end` are absolute byte offsets into the
    /// buffer's data (as returned by `pos()`); the caller guarantees they
    /// are in bounds and `start <= end`.
    pub(crate) fn raw_range(&self, start: usize, end: usize) -> &[u8] {
        &self.data[start..end]
    }

    /// Return the total length of the buffered data in bytes.
    pub const fn len(&self) -> usize {
        self.data.data_len()
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
        // SAFETY notes in the fn docs; for a Borrowed buffer the base points
        // into the caller's region, which the one-call front-ends guarantee
        // stays alive through the parse (§16.5.2).
        let bytes: &[u8] = &self.data;
        let data_ptr = bytes.as_ptr();
        let base = data_ptr as *const crate::abi::types::xmlChar;
        let cur = unsafe { data_ptr.add(self.pos) as *const crate::abi::types::xmlChar };
        let end = unsafe { data_ptr.add(bytes.len()) as *const crate::abi::types::xmlChar };

        input.base = base;
        input.cur = cur;
        input.end = end;
        input.line = self.line as c_int;
        input.col = self.col as c_int;
        input.length = bytes.len() as c_int;
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
    pub unsafe fn populate_parser_input_without_filename(&self, input: &mut _xmlParserInput) {
        // While the source encoding is still undecided the decoder holds the raw
        // bytes back (`pending_source`) and the materialized stream is empty.
        // Upstream has no such split: `xmlParseTryOrFinish`'s `START` gate
        // returns before `xmlDetectEncoding`, so `input->base`/`cur`/`end` still
        // point at the RAW bytes it has received. Exposing the held bytes keeps
        // the public window (`xmlCtxtGetInputWindow`, error handlers) identical
        // instead of showing an empty buffer.
        if self.source_parked() {
            let raw = self.pending_source.as_slice();
            let p = raw.as_ptr() as *const crate::abi::types::xmlChar;
            input.base = p;
            input.cur = p;
            input.end = unsafe { p.add(raw.len()) };
            input.line = self.line as c_int;
            input.col = self.col as c_int;
            input.length = raw.len() as c_int;
            input.consumed = 0;
            return;
        }
        let bytes: &[u8] = &self.data;
        let data_ptr = bytes.as_ptr();
        // Defensive: a window offset can never point past the cursor or the
        // buffer (a replacement resets it, and `pos` only moves forward).
        let wb = self.window_base_abs.min(self.pos).min(bytes.len());
        // The ABI window is DERIVED from the absolute stream: `base` sits at
        // the window offset, `cur` at the absolute position, `end` at the
        // buffer end. `consumed` counts the bytes that left the window, so
        // `consumed + (cur - base) == pos` always holds (upstream
        // `xmlParserShrink` / `xmlCtxtGetInputPosition`).
        let base = unsafe { data_ptr.add(wb) } as *const crate::abi::types::xmlChar;
        let cur = unsafe { data_ptr.add(self.pos) } as *const crate::abi::types::xmlChar;
        let end = unsafe { data_ptr.add(bytes.len()) } as *const crate::abi::types::xmlChar;

        input.base = base;
        input.cur = cur;
        input.end = end;
        input.line = self.line as c_int;
        input.col = self.col as c_int;
        // `length` is upstream-deprecated and unused; keep it coherent with the
        // buffer, but never make it an authority (the cursor plus `end` is).
        input.length = bytes.len() as c_int;
        input.consumed = (self.consumed_bias + wb) as c_ulong;
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
    /// This allows re-parsing the same input from the start. A progressive
    /// (push) buffer keeps its decoded stream and decoder state: only the
    /// read cursor is rewound.
    pub fn reset(&mut self) {
        self.pos = 0;
        self.window_base_abs = 0;
        self.consumed_bias = 0;
        self.line = 1;
        self.col = 1;
        self.bom_consumed = false;
        if self.decoding == SourceDecoding::WholeBuffer {
            self.detect_bom_and_encoding();
        }
    }
}

/// Decode complete UTF-16 code units from `src` into `out`.
///
/// Returns `true` on a definite encoding error — upstream
/// `UTF16LEToUTF8`/`UTF16BEToUTF8`'s `XML_ENC_ERR_INPUT` arms: an unpaired
/// low surrogate, or a high surrogate followed by a non-low unit. `consumed`
/// reports how many leading source bytes formed complete units; a trailing
/// incomplete unit (an odd byte, or a high surrogate whose low half has not
/// arrived) stays unconsumed and is the caller's carry.
fn decode_utf16_units(src: &[u8], be: bool, out: &mut Vec<u8>, consumed: &mut usize) -> bool {
    let unit = |i: usize| -> u16 {
        if be {
            u16::from_be_bytes([src[i], src[i + 1]])
        } else {
            u16::from_le_bytes([src[i], src[i + 1]])
        }
    };

    let mut i = 0usize;
    while i + 2 <= src.len() {
        let c = unit(i);
        if (0xD800..=0xDBFF).contains(&c) {
            // High surrogate: its low half must be present, and must be a low
            // surrogate (`inend - in < 4` in upstream = carry, not error).
            if i + 4 > src.len() {
                break;
            }
            let d = unit(i + 2);
            if !(0xDC00..=0xDFFF).contains(&d) {
                *consumed = i;
                return true;
            }
            let cp = 0x10000 + ((c as u32 - 0xD800) << 10) + (d as u32 - 0xDC00);
            push_utf8(out, cp);
            i += 4;
        } else if (0xDC00..=0xDFFF).contains(&c) {
            // Unpaired low surrogate.
            *consumed = i;
            return true;
        } else {
            push_utf8(out, c as u32);
            i += 2;
        }
    }
    *consumed = i;
    false
}

/// Decode complete fixed-width code units (UCS-2 `width = 2`, UCS-4
/// `width = 4`) from `src` into `out`.
///
/// Returns `true` on a definite encoding error (a surrogate code point or an
/// out-of-range value, matching upstream `fixed_width_input`). A trailing
/// partial unit (fewer than `width` bytes) stays unconsumed as the caller's
/// carry.
fn decode_fixed_units(
    src: &[u8],
    be: bool,
    width: usize,
    out: &mut Vec<u8>,
    consumed: &mut usize,
) -> bool {
    let mut i = 0usize;
    while i + width <= src.len() {
        let mut raw = [0u8; 4];
        raw[..width].copy_from_slice(&src[i..i + width]);
        let cp = if width == 2 {
            if be {
                u32::from(u16::from_be_bytes([raw[0], raw[1]]))
            } else {
                u32::from(u16::from_le_bytes([raw[0], raw[1]]))
            }
        } else if be {
            u32::from_be_bytes(raw)
        } else {
            u32::from_le_bytes(raw)
        };
        if cp > 0x10FFFF || (0xD800..=0xDFFF).contains(&cp) {
            *consumed = i;
            return true;
        }
        push_utf8(out, cp);
        i += width;
    }
    *consumed = i;
    false
}

/// Append `cp` to `out` as UTF-8 (surrogates never reach here).
fn push_utf8(out: &mut Vec<u8>, cp: u32) {
    if let Some(c) = char::from_u32(cp) {
        let mut buf = [0u8; 4];
        out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
    }
}

// ── Registry-served source encodings (declaration-named) ─────────────────────

/// How a registry-served source encoding must be decoded incrementally.
///
/// The registry is NOT one homogeneous category, and treating it as one is a
/// correctness bug: only a byte-wise mapping can be decoded chunk-by-chunk
/// independently. Classic examples:
///
/// ```text
/// Shift_JIS  82 A0  is one character
///   CALL 1: ... 82      -> an independent per-chunk decode calls 82 malformed
///   CALL 2: A0 ...      -> and the original character can never be recovered
///
/// ISO-2022-JP  ESC $ B (JIS state) ... ESC ( B
///   a chunk boundary may land where there is NO incomplete byte sequence,
///   so no byte carry can help: only the decoder's escape state survives
/// ```
///
/// Classification is FAIL-CLOSED: every name the registry can resolve must be
/// listed explicitly, and anything else is [`RegistryKind::Unclassified`].
/// `registry_classification_is_exhaustive` asserts that over
/// `registered_handler_names()`, so adding a codec to the registry (GBK, Big5,
/// EUC-KR, another stateful codec) fails the suite until someone states its
/// incremental semantics — instead of silently inheriting the per-tail path.
enum RegistryKind {
    /// Multibyte or stateful codec: needs a PERSISTENT `encoding_rs::Decoder`.
    Streaming(&'static encoding_rs::Encoding, &'static str),
    /// Fixed-width code units (UTF-16 LE/BE, UCS-2, UCS-4 LE/BE): 0..width-1
    /// trailing bytes carried in `pending_source`.
    FixedWidth(Encoding),
    /// One source byte is one character (ASCII/UTF-8, the ISO-8859 family,
    /// windows-1252/874, IBM037/EBCDIC): a tail-wise decode through the
    /// registered handler is exact.
    ByteWise,
    /// A registry codec nobody has classified. Keeps the historical tail-wise
    /// behavior so no existing stream changes, but is deliberately visible
    /// (and forbidden by the exhaustiveness test for the CURRENT registry).
    Unclassified,
}

/// Classify a declared registry encoding name (alias spellings accepted, via
/// the same canonicalization `decode_whole_buffer_declared` performs).
fn registry_kind(name: &[u8]) -> RegistryKind {
    let canonical =
        crate::xml::encoding::encoding_name(crate::xml::encoding::encoding_from_name(name));
    let lower = match canonical {
        Some(c) => String::from_utf8_lossy(c).to_ascii_lowercase(),
        None => String::from_utf8_lossy(name).trim().to_ascii_lowercase(),
    };
    match lower.as_str() {
        // ── Multibyte / stateful: persistent decoder ──────────────────────
        "shift_jis" | "sjis" | "cp932" | "ms_kanji" => {
            RegistryKind::Streaming(encoding_rs::SHIFT_JIS, "SHIFT_JIS")
        }
        "euc-jp" | "eucjp" => RegistryKind::Streaming(encoding_rs::EUC_JP, "EUC-JP"),
        "iso-2022-jp" | "iso2022jp" => {
            RegistryKind::Streaming(encoding_rs::ISO_2022_JP, "ISO-2022-JP")
        }
        // ── Fixed-width code units ───────────────────────────────────────
        "utf-16" | "utf16" | "utf-16le" | "utf16le" => RegistryKind::FixedWidth(Encoding::Utf16Le),
        "utf-16be" | "utf16be" => RegistryKind::FixedWidth(Encoding::Utf16Be),
        "ucs-2" | "ucs2" => RegistryKind::FixedWidth(Encoding::Ucs2Le),
        "ucs-4" | "ucs-4le" | "ucs4le" => RegistryKind::FixedWidth(Encoding::Ucs4Le),
        "ucs-4be" | "ucs4be" => RegistryKind::FixedWidth(Encoding::Ucs4Be),
        // ── Explicitly byte-wise ─────────────────────────────────────────
        "ascii" | "us-ascii" | "utf-8" | "utf8" | "iso-8859-1" | "iso8859-1" | "latin1"
        | "latin-1" | "iso-8859-2" | "iso-8859-3" | "iso-8859-4" | "iso-8859-5" | "iso-8859-6"
        | "iso-8859-7" | "iso-8859-8" | "iso-8859-9" | "iso-8859-10" | "iso-8859-11"
        | "iso-8859-13" | "iso-8859-14" | "iso-8859-15" | "iso-8859-16" | "windows-1252"
        | "cp1252" | "windows-874" | "ibm037" | "ebcdic" | "ebcdic-us" | "tis-620" => {
            RegistryKind::ByteWise
        }
        _ => RegistryKind::Unclassified,
    }
}

/// Why a registry decode stopped short.
enum RegistryTail {
    /// A definite invalid unit — upstream `XML_ENC_ERR_INPUT` raised by
    /// `xmlParserInputBufferPush` (reported on the call that finds it).
    Invalid,
    /// The feed was final and the decoder still holds an incomplete unit —
    /// the encoder flush error `xmlParserCheckEOF` reports.
    Truncated,
}

/// A persistent `encoding_rs` source decoder.
///
/// A fresh decoder per chunk is WRONG: an incomplete multibyte unit would be
/// reported as malformed (and its raw lead byte materialized), and a stateful
/// codec would silently restart in ASCII. `encoding_rs::Decoder` holds both
/// the incomplete unit and the shift state across calls, mirroring upstream's
/// iconv handle (`handler->inputCtxt`), which also persists across
/// `xmlCharEncInput` calls.
///
/// NOTE: `encoding_rs::Decoder` is not `Clone`, which is why
/// [`InputBuffer::duplicate_for_reparse`] carries no decoder: a reparse
/// duplicate is a parse-only artifact that is never pushed to.
struct RegistrySourceDecoder {
    decoder: encoding_rs::Decoder,
    /// The encoding, so a finished decoder can be restarted (see `finished`).
    encoding: &'static encoding_rs::Encoding,
    /// Canonical codec name (diagnostics via
    /// [`InputBuffer::registry_kind_name`]).
    name: &'static str,
    /// Set by the `last = true` flush.
    ///
    /// `encoding_rs` PANICS ("Must not use a decoder that has finished") if a
    /// flushed decoder is used again, and a REFEED onto a finished push
    /// context does exactly that: upstream's converter survives its
    /// `flush = 1` and happily takes the new bytes into the raw buffer (the
    /// oracle's window shows the second copy decoded), so a fresh decoder is
    /// started for them instead of feeding the finished one.
    finished: bool,
}

impl RegistrySourceDecoder {
    fn new(enc: &'static encoding_rs::Encoding, name: &'static str) -> Self {
        // `without_bom_handling`: a BOM is the input layer's business (the
        // BOM/signature sniff), not the tail decoder's.
        Self {
            decoder: enc.new_decoder_without_bom_handling(),
            encoding: enc,
            name,
            finished: false,
        }
    }

    /// Whether this decoder has been flushed by a terminating call.
    #[allow(dead_code)]
    const fn finished(&self) -> bool {
        self.finished
    }

    /// Feed `bytes`; decode complete characters into `out`.
    ///
    /// Decoding always runs with `last = false`, so an incomplete trailing unit
    /// is BUFFERED INSIDE the decoder and completed by a later call rather than
    /// reported as malformed. `last` is then applied as a separate flush
    /// (empty input, `last = true`), which surfaces exactly the terminating-call
    /// truncation — the same two-phase shape as upstream's
    /// `xmlCharEncInput(..., flush = 0)` during the push followed by
    /// `xmlParserCheckEOF`'s `flush = 1`.
    ///
    /// The decoder is never called with an EMPTY slice and `last = false`: on
    /// that path `encoding_rs`'s two-byte decoders (`ShiftJisDecoder` &
    /// friends) clear their buffered lead byte and return `InputEmpty`,
    /// silently losing it — after which the trail byte in the next chunk would
    /// be misdecoded. Only real bytes are fed `last = false`; the single
    /// empty-slice call in this function is the `last = true` flush.
    fn decode(&mut self, bytes: &[u8], last: bool, out: &mut Vec<u8>) -> Result<(), RegistryTail> {
        if self.finished {
            if bytes.is_empty() {
                // A repeated terminating flush is a no-op: the first one
                // already surfaced any pending truncation.
                return Ok(());
            }
            self.decoder = self.encoding.new_decoder_without_bom_handling();
            self.finished = false;
        }
        let mut buf = [0u8; 4096];
        let mut pos = 0usize;
        while pos < bytes.len() {
            let (res, read, written) =
                self.decoder
                    .decode_to_utf8_without_replacement(&bytes[pos..], &mut buf, false);
            out.extend_from_slice(&buf[..written]);
            pos += read;
            match res {
                encoding_rs::DecoderResult::InputEmpty => break,
                encoding_rs::DecoderResult::OutputFull => {
                    // Normal for a sufficiently large feed: the 4 KiB scratch
                    // is refilled and the loop continues. A ZERO-PROGRESS
                    // OutputFull cannot happen (the destination starts empty
                    // and every decoded scalar fits well inside 4 KiB), but
                    // breaking is cheaper than trusting that.
                    if read == 0 && written == 0 {
                        break;
                    }
                }
                encoding_rs::DecoderResult::Malformed(..) => return Err(RegistryTail::Invalid),
            }
        }
        if last {
            let (res, _, written) =
                self.decoder
                    .decode_to_utf8_without_replacement(&[], &mut buf, true);
            out.extend_from_slice(&buf[..written]);
            self.finished = true;
            if matches!(res, encoding_rs::DecoderResult::Malformed(..)) {
                return Err(RegistryTail::Truncated);
            }
        }
        Ok(())
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
    /// Index of the lowest input that must NOT be auto-popped when exhausted.
    ///
    /// 0 for an ordinary stack. It is raised to the pushed entity input for the
    /// duration of upstream's `xmlCtxtParseEntity` sub-parse: the entity's
    /// replacement text is a BOUNDED input (upstream's `xmlParseContentInternal`
    /// loop stops at `input->cur >= input->end`), so a construct may never read
    /// into the referencing document.
    barrier: usize,
}

impl std::fmt::Debug for InputStack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InputStack")
            .field("depth", &self.inputs.len())
            .field("current", &self.current)
            .field("barrier", &self.barrier)
            .finish()
    }
}

impl InputStack {
    /// Create a new input stack with the given base input.
    pub fn new(base: InputBuffer) -> Self {
        InputStack {
            inputs: vec![base],
            current: 0,
            barrier: 0,
        }
    }

    /// Seal the CURRENT input: it may no longer be auto-popped when exhausted,
    /// so it behaves as the end of the logical stream. Used for the entity
    /// sub-parse, whose content is a bounded input. Returns the previous
    /// barrier so a NESTED sub-parse can restore it (a plain reset to 0 would
    /// silently unseal the enclosing entity input).
    pub(crate) fn seal(&mut self) -> usize {
        let prev = self.barrier;
        self.barrier = self.current;
        prev
    }

    /// Undo [`Self::seal`], restoring the barrier it returned.
    pub(crate) fn unseal(&mut self, prev: usize) {
        self.barrier = prev;
    }

    /// The input at stack index `i` (0 = the base document).
    pub(crate) fn input_at(&self, i: usize) -> &InputBuffer {
        &self.inputs[i]
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
    /// (i.e., if there is only one input remaining) or if the current input is
    /// sealed (see [`Self::seal`]).
    pub fn pop(&mut self) -> Option<InputBuffer> {
        if self.inputs.len() <= 1 || self.current <= self.barrier {
            // Cannot pop the base or a sealed input.
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

    /// Get the BASE (bottom) input buffer — the document's own input, below
    /// any entity-expansion inputs pushed above it. `inputs[0]` always
    /// exists (`new` creates it and `pop` refuses to remove it).
    pub(crate) fn base_ref(&self) -> &InputBuffer {
        &self.inputs[0]
    }

    /// Get a mutable reference to the BASE input buffer (see
    /// [`Self::base_ref`]).
    pub(crate) fn base_mut(&mut self) -> &mut InputBuffer {
        &mut self.inputs[0]
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

    /// §16.5.3: byte range `[start, end)` of the BASE input's data — the
    /// storage a tokenizer [`XmlText::Span`] refers to.
    ///
    /// # Safety contract
    ///
    /// The range is valid while the base buffer is alive and its data is
    /// unmutated. Spans are produced only by base-input body runs and are
    /// consumed synchronously by the parser in the same loop iteration that
    /// produced them (Characters/CDATA/Comment tokens are never pushed back
    /// — only StartTag is), and `push_bytes` only happens between parse
    /// calls, so the bytes stay valid for the token's whole lifetime.
    pub(crate) fn base_input_range(&self, start: usize, end: usize) -> &[u8] {
        self.inputs[0].raw_range(start, end)
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
            // The parent's TRACKED (line, col) is exactly what upstream
            // `xmlCtxtVErr` reports: it reads `ctxt->inputTab[inputNr-2]->col`,
            // the same incrementally maintained column the published window
            // carries (which lags the byte offset by one after a raw `NEXT`
            // consume, e.g. an entity reference's trailing `;`). No adjustment
            // is applied — an earlier `- 1` compensated for a column that was
            // RECONSTRUCTED from the byte offset rather than tracked.
            let (pl, pc, _) = parent.pos();
            (parent.filename(), pl, pc.max(1))
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

    /// §16.5.5: advance past the character the caller just peeked on the
    /// current input WITHOUT decoding it again (see [`InputBuffer::consume_peeked`]).
    /// The caller must have peeked the same character with no intervening
    /// mutation, and the current input must not have been popped in between.
    pub fn consume_peeked(&mut self) {
        self.inputs[self.current].consume_peeked();
    }

    /// §16.6 scalar engine: consume an ASCII-whitespace run across the
    /// stack with the same line/col semantics as the per-character
    /// whitespace loop (a CRLF pair is one line break; an exhausted pushed
    /// input whose whitespace run continues in the parent is popped, exactly
    /// like the old peek+read loop).
    pub(crate) fn skip_ascii_whitespace(&mut self) {
        loop {
            self.pop_exhausted();
            self.inputs[self.current].skip_ascii_whitespace();
            // Continue across an exhausted non-base input only when the
            // whitespace run reached its end (a following byte may still be
            // whitespace in the parent input). A SEALED input is the end of the
            // logical stream, so the loop must stop there.
            if self.inputs.len() <= 1
                || self.current <= self.barrier
                || !self.inputs[self.current].is_eof()
            {
                return;
            }
        }
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

    /// §16.5.6: advance `n` line-break-free bytes of the current input (see
    /// [`InputBuffer::skip_linebreak_free`]).
    pub(crate) fn skip_linebreak_free(&mut self, n: usize) {
        self.inputs[self.current].skip_linebreak_free(n);
    }

    /// Pop any exhausted pushed inputs so that the current input always has
    /// remaining data (or is the base input). A sealed input is never popped.
    fn pop_exhausted(&mut self) {
        while self.inputs.len() > 1
            && self.current > self.barrier
            && self.inputs[self.current].is_eof()
        {
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
        // UPSTREAM-PARITY (parserInternals.c xmlCurrentChar, XML spec 2.11):
        // a literal CR (with or without a following LF) is DELIVERED to the
        // application as a single LF — the substitution happens at decode
        // time, so read_char yields '\n', not '\r'.
        assert_eq!(buf.read_char(), Some('\n'));
        // After CRLF, we're on line 2, col 1; both bytes were consumed
        assert_eq!(buf.pos(), (2, 1, 3));
        assert_eq!(buf.read_char(), Some('b'));
        assert_eq!(buf.pos(), (2, 2, 4));
    }

    #[test]
    fn test_crlf_decode_substitution_forms() {
        // Standalone CR and CRLF both deliver a single LF; position tracking
        // matches the source-byte advance (standalone CR consumes 1 byte,
        // CRLF consumes 2).
        let mut buf = InputBuffer::from_memory(b"a\rb\r\nc", None);
        assert_eq!(buf.read_char(), Some('a'));
        assert_eq!(buf.read_char(), Some('\n')); // standalone CR
        assert_eq!(buf.pos(), (2, 1, 2));
        assert_eq!(buf.read_char(), Some('b'));
        assert_eq!(buf.read_char(), Some('\n')); // CRLF
        assert_eq!(buf.pos(), (3, 1, 5));
        assert_eq!(buf.read_char(), Some('c'));
        // peek_raw sees the true source byte (raw reads are unnormalized).
        let mut buf = InputBuffer::from_memory(b"\r\n", None);
        assert_eq!(buf.peek_raw(), Some(b'\r'));
        assert_eq!(buf.peek_char(), Some('\n'));
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

    // ── §16.7.8 progressive source decoding ────────────────────────────────

    /// Drive a fresh push buffer through `parts` (all NON-final) and return it
    /// with the source still open.
    fn push_parts(parts: &[&[u8]]) -> InputBuffer {
        let mut ib = InputBuffer::for_push(&[], None);
        for p in parts {
            ib.push_bytes_ex(p, false);
        }
        ib
    }

    /// Materialized stream for a partition closed by a terminating call.
    fn materialized(parts: &[&[u8]]) -> Vec<u8> {
        let mut ib = push_parts(parts);
        ib.push_bytes_ex(&[], true);
        ib.remaining().to_vec()
    }

    fn utf16le(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    fn utf16be(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(u16::to_be_bytes).collect()
    }

    fn utf32le(s: &str) -> Vec<u8> {
        s.chars().flat_map(|c| (c as u32).to_le_bytes()).collect()
    }

    fn utf32be(s: &str) -> Vec<u8> {
        s.chars().flat_map(|c| (c as u32).to_be_bytes()).collect()
    }

    // ── Registry-served (declaration-named) source encodings ─────────────
    //
    // Shape of every fixture: an ASCII XML declaration that NAMES the
    // encoding, followed by a body in that encoding. For the ASCII-compatible
    // declared codecs (Shift_JIS, EUC-JP, ISO-2022-JP, ISO-8859-x) converting
    // the buffer leaves the declaration semantically unchanged. The
    // unit-aligned declared codecs (UCS-2, declared UTF-16) are different — see
    // `progressive_ucs2_partition_invariance_and_flush` — because upstream
    // re-reads the converted buffer from byte zero, which mangles an ASCII
    // declaration and makes those shapes fail (as the oracle does).

    /// FAIL-CLOSED classification invariant: every codec in the process-wide
    /// registry must have an explicit `RegistryKind`. Adding GBK/Big5/EUC-KR or
    /// another stateful codec to the registry without deciding its incremental
    /// semantics fails HERE, instead of silently acquiring the chunk-
    /// independent tail path.
    #[test]
    fn registry_classification_is_exhaustive() {
        let names = crate::xml::encoding::registered_handler_names();
        // Guard against a vacuous pass: the registry must actually be populated.
        assert!(
            names.len() >= 30,
            "expected the encoding registry to be populated, got {names:?}"
        );
        let mut unclassified = Vec::new();
        for name in &names {
            if matches!(registry_kind(name.as_bytes()), RegistryKind::Unclassified) {
                unclassified.push(name.clone());
            }
        }
        assert!(
            unclassified.is_empty(),
            "registry codecs with no incremental classification: {unclassified:?}\n\
             classify each in registry_kind() (Streaming / FixedWidth / ByteWise)"
        );
    }

    /// Spot-check that the classification is not just exhaustive but right.
    #[test]
    fn registry_classification_kinds() {
        for name in [
            &b"SHIFT_JIS"[..],
            b"SJIS",
            b"CP932",
            b"EUC-JP",
            b"EUCJP",
            b"ISO-2022-JP",
        ] {
            assert!(
                matches!(registry_kind(name), RegistryKind::Streaming(..)),
                "{name:?} must be Streaming"
            );
        }
        for name in [
            &b"UCS-2"[..],
            b"UCS-4",
            b"UCS-4LE",
            b"UCS-4BE",
            b"UTF-16",
            b"UTF-16LE",
            b"UTF-16BE",
        ] {
            assert!(
                matches!(registry_kind(name), RegistryKind::FixedWidth(_)),
                "{name:?} must be FixedWidth"
            );
        }
        for name in [
            &b"ASCII"[..],
            b"US-ASCII",
            b"UTF-8",
            b"IBM037",
            b"EBCDIC-US",
            b"ISO-8859-1",
            b"ISO-8859-15",
            b"windows-874",
        ] {
            assert!(
                matches!(registry_kind(name), RegistryKind::ByteWise),
                "{name:?} must be ByteWise"
            );
        }
    }

    const SHIFT_JIS_DECL: &[u8] = b"<?xml version=\"1.0\" encoding=\"Shift_JIS\"?>";
    const EUC_JP_DECL: &[u8] = b"<?xml version=\"1.0\" encoding=\"EUC-JP\"?>";
    const ISO2022_JP_DECL: &[u8] = b"<?xml version=\"1.0\" encoding=\"ISO-2022-JP\"?>";
    const UCS2_DECL: &[u8] = b"<?xml version=\"1.0\" encoding=\"UCS-2\"?>";

    fn shift_jis_doc() -> Vec<u8> {
        let mut v = SHIFT_JIS_DECL.to_vec();
        v.extend_from_slice(b"<a>");
        v.extend_from_slice(&[0x82, 0xA0]); // あ (one 2-byte Shift_JIS char)
        v.extend_from_slice(b"</a>");
        v
    }

    fn euc_jp_doc() -> Vec<u8> {
        let mut v = EUC_JP_DECL.to_vec();
        v.extend_from_slice(b"<a>");
        v.extend_from_slice(&[0xA4, 0xA2]); // あ (one 2-byte EUC-JP char)
        v.extend_from_slice(b"</a>");
        v
    }

    /// ISO-2022-JP: `ESC $ B` switches to JIS X 0208, the pair encodes あ, and
    /// `ESC ( B` switches back to ASCII.
    fn iso2022_jp_doc() -> Vec<u8> {
        let mut v = ISO2022_JP_DECL.to_vec();
        v.extend_from_slice(b"<a>");
        v.extend_from_slice(&[0x1B, 0x24, 0x42, 0x24, 0x22, 0x1B, 0x28, 0x42]);
        v.extend_from_slice(b"</a>");
        v
    }

    /// UCS-2 with the registry's host byte order (little-endian).
    fn ucs2_body(s: &str) -> Vec<u8> {
        s.chars()
            .flat_map(|c| (c as u32 as u16).to_le_bytes())
            .collect()
    }

    fn ucs2_doc() -> Vec<u8> {
        let mut v = UCS2_DECL.to_vec();
        v.extend(ucs2_body("<a>x</a>"));
        v
    }

    /// The architectural property for the registry-served codecs: ANY source
    /// chunk partitioning materializes exactly the whole-source decoded
    /// stream. A per-chunk decode fails this for every multibyte/stateful
    /// encoding (Shift_JIS/EUC-JP lose a split character; ISO-2022-JP also
    /// loses the escape state at a boundary with no incomplete bytes at all).
    fn assert_partition_equivalence(src: &[u8], expected: &[u8], what: &str) {
        for split in 1..src.len() {
            let parts: [&[u8]; 2] = [&src[..split], &src[split..]];
            let ib = push_parts(&parts);
            assert_eq!(ib.remaining(), expected, "{what}: split at {split}");
            assert_eq!(
                ib.materialized_bytes() as usize,
                ib.len(),
                "{what}: split at {split} (materialized accounting)"
            );
            assert_eq!(
                ib.source_bytes_received() as usize,
                src.len(),
                "{what}: split at {split} (source accounting)"
            );
        }
    }

    #[test]
    fn progressive_shift_jis_partition_equivalence() {
        let src = shift_jis_doc();
        let mut expected = SHIFT_JIS_DECL.to_vec();
        expected.extend_from_slice("<a>あ</a>".as_bytes());
        assert_eq!(materialized(&[&src]), expected);
        assert_partition_equivalence(&src, &expected, "Shift_JIS");
    }

    #[test]
    fn progressive_euc_jp_partition_equivalence() {
        let src = euc_jp_doc();
        let mut expected = EUC_JP_DECL.to_vec();
        expected.extend_from_slice("<a>あ</a>".as_bytes());
        assert_eq!(materialized(&[&src]), expected);
        assert_partition_equivalence(&src, &expected, "EUC-JP");
    }

    #[test]
    fn progressive_iso2022_jp_partition_equivalence() {
        let src = iso2022_jp_doc();
        let mut expected = ISO2022_JP_DECL.to_vec();
        expected.extend_from_slice("<a>あ</a>".as_bytes());
        assert_eq!(materialized(&[&src]), expected);
        assert_partition_equivalence(&src, &expected, "ISO-2022-JP");
    }

    /// The crucial ISO-2022-JP case: the chunk boundary falls at a point with
    /// NO incomplete byte sequence at all (between the JIS pair and the
    /// `ESC ( B` that leaves JIS state). Only the decoder's persistent escape
    /// state can carry the shifted state across the boundary — no byte carry
    /// can.
    #[test]
    fn progressive_iso2022_jp_shift_state_survives_a_boundary_without_partial_bytes() {
        let mut lead = ISO2022_JP_DECL.to_vec();
        lead.extend_from_slice(b"<a>");
        let shift_in = [0x1B, 0x24, 0x42];
        let jis_pair = [0x24, 0x22];
        let shift_out = [0x1B, 0x28, 0x42];
        let mut tail = shift_out.to_vec();
        tail.extend_from_slice(b"</a>");

        let ib = push_parts(&[&lead, &shift_in, &jis_pair, &tail]);
        assert!(!ib.source_encoding_error());
        assert!(ib.pending_source().is_empty());
        assert_eq!(
            ib.remaining(),
            [ISO2022_JP_DECL, "<a>あ</a>".as_bytes()]
                .concat()
                .as_slice()
        );
        assert_eq!(ib.registry_kind_name(), Some("ISO-2022-JP"));
    }

    /// An incomplete trailing unit in a multibyte codec is SUSPENSION (the
    /// decoder holds it), not a malformed byte — and a terminating call turns
    /// a still-held unit into the encoder flush error.
    #[test]
    fn progressive_shift_jis_incomplete_unit_is_carried_then_flushed() {
        let mut src = SHIFT_JIS_DECL.to_vec();
        src.extend_from_slice(b"<a>");
        src.push(0x82); // lead byte of あ, trail byte not yet sent
        let mut ib = push_parts(&[&src]);
        assert!(!ib.source_encoding_error());
        assert!(!ib.source_truncated());
        assert_eq!(
            ib.remaining(),
            [SHIFT_JIS_DECL, b"<a>"].concat().as_slice(),
            "the incomplete unit is not materialized"
        );
        ib.push_bytes_ex(&[], true);
        assert!(ib.source_truncated());
        assert!(!ib.source_encoding_error());
    }

    /// A DEFINITE invalid sequence (a Shift_JIS lead byte followed by a
    /// non-trail byte) is reported on the call that finds it, like upstream
    /// `xmlParserInputBufferPush` returning -1 — distinct from the incomplete
    /// unit above, which is suspended.
    #[test]
    fn progressive_shift_jis_invalid_sequence_is_an_immediate_error() {
        let mut src = SHIFT_JIS_DECL.to_vec();
        src.extend_from_slice(b"<a>");
        src.extend_from_slice(&[0x82, 0x20]); // lead byte, then a space
        let ib = push_parts(&[&src]);
        assert!(ib.source_encoding_error());
        assert!(!ib.source_truncated());
    }

    #[test]
    fn progressive_ucs2_partition_invariance_and_flush() {
        let src = ucs2_doc();
        // NOTE: no concrete expected text. A declared unit-aligned codec is
        // decoded as code units from the START of the buffer — including the
        // ASCII declaration that named it — which is what upstream's encoding
        // switch does, and it is why the oracle REJECTS these documents
        // (court `enc-ucs2*`: XML_ERR_SPACE_REQUIRED 65 + XML_ERR_'?>' expected
        // 57, wellFormed 0). The contract under test is partition invariance.
        let whole = materialized(&[&src]);
        assert!(!whole.is_empty());
        for split in 1..src.len() {
            let ib = push_parts(&[&src[..split], &src[split..]]);
            assert_eq!(ib.remaining(), whole.as_slice(), "UCS-2: split at {split}");
            assert_eq!(ib.materialized_bytes() as usize, ib.len());
            assert_eq!(ib.source_bytes_received() as usize, src.len());
        }

        // Odd trailing byte: carried, then the terminating call's flush.
        let mut odd = UCS2_DECL.to_vec();
        odd.extend(ucs2_body("<a>"));
        odd.push(0x78);
        let mut ib = push_parts(&[&odd]);
        assert!(!ib.source_encoding_error());
        assert!(!ib.source_truncated());
        assert_eq!(ib.pending_source(), &[0x78]);
        ib.push_bytes_ex(&[], true);
        assert!(ib.source_truncated());
        assert!(!ib.source_encoding_error());
    }

    /// A DECLARED UTF-16 (no BOM, no `<\0?\0` pattern) has fixed-width units
    /// and is decoded from byte zero like the declared UCS-2 above (upstream
    /// re-decodes the buffer after the switch, so the oracle also rejects this
    /// shape); the contract under test is partition invariance plus carry.
    #[test]
    fn progressive_declared_utf16le_is_decoded_as_units() {
        let mut src = b"<?xml version=\"1.0\" encoding=\"UTF-16LE\"?>".to_vec();
        src.extend(utf16le("<a>x</a>"));
        let whole = materialized(&[&src]);
        for split in 1..src.len() {
            let ib = push_parts(&[&src[..split], &src[split..]]);
            assert_eq!(
                ib.remaining(),
                whole.as_slice(),
                "declared UTF-16LE: split at {split}"
            );
        }

        // Odd trailing byte: carried, then flushed. Unit alignment starts at
        // byte zero, so a half unit dangles iff the total source length is odd.
        let decl = b"<?xml version=\"1.0\" encoding=\"UTF-16LE\"?>";
        let mut odd = decl.to_vec();
        odd.extend(utf16le("<a>"));
        if odd.len() % 2 == 0 {
            odd.push(0x3C);
        }
        assert_eq!(odd.len() % 2, 1);
        let mut ib = push_parts(&[&odd]);
        assert_eq!(
            ib.pending_source().len(),
            1,
            "the dangling half unit is held"
        );
        assert!(!ib.source_truncated());
        ib.push_bytes_ex(&[], true);
        assert!(ib.source_truncated());
    }

    /// Upstream `xmlParseTryOrFinish`'s `XML_PARSER_START` gate: a non-final
    /// call with fewer than four source bytes parks — the encoding is not
    /// decided, nothing is materialized, and the bytes are held.
    #[test]
    fn progressive_parks_until_the_start_gate_is_satisfied() {
        let mut ib = push_parts(&[b"<"]);
        assert!(ib.source_parked());
        assert_eq!(ib.materialized_bytes(), 0);
        assert_eq!(ib.pending_source(), b"<");
        ib.push_bytes_ex(b"a", false);
        ib.push_bytes_ex(b"/", false);
        assert!(ib.source_parked(), "avail < 4 must keep parking");
        assert_eq!(ib.pending_source(), b"<a/");
        assert_eq!(ib.source_bytes_received(), 3);
        ib.push_bytes_ex(b">", false);
        assert!(!ib.source_parked());
        assert_eq!(ib.remaining(), b"<a/>");
        assert_eq!(ib.materialized_bytes(), 4);
        assert_eq!(ib.source_bytes_received(), 4);
    }

    /// A UTF-8 BOM split across calls: `EF BB` cannot be recognized yet, so
    /// the buffer must park instead of defaulting to UTF-8.
    #[test]
    fn progressive_utf8_bom_split_across_calls() {
        let mut ib = push_parts(&[b"\xef\xbb"]);
        assert!(ib.source_parked());
        assert_eq!(ib.materialized_bytes(), 0);
        ib.push_bytes_ex(b"\xbf<a/>", false);
        assert!(!ib.source_parked());
        assert!(ib.bom_was_consumed());
        assert_eq!(ib.remaining(), b"<a/>");
        assert_eq!(ib.materialized_bytes() as usize, ib.len());
    }

    /// A UTF-16LE BOM split at every byte position must still decode the whole
    /// document — the classic "BOM split across two writes" bug.
    #[test]
    fn progressive_utf16le_bom_split_matches_whole_source_decode() {
        let mut src = vec![0xFF, 0xFE];
        src.extend(utf16le("<a>x</a>"));
        let whole = materialized(&[&src]);
        assert_eq!(whole, b"<a>x</a>");
        for split in 1..5 {
            let ib = push_parts(&[&src[..split], &src[split..]]);
            assert_eq!(ib.encoding(), &Encoding::Utf16Le);
            assert_eq!(ib.remaining(), b"<a>x</a>", "split at {split}");
        }
    }

    /// A half code unit at the end of the available source is SUSPENSION, not
    /// an error: it waits for the rest. Only a terminating call turns a still
    /// incomplete unit into the encoder-truncation error.
    #[test]
    fn progressive_truncated_unit_is_suspension_then_error_on_terminate() {
        let mut src = vec![0xFF, 0xFE];
        src.extend(utf16le("<a>x</a>"));
        src.push(0x20); // half of a trailing code unit
        let mut ib = push_parts(&[&src]);
        assert!(!ib.source_encoding_error());
        assert!(!ib.source_truncated());
        assert_eq!(ib.pending_source(), &[0x20]);
        assert_eq!(ib.remaining(), b"<a>x</a>");
        // The terminating call flushes the encoder: the byte is still never
        // consumed, and the incomplete unit becomes the error.
        ib.push_bytes_ex(&[], true);
        assert!(ib.source_truncated());
        assert!(!ib.source_encoding_error());
        assert_eq!(ib.pending_source(), &[0x20]);
        assert_eq!(ib.remaining(), b"<a>x</a>");
    }

    /// The high half of a surrogate pair is carried until its low half
    /// arrives in a later call (upstream `inend - in < 4` -> break).
    #[test]
    fn progressive_utf16_surrogate_split_across_calls() {
        let mut src = vec![0xFF, 0xFE];
        src.extend(utf16le("<a>🎉</a>"));
        let hi = src.windows(2).position(|w| w == [0x3C, 0xD8]).unwrap();
        let ib = push_parts(&[
            &src[..2],
            &src[2..hi + 2],
            &src[hi + 2..hi + 4],
            &src[hi + 4..],
        ]);
        assert_eq!(ib.remaining(), "<a>🎉</a>".as_bytes());
    }

    /// An unpaired low surrogate is a DEFINITE error on the call that finds it
    /// (upstream `XML_ENC_ERR_INPUT`), not a carry.
    #[test]
    fn progressive_utf16_lone_low_surrogate_is_immediate_error() {
        let mut src = vec![0xFF, 0xFE];
        src.extend(utf16le("<a>x</a>"));
        src.extend([0x00, 0xDC]);
        let ib = push_parts(&[&src]);
        assert!(ib.source_encoding_error());
        assert!(!ib.source_truncated());
    }

    /// A high surrogate followed by a non-low unit is likewise definite.
    #[test]
    fn progressive_utf16_high_then_non_low_is_immediate_error() {
        let mut src = vec![0xFF, 0xFE];
        src.extend(utf16le("<a>x</a>"));
        src.extend([0x3C, 0xD8, 0x41, 0x00]); // high surrogate then 'A'
        let ib = push_parts(&[&src]);
        assert!(ib.source_encoding_error());
    }

    /// The EBCDIC signature needs 200 available source bytes on a non-final
    /// call (upstream `xmlDetectEBCDIC` cannot pick a code page earlier).
    #[test]
    fn progressive_ebcdic_signature_parks_until_two_hundred_bytes() {
        let mut src = vec![0x4C, 0x6F, 0xA7, 0x94];
        src.resize(199, 0x40);
        let mut ib = push_parts(&[&src]);
        assert!(ib.source_parked(), "EBCDIC signature parks below 200 bytes");
        assert_eq!(ib.materialized_bytes(), 0);
        ib.push_bytes_ex(&[0x40], false);
        assert!(!ib.source_parked());
        assert_eq!(ib.encoding(), &Encoding::Ebcdic);
        assert!(ib.remaining().starts_with(b"<?xm"));
    }

    /// The architectural property behind the decoder: any source chunk
    /// partitioning materializes the same UTF-8 stream as whole-source
    /// decoding. This is independent of any oracle comparison.
    #[test]
    fn progressive_partition_equivalence_is_exact() {
        let mut cases: Vec<Vec<u8>> = Vec::new();
        cases.push({
            let mut v = vec![0xFF, 0xFE];
            v.extend(utf16le("<a>🎉x</a>"));
            v
        });
        cases.push({
            let mut v = vec![0xFE, 0xFF];
            v.extend(utf16be("<a>🎉x</a>"));
            v
        });
        cases.push(utf16le("<?xml version=\"1.0\"?><a>x</a>"));
        cases.push(utf16be("<?xml version=\"1.0\"?><a>x</a>"));
        cases.push(utf32le("<?xml version=\"1.0\"?><a>x</a>"));
        cases.push({
            // The UTF-32LE BOM is NOT a BOM upstream: it is a UTF-16LE BOM
            // followed by U+0000 (see detect_bom_and_encoding).
            let mut v = vec![0xFF, 0xFE, 0x00, 0x00];
            v.extend(utf32le("<a>x</a>"));
            v
        });
        cases.push({
            // Likewise the UTF-32BE BOM is undetected: the stream is read as
            // UTF-8 (NUL bytes included) and fails in the parser, not here.
            let mut v = vec![0x00, 0x00, 0xFE, 0xFF];
            v.extend(utf32be("<a>x</a>"));
            v
        });

        for src in &cases {
            let whole = materialized(&[src]);
            assert!(!whole.is_empty());
            for split in 1..src.len() {
                let ib = push_parts(&[&src[..split], &src[split..]]);
                assert_eq!(ib.remaining(), whole.as_slice(), "{src:?} split at {split}");
                assert_eq!(ib.materialized_bytes() as usize, ib.len());
                assert_eq!(ib.source_bytes_received() as usize, src.len());
            }
        }
    }

    /// Upstream's input buffer carries a NUL sentinel at `end`; the materialized
    /// buffer here does not. `xmlCtxtGetInputWindow` must therefore never
    /// dereference at `cur == end` — the ordinary fully-consumed position, where
    /// every chunk/callback tail sits — or it reads one byte past the
    /// allocation. It did (the shadow court crashed on its first cell); this
    /// pins the fix.
    #[test]
    fn input_window_never_reads_past_a_fully_consumed_buffer() {
        let mut ib = InputBuffer::for_push(b"<a/>", None);
        assert_eq!(ib.len(), 4);
        ib.skip_linebreak_free(4);
        assert_eq!(ib.pos().2, 4);
        unsafe {
            let mut input: _xmlParserInput = core::mem::zeroed();
            ib.populate_parser_input_without_filename(&mut input);
            assert_eq!(input.cur, input.end, "cursor is at the end");
            let mut start: *const crate::abi::types::xmlChar = std::ptr::null();
            let mut size: c_int = 80;
            let mut off: c_int = -1;
            crate::abi::exports_misc::parser_input_get_window_pub(
                &mut input, &mut start, &mut size, &mut off,
            );
            assert!(size > 0, "the window must still show the line");
            assert!(start >= input.base && start < input.end);
        }
    }
}
