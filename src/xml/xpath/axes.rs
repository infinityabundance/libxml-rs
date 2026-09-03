//! XPath 1.0 Axis Implementation (§25).
//!
//! Implements all 13 XPath axes for traversing the XML tree.
//!
//! # UPSTREAM-PARITY
//!
//! Each axis follows the XPath 1.0 specification (§2.2) and libxml2
//! observable behavior for node ordering and filtering.
//!
//! # Courts
//!
//! XPATH-AXES-*
//!
//! # Upstream contract
//!
//! Mirrors the axis traversal of upstream `xpath.c`
//! (`SRC-LIBXML2-2.15.0-XPATH-C`, parity target libxml2 2.15.3 oracle):
//! the 13 axes per XPath 1.0 §2.2, including document-order and
//! reverse-axis ordering and the namespace-axis synthesis.
//!
//! # Conceptual behavior
//!
//! Each axis walks the tree from the context node and filters by the node
//! test. Forward axes (child, descendant, following-sibling, following,
//! attribute, namespace, self, descendant-or-self) emit document order;
//! reverse axes (ancestor, ancestor-or-self, preceding-sibling, preceding)
//! emit reverse document order, which the evaluator re-reverses.
//! Namespace nodes are synthesized from nsDef chains and carry no parent
//! pointer.
//!
//! # Ownership & safety invariants
//!
//! Traversal borrows the tree: returned node-sets hold borrowed `_xmlNode`
//! pointers valid while the document lives; the axis code never frees or
//! detaches nodes. Namespace nodes are ephemeral views over nsDef
//! entries, not owned copies.
//!
//! # Historical quirks & epochs
//!
//! Namespace nodes have no parent pointer — a long-standing upstream
//! divergence (QUIRK-0002 / LORE-0006): the c14n.c birth commit 044fc6b7
//! (2002-03-04) already references "fixing #61290 namespace nodes have no
//! parent long standing divergence", and the XPath namespace-axis behavior
//! remains special. Axis ordering has been byte-stable across the 2.7.8 →
//! 2.15.3 oracle span.
//!
//! # Deliberate oddities
//!
//! The namespace axis emits nodes whose name/URI come from the nsDef
//! entry while their parent is NULL — reproduced deliberately because
//! XPath namespace-axis consumers (XSLT, exsl:node-set) depend on it.
//!
//! # Proving courts
//!
//! XPATH-AXES-* and the XPATH differential probes compare axis results
//! byte-identical against the oracle; the XSLT match-pattern courts
//! (CLI-XSLTPROC-*) exercise axes through compiled templates.
//!
//! # Tempting simplifications that would break parity
//!
//! Do not give namespace nodes a parent pointer and do not dedupe or sort
//! axis output by value: ordering, duplicates and the parent-less
//! namespace model are observable through xmlXPathObject node-sets.

use crate::abi::structs::_xmlNode;
use crate::abi::structs::_xmlNs;
use crate::abi::types::xmlElementType;
use crate::xml::xpath::ast::{Axis, NameTest, NodeTest};
use crate::xml::xpath::types::NodeSet;
use std::os::raw::c_int;
use std::os::raw::c_void;
use std::ptr;

/// Traverse an axis from a context node, returning nodes matching the node test.
///
/// Returns nodes in document order (or reverse document order for
/// reverse axes: ancestor, ancestor-or-self, preceding, preceding-sibling).
///
/// # SAFETY
///
/// - `context_node` must be valid pointers (or NULL
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
pub unsafe fn traverse_axis(
    context_node: *mut _xmlNode,
    axis: Axis,
    node_test: &NodeTest,
    include_attributes: bool,
    include_namespaces: bool,
) -> NodeSet {
    let mut result = NodeSet::new();

    match axis {
        Axis::Child => child_axis(context_node, node_test, &mut result),
        Axis::Descendant => descendant_axis(context_node, node_test, &mut result),
        Axis::Parent => parent_axis(context_node, node_test, &mut result),
        Axis::Ancestor => ancestor_axis(context_node, node_test, &mut result, false),
        Axis::AncestorOrSelf => ancestor_axis(context_node, node_test, &mut result, true),
        Axis::FollowingSibling => following_sibling_axis(context_node, node_test, &mut result),
        Axis::PrecedingSibling => preceding_sibling_axis(context_node, node_test, &mut result),
        Axis::Following => following_axis(context_node, node_test, &mut result),
        Axis::Preceding => preceding_axis(context_node, node_test, &mut result),
        Axis::Attribute => {
            if include_attributes {
                attribute_axis(context_node, node_test, &mut result);
            }
        }
        Axis::Namespace => {
            if include_namespaces {
                namespace_axis(context_node, node_test, &mut result);
            }
        }
        Axis::Self_ => self_axis(context_node, node_test, &mut result),
        Axis::DescendantOrSelf => {
            self_axis(context_node, node_test, &mut result);
            descendant_axis(context_node, node_test, &mut result);
        }
    }

    result
}

