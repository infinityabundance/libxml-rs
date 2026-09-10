//! XML tokenizer — lexical scanning for the parser state machine (§85 Phase 3).
//!
//! Produces `XmlToken` values from the input stack. The tokenizer handles
//! low-level scanning: tags, comments, PIs, CDATA sections, character data,
//! entity/character references, and the XML declaration.
//!
//! # Upstream contract
//!
//! Mirrors the lexical scanning of upstream parser.c and parserInternals.c
//! (SRC-LIBXML2-2.15.0, oracle tree `oracle/historical/src/libxml2-2.15.0/`).
//! Parity target: the system libxml2 2.15.3 oracle.
//!
//! # Conceptual behavior
//!
//! Produces `XmlToken` values from the input stack: tags, comments, PIs, CDATA
//! sections, character data, entity/character references and the XML
//! declaration. Structural errors are recorded into an error queue at their
//! exact detection points with upstream positions, then drained in order by the
//! parser (`record_error` / `record_error_at` / `take_errors`).
//!
//! # Ownership & safety invariants
//!
//! SAFETY: the tokenizer borrows the InputBuffer/InputStack owned by the
//! parser context and never allocates C objects. Recorded errors own their
//! message and str1-3 buffers. Tokens own their byte vectors; positions are
//! byte offsets into the current input.
//!
//! # Historical quirks & epochs
//!
//! Error positions are epoch-pinned: the 2.12.x error-handling rework (commit
//! c6083a32) changed how far the tokenizer may consume past an error before it
//! is reported, and R-000163 fixed carets that pointed one byte past the
//! upstream position. E-002 (single diagnostic) and E-005 (exit codes) both
//! originate in the same 2.12/2.13 parser rework era.
//!
//! # Deliberate oddities
//!
//! Deliberate oddities: StartTag carries `attr_start`/`attr_end` byte offsets
//! so the '<' in entity error caret lands on the '&' (R-000121) and namespace
//! diagnostics land at the tag end (R-000166); the error queue exists because
//! the tokenizer runs ahead of the parser.
//!
//! # Proving courts
//!
//! Exercised by the PARSER court family, the ERROR-001 data-ABI probe (48
//! cases, byte-identical), TREE-001, CLI-XMLLINT-0033/0034 and `cargo test
//! --lib`. Receipts under courts/receipts/phase-11.
//!
//! # Tempting simplifications that would break parity
//!
//! A naive report-at-current-position simplification would break the caret
//! contract that ERROR-001 pins byte-for-byte. Do not drop the attr_start /
//! attr_end offsets — the R-000121 and R-000166 carets depend on them. Do not
//! move structural validation into the parser: the tokenizer must record
//! errors before the parser advances (R-000163).

use crate::xml::parser::input::{InputBuffer, InputStack};
use std::os::raw::c_int;

/// A single lexical token produced by the XML tokenizer.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum XmlToken {
    /// End of input.
    Eof,

    /// XML declaration: `<?xml version="1.0" ...?>`
    XmlDecl {
        version: Vec<u8>,
        encoding: Option<Vec<u8>>,
        standalone: Option<Vec<u8>>,
    },

    /// DOCTYPE declaration (content between `<!DOCTYPE` and closing `>`);
    /// `unterminated` marks a body cut off by the end of the available input
    /// (its `>` may arrive on a later push call).
    DocType {
        content: Vec<u8>,
        unterminated: bool,
    },

    /// The HEAD of a `<!DOCTYPE` declaration, scanned the way upstream
    /// `xmlParseDocTypeDecl` does: it stops AT a `[` (leaving it unconsumed) or
    /// just past the closing `>` when there is no internal subset. The push
    /// driver needs the two phases separately because upstream sets
    /// `XML_PARSER_DTD` between them.
    DocTypeDecl {
        /// `name (ExternalID)?` — the bytes between `<!DOCTYPE` and the stop.
        content: Vec<u8>,
        /// A `[` was found: the internal subset follows.
        has_subset: bool,
        /// The declaration ended with `>`.
        closed: bool,
    },

    /// The internal subset, from its `[` through its matching `]` (the closing
    /// `>` is consumed but not part of `content`). Scanned only when the whole
    /// subset is available — upstream `xmlParseLookupInternalSubset` refuses
    /// progressive parsing of the subset.
    DocTypeSubset { content: Vec<u8>, closed: bool },

    /// Start tag: `<name ...>` or `<name ... />`
    ///
    /// `unterminated` marks a tag that never reached `>`/`/>` (upstream
    /// `xmlParseStartTag2` failed the end-of-tag check; the tokenizer has
    /// already recorded the corresponding errors).
    StartTag {
        name: Vec<u8>,
        attributes: Vec<(Vec<u8>, Vec<u8>)>,
        /// Byte offset just past each attribute value's closing quote
        /// (parallel to `attributes`; used for namespace-URI diagnostics).
        attr_end: Vec<usize>,
        /// Byte offset just after each attribute value's opening quote
        /// (parallel to `attributes`; start of the raw value, used for the
        /// '<' in entity error caret).
        attr_start: Vec<usize>,
        /// Byte offset of the tag's closing '>' (or '/' for empty elements) —
        /// upstream xmlParseStartTag2 raises the undefined-namespace-prefix
        /// error with the input still at the tag end.
        end_pos: usize,
        empty: bool,
        unterminated: bool,
    },

    /// End tag: `</name>` (carries the byte offset of the leading `<` so
    /// the parser can attribute document-level errors to the token start).
    EndTag {
        name: Vec<u8>,
        start_pos: usize,
        unterminated: bool,
    },

    /// Comment: `<!-- ... -->` (carries whether the comment was cut off by
    /// the end of the available input).
    Comment { data: XmlText, unterminated: bool },

    /// Processing instruction: `<?target ...?>`, with the byte offset of
    /// the leading `<?` (for document-level "invalid element name" errors)
    /// and whether the `?>` terminator was reached (upstream xmlParsePI:
    /// EOF without `?>` raises XML_ERR_PI_NOT_FINISHED; incremental probes
    /// pause instead of delivering the partial PI).
    ProcessingInstruction {
        target: Vec<u8>,
        data: Vec<u8>,
        start_pos: usize,
        unterminated: bool,
    },

    /// CDATA section: `<![CDATA[ ... ]]>` (carries the `<` byte offset and
    /// whether the section was terminated).
    Cdata {
        data: XmlText,
        unterminated: bool,
        start_pos: usize,
    },

    /// Character data (text content).
    Characters(XmlText),

    /// Entity or character reference (`&name;`, `&#123;`, `&#xAB;`).
    Reference(Vec<u8>),
}

/// Character data payload of a content token (§16.5.3 token-span
/// architecture): either patched owned bytes or a byte span over the source
/// input — the tokenizer must not allocate merely to describe bytes that
/// already exist in the input.
///
/// # Span validity contract
///
/// A [`XmlText::Span`] `{ start, end }` indexes the data of the buffer the
/// run was scanned from. Spans are produced ONLY by body scanners running at
/// the BASE input (depth 0) whose run consumed no patch event (invalid-char
/// / invalid-UTF-8 replacement, a source CR whose EOL substitution changes
/// the delivered byte, or a construct that drops bytes). The token is
/// consumed synchronously by the parser in the same loop iteration that
/// produced it (Characters/CDATA/Comment tokens are never pushed back —
/// only StartTag is), and the base buffer's data cannot be mutated between
/// token production and consumption (`push_bytes` only happens between
/// parse calls). Entity-content runs (depth > 0) never produce spans: an
/// exhausted pushed input is popped and dropped, so those runs materialize
/// owned bytes.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum XmlText {
    /// Patched bytes (or a clean entity-content run copied once).
    Owned(Vec<u8>),
    /// `[start, end)` byte offsets into the base input's data.
    Span { start: usize, end: usize },
}

impl XmlText {
    /// Byte length of the payload.
    #[inline]
    pub(crate) fn len(&self) -> usize {
        match self {
            XmlText::Owned(v) => v.len(),
            XmlText::Span { start, end } => end - start,
        }
    }

    /// Whether the payload is empty.
    #[inline]
    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// A parser error recorded by the tokenizer at its exact detection point
/// (upstream raises these immediately; the candidate queues them so the
/// tokenizer can keep scanning, and the parser drains + raises them in
/// order after each token — 11.1-M error-semantics parity).
#[derive(Debug, Clone)]
pub(crate) struct ErrorInfo {
    pub domain: c_int,
    pub code: c_int,
    pub level: c_int,
    /// Fully formatted upstream message (may end with `\n`).
    pub msg: String,
    pub str1: Option<Vec<u8>>,
    pub str2: Option<Vec<u8>>,
    pub str3: Option<Vec<u8>>,
    pub int1: c_int,
    /// 1-based line at the error position.
    pub line: c_int,
    /// 1-based byte column at the error position (upstream `input->col`).
    pub col: c_int,
    /// Absolute byte offset of the error in the base input. The recursive
    /// parser raises inline (so `input->cur` is already there); the push driver
    /// scans a whole construct and then flushes, so it repositions the published
    /// window per diagnostic to keep a structured-error handler's view of
    /// `input->cur` identical.
    pub byte_pos: usize,
    /// Source window (line bytes) + 0-based caret column, computed with
    /// upstream's `xmlParserInputGetWindow` algorithm (80-char cap).
    pub window: Option<(Vec<u8>, usize)>,
    /// For `XML_ERR_INVALID_ENCODING`: the 4 bytes at the error position
    /// (upstream `xmlFormatError` "Bytes:" fragment).
    pub enc_bytes: Option<([u8; 4], usize)>,
}

/// Byte offsets (within `content`) of the OPENING quotes of internal-subset
/// entity VALUES.
///
/// Upstream `xmlParseEntityValue` takes that one quote with a raw `CUR_PTR++`
/// (no `col` bump). An EXTERNAL identifier's literals go through
/// `xmlParseSystemLiteral` / `xmlParsePubidLiteral`, which use `NEXT` and DO
/// bump, so those are excluded.
fn dtd_entity_value_quote_offsets(content: &[u8]) -> Vec<usize> {
    const KW: &[u8] = b"<!ENTITY";
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + KW.len() <= content.len() {
        if !content[i..].starts_with(KW) {
            i += 1;
            continue;
        }
        let mut j = i + KW.len();
        let skip_ws = |j: &mut usize| {
            while *j < content.len() && content[*j].is_ascii_whitespace() {
                *j += 1;
            }
        };
        skip_ws(&mut j);
        if j < content.len() && content[j] == b'%' {
            j += 1;
            skip_ws(&mut j);
        }
        // Name.
        while j < content.len()
            && !content[j].is_ascii_whitespace()
            && content[j] != b'>'
            && content[j] != b'"'
            && content[j] != b'\''
        {
            j += 1;
        }
        skip_ws(&mut j);
        if j < content.len() && content[j] != b'>' {
            let external =
                content[j..].starts_with(b"SYSTEM") || content[j..].starts_with(b"PUBLIC");
            if !external && (content[j] == b'"' || content[j] == b'\'') {
                out.push(j);
            }
        }
        i = (j + 1).max(i + KW.len());
    }
    out
}

/// The `input->col` LAG at `offset` within `content`.
///
/// Upstream `xmlParseEntityValue` consumes an entity value's opening quote
/// with a raw `CUR_PTR++` that does not bump `col`, so the column runs one
/// short per such quote — but only until the next line break, which resets it
/// to 1 regardless. Counting only the quotes on `offset`'s own line is
/// therefore exact. Subtracting every quote in the subset (which this
/// replaces) wrongly leaks an earlier line's lag across the newline
/// (`staff.xml`: `]>` on line 13 reported `col=1` instead of 3; `inC14N5`:
/// `col=2` instead of 3).
pub(crate) fn dtd_col_lag_at(content: &[u8], offset: usize) -> usize {
    let offset = offset.min(content.len());
    let line_start = content[..offset]
        .iter()
        .rposition(|&b| b == b'\n' || b == b'\r')
        .map_or(0, |p| p + 1);
    dtd_entity_value_quote_offsets(content)
        .into_iter()
        .filter(|&q| q >= line_start && q < offset)
        .count()
}

/// The XML tokenizer — scans lexical tokens from the input stack.
pub(crate) struct XmlTokenizer {
    input: InputStack,
    /// Buffer for a single pushed-back token (for one-token lookahead).
    push_back: Option<XmlToken>,
    /// Parser errors recorded during scanning (drained by the parser).
    errors: Vec<ErrorInfo>,
    /// Diagnostics whose upstream raise point is LATER in the same construct
    /// than the scan that detects them. Upstream `xmlParseStartTag2` parses
    /// EVERY attribute (raising the per-attribute namespace diagnostics) and
    /// only then runs its hash-based duplicate scan, so "Attribute %s
    /// redefined" must not be flushed with the rest of the tag's scan errors —
    /// `ns-default-undeclare.xml` pins the order (the `xmlns: URI u1 is not
    /// absolute` warning at `i2=14` precedes the redefinition at `i2=23`).
    deferred_errors: Vec<ErrorInfo>,
    /// Byte offset at which a character-data run must break so the event
    /// segmentation matches an earlier eager-partial delivery of the same
    /// accumulated input (SP-14.3.1-6). None = no split.
    split_chars_at: Option<usize>,
    /// Maximum accepted name length in bytes — upstream XML_MAX_NAME_LENGTH
    /// (50 000) or XML_MAX_TEXT_LENGTH (10 000 000) with XML_PARSE_HUGE
    /// (parserInternals.h; SP-14.3.1-6).
    max_name_length: usize,
    /// Upstream XML_PARSE_OLD10: any XML-declaration version other than
    /// "1.0" is a FATAL XML_ERR_UNKNOWN_VERSION instead of the usual
    /// classification (warning for "1.x", fatal otherwise). Configured by
    /// the parser from `ctxt->options` (xmlParseXMLDecl).
    old10: bool,
    /// Silent incremental probing (SP-14.3.1-3/-6): when set, EOF-truncated
    /// constructs (an unterminated start tag / attribute value that may
    /// complete on a later push call) record NO diagnostics — the probe or
    /// eager-partial delivery pauses instead, and the error surfaces only on
    /// a real terminating parse (upstream xmlParseChunk buffers such input on
    /// non-final calls with a clean context).
    silent_truncated: bool,
    /// Byte offset at which the most recently scanned token started. The
    /// parser uses it as the eager-delivery boundary when a scan is truncated
    /// mid-construct: every construct completed before this offset was
    /// delivered; the truncated construct itself re-scans from scratch on the
    /// next call (SP-14.3.1-6, test_events eager-start semantics).
    last_token_start: usize,
    /// Whether the consumer is dispatched through the SAX2 handler table.
    ///
    /// Upstream words an invalid element name differently for the two scanner
    /// entries: `xmlParseStartTag2` (SAX2) says "StartTag: invalid element
    /// name", `xmlParseStartTag` (SAX1) says "xmlParseStartTag: invalid
    /// element name". Set from the parser's own SAX2 detection.
    sax2: bool,
    /// Set while the PUSH driver scans a start tag.
    ///
    /// Upstream 2.15 splits the start-tag end check between two callers with
    /// DIFFERENT diagnostics for the same failure: `xmlParseTryOrFinish`'s
    /// START_TAG arm (push) raises `XML_ERR_GT_REQUIRED` with
    /// "Couldn't find end of Start Tag %s\n" and `int1 = 0`, while
    /// `xmlParseElementStart` (pull) raises the same code with
    /// "Couldn't find end of Start Tag %s line %d\n" and `int1 = line`. The
    /// tokenizer models the pull variant by default.
    push_start_tag: bool,
}

impl XmlTokenizer {
    /// Append a character's UTF-8 encoding to a byte vector.
    ///
    /// # UPSTREAM-PARITY
    ///
    /// libxml2 operates on UTF-8 bytes throughout; a decoded `char` must be
    /// re-encoded as UTF-8, never truncated to a single byte.
    fn push_char(v: &mut Vec<u8>, c: char) {
        let mut buf = [0u8; 4];
        v.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
    }

    /// Create a new tokenizer over the given input stack.
    pub const fn new(input: InputStack) -> Self {
        XmlTokenizer {
            input,
            push_back: None,
            errors: Vec::new(),
            deferred_errors: Vec::new(),
            split_chars_at: None,
            max_name_length: 50_000,
            old10: false,
            silent_truncated: false,
            last_token_start: 0,
            sax2: true,
            push_start_tag: false,
        }
    }

