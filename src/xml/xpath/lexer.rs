//! XPath 1.0 Expression Lexer/Tokenizer (§25).
//!
//! Tokenizes XPath expression strings into a stream of tokens
//! for the parser to consume.
//!
//! # UPSTREAM-PARITY
//!
//! Covers all XPath 1.0 token types: names, numbers, strings, operators,
//! axes, function names, variable references, punctuation.
//!
//! # Courts
//!
//! XPATH-LEXER-*

use std::fmt;

// ═══════════════════════════════════════════════════════════════════════════════
// Token Types
// ═══════════════════════════════════════════════════════════════════════════════

/// A token in an XPath expression.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // ── Names ────────────────────────────────────────────────────────────
    /// Name (NCName or QName)
    Name(String),
    /// `*` wildcard
    Star,
    /// `.` (self)
    Dot,
    /// `..` (parent)
    DotDot,

    // ── Operators ────────────────────────────────────────────────────────
    /// `@` (attribute axis)
    At,
    /// `::` (axis separator)
    DoubleColon,
    /// `/`
    Slash,
    /// `//`
    DoubleSlash,
    /// `|`
    Pipe,
    /// `+`
    Plus,
    /// `-`
    Minus,
    /// `=`
    Eq,
    /// `!=`
    Ne,
    /// `<`
    Lt,
    /// `>`
    Gt,
    /// `<=`
    Le,
    /// `>=`
    Ge,
    /// `*` (multiplication operator, distinct from wildcard)
    Multiply,

    // ── Keywords ─────────────────────────────────────────────────────────
    /// `or`
    Or,
    /// `and`
    And,
    /// `mod`
    Mod,
    /// `div`
    Div,
    /// `ancestor`
    Ancestor,
    /// `ancestor-or-self`
    AncestorOrSelf,
    /// `attribute`
    Attribute,
    /// `child`
    Child,
    /// `descendant`
    Descendant,
    /// `descendant-or-self`
    DescendantOrSelf,
    /// `following`
    Following,
    /// `following-sibling`
    FollowingSibling,
    /// `namespace`
    Namespace,
    /// `parent`
    Parent,
    /// `preceding`
    Preceding,
    /// `preceding-sibling`
    PrecedingSibling,
    /// `self`
    Self_,

    // ── Literals ─────────────────────────────────────────────────────────
    /// String literal (without quotes)
    StringLiteral(String),
    /// Numeric literal
    NumberLiteral(f64),

    // ── Punctuation ──────────────────────────────────────────────────────
    /// `(` — left parenthesis (groups sub-expressions, opens function calls)
    LParen,
    /// `)` — right parenthesis
    RParen,
    /// `[` — left bracket (opens a predicate)
    LBracket,
    /// `]` — right bracket (closes a predicate)
    RBracket,
    /// `{` — left brace (for XSLT attribute value templates; rare in XPath)
    LBrace,
    /// `}` — right brace
    RBrace,
    /// `,` — separates function call arguments
    Comma,
    /// `$` (variable reference)
    Dollar,

    // ── Special ──────────────────────────────────────────────────────────
    /// End of expression
    Eof,
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Token::Name(n) => write!(f, "{}", n),
            Token::Star => write!(f, "*"),
            Token::Dot => write!(f, "."),
            Token::DotDot => write!(f, ".."),
            Token::At => write!(f, "@"),
            Token::DoubleColon => write!(f, "::"),
            Token::Slash => write!(f, "/"),
            Token::DoubleSlash => write!(f, "//"),
            Token::Pipe => write!(f, "|"),
            Token::Plus => write!(f, "+"),
            Token::Minus => write!(f, "-"),
            Token::Eq => write!(f, "="),
            Token::Ne => write!(f, "!="),
            Token::Lt => write!(f, "<"),
            Token::Gt => write!(f, ">"),
            Token::Le => write!(f, "<="),
            Token::Ge => write!(f, ">="),
            Token::Multiply => write!(f, "*"),
            Token::Or => write!(f, "or"),
            Token::And => write!(f, "and"),
            Token::Mod => write!(f, "mod"),
            Token::Div => write!(f, "div"),
            Token::Ancestor => write!(f, "ancestor"),
            Token::AncestorOrSelf => write!(f, "ancestor-or-self"),
            Token::Attribute => write!(f, "attribute"),
            Token::Child => write!(f, "child"),
            Token::Descendant => write!(f, "descendant"),
            Token::DescendantOrSelf => write!(f, "descendant-or-self"),
            Token::Following => write!(f, "following"),
            Token::FollowingSibling => write!(f, "following-sibling"),
            Token::Namespace => write!(f, "namespace"),
            Token::Parent => write!(f, "parent"),
            Token::Preceding => write!(f, "preceding"),
            Token::PrecedingSibling => write!(f, "preceding-sibling"),
            Token::Self_ => write!(f, "self"),
            Token::StringLiteral(s) => write!(f, "'{}'", s),
            Token::NumberLiteral(n) => write!(f, "{}", n),
            Token::LParen => write!(f, "("),
            Token::RParen => write!(f, ")"),
            Token::LBracket => write!(f, "["),
            Token::RBracket => write!(f, "]"),
            Token::LBrace => write!(f, "{{"),
            Token::RBrace => write!(f, "}}"),
            Token::Comma => write!(f, ","),
            Token::Dollar => write!(f, "$"),
            Token::Eof => write!(f, "<EOF>"),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Lexer