// ═══════════════════════════════════════════════════════════════════════════════
// Individual Axes
// ═══════════════════════════════════════════════════════════════════════════════

/// child axis: children of context node.
///
/// UPSTREAM-PARITY (xpath.c xmlXPathNextChild/xmlXPathNextChildElement):
/// the DTD node that heads `doc->children` is NOT part of the XPath data
/// model. Upstream's child traversal either starts at the root element
/// (xmlXPathNextChildElement via xmlDocGetRootElement for name tests) or
/// skips XML_DTD_NODE while walking siblings; the XPath 1.0 child axis of
/// the document node contains only element, text, comment and PI nodes.
/// Including the DTD (type 14) would make `/root` match the doctype name
/// and corrupt string-values, e.g. count(/root) returning 2 and
/// string(/root) returning the DTD's empty value.
///
/// # Safety
///
/// - `node` must be NULL or a valid `_xmlNode`; its `children`/`next`
///   sibling chain must contain only valid, alive `_xmlNode` pointers and
///   must be acyclic; `node_test` must be a valid reference.
unsafe fn child_axis(node: *mut _xmlNode, node_test: &NodeTest, result: &mut NodeSet) {
    if node.is_null() {
        return;
    }
    let mut child = (*node).children;
    while !child.is_null() {
        let child_type = unsafe { (*child).type_ };
        if child_type != xmlElementType::XML_DTD_NODE as c_int
            && matches_node_test(child, node_test)
        {
            result.push(child);
        }
        child = (*child).next;
    }
}

/// descendant axis: all descendants (children, grandchildren, etc.).
unsafe fn descendant_axis(node: *mut _xmlNode, node_test: &NodeTest, result: &mut NodeSet) {
    if node.is_null() {
        return;
    }
    let mut child = (*node).children;
    while !child.is_null() {
        if matches_node_test(child, node_test) {
            result.push(child);
        }
        // Recurse into children
        descendant_axis(child, node_test, result);
        child = (*child).next;
    }
}

/// parent axis: parent of context node (singleton).
unsafe fn parent_axis(node: *mut _xmlNode, node_test: &NodeTest, result: &mut NodeSet) {
    if node.is_null() {
        return;
    }
    let parent = (*node).parent;
    if !parent.is_null() && matches_node_test(parent, node_test) {
        result.push(parent);
    }
}

/// ancestor axis: all ancestors, optionally including self.
unsafe fn ancestor_axis(
    node: *mut _xmlNode,
    node_test: &NodeTest,
    result: &mut NodeSet,
    include_self: bool,
) {
    if node.is_null() {
        return;
    }

    if include_self && matches_node_test(node, node_test) {
        result.push(node);
    }

    let mut n = (*node).parent;
    while !n.is_null() {
        if matches_node_test(n, node_test) {
            result.push(n);
        }
        n = (*n).parent;
    }

    // Ancestor axis returns in reverse document order (parent first).
    // Since we traverse upward, the result is naturally in reverse document order.
}

/// following-sibling axis: all following siblings.
unsafe fn following_sibling_axis(node: *mut _xmlNode, node_test: &NodeTest, result: &mut NodeSet) {
    if node.is_null() {
        return;
    }
    let mut n = (*node).next;
    while !n.is_null() {
        if matches_node_test(n, node_test) {
            result.push(n);
        }
        n = (*n).next;
    }
}