    /// Set the PUSH start-tag diagnostic variant (see `push_start_tag`).
    pub(crate) const fn set_push_start_tag(&mut self, on: bool) {
        self.push_start_tag = on;
    }

    /// Set whether the consumer uses SAX2 (see `sax2`).
    pub(crate) const fn set_sax2(&mut self, on: bool) {
        self.sax2 = on;
    }

    /// Set whether EOF-truncated constructs are scanned silently (see
    /// `silent_truncated`).
    pub const fn set_silent_truncated(&mut self, silent: bool) {
        self.silent_truncated = silent;
    }

    /// Byte offset at which the most recently scanned token started (see
    /// `last_token_start`).
    pub fn last_token_start(&self) -> usize {
        self.last_token_start
    }

    /// Set the byte offset at which character-data runs split (see
    /// `split_chars_at`).
    pub const fn set_split_chars_at(&mut self, offset: Option<usize>) {
        self.split_chars_at = offset;
    }

    /// Set the maximum accepted name length (see `max_name_length`).
    pub const fn set_max_name_length(&mut self, limit: usize) {
        self.max_name_length = limit;
    }

    /// Set whether XML_PARSE_OLD10 is in force (see `old10`).
    pub const fn set_old10(&mut self, on: bool) {
        self.old10 = on;
    }

    /// Get a mutable reference to the input stack.
    #[allow(dead_code)]
    pub const fn input_mut(&mut self) -> &mut InputStack {
        &mut self.input
    }

    /// Get a reference to the input stack.
    pub const fn input(&self) -> &InputStack {
        &self.input
    }

    /// Consume the tokenizer and return the input stack.
    #[allow(dead_code)]
    pub fn into_input(self) -> InputStack {
        self.input
    }

    /// Push a new input onto the input stack (for entity expansion).
    pub fn push_input(&mut self, buf: InputBuffer) {
        self.input.push(buf);
    }

    /// Pop the current input from the stack.
    #[allow(dead_code)]
    pub fn pop_input(&mut self) -> Option<InputBuffer> {
        self.input.pop()
    }

    /// Return the current position as `(line, col, byte_offset)`.
    pub fn current_pos(&self) -> (usize, usize, usize) {
        self.input.current_pos()
    }

    /// §16.5.3: resolve a token payload ([`XmlText`]) to its bytes — an
    /// owned payload returns the Vec; a span reads the base input's data
    /// range. Valid while the token is alive (see [`XmlText`]'s safety
    /// contract).
    pub(crate) fn text_bytes<'a>(&'a self, t: &'a XmlText) -> &'a [u8] {
        match t {
            XmlText::Owned(v) => v,
            XmlText::Span { start, end } => self.input.base_input_range(*start, *end),
        }
    }

    // ── Error recording ─────────────────────────────────────────────────────

    /// Record a parser error at the current input position.
    #[allow(clippy::too_many_arguments)]
    pub fn record_error(
        &mut self,
        domain: c_int,
        code: c_int,
        level: c_int,
        msg: String,
        str1: Option<Vec<u8>>,
        str2: Option<Vec<u8>>,
        str3: Option<Vec<u8>>,
        int1: c_int,
        enc_bytes: Option<([u8; 4], usize)>,
    ) {
        let pos = self.input.current_pos().2;
        self.record_error_at(
            domain, code, level, msg, str1, str2, str3, int1, pos, enc_bytes,
        );
    }

    /// Record a parser error at an arbitrary byte position (token start).
    #[allow(clippy::too_many_arguments)]
    pub fn record_error_at(
        &mut self,
        domain: c_int,
        code: c_int,
        level: c_int,
        msg: String,
        str1: Option<Vec<u8>>,
        str2: Option<Vec<u8>>,
        str3: Option<Vec<u8>>,
        int1: c_int,
        byte_pos: usize,
        enc_bytes: Option<([u8; 4], usize)>,
    ) {
        let (line, col) = self.line_col_at(byte_pos);
        let window = self.window_at(byte_pos);
        self.errors.push(ErrorInfo {
            domain,
            code,
            level,
            msg,
            str1,
            str2,
            str3,
            int1,
            line,
            col,
            byte_pos,
            window,
            enc_bytes,
        });
    }

    /// Move the cursor to an absolute byte offset, recomputing line/column the
    /// way upstream's line/column tracking would have (used when a bulk construct
    /// scan must be REWOUND to a point where the parser actually stopped —
    /// upstream's internal-subset loop returns early on a content error while
    /// the tokenizer consumed the whole available subset).
    pub fn move_cursor_to(&mut self, byte_pos: usize) {
        let (line, col) = self.line_col_at(byte_pos);
        self.input
            .current()
            .set_diagnostic_position(byte_pos, line as usize, col as usize);
    }

    /// Drain the recorded errors (in order).
    pub fn take_errors(&mut self) -> Vec<ErrorInfo> {
        core::mem::take(&mut self.errors)
    }

    /// Subtract `lag` from the COLUMN of the most recently recorded error.
    ///
    /// `ErrorInfo::col` is normally derived from the byte offset
    /// ([`Self::line_col_at`]), but upstream raises at the parser's CURRENT
    /// column, which a construct can leave behind the true byte column (the
    /// internal-subset entity-value opening quote — see
    /// [`dtd_col_lag_at`]). Only the column is nudged; the byte position — and
    /// hence the source window — is untouched.
    pub fn apply_col_lag_to_last_error(&mut self, lag: usize) {
        if let Some(e) = self.errors.last_mut() {
            e.col = e.col.saturating_sub(lag as c_int);
        }
    }

    /// Record a diagnostic at the CURRENT position that upstream raises later
    /// in the same construct (see `deferred_errors`).
    pub fn record_deferred_error(
        &mut self,
        domain: c_int,
        code: c_int,
        level: c_int,
        msg: String,
        str1: Option<Vec<u8>>,
        str2: Option<Vec<u8>>,
        str3: Option<Vec<u8>>,
        int1: c_int,
        enc_bytes: Option<([u8; 4], usize)>,
    ) {
        let pos = self.input.current_pos().2;
        self.record_deferred_error_at(
            domain, code, level, msg, str1, str2, str3, int1, pos, enc_bytes,
        );
    }

    /// Record a diagnostic that upstream raises LATER in the same construct
    /// than the general scan would. See `deferred_errors`.
    #[allow(clippy::too_many_arguments)]
    pub fn record_deferred_error_at(
        &mut self,
        domain: c_int,
        code: c_int,
        level: c_int,
        msg: String,
        str1: Option<Vec<u8>>,
        str2: Option<Vec<u8>>,
        str3: Option<Vec<u8>>,
        int1: c_int,
        byte_pos: usize,
        enc_bytes: Option<([u8; 4], usize)>,
    ) {
        let (line, col) = self.line_col_at(byte_pos);
        let window = self.window_at(byte_pos);
        self.deferred_errors.push(ErrorInfo {
            domain,
            code,
            level,
            msg,
            str1,
            str2,
            str3,
            int1,
            line,
            col,
            byte_pos,
            window,
            enc_bytes,
        });
    }

    /// Drain the diagnostics the tokenizer DEFERRED to a later point in the
    /// construct it just scanned.
    pub fn take_deferred_errors(&mut self) -> Vec<ErrorInfo> {
        core::mem::take(&mut self.deferred_errors)
    }

    /// Drain only the deferred diagnostics whose code appears in `codes`,
    /// leaving the rest queued for their own upstream raise point.
    pub fn take_deferred_errors_matching(&mut self, codes: &[c_int]) -> Vec<ErrorInfo> {
        let mut matched = Vec::new();
        let mut rest = Vec::new();
        for e in core::mem::take(&mut self.deferred_errors) {
            if codes.contains(&e.code) {
                matched.push(e);
            } else {
                rest.push(e);
            }
        }
        self.deferred_errors = rest;
        matched
    }

    /// The codes of the diagnostics recorded by the last scan, WITHOUT
    /// consuming them. The push driver uses this to distinguish two failures
    /// that present identically as an unterminated start tag but end in
    /// different upstream states (see `pushdrive::open_start_tag`).
    #[allow(dead_code)]
    pub(crate) fn peek_error_codes(&self) -> Vec<c_int> {
        self.errors.iter().map(|e| e.code).collect()
    }

    /// The codes of the DEFERRED diagnostics recorded by the last scan,
    /// WITHOUT consuming them (see `deferred_errors`).
    pub(crate) fn peek_deferred_error_codes(&self) -> Vec<c_int> {
        self.deferred_errors.iter().map(|e| e.code).collect()
    }

    /// Whether the current input has no bytes at all (upstream
    /// `xmlParseDocument` `CUR == 0` check → "Document is empty").
    pub fn is_input_empty(&self) -> bool {
        self.input.current_ref().consumed().is_empty()
            && self.input.current_ref().remaining().is_empty()
    }

    /// Capture `(line, byte-col, window)` at the current position for
    /// parser-side error raising.
    pub fn capture_error_pos(&self) -> (c_int, c_int, Option<(Vec<u8>, usize)>) {
        let byte_pos = self.input.current_pos().2;
        // Upstream reports the CURRENT input's TRACKED line/col (`input->line`,
        // `input->col`), never a value reconstructed from the byte offset. The
        // two differ after a raw `NEXT` consume that skips the column bump —
        // notably the trailing `;` of a general entity reference (upstream
        // xmlParseEntityRefInternal ends with NEXT without xmlCurrentChar
        // col++), after which the column lags the byte offset by one for the
        // rest of the line. Recomputing hid that lag from every error raised at
        // the cursor (the oracle-shadow court's entity documents).
        let (line, col, _) = self.input.current_ref().pos();
        let window = self.window_at(byte_pos);
        (line as c_int, col as c_int, window)
    }

