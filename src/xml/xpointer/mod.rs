//! XPointer implementation (§26, §85 Phase 5).
//!
//! XML Pointer Language (XPointer) v1.0 support based on the
//! [XPointer Framework](https://www.w3.org/TR/xptr-framework/) and
//! [element() Scheme](https://www.w3.org/TR/xptr-element/) W3C Recommendations.
//!
//! This module provides:
//!
//! - **Shorthand pointers** — bare names treated as element IDs
//! - **`element()` scheme** — `element(id)` or `element(id/N/M/…)` for
//!   child-axis traversal
//! - **`xmlXPtrEval` C ABI** — for interop with libxml2 consumers
//!
//! The caller is responsible for stripping the `#` from the URI fragment;
//! this module receives only the fragment content.

use crate::abi::structs::{_xmlAttr, _xmlDoc, _xmlNode};
use crate::abi::types::xmlAttributeType::XML_ATTRIBUTE_ID;
use crate::abi::types::xmlElementType::{XML_ELEMENT_NODE, XML_TEXT_NODE};
use crate::xml::xpath::context::XPathContext;
use crate::xml::xpath::types::NodeSet;
use std::ffi::CStr;

#[cfg(test)]
use std::ffi::CString;
use std::os::raw::c_char;
use std::ptr;

// ═══════════════════════════════════════════════════════════════════════════════
// Public API
// ═══════════════════════════════════════════════════════════════════════════════

/// Evaluate an XPointer expression and return the pointed-to node.
///
/// Supports:
/// - **Shorthand pointers** — bare name treated as an element ID.
/// - **`element()` scheme** — `element(id)` selects the element with that ID;
///   `element(id/N)` selects the N-th child (1-indexed) of that element, etc.
///
/// Returns `None` if the pointer does not resolve to a node.
///
/// # Parameters
///
/// * `expr` — the XPointer expression (without the leading `#`).
/// * `doc` — pointer to the XML document to search in.
///
/// # Safety
///
/// `doc` must be a valid, non-null pointer to a fully parsed `_xmlDoc`.
pub unsafe fn xptr_eval(expr: &str, doc: *mut _xmlDoc) -> Option<*mut _xmlNode> {
    if doc.is_null() {
        return None;
    }

    let expr = expr.trim();

    if expr.is_empty() {
        return None;
    }

    // Try to parse as a scheme-based pointer: scheme(data)
    if let Some(result) = try_eval_scheme(expr, doc) {
        return result;
    }

    // Fall back to shorthand pointer (bare name as ID).
    shorthand_lookup(expr, doc)
}

/// Evaluate an XPointer using the full XPath/XPointer context.
///
/// This is a convenience wrapper that creates a temporary XPath context
/// and delegates to [`xptr_eval`].
///
/// # Safety
///
/// `doc` must be a valid, non-null pointer to a fully parsed `_xmlDoc`.
pub unsafe fn xptr_eval_with_context(
    expr: &str,
    doc: *mut _xmlDoc,
    _context: Option<&mut XPathContext>,
) -> Option<*mut _xmlNode> {
    xptr_eval(expr, doc)
}

// ═══════════════════════════════════════════════════════════════════════════════
// C ABI
// ═══════════════════════════════════════════════════════════════════════════════

/// C ABI entry point for XPointer evaluation.
///
/// Corresponds to `xmlXPtrEval` in libxml2.
///
/// # Safety
///
/// * `expr` must be a valid null-terminated C string.
/// * `doc` must be a valid pointer to `_xmlDoc` or NULL.
///
/// Returns a pointer to the selected `_xmlNode`, or NULL if the pointer
/// does not resolve.
pub unsafe extern "C" fn xmlXPtrEval(expr: *const c_char, doc: *mut _xmlDoc) -> *mut _xmlNode {
    if expr.is_null() || doc.is_null() {
        return ptr::null_mut();
    }

    let expr_str = match unsafe { CStr::from_ptr(expr) }.to_str() {
        Ok(s) => s,
        Err(_) => return ptr::null_mut(),
    };

    match unsafe { xptr_eval(expr_str, doc) } {
        Some(node) => node,
        None => ptr::null_mut(),
    }
}

