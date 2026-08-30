//! XPath 1.0 Runtime Types (§25).
//!
//! Internal Rust representation of XPath values: node-sets, strings,
//! numbers, booleans, and conversions between them.
//!
//! # UPSTREAM-PARITY
//!
//! XPath 1.0 type system with exact IEEE 754 floating-point semantics:
//! NaN, infinity, negative zero, rounding behavior.
//!
//! # Courts
//!
//! XPATH-TYPES-*

use crate::abi::structs::_xmlNode;
use crate::abi::types::xmlChar;
use std::cmp::Ordering;
use std::ptr;

// ═══════════════════════════════════════════════════════════════════════════════
// XPath Value Types
// ═══════════════════════════════════════════════════════════════════════════════

/// XPath type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XPathType {
    NodeSet,
    String,
    Number,
    Boolean,
    Point,
    Range,
    LocationSet,
    Users,
    XsltTree,
    Undefined,
}

/// A node in a node-set, identified by pointer.
///
/// We use raw pointers because:
/// 1. The tree is owned by the document, not by XPath.
/// 2. The C ABI exposes node pointers that callers manipulate.
/// 3. Multiple XPath evaluations may reference the same tree.
///
/// SAFETY: Node pointers must remain valid for the duration of evaluation.
#[derive(Debug, Clone, Copy)]
pub struct XPathNode(pub *mut _xmlNode);

impl PartialEq for XPathNode {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for XPathNode {}

impl PartialOrd for XPathNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for XPathNode {
    fn cmp(&self, other: &Self) -> Ordering {
        // Compare by pointer value for document order
        // In a full implementation, this would use the document order algorithm
        self.0.cmp(&other.0)
    }
}

impl std::hash::Hash for XPathNode {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

/// An XPath node-set.
///
/// Internally stored as a sorted, deduplicated Vec of node pointers
/// in document order.
#[derive(Debug, Clone)]
pub struct NodeSet {
    nodes: Vec<XPathNode>,
}

impl NodeSet {
    pub fn new() -> Self {
        Self { nodes: Vec::new() }
    }

    pub fn singleton(node: *mut _xmlNode) -> Self {
        Self {
            nodes: vec![XPathNode(node)],
        }
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = *mut _xmlNode> + '_ {
        self.nodes.iter().map(|n| n.0)
    }

    pub fn get(&self, index: usize) -> Option<*mut _xmlNode> {
        self.nodes.get(index).map(|n| n.0)
    }

    pub fn first(&self) -> Option<*mut _xmlNode> {
        self.nodes.first().map(|n| n.0)
    }

    pub fn last(&self) -> Option<*mut _xmlNode> {
        self.nodes.last().map(|n| n.0)
    }

    pub fn contains(&self, node: *mut _xmlNode) -> bool {
        self.nodes.iter().any(|n| n.0 == node)
    }

    /// Add a node to the set, maintaining document order and uniqueness.
    pub fn push(&mut self, node: *mut _xmlNode) {
        if !self.nodes.iter().any(|n| n.0 == node) {
            self.nodes.push(XPathNode(node));
            self.sort();
        }
    }

    /// Extend with another node-set.
    pub fn extend(&mut self, other: &NodeSet) {
        for node in other.iter() {
            self.push(node);
        }
    }

    /// Sort nodes in document order.
    ///
    /// # UPSTREAM-PARITY
    ///
    /// XPath node-sets are always in document order (XPath 1.0 §3.3).
    /// libxml2 maintains this via its node-set insertion/merge logic plus
    /// the document-order comparator (xmlXPathNodeSetSort). Sorting by
    /// pointer address is NOT document order and breaks downstream ordering
    /// guarantees; the oracle-observed symptom is rotated results on the
    /// second of two transforms in one process.
    pub fn sort(&mut self) {
        self.nodes
            .sort_by(|a, b| unsafe { compare_document_order(a.0, b.0) });
        self.nodes.dedup();
    }