    /// Compute the 1-based line and character column for a byte position,
    /// using the same line-break semantics as `InputBuffer::advance_past_char`
    /// (`\r\n` = one break, `\r` = one break, `\n` = one break). The column
    /// counts characters (upstream `input->col` semantics).
    fn line_col_at(&self, byte_pos: usize) -> (c_int, c_int) {
        let consumed = self.input.current_ref().consumed();
        let end = byte_pos.min(consumed.len());
        let mut line = 1i32;
        let mut col = 1i32;
        let mut i = 0usize;
        while i < end {
            match consumed[i] {
                b'\n' => {
                    line += 1;
                    col = 1;
                    i += 1;
                }
                b'\r' => {
                    if i + 1 < end && consumed[i + 1] == b'\n' {
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
                _ => {
                    let l = utf8_char_len(&consumed[i..]);
                    if l == 0 {
                        col += 1;
                        i += 1;
                    } else {
                        col += 1;
                        i += l;
                    }
                }
            }
        }
        (line, col)
    }

    /// Build the source window + 0-based caret column at a byte position,
    /// replicating upstream `xmlParserInputGetWindow` (parserInternals.c):
    /// skip back over trailing EOLs, search back at most 80 bytes for the
    /// line start, then scan forward at most 80 bytes of valid UTF-8.
    fn window_at(&self, byte_pos: usize) -> Option<(Vec<u8>, usize)> {
        let consumed = self.input.current_ref().consumed();
        let remaining = self.input.current_ref().remaining();
        let mut data = Vec::with_capacity(consumed.len() + remaining.len());
        data.extend_from_slice(consumed);
        data.extend_from_slice(remaining);
        window_at_data(&data, byte_pos)
    }

    // ── Token scanning ──────────────────────────────────────────────────────

    /// Read the next token from the input, skipping leading whitespace.
    ///
    /// Returns `XmlToken::Eof` when input is exhausted.
    pub fn next_token(&mut self) -> XmlToken {
        // Check for pushed-back token first.
        if let Some(token) = self.push_back.take() {
            return token;
        }
        self.skip_whitespace();
        self.next_token_raw()
    }

    /// Read the next token (skipping leading whitespace), also returning
    /// the byte offset of the token's first byte (used to attribute errors
    /// to the token start, e.g. "Start tag expected").
    pub fn next_token_with_start(&mut self) -> (XmlToken, usize) {
        if let Some(token) = self.push_back.take() {
            return (token, 0);
        }
        self.skip_whitespace();
        let start = self.input.current_pos().2;
        let token = self.next_token_raw();
        (token, start)
    }

    /// Push a token back onto the input, to be returned by the next `next_token` call.
    ///
    /// Only one token can be pushed back at a time.
    pub fn push_back_token(&mut self, token: XmlToken) {
        self.push_back = Some(token);
    }

    /// Read the next token without skipping leading whitespace.
    /// Used for content inside elements where whitespace is significant.
    pub fn next_token_raw(&mut self) -> XmlToken {
        // Check for pushed-back token first.
        if let Some(token) = self.push_back.take() {
            return token;
        }
        if self.input.is_eof() {
            return XmlToken::Eof;
        }
        self.last_token_start = self.input.current_pos().2;

        match self.input.peek_char() {
            Some('<') => self.scan_tag_or_markup(),
            Some('&') => self.scan_reference(),
            Some(_) => self.scan_characters(),
            None => {
                if self.input.peek_raw().is_some() {
                    // Invalid UTF-8 bytes: the character-data scanner records
                    // the encoding error and skips the byte.
                    self.scan_characters()
                } else {
                    XmlToken::Eof
                }
            }
        }
    }

    /// Skip whitespace characters (space, tab, CR, LF, form feed) — §16.6
    /// bulk byte-run scan with per-char-identical line/col semantics.
    fn skip_whitespace(&mut self) {
        self.input.skip_ascii_whitespace();
    }

    // ── Tag/markup scanning ─────────────────────────────────────────────────

    /// Scan after seeing '<'. Determines whether this is a tag, comment, PI, CDATA, or DOCTYPE.
    fn scan_tag_or_markup(&mut self) -> XmlToken {
        debug_assert_eq!(self.input.peek_char(), Some('<'));
        let start_pos = self.input.current_pos().2;
        // Consume '<'
        self.input.read_char();

        // UPSTREAM-PARITY: '<' at the end of the available input is NOT text
        // — it starts a tag whose name parse fails (or, on a non-final push
        // call, waits for more data). Routing it through scan_start_tag
        // records XML_ERR_NAME_REQUIRED "StartTag: invalid element name" and
        // marks the tag unterminated, so incremental probes pause instead of
        // delivering a bogus '<' character event (SP-14.3.1-8, gh20439_1's
        // per-character feed; the oracle: push non-final "<" waits, final
        // and pull "<" raise 68).
        match self.input.peek_char() {
            Some('/') => self.scan_end_tag(start_pos),
            Some('?') => self.scan_pi_or_xml_decl(start_pos),
            Some('!') => {
                // KEY-2 (content-`<!`-markup rule): a `<!` is only a markup
                // construct when it is `<!--` (comment), `<![CDATA[`, or
                // `<!DOCTYPE` in its legal position. Everything else
                // (`<!ENTITY`, `<!ELEMENT`, `<!ATTLIST`, `<!NOTATION`, any
                // unknown `<!...`) is what upstream's xmlParseStartTag sees:
                // the byte after '<' is not a name character, so the element
                // name parse fails with XML_ERR_NAME_REQUIRED (68)
                // "StartTag: invalid element name" — upstream never swallows
                // such a construct as text in element content. Routing it
                // through scan_start_tag (empty name -> 68 at the '!')
                // reproduces the oracle, incl. clearing wellFormed.
                let nb = self.peek_bytes(10);
                let comment = nb.len() >= 3 && nb[1] == b'-' && nb[2] == b'-';
                let cdata = nb.len() >= 8 && nb[1] == b'[' && &nb[2..8] == b"CDATA[";
                let doctype = nb.len() >= 8 && nb[1..8].eq_ignore_ascii_case(b"DOCTYPE");
                if comment || cdata || doctype {
                    self.scan_markup_decl(start_pos)
                } else {
                    self.scan_start_tag()
                }
            }
            // Some(_) | None: an empty or invalid element name (EOF or a
            // non-name byte right after '<') fails the start-tag parse.
            _ => self.scan_start_tag(),
        }
    }

    /// Scan an end tag: `</name>`
    fn scan_end_tag(&mut self, start_pos: usize) -> XmlToken {
        debug_assert_eq!(self.input.peek_char(), Some('/'));
        // Consume '/'
        self.input.read_char();

        let name = self.scan_name();
        self.skip_whitespace();

        // Expect '>' (upstream xmlParseEndTag2: SKIP_BLANKS then
        // `(!IS_BYTE_CHAR(RAW)) || (RAW != '>')` → xmlFatalErr(
        // XML_ERR_GT_REQUIRED) "expected '>'>\n"; only a real '>' is
        // consumed). EOF is reported by the parser as unterminated.
        if self.input.peek_char() == Some('>') {
            self.input.read_char();
            XmlToken::EndTag {
                name,
                start_pos,
                unterminated: false,
            }
        } else if self.input.is_eof() {
            XmlToken::EndTag {
                name,
                start_pos,
                unterminated: true,
            }
        } else {
            self.record_error(
                crate::abi::types::XML_FROM_PARSER,
                crate::abi::types::XML_ERR_GT_REQUIRED,
                crate::abi::types::xmlErrorLevel::XML_ERR_FATAL as c_int,
                "expected '>'\n".to_string(),
                None,
                None,
                None,
                0,
                None,
            );
            XmlToken::EndTag {
                name,
                start_pos,
                unterminated: false,
            }
        }
    }

    /// Scan a start tag: `<name ...>` or `<name ... />`, replicating
    /// upstream `xmlParseStartTag2` error semantics (11.1-M): invalid
    /// names, attribute-value errors, missing values, duplicate
    /// attributes, "attributes construct error", and unterminated tags are
    /// recorded with upstream codes/levels at the exact detection position.
    fn scan_start_tag(&mut self) -> XmlToken {
        // UPSTREAM-PARITY (parser.c xmlParseTryOrFinish XML_PARSER_START_TAG
        // state): a start tag is only scanned once a '>' is available in the
        // current buffer (xmlParseLookupGt gate). Without it a non-final push
        // call defers the whole tag — name/attribute/close constructs may
        // complete on a later push call (`<a b="c"/` fed alone must stay
        // clean; only the terminating parse reports the truncation).
        if self.silent_truncated
            && !self
                .input
                .current_ref()
                .remaining()
                .iter()
                .any(|&b| b == b'>')
        {
            return XmlToken::StartTag {
                name: Vec::new(),
                attributes: Vec::new(),
                attr_end: Vec::new(),
                attr_start: Vec::new(),
                end_pos: self.input.current_pos().2,
                empty: false,
                unterminated: true,
            };
        }
        let name = self.scan_name();
        let attributes: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        let mut empty = false;
        let mut unterminated = false;
        // Byte offset of the tag's closing '>'/'/' (upstream raises the
        // duplicate-attribute error with RAW still at the tag end).
        let mut end_pos: Option<usize> = None;
        // 1-based line of the tag's start (upstream `pushTab[].line` used in
        // the "Couldn't find end of Start Tag %s line %d" message).
        let open_line = self.input.current_pos().0 as c_int;

        if name.is_empty() {
            // upstream xmlParseStartTag2: name == NULL →
            // "StartTag: invalid element name\n" (XML_ERR_NAME_REQUIRED);
            // the SAX1 scanner xmlParseStartTag words it
            // "xmlParseStartTag: invalid element name\n".
            // (A lone '<' or a '< ' at the end of the available input never
            // reaches this point in incremental probes/partial deliveries —
            // the no-'>' gate above defers the whole tag.)
            self.record_error(
                crate::abi::types::XML_FROM_PARSER,
                crate::abi::types::XML_ERR_NAME_REQUIRED,
                crate::abi::types::xmlErrorLevel::XML_ERR_FATAL as c_int,
                if self.sax2 {
                    "StartTag: invalid element name\n".to_string()
                } else {
                    "xmlParseStartTag: invalid element name\n".to_string()
                },
                None,
                None,
                None,
                0,
                None,
            );
            // Consume until '>' or EOF so scanning cannot stall (upstream
            // continues as character data after the failed start tag).
            while let Some(c) = self.input.peek_char() {
                if c == '>' {
                    self.input.read_char();
                    break;
                }
                self.input.read_char();
            }
            return XmlToken::StartTag {
                name,
                attributes,
                attr_end: Vec::new(),
                attr_start: Vec::new(),
                end_pos: self.input.current_pos().2,
                empty,
                unterminated: true,
            };
        }

        // UPSTREAM-PARITY (parser.c xmlParseStartTag2 + xmlParseName): a
        // name longer than XML_MAX_NAME_LENGTH (50 000 bytes, or
        // XML_MAX_TEXT_LENGTH with XML_PARSE_HUGE) fails the element-name
        // parse — the tag then raises XML_ERR_NAME_REQUIRED "StartTag:
        // invalid element name" and the element is never reported
        // (SP-14.3.1-6: XML_OPTION_PARSE_HUGE — the 5 MB name must error
        // without HUGE and parse with it).
        if name.len() > self.max_name_length {
            self.record_error(
                crate::abi::types::XML_FROM_PARSER,
                crate::abi::types::XML_ERR_NAME_REQUIRED,
                crate::abi::types::xmlErrorLevel::XML_ERR_FATAL as c_int,
                "StartTag: invalid element name\n".to_string(),
                None,
                None,
                None,
                0,
                None,
            );
            return XmlToken::StartTag {
                name,
                attributes,
                attr_end: Vec::new(),
                attr_start: Vec::new(),
                end_pos: self.input.current_pos().2,
                empty,
                unterminated: true,
            };
        }

        let mut attributes: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        let mut attr_end: Vec<usize> = Vec::new();
        // Byte offset just after each attribute value's opening quote
        // (start of the raw value; used for the '<' in entity error caret).
        let mut attr_start: Vec<usize> = Vec::new();
        loop {
            self.skip_whitespace();

            match self.input.peek_char() {
                Some('>') => {
                    end_pos = Some(self.input.current_pos().2);
                    self.input.read_char();
                    break;
                }
                Some('/') if self.peek_bytes(2).get(1) == Some(&b'>') => {
                    // Self-closing tag: <name .../>. UPSTREAM-PARITY
                    // (parser.c xmlParseStartTag2): the attribute loop only
                    // stops at '/' when NXT(1) == '>' — any other '/'
                    // (e.g. `<a/foo>`) is parsed as an attribute, whose
                    // name scan fails on '/': "error parsing attribute
                    // name\n" + "Couldn't find end of Start Tag" (the
                    // `Some(_)` arm below — scan_name on '/' is empty).
                    end_pos = Some(self.input.current_pos().2);
                    self.input.read_char();
                    self.input.read_char();
                    empty = true;
                    break;
                }
                None => {
                    // EOF before the tag closed (upstream end-of-tag check).
                    unterminated = true;
                    break;
                }
                Some(_) => {
                    // Scan attribute: name="value" | name='value'
                    let attr_name = self.scan_name();
                    if attr_name.is_empty() {
                        // upstream xmlParseAttribute2: name == NULL.
                        self.record_error(
                            crate::abi::types::XML_FROM_PARSER,
                            crate::abi::types::XML_ERR_NAME_REQUIRED,
                            crate::abi::types::xmlErrorLevel::XML_ERR_FATAL as c_int,
                            "error parsing attribute name\n".to_string(),
                            None,
                            None,
                            None,
                            0,
                            None,
                        );
                        unterminated = true;
                        break;
                    }
                    self.skip_whitespace();

                    let mut attr_value: Option<Vec<u8>> = None;
                    let mut value_start: usize = 0;
                    if self.input.peek_char() == Some('=') {
                        self.input.read_char();
                        self.skip_whitespace();
                        match self.input.peek_char() {
                            Some(q @ ('"' | '\'')) => {
                                self.input.read_char();
                                value_start = self.input.current_pos().2;
                                let (value, closed) = self.scan_attr_value_inner(q);
                                if !closed {
                                    // upstream xmlParseAttValueInternal at
                                    // EOF: "AttValue: ' expected\n" (40).
                                    self.record_error(
                                        crate::abi::types::XML_FROM_PARSER,
                                        crate::abi::types::XML_ERR_ATTRIBUTE_NOT_FINISHED,
                                        crate::abi::types::xmlErrorLevel::XML_ERR_FATAL as c_int,
                                        "AttValue: ' expected\n".to_string(),
                                        None,
                                        None,
                                        None,
                                        0,
                                        None,
                                    );
                                } else {
                                    attr_value = Some(value);
                                }
                            }
                            _ => {
                                // upstream xmlParseAttValueInternal: value
                                // not quoted → "AttValue: \" or ' expected\n"
                                // (39).
                                self.record_error(
                                    crate::abi::types::XML_FROM_PARSER,
                                    crate::abi::types::XML_ERR_ATTRIBUTE_NOT_STARTED,
                                    crate::abi::types::xmlErrorLevel::XML_ERR_FATAL as c_int,
                                    "AttValue: \" or ' expected\n".to_string(),
                                    None,
                                    None,
                                    None,
                                    0,
                                    None,
                                );
                            }
                        }
                    } else {
                        // upstream xmlParseAttribute2: RAW != '=' →
                        // "Specification mandates value for attribute %s\n"
                        // (41), str1 = name.
                        self.record_error(
                            crate::abi::types::XML_FROM_PARSER,
                            crate::abi::types::XML_ERR_ATTRIBUTE_WITHOUT_VALUE,
                            crate::abi::types::xmlErrorLevel::XML_ERR_FATAL as c_int,
                            format!(
                                "Specification mandates value for attribute {}\n",
                                String::from_utf8_lossy(&attr_name)
                            ),
                            Some(attr_name.clone()),
                            None,
                            None,
                            0,
                            None,
                        );
                    }

                    if let Some(v) = attr_value {
                        // The value-end offset is captured just past the
                        // closing quote (upstream's error position for
                        // namespace-URI diagnostics).
                        attr_end.push(self.input.current_pos().2);
                        attr_start.push(value_start);
                        attributes.push((attr_name, v));
                    }

                    // upstream `next_attr`: the tag end is allowed directly;
                    // otherwise blanks must follow, else "attributes
                    // construct error\n" (65).
                    match self.input.peek_char() {
                        Some('>') => {
                            end_pos = Some(self.input.current_pos().2);
                            self.input.read_char();
                            break;
                        }
                        Some('/') if self.peek_bytes(2).get(1) == Some(&b'>') => {
                            end_pos = Some(self.input.current_pos().2);
                            self.input.read_char();
                            self.input.read_char();
                            empty = true;
                            break;
                        }
                        _ => {
                            let before = self.input.current_pos().2;
                            self.skip_whitespace();
                            if self.input.current_pos().2 == before {
                                self.record_error(
                                    crate::abi::types::XML_FROM_PARSER,
                                    crate::abi::types::XML_ERR_SPACE_REQUIRED,
                                    crate::abi::types::xmlErrorLevel::XML_ERR_FATAL as c_int,
                                    "attributes construct error\n".to_string(),
                                    None,
                                    None,
                                    None,
                                    0,
                                    None,
                                );
                                // UPSTREAM-PARITY (parser.c xmlParseStartTag2
                                // vs xmlParseStartTag): the SAX2 scanner's
                                // `next_attr` arm BREAKS out of the attribute
                                // loop here, leaving the tag unterminated so
                                // the START_TAG arm reports GT_REQUIRED
                                // ("Couldn't find end of Start Tag %s"). The
                                // SAX1 scanner has no `break` — it reports the
                                // SAME error and keeps reading attributes, so
                                // `<a b='1'c='2'/>` still completes and only
                                // the 65 is observable (court attr-nospaces
                                // `-s sax1`: one error, p=15, EPILOG).
                                if self.sax2 {
                                    unterminated = true;
                                    break;
                                }
                            }
                            // Blanks consumed (or SAX1): continue the loop.
                        }
                    }
                }
            }
        }

        // upstream: duplicate attribute names are detected after the tag
        // (RAW still at '>' or '/') → "Attribute %s redefined\n" (42),
        // str1 = name.
        let dup_pos = end_pos.unwrap_or_else(|| self.input.current_pos().2);
        // §16.5.4: the duplicate scan compares attribute NAME BYTES in place
        // — the names are already owned inside `attributes`, so the previous
        // per-name `Vec` clone into a `seen` list cost one heap allocation
        // per attribute on EVERY tag. Real tags carry few attributes, where a
        // nested slice compare beats hashing; only pathological attribute
        // counts (> 8) pay for a hash set, keeping the worst case O(k).
        let dup_attr: Option<usize> = if attributes.len() >= 8 {
            let mut seen: std::collections::HashSet<&[u8]> =
                std::collections::HashSet::with_capacity(attributes.len() * 2);
            attributes
                .iter()
                .position(|(an, _)| !seen.insert(an.as_slice()))
        } else {
            let mut found = None;
            'outer: for i in 1..attributes.len() {
                for j in 0..i {
                    if attributes[i].0 == attributes[j].0 {
                        found = Some(i);
                        break 'outer;
                    }
                }
            }
            found
        };
        if let Some(dup) = dup_attr {
            let an = &attributes[dup].0;
            self.record_deferred_error_at(
                crate::abi::types::XML_FROM_PARSER,
                crate::abi::types::XML_ERR_ATTRIBUTE_REDEFINED,
                crate::abi::types::xmlErrorLevel::XML_ERR_FATAL as c_int,
                format!("Attribute {} redefined\n", String::from_utf8_lossy(an)),
                Some(an.clone()),
                None,
                None,
                0,
                dup_pos,
                None,
            );
        }

        if unterminated {
            // The start-tag end check failed. Which upstream caller we are
            // scanning for decides the diagnostic (see `push_start_tag`).
            let local = match name.iter().rposition(|&b| b == b':') {
                Some(i) => &name[i + 1..],
                None => name.as_slice(),
            };
            if self.push_start_tag {
                // xmlParseTryOrFinish's START_TAG arm: no line, int1 = 0.
                //
                // DEFERRED: this diagnostic is raised AFTER the start-element
                // event. upstream xmlParseStartTag2 dispatches
                // startElementNs itself and only the ARM (which runs once
                // xmlParseStartTag2 returned) reports the missing '>' — see the
                // oracle's `starttag-trunc-name` cell (startElement, then the
                // code-73 error).
                self.record_deferred_error(
                    crate::abi::types::XML_FROM_PARSER,
                    crate::abi::types::XML_ERR_GT_REQUIRED,
                    crate::abi::types::xmlErrorLevel::XML_ERR_FATAL as c_int,
                    format!(
                        "Couldn't find end of Start Tag {}\n",
                        String::from_utf8_lossy(local)
                    ),
                    Some(name.clone()),
                    None,
                    None,
                    0,
                    None,
                );
            } else {
                // upstream xmlParseElementStart end-of-tag check:
                // "Couldn't find end of Start Tag %s line %d\n" (73),
                // str1 = local name (xmlParseStartTag2 returns localname),
                // int1 = start line.
                self.record_error(
                    crate::abi::types::XML_FROM_PARSER,
                    crate::abi::types::XML_ERR_GT_REQUIRED,
                    crate::abi::types::xmlErrorLevel::XML_ERR_FATAL as c_int,
                    format!(
                        "Couldn't find end of Start Tag {} line {}\n",
                        String::from_utf8_lossy(local),
                        open_line
                    ),
                    Some(name.clone()),
                    None,
                    None,
                    open_line,
                    None,
                );
            }
        }

        XmlToken::StartTag {
            name,
            attributes,
            attr_end,
            attr_start,
            end_pos: end_pos.unwrap_or_else(|| self.input.current_pos().2),
            empty,
            unterminated,
        }
    }

    /// Scan a quoted attribute value; returns the raw bytes and whether the
    /// closing quote was found. Entity references without a trailing ';'
    /// raise upstream's "EntityRef: expecting ';'\n" (23), and invalid
    /// UTF-8 raises the I/O encoding error (81).
    fn scan_attr_value_inner(&mut self, quote: char) -> (Vec<u8>, bool) {
        let mut value = Vec::new();
        loop {
            if self.input.is_eof() {
                return (value, false);
            }
            // Invalid UTF-8 byte: upstream xmlCurrentChar encoding error.
            if let Some(b) = self.input.peek_raw() {
                if b >= 0x80 && self.input.peek_char().is_none() {
                    self.record_encoding_error();
                    self.input.skip_raw_bytes(1);
                    // UPSTREAM-PARITY (parserInternals.c xmlCurrentChar
                    // encoding_error): the byte is consumed and replaced
                    // with XML_INVALID_CHAR (U+FFFD) in the content — not
                    // dropped.
                    value.extend_from_slice(b"\xEF\xBF\xBD");
                    continue;
                }
            }
            match self.input.peek_char() {
                Some(c) if c == quote => {
                    // §16.5.5: consume the already-peeked char.
                    self.input.consume_peeked();
                    return (value, true);
                }
                Some('&') => {
                    self.input.consume_peeked();
                    value.push(b'&');
                    let mut name = Vec::new();
                    loop {
                        match self.input.peek_char() {
                            Some(c) if is_name_byte(c as u8) => {
                                value.push(c as u8);
                                name.push(c as u8);
                                // §16.5.5: already peeked — no second decode.
                                self.input.consume_peeked();
                            }
                            _ => break,
                        }
                    }
                    if self.input.peek_char() == Some(';') {
                        value.push(b';');
                        self.input.consume_peeked();
                    } else if !name.is_empty() {
                        // upstream xmlParseEntityRefInternal inside
                        // xmlParseAttValueInternal: RAW != ';'.
                        self.record_error(
                            crate::abi::types::XML_FROM_PARSER,
                            crate::abi::types::XML_ERR_ENTITYREF_SEMICOL_MISSING,
                            crate::abi::types::xmlErrorLevel::XML_ERR_FATAL as c_int,
                            "EntityRef: expecting ';'\n".to_string(),
                            None,
                            None,
                            None,
                            0,
                            None,
                        );
                    }
                }
                Some(c) => {
                    if c == '<' {
                        // UPSTREAM-PARITY (parser.c xmlParseAttValueInternal):
                        // a raw '<' inside an attribute value is a fatal WFC
                        // violation, but the character still becomes part of
                        // the value.
                        self.record_error(
                            crate::abi::types::XML_FROM_PARSER,
                            crate::abi::types::XML_ERR_LT_IN_ATTRIBUTE,
                            crate::abi::types::xmlErrorLevel::XML_ERR_FATAL as c_int,
                            "Unescaped '<' not allowed in attributes values\n".to_string(),
                            None,
                            None,
                            None,
                            0,
                            None,
                        );
                    }
                    // UPSTREAM-PARITY (parser.c 2.15
                    // xmlParseAttValueComplex): a control character that is
                    // not an XML Char (NUL, 0x1-0x8, 0xB, 0xC, 0xE-0x1F —
                    // `!IS_BYTE_CHAR`) raises "invalid character in
                    // attribute value\n" (XML_ERR_INVALID_CHAR) and is
                    // replaced with U+FFFD in the value (xmlSBufAddReplChar).
                    let cp = c as u32;
                    if cp < 0x20 && !matches!(cp, 0x09 | 0x0A | 0x0D) {
                        self.record_error(
                            crate::abi::types::XML_FROM_PARSER,
                            crate::abi::types::XML_ERR_INVALID_CHAR,
                            crate::abi::types::xmlErrorLevel::XML_ERR_FATAL as c_int,
                            "invalid character in attribute value\n".to_string(),
                            None,
                            None,
                            None,
                            cp as c_int,
                            None,
                        );
                        value.extend_from_slice(b"\xEF\xBF\xBD");
                        self.input.consume_peeked();
                        continue;
                    }
                    // UPSTREAM-PARITY (parser.c xmlParseAttValueInternal, XML
                    // spec 3.3.3 Attribute-Value Normalization): a literal
                    // whitespace character (#x20, #xD, #xA, #x9) is processed
                    // by appending a single #x20 to the normalized value —
                    // for EVERY attribute type (the CDATA-vs-other distinction
                    // only controls the later collapse/trim in
                    // substitute_refs). EOL handling (§2.11, xmlCurrentChar)
                    // already delivered any literal CR as a single LF, so
                    // \\t and \\n are the only control whitespace that reach
                    // this arm; each becomes exactly one space, and literal
                    // spaces (0x20) pass through unchanged (CDATA values keep
                    // space runs raw).
                    let out = if c == '\t' || c == '\n' || c == '\r' {
                        ' '
                    } else {
                        c
                    };
                    Self::push_char(&mut value, out);
                    // §16.5.5: already peeked — no second decode.
                    self.input.consume_peeked();
                }
                None => return (value, false),
            }
        }
    }

    /// Record the I/O-domain encoding error (upstream `xmlCurrentChar` /
    /// `xmlUTF8MultibyteLen` encoding_error path): message "Invalid bytes
    /// in character encoding\n" (81), carrying the 4 bytes at the current
    /// position for the "Bytes:" fragment.
    fn record_encoding_error(&mut self) {
        let pos = self.input.current_pos().2;
        let remaining = self.input.current_ref().remaining();
        // UPSTREAM-PARITY (error.c xmlFormatError "Bytes:" dump): only the
        // bytes actually present in the input are shown — the loop breaks
        // at input->end (no zero padding for a sequence truncated by EOF).
        let mut bytes = [0u8; 4];
        for (i, slot) in bytes.iter_mut().enumerate() {
            if i < remaining.len() {
                *slot = remaining[i];
            }
        }
        self.record_error_at(
            crate::abi::types::XML_FROM_IO,
            crate::abi::types::XML_ERR_INVALID_ENCODING,
            crate::abi::types::xmlErrorLevel::XML_ERR_FATAL as c_int,
            "Invalid bytes in character encoding\n".to_string(),
            None,
            None,
            None,
            0,
            pos,
            Some((bytes, remaining.len().min(4))),
        );
    }

    /// Scan a PI or XML declaration after `<?`.
    fn scan_pi_or_xml_decl(&mut self, start_pos: usize) -> XmlToken {
        debug_assert_eq!(self.input.peek_char(), Some('?'));
        // Consume '?'
        self.input.read_char();

        // UPSTREAM-PARITY (parser.c xmlParseTryOrFinish: every PI-bearing state
        // — XML_DECL, MISC, CONTENT, EPILOG — gates xmlParsePI behind
        // `(!terminate) && (!xmlParseLookupString(ctxt, 2, "?>", 2))`): on a
        // NON-final push call a `<?` construct is only scanned once its `?>`
        // is already present in the available input; otherwise the whole
        // construct is deferred (xmlParseLookupString searches from cur+2, so
        // even `<?>` parks until it is complete). This subsumes the
        // truncation cases of the declaration, an empty target (`<?`) and a
        // complete-looking reserved name (`<?xml` with no blank yet): none of
        // them reaches xmlParsePI/xmlParseXMLDecl upstream, so none of them
        // may raise PI_NOT_STARTED / RESERVED_XML_NAME on a non-final call.
        if self.silent_truncated
            && !self
                .input
                .current_ref()
                .remaining()
                .windows(2)
                .any(|w| w == b"?>")
        {
            return XmlToken::Eof;
        }

        // Peek ahead to see if the next characters are "xml" (case-sensitive)
        // followed by a blank.
        let next_bytes = self.peek_bytes(4);
        // UPSTREAM-PARITY (parser.c xmlParseDocument): `<?xml` is an XML
        // declaration ONLY at the very start of the document AND when the
        // character after "xml" is a blank (space/tab/CR/LF) — lowercase only
        // (CMP5 + IS_BLANK(NXT(5))). Every other `<?xml...` is an ordinary PI
        // whose target must pass xmlParsePITarget (the reserved "xml" target
        // names are FATAL XML_ERR_RESERVED_XML_NAME; "xml-stylesheet" /
        // "xml-model" are exempt). The previous any-case-insensitive
        // `<?xml`-at-0 routing misparsed `<?xml?>`, `<?xml>`, `<?XML ...?>`
        // and even the legal `<?xml-stylesheet ...?>` as XML declarations
        // (KEY-3, xml_error_string rows 47/64).
        let is_xml_decl = self.input.at_base_input()
            && start_pos == self.input.doc_start_offset()
            && next_bytes.len() >= 4
            && next_bytes[0] == b'x'
            && next_bytes[1] == b'm'
            && next_bytes[2] == b'l'
            && matches!(next_bytes[3], b' ' | b'\t' | b'\r' | b'\n');

        if is_xml_decl {
            // Consume "xml"
            self.input.read_char(); // x
            self.input.read_char(); // m
            self.input.read_char(); // l
            return self.scan_xml_decl_rest();
        }

        // Regular processing instruction. Mirror upstream xmlParsePITarget:
        // a target that starts with "xml" (case-insensitive) is reserved.
        let target = self.scan_name();
        if target.is_empty() {
            // upstream xmlParsePI: target == NULL → "xmlParsePI : no target
            // name\n" (XML_ERR_PI_NOT_STARTED), at the current position.
            self.record_error(
                crate::abi::types::XML_FROM_PARSER,
                crate::abi::types::XML_ERR_PI_NOT_STARTED,
                crate::abi::types::xmlErrorLevel::XML_ERR_FATAL as c_int,
                "xmlParsePI : no target name\n".to_string(),
                None,
                None,
                None,
                0,
                None,
            );
            return XmlToken::ProcessingInstruction {
                target,
                data: Vec::new(),
                start_pos,
                unterminated: false,
            };
        }
        let xml_reserved = target.len() >= 3
            && matches!(target[0], b'x' | b'X')
            && matches!(target[1], b'm' | b'M')
            && matches!(target[2], b'l' | b'L');
        if xml_reserved {
            if target == b"xml" {
                // Exact lowercase "xml" PI: this would be an XML declaration
                // had it been at the start with a blank after it.
                self.record_error(
                    crate::abi::types::XML_FROM_PARSER,
                    crate::abi::types::XML_ERR_RESERVED_XML_NAME,
                    crate::abi::types::xmlErrorLevel::XML_ERR_FATAL as c_int,
                    "XML declaration allowed only at the start of the document\n".to_string(),
                    None,
                    None,
                    None,
                    0,
                    None,
                );
            } else if target.len() == 3 {
                // "XML"/"xMl"/... case variant of the reserved name.
                self.record_error(
                    crate::abi::types::XML_FROM_PARSER,
                    crate::abi::types::XML_ERR_RESERVED_XML_NAME,
                    crate::abi::types::xmlErrorLevel::XML_ERR_FATAL as c_int,
                    "Reserved XML Name\n".to_string(),
                    None,
                    None,
                    None,
                    0,
                    None,
                );
            } else if target != b"xml-stylesheet" && target != b"xml-model" {
                // xml-prefixed target not in the W3C list: warning only
                // (upstream xmlParsePITarget).
                self.record_error(
                    crate::abi::types::XML_FROM_PARSER,
                    crate::abi::types::XML_ERR_RESERVED_XML_NAME,
                    crate::abi::types::xmlErrorLevel::XML_ERR_WARNING as c_int,
                    "xmlParsePITarget: invalid name prefix 'xml'\n".to_string(),
                    None,
                    None,
                    None,
                    0,
                    None,
                );
            }
        }

        // Skip whitespace before data — upstream xmlParsePI SKIP_BLANKS
        // consumes ALL blanks between the target and the data, and reports
        // XML_ERR_SPACE_REQUIRED when there was none and the PI is not
        // immediately terminated.
        let mut had_blank = false;
        while self
            .input
            .peek_char()
            .is_some_and(|c| c.is_ascii_whitespace())
        {
            had_blank = true;
            self.input.read_char();
        }
        if !had_blank && !matches!(self.peek_bytes(2)[..], [b'?', b'>']) && !self.silent_truncated {
            self.record_error(
                crate::abi::types::XML_FROM_PARSER,
                crate::abi::types::XML_ERR_SPACE_REQUIRED,
                crate::abi::types::xmlErrorLevel::XML_ERR_FATAL as c_int,
                format!(
                    "ParsePI: PI {} space expected\n",
                    String::from_utf8_lossy(&target)
                ),
                Some(target.clone()),
                None,
                None,
                0,
                None,
            );
        }

        // Read data until "?>". Upstream copies every character up to the
        // '?' of the terminator — including any whitespace immediately
        // before it (the SAX data of `<?foo pi contents ?>` is
        // "pi contents ", GH-12167); there is NO trailing trim.
        let mut data = Vec::new();
        let mut terminated = false;
        loop {
            if self.input.is_eof() {
                break;
            }
            if self.input.peek_char() == Some('?') {
                let _saved = self.input.current().pos();
                self.input.read_char();
                if self.input.peek_char() == Some('>') {
                    self.input.read_char();
                    terminated = true;
                    break;
                }
                // Not "?>", push the '?' back... we can't easily unread.
                // Instead, just include the '?' in the data.
                data.push(b'?');
                continue;
            }
            match self.input.read_char() {
                Some(c) => Self::push_char(&mut data, c),
                None => break,
            }
        }

        if !terminated {
            // upstream xmlParsePI end: EOF without "?>" →
            // "ParsePI: PI %s never end ...\n" (XML_ERR_PI_NOT_FINISHED).
            // The scan may continue on a later push call, so an incremental
            // probe pauses instead of delivering the error (SP-14.3.1-8
            // pattern, same as unterminated comments/CDATA; the construct
            // is EOF-truncated by construction).
            if !self.silent_truncated {
                self.record_error(
                    crate::abi::types::XML_FROM_PARSER,
                    crate::abi::types::XML_ERR_PI_NOT_FINISHED,
                    crate::abi::types::xmlErrorLevel::XML_ERR_FATAL as c_int,
                    format!(
                        "ParsePI: PI {} never end ...\n",
                        String::from_utf8_lossy(&target)
                    ),
                    Some(target.clone()),
                    None,
                    None,
                    0,
                    None,
                );
            }
        }

        XmlToken::ProcessingInstruction {
            target,
            data,
            start_pos,
            unterminated: !terminated,
        }
    }

    /// The raw byte at the cursor (upstream `RAW = *input->cur`); 0 at EOF,
    /// mirroring the NUL terminator upstream relies on.
    fn decl_raw(&self) -> u8 {
        self.input.current_ref().peek_raw().unwrap_or(0)
    }

    /// The raw byte `n` ahead of the cursor (upstream `NXT(n)`); 0 past EOF.
    fn decl_nxt(&self, n: usize) -> u8 {
        self.peek_bytes(n + 1).get(n).copied().unwrap_or(0)
    }

    /// `IS_BLANK_CH` — the four XML blanks (note: NOT form feed, unlike
    /// `xmlSkipBlankChars`).
    fn decl_is_blank(b: u8) -> bool {
        matches!(b, b' ' | b'\t' | b'\r' | b'\n')
    }

    /// `SKIP(n)`: advance `n` bytes, bumping the column once per byte (the
    /// keywords this is used on are pure ASCII, so this is `NEXT`-equivalent).
    fn decl_skip(&mut self, n: usize) {
        self.input.current().skip_raw_bytes(n);
    }

    /// `NEXT` (`xmlNextChar`): consume one whole UTF-8 character.
    fn decl_advance(&mut self) {
        self.input.read_char();
    }

    /// Record `xmlFatalErr(ctxt, code, NULL)` — the default message for the
    /// code from `xmlErrString`, terminated by `\n`.
    fn decl_fatal(&mut self, code: c_int, msg: &str) {
        self.record_error(
            crate::abi::types::XML_FROM_PARSER,
            code,
            crate::abi::types::xmlErrorLevel::XML_ERR_FATAL as c_int,
            format!("{msg}\n"),
            None,
            None,
            None,
            0,
            None,
        );
    }

    /// `xmlFatalErrMsg` — an explicit message, already `\n`-terminated.
    fn decl_fatal_msg(&mut self, code: c_int, msg: &str) {
        self.record_error(
            crate::abi::types::XML_FROM_PARSER,
            code,
            crate::abi::types::xmlErrorLevel::XML_ERR_FATAL as c_int,
            msg.to_string(),
            None,
            None,
            None,
            0,
            None,
        );
    }

    /// `xmlParseVersionNum`: `<digit> '.' <digit>*`. Returns an empty vector
    /// where upstream returns NULL.
    fn decl_parse_version_num(&mut self) -> Vec<u8> {
        let mut buf = Vec::new();
        let mut cur = self.decl_raw();
        if !cur.is_ascii_digit() {
            return buf;
        }
        buf.push(cur);
        self.decl_advance();
        cur = self.decl_raw();
        if cur != b'.' {
            return Vec::new();
        }
        buf.push(cur);
        self.decl_advance();
        cur = self.decl_raw();
        while cur.is_ascii_digit() {
            buf.push(cur);
            self.decl_advance();
            cur = self.decl_raw();
        }
        buf
    }

    /// `xmlParseVersionInfo`. Returns `(literal, is_null)` — `is_null` where
    /// upstream returns NULL (the caller then raises VERSION_MISSING).
    fn decl_parse_version_info(&mut self) -> (Vec<u8>, bool) {
        if self.peek_bytes(7) != b"version" {
            return (Vec::new(), true);
        }
        self.decl_skip(7);
        self.skip_whitespace();
        if self.decl_raw() != b'=' {
            // XML_ERR_EQUAL_REQUIRED (75) "expected '='".
            self.decl_fatal(75, "expected '='");
            return (Vec::new(), true);
        }
        self.decl_advance();
        self.skip_whitespace();
        let quote = self.decl_raw();
        if quote == b'"' || quote == b'\'' {
            self.decl_advance();
            let version = self.decl_parse_version_num();
            if self.decl_raw() != quote {
                // XML_ERR_STRING_NOT_CLOSED (34).
                self.decl_fatal(34, "String not closed expecting \" or '");
            } else {
                self.decl_advance();
            }
            let is_null = version.is_empty();
            (version, is_null)
        } else {
            // XML_ERR_STRING_NOT_STARTED (33).
            self.decl_fatal(33, "String not started expecting ' or \"");
            (Vec::new(), true)
        }
    }

    /// `xmlParseEncName`: `[A-Za-z] ([A-Za-z0-9._] | '-')*`. Returns `None`
    /// where upstream returns NULL (the caller then raises nothing further and
    /// does NOT switch the encoding).
    fn decl_parse_enc_name(&mut self) -> Option<Vec<u8>> {
        let cur = self.decl_raw();
        if !(cur.is_ascii_lowercase() || cur.is_ascii_uppercase()) {
            // XML_ERR_ENCODING_NAME (79).
            self.decl_fatal(79, "Invalid XML encoding name");
            return None;
        }
        let mut buf = vec![cur];
        self.decl_advance();
        let mut c = self.decl_raw();
        while c.is_ascii_alphanumeric() || c == b'.' || c == b'_' || c == b'-' {
            buf.push(c);
            self.decl_advance();
            c = self.decl_raw();
        }
        Some(buf)
    }

    /// `xmlParseEncodingDecl` — ending with upstream's
    /// `xmlSetDeclaredEncoding`, i.e. the mid-declaration input ENCODING
    /// SWITCH. For a declared fixed-width codec the switch replaces the
    /// remaining input with the converted tail, so the following bytes are no
    /// longer the declaration's ASCII remainder; the scanner simply continues
    /// at the (unchanged) cursor over the new bytes, exactly as upstream's
    /// `xmlBufResetInput` leaves `cur` at the start of the converted buffer.
    fn decl_parse_encoding_decl(&mut self) -> Option<Vec<u8>> {
        self.skip_whitespace();
        if self.peek_bytes(8) != b"encoding" {
            return None;
        }
        self.decl_skip(8);
        self.skip_whitespace();
        if self.decl_raw() != b'=' {
            self.decl_fatal(75, "expected '='");
            return None;
        }
        self.decl_advance();
        self.skip_whitespace();
        let quote = self.decl_raw();
        if quote != b'"' && quote != b'\'' {
            self.decl_fatal(33, "String not started expecting ' or \"");
            return None;
        }
        self.decl_advance();
        let name = self.decl_parse_enc_name();
        if self.decl_raw() != quote {
            self.decl_fatal(34, "String not closed expecting \" or '");
            return None;
        }
        self.decl_advance();
        let name = name?;
        // `xmlSetDeclaredEncoding(ctxt, encoding)`: the input switch. Only a
        // declared FIXED-WIDTH codec changes the bytes still to be scanned
        // (the ASCII-compatible ones were already converted up front and left
        // the declaration unchanged, so this returns false for them).
        self.input
            .base_mut()
            .apply_declared_fixed_width_switch(&name);
        Some(name)
    }

    /// `xmlParseSDDecl`. Returns the scanned value literal, if any.
    fn decl_parse_sd_decl(&mut self) -> Option<Vec<u8>> {
        self.skip_whitespace();
        if self.peek_bytes(10) != b"standalone" {
            return None;
        }
        self.decl_skip(10);
        self.skip_whitespace();
        if self.decl_raw() != b'=' {
            self.decl_fatal(75, "expected '='");
            return None;
        }
        self.decl_advance();
        self.skip_whitespace();
        let quote = self.decl_raw();
        if quote != b'\'' && quote != b'"' {
            self.decl_fatal(33, "String not started expecting ' or \"");
            return None;
        }
        self.decl_advance();
        let mut value: Vec<u8> = Vec::new();
        if self.decl_raw() == b'n' && self.decl_nxt(1) == b'o' {
            value.extend_from_slice(b"no");
            self.decl_skip(2);
        } else if self.decl_raw() == b'y' && self.decl_nxt(1) == b'e' && self.decl_nxt(2) == b's' {
            value.extend_from_slice(b"yes");
            self.decl_skip(3);
        } else {
            // XML_ERR_STANDALONE_VALUE (80). Upstream leaves `standalone` at
            // -2, which is what an unrecognized literal stores here.
            self.decl_fatal(80, "standalone accepts only 'yes' or 'no'");
        }
        if self.decl_raw() != quote {
            self.decl_fatal(34, "String not closed expecting \" or '");
        } else {
            self.decl_advance();
        }
        Some(value)
    }

    /// `xmlParseXMLDecl`'s VersionInfo step INCLUDING its caller's
    /// diagnostics: VERSION_MISSING (96) for a NULL parse, then the
    /// unsupported-version classification.
    fn decl_finish_version(&mut self) -> Vec<u8> {
        let (v, version_is_null) = self.decl_parse_version_info();
        if version_is_null {
            // XML_ERR_VERSION_MISSING (96).
            self.decl_fatal(96, "Malformed declaration expecting version");
            return Vec::new();
        }
        if v != b"1.0" {
            // XML_PARSE_OLD10 makes any non-1.0 version fatal; otherwise
            // "1.x" is a warning and everything else is fatal.
            let is_1x = v.len() >= 2 && v[0] == b'1' && v[1] == b'.';
            let fatal = self.old10 || !is_1x;
            let code: c_int = if fatal { 108 } else { 97 };
            let level = if fatal {
                crate::abi::types::xmlErrorLevel::XML_ERR_FATAL as c_int
            } else {
                crate::abi::types::xmlErrorLevel::XML_ERR_WARNING as c_int
            };
            self.record_error(
                crate::abi::types::XML_FROM_PARSER,
                code,
                level,
                format!("Unsupported version '{}'\n", String::from_utf8_lossy(&v)),
                Some(v.clone()),
                None,
                None,
                0,
                None,
            );
        }
        v
    }

    /// Scan after `<?xml` — a faithful port of upstream `xmlParseXMLDecl`
    /// (parser.c 2.15), including the mid-declaration ENCODING SWITCH
    /// performed inside `xmlParseEncodingDecl`.
    ///
    /// The switch is the part that cannot be approximated. Upstream's
    /// `xmlSetDeclaredEncoding` shrinks the raw buffer by the already-consumed
    /// prefix and converts only what is left, so for a unit-aligned declared
    /// codec (UCS-2, declared UTF-16/UCS-4) the ASCII `?>` that closes the
    /// declaration is decoded as a code unit and the declaration itself becomes
    /// unparsable — exactly the `enc-ucs2*` oracle traces (65 "Blank needed
    /// here" + 57 "'?>' expected", wellFormed 0, no `startDocument`).
    fn scan_xml_decl_rest(&mut self) -> XmlToken {
        // The cursor is just past `<?xml`. `scan_pi_or_xml_decl` only routed
        // here when the byte after `xml` is a blank (upstream's
        // `IS_BLANK(NXT(5))`), so "Blank needed after '<?xml'" cannot fire
        // here; upstream then does `SKIP_BLANKS`.
        self.skip_whitespace();

        // [25] VersionInfo ::= S 'version' Eq ("'" VersionNum "'" | '"' VersionNum '"')
        let version = self.decl_finish_version();

        // A blank (or the end of the declaration) must follow the version.
        if !Self::decl_is_blank(self.decl_raw()) {
            if self.decl_raw() == b'?' && self.decl_nxt(1) == b'>' {
                self.decl_advance();
                self.decl_advance();
                return XmlToken::XmlDecl {
                    version,
                    encoding: None,
                    standalone: None,
                };
            }
            // XML_ERR_SPACE_REQUIRED (65).
            self.decl_fatal_msg(65, "Blank needed here\n");
        }

        // [80] EncodingDecl ::= S 'encoding' Eq ('"' EncName '"' | "'" EncName "'")
        let encoding = self.decl_parse_encoding_decl();

        // `ctxt->encoding != NULL` gates the second blank check: it is set
        // only when the encoding pseudo-attribute parsed, and it survives the
        // switch (upstream assigns it in `xmlSetDeclaredEncoding`).
        if encoding.is_some() && !Self::decl_is_blank(self.decl_raw()) {
            if self.decl_raw() == b'?' && self.decl_nxt(1) == b'>' {
                self.decl_advance();
                self.decl_advance();
                return XmlToken::XmlDecl {
                    version,
                    encoding,
                    standalone: None,
                };
            }
            self.decl_fatal_msg(65, "Blank needed here\n");
        }

        // [32] SDDecl? S? '?>'
        let standalone = self.decl_parse_sd_decl();
        self.skip_whitespace();

        if self.decl_raw() == b'?' && self.decl_nxt(1) == b'>' {
            self.decl_advance();
            self.decl_advance();
        } else if self.decl_raw() == b'>' {
            // Deprecated old working draft: a bare `>`.
            self.decl_fatal(57, "parsing XML declaration: '?>' expected");
            self.decl_advance();
        } else {
            self.decl_fatal(57, "parsing XML declaration: '?>' expected");
            // `while ((c = CUR) != 0) { NEXT; if (c == '>') break; }`
            loop {
                let c = self.decl_raw();
                if c == 0 {
                    break;
                }
                self.decl_advance();
                if c == b'>' {
                    break;
                }
            }
        }

        XmlToken::XmlDecl {
            version,
            encoding,
            standalone,
        }
    }

    /// Scan a markup declaration after `<!`.
    fn scan_markup_decl(&mut self, start_pos: usize) -> XmlToken {
        debug_assert_eq!(self.input.peek_char(), Some('!'));
        // Consume '!'
        self.input.read_char();

        if self.input.is_eof() {
            // `<!` cut off by the end of the available input: the construct
            // may continue on a later push call — return Eof so incremental
            // probes pause instead of treating the prefix as text
            // (SP-14.3.1-8, gh20439_1 per-char feed).
            return XmlToken::Eof;
        }

        // Peek ahead to determine the type
        let next = self.peek_bytes(10);

        // Comment: `<!--`
        if next.len() >= 2 && next[0] == b'-' && next[1] == b'-' {
            self.input.read_char(); // '-'
            self.input.read_char(); // '-'
            return self.scan_comment_body();
        }

        // CDATA: `<![CDATA[`
        if next.len() >= 7
            && next[0] == b'['
            && next[1] == b'C'
            && next[2] == b'D'
            && next[3] == b'A'
            && next[4] == b'T'
            && next[5] == b'A'
            && next[6] == b'['
        {
            for _ in 0..7 {
                self.input.read_char();
            }
            return self.scan_cdata_body(start_pos);
        }

        // DOCTYPE: `DOCTYPE`
        if next.len() >= 7 {
            let is_doctype = next[..7].eq_ignore_ascii_case(b"DOCTYPE");
            if is_doctype {
                for _ in 0..7 {
                    self.input.read_char();
                }
                return self.scan_doctype_body();
            }
        }

        // The available input ended while the markup-decl type was still
        // undecided (`<!-`, `<!D`, `<![C` ...): wait for more data on a later
        // push call instead of treating the prefix as markup text
        // (SP-14.3.1-8).
        if self.input.is_eof() {
            return XmlToken::Eof;
        }

        // Unknown markup declaration — consume until '>'
        let mut content = vec![b'!'];
        let mut closed = false;
        loop {
            match self.input.read_char() {
                Some('>') => {
                    closed = true;
                    break;
                }
                Some(c) => Self::push_char(&mut content, c),
                None => break,
            }
        }

        // An unknown markup declaration cut off at the end of the input: the
        // '>' may arrive on a later push call — pause rather than emitting
        // the partial text (SP-14.3.1-8).
        if !closed {
            return XmlToken::Eof;
        }

        XmlToken::Characters(XmlText::Owned(content))
    }

    /// Scan a comment body (after `<!--`).
    fn scan_comment_body(&mut self) -> XmlToken {
        // UPSTREAM-PARITY (parser.c XML_PARSER_CONTENT/MISC states): a
        // comment is only scanned once its `-->` terminator is available in
        // the current buffer (xmlParseLookupString gate). Without it a
        // non-final push call defers the whole construct — scanning to the
        // buffer end could otherwise misfire a "Double hyphen within comment"
        // (79) for a trailing `--` that the next chunk completes.
        if self.silent_truncated
            && !self
                .input
                .current_ref()
                .remaining()
                .windows(3)
                .any(|w| w == b"-->")
        {
            return XmlToken::Comment {
                data: XmlText::Owned(Vec::new()),
                unterminated: true,
            };
        }
        // §16.5.3: a comment body INSIDE a pushed (entity-content) input
        // uses the legacy per-char owned path — the body may run past the
        // entity boundary (auto-pop) into the outer input, so no source
        // segment offsets can describe it. Only base-input bodies (never
        // popped mid-scan) can use the span/pending model.
        if !self.input.at_base_input() {
            let mut content = Vec::new();
            let mut unterminated = false;
            loop {
                if self.input.is_eof() {
                    unterminated = true;
                    break;
                }
                // Check for `-->`
                if self.input.peek_char() == Some('-') {
                    let err_pos = self.input.current_pos().2;
                    self.input.read_char();
                    if self.input.peek_char() == Some('-') {
                        self.input.read_char();
                        if self.input.peek_char() == Some('>') {
                            self.input.read_char();
                            break;
                        }
                        let mut preview: Vec<u8> = content.clone();
                        preview.truncate(50);
                        let msg = format!(
                            "Double hyphen within comment: <!--{}\n",
                            String::from_utf8_lossy(&preview)
                        );
                        self.record_error_at(
                            crate::abi::types::XML_FROM_PARSER,
                            crate::abi::types::XML_ERR_HYPHEN_IN_COMMENT,
                            crate::abi::types::xmlErrorLevel::XML_ERR_FATAL as c_int,
                            msg,
                            None,
                            None,
                            None,
                            0,
                            err_pos,
                            None,
                        );
                        continue;
                    }
                    content.push(b'-');
                    continue;
                }
                match self.input.read_char() {
                    Some(c) => Self::push_char(&mut content, c),
                    None => break,
                }
            }
            if unterminated && !self.silent_truncated {
                self.record_error(
                    crate::abi::types::XML_FROM_PARSER,
                    crate::abi::types::XML_ERR_COMMENT_NOT_FINISHED,
                    crate::abi::types::xmlErrorLevel::XML_ERR_FATAL as c_int,
                    "Comment not terminated\n".to_string(),
                    None,
                    None,
                    None,
                    0,
                    None,
                );
            }
            return XmlToken::Comment {
                data: XmlText::Owned(content),
                unterminated,
            };
        }
        // §16.5.3 owned-or-span state (see scan_characters): comments at the
        // base input whose bytes all reach the content verbatim are spans;
        // the double-hyphen WFC error drops two hyphens, so it materializes.
        let at_base = true;
        let mut owned: Option<Vec<u8>> = None;
        let mut seg_start = self.input.current_pos().2;
        // Byte offset just past the last CONTENT byte: the `-->` terminator
        // is consumed before the break, so the span must end at the first
        // '-' of the terminator, not at the post-consumption position.
        let mut content_end = seg_start;
        let mut unterminated = false;

        loop {
            if self.input.is_eof() {
                unterminated = true;
                content_end = self.input.current_pos().2;
                break;
            }
            // §2.11/encoding-error patches shared with text scanning.
            if let Some(b) = self.input.peek_raw() {
                if b >= 0x80 && self.input.peek_char().is_none() {
                    self.record_encoding_error();
                    self.flush_clean_segment(&mut owned, seg_start, self.input.current_pos().2);
                    self.input.skip_raw_bytes(1);
                    if let Some(v) = owned.as_mut() {
                        v.extend_from_slice(b"\xEF\xBF\xBD");
                    }
                    seg_start = self.input.current_pos().2;
                    continue;
                }
            }
            let c = match self.input.peek_char() {
                Some(c) => c,
                None => break,
            };
            if c == '\n' && self.input.peek_raw() == Some(b'\r') {
                // §2.11: literal CR is delivered as LF.
                self.flush_clean_segment(&mut owned, seg_start, self.input.current_pos().2);
                self.input.read_char();
                if let Some(v) = owned.as_mut() {
                    v.push(b'\n');
                }
                seg_start = self.input.current_pos().2;
                continue;
            }

            // Check for `-->`
            if c == '-' {
                let err_pos = self.input.current_pos().2;
                self.input.read_char();
                if self.input.peek_char() == Some('-') {
                    self.input.read_char();
                    if self.input.peek_char() == Some('>') {
                        self.input.read_char();
                        content_end = err_pos;
                        break;
                    }
                    // UPSTREAM-PARITY (parser.c xmlParseCommentComplex): a
                    // double hyphen inside a comment (not `-->`) is a fatal
                    // WFC error "Double hyphen within comment: <!--%.50s\n"
                    // (XML_ERR_HYPHEN_IN_COMMENT); parsing continues past
                    // the two hyphens, which are NOT part of the content.
                    // R-000166. The dropped hyphens are a patch: materialize
                    // the pending segment first (the error preview reads it).
                    self.flush_clean_segment(&mut owned, seg_start, err_pos);
                    let preview: Vec<u8> = match &owned {
                        Some(v) => v.iter().copied().take(50).collect(),
                        None => Vec::new(),
                    };
                    let msg = format!(
                        "Double hyphen within comment: <!--{}\n",
                        String::from_utf8_lossy(&preview)
                    );
                    self.record_error_at(
                        crate::abi::types::XML_FROM_PARSER,
                        crate::abi::types::XML_ERR_HYPHEN_IN_COMMENT,
                        crate::abi::types::xmlErrorLevel::XML_ERR_FATAL as c_int,
                        msg,
                        // upstream xmlFatalErrMsgStr(..., "Double hyphen within
                        // comment: <!--%.50s\n", buf) — the preview is `str1`.
                        Some(preview),
                        None,
                        None,
                        0,
                        err_pos,
                        None,
                    );
                    seg_start = self.input.current_pos().2;
                    continue;
                }
                // Single '-' is ordinary content: it stays in the pending
                // segment (bulk-flushed at the next patch / finish).
                continue;
            }

            match self.input.read_char() {
                Some(_c) => {
                    // §16.5.3: clean bytes stay pending in `[seg_start, pos)`
                    // and are bulk-flushed at the next patch / finish.
                }
                None => break,
            }
        }

        if unterminated {
            // upstream xmlParseComment: EOF → "Comment not terminated\n"
            // (XML_ERR_COMMENT_NOT_FINISHED). EOF-truncated by
            // construction: incremental probes/partial deliveries must not
            // raise it (the comment may complete on a later push call).
            if !self.silent_truncated {
                self.record_error(
                    crate::abi::types::XML_FROM_PARSER,
                    crate::abi::types::XML_ERR_COMMENT_NOT_FINISHED,
                    crate::abi::types::xmlErrorLevel::XML_ERR_FATAL as c_int,
                    "Comment not terminated\n".to_string(),
                    None,
                    None,
                    None,
                    0,
                    None,
                );
            }
        }

        XmlToken::Comment {
            data: self.finish_text_run(&mut owned, seg_start, content_end, at_base),
            unterminated,
        }
    }

    /// Scan a CDATA section body (after `<![CDATA[`).
    fn scan_cdata_body(&mut self, start_pos: usize) -> XmlToken {
        // UPSTREAM-PARITY (parser.c XML_PARSER_CONTENT state): a CDATA
        // section is only scanned once its `]]>` terminator is available in
        // the current buffer (xmlParseLookupString gate); without it a
        // non-final push call defers the whole construct.
        if self.silent_truncated
            && !self
                .input
                .current_ref()
                .remaining()
                .windows(3)
                .any(|w| w == b"]]>")
        {
            return XmlToken::Cdata {
                data: XmlText::Owned(Vec::new()),
                unterminated: true,
                start_pos,
            };
        }
        // §16.5.3: a CDATA body inside a pushed (entity-content) input uses
        // the legacy per-char owned path (the body may run past the entity
        // boundary via auto-pop). Only base-input bodies can span.
        if !self.input.at_base_input() {
            let mut content = Vec::new();
            let mut unterminated = false;
            loop {
                if self.input.is_eof() {
                    unterminated = true;
                    break;
                }
                // Check for `]]>`
                if self.input.peek_char() == Some(']') {
                    self.input.read_char();
                    if self.input.peek_char() == Some(']') {
                        self.input.read_char();
                        if self.input.peek_char() == Some('>') {
                            self.input.read_char();
                            break;
                        }
                        content.push(b']');
                        content.push(b']');
                        continue;
                    }
                    content.push(b']');
                    continue;
                }
                match self.input.read_char() {
                    Some(c) => Self::push_char(&mut content, c),
                    None => break,
                }
            }
            if unterminated && !self.silent_truncated {
                self.record_cdata_not_finished(&content);
            }
            return XmlToken::Cdata {
                data: XmlText::Owned(content),
                unterminated,
                start_pos,
            };
        }
        // §16.5.3 owned-or-span state (see scan_characters): CDATA content at
        // the base input is a span when no byte was patched; `]` sequences
        // are delivered verbatim (only the `]]>` terminator ends the run).
        let at_base = true;
        let mut owned: Option<Vec<u8>> = None;
        let mut seg_start = self.input.current_pos().2;
        // Byte offset just past the last CONTENT byte: the `]]>` terminator
        // is consumed before the break, so the span must end at the first
        // ']' of the terminator.
        let mut content_end = seg_start;
        let mut unterminated = false;

        loop {
            if self.input.is_eof() {
                unterminated = true;
                content_end = self.input.current_pos().2;
                break;
            }
            // §2.11/encoding-error patches shared with text scanning.
            if let Some(b) = self.input.peek_raw() {
                if b >= 0x80 && self.input.peek_char().is_none() {
                    self.record_encoding_error();
                    self.flush_clean_segment(&mut owned, seg_start, self.input.current_pos().2);
                    self.input.skip_raw_bytes(1);
                    if let Some(v) = owned.as_mut() {
                        v.extend_from_slice(b"\xEF\xBF\xBD");
                    }
                    seg_start = self.input.current_pos().2;
                    continue;
                }
            }
            let c = match self.input.peek_char() {
                Some(c) => c,
                None => break,
            };
            if c == '\n' && self.input.peek_raw() == Some(b'\r') {
                // §2.11: literal CR is delivered as LF.
                self.flush_clean_segment(&mut owned, seg_start, self.input.current_pos().2);
                self.input.read_char();
                if let Some(v) = owned.as_mut() {
                    v.push(b'\n');
                }
                seg_start = self.input.current_pos().2;
                continue;
            }

            // Check for `]]>`
            if c == ']' {
                let term_start = self.input.current_pos().2;
                self.input.read_char();
                if self.input.peek_char() == Some(']') {
                    self.input.read_char();
                    if self.input.peek_char() == Some('>') {
                        self.input.read_char();
                        content_end = term_start;
                        break;
                    }
                    // `]]` not followed by `>`: both are ordinary content
                    // and stay in the pending segment (bulk-flushed later).
                    continue;
                }
                // Single ']' is ordinary content (pending segment).
                continue;
            }

            match self.input.read_char() {
                Some(_c) => {
                    // §16.5.3: clean bytes stay pending in `[seg_start, pos)`
                    // and are bulk-flushed at the next patch / finish.
                }
                None => break,
            }
        }

        let data = self.finish_text_run(&mut owned, seg_start, content_end, at_base);
        if unterminated && !self.silent_truncated {
            // upstream xmlParseCDSect: EOF -> the XML_ERR_CDATA_NOT_FINISHED
            // diagnostic. The parser raises it only when the CDATA is in
            // element content; at document level it reports the invalid
            // element name instead. EOF-truncated by construction: incremental
            // probes/partial deliveries must not raise it.
            let bytes: Vec<u8> = match &data {
                XmlText::Owned(v) => v.clone(),
                XmlText::Span { start, end } => {
                    self.input.base_ref().raw_range(*start, *end).to_vec()
                }
            };
            self.record_cdata_not_finished(&bytes);
        }

        XmlToken::Cdata {
            data,
            unterminated,
            start_pos,
        }
    }

    /// Upstream `xmlParseCDSect`'s EOF diagnostic for an unterminated CDATA
    /// section.
    ///
    /// The scanner reads `r`, `s` and `cur` one character apart, so when the
    /// input runs out the failure comes from one of two places:
    ///   * fewer than two content characters — the initial `r`/`s` reads
    ///     already failed and the table message for XML_ERR_CDATA_NOT_FINISHED
    ///     applies (upstream `xmlFatalErr(..., NULL)`); that table entry does
    ///     not exist, so it renders as the generic "Unregistered error
    ///     message";
    ///   * two or more — `xmlFatalErrMsgStr(ctxt, ..., "CData section not
    ///     finished\n%.50s\n", buf)` where `buf` is the accumulated content
    ///     with the LAST TWO characters still in flight (they were never
    ///     copied) and `%.50s` truncates to 50 BYTES in both the message and
    ///     `str1`.
    fn record_cdata_not_finished(&mut self, content: &[u8]) {
        let nchars = content.iter().filter(|&&b| (b & 0xC0) != 0x80).count();
        if nchars < 2 {
            self.record_error(
                crate::abi::types::XML_FROM_PARSER,
                crate::abi::types::XML_ERR_CDATA_NOT_FINISHED,
                crate::abi::types::xmlErrorLevel::XML_ERR_FATAL as c_int,
                "Unregistered error message\n".to_string(),
                None,
                None,
                None,
                0,
                None,
            );
            return;
        }
        let mut cut = content.len();
        for _ in 0..2 {
            if cut == 0 {
                break;
            }
            cut -= 1;
            while cut > 0 && (content[cut] & 0xC0) == 0x80 {
                cut -= 1;
            }
        }
        let preview: Vec<u8> = content[..cut].iter().copied().take(50).collect();
        self.record_error(
            crate::abi::types::XML_FROM_PARSER,
            crate::abi::types::XML_ERR_CDATA_NOT_FINISHED,
            crate::abi::types::xmlErrorLevel::XML_ERR_FATAL as c_int,
            format!(
                "CData section not finished\n{}\n",
                String::from_utf8_lossy(&preview)
            ),
            Some(preview),
            None,
            None,
            0,
            None,
        );
    }

    /// Scan a DOCTYPE body (after `<!DOCTYPE`).
    fn scan_doctype_body(&mut self) -> XmlToken {
        // §16.5.3 byte model: the DOCTYPE body is TRANSPORT — parse_dtd
        // re-scans these raw bytes to populate the DTD's declaration tables
        // and entity values. Unlike element content it is NOT subject to the
        // xmlCurrentChar EOL substitution: upstream scans the internal
        // subset with raw-byte macros and keeps literal CRLF bytes in
        // entity/notation values (XML spec 2.11 only mandates EOL
        // normalization for *parsed* content — the document/entity re-parse
        // applies it later, when an entity is expanded). The capture must
        // therefore be a raw source range, not decoded re-encoded chars
        // (per-char decode dropped the LF of every CRLF pair and substituted
        // nothing for standalone CR). Because every consumed byte between
        // the opening and the depth-0 '>' is part of the body (that '>' is
        // the only consumed byte not included), the content is exactly the
        // source slice [start, '>'-position).
        let content_start = self.input.current_pos().2;
        let mut depth: usize = 0;
        let mut closed = false;

        loop {
            if self.input.is_eof() {
                break;
            }

            match self.input.peek_char() {
                Some('>') if depth == 0 => {
                    self.input.read_char();
                    closed = true;
                    break;
                }
                Some('[') => {
                    depth += 1;
                    self.input.read_char();
                }
                Some(']') => {
                    depth = depth.saturating_sub(1);
                    self.input.read_char();
                }
                Some(_) => {
                    self.input.read_char();
                }
                None => break,
            }
        }

        let content = self
            .input
            .current_ref()
            .raw_range(content_start, self.input.current_pos().2)
            .to_vec();

        XmlToken::DocType {
            content,
            unterminated: !closed,
        }
    }

    /// Scan the HEAD of a `<!DOCTYPE` declaration, mirroring upstream
    /// `xmlParseDocTypeDecl`: consume `<!DOCTYPE`, then the bytes up to (but not
    /// including) a depth-0 `[`, or through the depth-0 `>` when there is no
    /// internal subset. Quoted literals are skipped so a `[`/`>` inside a
    /// SYSTEM/PUBLIC id does not terminate the head.
    pub fn scan_doctype_decl(&mut self) -> XmlToken {
        // Consume "<!DOCTYPE" (9 bytes).
        for _ in 0..9 {
            if self.input.is_eof() {
                break;
            }
            self.input.read_char();
        }
        let content_start = self.input.current_pos().2;
        let mut quote: u8 = 0;
        let mut has_subset = false;
        let mut closed = false;
        loop {
            if self.input.is_eof() {
                break;
            }
            let c = self.input.peek_char().unwrap_or('\0') as u8;
            if quote == 0 && c == b'>' {
                // Leave the cursor AT the `>`: upstream fires `internalSubset`
                // with `RAW == '>'` still unconsumed (the MISC arm does
                // `if (RAW == '>') NEXT;` afterwards).
                closed = true;
                break;
            }
            if quote == 0 && c == b'[' {
                // Leave the cursor AT the `[`: upstream sets XML_PARSER_DTD and
                // xmlParseInternalSubset consumes it.
                has_subset = true;
                break;
            }
            if c == b'"' || c == b'\'' {
                if quote == 0 {
                    quote = c;
                } else if quote == c {
                    quote = 0;
                }
            }
            self.input.read_char();
        }
        if !has_subset && !closed {
            // upstream: `if ((RAW != '[') && (RAW != '>')) xmlFatalErr(
            // ctxt, XML_ERR_DOCTYPE_NOT_FINISHED, NULL);`
            self.record_error(
                crate::abi::types::XML_FROM_PARSER,
                crate::abi::types::XML_ERR_DOCTYPE_NOT_FINISHED,
                crate::abi::types::xmlErrorLevel::XML_ERR_FATAL as c_int,
                "DOCTYPE improperly terminated\n".to_string(),
                None,
                None,
                None,
                0,
                None,
            );
        }
        let content = self
            .input
            .current_ref()
            .raw_range(content_start, self.input.current_pos().2)
            .to_vec();
        XmlToken::DocTypeDecl {
            content,
            has_subset,
            closed,
        }
    }

    /// Scan the internal subset with the cursor AT its `[`, consuming through
    /// the matching depth-0 `]` and the `>` that must follow. `content` is the
    /// raw slice `[`..`]` inclusive, which is exactly what
    /// `parse_internal_subset` re-scans.
    pub fn scan_doctype_subset(&mut self) -> XmlToken {
        debug_assert_eq!(self.input.peek_char(), Some('['));
        let content_start = self.input.current_pos().2;
        let mut depth: usize = 0;
        let mut closed = false;
        let mut saw_close_bracket = false;
        loop {
            if self.input.is_eof() {
                break;
            }
            match self.input.peek_char() {
                Some('[') => {
                    depth += 1;
                    self.input.read_char();
                }
                Some(']') => {
                    depth = depth.saturating_sub(1);
                    self.input.read_char();
                    if depth == 0 {
                        saw_close_bracket = true;
                        if self.input.peek_char() == Some('>') {
                            self.input.read_char();
                            closed = true;
                        }
                        break;
                    }
                }
                Some(_) => {
                    self.input.read_char();
                }
                None => break,
            }
        }
        if !closed && saw_close_bracket {
            // `]` reached, `>` missing. Upstream reports this from the TAIL of
            // `xmlParseInternalSubset` — i.e. AFTER the declaration it just
            // parsed (which therefore still dispatches its `elementDecl`), as
            // `doctype-trunc-seq` shows. The driver leaves the diagnostic to
            // `parse_internal_subset` so the ordering survives.
        }
        let content = self
            .input
            .current_ref()
            .raw_range(content_start, self.input.current_pos().2)
            .to_vec();
        // UPSTREAM-PARITY (entities.c xmlParseEntityValue): an entity value's
        // opening quote is consumed with a raw `CUR_PTR++`, which advances
        // `cur` WITHOUT bumping `col`. The subset scan above is uniform, so
        // the lag is applied here — for the quotes on the cursor's OWN line
        // only (a line break resets the column).
        let raw_quotes = dtd_col_lag_at(&content, content.len());
        if raw_quotes > 0 {
            self.input.current().adjust_col_back(raw_quotes);
        }
        XmlToken::DocTypeSubset { content, closed }
    }

    // ── Reference scanning ──────────────────────────────────────────────────

    /// Scan a reference starting with `&`, replicating upstream
    /// `xmlParseCharRef` + `xmlParseEntityRefInternal` error semantics
    /// (11.1-M): every error is recorded at the exact detection position
    /// with the upstream code/level/message.
    fn scan_reference(&mut self) -> XmlToken {
        debug_assert_eq!(self.input.peek_char(), Some('&'));
        // Consume '&'
        self.input.read_char();

        let mut content = vec![b'&'];

        // ── Character reference: &#...; / &#x...; ────────────────────────
        if self.input.peek_char() == Some('#') {
            content.push(b'#');
            self.input.read_char();
            // UPSTREAM-PARITY (parser.c xmlParseCharRef): the HEX form is
            // recognised only for a lower-case `x` (`RAW == '&' && NXT(1) ==
            // '#' && NXT(2) == 'x'`). An upper-case `X` is NOT a hex marker, so
            // `&#X43;` takes the DECIMAL path, fails on the first non-digit with
            // "CharRef: invalid decimal value" and then reports
            // "xmlParseCharRef: invalid xmlChar value 0" — with the cursor left
            // ON the `X` (SKIP(2) consumed only `&#`). `text-amp` pins both
            // diagnostics and the position.
            let hex = self.input.peek_char() == Some('x');
            if hex {
                if let Some(c) = self.input.peek_char() {
                    content.push(c as u8);
                    self.input.read_char();
                }
            }

            // Upstream value clamp: 0x110000.
            let mut val: u32 = 0;
            let mut over = false;
            loop {
                match self.input.peek_char() {
                    Some(';') => {
                        content.push(b';');
                        self.input.read_char();
                        break;
                    }
                    Some(c) if (hex && c.is_ascii_hexdigit()) || (!hex && c.is_ascii_digit()) => {
                        let d = c.to_digit(if hex { 16 } else { 10 }).unwrap();
                        if !over {
                            val = val * (if hex { 16 } else { 10 }) + d;
                            if val > 0x110000 {
                                val = 0x110000;
                                over = true;
                            }
                        }
                        content.push(c as u8);
                        self.input.read_char();
                    }
                    _ => {
                        // Invalid digit or EOF: upstream raises the
                        // hex/decimal-value error at the current position.
                        let (code, msg) = if hex {
                            (
                                crate::abi::types::XML_ERR_INVALID_HEX_CHARREF,
                                "CharRef: invalid hexadecimal value\n".to_string(),
                            )
                        } else {
                            (
                                crate::abi::types::XML_ERR_INVALID_DEC_CHARREF,
                                "CharRef: invalid decimal value\n".to_string(),
                            )
                        };
                        self.record_error(
                            crate::abi::types::XML_FROM_PARSER,
                            code,
                            crate::abi::types::xmlErrorLevel::XML_ERR_FATAL as c_int,
                            msg,
                            None,
                            None,
                            None,
                            0,
                            None,
                        );
                        val = 0;
                        break;
                    }
                }
            }

            // Upstream post-scan validation: out-of-bounds / invalid Char
            // (raised after the ';' — i.e., at the current position). Also
            // runs for the invalid-digit case (val clamps to 0 → "invalid
            // xmlChar value 0"), matching xmlParseCharRef.
            if val >= 0x110000 {
                self.record_error(
                    crate::abi::types::XML_FROM_PARSER,
                    crate::abi::types::XML_ERR_INVALID_CHAR,
                    crate::abi::types::xmlErrorLevel::XML_ERR_FATAL as c_int,
                    "xmlParseCharRef: character reference out of bounds\n".to_string(),
                    None,
                    None,
                    None,
                    val as c_int,
                    None,
                );
            } else if !is_valid_char_ref(val) {
                self.record_error(
                    crate::abi::types::XML_FROM_PARSER,
                    crate::abi::types::XML_ERR_INVALID_CHAR,
                    crate::abi::types::xmlErrorLevel::XML_ERR_FATAL as c_int,
                    format!("xmlParseCharRef: invalid xmlChar value {}\n", val),
                    None,
                    None,
                    None,
                    val as c_int,
                    None,
                );
            }
            return XmlToken::Reference(content);
        }

        // ── Entity reference: &name; ─────────────────────────────────────
        // UPSTREAM-PARITY (parser.c xmlParseEntityRef → xmlParseName): the
        // name must START with a NameStartChar; `&6…`, `&.…` yield an empty
        // name → "xmlParseEntityRef: no name\n" (XML_ERR_NAME_REQUIRED) and
        // the offending byte is NOT consumed (the caret sits on it).
        // is_name_byte is the continuation set (digits/'.'/'-' allowed only
        // after the start char).
        let mut name = Vec::new();
        loop {
            let ok = match self.input.peek_char() {
                Some(c) => {
                    let b = c as u8;
                    if name.is_empty() {
                        // First char: NameStartChar only.
                        b.is_ascii_alphabetic() || b == b'_' || b == b':' || b >= 0x80
                    } else {
                        is_name_byte(b)
                    }
                }
                None => false,
            };
            if !ok {
                break;
            }
            let c = self.input.peek_char().unwrap();
            content.push(c as u8);
            name.push(c as u8);
            self.input.read_char();
        }

        if name.is_empty() {
            // upstream xmlParseEntityRefInternal: name == NULL →
            // "xmlParseEntityRef: no name\n" (XML_ERR_NAME_REQUIRED).
            self.record_error(
                crate::abi::types::XML_FROM_PARSER,
                crate::abi::types::XML_ERR_NAME_REQUIRED,
                crate::abi::types::xmlErrorLevel::XML_ERR_FATAL as c_int,
                "xmlParseEntityRef: no name\n".to_string(),
                None,
                None,
                None,
                0,
                None,
            );
            return XmlToken::Reference(content);
        }

        if self.input.peek_char() == Some(';') {
            content.push(b';');
            self.input.read_char();
        } else {
            // upstream: RAW != ';' → "EntityRef: expecting ';'\n"
            // (XML_ERR_ENTITYREF_SEMICOL_MISSING).
            self.record_error(
                crate::abi::types::XML_FROM_PARSER,
                crate::abi::types::XML_ERR_ENTITYREF_SEMICOL_MISSING,
                crate::abi::types::xmlErrorLevel::XML_ERR_FATAL as c_int,
                "EntityRef: expecting ';'\n".to_string(),
                None,
                None,
                None,
                0,
                None,
            );
        }

        XmlToken::Reference(content)
    }

    // ── Character data scanning ─────────────────────────────────────────────

    /// Scan character data until '<' or '&' is encountered, replicating
    /// upstream `xmlParseCharDataComplex` error semantics (11.1-M):
    /// "Sequence ']]>' not allowed in content\n" (62) at the first ']',
    /// "PCDATA invalid Char value %d\n" (9, int1 = value) for invalid
    /// characters (the offending char is skipped), and the I/O encoding
    /// error (81) for invalid UTF-8 bytes (the byte is skipped).
    ///
    /// §16.5.3: the run is returned as an [`XmlText::Span`] of the base
    /// input when it needed no patching (source bytes == delivered bytes);
    /// any patch (invalid char, encoding-error replacement, or a source CR
    /// whose §2.11 EOL substitution turns it into an LF) materializes the
    /// bytes collected so far and switches to an owned buffer. Entity-
    /// content runs (depth > 0) never span: their buffer may be auto-popped
    /// and dropped at the run end, so they materialize owned bytes.
    fn scan_characters(&mut self) -> XmlToken {
        // UPSTREAM-PARITY (SAX2 entity boundary): when character data comes out
        // of a substituted general entity (an input pushed by xmlParseReference)
        // the parser emits a discrete SAX `characters` run for the entity
        // content and a SEPARATE run for any following outer literal text — the
        // oracle never merges text across an entity-content input boundary
        // (e.g. NOENT `ab&e;cd`, e="ENT", yields CD[ab] CD[ENT] CD[cd], not
        // CD[ab] CD[ENTcd]). The input stack auto-pops an exhausted pushed
        // input, so a monotonic text scan would otherwise swallow the outer
        // tail into the same token; break the run when the scan crosses back
        // below its starting depth.
        let start_depth = self.input.depth();
        let at_base = self.input.at_base_input();
        // SP-14.3.1-6: whether any byte of the CURRENT run was consumed
        // strictly below the split offset. Only a run that begins in the
        // already-delivered prefix and crosses the boundary is split there; a
        // run starting at or after the boundary is fresh and must be delivered
        // whole (splitting it would emit one character per token and turn
        // text merges into O(n²) appends).
        let mut below_split = false;
        // §16.5.3 owned-or-span state: `owned` is None while the delivered
        // bytes equal the source bytes since `seg_start` (a byte offset into
        // the buffer the run is scanned from). Invariant: bytes in
        // `[seg_start, pos)` are pending — never yet delivered. Delivery
        // happens ONLY as bulk segment flushes (at a patch, before an entity
        // input is popped, and at finish); patched bytes (EOL `\n`, U+FFFD)
        // are appended explicitly after flushing, with `seg_start` advanced
        // past them. A run with no patch at all is a span (base) or one bulk
        // copy (entity).
        let mut owned: Option<Vec<u8>> = None;
        let mut seg_start = self.input.current_pos().2;
        // Set when the run crossed an entity-content boundary (the pre-pop
        // flush below completed the owned buffer at the boundary).
        let mut crossed_pop = false;

        loop {
            // An exhausted pushed (entity-content) input is about to be
            // auto-popped by is_eof() below; its bytes are only reachable
            // before the pop, so flush the pending segment first (base
            // inputs are never popped).
            if !at_base && self.input.current_ref().is_eof() {
                let end = self.input.current_pos().2;
                self.flush_clean_segment(&mut owned, seg_start, end);
                seg_start = end;
                crossed_pop = true;
            }
            if self.input.is_eof() {
                break;
            }
            let pos_before = self.input.current_pos().2;
            // SP-14.3.1-6 delivery boundary: a character run crossing the
            // already-delivered prefix of the accumulated input is split at
            // the boundary so the re-parse's event segmentation matches the
            // earlier eager-partial parse exactly (the prefix part was
            // delivered; the suffix part must fire as its own event).
            if let Some(split) = self.split_chars_at {
                if below_split && pos_before >= split {
                    break;
                }
            }
            // An exhausted entity-content input was auto-popped: end the run at
            // the entity boundary (the following char belongs to the outer
            // scope and must start a fresh token).
            if self.input.depth() < start_depth {
                break;
            }
            // §16.5.6/§16.7 contiguous text scanning: most text bytes are
            // printable ASCII that needs no per-character decision — never
            // '<'/'&' (run breaks), never ']' (the `]]>` lookahead), never
            // CR/LF (no §2.11 EOL substitution or line/col change), and
            // always a valid XML Char that decodes to itself. The run is
            // found by the runtime-dispatched structural scanner
            // (scalar/AVX2/AVX-512BW — §16.7) and extends the pending
            // segment in one step. The re-parse split boundary
            // (split_chars_at) is the only state that may break a run
            // mid-bytes, so the fast path yields to the per-char loop when
            // it is active.
            if self.split_chars_at.is_none() {
                let remaining = self.input.current_ref().remaining();
                let run = crate::xml::parser::scan::text_run_len_auto(remaining);
                if run > 0 {
                    self.input.skip_linebreak_free(run);
                    continue;
                }
            }
            // Invalid UTF-8 byte. UPSTREAM-PARITY (parserInternals.c 2.15
            // xmlCurrentChar + parser.c xmlParseCharDataComplex): two
            // distinct failure modes —
            //
            //  incomplete_sequence: fewer bytes remain in the current input
            //  than the character needs (avail < 2 for any byte >= 0x80,
            //  < 3 for a 3-byte lead, < 4 for a 4-byte lead). xmlCurrentChar
            //  returns 0 without raising; xmlParseCharDataComplex then
            //  raises a FATAL PARSER-domain XML_ERR_INVALID_CHAR
            //  "Incomplete UTF-8 sequence starting with %02X\n" per
            //  offending byte, each consumed alone (NEXTL(1) — a truncated
            //  3-byte sequence at EOF yields one error per remaining byte),
            //  and the byte is NOT added to the content (no U+FFFD).
            //  With `partial` (incremental) parsing no error fires at all.
            //
            //  encoding_error: enough bytes are present but they are not
            //  valid UTF-8 (bad continuation, overlong, surrogate, out of
            //  range). The I/O-domain XML_ERR_INVALID_ENCODING (81) is
            //  raised once per input; the byte is consumed and the content
            //  carries a U+FFFD replacement (xmlCurrentChar returns
            //  XML_INVALID_CHAR, xmlParseCharDataComplex skips it).
            if let Some(b) = self.input.peek_raw() {
                if b >= 0x80 && self.input.peek_char().is_none() {
                    let rem = self.input.current_ref().remaining();
                    let incomplete = rem.len() < 2
                        || (b >= 0xE0 && rem.len() < 3)
                        || (b >= 0xF0 && rem.len() < 4);
                    if incomplete && !self.silent_truncated {
                        below_split |= pos_before < self.split_chars_at.unwrap_or(usize::MAX);
                        self.flush_clean_segment(&mut owned, seg_start, pos_before);
                        self.record_error(
                            crate::abi::types::XML_FROM_PARSER,
                            crate::abi::types::XML_ERR_INVALID_CHAR,
                            crate::abi::types::xmlErrorLevel::XML_ERR_FATAL as c_int,
                            format!("Incomplete UTF-8 sequence starting with {:02X}\n", b),
                            None,
                            None,
                            None,
                            b as c_int,
                            None,
                        );
                        self.input.skip_raw_bytes(1);
                        seg_start = self.input.current_pos().2;
                        continue;
                    }
                    self.record_encoding_error();
                    below_split |= pos_before < self.split_chars_at.unwrap_or(usize::MAX);
                    // UPSTREAM-PARITY (parserInternals.c xmlCurrentChar
                    // encoding_error): the byte is consumed and replaced
                    // with XML_INVALID_CHAR (U+FFFD) in the character data
                    // — not dropped (the tree carries the replacement). The
                    // replacement differs from the source, so the pending
                    // segment materializes first.
                    self.flush_clean_segment(&mut owned, seg_start, pos_before);
                    self.input.skip_raw_bytes(1);
                    if let Some(v) = owned.as_mut() {
                        v.extend_from_slice(b"\xEF\xBF\xBD");
                    }
                    seg_start = self.input.current_pos().2;
                    continue;
                }
            }

            match self.input.peek_char() {
                Some('<') | Some('&') => break,
                Some(c) => {
                    let cp = c as u32;
                    // upstream xmlParseCharDataComplex: PCDATA invalid Char.
                    if !is_valid_char_ref(cp) {
                        // UPSTREAM-PARITY (parserInternals.c 2.15
                        // xmlCurrentChar, c == 0 branch): a literal NUL
                        // mid-buffer first raises its own error — "Char 0x0
                        // out of allowed range\n" — and returns len 1; the
                        // char-data loop then exits (IS_CHAR(0) false) and
                        // the post-loop adds "PCDATA invalid Char value 0"
                        // (both at the NUL's position).
                        if cp == 0 {
                            // UPSTREAM-PARITY (parserInternals.c 2.15
                            // xmlFatalErr): the composed message is
                            // xmlErrString(XML_ERR_INVALID_CHAR) (
                            // "Invalid character") + ": " + the info
                            // string ("Char 0x0 out of allowed range\n",
                            // with its own trailing newline) — hence the
                            // doubled newline before the source window.
                            self.record_error(
                                crate::abi::types::XML_FROM_PARSER,
                                crate::abi::types::XML_ERR_INVALID_CHAR,
                                crate::abi::types::xmlErrorLevel::XML_ERR_FATAL as c_int,
                                "Invalid character: Char 0x0 out of allowed range\n\n".to_string(),
                                None,
                                None,
                                None,
                                0,
                                None,
                            );
                        }
                        self.record_error(
                            crate::abi::types::XML_FROM_PARSER,
                            crate::abi::types::XML_ERR_INVALID_CHAR,
                            crate::abi::types::xmlErrorLevel::XML_ERR_FATAL as c_int,
                            format!("PCDATA invalid Char value {}\n", cp),
                            None,
                            None,
                            None,
                            cp as c_int,
                            None,
                        );
                        // Skip the offending character (upstream NEXTL after
                        // the error) — it is dropped from the content, so the
                        // pending clean segment materializes. (Already peeked
                        // above — §16.5.5 consume without a second decode.)
                        below_split |= pos_before < self.split_chars_at.unwrap_or(usize::MAX);
                        self.flush_clean_segment(&mut owned, seg_start, pos_before);
                        self.input.consume_peeked();
                        seg_start = self.input.current_pos().2;
                        continue;
                    }
                    // §2.11 EOL handling: a literal CR is delivered as an LF
                    // (the CRLF pair is consumed as one advance), so the
                    // delivered byte differs from the source — a patch.
                    if c == '\n' && self.input.peek_raw() == Some(b'\r') {
                        below_split |= pos_before < self.split_chars_at.unwrap_or(usize::MAX);
                        self.flush_clean_segment(&mut owned, seg_start, pos_before);
                        // Already peeked above — consume without a second
                        // decode (§16.5.5).
                        self.input.consume_peeked();
                        if let Some(v) = owned.as_mut() {
                            v.push(b'\n');
                        }
                        seg_start = self.input.current_pos().2;
                        continue;
                    }
                    // upstream: ']]>' is reported when cur is at the first ']'.
                    if c == ']' {
                        // Lookahead at bytes pos+1 and pos+2 (the current
                        // byte is the first ']').
                        let rest = self.peek_bytes(3);
                        if rest.len() == 3 && rest[1] == b']' && rest[2] == b'>' {
                            // UPSTREAM-PARITY (parser.c 2.15
                            // xmlParseCharDataInternal slow path): the error is
                            // raised with `input->col` at the position just past
                            // the first ']' (NEXTL bumped the column for every
                            // character consumed in the run), which the oracle
                            // reports as the SECOND ']' — not the start of the
                            // character-data run.
                            let at = self.input.current_pos().2 + 1;
                            self.record_error_at(
                                crate::abi::types::XML_FROM_PARSER,
                                crate::abi::types::XML_ERR_MISPLACED_CDATA_END,
                                crate::abi::types::xmlErrorLevel::XML_ERR_FATAL as c_int,
                                "Sequence ']]>' not allowed in content\n".to_string(),
                                None,
                                None,
                                None,
                                0,
                                at,
                                None,
                            );
                        }
                    }
                    // Already peeked above — consume without a second decode
                    // (§16.5.5).
                    self.input.consume_peeked();
                    below_split |= pos_before < self.split_chars_at.unwrap_or(usize::MAX);
                    // §16.5.3: nothing is appended here — clean bytes stay in
                    // the pending segment `[seg_start, pos)` and are flushed
                    // in bulk at the next patch / pop / finish.
                }
                None => break,
            }
        }

        // After a boundary-crossing pop the owned buffer is complete (the
        // pre-pop flush captured everything); the current position belongs to
        // the OUTER input, so it must not extend the run.
        let end = if crossed_pop {
            seg_start
        } else {
            self.input.current_pos().2
        };
        XmlToken::Characters(self.finish_text_run(&mut owned, seg_start, end, at_base))
    }

    /// §16.5.3: finish a text/body run. Flushes the pending tail
    /// `[seg_start, end)` into `owned` when the run already materialized;
    /// then returns: a never-patched BASE-input run as a span; a never-
    /// patched entity run (whose buffer may be dropped on pop) and any
    /// patched run as owned bytes. `end` is the byte offset just past the
    /// last CONTENT byte — for body scanners whose terminator is consumed
    /// before the break (comments `-->`, CDATA `]]>`) the caller passes the
    /// pre-terminator position.
    fn finish_text_run(
        &self,
        owned: &mut Option<Vec<u8>>,
        seg_start: usize,
        end: usize,
        at_base: bool,
    ) -> XmlText {
        if owned.is_some() && end > seg_start {
            self.flush_clean_segment(owned, seg_start, end);
        }
        match owned.take() {
            Some(v) => XmlText::Owned(v),
            None => {
                if at_base {
                    XmlText::Span {
                        start: seg_start,
                        end,
                    }
                } else {
                    XmlText::Owned(self.input.current_ref().raw_range(seg_start, end).to_vec())
                }
            }
        }
    }

    /// §16.5.3: append the source bytes `[start, end)` of the current input
    /// buffer to `owned`, materializing the buffer on first use. Called at
    /// the first patch point of a run (and before an entity input is
    /// auto-popped); after materialization the caller appends patched bytes
    /// explicitly and keeps `seg_start` advanced past them.
    fn flush_clean_segment(&self, owned: &mut Option<Vec<u8>>, start: usize, end: usize) {
        if owned.is_none() {
            *owned = Some(Vec::new());
        }
        if let Some(v) = owned.as_mut() {
            v.extend_from_slice(self.input.current_ref().raw_range(start, end));
        }
    }

    // ── Name scanning ───────────────────────────────────────────────────────

    /// §16.6 scalar fast path: whether `b` is an ASCII XML Name character
    /// that may follow the first character. UPSTREAM-PARITY
    /// (parserInternals.c xmlParseName / xmlIsNameChar): the ASCII set is
    /// alphanumerics plus `. - _ :` — NOT `+` (a name stops at `+`; the
    /// attribute loop then reports "error parsing attribute name" /
    /// "Specification mandates value for attribute", verified against the
    /// 2.15.3 oracle by the §16.7.7 differential court).
    #[inline]
    fn ascii_name_byte(b: u8) -> bool {
        b.is_ascii_alphanumeric() || b == b'.' || b == b'-' || b == b'_' || b == b':'
    }

    /// Whether the multi-byte UTF-8 sequence at the current position is
    /// MALFORMED (as opposed to merely INCOMPLETE). Mirrors every rejection in
    /// upstream `xmlCurrentChar`: a missing or wrong continuation byte, an
    /// overlong form, a surrogate code point, or a code point above U+10FFFF.
    fn utf8_sequence_is_malformed(&self) -> bool {
        let rem = self.input.current_ref().remaining();
        let b0 = match rem.first() {
            Some(&b) => b,
            None => return false,
        };
        if b0 < 0x80 {
            return false;
        }
        let at = |i: usize| rem.get(i).copied().unwrap_or(0);
        let cont = |b: u8| (b & 0xC0) == 0x80;
        if rem.len() < 2 {
            return false;
        }
        if !cont(at(1)) {
            return true;
        }
        if b0 < 0xE0 {
            return b0 < 0xC2;
        }
        if rem.len() < 3 {
            return false;
        }
        if !cont(at(2)) {
            return true;
        }
        if b0 < 0xF0 {
            let val = (u32::from(b0 & 0x0F) << 12)
                | (u32::from(at(1) & 0x3F) << 6)
                | u32::from(at(2) & 0x3F);
            return val < 0x800 || (0xD800..0xE000).contains(&val);
        }
        if rem.len() < 4 {
            return false;
        }
        if !cont(at(3)) {
            return true;
        }
        let val = (u32::from(b0 & 0x07) << 18)
            | (u32::from(at(1) & 0x3F) << 12)
            | (u32::from(at(2) & 0x3F) << 6)
            | u32::from(at(3) & 0x3F);
        !(0x10000..0x110000).contains(&val)
    }

    /// Record XML_ERR_INVALID_ENCODING for the current byte, ONCE per input
    /// (upstream `XML_INPUT_ENCODING_ERROR`).
    fn record_utf8_encoding_error(&mut self) {
        if self.input.current_ref().utf8_error_reported() {
            return;
        }
        self.input.current().set_utf8_error_reported();
        self.record_error(
            crate::abi::types::XML_FROM_IO,
            crate::abi::types::XML_ERR_INVALID_ENCODING,
            crate::abi::types::xmlErrorLevel::XML_ERR_FATAL as c_int,
            "Invalid bytes in character encoding\n".to_string(),
            None,
            None,
            None,
            0,
            None,
        );
    }

    /// Scan an XML Name.
    fn scan_name(&mut self) -> Vec<u8> {
        let mut name = Vec::new();
        let mut first = true;
        // §16.6 scalar engine: real names are overwhelmingly ASCII. When the
        // first byte is an ASCII NameStartChar (letter/'_'/':'), bulk-scan the
        // ASCII continuation bytes in one tight pass (no per-char UTF-8
        // decode); a name that starts non-ASCII (any byte >= 0x80 is a
        // NameStartChar) or whose ASCII prefix is followed by a multi-byte
        // char falls back to the per-char loop below for the remainder. The
        // consumed bytes are ASCII name chars, so none can be a line break
        // (safe for `skip_linebreak_free`).
        if let Some(b0) = self.input.peek_raw() {
            if b0 < 0x80 && (b0.is_ascii_alphabetic() || b0 == b'_' || b0 == b':') {
                let remaining = self.input.current_ref().remaining();
                let run = remaining
                    .iter()
                    .take_while(|&&b| Self::ascii_name_byte(b))
                    .count();
                if run > 0 {
                    name.extend_from_slice(&remaining[..run]);
                    self.input.skip_linebreak_free(run);
                    // The name already has at least one character: any tail
                    // (e.g. a multi-byte continuation) is handled by the
                    // per-char loop with `first == false`.
                    first = false;
                }
            }
        }

        loop {
            if self.input.is_eof() {
                break;
            }

            let c = match self.input.peek_char() {
                Some(c) => c,
                None => {
                    // A multi-byte UTF-8 sequence the decoder refused. Upstream
                    // `xmlParseNameComplex` reads through `xmlCurrentChar`, so a
                    // MALFORMED sequence raises XML_ERR_INVALID_ENCODING (once
                    // per input) at the offending byte before the name scan
                    // fails with XML_ERR_NAME_REQUIRED (`raw-high-name`: error
                    // 81 then the code-68 start-tag error, both at col 2, with
                    // the cursor left ON the byte). An INCOMPLETE sequence
                    // (fewer bytes than the length byte promises) is NOT an
                    // error — it simply ends the name.
                    if self.input.peek_raw().is_some() && self.utf8_sequence_is_malformed() {
                        self.record_utf8_encoding_error();
                    }
                    break;
                }
            };

            // XML Name characters (upstream xmlParseName / xmlIsNameChar):
            // the first byte must be a NameStartChar (letter, '_', ':' or
            // any byte >= 0x80); subsequent bytes may also be digits, '.',
            // '-', '_', ':' — NOT '+' (the §16.7.7 oracle court proved
            // names stop at '+': `<a+b/>` fails with "error parsing
            // attribute name" on 2.15.3).
            let ok = if first {
                c.is_alphabetic() || c == '_' || c == ':' || c as u32 >= 0x80
            } else {
                c.is_alphanumeric()
                    || c == '.'
                    || c == '-'
                    || c == '_'
                    || c == ':'
                    || c as u32 >= 0x80
            };
            if ok {
                // §16.5.5: the char was decoded by the peek above — consume
                // without a second decode.
                self.input.consume_peeked();
                Self::push_char(&mut name, c);
                first = false;
            } else {
                break;
            }
        }

        name
    }

    // ── Attribute value scanning ────────────────────────────────────────────

    // ── Byte-level peeking ──────────────────────────────────────────────────

    /// Peek at the next `n` bytes without consuming them.
    fn peek_bytes(&self, n: usize) -> Vec<u8> {
        let data = self.input.current_ref().remaining();
        data.iter().take(n).copied().collect()
    }
}

/// Byte length of the UTF-8 character starting at `data[0]` (0 if the
/// sequence is invalid or truncated) — upstream `xmlGetUTF8Char` length
/// semantics used by the source-window forward scan.
/// Compute the upstream-style source window `(context bytes, caret col)`
/// for an error at `byte_pos` in `data` (tokenizer `window_at` core — also
/// used by the SAX-layer depth error, HOSTILE-FAILURE F1).
pub(crate) fn window_at_data(data: &[u8], byte_pos: usize) -> Option<(Vec<u8>, usize)> {
    if byte_pos > data.len() {
        return None;
    }
    let byte_at = |p: usize| -> u8 { data.get(p).copied().unwrap_or(0) };
    let size = 80usize;

    // 1. Skip backwards over any end-of-lines.
    let mut cur = byte_pos;
    while cur > 0 && matches!(byte_at(cur), b'\n' | b'\r') {
        cur -= 1;
    }
    // 2. Search backwards for the beginning of the line (max 80 bytes).
    let mut n = 0usize;
    while n < size && cur > 0 && !matches!(byte_at(cur), b'\n' | b'\r') {
        cur -= 1;
        n += 1;
    }
    // 3. If a line break was found, step past it; otherwise skip
    //    continuation bytes so the window starts on a character boundary.
    if n > 0 && matches!(byte_at(cur), b'\n' | b'\r') {
        cur += 1;
    } else {
        while cur < byte_pos && (byte_at(cur) & 0xC0) == 0x80 {
            cur += 1;
        }
    }
    // 4. Caret column = offset of the error position within the window.
    let col = byte_pos - cur;
    // 5. Search forward for the end of the line (max 80 bytes of valid
    //    UTF-8; invalid bytes terminate the window like upstream).
    let mut fwd = cur;
    let mut n2 = 0usize;
    while !matches!(byte_at(fwd), 0 | b'\n' | b'\r') {
        let len = utf8_char_len(&data[fwd..]);
        if len == 0 || n2 + len > size {
            break;
        }
        fwd += len;
        n2 += len;
    }
    // Upstream (2.15): the caret can only point to the end of the
    // buffer if there's space for the marker — clamp to size-1.
    let mut col = col;
    if col >= n2 {
        col = if n2 < size { n2 } else { size - 1 };
    }
    Some((data[cur..fwd].to_vec(), col))
}

fn utf8_char_len(data: &[u8]) -> usize {
    if data.is_empty() {
        return 0;
    }
    let b = data[0];
    if b < 0x80 {
        return 1;
    }
    if (0xC2..=0xDF).contains(&b) {
        if data.len() >= 2 && (data[1] & 0xC0) == 0x80 {
            return 2;
        }
        return 0;
    }
    if (0xE0..=0xEF).contains(&b) {
        if data.len() >= 3 && (data[1] & 0xC0) == 0x80 && (data[2] & 0xC0) == 0x80 {
            return 3;
        }
        return 0;
    }
    if (0xF0..=0xF4).contains(&b) {
        if data.len() >= 4
            && (data[1] & 0xC0) == 0x80
            && (data[2] & 0xC0) == 0x80
            && (data[3] & 0xC0) == 0x80
        {
            return 4;
        }
        return 0;
    }
    0
}

/// Whether a byte is a valid XML Name character (upstream `IS_CHAR`-style
/// byte check used by the entity-name scan).
const fn is_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'.' || b == b'-' || b == b'_' || b == b':' || b >= 0x80
}

/// Upstream `IS_CHAR` (XML Char production).
fn is_valid_char_ref(codepoint: u32) -> bool {
    matches!(codepoint, 0x09 | 0x0A | 0x0D)
        || (0x20..=0xD7FF).contains(&codepoint)
        || (0xE000..=0xFFFD).contains(&codepoint)
        || (0x10000..=0x10FFFF).contains(&codepoint)
}
