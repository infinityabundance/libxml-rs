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

use ast::{CompiledExpr, Expr};
use context::XPathContext;
use parser::parse_xpath;
use types::XPathValue;

/// Parse and compile an XPath expression string.
///
/// Returns `None` on parse error.
pub fn compile(expr_str: &str) -> Option<CompiledExpr> {
    match parse_xpath(expr_str) {
        Ok(expr) => Some(CompiledExpr::new(expr_str.to_string(), expr)),
        Err(_) => None,
    }
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
