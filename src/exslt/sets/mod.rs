//! EXSLT Sets (set:) — set:difference, set:intersection, set:distinct,
//! set:has-same-node, set:leading, set:trailing (§35).
//!
//! # UPSTREAM-PARITY
//!
//! Upstream libxslt (libexslt/sets.c) semantics:
//!
//! - `set:difference(ns1, ns2)` — nodes in ns1 that are not in ns2,
//!   preserving ns1's document order.
//! - `set:intersection(ns1, ns2)` — nodes present in both node-sets.
//! - `set:distinct(ns)` — the first node for each distinct string value.
//! - `set:has-same-node(ns1, ns2)` — boolean: true if they share a node.
//! - `set:leading(ns1, ns2)` — nodes of ns1 that precede the first node of
//!   ns2 that is also in ns1 (document order).
//! - `set:trailing(ns1, ns2)` — nodes of ns1 that follow the last node of
//!   ns2 that is also in ns1.

use super::{register, ExsltFunction};
use crate::xml::xpath::context::XPathContext;
use crate::xml::xpath::types::{node_string_value, NodeSet, XPathValue};

/// set:difference(ns1, ns2) — nodes in ns1 not in ns2.
fn difference_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    let (ns1, ns2) = two_sets(args);
    let mut out = NodeSet::new();
    for n in ns1.iter() {
        if !ns2.contains(n) {
            out.push(n);
        }
    }
    Ok(XPathValue::NodeSet(out))
}

/// set:intersection(ns1, ns2) — nodes in both node-sets.
fn intersection_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    let (ns1, ns2) = two_sets(args);
    let mut out = NodeSet::new();
    for n in ns1.iter() {
        if ns2.contains(n) {
            out.push(n);
        }
    }
    Ok(XPathValue::NodeSet(out))
}

/// set:distinct(ns) — first node for each distinct string value.
fn distinct_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    let ns = one_set(args);
    let mut out = NodeSet::new();
    let mut seen: Vec<String> = Vec::new();
    for n in ns.iter() {
        let s = node_string_value(n);
        if !seen.contains(&s) {
            seen.push(s);
            out.push(n);
        }
    }
    Ok(XPathValue::NodeSet(out))
}

/// set:has-same-node(ns1, ns2) — true if the node-sets share a node.
fn has_same_node_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    let (ns1, ns2) = two_sets(args);
    for n in ns1.iter() {
        if ns2.contains(n) {
            return Ok(XPathValue::Boolean(true));
        }
    }
    Ok(XPathValue::Boolean(false))
}

/// set:leading(ns1, ns2) — nodes of ns1 before the first ns1-node in ns2.
fn leading_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    let (ns1, ns2) = two_sets(args);
    let mut out = NodeSet::new();
    for n in ns1.iter() {
        if ns2.contains(n) {
            break; // found the first shared node — stop
        }
        out.push(n);
    }
    Ok(XPathValue::NodeSet(out))
}

/// set:trailing(ns1, ns2) — nodes of ns1 after the last ns1-node in ns2.
fn trailing_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    let (ns1, ns2) = two_sets(args);
    // Find the index of the last ns1 node that appears in ns2.
    let mut last_shared: Option<usize> = None;
    let nodes: Vec<_> = ns1.iter().collect();
    for (i, n) in nodes.iter().enumerate() {
        if ns2.contains(*n) {
            last_shared = Some(i);
        }
    }
    let mut out = NodeSet::new();
    if let Some(i) = last_shared {
        for n in nodes.into_iter().skip(i + 1) {
            out.push(n);
        }
    } else {
        for n in nodes {
            out.push(n);
        }
    }
    Ok(XPathValue::NodeSet(out))
}

fn one_set(args: &[XPathValue]) -> NodeSet {
    match args.first() {
        Some(XPathValue::NodeSet(ns)) => ns.clone(),
        _ => NodeSet::new(),
    }
}

fn two_sets(args: &[XPathValue]) -> (NodeSet, NodeSet) {
    match (args.first(), args.get(1)) {
        (Some(XPathValue::NodeSet(a)), Some(XPathValue::NodeSet(b))) => (a.clone(), b.clone()),
        (Some(XPathValue::NodeSet(a)), _) => (a.clone(), NodeSet::new()),
        _ => (NodeSet::new(), NodeSet::new()),
    }
}

