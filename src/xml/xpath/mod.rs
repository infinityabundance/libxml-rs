//! XPath 1.0 implementation (§25, §85 Phase 5).
//!
//! Complete XPath 1.0 engine: location paths, axes, node tests, predicates,
//! functions, variables, namespaces, node sets, boolean/number/string
//! conversion, comparison semantics, NaN/infinity/negative zero, document
//! order, context position/size, extension functions, compiled expressions.
//!
//! # Modules
//!
//! - `ast` — expression AST types (axes, node tests, steps, operators)
//! - `lexer` — tokenizer
//! - `parser` — recursive descent parser
//! - `types` — XPath value types (node sets, numbers, strings, booleans)
//! - `context` — evaluation context (variables, namespaces, functions)
//! - `axes` — axis traversal
//! - `functions` — core function library (25 XPath 1.0 functions)
//! - `eval` — evaluation engine
//!
//! # Upstream contract
//!
//! Mirrors upstream `xpath.c` / `xpathInternals.h`
//! (`SRC-LIBXML2-2.15.0-XPATH-C`, parity target libxml2 2.15.3 oracle):
//! expression compilation, the 13 axes, node tests, predicates, the core
//! function library, value stack semantics and the `xmlXPathObject` /
//! `xmlXPathCompExpr` / `xmlXPathContext` C types (R-000126 restored the
//! full xpathInternals.h header surface; R-000128 fixed the opLimit/
//! opCount field types).
//!
//! # Conceptual behavior
//!
//! The engine implements XPath 1.0 (W3C-XPATH-1.0): the lexer/parser build
//! an AST, `eval` walks it with document-order node-set semantics,
//! IEEE 754 number handling (NaN/infinity/negative zero), boolean/string
//! conversion per §4, context position/size, and the extension-function
//! registry bridged to C callbacks (R-000162).
//!
//! # Ownership & safety invariants
//!
//! `xmlXPathCompile` results are freed with `xmlXPathFreeCompExpr`; XPath
//! objects with `xmlXPathFreeObject` (object owns its node-set/string/
//! number storage). Node-set entries are borrowed tree pointers — the tree
//! outlives evaluation (types.rs `XPathNode` SAFETY note). The parser
//! context bridge owns the value stack and the popped-string copy
//! (R-000169 fixed xml_strdup on a non-NUL-terminated Rust String).
//!
//! # Historical quirks & epochs
//!
//! R-000102 (absolute paths evaluate from the document root node),
//! R-000159 (predicate position() semantics) and R-000166 (number
//! formatting, 1e9/1e-5 threshold, DBL_DIG=15) are upstream behaviors
//! locked by residuals. The node-set dump became newline-separated in the
//! 2.9.10 epoch (E-001, commit da35eeae, an upstream-documented breaking
//! change); the empty-node-set exit-code epochs (E-003) sit at the
//! xmllint layer this engine feeds.
//!
//! # Deliberate oddities
//!
//! `xmlXPathCastNumberToString` trailing-zero trimming, the integer
//! shortcut and the `e+NN`/`e-NN` exponent form are reproduced exactly
//! (R-000166) rather than delegating to Rust float formatting, which
//! differs on exponent width and trimming.
//!
//! # Proving courts
//!
//! XPATH / XPOINTER / XINCLUDE court families, XPATH-001 differential
//! probes (courts/suites/data-abi/*), the 967/967 number() corpus and
//! CLI-XSLTPROC-0014/0015/0017 (format-number) require byte-identical
//! output vs the oracle; cargo test runs the engine suites.
//!
//! # Tempting simplifications that would break parity
//!
//! Do not swap number formatting to `format!("{}")`: the 1e9/1e-5
//! scientific threshold, 15-digit fraction computation and exponent
//! padding are oracle-observable (R-000166). Do not deduplicate node-sets
//! by value instead of pointer, and do not drop the borrowed-node
//! invariant — the C ABI hands out raw node pointers.

pub mod ast;
pub mod axes;
pub mod context;
pub mod eval;
pub mod exports;
pub mod functions;
pub mod lexer;
pub mod parser;
pub mod parser_context;
pub mod types;

use ast::CompiledExpr;
use context::XPathContext;
use parser::parse_xpath;
use types::XPathValue;

/// Parse and compile an XPath expression string.
///
/// Returns `None` on parse error.
pub fn compile(expr_str: &str) -> Option<CompiledExpr> {
    compile_result(expr_str).ok()
}

/// Parse and compile an XPath expression string, exposing the parse error
/// (message + byte offset) for upstream-faithful diagnostics
/// (HOSTILE-FAILURE F3).
pub fn compile_result(
    expr_str: &str,
) -> Result<CompiledExpr, crate::xml::xpath::parser::ParseError> {
    parse_xpath(expr_str).map(|expr| CompiledExpr::new(expr_str.to_string(), expr))
}

/// Evaluate a compiled XPath expression.
///
/// Returns `None` on evaluation error; the error message is recorded on the
/// context (`XPathContext::error`) so callers can surface it exactly as
/// upstream does ("XPath error : ...").
pub fn evaluate(compiled: &CompiledExpr, context: &mut XPathContext) -> Option<XPathValue> {
    match eval::eval(context, &compiled.expr) {
        Ok(value) => Some(value),
        Err(msg) => {
            context.set_error(&msg);
            None
        }
    }
}

/// Parse and evaluate in one step.
pub fn evaluate_str(expr_str: &str, context: &mut XPathContext) -> Option<XPathValue> {
    compile(expr_str).and_then(|compiled| evaluate(&compiled, context))
}
