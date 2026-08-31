//! EXSLT Saxon extensions — `saxon:` (exslt.c / saxon.c 1.1.45).
//!
//! Namespace URI: `http://icl.com/saxon` (upstream `SAXON_NAMESPACE`).
//!
//! Functions:
//!
//! - `saxon:line-number([node-set])` — line number of the context node, or
//!   of the first node in document order of the given node-set; -1 when the
//!   node carries no line information.
//! - `saxon:systemId()` — the system ID (document URL) of the source
//!   document.
//! - `saxon:evaluate(string)` — evaluate the string as an XPath expression
//!   in the current context.
//! - `saxon:expression(string)` — return a stored expression (upstream
//!   compiles and caches it in a per-transform hash; the candidate stores
//!   the expression text behind a marker).
//! - `saxon:eval(stored-expression)` — evaluate a stored expression.
//!
//! # UPSTREAM-PARITY
//!
//! Upstream `saxon:eval` rejects non-stored arguments with `Invalid type`;
//! wrong arities report `Invalid number of arguments`. Both surface as
//! `XPath error : ...` and stop the transformation (exit 10), matching the
//! candidate's XPath error reporting.

use super::{register, ExsltFunction};
use crate::xml::xpath::context::XPathContext;
use crate::xml::xpath::types::XPathValue;
use std::ffi::c_char;
use std::ptr;

/// Marker prefix for a stored expression returned by `saxon:expression`.
/// A NUL byte cannot appear in an XML string, so the marker cannot collide
/// with a legitimate expression.
const STORED_EXPR_PREFIX: &str = "\u{0}exslt-saxon-expr:";

/// `saxon:line-number([node-set])`.
fn line_number_fn(ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    let node: *mut crate::abi::structs::_xmlNode = match args.len() {
        0 => ctx.context_node,
        1 => {
            let ns = match &args[0] {
                XPathValue::NodeSet(ns) => ns,
                _ => return Err("Invalid type".to_string()),
            };
            // First node in document order (upstream saxon.c picks the
            // minimum via xmlXPathCmpNodes).
            let mut best: *mut crate::abi::structs::_xmlNode = ptr::null_mut();
            for n in ns.iter() {
                if best.is_null() {
                    best = n;
                } else {
                    // SAFETY: both nodes are valid.
                    let c = unsafe { crate::abi::exports_xml2::xmlXPathCmpNodes(best, n) };
                    if c == -1 {
                        best = n;
                    }
                }
            }
            best
        }
        _ => return Err("Invalid number of arguments".to_string()),
    };
    if node.is_null() {
        return Ok(XPathValue::Number(-1.0));
    }
    // SAFETY: node is valid.
    let node_ref = unsafe { &*node };
    let mut cur = node;
    if node_ref.type_ == crate::abi::types::xmlElementType::XML_NAMESPACE_DECL as i32 {
        // The XPath module stores the owner element of a namespace node in
        // the ns->next field (upstream saxon.c).
        let ns = node as *mut crate::abi::structs::_xmlNs;
        // SAFETY: ns is valid.
        let owner = unsafe { (*ns).next } as *mut crate::abi::structs::_xmlNode;
        if owner.is_null()
            || unsafe { (*owner).type_ }
                != crate::abi::types::xmlElementType::XML_ELEMENT_NODE as i32
        {
            return Ok(XPathValue::Number(-1.0));
        }
        cur = owner;
    }
    // SAFETY: cur is valid.
    let line = unsafe { crate::abi::exports_xml2::xmlGetLineNo(cur) };
    Ok(XPathValue::Number(line as f64))
}

/// `saxon:systemId()`.
fn system_id_fn(ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    if !args.is_empty() {
        return Err("Invalid number of arguments".to_string());
    }
    let doc = ctx.document;
    let s = if doc.is_null() {
        String::new()
    } else {
        // SAFETY: doc is valid; URL may be NULL.
        let url = unsafe { (*doc).URL };
        if url.is_null() {
            String::new()
        } else {
            unsafe {
                std::ffi::CStr::from_ptr(url as *const c_char)
                    .to_string_lossy()
                    .into_owned()
            }
        }
    };
    Ok(XPathValue::String(s))
}

