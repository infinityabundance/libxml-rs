//! XML tokenizer — lexical scanning for the parser state machine (§85 Phase 3).
//!
//! Produces `XmlToken` values from the input stack. The tokenizer handles
//! low-level scanning: tags, comments, PIs, CDATA sections, character data,
//! entity/character references, and the XML declaration.

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

    /// DOCTYPE declaration (content between `<!DOCTYPE` and closing `>`).
    DocType(Vec<u8>),

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
    EndTag { name: Vec<u8>, start_pos: usize },

    /// Comment: `<!-- ... -->`
    Comment(Vec<u8>),

    /// Processing instruction: `<?target ...?>`, with the byte offset of
    /// the leading `<?` (for document-level "invalid element name" errors).
    ProcessingInstruction {
        target: Vec<u8>,
        data: Vec<u8>,
        start_pos: usize,
    },

    /// CDATA section: `<![CDATA[ ... ]]>` (carries the `<` byte offset and
    /// whether the section was terminated).
    Cdata {
        data: Vec<u8>,
        unterminated: bool,
        start_pos: usize,
    },

    /// Character data (text content).
    Characters(Vec<u8>),

    /// Entity or character reference (`&name;`, `&#123;`, `&#xAB;`).
    Reference(Vec<u8>),
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
    /// Source window (line bytes) + 0-based caret column, computed with
    /// upstream's `xmlParserInputGetWindow` algorithm (80-char cap).
    pub window: Option<(Vec<u8>, usize)>,
    /// For `XML_ERR_INVALID_ENCODING`: the 4 bytes at the error position
    /// (upstream `xmlFormatError` "Bytes:" fragment).
    pub enc_bytes: Option<[u8; 4]>,
}

/// The XML tokenizer — scans lexical tokens from the input stack.
pub(crate) struct XmlTokenizer {
    input: InputStack,
    /// Buffer for a single pushed-back token (for one-token lookahead).
    push_back: Option<XmlToken>,
    /// Parser errors recorded during scanning (drained by the parser).
    errors: Vec<ErrorInfo>,
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
    pub fn new(input: InputStack) -> Self {
        XmlTokenizer {
            input,
            push_back: None,
            errors: Vec::new(),
        }
    }

    /// Get a mutable reference to the input stack.
    pub fn input_mut(&mut self) -> &mut InputStack {
        &mut self.input
    }

    /// Get a reference to the input stack.
    pub fn input(&self) -> &InputStack {
        &self.input
    }

    /// Consume the tokenizer and return the input stack.
    pub fn into_input(self) -> InputStack {
        self.input
    }

    /// Push a new input onto the input stack (for entity expansion).
    pub fn push_input(&mut self, buf: InputBuffer) {
        self.input.push(buf);
    }

    /// Pop the current input from the stack.
    pub fn pop_input(&mut self) -> Option<InputBuffer> {
        self.input.pop()
    }

    /// Return the current position as `(line, col, byte_offset)`.
    pub fn current_pos(&self) -> (usize, usize, usize) {
        self.input.current_pos()
    }

    // ── Error recording ─────────────────────────────────────────────────────