/// Register all `set:` functions.
pub fn register_all() {
    register("set:difference", difference_fn as ExsltFunction);
    register("set:intersection", intersection_fn as ExsltFunction);
    register("set:distinct", distinct_fn as ExsltFunction);
    register("set:has-same-node", has_same_node_fn as ExsltFunction);
    register("set:leading", leading_fn as ExsltFunction);
    register("set:trailing", trailing_fn as ExsltFunction);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml::xpath::context::XPathContext;
    use core::ptr;

    fn ctx() -> XPathContext {
        XPathContext::new(ptr::null_mut())
    }

    fn text_node(s: &str) -> *mut crate::abi::structs::_xmlNode {
        unsafe {
            crate::xml::tree::new_text(
                format!("{}\0", s).as_ptr() as *const crate::abi::types::xmlChar
            )
        }
    }

    #[test]
    fn test_difference() {
        let a = text_node("a");
        let b = text_node("b");
        let c = text_node("c");
        let mut ns1 = NodeSet::new();
        ns1.push(a);
        ns1.push(b);
        let mut ns2 = NodeSet::new();
        ns2.push(b);
        ns2.push(c);
        let mut x = ctx();
        let r = difference_fn(
            &mut x,
            &[XPathValue::NodeSet(ns1), XPathValue::NodeSet(ns2)],
        )
        .unwrap();
        let out = r.as_node_set();
        assert_eq!(out.len(), 1);
        assert_eq!(out.first(), Some(a));
        unsafe {
            crate::xml::tree::free_node(a);
            crate::xml::tree::free_node(b);
            crate::xml::tree::free_node(c);
        }
    }

    #[test]
    fn test_intersection() {
        let a = text_node("a");
        let b = text_node("b");
        let c = text_node("c");
        let mut ns1 = NodeSet::new();
        ns1.push(a);
        ns1.push(b);
        let mut ns2 = NodeSet::new();
        ns2.push(b);
        ns2.push(c);
        let mut x = ctx();
        let r = intersection_fn(
            &mut x,
            &[XPathValue::NodeSet(ns1), XPathValue::NodeSet(ns2)],
        )
        .unwrap();
        let out = r.as_node_set();
        assert_eq!(out.len(), 1);
        assert_eq!(out.first(), Some(b));
        unsafe {
            crate::xml::tree::free_node(a);
            crate::xml::tree::free_node(b);
            crate::xml::tree::free_node(c);
        }
    }

    #[test]
    fn test_distinct() {
        let a = text_node("x");
        let b = text_node("y");
        let c = text_node("x");
        let mut ns = NodeSet::new();
        ns.push(a);
        ns.push(b);
        ns.push(c);
        let mut x = ctx();
        let r = distinct_fn(&mut x, &[XPathValue::NodeSet(ns)]).unwrap();
        let out = r.as_node_set();
        assert_eq!(out.len(), 2);
        unsafe {
            crate::xml::tree::free_node(a);
            crate::xml::tree::free_node(b);
            crate::xml::tree::free_node(c);
        }
    }

    #[test]
    fn test_has_same_node() {
        let a = text_node("a");
        let b = text_node("b");
        let mut ns1 = NodeSet::new();
        ns1.push(a);
        let mut ns2 = NodeSet::new();
        ns2.push(b);
        let mut x = ctx();
        assert_eq!(
            has_same_node_fn(
                &mut x,
                &[XPathValue::NodeSet(ns1.clone()), XPathValue::NodeSet(ns2)]
            )
            .unwrap()
            .as_boolean(),
            false
        );
        let mut ns3 = NodeSet::new();
        ns3.push(a);
        assert_eq!(
            has_same_node_fn(
                &mut x,
                &[XPathValue::NodeSet(ns1), XPathValue::NodeSet(ns3)]
            )
            .unwrap()
            .as_boolean(),
            true
        );
        unsafe {
            crate::xml::tree::free_node(a);
            crate::xml::tree::free_node(b);
        }
    }
}