/// `saxon:evaluate(string)` — shorthand for
/// `saxon:eval(saxon:expression(string))`.
fn evaluate_fn(ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    if args.len() != 1 {
        return Err("Invalid number of arguments".to_string());
    }
    let expr = args[0].as_string();
    crate::xml::xpath::evaluate_str(&expr, ctx).ok_or_else(|| "Invalid expression".to_string())
}

/// `saxon:expression(string)` — store an XPath expression for later
/// evaluation with `saxon:eval`.
fn expression_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    if args.len() != 1 {
        return Err("Invalid number of arguments".to_string());
    }
    let expr = args[0].as_string();
    Ok(XPathValue::String(format!(
        "{}{}",
        STORED_EXPR_PREFIX, expr
    )))
}

/// `saxon:eval(stored-expression)` — evaluate a stored expression in the
/// current context. Any non-stored argument is rejected with `Invalid type`
/// (upstream `xmlXPathStackIsExternal` check).
fn eval_fn(ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    if args.len() != 1 {
        return Err("Invalid number of arguments".to_string());
    }
    let s = args[0].as_string();
    match s.strip_prefix(STORED_EXPR_PREFIX) {
        Some(expr) => crate::xml::xpath::evaluate_str(expr, ctx)
            .ok_or_else(|| "Invalid expression".to_string()),
        None => Err("Invalid type".to_string()),
    }
}

/// Register all `saxon:` functions.
pub fn register_all() {
    register("saxon:line-number", line_number_fn as ExsltFunction);
    register("saxon:systemId", system_id_fn as ExsltFunction);
    register("saxon:evaluate", evaluate_fn as ExsltFunction);
    register("saxon:expression", expression_fn as ExsltFunction);
    register("saxon:eval", eval_fn as ExsltFunction);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml::xpath::context::XPathContext;
    use core::ptr;

    fn ctx() -> XPathContext {
        XPathContext::new(ptr::null_mut())
    }

    #[test]
    fn test_evaluate() {
        let mut c = ctx();
        let r = evaluate_fn(&mut c, &[XPathValue::String("2 + 3".to_string())]).unwrap();
        assert_eq!(r.as_number(), 5.0);
    }

    #[test]
    fn test_expression_eval_roundtrip() {
        let mut c = ctx();
        let stored = expression_fn(&mut c, &[XPathValue::String("5 * 5".to_string())]).unwrap();
        let r = eval_fn(&mut c, &[stored]).unwrap();
        assert_eq!(r.as_number(), 25.0);
    }

    #[test]
    fn test_eval_rejects_plain_string() {
        let mut c = ctx();
        let r = eval_fn(&mut c, &[XPathValue::String("2 + 3".to_string())]);
        assert!(r.is_err());
        assert_eq!(r.unwrap_err(), "Invalid type");
    }

    #[test]
    fn test_arity_errors() {
        let mut c = ctx();
        assert_eq!(
            system_id_fn(&mut c, &[XPathValue::String("x".to_string())]).unwrap_err(),
            "Invalid number of arguments"
        );
        assert_eq!(
            line_number_fn(
                &mut c,
                &[
                    XPathValue::String("x".to_string()),
                    XPathValue::String("y".to_string())
                ]
            )
            .unwrap_err(),
            "Invalid number of arguments"
        );
    }

    #[test]
    fn test_line_number_negative_no_context() {
        let mut c = ctx();
        // No context node: -1.
        let r = line_number_fn(&mut c, &[]).unwrap();
        assert_eq!(r.as_number(), -1.0);
    }

    #[test]
    fn test_system_id_empty() {
        let mut c = ctx();
        let r = system_id_fn(&mut c, &[]).unwrap();
        assert_eq!(r.as_string(), "");
    }

    #[test]
    fn test_node_set_type_error() {
        let mut c = ctx();
        let r = line_number_fn(&mut c, &[XPathValue::Number(1.0)]);
        assert_eq!(r.unwrap_err(), "Invalid type");
    }
}
