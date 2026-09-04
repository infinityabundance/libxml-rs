//! XPath 1.0 Expression Parser (§25).
//!
//! Parses token streams from the lexer into the AST defined in `ast.rs`.
//! Implements the full XPath 1.0 grammar.
//!
//! # UPSTREAM-PARITY
//!
//! Grammar (from XPath 1.0 spec §3.7):
//!
//! ```text
//! Expr        ::= OrExpr
//! OrExpr      ::= AndExpr ('or' AndExpr)*
//! AndExpr     ::= EqualityExpr ('and' EqualityExpr)*
//! EqualityExpr ::= RelationalExpr (('=' | '!=') RelationalExpr)*
//! RelationalExpr ::= AdditiveExpr (('<' | '>' | '<=' | '>=') AdditiveExpr)*
//! AdditiveExpr ::= MultiplicativeExpr (('+' | '-') MultiplicativeExpr)*
//! MultiplicativeExpr ::= UnaryExpr (('*' | 'div' | 'mod') UnaryExpr)*
//! UnaryExpr   ::= '-'* UnionExpr
//! UnionExpr   ::= PathExpr ('|' PathExpr)*
//! PathExpr    ::= LocationPath | FilterExpr (('/' | '//') RelativeLocationPath)?
//! LocationPath ::= AbsoluteLocationPath | RelativeLocationPath
//! AbsoluteLocationPath ::= '/' RelativeLocationPath? | '//' RelativeLocationPath
//! RelativeLocationPath ::= Step (('/' | '//') Step)*
//! Step        ::= AxisSpecifier NodeTest Predicate*
//!              |  AbbreviatedStep
//! AxisSpecifier ::= AxisName '::' | '@'?
//! AbbreviatedStep ::= '.' | '..'
//! Predicate   ::= '[' Expr ']'
//! FilterExpr  ::= PrimaryExpr Predicate*
//! PrimaryExpr ::= VariableReference | '(' Expr ')' | Literal | Number | FunctionCall
//! ```
//!
//! # Courts
//!
//! XPATH-PARSER-*
//!
//! # Upstream contract
//!
//! Mirrors the compilation half of upstream `xpath.c`
//! (`SRC-LIBXML2-2.15.0-XPATH-C`, parity target libxml2 2.15.3 oracle):
//! xmlXPathCompile builds an xmlXPathCompExpr from the same grammar this
//! recursive-descent parser implements (XPath 1.0 §3.7).
//!
//! # Conceptual behavior
//!
//! Implements the full XPath 1.0 grammar from the lexer token stream into
//! the ast.rs expression tree, including precedence (or → and → equality
//! → relational → additive → multiplicative → unary → union → path),
//! predicates on steps and filter expressions, abbreviated steps and the
//! axis-specifier forms.
//!
//! # Ownership & safety invariants
//!
//! The parser owns the token stream for the duration of the parse and
//! produces an owned AST; `ParseError` carries an owned message and
//! position. No C pointers cross the parser boundary — compilation is
//! safe to run on any thread.
//!
//! # Historical quirks & epochs
//!
//! R-000105: node tests (`node()`, `text()`, `comment()`, `processing-
//! instruction()`) were originally parsed as function calls; the fix
//! distinguishes them at the node-test production, matching the 2.15.3
//! oracle. The grammar itself is stable across the oracle matrix (the
//! E-001 epoch changed xmllint node-set output, not expression parsing).
//!
//! # Deliberate oddities
//!
//! The parser accepts the upstream-lenient forms (e.g. whitespace
//! handling around abbreviated axes) that a strict grammar would reject,
//! because compile errors are observable through xmlXPathCompile return
//! values.
//!
//! # Proving courts
//!
//! XPATH-PARSER-* differential probes compile expressions against the
//! oracle and compare success/error byte-identical; the XSLT pattern
//! courts compile match patterns through this parser.
//!
//! # Tempting simplifications that would break parity
//!
//! Do not treat node-test names as generic function calls: R-000105
//! proved that breaks `//text()` style paths. Do not normalize or reject
//! lenient whitespace forms — xmlXPathCompile error parity is part of the
//! C ABI.

use crate::xml::xpath::ast::*;
use crate::xml::xpath::lexer::Token;