/// Evaluate an XPointer expression and return a node-set.
///
/// Corresponds to `xmlXPtrEval` returning a node-set in some libxml2 APIs.
///
/// # Safety
///
/// * `expr` must be a valid null-terminated C string.
/// * `doc` must be a valid pointer to `_xmlDoc` or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPtrEvalNodeSet(
    expr: *const c_char,
    doc: *mut _xmlDoc,
) -> *mut crate::abi::structs::_xmlNodeSet {
    if expr.is_null() || doc.is_null() {
        return ptr::null_mut();
    }

    let expr_str = match unsafe { CStr::from_ptr(expr) }.to_str() {
        Ok(s) => s,
        Err(_) => return ptr::null_mut(),
    };

    let node = unsafe { xptr_eval(expr_str, doc) };

    let mut ns = NodeSet::new();
    if let Some(n) = node {
        ns.push(n);
    }

    unsafe { ns.to_raw() }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Scheme-based pointer evaluation
// ═══════════════════════════════════════════════════════════════════════════════

/// Try to evaluate `expr` as a scheme-based pointer (`scheme(data)`).
///
/// Returns `None` if the expression does not match a known scheme pattern.
unsafe fn try_eval_scheme(expr: &str, doc: *mut _xmlDoc) -> Option<Option<*mut _xmlNode>> {
    let expr = expr.trim();

    // Try to match `element(...)` scheme
    if let Some(inner) = strip_scheme(expr, "element") {
        return Some(unsafe { eval_element_scheme(inner, doc) });
    }

    // No known scheme matched; return None to let the caller fall back to
    // shorthand pointer.
    None
}

/// Strip a scheme name and parentheses from the front of `expr`.
///
/// If `expr` starts with `scheme(` and ends with `)`, returns the inner
/// content. Otherwise returns `None`.
fn strip_scheme<'a>(expr: &'a str, scheme: &str) -> Option<&'a str> {
    let expr = expr.trim();

    let expected_prefix = format!("{}(", scheme);
    if !expr.starts_with(&expected_prefix) {
        return None;
    }

    let inner_start = expected_prefix.len();
    if !expr.ends_with(')') {
        return None;
    }

    let inner_end = expr.len() - 1;
    if inner_end <= inner_start {
        return Some("");
    }

    Some(&expr[inner_start..inner_end])
}

// ═══════════════════════════════════════════════════════════════════════════════
// element() scheme
// ═══════════════════════════════════════════════════════════════════════════════

/// Evaluate an `element()` scheme pointer.
///
/// Syntax: `element(id)` or `element(id/N1/N2/...)`
///
/// * `element(id)` — select the element with the given ID.
/// * `element(id/N)` — select the N-th child (1-indexed) of the element
///   with the given ID.
/// * `element(id/N1/N2/...)` — traverse deeper child levels.
unsafe fn eval_element_scheme(inner: &str, doc: *mut _xmlDoc) -> Option<*mut _xmlNode> {
    let inner = inner.trim();
    if inner.is_empty() {
        return None;
    }

    // Split on '/'
    let parts: Vec<&str> = inner.split('/').collect();
    if parts.is_empty() {
        return None;
    }

    let id = parts[0].trim();
    if id.is_empty() {
        return None;
    }

    // Find the element with this ID
    let base = unsafe { find_element_by_id(id, doc) }?;

    // If only ID was given, return the element directly
    if parts.len() == 1 {
        return Some(base);
    }

    // Otherwise traverse child indices: element(id/N1/N2/...)
    let mut current = base;
    for &part in &parts[1..] {
        let index_str = part.trim();
        let index: usize = match index_str.parse() {
            Ok(n) if n >= 1 => n,
            _ => return None,
        };

        // Get the N-th child element (1-indexed)
        current = unsafe { nth_child_element(current, index) }?;
    }

    Some(current)
}