// ═══════════════════════════════════════════════════════════════════════════════

/// XPath expression lexer.
///
/// Produces a stream of tokens from an XPath expression string.
#[derive(Debug, Clone)]
pub struct Lexer {
    /// Input bytes
    input: Vec<u8>,
    /// Current position
    pos: usize,
    /// Look-ahead character (0 if EOF)
    ch: u8,
    /// Whether we're at the start of an expression (helps with `-` vs `-`)
    at_start: bool,
}

impl Lexer {
    /// Create a lexer for the given XPath expression string.
    pub fn new(input: &str) -> Self {
        let bytes = input.as_bytes().to_vec();
        let ch = if bytes.is_empty() { 0 } else { bytes[0] };
        Self {
            input: bytes,
            pos: 0,
            ch,
            at_start: true,
        }
    }

    /// Advance to the next character.
    fn advance(&mut self) {
        self.pos += 1;
        self.ch = if self.pos < self.input.len() {
            self.input[self.pos]
        } else {
            0
        };
    }

    /// Peek at the next character without consuming it.
    fn peek(&self) -> u8 {
        if self.pos + 1 < self.input.len() {
            self.input[self.pos + 1]
        } else {
            0
        }
    }

    /// Skip whitespace.
    fn skip_ws(&mut self) {
        while self.ch != 0
            && (self.ch == b' ' || self.ch == b'\t' || self.ch == b'\n' || self.ch == b'\r')
        {
            self.advance();
        }
    }

    /// Read a name token (NCName).
    fn read_name(&mut self) -> String {
        let start = self.pos;
        while self.ch != 0
            && (self.ch.is_ascii_alphanumeric()
                || self.ch == b'_'
                || self.ch == b'-'
                || self.ch == b'.')
        {
            self.advance();
        }
        String::from_utf8_lossy(&self.input[start..self.pos]).to_string()
    }

    /// Try to match an axis name or keyword.
    fn try_keyword_or_axis(&self, name: &str) -> Option<Token> {
        match name {
            "or" => Some(Token::Or),
            "and" => Some(Token::And),
            "mod" => Some(Token::Mod),
            "div" => Some(Token::Div),
            "ancestor" => Some(Token::Ancestor),
            "ancestor-or-self" => Some(Token::AncestorOrSelf),
            "attribute" => Some(Token::Attribute),
            "child" => Some(Token::Child),
            "descendant" => Some(Token::Descendant),
            "descendant-or-self" => Some(Token::DescendantOrSelf),
            "following" => Some(Token::Following),
            "following-sibling" => Some(Token::FollowingSibling),
            "namespace" => Some(Token::Namespace),
            "parent" => Some(Token::Parent),
            "preceding" => Some(Token::Preceding),
            "preceding-sibling" => Some(Token::PrecedingSibling),
            "self" => Some(Token::Self_),
            _ => None,
        }
    }