    /// Convert to raw C ABI node-set.
    ///
    /// SAFETY: The returned pointer must be freed with xmlXPathFreeNodeSet
    /// or the owning XPath object must be freed.
    pub unsafe fn to_raw(&self) -> *mut crate::abi::structs::_xmlNodeSet {
        let node_max = self.nodes.len();
        let node_tab = if node_max > 0 {
            let ptr =
                crate::abi::allocator::xmlMallocImpl(node_max * std::mem::size_of::<*mut _xmlNode>())
                    as *mut *mut _xmlNode;
            if ptr.is_null() {
                return ptr::null_mut();
            }
            for (i, node) in self.nodes.iter().enumerate() {
                ptr::write(ptr.add(i), node.0);
            }
            ptr
        } else {
            ptr::null_mut()
        };

        let raw = crate::abi::allocator::xmlMallocImpl(std::mem::size_of::<
            crate::abi::structs::_xmlNodeSet,
        >()) as *mut crate::abi::structs::_xmlNodeSet;
        if raw.is_null() {
            if !node_tab.is_null() {
                crate::abi::allocator::xmlFreeImpl(node_tab as *mut _);
            }
            return ptr::null_mut();
        }
        ptr::write(
            raw,
            crate::abi::structs::_xmlNodeSet {
                nodeNr: node_max as std::os::raw::c_int,
                nodeMax: node_max as std::os::raw::c_int,
                nodeTab: node_tab,
            },
        );
        raw
    }
}

impl Default for NodeSet {
    fn default() -> Self {
        Self::new()
    }
}

/// XPath runtime value.
#[derive(Debug, Clone)]
pub enum XPathValue {
    NodeSet(NodeSet),
    String(String),
    Number(f64),
    Boolean(bool),
}

impl XPathValue {
    /// Get the XPath type of this value.
    pub fn xpath_type(&self) -> XPathType {
        match self {
            XPathValue::NodeSet(_) => XPathType::NodeSet,
            XPathValue::String(_) => XPathType::String,
            XPathValue::Number(_) => XPathType::Number,
            XPathValue::Boolean(_) => XPathType::Boolean,
        }
    }

    /// Convert to boolean (XPath 1.0 §3.4).
    pub fn as_boolean(&self) -> bool {
        match self {
            XPathValue::NodeSet(ns) => !ns.is_empty(),
            XPathValue::String(s) => !s.is_empty(),
            XPathValue::Number(n) => *n != 0.0 && !n.is_nan(),
            XPathValue::Boolean(b) => *b,
        }
    }

    /// Convert to number (XPath 1.0 §3.5).
    pub fn as_number(&self) -> f64 {
        match self {
            XPathValue::NodeSet(ns) => {
                // Convert string value of first node to number
                if let Some(node) = ns.first() {
                    let s = node_string_value(node);
                    string_to_number(&s)
                } else {
                    f64::NAN
                }
            }
            XPathValue::String(s) => string_to_number(s),
            XPathValue::Number(n) => *n,
            XPathValue::Boolean(true) => 1.0,
            XPathValue::Boolean(false) => 0.0,
        }
    }

    /// Convert to string (XPath 1.0 §3.6).
    pub fn as_string(&self) -> String {
        match self {
            XPathValue::NodeSet(ns) => {
                if let Some(node) = ns.first() {
                    node_string_value(node)
                } else {
                    String::new()
                }
            }
            XPathValue::String(s) => s.clone(),
            XPathValue::Number(n) => number_to_string(*n),
            XPathValue::Boolean(true) => "true".to_string(),
            XPathValue::Boolean(false) => "false".to_string(),
        }
    }

    /// Get node-set reference (panics if not a node-set).
    pub fn as_node_set(&self) -> &NodeSet {
        match self {
            XPathValue::NodeSet(ns) => ns,
            _ => panic!("XPathValue is not a node-set"),
        }
    }