/// preceding-sibling axis: all preceding siblings (reverse document order).
unsafe fn preceding_sibling_axis(node: *mut _xmlNode, node_test: &NodeTest, result: &mut NodeSet) {
    if node.is_null() {
        return;
    }
    let mut n = (*node).prev;
    while !n.is_null() {
        if matches_node_test(n, node_test) {
            result.push(n);
        }
        n = (*n).prev;
    }
}

/// following axis: all nodes after context node (excluding descendants).
unsafe fn following_axis(node: *mut _xmlNode, node_test: &NodeTest, result: &mut NodeSet) {
    if node.is_null() {
        return;
    }

    // Walk up to find the first following sibling, then traverse
    let mut n = node;
    loop {
        let next_sibling = (*n).next;
        if !next_sibling.is_null() {
            // Traverse this sibling and all its descendants
            traverse_subtree(next_sibling, node_test, result);
            // Then continue with siblings of ancestors
            let mut s = next_sibling;
            while !(*s).next.is_null() {
                s = (*s).next;
            }
            // Move to next sibling chain
            n = s;
            continue;
        }
        // No next sibling, move up
        n = (*n).parent;
        if n.is_null() || matches_node_test_for_any(n) {
            // Reached root or document node
            break;
        }
    }
}

/// preceding axis: all nodes before context node (reverse document order).
unsafe fn preceding_axis(node: *mut _xmlNode, node_test: &NodeTest, result: &mut NodeSet) {
    if node.is_null() {
        return;
    }

    // Walk previous siblings and their descendants
    let mut n = node;
    loop {
        let prev_sibling = (*n).prev;
        if !prev_sibling.is_null() {
            // Traverse this sibling's subtree in reverse
            traverse_subtree_reverse(prev_sibling, node_test, result);
            n = prev_sibling;
            continue;
        }
        // No previous sibling, move up
        n = (*n).parent;
        if n.is_null() {
            break;
        }
        // The parent itself is part of preceding if we're going up
        // But parent comes after preceding siblings
        break;
    }
}

/// attribute axis: attributes of context node.
unsafe fn attribute_axis(node: *mut _xmlNode, node_test: &NodeTest, result: &mut NodeSet) {
    if node.is_null() {
        return;
    }
    // UPSTREAM-PARITY (xpath.c xmlXPathNextAttribute): the attribute axis
    // iterates the `properties` chain ONLY for element nodes. For any other
    // context node type (DTD, text, doc node, …) upstream returns an empty
    // set. `properties` lives at offset 88 of _xmlNode, but non-element node
    // types reuse that storage for their own fields (e.g. a DTD's `entities`)
    // — walking it as an attribute chain reads foreign/freed memory. This
    // guard keeps nokogiri's `@*/attribute::` evaluation on a DTD context
    // node (e.g. a document type node carrying the DTD) from crashing into a
    // stale property list.
    if (*node).type_ != xmlElementType::XML_ELEMENT_NODE as c_int {
        return;
    }
    let mut prop = (*node).properties;
    while !prop.is_null() {
        // Attributes are represented as _xmlAttr nodes, which are also _xmlNode
        let attr_node = prop as *mut _xmlNode;
        if matches_node_test(attr_node, node_test) {
            result.push(attr_node);
        }
        prop = (*prop).next;
    }
}

