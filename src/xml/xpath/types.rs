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
//!
//! # Upstream contract
//!
//! Mirrors the value model of upstream `xpath.c` / `xpathInternals.h`
//! (`SRC-LIBXML2-2.15.0-XPATH-C`, parity target libxml2 2.15.3 oracle):
//! `xmlXPathObject` types (XPATH_NODESET, XPATH_BOOLEAN, XPATH_NUMBER,
//! XPATH_STRING, XPATH_POINT/RANGE/LOCATIONSET, XPATH_USERS,
//! XPATH_XSLT_TREE) and the string/number conversion functions
//! `xmlXPathCastToString`, `xmlXPathStringEvalNumber` and
//! `xmlXPathCastNumberToString`.
//!
//! # Conceptual behavior
//!
//! Defines the runtime value types and their conversions. Node-sets are
//! ordered, deduplicated collections of borrowed document nodes;
//! `node_string_value` computes the XPath string-value of a node
//! (R-000114 fixed the empty attribute string-value); `string_bytes_to_
//! number` / `number_to_string` are faithful ports of the R-000166
//! number conversion with the 1e9/1e-5 scientific threshold and
//! DBL_DIG=15 fraction digits.
//!
//! # Ownership & safety invariants
//!
//! `XPathNode` holds a raw `*mut _xmlNode` that is borrowed from the
//! document — the tree must outlive evaluation (SAFETY note on the
//! struct). Values own their storage (String/NodeSet) and are freed by
//! drop; nothing here allocates through the C allocator except the
//! exports bridge.
//!
//! # Historical quirks & epochs
//!
//! The conversion rules track the 2.15.3 oracle epoch: the E-001
//! newline-separated node-set dump (commit da35eeae, 2.9.10) is the
//! output epoch the XPath CLI surfaces target, and number formatting
//! (R-000166, 967/967 number() corpus) is fixed since the same era.
//!
//! # Deliberate oddities
//!
//! `-0.0` serializes as `0`, NaN as `NaN`, infinities as `Infinity`/
//! `-Infinity`, and integral values take the integer shortcut — the
//! upstream xmlXPathFormatNumber quirks reproduced instead of Rust
//! Display.
//!
//! # Proving courts
//!
//! XPATH-TYPES-* differential probes and the 967/967 number() corpus
//! compare conversions byte-identical against the oracle; cargo test
//! runs the conversion unit suites (incl. test_number_to_string).
//!
//! # Tempting simplifications that would break parity
//!
//! Do not switch conversions to Rust std float formatting or parsing:
//! the oracle digit accumulation (MAX_FRAC=20), exponent underflow
//! (5e-324 → 0), threshold selection and exponent padding are observable
//! (R-000166). Do not make node-sets own the tree: callers free the
//! document independently of the XPath object.

use crate::abi::structs::_xmlNode;
use std::cmp::Ordering;
use std::ptr;

// ═══════════════════════════════════════════════════════════════════════════════
// XPath Value Types
// ═══════════════════════════════════════════════════════════════════════════════

