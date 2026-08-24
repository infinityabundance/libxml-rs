//! XPath 1.0 Expression AST (§25).
//!
//! Internal Rust representation of XPath expressions, separate from the C ABI.
//! The parser builds this AST; the evaluator walks it.
//!
//! # UPSTREAM-PARITY
//!
//! Covers the full XPath 1.0 grammar:
//! - Location paths (relative/absolute, steps, axes, node tests, predicates)
//! - Operators (union, comparison, boolean, arithmetic)
//! - Functions, variables, literals
//! - Filter expressions (primary with predicates)

use std::fmt;

// ═══════════════════════════════════════════════════════════════════════════════
// Axes
// ═══════════════════════════════════════════════════════════════════════════════

/// XPath 1.0 axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Axis {
    Ancestor,
    AncestorOrSelf,
    Attribute,
    Child,
    Descendant,
    DescendantOrSelf,
    Following,
    FollowingSibling,
    Namespace,
    Parent,
    Preceding,
    PrecedingSibling,
    Self_,
}

impl Axis {
    /// All 13 XPath 1.0 axes.
    pub const ALL: &'static [Axis] = &[
        Axis::Ancestor,
        Axis::AncestorOrSelf,
        Axis::Attribute,
        Axis::Child,
        Axis::Descendant,
        Axis::DescendantOrSelf,
        Axis::Following,
        Axis::FollowingSibling,
        Axis::Namespace,
        Axis::Parent,
        Axis::Preceding,
        Axis::PrecedingSibling,
        Axis::Self_,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            Axis::Ancestor => "ancestor",
            Axis::AncestorOrSelf => "ancestor-or-self",
            Axis::Attribute => "attribute",
            Axis::Child => "child",
            Axis::Descendant => "descendant",
            Axis::DescendantOrSelf => "descendant-or-self",
            Axis::Following => "following",
            Axis::FollowingSibling => "following-sibling",
            Axis::Namespace => "namespace",
            Axis::Parent => "parent",
            Axis::Preceding => "preceding",
            Axis::PrecedingSibling => "preceding-sibling",
            Axis::Self_ => "self",
        }
    }
}

impl fmt::Display for Axis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Node Tests
// ═══════════════════════════════════════════════════════════════════════════════

/// XPath node test.
#[derive(Debug, Clone, PartialEq)]
pub enum NodeTest {
    /// name() — matches any node of principal node type
    NameTest(NameTest),
    /// comment()
    Comment,
    /// text()
    Text,
    /// processing-instruction() or processing-instruction("target")
    ProcessingInstruction(Option<String>),
    /// node()
    Node,
    /// * — matches any node of principal node type
    Wildcard,
    /// prefix:* — namespace wildcard
    NsWildcard(String),
}

/// A name test (QName or wildcard).
#[derive(Debug, Clone, PartialEq)]
pub enum NameTest {
    /// Just a local name: "para"
    LocalName(String),
    /// Qualified name: "xslt:template"
    QName { prefix: String, local: String },
    /// Wildcard name: *
    Any,
}

impl fmt::Display for NodeTest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NodeTest::NameTest(n) => match n {
                NameTest::LocalName(s) => write!(f, "{}", s),
                NameTest::QName { prefix, local } => write!(f, "{}:{}", prefix, local),
                NameTest::Any => write!(f, "*"),
            },
            NodeTest::Comment => write!(f, "comment()"),
            NodeTest::Text => write!(f, "text()"),
            NodeTest::ProcessingInstruction(None) => write!(f, "processing-instruction()"),
            NodeTest::ProcessingInstruction(Some(t)) => {
                write!(f, "processing-instruction('{}')", t)
            }
            NodeTest::Node => write!(f, "node()"),
            NodeTest::Wildcard => write!(f, "*"),
            NodeTest::NsWildcard(prefix) => write!(f, "{}:*", prefix),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step
// ═══════════════════════════════════════════════════════════════════════════════

/// A single location step: axis::node-test[predicates]
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub axis: Axis,
    pub node_test: NodeTest,
    pub predicates: Vec<Expr>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Binary Operators
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinaryOp {
    /// `|` — union
    Union,
    /// `and`
    And,
    /// `or`
    Or,
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
    /// `+`
    Add,
    /// `-`
    Sub,
    /// `*` (multiplication)
    Mul,
    /// `div`
    Div,
    /// `mod`
    Mod,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Expressions
// ═══════════════════════════════════════════════════════════════════════════════

/// XPath expression AST node.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// Absolute location path: `/expr`
    AbsolutePath(Box<Expr>),
    /// Relative location path: `step1/step2`
    RelativePath(Box<Expr>, Box<Expr>),
    /// A single step
    Step(Step),
    /// Filter expression: `primary[pred1][pred2]`
    Filter(Box<Expr>, Vec<Expr>),
    /// Variable reference: `$name`
    Variable(String),
    /// String literal: `'hello'`
    StringLiteral(String),
    /// Numeric literal: `42` or `3.14`
    NumberLiteral(f64),
    /// Boolean literal
    BooleanLiteral(bool),
    /// Function call: `name(arg1, arg2)`
    FunctionCall { name: String, args: Vec<Expr> },
    /// Binary operation: `left op right`
    BinaryOp {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    /// Unary minus: `-expr`
    UnaryMinus(Box<Expr>),
    /// Union expression: `left | right`
    Union(Box<Expr>, Box<Expr>),
}

impl Expr {
    /// Check if this expression is a location path (returns nodeset).
    pub fn is_location_path(&self) -> bool {
        matches!(
            self,
            Expr::AbsolutePath(_) | Expr::RelativePath(_, _) | Expr::Step(_) | Expr::Filter(_, _)
        )
    }

    /// Check if this is a constant value (no evaluation needed).
    pub fn is_constant(&self) -> bool {
        matches!(
            self,
            Expr::StringLiteral(_) | Expr::NumberLiteral(_) | Expr::BooleanLiteral(_)
        )
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Compiled Expression
// ═══════════════════════════════════════════════════════════════════════════════

/// A compiled XPath expression.
///
/// Internal representation, not the C ABI `xmlXPathCompExprPtr`.
#[derive(Debug, Clone)]
pub struct CompiledExpr {
    pub original: String,
    pub expr: Expr,
}

impl CompiledExpr {
    pub fn new(original: String, expr: Expr) -> Self {
        Self { original, expr }
    }
}