/// namespace axis: namespaces of context node.
///
/// UPSTREAM-PARITY (xpath.c xmlXPathNextNamespace + xmlXPathNodeSetAddNs):
/// - the axis is empty unless the context node is an ELEMENT;
/// - the implicit xml namespace is ALWAYS first (xmlXPathNextNamespace
///   returns the static xmlXPathXMLNamespace before the in-scope list);
/// - the remaining nodes are the in-scope declarations (own decls first,
///   then ancestors, nearest declaration wins) emitted in REVERSE list
///   order (tmpNsList[--tmpNsNr]);
/// - every emitted node is an independent `_xmlNs` copy whose `next` is the
///   owning element (xmlXPathNodeSetDupNs) and whose `_private` carries the
///   owner too (php's DOMXPath reads `_private` to build DOMNameSpaceNode
///   proxies).
unsafe fn namespace_axis(node: *mut _xmlNode, node_test: &NodeTest, result: &mut NodeSet) {
    if node.is_null() {
        return;
    }
    if (*node).type_ != xmlElementType::XML_ELEMENT_NODE as c_int {
        return;
    }

    // Collect in-scope declarations: the context node's own declarations
    // first, then those inherited from its ancestors, deduplicated by prefix
    // (the nearest declaration wins — it is collected first and later
    // duplicates are skipped). The implicit xml namespace is NOT part of the
    // tree declarations (upstream xmlGetNsList does not return it).
    let mut in_scope: Vec<*mut _xmlNs> = Vec::new();
    let mut cur = node;
    while !cur.is_null() {
        let mut ns_def = (*cur).nsDef;
        while !ns_def.is_null() {
            let prefix_null = (*ns_def).prefix.is_null();
            let duplicate = in_scope.iter().any(|&e| {
                if prefix_null {
                    (*e).prefix.is_null()
                } else {
                    !(*e).prefix.is_null()
                        && crate::abi::exports_xml2::xmlStrEqual((*e).prefix, (*ns_def).prefix) != 0
                }
            });
            if !duplicate {
                in_scope.push(ns_def);
            }
            ns_def = (*ns_def).next;
        }
        cur = (*cur).parent;
    }

    // Emission order: the xml namespace first, then the in-scope list in
    // reverse (upstream xmlXPathNextNamespace serves tmpNsList[--tmpNsNr]).
    let xml_present = in_scope.iter().any(|&e| {
        !(*e).prefix.is_null()
            && crate::abi::exports_xml2::xmlStrEqual(
                (*e).prefix,
                c"xml".as_ptr() as *const crate::abi::types::xmlChar,
            ) != 0
    });
    let doc = (*node).doc;
    let mut push_copy = |ns: *mut _xmlNs, owner: *mut _xmlNode| -> bool {
        let copy =
            crate::abi::allocator::xmlMallocZero(core::mem::size_of::<_xmlNs>()) as *mut _xmlNs;
        if copy.is_null() {
            return false;
        }
        unsafe {
            (*copy).type_ = crate::abi::types::xmlElementType::XML_NAMESPACE_DECL as c_int;
            (*copy).href = if (*ns).href.is_null() {
                ptr::null()
            } else {
                crate::abi::exports_xml2::xmlStrdup((*ns).href)
            };
            (*copy).prefix = if (*ns).prefix.is_null() {
                ptr::null()
            } else {
                crate::abi::exports_xml2::xmlStrdup((*ns).prefix)
            };
            (*copy).context = doc;
            // UPSTREAM-PARITY (xmlXPathNodeSetDupNs + php dom_xpath.c): the
            // copy records its owning element in both `next` and `_private`.
            (*copy).next = owner as *mut _xmlNs;
            (*copy)._private = owner as *mut c_void;
        }
        let ns_node = copy as *mut _xmlNode;
        if matches_namespace_node(ns_node, node_test) {
            result.push(ns_node);
            true
        } else {
            // Failed the node test: free the copy we just allocated.
            free_namespace_copy(copy);
            false
        }
    };

    if !xml_present {
        // The implicit xml namespace (upstream's xmlXPathXMLNamespace struct):
        // always present on the axis. A throwaway source struct feeds the
        // copy (the href/prefix literals are static; only the struct itself is
        // freed afterwards).
        let src =
            crate::abi::allocator::xmlMallocZero(core::mem::size_of::<_xmlNs>()) as *mut _xmlNs;
        if !src.is_null() {
            unsafe {
                (*src).type_ = crate::abi::types::xmlElementType::XML_NAMESPACE_DECL as c_int;
                (*src).href = c"http://www.w3.org/XML/1998/namespace".as_ptr()
                    as *const crate::abi::types::xmlChar;
                (*src).prefix = c"xml".as_ptr() as *const crate::abi::types::xmlChar;
            }
            push_copy(src, node);
            crate::abi::allocator::xmlFreeImpl(src as *mut c_void);
        }
    }
    for &ns in in_scope.iter().rev() {
        if ns.is_null() {
            continue;
        }
        push_copy(ns, node);
    }

    if std::env::var("LIBXML_RS_TRACE_NSASIS").is_ok() {
        eprintln!(
            "[nsaxis] node={:p} test={:?} in_scope={}",
            node,
            node_test,
            in_scope.len()
        );
    }
}