    /// Get mutable node-set reference.
    pub fn as_node_set_mut(&mut self) -> &mut NodeSet {
        match self {
            XPathValue::NodeSet(ns) => ns,
            _ => panic!("XPathValue is not a node-set"),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// String value of a node
// ═══════════════════════════════════════════════════════════════════════════════

/// Get the string value of a node (XPath 1.0 §5.1).
///
/// For element/root nodes: concatenation of all descendant text nodes.
/// For text nodes: the text content.
/// For attribute nodes: the attribute value.
/// For namespace nodes: the namespace URI.
/// For comment/PI nodes: the content.
pub fn node_string_value(node: *mut _xmlNode) -> String {
    if node.is_null() {
        return String::new();
    }

    unsafe {
        let node_ref = &*node;
        match node_ref.type_ {
            1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 12 | 13 | 14 | 15 | 16 | 17 | 18 | 19
            | 20 => {}
            _ => return String::new(),
        }

        // Element / document / HTML document: concatenate text descendants
        if node_ref.type_ == 1 || node_ref.type_ == 9 || node_ref.type_ == 13 {
            let mut result = String::new();
            collect_text(&mut result, node);
            return result;
        }

        // Attribute node (type 2): the value is stored as the first text
        // child of the attribute node (tree::set_prop layout, matching
        // libxml2's xmlAttr->children). NOTE: type 13 is
        // XML_HTML_DOCUMENT_NODE, not attribute.
        if node_ref.type_ == 2 {
            if !node_ref.children.is_null() {
                let child = &*node_ref.children;
                if (child.type_ == 3 || child.type_ == 4) && !child.content.is_null() {
                    return crate::xml::string::xmlstr_to_string(child.content);
                }
            }
            return String::new();
        }

        // Text / CDATA
        if node_ref.type_ == 3 || node_ref.type_ == 4 {
            if !node_ref.content.is_null() {
                return crate::xml::string::xmlstr_to_string(node_ref.content);
            }
            return String::new();
        }

        // Comment / PI
        if node_ref.type_ == 7 {
            // PI: content
            if !node_ref.content.is_null() {
                return crate::xml::string::xmlstr_to_string(node_ref.content);
            }
            return String::new();
        }

        String::new()
    }
}

/// Recursively collect text content from element/document nodes.
unsafe fn collect_text(result: &mut String, node: *mut _xmlNode) {
    if node.is_null() {
        return;
    }
    let node_ref = &*node;

    // If this is a text or CDATA node, append its content
    if node_ref.type_ == 3 || node_ref.type_ == 4 {
        if !node_ref.content.is_null() {
            result.push_str(&crate::xml::string::xmlstr_to_string(node_ref.content));
        }
        return;
    }

    // For element/document nodes, recurse into children
    if node_ref.type_ == 1 || node_ref.type_ == 9 || node_ref.type_ == 19 {
        let mut child = node_ref.children;
        while !child.is_null() {
            collect_text(result, child);
            child = (*child).next;
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Number <-> String conversions
// ═══════════════════════════════════════════════════════════════════════════════

/// Convert a string to a number (XPath 1.0 §4.7.1).
pub fn string_to_number(s: &str) -> f64 {
    let s = s.trim();
    if s.is_empty() {
        return f64::NAN;
    }
    // Handle IEEE special values
    match s {
        "NaN" => return f64::NAN,
        "Infinity" | "INF" => return f64::INFINITY,
        "-Infinity" | "-INF" => return f64::NEG_INFINITY,
        _ => {}
    }
    // Try parsing as a number
    if let Ok(n) = s.parse::<f64>() {
        n
    } else {
        f64::NAN
    }
}

/// Convert a number to a string (XPath 1.0 §4.7.2).
///
/// UPSTREAM-PARITY:
/// - NaN → "NaN"
/// - +0 → "0"
/// - -0 → "0" (XPath says negative zero stringifies as "0")
/// - Infinity → "Infinity"
/// - -Infinity → "-Infinity"
/// - Integer → no decimal point: "42"
/// - Non-integer → at least one digit after decimal: "3.14"
pub fn number_to_string(n: f64) -> String {
    if n.is_nan() {
        return "NaN".to_string();
    }
    if n.is_infinite() {
        if n.is_sign_negative() {
            return "-Infinity".to_string();
        }
        return "Infinity".to_string();
    }
    if n == 0.0 {
        return "0".to_string();
    }

    // For integers, no decimal point
    if n.fract() == 0.0 && n.is_finite() {
        // Check if it's within safe integer range
        if n.abs() < 1e16 {
            return format!("{:.0}", n);
        }
    }

    // Format with minimal decimal places
    let s = format!("{:.15}", n);
    // Trim trailing zeros
    let trimmed = s.trim_end_matches('0');
    // Ensure at least one digit after decimal
    if trimmed.ends_with('.') {
        format!("{}0", trimmed)
    } else {
        trimmed.to_string()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Node comparison for document order
// ═══════════════════════════════════════════════════════════════════════════════

/// Compare two nodes in document order.
///
/// Returns:
/// - `Ordering::Less` if `a` comes before `b` in document order
/// - `Ordering::Greater` if `a` comes after `b`
/// - `Ordering::Equal` if `a == b`
///
/// UPSTREAM-PARITY: Uses the `xmlXPathCmpNodes` algorithm.
pub unsafe fn compare_document_order(a: *mut _xmlNode, b: *mut _xmlNode) -> Ordering {
    if a.is_null() && b.is_null() {
        return Ordering::Equal;
    }
    if a.is_null() {
        return Ordering::Less;
    }
    if b.is_null() {
        return Ordering::Greater;
    }
    if a == b {
        return Ordering::Equal;
    }

    // Find depths of both nodes
    let depth_a = node_depth(a);
    let depth_b = node_depth(b);

    // If one is an ancestor of the other, the ancestor comes first
    if depth_a < depth_b {
        let mut n = b;
        for _ in 0..(depth_b - depth_a) {
            n = (*n).parent;
            if n.is_null() {
                break;
            }
        }
        if n == a {
            return Ordering::Less;
        }
    } else if depth_b < depth_a {
        let mut n = a;
        for _ in 0..(depth_a - depth_b) {
            n = (*n).parent;
            if n.is_null() {
                break;
            }
        }
        if n == b {
            return Ordering::Greater;
        }
    }

    // Find the common ancestor and the first differing child
    let mut parent_a = a;
    let mut parent_b = b;

    // Move both up to the same depth
    let mut d_a = depth_a;
    let mut d_b = depth_b;
    while d_a > d_b {
        parent_a = (*parent_a).parent;
        d_a -= 1;
    }
    while d_b > d_a {
        parent_b = (*parent_b).parent;
        d_b -= 1;
    }

    // Move both up until they share the same parent
    while (*parent_a).parent != (*parent_b).parent {
        parent_a = (*parent_a).parent;
        parent_b = (*parent_b).parent;
        if parent_a.is_null() || parent_b.is_null() {
            // Fallback: compare by pointer
            return a.cmp(&b);
        }
    }

    // Now parent_a and parent_b are siblings. Find which comes first.
    let mut n = (*parent_a).parent;
    if n.is_null() {
        return a.cmp(&b);
    }
    let mut child = (*n).children;
    while !child.is_null() {
        if child == parent_a {
            return Ordering::Less;
        }
        if child == parent_b {
            return Ordering::Greater;
        }
        child = (*child).next;
    }

    // Fallback
    a.cmp(&b)
}

/// Compute the depth of a node (root = 0).
unsafe fn node_depth(node: *mut _xmlNode) -> usize {
    let mut depth = 0;
    let mut n = node;
    while !(*n).parent.is_null() {
        depth += 1;
        n = (*n).parent;
    }
    depth
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_string_to_number() {
        assert!(string_to_number("").is_nan());
        assert!(string_to_number("NaN").is_nan());
        assert_eq!(string_to_number("42"), 42.0);
        assert_eq!(string_to_number("-42"), -42.0);
        assert_eq!(string_to_number("3.14"), 3.14);
        assert_eq!(string_to_number("  42  "), 42.0);
        assert!(string_to_number("true").is_nan());
        assert!(string_to_number("false").is_nan());
        assert_eq!(string_to_number("0"), 0.0);
    }

    #[test]
    fn test_number_to_string() {
        assert_eq!(number_to_string(f64::NAN), "NaN");
        assert_eq!(number_to_string(f64::INFINITY), "Infinity");
        assert_eq!(number_to_string(f64::NEG_INFINITY), "-Infinity");
        assert_eq!(number_to_string(0.0), "0");
        assert_eq!(number_to_string(-0.0), "0");
        assert_eq!(number_to_string(42.0), "42");
        assert_eq!(number_to_string(3.14), "3.14");
    }

    #[test]
    fn test_value_conversions() {
        let v = XPathValue::Number(42.0);
        assert_eq!(v.as_number(), 42.0);
        assert_eq!(v.as_string(), "42");
        assert_eq!(v.as_boolean(), true);

        let v = XPathValue::Number(0.0);
        assert_eq!(v.as_boolean(), false);

        let v = XPathValue::Number(f64::NAN);
        assert_eq!(v.as_boolean(), false);

        let v = XPathValue::String("hello".into());
        assert_eq!(v.as_string(), "hello");
        assert_eq!(v.as_boolean(), true);

        let v = XPathValue::String("".into());
        assert_eq!(v.as_boolean(), false);

        let v = XPathValue::Boolean(true);
        assert_eq!(v.as_number(), 1.0);
        assert_eq!(v.as_string(), "true");

        let v = XPathValue::Boolean(false);
        assert_eq!(v.as_number(), 0.0);
        assert_eq!(v.as_string(), "false");
    }

    #[test]
    fn test_node_set() {
        let mut ns = NodeSet::new();
        assert!(ns.is_empty());
        assert_eq!(ns.len(), 0);
    }
}