    /// Read a numeric literal — a faithful port of upstream xpath.c
    /// `xmlXPathCompNumber` (R-000166). The oracle accumulates digits
    /// directly (`ret = ret * 10 + d`), caps the fraction at MAX_FRAC=20
    /// digits after any leading zeros, and applies the exponent with
    /// `pow(10.0, exp)` — which underflows to 0 for exponents below the
    /// smallest subnormal (e.g. `5e-324`). Rust's correctly-rounded
    /// `strtod`-style parse differs in those edge cases, so the accumulation
    /// is reproduced exactly.
    fn read_number(&mut self) -> f64 {
        let input = &self.input;
        let len = input.len();
        let mut cur = self.pos;

        // Integer part.
        let mut ret = 0.0f64;
        while cur < len && input[cur].is_ascii_digit() {
            ret = ret * 10.0 + (input[cur] - b'0') as f64;
            cur += 1;
        }

        // Fractional part (upstream consumes a trailing '.' even without
        // digits, so `5.` is a single number literal).
        let mut frac: i32 = 0;
        if cur < len && input[cur] == b'.' {
            cur += 1;
            while cur < len && input[cur] == b'0' {
                frac += 1;
                cur += 1;
            }
            let max = frac + 20; // MAX_FRAC
            let mut fraction = 0.0f64;
            while cur < len && input[cur].is_ascii_digit() && frac < max {
                let v = (input[cur] - b'0') as f64;
                fraction = fraction * 10.0 + v;
                frac += 1;
                cur += 1;
            }
            fraction /= 10f64.powf(frac as f64);
            ret += fraction;
            while cur < len && input[cur].is_ascii_digit() {
                cur += 1;
            }
        }

        // Exponent part (upstream xmlXPathCompNumber consumes 'e'/'E'
        // unconditionally, then an optional sign, then digits — greedily
        // even when malformed).
        let mut exponent: i32 = 0;
        let mut is_exponent_negative = false;
        if cur < len && (input[cur] == b'e' || input[cur] == b'E') {
            cur += 1;
            if cur < len && input[cur] == b'-' {
                is_exponent_negative = true;
                cur += 1;
            } else if cur < len && input[cur] == b'+' {
                cur += 1;
            }
            while cur < len && input[cur].is_ascii_digit() {
                if exponent < 1000000 {
                    exponent = exponent * 10 + (input[cur] - b'0') as i32;
                }
                cur += 1;
            }
        }
        if is_exponent_negative {
            exponent = -exponent;
        }
        ret *= 10f64.powf(exponent as f64);

        self.pos = cur;
        self.ch = if cur < len { input[cur] } else { 0 };
        ret
    }

    /// Read a string literal.
    fn read_string(&mut self, quote: u8) -> String {
        self.advance(); // consume opening quote
        let start = self.pos;
        while self.ch != 0 && self.ch != quote {
            self.advance();
        }
        let s = String::from_utf8_lossy(&self.input[start..self.pos]).to_string();
        if self.ch == quote {
            self.advance(); // consume closing quote
        }
        s
    }