/// Get the N-th child element node (1-indexed) of `node`.
///
/// Only counts element nodes (XML_ELEMENT_NODE).
unsafe fn nth_child_element(node: *mut _xmlNode, n: usize) -> Option<*mut _xmlNode> {
    if node.is_null() {
        return None;
    }

    let mut count = 0usize;
    let mut child = unsafe { (*node).children };

    while !child.is_null() {
        let ty = unsafe { (*child).type_ };
        if ty == XML_ELEMENT_NODE as std::os::raw::c_int {
            count += 1;
            if count == n {
                return Some(child);
            }
        }
        child = unsafe { (*child).next };
    }

    None
}

// ═══════════════════════════════════════════════════════════════════════════════
// Shorthand pointer (bare name as ID)
// ═══════════════════════════════════════════════════════════════════════════════

/// Look up a bare name as an element ID (shorthand pointer).
///
/// Per the XPointer Framework, a shorthand pointer is treated as if it were
/// `element(id)`.
unsafe fn shorthand_lookup(name: &str, doc: *mut _xmlDoc) -> Option<*mut _xmlNode> {
    unsafe { find_element_by_id(name, doc) }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Element-by-ID lookup
// ═══════════════════════════════════════════════════════════════════════════════

/// Find an element by its ID attribute.
///
/// This function searches the document tree for an element whose `id`
/// attribute (case-insensitive name match) has the given value.
///
/// It also checks the DTD-declared ID type (`_xmlAttr.atype ==
/// XML_ATTRIBUTE_ID`) as a secondary identification mechanism.
///
/// # Parameters
///
/// * `id` — the ID value to search for.
/// * `doc` — the document to search.
///
/// # Returns
///
/// The first matching element node, or `None`.
unsafe fn find_element_by_id(id: &str, doc: *mut _xmlDoc) -> Option<*mut _xmlNode> {
    if doc.is_null() || id.is_empty() {
        return None;
    }

    // Walk the document tree searching for an element with a matching ID
    // attribute.
    let root = unsafe { (*doc).children };
    if root.is_null() {
        return None;
    }

    unsafe { walk_for_id(root, id) }
}

/// Recursively walk the tree looking for an element with the given ID.
unsafe fn walk_for_id(node: *mut _xmlNode, id: &str) -> Option<*mut _xmlNode> {
    if node.is_null() {
        return None;
    }

    // Check if this node is an element with a matching ID attribute
    let ty = unsafe { (*node).type_ };
    if ty == XML_ELEMENT_NODE as std::os::raw::c_int {
        if unsafe { element_has_id(node, id) } {
            return Some(node);
        }
    }

    // Recurse into children
    let mut child = unsafe { (*node).children };
    while !child.is_null() {
        if let Some(found) = unsafe { walk_for_id(child, id) } {
            return Some(found);
        }
        child = unsafe { (*child).next };
    }

    None
}

/// Check if an element node has an attribute whose ID value matches.
///
/// Checks:
/// 1. If the attribute's `atype` is `XML_ATTRIBUTE_ID`, compare its value.
/// 2. If the attribute's name is "id" (case-insensitive), compare its value.
unsafe fn element_has_id(node: *mut _xmlNode, id: &str) -> bool {
    if node.is_null() {
        return false;
    }

    let mut prop = unsafe { (*node).properties };
    while !prop.is_null() {
        let attr = unsafe { &*prop };

        // Check 1: DTD-declared ID type
        if attr.atype == XML_ATTRIBUTE_ID as std::os::raw::c_int {
            if let Some(val) = unsafe { get_attr_value(prop) } {
                if val == id {
                    return true;
                }
            }
        }

        // Check 2: attribute named "id" (case-insensitive)
        if !attr.name.is_null() {
            let name_str = unsafe { c_xmlchar_to_str(attr.name) };
            if name_str.as_deref() == Some("id") || name_str.as_deref() == Some("ID") {
                if let Some(val) = unsafe { get_attr_value(prop) } {
                    if val == id {
                        return true;
                    }
                }
            }
        }

        prop = unsafe { (*prop).next };
    }

    false
}

/// Extract the string value of an attribute.
unsafe fn get_attr_value(attr: *mut _xmlAttr) -> Option<String> {
    if attr.is_null() {
        return None;
    }

    let children = unsafe { (*attr).children };
    if children.is_null() {
        return None;
    }

    let text = unsafe { &*children };
    if text.type_ == XML_TEXT_NODE as std::os::raw::c_int && !text.content.is_null() {
        let val = unsafe { c_xmlchar_to_str(text.content) };
        return val;
    }

    None
}

/// Convert a `*const xmlChar` (C string) to a Rust `String`.
///
/// SAFETY: `ptr` must point to a null-terminated sequence of bytes.
unsafe fn c_xmlchar_to_str(ptr: *const crate::abi::types::xmlChar) -> Option<String> {
    if ptr.is_null() {
        return None;
    }

    // xmlChar is `c_uchar`; we reinterpret as `*const c_char` for CStr.
    let c_str = unsafe { CStr::from_ptr(ptr as *const c_char) };
    match c_str.to_str() {
        Ok(s) => Some(s.to_string()),
        Err(_) => None,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::allocator::xmlMallocZero;
    use crate::abi::types::xmlElementType::*;
    use std::mem;
    use std::os::raw::c_int;
    use std::ptr;

    // ── Helper: create a minimal document tree for testing ────────────────

    /// Create a minimal document with one element: `<root id="main">`.
    unsafe fn create_simple_doc() -> *mut _xmlDoc {
        let doc = xmlMallocZero(mem::size_of::<_xmlDoc>()) as *mut _xmlDoc;
        assert!(!doc.is_null());

        let root = xmlMallocZero(mem::size_of::<_xmlNode>()) as *mut _xmlNode;
        assert!(!root.is_null());

        unsafe {
            (*doc).type_ = XML_DOCUMENT_NODE as c_int;
            (*doc).doc = doc;
            (*doc).children = root;

            (*root).type_ = XML_ELEMENT_NODE as c_int;
            (*root).name = string_to_xmlchar("root");
            (*root).parent = doc as *mut _xmlNode;
            (*root).doc = doc;
            (*root).properties = ptr::null_mut();
        }

        // Add id="main" attribute
        let attr = unsafe { add_id_attr(root, "id", "main") };
        unsafe {
            (*root).properties = attr;
        }

        doc
    }

    /// Create a more complex document tree:
    /// ```
    /// <root id="main">
    ///   <child1 id="a"/>
    ///   <child2 id="b">
    ///     <grandchild id="c"/>
    ///   </child2>
    ///   <child3/>
    /// </root>
    /// ```
    unsafe fn create_complex_doc() -> *mut _xmlDoc {
        let doc = xmlMallocZero(mem::size_of::<_xmlDoc>()) as *mut _xmlDoc;
        assert!(!doc.is_null());

        // root element
        let root = xmlMallocZero(mem::size_of::<_xmlNode>()) as *mut _xmlNode;
        assert!(!root.is_null());

        unsafe {
            (*doc).type_ = XML_DOCUMENT_NODE as c_int;
            (*doc).doc = doc;
            (*doc).children = root;

            (*root).type_ = XML_ELEMENT_NODE as c_int;
            (*root).name = string_to_xmlchar("root");
            (*root).parent = doc as *mut _xmlNode;
            (*root).doc = doc;
        }

        let attr_root = unsafe { add_id_attr(root, "id", "main") };
        unsafe { (*root).properties = attr_root };

        // child1
        let child1 = unsafe { append_child_element(root, "child1") };
        let attr_c1 = unsafe { add_id_attr(child1, "id", "a") };
        unsafe { (*child1).properties = attr_c1 };

        // child2
        let child2 = unsafe { append_child_element(root, "child2") };
        let attr_c2 = unsafe { add_id_attr(child2, "id", "b") };
        unsafe { (*child2).properties = attr_c2 };

        // grandchild (child of child2)
        let grandchild = unsafe { append_child_element(child2, "grandchild") };
        let attr_gc = unsafe { add_id_attr(grandchild, "id", "c") };
        unsafe { (*grandchild).properties = attr_gc };

        // child3 (no ID)
        let _child3 = unsafe { append_child_element(root, "child3") };

        doc
    }

    unsafe fn string_to_xmlchar(s: &str) -> *const crate::abi::types::xmlChar {
        let c_str = CString::new(s).unwrap();
        c_str.into_raw() as *const crate::abi::types::xmlChar
    }

    unsafe fn append_child_element(parent: *mut _xmlNode, name: &str) -> *mut _xmlNode {
        let node = xmlMallocZero(mem::size_of::<_xmlNode>()) as *mut _xmlNode;
        assert!(!node.is_null());

        unsafe {
            (*node).type_ = XML_ELEMENT_NODE as c_int;
            (*node).name = string_to_xmlchar(name);
            (*node).parent = parent;
            (*node).doc = (*parent).doc;
            (*node).next = ptr::null_mut();
            (*node).prev = (*parent).last;
            (*node).properties = ptr::null_mut();

            // Link into parent's child list
            if (*parent).children.is_null() {
                (*parent).children = node;
                (*parent).last = node;
            } else {
                let last = (*parent).last;
                if !last.is_null() {
                    (*last).next = node;
                }
                (*parent).last = node;
            }
        }

        node
    }

    unsafe fn add_id_attr(node: *mut _xmlNode, name: &str, value: &str) -> *mut _xmlAttr {
        let attr = xmlMallocZero(mem::size_of::<_xmlAttr>()) as *mut _xmlAttr;
        assert!(!attr.is_null());

        // Create text child for the attribute value
        let text = xmlMallocZero(mem::size_of::<_xmlNode>()) as *mut _xmlNode;
        assert!(!text.is_null());

        unsafe {
            (*attr).type_ = 2; // XML_ATTRIBUTE_NODE
            (*attr).name = string_to_xmlchar(name);
            (*attr).parent = node;
            (*attr).doc = (*node).doc;
            (*attr).children = text;
            (*attr).last = text;
            (*attr).atype = crate::abi::types::xmlAttributeType::XML_ATTRIBUTE_CDATA as c_int;
            (*attr).next = ptr::null_mut();
            (*attr).prev = ptr::null_mut();

            (*text).type_ = XML_TEXT_NODE as c_int;
            (*text).name = string_to_xmlchar("text");
            (*text).content = string_to_xmlchar(value) as *mut crate::abi::types::xmlChar;
            (*text).parent = attr as *mut _xmlNode;
            (*text).doc = (*node).doc;
            (*text).next = ptr::null_mut();
            (*text).prev = ptr::null_mut();
        }

        attr
    }

    // ── Tests ────────────────────────────────────────────────────────────

    macro_rules! c_name_eq {
        ($node:expr, $expected:expr) => {
            assert_eq!(
                CStr::from_ptr((*$node).name as *const c_char)
                    .to_str()
                    .unwrap(),
                $expected
            );
        };
    }

    #[test]
    fn test_shorthand_pointer() {
        unsafe {
            let doc = create_simple_doc();
            let result = xptr_eval("main", doc);
            assert!(result.is_some());
            c_name_eq!(result.unwrap(), "root");
        }
    }

    #[test]
    fn test_shorthand_pointer_not_found() {
        unsafe {
            let doc = create_simple_doc();
            let result = xptr_eval("nonexistent", doc);
            assert!(result.is_none());
        }
    }

    #[test]
    fn test_element_scheme_basic() {
        unsafe {
            let doc = create_complex_doc();

            let result = xptr_eval("element(main)", doc);
            assert!(result.is_some());
            c_name_eq!(result.unwrap(), "root");

            let result = xptr_eval("element(a)", doc);
            assert!(result.is_some());
            c_name_eq!(result.unwrap(), "child1");

            let result = xptr_eval("element(c)", doc);
            assert!(result.is_some());
            c_name_eq!(result.unwrap(), "grandchild");
        }
    }

    #[test]
    fn test_element_scheme_with_child_sequence() {
        unsafe {
            let doc = create_complex_doc();

            let result = xptr_eval("element(main/1)", doc);
            assert!(result.is_some());
            c_name_eq!(result.unwrap(), "child1");

            let result = xptr_eval("element(main/2)", doc);
            assert!(result.is_some());
            c_name_eq!(result.unwrap(), "child2");

            let result = xptr_eval("element(main/2/1)", doc);
            assert!(result.is_some());
            c_name_eq!(result.unwrap(), "grandchild");
        }
    }

    #[test]
    fn test_element_scheme_child_out_of_range() {
        unsafe {
            let doc = create_complex_doc();
            let result = xptr_eval("element(main/99)", doc);
            assert!(result.is_none());
        }
    }

    #[test]
    fn test_element_scheme_zero_index() {
        unsafe {
            let doc = create_complex_doc();
            let result = xptr_eval("element(main/0)", doc);
            assert!(result.is_none());
        }
    }

    #[test]
    fn test_empty_expr() {
        unsafe {
            let doc = create_simple_doc();
            let result = xptr_eval("", doc);
            assert!(result.is_none());
        }
    }

    #[test]
    fn test_null_doc() {
        unsafe {
            let result = xptr_eval("main", ptr::null_mut());
            assert!(result.is_none());
        }
    }

    #[test]
    fn test_xml_xptr_eval_c_abi() {
        unsafe {
            let doc = create_simple_doc();
            let c_expr = CString::new("main").unwrap();
            let node = xmlXPtrEval(c_expr.as_ptr(), doc);
            assert!(!node.is_null());
            c_name_eq!(node, "root");
        }
    }

    #[test]
    fn test_xml_xptr_eval_null_expr() {
        unsafe {
            let doc = create_simple_doc();
            let node = xmlXPtrEval(ptr::null(), doc);
            assert!(node.is_null());
        }
    }

    #[test]
    fn test_xml_xptr_eval_null_doc() {
        unsafe {
            let c_expr = CString::new("main").unwrap();
            let node = xmlXPtrEval(c_expr.as_ptr(), ptr::null_mut());
            assert!(node.is_null());
        }
    }

    #[test]
    fn test_xml_xptr_eval_node_set() {
        unsafe {
            let doc = create_simple_doc();
            let c_expr = CString::new("main").unwrap();
            let ns = xmlXPtrEvalNodeSet(c_expr.as_ptr(), doc);
            assert!(!ns.is_null());
            assert_eq!((*ns).nodeNr, 1);
            assert!(!(*ns).nodeTab.is_null());
            let node = *(*ns).nodeTab;
            c_name_eq!(node, "root");
        }
    }

    #[test]
    fn test_element_scheme_not_found() {
        unsafe {
            let doc = create_complex_doc();
            let result = xptr_eval("element(nonexistent)", doc);
            assert!(result.is_none());
        }
    }

    #[test]
    fn test_element_scheme_extra_spaces() {
        unsafe {
            let doc = create_complex_doc();
            let result = xptr_eval("element( main )", doc);
            assert!(result.is_some());
            c_name_eq!(result.unwrap(), "root");
        }
    }

    #[test]
    fn test_child3_no_id() {
        unsafe {
            let doc = create_complex_doc();
            let result = xptr_eval("child3", doc);
            assert!(result.is_none());
        }
    }
}
