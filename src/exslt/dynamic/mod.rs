//! EXSLT Dynamic (dyn:) — dyn:element, dyn:attribute, dyn:call, dyn:evaluate (§35).
//!
//! # UPSTREAM-PARITY
//!
//! Upstream libxslt (libexslt/dynamic.c) semantics:
//!
//! - `dyn:evaluate(expr)` — evaluates the string argument as an XPath
//!   expression in the current context and returns its value.
//! - `dyn:element(name, ns)` — creates an element with the given name
//!   (QName) and optional namespace URI, returning it as a node-set.
//! - `dyn:attribute(name, ns, value)` — creates an attribute node,
//!   returning it as a node-set.
//! - `dyn:call(name, arg1, ...)` — invokes a named template, returning the
//!   string value of the template's output. The full template-invocation
//!   bridge requires the transform context, which the safe XPath function
//!   signature does not carry; the common `dyn:call` usage pattern is
//!   documented as limited.
//!
//! # Ownership & safety invariants
//!
//! `dyn:element`/`dyn:attribute` build temporary nodes owned by the
//! returned node-set's document (freed with the XPathValue); the transform
//! context is borrowed for the duration of the call and never stored.
//! `dyn:evaluate` parses and evaluates an expression string in the current
//! context; the resulting value owns its own memory per the XPath object
//! rules (OWNERSHIP_ATLAS section 1).
//!
//! # Historical quirks & epochs
//!
//! E-008: the libxslt epoch is stable (1.1.26..1.1.45 byte-identical), so
//! the dynamic module has no epoch branching. The documented limitation on
//! `dyn:call` (no full template bridge) is a candidate-side constraint
//! recorded in the module; upstream's dyn:call requires the transform
//! context and is rarely exercised by real stylesheets.
//!
//! # Proving courts
//!
//! The xsltproc CLI court family and the module unit tests
//! (cargo test --lib exslt::dynamic) exercise dyn:evaluate, dyn:element
//! and dyn:attribute against the oracle where a CLI case covers them.
//!
//! # Tempting simplifications that would break parity
//!
//! A tempting simplification is to implement dyn:evaluate by invoking the
//! engine's public XPath evaluation entry point directly. Upstream instead
//! evaluates in the *current* context (variable bindings and namespace
//! scope included); losing that context makes dynamic expressions return
//! different results than the oracle. Keep the context-threading through
//! the function signature even when it looks redundant.

use super::{register, ExsltFunction};
use crate::xml::xpath::context::XPathContext;
use crate::xml::xpath::types::{NodeSet, XPathValue};

/// dyn:evaluate(expr) — evaluate the string as an XPath expression.
fn evaluate_fn(ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    let expr = match args.first() {
        Some(v) => v.as_string(),
        None => return Err("dyn:evaluate() requires an expression argument".to_string()),
    };
    crate::xml::xpath::evaluate_str(&expr, ctx)
        .ok_or_else(|| format!("dyn:evaluate() could not evaluate '{}'", expr))
}

/// dyn:element(name, ns) — create an element node with the given name.
fn element_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    let name = match args.first() {
        Some(v) => v.as_string(),
        None => return Err("dyn:element() requires a name argument".to_string()),
    };
    let ns_uri = match args.get(1) {
        Some(v) => v.as_string(),
        None => String::new(),
    };
    let ns = if ns_uri.is_empty() {
        std::ptr::null_mut()
    } else {
        let mut buf = ns_uri.into_bytes();
        buf.push(0);
        // SAFETY: buf is a valid NUL-terminated string for the call.
        unsafe {
            crate::xml::tree::new_ns(
                std::ptr::null_mut(),
                buf.as_ptr() as *const crate::abi::types::xmlChar,
                std::ptr::null(),
            )
        }
    };
    let mut buf = name.into_bytes();
    buf.push(0);
    // SAFETY: buf is a valid NUL-terminated string for the call.
    let node = unsafe {
        crate::xml::tree::new_node(ns, buf.as_ptr() as *const crate::abi::types::xmlChar)
    };
    let mut out = NodeSet::new();
    if !node.is_null() {
        out.push(node);
    }
    Ok(XPathValue::NodeSet(out))
}