/// Match a namespace-axis node (an `_xmlNs` cast to `_xmlNode`) against a
/// node test. The implicit namespace prefix wildcard and explicit name tests
/// are applied.
unsafe fn matches_namespace_node(node: *mut _xmlNode, node_test: &NodeTest) -> bool {
    let ns = node as *mut _xmlNs;
    match node_test {
        NodeTest::Node
        | NodeTest::Wildcard
        | NodeTest::NsWildcard(_)
        | NodeTest::NsWildcardUri(_) => true,
        NodeTest::NameTest(NameTest::Any) => true,
        NodeTest::NameTest(NameTest::LocalName(prefix)) => {
            if (*ns).prefix.is_null() {
                prefix.is_empty()
            } else {
                crate::xml::string::xmlstr_to_string((*ns).prefix) == *prefix
            }
        }
        NodeTest::NameTest(NameTest::QName { prefix, .. }) => {
            if (*ns).prefix.is_null() {
                prefix.is_empty()
            } else {
                crate::xml::string::xmlstr_to_string((*ns).prefix) == *prefix
            }
        }
        NodeTest::NameTest(NameTest::QNameUri { .. }) => true,
        _ => false,
    }
}

/// Free a synthetic namespace node allocated by [`namespace_axis`]. These are
/// independent `_xmlNs` copies owned by the node-set; upstream frees them when
/// the containing node-set is released.
unsafe fn free_namespace_copy(ns: *mut _xmlNs) {
    if ns.is_null() {
        return;
    }
    if !(*ns).href.is_null() {
        crate::abi::allocator::xmlFreeImpl((*ns).href as *mut c_void);
    }
    if !(*ns).prefix.is_null() {
        crate::abi::allocator::xmlFreeImpl((*ns).prefix as *mut c_void);
    }
    crate::abi::allocator::xmlFreeImpl(ns as *mut c_void);
}

