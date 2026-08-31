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
//!
//! # Upstream contract
//!
//! Mirrors the compiled-expression tree of upstream `xpath.c`
//! (`SRC-LIBXML2-2.15.0-XPATH-C`, parity target libxml2 2.15.3 oracle):
//! where upstream lowers an expression to an `xmlXPathCompExpr` op tree
//! (the XPATH_OP_* nodes), this module is the internal Rust AST that the
//! C ABI surface keeps behind a compiled-expression registry
//! (exports.rs `xmlXPathCtxtCompile` / `xmlXPathCompiledEval`).
//!
//! # Conceptual behavior
//!
//! The parser builds this AST; the evaluator walks it. The model covers
//! the full XPath 1.0 grammar: location paths (relative/absolute, steps,
//! axes, node tests, predicates), operators, function calls, variables,
//! literals and filter expressions — including the union type and the
//! step-level attribute/namespace flags that mirror the axis semantics.
//!
//! # Ownership & safety invariants
//!
//! AST nodes are owned by `CompiledExpr` (a single owning tree, no shared
//! subnodes); the evaluator borrows it. No raw pointers cross the AST
//! boundary — node-sets hold borrowed `_xmlNode` pointers defined in
//! types.rs, so the AST itself is Send-safe for concurrent compilation.
//!
//! # Historical quirks & epochs
//!
//! R-000105: node tests like `node()` / `text()` must parse as node
//! tests, not as function calls — a parser-epoch bug fixed during the
//! XPath closure. The step model matches the 2.15.3 oracle, which is the
//! E-001 epoch for node-set output semantics.
//!
//! # Deliberate oddities
//!
//! The internal AST deliberately does NOT reproduce the XPATH_OP_* byte
//! layout of upstream `xmlXPathCompExpr`: the op tree is opaque to C
//! callers, so the divergence is invisible at the ABI and only the
//! observable evaluation semantics must match.
//!
//! # Proving courts
//!
//! XPATH / XPOINTER / XINCLUDE court families exercise compiled
//! expressions end-to-end (byte-identical against the oracle DSO); cargo
//! test covers the AST round-trip unit suites.
//!
//! # Tempting simplifications that would break parity
//!
//! Do not flatten steps/predicates into a linear list: predicate
//! evaluation order and context position/size depend on the step tree.
//! Do not share subnodes (e.g. via Rc): expression ownership is exclusive
//! and the compiled-expression registry frees whole trees.

use std::fmt;

// ═══════════════════════════════════════════════════════════════════════════════
// Axes
// ═══════════════════════════════════════════════════════════════════════════════

/// XPath 1.0 axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Axis {
    /// `ancestor` — all ancestors of the context node (parent, grandparent, …)
    Ancestor,
    /// `ancestor-or-self` — all ancestors plus the context node itself
    AncestorOrSelf,
    /// `attribute` — the attributes of the context node
    Attribute,
    /// `child` — the children of the context node
    Child,
    /// `descendant` — all descendants of the context node (children, grandchildren, …)
    Descendant,
    /// `descendant-or-self` — all descendants plus the context node itself
    DescendantOrSelf,
    /// `following` — every node after the context node in document order, excluding descendants
    Following,
    /// `following-sibling` — the siblings that follow the context node
    FollowingSibling,
    /// `namespace` — the namespace nodes of the context node
    Namespace,
    /// `parent` — the parent of the context node (at most one node)
    Parent,
    /// `preceding` — every node before the context node in document order, excluding ancestors
    Preceding,
    /// `preceding-sibling` — the siblings that precede the context node
    PrecedingSibling,
    /// `self` — the context node itself
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

    /// Return the axis name as written in an XPath expression
    /// (e.g. `"ancestor-or-self"`).
    pub const fn as_str(&self) -> &'static str {
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
    QName {
        /// Namespace prefix, e.g. `xslt` in `xslt:template`
        prefix: String,
        /// Local part, e.g. `template` in `xslt:template`
        local: String,
    },
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

/// A single location step: axis::node-test\[predicates\]
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    /// Axis the step traverses (defaults to `child` for abbreviated steps)
    pub axis: Axis,
    /// Node test selecting which nodes the step matches
    pub node_test: NodeTest,
    /// Predicates filtering the selected nodes, applied left to right
    pub predicates: Vec<Expr>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Binary Operators
// ═══════════════════════════════════════════════════════════════════════════════

/// XPath 1.0 binary operator.
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
    FunctionCall {
        /// Function name (a QName, possibly with a namespace prefix)
        name: String,
        /// Argument expressions, evaluated in order
        args: Vec<Expr>,
    },
    /// Binary operation: `left op right`
    BinaryOp {
        /// The operator applied to the two operands
        op: BinaryOp,
        /// Left operand
        left: Box<Expr>,
        /// Right operand
        right: Box<Expr>,
    },
    /// Unary minus: `-expr`
    UnaryMinus(Box<Expr>),
    /// Union expression: `left | right`
    Union(Box<Expr>, Box<Expr>),
}

impl Expr {
    /// Check if this expression is a location path (returns nodeset).
    pub const fn is_location_path(&self) -> bool {
        matches!(
            self,
            Expr::AbsolutePath(_) | Expr::RelativePath(_, _) | Expr::Step(_) | Expr::Filter(_, _)
        )
    }

    /// Check if this is a constant value (no evaluation needed).
    pub const fn is_constant(&self) -> bool {
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
    /// The original XPath expression source text
    pub original: String,
    /// The parsed expression tree
    pub expr: Expr,
}

impl CompiledExpr {
    /// Create a compiled expression from its source text and parsed AST.
    pub const fn new(original: String, expr: Expr) -> Self {
        Self { original, expr }
    }
}