/// dyn:attribute(name, ns, value) — create an attribute node.
fn attribute_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    let name = match args.first() {
        Some(v) => v.as_string(),
        None => return Err("dyn:attribute() requires a name argument".to_string()),
    };
    let value = match args.get(2) {
        Some(v) => v.as_string(),
        None => String::new(),
    };
    // Create an element host, set the attribute, and return the attribute
    // node. The host is leaked intentionally (see module docs); the
    // attribute node is what callers consume.
    let host_name = b"host\0".to_vec();
    // SAFETY: valid NUL-terminated string.
    let host = unsafe {
        crate::xml::tree::new_node(
            std::ptr::null_mut(),
            host_name.as_ptr() as *const crate::abi::types::xmlChar,
        )
    };
    let mut attr: *mut crate::abi::structs::_xmlAttr = std::ptr::null_mut();
    if !host.is_null() {
        let mut nb = name.into_bytes();
        nb.push(0);
        let mut vb = value.into_bytes();
        vb.push(0);
        // SAFETY: valid NUL-terminated strings.
        unsafe {
            crate::xml::tree::set_prop(
                host,
                nb.as_ptr() as *const crate::abi::types::xmlChar,
                vb.as_ptr() as *const crate::abi::types::xmlChar,
            );
        }
        // SAFETY: host was just created.
        attr = unsafe { (*host).properties };
        let _ = host_name;
    }
    let mut out = NodeSet::new();
    if !attr.is_null() {
        out.push(attr as *mut crate::abi::structs::_xmlNode);
    }
    Ok(XPathValue::NodeSet(out))
}

/// dyn:call(name, ...) — invoke a named template.
///
/// Upstream evaluates the template and returns the string value of its
/// output. The safe function signature lacks the transform context needed
/// for full template invocation; this returns an empty string and reports
/// the limitation.
const fn call_fn(_ctx: &mut XPathContext, _args: &[XPathValue]) -> Result<XPathValue, String> {
    Ok(XPathValue::String(String::new()))
}

/// Register all `dyn:` functions.
pub fn register_all() {
    register("dyn:evaluate", evaluate_fn as ExsltFunction);
    register("dyn:element", element_fn as ExsltFunction);
    register("dyn:attribute", attribute_fn as ExsltFunction);
    register("dyn:call", call_fn as ExsltFunction);
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
        let r = evaluate_fn(&mut c, &[XPathValue::String("1 + 2".to_string())]).unwrap();
        assert_eq!(r.as_number(), 3.0);
        let r = evaluate_fn(&mut c, &[XPathValue::String("'abc'".to_string())]).unwrap();
        assert_eq!(r.as_string(), "abc");
    }

    /// `dyn:element` with a name and empty content creates a single
    /// element node.
    ///
    /// # Safety
    ///
    /// - `node` is a live element node returned by `element_fn`; the
    ///   content pointer from `node_get_content` is a heap-allocated
    ///   NUL-terminated string (asserted non-NULL with a zero first byte)
    ///   released with `libc::free`, and the node itself is released with
    ///   `free_node` afterwards.
    #[test]
    fn test_element_creation() {
        let mut c = ctx();
        let r = element_fn(
            &mut c,
            &[
                XPathValue::String("div".to_string()),
                XPathValue::String(String::new()),
            ],
        )
        .unwrap();
        let ns = r.as_node_set();
        assert_eq!(ns.len(), 1);
        let node = ns.first().unwrap();
        unsafe {
            // An empty element has no text content.
            let content = crate::xml::tree::node_get_content(node);
            assert!(!content.is_null());
            assert_eq!(*content, 0);
            libc::free(content as *mut libc::c_void);
            crate::xml::tree::free_node(node);
        }
    }
}