/// Errors that can occur during parsing.
#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    /// Human-readable description of what went wrong
    pub message: String,
    /// Token index at which the error was detected
    pub pos: usize,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "XPath parse error at position {}: {}",
            self.pos, self.message
        )
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Parser
// ═══════════════════════════════════════════════════════════════════════════════

/// Recursive-descent parser for XPath 1.0 expressions.
///
/// Consumes the token stream produced by the lexer and builds the
/// expression AST defined in `crate::xml::xpath::ast`.
#[derive(Debug)]
pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    /// Create a parser over a token stream produced by the lexer.
    pub const fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    /// Parse a complete XPath expression.
    pub fn parse(&mut self) -> Result<Expr, ParseError> {
        let expr = self.parse_or_expr()?;
        if !self.is_eof() {
            Err(self.error(format!("Unexpected token: {}", self.current())))?;
        }
        Ok(expr)
    }

    // ── Current token helpers ────────────────────────────────────────────

    fn current(&self) -> Token {
        if self.pos < self.tokens.len() {
            self.tokens[self.pos].clone()
        } else {
            Token::Eof
        }
    }

    fn peek(&self) -> Token {
        if self.pos + 1 < self.tokens.len() {
            self.tokens[self.pos + 1].clone()
        } else {
            Token::Eof
        }
    }

    const fn advance(&mut self) {
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
    }

    fn is_eof(&self) -> bool {
        matches!(self.current(), Token::Eof)
    }

    const fn error(&self, msg: String) -> ParseError {
        ParseError {
            message: msg,
            pos: self.pos,
        }
    }

    /// Check if the current token matches the given token.
    fn at(&self, token: &Token) -> bool {
        std::mem::discriminant(&self.current()) == std::mem::discriminant(token)
    }

    /// Expect and consume a specific token.
    fn expect(&mut self, expected: &Token) -> Result<(), ParseError> {
        if self.at(expected) {
            self.advance();
            Ok(())
        } else {
            Err(self.error(format!("Expected {}, got {}", expected, self.current())))
        }
    }

    // ── Grammar productions ──────────────────────────────────────────────

    /// OrExpr ::= AndExpr ('or' AndExpr)*
    fn parse_or_expr(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_and_expr()?;
        while matches!(self.current(), Token::Or) {
            self.advance();
            let right = self.parse_and_expr()?;
            left = Expr::BinaryOp {
                op: BinaryOp::Or,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    /// AndExpr ::= EqualityExpr ('and' EqualityExpr)*
    fn parse_and_expr(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_equality_expr()?;
        while matches!(self.current(), Token::And) {
            self.advance();
            let right = self.parse_equality_expr()?;
            left = Expr::BinaryOp {
                op: BinaryOp::And,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    /// EqualityExpr ::= RelationalExpr (('=' | '!=') RelationalExpr)*
    fn parse_equality_expr(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_relational_expr()?;
        loop {
            let op = match self.current() {
                Token::Eq => BinaryOp::Eq,
                Token::Ne => BinaryOp::Ne,
                _ => break,
            };
            self.advance();
            let right = self.parse_relational_expr()?;
            left = Expr::BinaryOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    /// RelationalExpr ::= AdditiveExpr (('<' | '>' | '<=' | '>=') AdditiveExpr)*
    fn parse_relational_expr(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_additive_expr()?;
        loop {
            let op = match self.current() {
                Token::Lt => BinaryOp::Lt,
                Token::Gt => BinaryOp::Gt,
                Token::Le => BinaryOp::Le,
                Token::Ge => BinaryOp::Ge,
                _ => break,
            };
            self.advance();
            let right = self.parse_additive_expr()?;
            left = Expr::BinaryOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    /// AdditiveExpr ::= MultiplicativeExpr (('+' | '-') MultiplicativeExpr)*
    fn parse_additive_expr(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_multiplicative_expr()?;
        loop {
            let op = match self.current() {
                Token::Plus => BinaryOp::Add,
                Token::Minus => BinaryOp::Sub,
                _ => break,
            };
            self.advance();
            let right = self.parse_multiplicative_expr()?;
            left = Expr::BinaryOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    /// MultiplicativeExpr ::= UnaryExpr (('*' | 'div' | 'mod') UnaryExpr)*
    fn parse_multiplicative_expr(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_unary_expr()?;
        loop {
            let op = match self.current() {
                // '*' after an expression is multiply, not wildcard
                Token::Star => BinaryOp::Mul,
                Token::Div => BinaryOp::Div,
                Token::Mod => BinaryOp::Mod,
                _ => break,
            };
            self.advance();
            let right = self.parse_unary_expr()?;
            left = Expr::BinaryOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    /// UnaryExpr ::= '-'* UnionExpr
    fn parse_unary_expr(&mut self) -> Result<Expr, ParseError> {
        let mut minus_count = 0;
        while matches!(self.current(), Token::Minus) {
            self.advance();
            minus_count += 1;
        }
        let mut expr = self.parse_union_expr()?;
        if minus_count % 2 == 1 {
            expr = Expr::UnaryMinus(Box::new(expr));
        }
        Ok(expr)
    }

    /// UnionExpr ::= PathExpr ('|' PathExpr)*
    fn parse_union_expr(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_path_expr()?;
        while matches!(self.current(), Token::Pipe) {
            self.advance();
            let right = self.parse_path_expr()?;
            left = Expr::Union(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    /// PathExpr ::= LocationPath | FilterExpr (('/' | '//') RelativeLocationPath)?
    fn parse_path_expr(&mut self) -> Result<Expr, ParseError> {
        // Check if it starts with a location path
        if self.is_location_path_start() {
            return self.parse_location_path();
        }

        // Otherwise it's a FilterExpr (primary with optional predicates and path)
        let mut expr = self.parse_filter_expr()?;

        // Optional / or // followed by relative location path
        loop {
            match self.current() {
                Token::Slash => {
                    self.advance();
                    let step = self.parse_relative_location_path()?;
                    expr = Expr::RelativePath(Box::new(expr), Box::new(step));
                }
                Token::DoubleSlash => {
                    self.advance();
                    let step = self.parse_relative_location_path()?;
                    // // is shorthand for /descendant-or-self::node()/
                    let descendant = Expr::Step(Step {
                        axis: Axis::DescendantOrSelf,
                        node_test: NodeTest::Node,
                        predicates: vec![],
                    });
                    let path = Expr::RelativePath(Box::new(descendant), Box::new(step));
                    expr = Expr::RelativePath(Box::new(expr), Box::new(path));
                }
                _ => break,
            }
        }

        Ok(expr)
    }

    /// Check if the current position starts a location path.
    fn is_location_path_start(&self) -> bool {
        match self.current() {
            Token::Slash | Token::DoubleSlash => true,
            Token::Dot | Token::DotDot => true,
            Token::At => true,
            Token::Star => true,
            // Name that could be a step (not followed by '(' which means function call)
            Token::Name(ref n) => {
                // Check if this is followed by :: (axis), /, //, [, or is a simple step
                let next = self.peek();
                if matches!(next, Token::LParen) {
                    // UPSTREAM-PARITY (xpath.c xmlXPathCompPathExpr): a name
                    // followed by '(' is only a function call for
                    // non-node-type names; node/text/comment/
                    // processing-instruction are node-type tests that BEGIN a
                    // location path (an expression-initial `node()` was
                    // previously misparsed as a function call, making
                    // apply-templates select="node()|@*" fail with
                    // "Unregistered function: node").
                    !n.contains(':')
                        && matches!(
                            n.as_str(),
                            "node" | "text" | "comment" | "processing-instruction"
                        )
                } else {
                    true
                }
            }
            // Axis keywords
            Token::Child
            | Token::Descendant
            | Token::DescendantOrSelf
            | Token::Ancestor
            | Token::AncestorOrSelf
            | Token::Attribute
            | Token::Following
            | Token::FollowingSibling
            | Token::Namespace
            | Token::Parent
            | Token::Preceding
            | Token::PrecedingSibling
            | Token::Self_ => true,
            _ => false,
        }
    }

    /// LocationPath ::= AbsoluteLocationPath | RelativeLocationPath
    fn parse_location_path(&mut self) -> Result<Expr, ParseError> {
        match self.current() {
            Token::Slash => {
                self.advance();
                if self.is_location_path_start() {
                    let path = self.parse_relative_location_path()?;
                    Ok(Expr::AbsolutePath(Box::new(path)))
                } else {
                    // Just "/" - root node
                    Ok(Expr::Step(Step {
                        axis: Axis::Self_,
                        node_test: NodeTest::Node,
                        predicates: vec![],
                    }))
                }
            }
            Token::DoubleSlash => {
                self.advance();
                let path = self.parse_relative_location_path()?;
                let descendant = Expr::Step(Step {
                    axis: Axis::DescendantOrSelf,
                    node_test: NodeTest::Node,
                    predicates: vec![],
                });
                Ok(Expr::AbsolutePath(Box::new(Expr::RelativePath(
                    Box::new(descendant),
                    Box::new(path),
                ))))
            }
            _ => self.parse_relative_location_path(),
        }
    }

    /// RelativeLocationPath ::= Step (('/' | '//') Step)*
    fn parse_relative_location_path(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_step()?;
        loop {
            match self.current() {
                Token::Slash => {
                    self.advance();
                    let step = self.parse_step()?;
                    expr = Expr::RelativePath(Box::new(expr), Box::new(step));
                }
                Token::DoubleSlash => {
                    self.advance();
                    let step = self.parse_step()?;
                    let descendant = Expr::Step(Step {
                        axis: Axis::DescendantOrSelf,
                        node_test: NodeTest::Node,
                        predicates: vec![],
                    });
                    let path = Expr::RelativePath(Box::new(descendant), Box::new(step));
                    expr = Expr::RelativePath(Box::new(expr), Box::new(path));
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    /// Step ::= AxisSpecifier NodeTest Predicate*
    ///        | AbbreviatedStep
    fn parse_step(&mut self) -> Result<Expr, ParseError> {
        // AbbreviatedStep ::= '.' | '..'
        match self.current() {
            Token::Dot => {
                self.advance();
                return Ok(Expr::Step(Step {
                    axis: Axis::Self_,
                    node_test: NodeTest::Node,
                    predicates: vec![],
                }));
            }
            Token::DotDot => {
                self.advance();
                return Ok(Expr::Step(Step {
                    axis: Axis::Parent,
                    node_test: NodeTest::Node,
                    predicates: vec![],
                }));
            }
            _ => {}
        }

        // Determine axis
        let axis = self.parse_axis_specifier();

        // Parse node test
        let node_test = self.parse_node_test()?;

        // Parse predicates
        let mut predicates = Vec::new();
        while matches!(self.current(), Token::LBracket) {
            self.advance(); // consume '['
            let pred = self.parse_or_expr()?;
            self.expect(&Token::RBracket)?;
            predicates.push(pred);
        }

        Ok(Expr::Step(Step {
            axis,
            node_test,
            predicates,
        }))
    }

    /// AxisSpecifier ::= AxisName '::' | '@'?
    fn parse_axis_specifier(&mut self) -> Axis {
        // Check for @ (attribute axis shorthand)
        if matches!(self.current(), Token::At) {
            self.advance();
            return Axis::Attribute;
        }

        // Check for axis keyword followed by ::
        // We check if the NEXT token is DoubleColon to decide whether this
        // is an axis specifier or just a name being used as a node test.
        let is_axis = match self.current() {
            Token::Ancestor
            | Token::AncestorOrSelf
            | Token::Attribute
            | Token::Child
            | Token::Descendant
            | Token::DescendantOrSelf
            | Token::Following
            | Token::FollowingSibling
            | Token::Namespace
            | Token::Parent
            | Token::Preceding
            | Token::PrecedingSibling
            | Token::Self_ => matches!(self.peek(), Token::DoubleColon),
            _ => false,
        };

        if is_axis {
            let axis = match self.current() {
                Token::Ancestor => Axis::Ancestor,
                Token::AncestorOrSelf => Axis::AncestorOrSelf,
                Token::Attribute => Axis::Attribute,
                Token::Child => Axis::Child,
                Token::Descendant => Axis::Descendant,
                Token::DescendantOrSelf => Axis::DescendantOrSelf,
                Token::Following => Axis::Following,
                Token::FollowingSibling => Axis::FollowingSibling,
                Token::Namespace => Axis::Namespace,
                Token::Parent => Axis::Parent,
                Token::Preceding => Axis::Preceding,
                Token::PrecedingSibling => Axis::PrecedingSibling,
                Token::Self_ => Axis::Self_,
                _ => unreachable!(),
            };
            self.advance(); // consume axis keyword
            self.advance(); // consume ::
            return axis;
        }

        // Default axis is "child" for everything except attribute
        Axis::Child
    }

    /// Convert a keyword token back to its string name for use as a node test.
    fn token_to_name(&self, token: &Token) -> Option<String> {
        match token {
            Token::Name(ref s) => Some(s.clone()),
            Token::Div => Some("div".to_string()),
            Token::Mod => Some("mod".to_string()),
            Token::And => Some("and".to_string()),
            Token::Or => Some("or".to_string()),
            Token::Ancestor => Some("ancestor".to_string()),
            Token::AncestorOrSelf => Some("ancestor-or-self".to_string()),
            Token::Attribute => Some("attribute".to_string()),
            Token::Child => Some("child".to_string()),
            Token::Descendant => Some("descendant".to_string()),
            Token::DescendantOrSelf => Some("descendant-or-self".to_string()),
            Token::Following => Some("following".to_string()),
            Token::FollowingSibling => Some("following-sibling".to_string()),
            Token::Namespace => Some("namespace".to_string()),
            Token::Parent => Some("parent".to_string()),
            Token::Preceding => Some("preceding".to_string()),
            Token::PrecedingSibling => Some("preceding-sibling".to_string()),
            Token::Self_ => Some("self".to_string()),
            _ => None,
        }
    }

    /// NodeTest ::= NameTest | 'comment()' | 'text()' | 'processing-instruction()' | 'node()'
    /// NameTest ::= '*' | NCName ':' '*' | QName
    fn parse_node_test(&mut self) -> Result<NodeTest, ParseError> {
        // Try to get the current token as a potential name
        let name_opt = self.token_to_name(&self.current());

        match self.current() {
            Token::Star => {
                self.advance();
                Ok(NodeTest::NameTest(NameTest::Any))
            }
            _ if name_opt.is_some() => {
                let name = name_opt.unwrap();

                // Check for function-style node tests: node(), text(), comment(), processing-instruction()
                if matches!(self.peek(), Token::LParen) {
                    match name.as_str() {
                        "node" => {
                            self.advance();
                            self.advance();
                            self.advance(); // name, (, )
                            Ok(NodeTest::Node)
                        }
                        "text" => {
                            self.advance();
                            self.advance();
                            self.advance();
                            Ok(NodeTest::Text)
                        }
                        "comment" => {
                            self.advance();
                            self.advance();
                            self.advance();
                            Ok(NodeTest::Comment)
                        }
                        "processing-instruction" => {
                            self.advance(); // name
                            self.advance(); // (
                                            // Check for optional string argument
                            let target = if matches!(self.current(), Token::StringLiteral(_)) {
                                if let Token::StringLiteral(s) = self.current() {
                                    self.advance();
                                    Some(s)
                                } else {
                                    None
                                }
                            } else {
                                None
                            };
                            self.expect(&Token::RParen)?;
                            Ok(NodeTest::ProcessingInstruction(target))
                        }
                        _ => {
                            // Regular function call, not a node test
                            self.advance(); // function name
                            self.advance(); // (
                            let mut args = Vec::new();
                            if !matches!(self.current(), Token::RParen) {
                                args.push(self.parse_or_expr()?);
                                while matches!(self.current(), Token::Comma) {
                                    self.advance();
                                    args.push(self.parse_or_expr()?);
                                }
                            }
                            self.expect(&Token::RParen)?;
                            // Wrap in a step with a name test
                            Ok(NodeTest::NameTest(NameTest::LocalName(name)))
                        }
                    }
                } else {
                    self.advance();
                    // Check for prefix:*
                    if let Some(rest) = name.strip_suffix(":*") {
                        Ok(NodeTest::NsWildcard(rest.to_string()))
                    } else if let Some((prefix, local)) = name.split_once(':') {
                        Ok(NodeTest::NameTest(NameTest::QName {
                            prefix: prefix.to_string(),
                            local: local.to_string(),
                        }))
                    } else {
                        Ok(NodeTest::NameTest(NameTest::LocalName(name)))
                    }
                }
            }
            _ => Err(self.error(format!("Expected node test, got {}", self.current()))),
        }
    }

    /// FilterExpr ::= PrimaryExpr Predicate*
    fn parse_filter_expr(&mut self) -> Result<Expr, ParseError> {
        let primary = self.parse_primary_expr()?;

        // Predicates after primary
        let mut predicates = Vec::new();
        while matches!(self.current(), Token::LBracket) {
            self.advance(); // consume '['
            let pred = self.parse_or_expr()?;
            self.expect(&Token::RBracket)?;
            predicates.push(pred);
        }

        if predicates.is_empty() {
            Ok(primary)
        } else {
            Ok(Expr::Filter(Box::new(primary), predicates))
        }
    }

    /// PrimaryExpr ::= VariableReference | '(' Expr ')' | Literal | Number | FunctionCall
    fn parse_primary_expr(&mut self) -> Result<Expr, ParseError> {
        match self.current() {
            Token::Dollar => {
                self.advance();
                if let Token::Name(name) = self.current() {
                    let name = name.clone();
                    self.advance();
                    Ok(Expr::Variable(name))
                } else {
                    Err(self.error("Expected variable name after $".to_string()))
                }
            }
            Token::LParen => {
                self.advance();
                let expr = self.parse_or_expr()?;
                self.expect(&Token::RParen)?;
                Ok(expr)
            }
            Token::StringLiteral(ref s) => {
                let s = s.clone();
                self.advance();
                Ok(Expr::StringLiteral(s))
            }
            Token::NumberLiteral(n) => {
                self.advance();
                Ok(Expr::NumberLiteral(n))
            }
            Token::Name(ref name) => {
                let name = name.clone();
                if matches!(self.peek(), Token::LParen) {
                    // Function call
                    self.advance(); // function name
                    self.advance(); // (
                    let mut args = Vec::new();
                    if !matches!(self.current(), Token::RParen) {
                        args.push(self.parse_or_expr()?);
                        while matches!(self.current(), Token::Comma) {
                            self.advance();
                            args.push(self.parse_or_expr()?);
                        }
                    }
                    self.expect(&Token::RParen)?;
                    Ok(Expr::FunctionCall { name, args })
                } else {
                    // Standalone name - could be a step or something else
                    // But at the primary level, this shouldn't happen
                    Err(self.error(format!("Unexpected name '{}' in primary expression", name)))
                }
            }
            _ => Err(self.error(format!(
                "Expected primary expression, got {}",
                self.current()
            ))),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Convenience function
// ═══════════════════════════════════════════════════════════════════════════════

/// Parse an XPath expression string into an AST.
pub fn parse_xpath(input: &str) -> Result<Expr, ParseError> {
    let mut lexer = crate::xml::xpath::lexer::Lexer::new(input);
    let mut tokens = Vec::new();
    loop {
        let tok = lexer.next_token();
        let is_eof = matches!(tok, Token::Eof);
        tokens.push(tok);
        if is_eof {
            break;
        }
    }
    let starts = lexer.token_starts();
    let mut parser = Parser::new(tokens);
    parser.parse().map_err(|e| {
        // UPSTREAM-PARITY (xpath.c xmlXPathErrFmt): the recorded error
        // position is the BYTE offset into the expression (`int1 =
        // ctxt->cur - ctxt->base`), not the token index — it drives the
        // caret of the "XPath error : Invalid expression" diagnostic
        // (HOSTILE-FAILURE F3).
        let byte_off = starts
            .get(e.pos)
            .copied()
            .unwrap_or(input.len())
            .min(input.len());
        ParseError {
            message: e.message,
            pos: byte_off,
        }
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_name() {
        let expr = parse_xpath("para").unwrap();
        assert!(matches!(expr, Expr::Step(_)));
    }

    #[test]
    fn test_parse_absolute_path() {
        let expr = parse_xpath("/child::para").unwrap();
        assert!(matches!(expr, Expr::AbsolutePath(_)));
    }

    #[test]
    fn test_parse_attribute() {
        let expr = parse_xpath("@attr").unwrap();
        assert!(matches!(expr, Expr::Step(_)));
    }

    #[test]
    fn test_parse_predicate() {
        let expr = parse_xpath("para[1]").unwrap();
        assert!(matches!(expr, Expr::Step(_)));
    }

    #[test]
    fn test_parse_function_call() {
        let expr = parse_xpath("position()").unwrap();
        assert!(matches!(expr, Expr::FunctionCall { .. }));
    }

    #[test]
    fn test_parse_binary_op() {
        let expr = parse_xpath("a = b").unwrap();
        assert!(matches!(
            expr,
            Expr::BinaryOp {
                op: BinaryOp::Eq,
                ..
            }
        ));
    }

    #[test]
    fn test_parse_union() {
        let expr = parse_xpath("a | b").unwrap();
        assert!(matches!(expr, Expr::Union(_, _)));
    }

    #[test]
    fn test_parse_variable() {
        let expr = parse_xpath("$var").unwrap();
        assert!(matches!(expr, Expr::Variable(_)));
    }

    #[test]
    fn test_parse_string_literal() {
        let expr = parse_xpath("'hello'").unwrap();
        assert_eq!(expr, Expr::StringLiteral("hello".to_string()));
    }

    #[test]
    fn test_parse_number() {
        let expr = parse_xpath("42").unwrap();
        assert_eq!(expr, Expr::NumberLiteral(42.0));
    }

    #[test]
    fn test_parse_nested_expression() {
        let expr = parse_xpath("(1 + 2) * 3").unwrap();
        assert!(matches!(
            expr,
            Expr::BinaryOp {
                op: BinaryOp::Mul,
                ..
            }
        ));
    }

    #[test]
    fn test_parse_chained_path() {
        let expr = parse_xpath("a/b/c").unwrap();
        assert!(matches!(expr, Expr::RelativePath(_, _)));
    }

    #[test]
    fn test_parse_double_slash() {
        let expr = parse_xpath("//para").unwrap();
        assert!(matches!(expr, Expr::AbsolutePath(_)));
    }

    #[test]
    fn test_parse_dot() {
        let expr = parse_xpath(".").unwrap();
        assert!(matches!(expr, Expr::Step(_)));
    }

    #[test]
    fn test_parse_dot_dot() {
        let expr = parse_xpath("..").unwrap();
        assert!(matches!(expr, Expr::Step(_)));
    }

    #[test]
    fn test_parse_unary_minus() {
        let expr = parse_xpath("-5").unwrap();
        assert!(matches!(expr, Expr::UnaryMinus(_)));
    }

    #[test]
    fn test_parse_double_unary_minus() {
        let expr = parse_xpath("--5").unwrap();
        // Should cancel out
        assert!(!matches!(expr, Expr::UnaryMinus(_)));
    }

    #[test]
    fn test_parse_complex_expression() {
        let expr = parse_xpath("/html/body//div[@class='main']/p[1]").unwrap();
        assert!(matches!(expr, Expr::AbsolutePath(_)));
    }

    #[test]
    fn test_parse_error() {
        let result = parse_xpath("(");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_empty() {
        let result = parse_xpath("");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_and_or() {
        let expr = parse_xpath("a = 1 and b = 2 or c = 3").unwrap();
        assert!(matches!(expr, Expr::BinaryOp { .. }));
    }

    #[test]
    fn test_parse_comparison_chain() {
        let expr = parse_xpath("a < b <= c > d >= e").unwrap();
        // Should parse as: ((((a < b) <= c) > d) >= e)
        assert!(matches!(
            expr,
            Expr::BinaryOp {
                op: BinaryOp::Ge,
                ..
            }
        ));
    }

    #[test]
    fn test_parse_arithmetic() {
        let expr = parse_xpath("1 + 2 * 3").unwrap();
        // 2 * 3 should bind tighter: 1 + (2 * 3)
        match expr {
            Expr::BinaryOp {
                op: BinaryOp::Add,
                left,
                right,
            } => {
                assert!(matches!(*left, Expr::NumberLiteral(1.0)));
                assert!(matches!(
                    *right,
                    Expr::BinaryOp {
                        op: BinaryOp::Mul,
                        ..
                    }
                ));
            }
            _ => panic!("Expected Add expression"),
        }
    }

    #[test]
    fn test_parse_filter_path() {
        let expr = parse_xpath("//div/span").unwrap();
        assert!(matches!(expr, Expr::AbsolutePath(_)));
    }

    #[test]
    fn test_parse_node_test_functions() {
        let expr = parse_xpath("child::node()").unwrap();
        assert!(matches!(expr, Expr::Step(_)));
        if let Expr::Step(step) = expr {
            assert_eq!(step.node_test, NodeTest::Node);
        }
    }
}