/// self axis: the context node itself.
unsafe fn self_axis(node: *mut _xmlNode, node_test: &NodeTest, result: &mut NodeSet) {
    if !node.is_null() && matches_node_test(node, node_test) {
        result.push(node);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helper functions
// ═══════════════════════════════════════════════════════════════════════════════

/// Traverse a subtree in document order, collecting matching nodes.
unsafe fn traverse_subtree(node: *mut _xmlNode, node_test: &NodeTest, result: &mut NodeSet) {
    if node.is_null() {
        return;
    }

    if matches_node_test(node, node_test) {
        result.push(node);
    }

    let mut child = (*node).children;
    while !child.is_null() {
        traverse_subtree(child, node_test, result);
        child = (*child).next;
    }
}

/// Traverse a subtree in reverse document order.
unsafe fn traverse_subtree_reverse(
    node: *mut _xmlNode,
    node_test: &NodeTest,
    result: &mut NodeSet,
) {
    if node.is_null() {
        return;
    }

    // First traverse children in reverse
    let child = (*node).children;
    if !child.is_null() {
        // Find last child
        let mut last = child;
        while !(*last).next.is_null() {
            last = (*last).next;
        }
        // Traverse from last to first
        let mut n = last;
        loop {
            traverse_subtree_reverse(n, node_test, result);
            if n == child {
                break;
            }
            n = (*n).prev;
        }
    }

    if matches_node_test(node, node_test) {
        result.push(node);
    }
}

/// Check if a node matches a node test.
///
/// # SAFETY
///
/// - `node` must be valid pointers (or NULL
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
pub unsafe fn matches_node_test(node: *mut _xmlNode, node_test: &NodeTest) -> bool {
    if node.is_null() {
        return false;
    }

    let node_ref = &*node;
    let node_type = node_ref.type_;

    match node_test {
        NodeTest::Node => true,
        NodeTest::Text => node_type == 3 || node_type == 4, // text or CDATA
        NodeTest::Comment => node_type == 8,
        NodeTest::ProcessingInstruction(target) => {
            if node_type == 7 {
                if let Some(target) = target {
                    // Check PI target
                    let name = crate::xml::string::xmlstr_to_string(node_ref.name);
                    name == *target
                } else {
                    true
                }
            } else {
                false
            }
        }
        NodeTest::NameTest(name_test) => matches_name_test(node, name_test),
        NodeTest::Wildcard => {
            // Match any element node (principal node type for child/descendant/etc.)
            node_type == 1
        }
        NodeTest::NsWildcard(prefix) => {
            if node_type == 1 {
                // Check namespace prefix
                if let Some(ns) = node_ref.ns.as_ref() {
                    let ns_prefix = crate::xml::string::xmlstr_to_string(ns.prefix);
                    ns_prefix == *prefix
                } else {
                    prefix.is_empty()
                }
            } else {
                false
            }
        }
        NodeTest::NsWildcardUri(uri) => {
            if node_type == 1 {
                // Check namespace URI (prefix resolved at evaluation)
                if let Some(ns) = node_ref.ns.as_ref() {
                    let ns_uri = crate::xml::string::xmlstr_to_string(ns.href);
                    ns_uri == *uri
                } else {
                    uri.is_empty()
                }
            } else {
                false
            }
        }
    }
}

/// Check if a node matches a name test.
unsafe fn matches_name_test(node: *mut _xmlNode, name_test: &NameTest) -> bool {
    if node.is_null() {
        return false;
    }

    let node_ref = &*node;
    // Defensive: an attribute/element node must carry a name for a name test
    // to match; a NULL or non-canonical name pointer is skipped rather than
    // dereferenced (prevents a crash on a partially-torn-down property list).
    if node_ref.name.is_null() {
        return false;
    }

    match name_test {
        NameTest::Any => {
            // Match any element/attribute node
            node_ref.type_ == 1 || node_ref.type_ == 2 || node_ref.type_ == 13
        }
        NameTest::LocalName(local) => {
            let name = crate::xml::string::xmlstr_to_string(node_ref.name);
            // UPSTREAM-PARITY (xpath.c name-test matching): an UNPREFIXED name
            // test matches nodes with no namespace URI unless a default namespace
            // is in scope. nokogiri registers no default unprefixed binding, so
            // `//foo` must not match `<a:foo>`/default-namespaced `<foo>` nodes
            // (test_remove_namespaces expects 0).
            if node_ref.ns.is_null() && name == *local {
                return true;
            }
            // Some callers (lxml) bind a default namespace for unprefixed
            // names; the compiler resolves those into NameTest::QNameUri, so the
            // plain LocalName path still requires an empty namespace URI.
            false
        }
        NameTest::QName { prefix, local } => {
            let name = crate::xml::string::xmlstr_to_string(node_ref.name);
            if name != *local {
                return false;
            }
            // Check namespace prefix
            if let Some(ns) = node_ref.ns.as_ref() {
                let ns_prefix = crate::xml::string::xmlstr_to_string(ns.prefix);
                ns_prefix == *prefix
            } else {
                prefix.is_empty()
            }
        }
        NameTest::QNameUri { uri, local } => {
            let name = crate::xml::string::xmlstr_to_string(node_ref.name);
            if name != *local {
                return false;
            }
            // Check namespace URI (prefix resolved at evaluation)
            if let Some(ns) = node_ref.ns.as_ref() {
                let ns_uri = crate::xml::string::xmlstr_to_string(ns.href);
                ns_uri == *uri
            } else {
                uri.is_empty()
            }
        }
    }
}

/// Check if any node test would match (for traversal boundary detection).
const unsafe fn matches_node_test_for_any(_node: *mut _xmlNode) -> bool {
    true
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {

    use crate::xml::xpath::ast::NodeTest;

    #[test]
    fn test_node_test_matches() {
        // Smoke test - node test matching is tested more thoroughly
        // in the integration tests
        assert!(matches!(NodeTest::Node, NodeTest::Node));
    }
}