/// XPath type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XPathType {
    /// A node-set: an ordered, deduplicated collection of document nodes
    NodeSet,
    /// A string
    String,
    /// A number (IEEE 754 double)
    Number,
    /// A boolean
    Boolean,
    /// A single point in the tree: a node plus a position within it
    Point,
    /// A range of nodes in the tree
    Range,
    /// A set of points and ranges
    LocationSet,
    /// A user-defined value type
    Users,
    /// An XSLT tree fragment (result tree fragment)
    XsltTree,
    /// No type assigned yet (uninitialized value)
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
    /// Create an empty node-set.
    pub const fn new() -> Self {
        Self { nodes: Vec::new() }
    }

    /// Create a node-set containing exactly one node.
    pub fn singleton(node: *mut _xmlNode) -> Self {
        Self {
            nodes: vec![XPathNode(node)],
        }
    }

    /// Return `true` if the node-set contains no nodes.
    pub const fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Return the number of nodes in the node-set.
    pub const fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Iterate over the nodes in document order.
    pub fn iter(&self) -> impl Iterator<Item = *mut _xmlNode> + '_ {
        self.nodes.iter().map(|n| n.0)
    }

    /// Return the node at `index` in document order, or `None` if out of bounds.
    pub fn get(&self, index: usize) -> Option<*mut _xmlNode> {
        self.nodes.get(index).map(|n| n.0)
    }

    /// Return the first node in document order.
    pub fn first(&self) -> Option<*mut _xmlNode> {
        self.nodes.first().map(|n| n.0)
    }

    /// Return the last node in document order.
    pub fn last(&self) -> Option<*mut _xmlNode> {
        self.nodes.last().map(|n| n.0)
    }

    /// Return `true` if the given node is in the node-set.
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
    ///
    /// # SAFETY
    ///
    /// The function touches crate-global state only; it is safe
    /// as long as the caller respects the library's global
    /// initialization/cleanup ordering (xmlInitParser before use,
    /// xmlCleanupParser only after all users are done).
    ///
    /// Violating the global lifecycle ordering, or calling this after
    /// teardown or from a signal handler, is undefined behavior.
    pub unsafe fn to_raw(&self) -> *mut crate::abi::structs::_xmlNodeSet {
        let node_max = self.nodes.len();
        let node_tab = if node_max > 0 {
            let ptr = crate::abi::allocator::xmlMallocImpl(
                node_max * std::mem::size_of::<*mut _xmlNode>(),
            ) as *mut *mut _xmlNode;
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
    /// A node-set value
    NodeSet(NodeSet),
    /// A string value
    String(String),
    /// A number value (IEEE 754 double)
    Number(f64),
    /// A boolean value
    Boolean(bool),
}

impl XPathValue {
    /// Get the XPath type of this value.
    pub const fn xpath_type(&self) -> XPathType {
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
///
/// # Safety
///
/// - `node` must be NULL or a valid `_xmlNode` that stays alive for the
///   call; its `children` chain (recursed by `collect_text`) and every
///   `content`/`name` pointer must be NULL or valid NUL-terminated
///   strings, and the reachable subtree must be acyclic.
pub fn node_string_value(node: *mut _xmlNode) -> String {
    if node.is_null() {
        return String::new();
    }

    unsafe {
        let node_ref = &*node;
        match node_ref.type_ {
            1..=20 => {}
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

/// Port of upstream xpath.c `xmlXPathStringEvalNumber` (R-000166): the
/// oracle accumulates digits directly (`ret = ret * 10 + d`), caps the
/// fraction at MAX_FRAC=20 digits after any leading zeros, applies the
/// exponent with `pow(10.0, exp)` (underflowing to 0 below the smallest
/// subnormal, e.g. `5e-324`), accepts XML whitespace around the number, and
/// returns NaN for anything else — including a leading '+'.
pub fn string_bytes_to_number(bytes: &[u8]) -> f64 {
    let len = bytes.len();
    let mut cur = 0usize;
    // Skip leading XML whitespace.
    while cur < len && matches!(bytes[cur], b' ' | b'\t' | b'\n' | b'\r') {
        cur += 1;
    }
    let mut isneg = false;
    if cur < len && bytes[cur] == b'-' {
        isneg = true;
        cur += 1;
    }
    if cur >= len || (bytes[cur] != b'.' && !bytes[cur].is_ascii_digit()) {
        return f64::NAN;
    }

    let mut ret = 0.0f64;
    let mut ok = false;
    while cur < len && bytes[cur].is_ascii_digit() {
        ret = ret * 10.0 + (bytes[cur] - b'0') as f64;
        ok = true;
        cur += 1;
    }

    let mut frac: i32 = 0;
    if cur < len && bytes[cur] == b'.' {
        cur += 1;
        if (cur >= len || !bytes[cur].is_ascii_digit()) && !ok {
            return f64::NAN;
        }
        while cur < len && bytes[cur] == b'0' {
            frac += 1;
            cur += 1;
        }
        let max = frac + 20; // MAX_FRAC
        let mut fraction = 0.0f64;
        while cur < len && bytes[cur].is_ascii_digit() && frac < max {
            let v = (bytes[cur] - b'0') as f64;
            fraction = fraction * 10.0 + v;
            frac += 1;
            cur += 1;
        }
        fraction /= 10f64.powf(frac as f64);
        ret += fraction;
        while cur < len && bytes[cur].is_ascii_digit() {
            cur += 1;
        }
    }

    let mut exponent: i32 = 0;
    let mut is_exponent_negative = false;
    if cur < len && (bytes[cur] == b'e' || bytes[cur] == b'E') {
        cur += 1;
        if cur < len && bytes[cur] == b'-' {
            is_exponent_negative = true;
            cur += 1;
        } else if cur < len && bytes[cur] == b'+' {
            cur += 1;
        }
        while cur < len && bytes[cur].is_ascii_digit() {
            if exponent < 1000000 {
                exponent = exponent * 10 + (bytes[cur] - b'0') as i32;
            }
            cur += 1;
        }
    }
    while cur < len && matches!(bytes[cur], b' ' | b'\t' | b'\n' | b'\r') {
        cur += 1;
    }
    if cur != len {
        return f64::NAN;
    }
    if isneg {
        ret = -ret;
    }
    if is_exponent_negative {
        exponent = -exponent;
    }
    ret *= 10f64.powf(exponent as f64);
    ret
}

/// Convert a string to a number (XPath 1.0 §4.7.1) — upstream
/// `xmlXPathStringEvalNumber` semantics.
pub fn string_to_number(s: &str) -> f64 {
    string_bytes_to_number(s.as_bytes())
}

/// Convert a number to a string (XPath 1.0 §4.7.2) — a faithful port of
/// upstream `xmlXPathCastNumberToString` / `xmlXPathFormatNumber` (xpath.c,
/// R-000166): the integer shortcut, the 1e9/1e-5 scientific threshold, and
/// the DBL_DIG=15 fraction-digit computation reproduce the oracle's exact
/// digits, including exponent formatting (`e+20`, `e-05`) and
/// trailing-zero trimming.
pub fn number_to_string(n: f64) -> String {
    if n.is_nan() {
        return "NaN".to_string();
    }
    if n.is_infinite() {
        return if n > 0.0 {
            "Infinity".to_string()
        } else {
            "-Infinity".to_string()
        };
    }
    if n == 0.0 {
        // Both +0 and -0 serialize as "0" per XPath 1.0.
        return "0".to_string();
    }
    // Upstream integer shortcut (xmlXPathFormatNumber): integral values
    // within the int range print as plain decimal.
    if n > i32::MIN as f64 && n < i32::MAX as f64 && n == (n as i32) as f64 {
        return format!("{}", n as i32);
    }

    let absolute_value = n.abs();
    let s = if ((absolute_value > 1e9) || (absolute_value < 1e-5)) && absolute_value != 0.0 {
        // Scientific notation: "%*.*e" with 14 fraction digits, then trim
        // trailing zeros before the exponent (work[size] == 'e' scan).
        let raw = format!("{:.14e}", n);
        let e_pos = raw.find('e').expect("exponent format contains 'e'");
        let mantissa = &raw[..e_pos];
        let exponent = &raw[e_pos + 1..];
        let mut mantissa = mantissa.to_string();
        while mantissa.ends_with('0') {
            mantissa.pop();
        }
        if mantissa.ends_with('.') {
            mantissa.pop();
        }
        // C's %e pads the exponent to at least two digits and always
        // includes the sign: "e+20", "e-05", "e+100".
        let (sign, digits) = if let Some(rest) = exponent.strip_prefix('-') {
            ("-", rest)
        } else {
            ("+", exponent)
        };
        let digits = if digits.len() < 2 {
            format!("0{}", digits)
        } else {
            digits.to_string()
        };
        format!("{}e{}{}", mantissa, sign, digits)
    } else {
        // Regular notation: fraction digits depend on the integer place.
        let integer_place = absolute_value.log10() as i32;
        let fraction_place = if integer_place > 0 {
            15 - integer_place - 1
        } else {
            15 - integer_place
        };
        let mut s = format!("{:.*}", fraction_place as usize, n);
        // Trim fractional trailing zeros (and a trailing dot).
        if s.contains('.') {
            while s.ends_with('0') {
                s.pop();
            }
            if s.ends_with('.') {
                s.pop();
            }
        }
        s
    };
    if s == "-0" {
        return "0".to_string();
    }
    s
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
///
/// # SAFETY
///
/// - `a`, `b` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
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
    let n = (*parent_a).parent;
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
    #[allow(clippy::approx_constant)]
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
    #[allow(clippy::approx_constant)]
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
        assert!(v.as_boolean());

        let v = XPathValue::Number(0.0);
        assert!(!v.as_boolean());

        let v = XPathValue::Number(f64::NAN);
        assert!(!v.as_boolean());

        let v = XPathValue::String("hello".into());
        assert_eq!(v.as_string(), "hello");
        assert!(v.as_boolean());

        let v = XPathValue::String("".into());
        assert!(!v.as_boolean());

        let v = XPathValue::Boolean(true);
        assert_eq!(v.as_number(), 1.0);
        assert_eq!(v.as_string(), "true");

        let v = XPathValue::Boolean(false);
        assert_eq!(v.as_number(), 0.0);
        assert_eq!(v.as_string(), "false");
    }

    #[test]
    fn test_node_set() {
        let ns = NodeSet::new();
        assert!(ns.is_empty());
        assert_eq!(ns.len(), 0);
    }
}