    /// Get the next token.
    pub fn next_token(&mut self) -> Token {
        self.skip_ws();

        if self.ch == 0 {
            return Token::Eof;
        }

        // Save at_start for unary minus detection
        let _was_at_start = self.at_start;
        self.at_start = false;

        // ── Single-char tokens ────────────────────────────────────────────
        match self.ch {
            b'(' => {
                self.advance();
                return Token::LParen;
            }
            b')' => {
                self.advance();
                return Token::RParen;
            }
            b'[' => {
                self.advance();
                return Token::LBracket;
            }
            b']' => {
                self.advance();
                return Token::RBracket;
            }
            b'{' => {
                self.advance();
                return Token::LBrace;
            }
            b'}' => {
                self.advance();
                return Token::RBrace;
            }
            b',' => {
                self.advance();
                return Token::Comma;
            }
            b'$' => {
                self.advance();
                return Token::Dollar;
            }
            b'|' => {
                self.advance();
                return Token::Pipe;
            }
            b'+' => {
                self.advance();
                return Token::Plus;
            }
            b'@' => {
                self.advance();
                return Token::At;
            }
            b'.' => {
                if self.peek() == b'.' {
                    self.advance();
                    self.advance();
                    return Token::DotDot;
                }
                // Check if it's a number starting with '.'
                if self.peek().is_ascii_digit() {
                    return Token::NumberLiteral(self.read_number());
                }
                self.advance();
                return Token::Dot;
            }
            b'-' => {
                self.advance();
                // If at start or after operator, this is unary minus
                // We handle this at the parser level, just return Minus
                return Token::Minus;
            }
            b'=' => {
                self.advance();
                return Token::Eq;
            }
            b'!' => {
                if self.peek() == b'=' {
                    self.advance();
                    self.advance();
                    return Token::Ne;
                }
                // Invalid character, skip
                self.advance();
                return self.next_token();
            }
            b'<' => {
                self.advance();
                if self.ch == b'=' {
                    self.advance();
                    return Token::Le;
                }
                return Token::Lt;
            }
            b'>' => {
                self.advance();
                if self.ch == b'=' {
                    self.advance();
                    return Token::Ge;
                }
                return Token::Gt;
            }
            b'/' => {
                self.advance();
                if self.ch == b'/' {
                    self.advance();
                    return Token::DoubleSlash;
                }
                return Token::Slash;
            }
            b'*' => {
                self.advance();
                return Token::Star; // lexer returns Star; parser disambiguates
            }
            b':' => {
                if self.peek() == b':' {
                    self.advance();
                    self.advance();
                    return Token::DoubleColon;
                }
                // Single colon is part of a QName, handled below
                // Actually, if we see a colon, it should be part of a name
                // This case handles axis::name or prefix:name
                // Since we read the full name first, this shouldn't normally happen alone
                self.advance();
                return self.next_token();
            }
            b'\'' | b'"' => {
                let quote = self.ch;
                let s = self.read_string(quote);
                return Token::StringLiteral(s);
            }
            _ => {}
        }

        // ── Number ───────────────────────────────────────────────────────
        if self.ch.is_ascii_digit() {
            return Token::NumberLiteral(self.read_number());
        }

        // ── Name ─────────────────────────────────────────────────────────
        if self.ch.is_ascii_alphabetic() || self.ch == b'_' {
            let name = self.read_name();

            // Check for QName (prefix:local)
            if self.ch == b':' && self.peek() != b':' {
                self.advance(); // consume ':'
                if self.ch.is_ascii_alphabetic() || self.ch == b'_' || self.ch == b'*' {
                    if self.ch == b'*' {
                        self.advance();
                        let full = format!("{}:*", name);
                        return Token::Name(full);
                    }
                    let local = self.read_name();
                    return Token::Name(format!("{}:{}", name, local));
                }
                // If the colon is not followed by a valid name character,
                // it might be an axis separator that got split. Push back?
                // Actually in well-formed XPath, `name:` is followed by `:`
                // for axis:: or by a local name for QName.
                // We already checked peek != ':', so this is a QName prefix.
                // If the local part is missing, treat the whole thing as a name.
                return Token::Name(name);
            }

            // Check for axis separator: name::
            // We DON'T consume the :: here — we return just the axis keyword token.
            // The :: will be tokenized as DoubleColon on the next call to next_token().
            if self.ch == b':' && self.peek() == b':' {
                if let Some(axis) = self.try_keyword_or_axis(&name) {
                    return axis;
                }
                // Not an axis keyword — could be a QName prefix followed by ::?
                // Treat it as a regular name and let the :: be consumed separately.
                return Token::Name(name);
            }

            // Check for keyword or axis
            if let Some(keyword) = self.try_keyword_or_axis(&name) {
                return keyword;
            }

            return Token::Name(name);
        }

        // Unknown character, skip
        self.advance();
        self.next_token()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn tokenize(s: &str) -> Vec<Token> {
        let mut lexer = Lexer::new(s);
        let mut tokens = Vec::new();
        loop {
            let tok = lexer.next_token();
            let is_eof = matches!(tok, Token::Eof);
            tokens.push(tok);
            if is_eof {
                break;
            }
        }
        tokens
    }

    #[test]
    fn test_empty() {
        let tokens = tokenize("");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0], Token::Eof);
    }

    #[test]
    fn test_simple_path() {
        let tokens = tokenize("child::para");
        assert_eq!(
            tokens,
            vec![
                Token::Child,
                Token::DoubleColon,
                Token::Name("para".into()),
                Token::Eof,
            ]
        );
    }

    #[test]
    fn test_absolute_path() {
        let tokens = tokenize("/child::para");
        assert_eq!(
            tokens,
            vec![
                Token::Slash,
                Token::Child,
                Token::DoubleColon,
                Token::Name("para".into()),
                Token::Eof,
            ]
        );
    }

    #[test]
    fn test_short_form() {
        let tokens = tokenize("para");
        assert_eq!(tokens, vec![Token::Name("para".into()), Token::Eof]);
    }

    #[test]
    fn test_attribute() {
        let tokens = tokenize("@attr");
        assert_eq!(
            tokens,
            vec![Token::At, Token::Name("attr".into()), Token::Eof]
        );
    }

    #[test]
    fn test_predicate() {
        let tokens = tokenize("para[1]");
        assert_eq!(
            tokens,
            vec![
                Token::Name("para".into()),
                Token::LBracket,
                Token::NumberLiteral(1.0),
                Token::RBracket,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn test_function_call() {
        let tokens = tokenize("position()");
        assert_eq!(
            tokens,
            vec![
                Token::Name("position".into()),
                Token::LParen,
                Token::RParen,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn test_string_literal() {
        let tokens = tokenize("'hello'");
        assert_eq!(
            tokens,
            vec![Token::StringLiteral("hello".into()), Token::Eof]
        );
    }

    #[test]
    fn test_number() {
        let tokens = tokenize("42");
        assert_eq!(tokens, vec![Token::NumberLiteral(42.0), Token::Eof]);
    }
    #[allow(clippy::approx_constant)]
    #[test]
    fn test_decimal() {
        let tokens = tokenize("3.14");
        assert_eq!(tokens, vec![Token::NumberLiteral(3.14), Token::Eof]);
    }

    #[test]
    fn test_operators() {
        let tokens = tokenize("a = b and c != d or e < f");
        assert!(tokens.contains(&Token::Eq));
        assert!(tokens.contains(&Token::And));
        assert!(tokens.contains(&Token::Ne));
        assert!(tokens.contains(&Token::Or));
        assert!(tokens.contains(&Token::Lt));
    }

    #[test]
    fn test_union() {
        let tokens = tokenize("a | b");
        assert_eq!(
            tokens,
            vec![
                Token::Name("a".into()),
                Token::Pipe,
                Token::Name("b".into()),
                Token::Eof,
            ]
        );
    }

    #[test]
    fn test_double_slash() {
        let tokens = tokenize("//para");
        assert_eq!(
            tokens,
            vec![Token::DoubleSlash, Token::Name("para".into()), Token::Eof]
        );
    }

    #[test]
    fn test_qname() {
        let tokens = tokenize("xslt:template");
        assert_eq!(
            tokens,
            vec![Token::Name("xslt:template".into()), Token::Eof]
        );
    }

    #[test]
    fn test_wildcard() {
        let tokens = tokenize("*");
        assert_eq!(tokens, vec![Token::Star, Token::Eof]);
    }

    #[test]
    fn test_ns_wildcard() {
        let tokens = tokenize("ns:*");
        assert_eq!(tokens, vec![Token::Name("ns:*".into()), Token::Eof]);
    }

    #[test]
    fn test_dot_dot() {
        let tokens = tokenize("..");
        assert_eq!(tokens, vec![Token::DotDot, Token::Eof]);
    }

    #[test]
    fn test_axis_keyword() {
        let tokens = tokenize("ancestor-or-self::node()");
        assert_eq!(
            tokens,
            vec![
                Token::AncestorOrSelf,
                Token::DoubleColon,
                Token::Name("node".into()),
                Token::LParen,
                Token::RParen,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn test_complex_expression() {
        let tokens = tokenize("/html/body//div[@class='main']/p[1]");
        // Collect name-like tokens (including keyword tokens that can be element names)
        let names: Vec<String> = tokens
            .iter()
            .filter_map(|t| match t {
                Token::Name(n) => Some(n.clone()),
                Token::Div => Some("div".to_string()),
                Token::Mod => Some("mod".to_string()),
                Token::And => Some("and".to_string()),
                Token::Or => Some("or".to_string()),
                _ => None,
            })
            .collect();
        assert_eq!(names, vec!["html", "body", "div", "class", "p"]);
    }

    #[test]
    fn test_variable() {
        let tokens = tokenize("$var");
        assert_eq!(
            tokens,
            vec![Token::Dollar, Token::Name("var".into()), Token::Eof]
        );
    }
}
