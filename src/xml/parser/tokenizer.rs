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
    StartTag {
        name: Vec<u8>,
        attributes: Vec<(Vec<u8>, Vec<u8>)>,
        empty: bool,
    },

    /// End tag: `</name>`
    EndTag(Vec<u8>),

    /// Comment: `<!-- ... -->`
    Comment(Vec<u8>),

    /// Processing instruction: `<?target ...?>`
    ProcessingInstruction { target: Vec<u8>, data: Vec<u8> },

    /// CDATA section: `<![CDATA[ ... ]]>`
    Cdata(Vec<u8>),

    /// Character data (text content).
    Characters(Vec<u8>),

    /// Entity or character reference (`&name;`, `&#123;`, `&#xAB;`).
    Reference(Vec<u8>),
}

/// The XML tokenizer — scans lexical tokens from the input stack.
pub(crate) struct XmlTokenizer {
    input: InputStack,
    /// Buffer for a single pushed-back token (for one-token lookahead).
    push_back: Option<XmlToken>,
}

impl XmlTokenizer {
    /// Create a new tokenizer over the given input stack.
    pub fn new(input: InputStack) -> Self {
        XmlTokenizer {
            input,
            push_back: None,
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
            None => XmlToken::Eof,
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
        // Consume '<'
        self.input.read_char();

        if self.input.is_eof() {
            return XmlToken::Characters(b"<".to_vec());
        }

        match self.input.peek_char() {
            Some('/') => self.scan_end_tag(),
            Some('?') => self.scan_pi_or_xml_decl(),
            Some('!') => self.scan_markup_decl(),
            Some(_) => self.scan_start_tag(),
            None => XmlToken::Characters(b"<".to_vec()),
        }
    }

    /// Scan an end tag: `</name>`
    fn scan_end_tag(&mut self) -> XmlToken {
        debug_assert_eq!(self.input.peek_char(), Some('/'));
        // Consume '/'
        self.input.read_char();

        let name = self.scan_name();
        self.skip_whitespace();

        // Expect '>'
        if self.input.peek_char() == Some('>') {
            self.input.read_char();
        }

        XmlToken::EndTag(name)
    }

    /// Scan a start tag: `<name ...>` or `<name ... />`
    fn scan_start_tag(&mut self) -> XmlToken {
        let name = self.scan_name();
        let mut attributes: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        let mut empty = false;

        loop {
            self.skip_whitespace();

            if self.input.is_eof() {
                break;
            }

            match self.input.peek_char() {
                Some('>') => {
                    self.input.read_char();
                    break;
                }
                Some('/') => {
                    // Self-closing tag: <name .../>
                    self.input.read_char();
                    if self.input.peek_char() == Some('>') {
                        self.input.read_char();
                    }
                    empty = true;
                    break;
                }
                Some(_) => {
                    // Scan attribute: name="value" or name='value'
                    let attr_name = self.scan_name();
                    // If name is empty, break out to avoid infinite loop
                    if attr_name.is_empty() {
                        break;
                    }
                    self.skip_whitespace();

                    if self.input.peek_char() == Some('=') {
                        self.input.read_char();
                        self.skip_whitespace();
                        let value = self.scan_attr_value();
                        attributes.push((attr_name, value));
                    } else {
                        // Boolean attribute (minimized)
                        attributes.push((attr_name, Vec::new()));
                    }
                }
                None => break,
            }
        }

        XmlToken::StartTag {
            name,
            attributes,
            empty,
        }
    }

    /// Scan a PI or XML declaration after `<?`.
    fn scan_pi_or_xml_decl(&mut self) -> XmlToken {
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
                Some(c) => data.push(c as u8),
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

        XmlToken::ProcessingInstruction { target, data }
    }

    /// Scan after `<?xml` — read version, encoding, standalone pseudo-attributes.
    fn scan_xml_decl_rest(&mut self) -> XmlToken {
        let mut version = Vec::new();
        let mut encoding: Option<Vec<u8>> = None;
        let mut standalone: Option<Vec<u8>> = None;

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
        }

        XmlToken::XmlDecl {
            version,
            encoding,
            standalone,
        }
    }

    /// Scan a markup declaration after `<!`.
    fn scan_markup_decl(&mut self) -> XmlToken {
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
            return self.scan_cdata_body();
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
                Some(c) => content.push(c as u8),
                None => break,
            }
        }

        XmlToken::Characters(content)
    }

    /// Scan a comment body (after `<!--`).
    fn scan_comment_body(&mut self) -> XmlToken {
        let mut content = Vec::new();

        loop {
            if self.input.is_eof() {
                break;
            }

            // Check for `-->`
            if self.input.peek_char() == Some('-') {
                self.input.read_char();
                if self.input.peek_char() == Some('-') {
                    self.input.read_char();
                    if self.input.peek_char() == Some('>') {
                        self.input.read_char();
                        break;
                    }
                    // Not `-->`, include what we read
                    content.push(b'-');
                    content.push(b'-');
                    continue;
                }
                content.push(b'-');
                continue;
            }

            match self.input.read_char() {
                Some(c) => content.push(c as u8),
                None => break,
            }
        }

        XmlToken::Comment(content)
    }

    /// Scan a CDATA section body (after `<![CDATA[`).
    fn scan_cdata_body(&mut self) -> XmlToken {
        let mut content = Vec::new();

        loop {
            if self.input.is_eof() {
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
                Some(c) => content.push(c as u8),
                None => break,
            }
        }

        XmlToken::Cdata(content)
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
                    content.push(c as u8);
                }
                None => break,
            }
        }

        XmlToken::DocType(content)
    }

    // ── Reference scanning ──────────────────────────────────────────────────

    /// Scan a reference starting with `&`
    fn scan_reference(&mut self) -> XmlToken {
        debug_assert_eq!(self.input.peek_char(), Some('&'));
        // Consume '&'
        self.input.read_char();

        let mut content = vec![b'&'];

        loop {
            match self.input.read_char() {
                Some(';') => {
                    content.push(b';');
                    break;
                }
                Some(c) => content.push(c as u8),
                None => break,
            }
        }

        XmlToken::Reference(content)
    }

    // ── Character data scanning ─────────────────────────────────────────────

    /// Scan character data until '<' or '&' is encountered.
    fn scan_characters(&mut self) -> XmlToken {
        let mut content = Vec::new();

        loop {
            if self.input.is_eof() {
                break;
            }

            match self.input.peek_char() {
                Some('<') | Some('&') => break,
                Some(c) => {
                    self.input.read_char();
                    content.push(c as u8);
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

        loop {
            if self.input.is_eof() {
                break;
            }

            let c = match self.input.peek_char() {
                Some(c) => c,
                None => break,
            };

            // XML Name characters: letters, digits, '.', '-', '_', ':'
            if c.is_alphanumeric() || c == '.' || c == '-' || c == '_' || c == ':' {
                self.input.read_char();
                name.push(c as u8);
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
                Some(c) => value.push(c as u8),
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