    /// Record a parser error at the current input position.
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
        enc_bytes: Option<[u8; 4]>,
    ) {
        let pos = self.input.current_pos().2;
        self.record_error_at(
            domain, code, level, msg, str1, str2, str3, int1, pos, enc_bytes,
        );
    }

    /// Record a parser error at an arbitrary byte position (token start).
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
        enc_bytes: Option<[u8; 4]>,
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
            window,
            enc_bytes,
        });
    }

    /// Drain the recorded errors (in order).
    pub fn take_errors(&mut self) -> Vec<ErrorInfo> {
        core::mem::take(&mut self.errors)
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
        let (line, col) = self.line_col_at(byte_pos);
        let window = self.window_at(byte_pos);
        (line, col, window)
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

    /// Skip whitespace characters (space, tab, CR, LF).
    fn skip_whitespace(&mut self) {
        loop {
            match self.input.peek_char() {
                Some(c) if c.is_ascii_whitespace() && c != '\0' => {
                    self.input.read_char();
                }
                _ => break,
            }
        }
    }

    // ── Tag/markup scanning ─────────────────────────────────────────────────

    /// Scan after seeing '<'. Determines whether this is a tag, comment, PI, CDATA, or DOCTYPE.
    fn scan_tag_or_markup(&mut self) -> XmlToken {
        debug_assert_eq!(self.input.peek_char(), Some('<'));
        let start_pos = self.input.current_pos().2;
        // Consume '<'
        self.input.read_char();

        if self.input.is_eof() {
            return XmlToken::Characters(b"<".to_vec());
        }

        match self.input.peek_char() {
            Some('/') => self.scan_end_tag(start_pos),
            Some('?') => self.scan_pi_or_xml_decl(start_pos),
            Some('!') => self.scan_markup_decl(start_pos),
            Some(_) => self.scan_start_tag(),
            None => XmlToken::Characters(b"<".to_vec()),
        }
    }

    /// Scan an end tag: `</name>`
    fn scan_end_tag(&mut self, start_pos: usize) -> XmlToken {
        debug_assert_eq!(self.input.peek_char(), Some('/'));
        // Consume '/'
        self.input.read_char();

        let name = self.scan_name();
        self.skip_whitespace();

        // Expect '>'
        if self.input.peek_char() == Some('>') {
            self.input.read_char();
        }

        XmlToken::EndTag { name, start_pos }
    }

    /// Scan a start tag: `<name ...>` or `<name ... />`, replicating
    /// upstream `xmlParseStartTag2` error semantics (11.1-M): invalid
    /// names, attribute-value errors, missing values, duplicate
    /// attributes, "attributes construct error", and unterminated tags are
    /// recorded with upstream codes/levels at the exact detection position.
    fn scan_start_tag(&mut self) -> XmlToken {
        let name = self.scan_name();
        let mut attributes: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
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
            // "StartTag: invalid element name\n" (XML_ERR_NAME_REQUIRED).
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
                Some('/') => {
                    // Self-closing tag: <name .../>
                    end_pos = Some(self.input.current_pos().2);
                    self.input.read_char();
                    if self.input.peek_char() == Some('>') {
                        self.input.read_char();
                    }
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
                                unterminated = true;
                                break;
                            }
                            // Blanks consumed: continue the attribute loop.
                        }
                    }
                }
            }
        }

        // upstream: duplicate attribute names are detected after the tag
        // (RAW still at '>' or '/') → "Attribute %s redefined\n" (42),
        // str1 = name.
        let mut seen: Vec<Vec<u8>> = Vec::new();
        let dup_pos = end_pos.unwrap_or_else(|| self.input.current_pos().2);
        for (an, _) in &attributes {
            if seen.iter().any(|s| s == an) {
                self.record_error_at(
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
                break;
            }
            seen.push(an.clone());
        }

        if unterminated {
            // upstream xmlParseElementStart end-of-tag check:
            // "Couldn't find end of Start Tag %s line %d\n" (73),
            // str1 = name, int1 = start line.
            self.record_error(
                crate::abi::types::XML_FROM_PARSER,
                crate::abi::types::XML_ERR_GT_REQUIRED,
                crate::abi::types::xmlErrorLevel::XML_ERR_FATAL as c_int,
                format!(
                    "Couldn't find end of Start Tag {} line {}\n",
                    String::from_utf8_lossy(&name),
                    open_line
                ),
                Some(name.clone()),
                None,
                None,
                open_line,
                None,
            );
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
                    continue;
                }
            }
            match self.input.peek_char() {
                Some(c) if c == quote => {
                    self.input.read_char();
                    return (value, true);
                }
                Some('&') => {
                    self.input.read_char();
                    value.push(b'&');
                    let mut name = Vec::new();
                    loop {
                        match self.input.peek_char() {
                            Some(c) if is_name_byte(c as u8) => {
                                value.push(c as u8);
                                name.push(c as u8);
                                self.input.read_char();
                            }
                            _ => break,
                        }
                    }
                    if self.input.peek_char() == Some(';') {
                        value.push(b';');
                        self.input.read_char();
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
                    Self::push_char(&mut value, c);
                    self.input.read_char();
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
        let mut bytes = [0u8; 4];
        for (i, slot) in bytes.iter_mut().enumerate() {
            if let Some(&b) = remaining.get(i) {
                *slot = b;
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
            Some(bytes),
        );
    }

    /// Scan a PI or XML declaration after `<?`.
    fn scan_pi_or_xml_decl(&mut self, start_pos: usize) -> XmlToken {
        debug_assert_eq!(self.input.peek_char(), Some('?'));
        // Consume '?'
        self.input.read_char();

        // Peek ahead to see if the next characters are "xml" (case-insensitive)
        let next_bytes = self.peek_bytes(3);
        let is_xml_decl = next_bytes.len() >= 3
            && (next_bytes[0] == b'x' || next_bytes[0] == b'X')
            && (next_bytes[1] == b'm' || next_bytes[1] == b'M')
            && (next_bytes[2] == b'l' || next_bytes[2] == b'L');

        if is_xml_decl {
            // Consume "xml"
            self.input.read_char(); // x/X
            self.input.read_char(); // m/M
            self.input.read_char(); // l/L
            return self.scan_xml_decl_rest();
        }

        // Regular processing instruction
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
            };
        }

        // Skip whitespace before data
        if self
            .input
            .peek_char()
            .map_or(false, |c| c.is_ascii_whitespace())
        {
            self.input.read_char();
        }

        // Read data until "?>"
        let mut data = Vec::new();
        loop {
            if self.input.is_eof() {
                break;
            }
            if self.input.peek_char() == Some('?') {
                // Peek for '>'
                let saved = self.input.current().pos();
                self.input.read_char();
                if self.input.peek_char() == Some('>') {
                    self.input.read_char();
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

        // Trim trailing whitespace from data (libxml2 behavior)
        while data.last() == Some(&b' ')
            || data.last() == Some(&b'\t')
            || data.last() == Some(&b'\n')
            || data.last() == Some(&b'\r')
        {
            data.pop();
        }

        XmlToken::ProcessingInstruction {
            target,
            data,
            start_pos,
        }
    }

    /// Scan after `<?xml` — read version, encoding, standalone pseudo-attributes.
    /// Records upstream `xmlParseXMLDecl` errors ("Blank needed here\n" (65)
    /// and "parsing XML declaration: '?>' expected\n" (57)).
    fn scan_xml_decl_rest(&mut self) -> XmlToken {
        let mut version = Vec::new();
        let mut encoding: Option<Vec<u8>> = None;
        let mut standalone: Option<Vec<u8>> = None;
        let mut terminated = false;

        loop {
            self.skip_whitespace();

            if self.input.is_eof() {
                break;
            }

            // Check for closing ?>
            if self.input.peek_char() == Some('?') {
                self.input.read_char();
                if self.input.peek_char() == Some('>') {
                    self.input.read_char();
                    terminated = true;
                    break;
                }
                // Not "?>", push '?' into data? Just continue.
                continue;
            }

            // Read pseudo-attribute name
            let attr_name = self.scan_name();
            if attr_name.is_empty() {
                break;
            }
            self.skip_whitespace();

            if self.input.peek_char() == Some('=') {
                self.input.read_char();
                self.skip_whitespace();
                let value = self.scan_attr_value();

                let lower = attr_name.to_ascii_lowercase();
                if lower == b"version" {
                    version = value;
                } else if lower == b"encoding" {
                    encoding = Some(value);
                } else if lower == b"standalone" {
                    standalone = Some(value);
                }
            }

            // upstream xmlParseXMLDecl: after each pseudo-attribute a blank
            // must follow unless the declaration ends here ('?>'). At EOF
            // RAW is 0 (not blank, not '?') so "Blank needed here" fires.
            if self.input.peek_char() != Some('?') {
                let before = self.input.current_pos().2;
                self.skip_whitespace();
                if self.input.current_pos().2 == before {
                    self.record_error(
                        crate::abi::types::XML_FROM_PARSER,
                        crate::abi::types::XML_ERR_SPACE_REQUIRED,
                        crate::abi::types::xmlErrorLevel::XML_ERR_FATAL as c_int,
                        "Blank needed here\n".to_string(),
                        None,
                        None,
                        None,
                        0,
                        None,
                    );
                }
            }
        }

        if !terminated {
            // upstream xmlParseXMLDecl end: missing '?>' →
            // "parsing XML declaration: '?>' expected\n" (57).
            self.record_error(
                crate::abi::types::XML_FROM_PARSER,
                crate::abi::types::XML_ERR_XMLDECL_NOT_FINISHED,
                crate::abi::types::xmlErrorLevel::XML_ERR_FATAL as c_int,
                "parsing XML declaration: '?>' expected\n".to_string(),
                None,
                None,
                None,
                0,
                None,
            );
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
            return XmlToken::Characters(b"<!".to_vec());
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

        // Unknown markup declaration — consume until '>'
        let mut content = vec![b'!'];
        loop {
            match self.input.read_char() {
                Some('>') => break,
                Some(c) => Self::push_char(&mut content, c),
                None => break,
            }
        }

        XmlToken::Characters(content)
    }

    /// Scan a comment body (after `<!--`).
    fn scan_comment_body(&mut self) -> XmlToken {
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
                    // UPSTREAM-PARITY (parser.c xmlParseCommentComplex): a
                    // double hyphen inside a comment (not `-->`) is a fatal
                    // WFC error "Double hyphen within comment: <!--%.50s\n"
                    // (XML_ERR_HYPHEN_IN_COMMENT); parsing continues past
                    // the two hyphens, which are NOT part of the content.
                    // R-000166.
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

        if unterminated {
            // upstream xmlParseComment: EOF → "Comment not terminated\n"
            // (XML_ERR_COMMENT_NOT_FINISHED).
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

        XmlToken::Comment(content)
    }

    /// Scan a CDATA section body (after `<![CDATA[`).
    fn scan_cdata_body(&mut self, start_pos: usize) -> XmlToken {
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

        if unterminated {
            // upstream xmlParseCDSect: EOF → "Premature end of data in
            // CDATA section\n" (XML_ERR_CDATA_NOT_FINISHED). The parser
            // raises this only when the CDATA is in element content; at
            // document level it reports the invalid element name instead.
            self.record_error(
                crate::abi::types::XML_FROM_PARSER,
                crate::abi::types::XML_ERR_CDATA_NOT_FINISHED,
                crate::abi::types::xmlErrorLevel::XML_ERR_FATAL as c_int,
                "Premature end of data in CDATA section\n".to_string(),
                None,
                None,
                None,
                0,
                None,
            );
        }

        XmlToken::Cdata {
            data: content,
            unterminated,
            start_pos,
        }
    }

    /// Scan a DOCTYPE body (after `<!DOCTYPE`).
    fn scan_doctype_body(&mut self) -> XmlToken {
        let mut content = Vec::new();
        let mut depth: usize = 0;

        loop {
            if self.input.is_eof() {
                break;
            }

            match self.input.peek_char() {
                Some('>') if depth == 0 => {
                    self.input.read_char();
                    break;
                }
                Some('[') => {
                    depth += 1;
                    self.input.read_char();
                    content.push(b'[');
                }
                Some(']') => {
                    if depth > 0 {
                        depth -= 1;
                    }
                    self.input.read_char();
                    content.push(b']');
                }
                Some(c) => {
                    self.input.read_char();
                    Self::push_char(&mut content, c);
                }
                None => break,
            }
        }

        XmlToken::DocType(content)
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
            let hex = matches!(self.input.peek_char(), Some('x') | Some('X'));
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
        let mut name = Vec::new();
        loop {
            match self.input.peek_char() {
                Some(c) if is_name_byte(c as u8) => {
                    content.push(c as u8);
                    name.push(c as u8);
                    self.input.read_char();
                }
                _ => break,
            }
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
    fn scan_characters(&mut self) -> XmlToken {
        let mut content = Vec::new();

        loop {
            if self.input.is_eof() {
                break;
            }
            // Invalid UTF-8 byte: upstream xmlCurrentChar encoding error.
            if let Some(b) = self.input.peek_raw() {
                if b >= 0x80 && self.input.peek_char().is_none() {
                    self.record_encoding_error();
                    self.input.skip_raw_bytes(1);
                    continue;
                }
            }

            match self.input.peek_char() {
                Some('<') | Some('&') => break,
                Some(c) => {
                    let cp = c as u32;
                    // upstream xmlParseCharDataComplex: PCDATA invalid Char.
                    if !is_valid_char_ref(cp) {
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
                        // the error).
                        self.input.read_char();
                        continue;
                    }
                    // upstream: ']]>' is reported when cur is at the first ']'.
                    if c == ']' {
                        // Lookahead at bytes pos+1 and pos+2 (the current
                        // byte is the first ']').
                        let rest = self.peek_bytes(3);
                        if rest.len() == 3 && rest[1] == b']' && rest[2] == b'>' {
                            self.record_error(
                                crate::abi::types::XML_FROM_PARSER,
                                crate::abi::types::XML_ERR_MISPLACED_CDATA_END,
                                crate::abi::types::xmlErrorLevel::XML_ERR_FATAL as c_int,
                                "Sequence ']]>' not allowed in content\n".to_string(),
                                None,
                                None,
                                None,
                                0,
                                None,
                            );
                        }
                    }
                    self.input.read_char();
                    Self::push_char(&mut content, c);
                }
                None => break,
            }
        }

        XmlToken::Characters(content)
    }

    // ── Name scanning ───────────────────────────────────────────────────────

    /// Scan an XML Name.
    fn scan_name(&mut self) -> Vec<u8> {
        let mut name = Vec::new();
        let mut first = true;

        loop {
            if self.input.is_eof() {
                break;
            }

            let c = match self.input.peek_char() {
                Some(c) => c,
                None => break,
            };

            // XML Name characters (upstream xmlParseName): the first byte
            // must be a NameStartChar (letter, '_', ':' or any byte >= 0x80);
            // subsequent bytes may also be digits, '.', '-', '+', '_', ':'
            // (the '+' is accepted by libxml2's lenient IS_CHAR check).
            let ok = if first {
                c.is_alphabetic() || c == '_' || c == ':' || c as u32 >= 0x80
            } else {
                c.is_alphanumeric()
                    || c == '.'
                    || c == '-'
                    || c == '+'
                    || c == '_'
                    || c == ':'
                    || c as u32 >= 0x80
            };
            if ok {
                self.input.read_char();
                Self::push_char(&mut name, c);
                first = false;
            } else {
                break;
            }
        }

        name
    }

    // ── Attribute value scanning ────────────────────────────────────────────

    /// Scan an attribute value (between quotes).
    fn scan_attr_value(&mut self) -> Vec<u8> {
        let quote = match self.input.peek_char() {
            Some('"') | Some('\'') => self.input.read_char().unwrap(),
            _ => return Vec::new(),
        };

        let mut value = Vec::new();

        loop {
            match self.input.read_char() {
                Some(c) if c == quote => break,
                Some(c) => Self::push_char(&mut value, c),
                None => break,
            }
        }

        value
    }

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
fn is_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'.' || b == b'-' || b == b'_' || b == b':' || b >= 0x80
}

/// Upstream `IS_CHAR` (XML Char production).
fn is_valid_char_ref(codepoint: u32) -> bool {
    matches!(codepoint, 0x09 | 0x0A | 0x0D)
        || (0x20..=0xD7FF).contains(&codepoint)
        || (0xE000..=0xFFFD).contains(&codepoint)
        || (0x10000..=0x10FFFF).contains(&codepoint)
}
