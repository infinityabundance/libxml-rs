//! EXSLT Common (exsl:) — exsl:node-set, exsl:object-type (§35).
//!
//! # UPSTREAM-PARITY
//!
//! Upstream libxslt (libexslt/common.c) implements:
//!
//! - `exsl:node-set(object)` — converts a result tree fragment (RTF) to a
//!   node-set containing the RTF's root node. Also converts node-sets,
//!   strings, numbers and booleans: node-sets pass through unchanged, and
//!   atomic values are wrapped in a document containing a single text node
//!   (upstream wraps non-node-set values in an `xmlDoc` with a text node
//!   containing the string value).
//! - `exsl:object-type(object)` — returns the XPath type of the argument as
//!   one of: `string`, `number`, `boolean`, `node-set`, `RTF`, or `external`.
//!
//! Both are also registered as extension *elements* in the `exsl:` namespace
//! by upstream; the element forms are only meaningful inside `<exsl:document>`
//! context and are treated as no-ops outside it.

use super::{register, ExsltFunction};
use crate::xml::xpath::context::XPathContext;
use crate::xml::xpath::types::{node_string_value, NodeSet, XPathValue};

/// exsl:node-set(object) — convert an RTF (or other value) to a node-set.
fn node_set_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    let arg = match args.first() {
        Some(a) => a,
        None => return Ok(XPathValue::NodeSet(NodeSet::new())),
    };
    match arg {
        // Already a node-set: pass through unchanged (upstream copies).
        XPathValue::NodeSet(ns) => Ok(XPathValue::NodeSet(ns.clone())),
        // Atomic values: wrap in a synthetic document whose single text
        // node carries the string value. Upstream (exsltNodeSetFunction)
        // creates an xmlDoc with one text child for non-node-set inputs.
        XPathValue::String(s) => Ok(make_text_wrapper(s)),
        XPathValue::Number(n) => Ok(make_text_wrapper(
            &crate::xml::xpath::types::number_to_string(*n),
        )),
        XPathValue::Boolean(b) => Ok(make_text_wrapper(if *b { "true" } else { "false" })),
    }
}

/// Build a node-set containing a synthetic text node with the given content.
fn make_text_wrapper(s: &str) -> XPathValue {
    let node = unsafe {
        crate::xml::tree::new_text(cstr(s).as_ptr() as *const crate::abi::types::xmlChar)
    };
    let mut ns = NodeSet::new();
    if !node.is_null() {
        ns.push(node);
    }
    let _ = s;
    XPathValue::NodeSet(ns)
}

/// Create a NUL-terminated byte buffer for a string.
fn cstr(s: &str) -> Vec<u8> {
    let mut v = s.as_bytes().to_vec();
    v.push(0);
    v
}

/// exsl:object-type(object) — report the XPath type of the argument.
fn object_type_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    let t = match args.first() {
        None => "string", // Upstream returns "string" for a missing argument
        Some(XPathValue::NodeSet(_)) => "node-set",
        Some(XPathValue::String(_)) => "string",
        Some(XPathValue::Number(_)) => "number",
        Some(XPathValue::Boolean(_)) => "boolean",
    };
    Ok(XPathValue::String(t.to_string()))
}

/// Register all `exsl:` Common functions.
pub fn register_all() {
    register("exsl:node-set", node_set_fn as ExsltFunction);
    register("exsl:object-type", object_type_fn as ExsltFunction);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml::xpath::context::XPathContext;
    use crate::xml::xpath::types::XPathValue;

    fn ctx() -> XPathContext {
        XPathContext::new(std::ptr::null_mut())
    }

    #[test]
    fn test_node_set_node_set_passthrough() {
        let mut ns = NodeSet::new();
        let n = unsafe {
            crate::xml::tree::new_text(b"x\0".as_ptr() as *const crate::abi::types::xmlChar)
        };
        ns.push(n);
        let mut c = ctx();
        let r = node_set_fn(&mut c, &[XPathValue::NodeSet(ns.clone())]).unwrap();
        match r {
            XPathValue::NodeSet(out) => assert_eq!(out.len(), 1),
            _ => panic!("expected node-set"),
        }
        unsafe { crate::xml::tree::free_node(n) };
    }

    #[test]
    fn test_node_set_string_wraps() {
        let mut c = ctx();
        let r = node_set_fn(&mut c, &[XPathValue::String("abc".to_string())]).unwrap();
        match r {
            XPathValue::NodeSet(ns) => {
                assert_eq!(ns.len(), 1);
                let first = ns.first().unwrap();
                let s = node_string_value(first);
                assert_eq!(s, "abc");
                unsafe { crate::xml::tree::free_node(first) };
            }
            _ => panic!("expected node-set"),
        }
    }

    #[test]
    fn test_object_type() {
        let mut c = ctx();
        let r = object_type_fn(&mut c, &[XPathValue::Number(1.0)]).unwrap();
        assert_eq!(r.as_string(), "number");
        let r = object_type_fn(&mut c, &[XPathValue::Boolean(true)]).unwrap();
        assert_eq!(r.as_string(), "boolean");
        let r = object_type_fn(&mut c, &[XPathValue::String("s".to_string())]).unwrap();
        assert_eq!(r.as_string(), "string");
        let r = object_type_fn(&mut c, &[XPathValue::NodeSet(NodeSet::new())]).unwrap();
        assert_eq!(r.as_string(), "node-set");
    }

    #[test]
    fn test_node_set_no_args() {
        let mut c = ctx();
        let r = node_set_fn(&mut c, &[]).unwrap();
        assert!(matches!(r, XPathValue::NodeSet(_)));
    }
}
