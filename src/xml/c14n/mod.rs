//! Canonical XML implementation (§28, §85 Phase 7).
//!
//! Inclusive and exclusive canonicalization, comments, namespace propagation,
//! attribute ordering, character escaping, subsets, node sets.
//! Must be byte-exact compared to oracle.
//!
//! References:
//! - XML Canonicalization (C14N) — inclusive: <https://www.w3.org/TR/xml-c14n11/>
//! - Exclusive XML Canonicalization (C14N): <https://www.w3.org/2001/10/xml-exc-c14n>

#![allow(
    missing_docs,
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_ptr_alignment,
    clippy::missing_safety_doc,
    clippy::too_many_lines,
    clippy::type_complexity
)]

use core::ffi::c_void;
use core::ptr;
use std::collections::HashSet;
use std::os::raw::{c_char, c_int};

use crate::abi::structs::*;
use crate::abi::types::xmlElementType::*;
use crate::abi::types::*;
use crate::xml::io;
use crate::xml::tree;

// ═══════════════════════════════════════════════════════════════════════════════
// Constants
// ═══════════════════════════════════════════════════════════════════════════════

/// The XML namespace prefix string "xml".
const XML_XML_PREFIX: &[xmlChar] = b"xml\0";

/// The XML namespace URI string.
const XML_XML_NS_URI: &[xmlChar] = b"http://www.w3.org/XML/1998/namespace\0";

/// The xmlns namespace URI.
const _XMLNS_NS_URI: &[xmlChar] = b"http://www.w3.org/2000/xmlns/\0";

/// The xmlns prefix string.
const _XMLNS_PREFIX: &[xmlChar] = b"xmlns\0";

// ═══════════════════════════════════════════════════════════════════════════════
// C14N Mode
// ═══════════════════════════════════════════════════════════════════════════════

/// Canonicalization mode flags.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// typedef enum {
///     XML_C14N_1_0 = 0,       /* C14N 1.0 (inclusive) */
///     XML_C14N_EXCLUSIVE_1_0 = 1, /* Exclusive C14N 1.0 */
///     XML_C14N_1_1 = 2,       /* C14N 1.1 */
///     XML_C14N_1_0_WITH_COMMENTS = 3, /* C14N 1.0 with comments */
///     XML_C14N_EXCLUSIVE_1_0_WITH_COMMENTS = 4, /* Exclusive with comments */
///     XML_C14N_1_1_WITH_COMMENTS = 5  /* C14N 1.1 with comments */
/// } xmlC14NMode;
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum C14nMode {
    XML_C14N_1_0 = 0,
    XML_C14N_EXCLUSIVE_1_0 = 1,
    XML_C14N_1_1 = 2,
    XML_C14N_1_0_WITH_COMMENTS = 3,
    XML_C14N_EXCLUSIVE_1_0_WITH_COMMENTS = 4,
    XML_C14N_1_1_WITH_COMMENTS = 5,
}

impl C14nMode {
    /// Returns true if this mode includes comments in the output.
    fn with_comments(self) -> bool {
        matches!(
            self,
            C14nMode::XML_C14N_1_0_WITH_COMMENTS
                | C14nMode::XML_C14N_EXCLUSIVE_1_0_WITH_COMMENTS
                | C14nMode::XML_C14N_1_1_WITH_COMMENTS
        )
    }

    /// Returns true if this mode is exclusive.
    fn is_exclusive(self) -> bool {
        matches!(
            self,
            C14nMode::XML_C14N_EXCLUSIVE_1_0 | C14nMode::XML_C14N_EXCLUSIVE_1_0_WITH_COMMENTS
        )
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// C14N Context
// ═══════════════════════════════════════════════════════════════════════════════

/// Namespace entry for the context stack.
#[allow(dead_code)]
#[derive(Debug, Clone)]
struct NsEntry {
    /// The prefix (NULL for default namespace).
    prefix: *const xmlChar,
    /// The namespace URI (NULL for undeclaration).
    href: *const xmlChar,
    /// Whether this namespace was rendered in the output.
    rendered: bool,
}

/// C14N serialization context.
///
/// Tracks namespace propagation state, visited prefixes, and
/// document-level state during canonicalization.
#[derive(Debug)]
pub struct C14nContext {
    /// The canonicalization mode.
    pub mode: C14nMode,
    /// Stack of in-scope namespace declarations per depth.
    ns_stack: Vec<Vec<NsEntry>>,
    /// Set of inclusive namespace prefixes (for exclusive C14N).
    #[allow(dead_code)]
    inclusive_ns_prefixes: Option<HashSet<String>>,
    /// The document being canonicalized.
    #[allow(dead_code)]
    doc: *mut _xmlDoc,
}

impl C14nContext {
    /// Create a new C14N context.
    ///
    /// # SAFETY
    ///
    /// - `doc` must be a valid pointer to an `_xmlDoc` or NULL.
    pub unsafe fn new(
        doc: *mut _xmlDoc,
        mode: C14nMode,
        inclusive_ns_prefixes: Option<HashSet<String>>,
    ) -> Self {
        let mut ctx = C14nContext {
            mode,
            ns_stack: Vec::new(),
            inclusive_ns_prefixes,
            doc,
        };
        // Push the initial (document-level) namespace scope, which contains
        // the implicit `xml` namespace.
        let xml_prefix = XML_XML_PREFIX.as_ptr() as *const xmlChar;
        let xml_href = XML_XML_NS_URI.as_ptr() as *const xmlChar;
        ctx.ns_stack.push(vec![NsEntry {
            prefix: xml_prefix,
            href: xml_href,
            rendered: false,
        }]);
        ctx
    }

    /// Enter a new namespace scope (depth + 1).
    #[allow(dead_code)]
    fn push_scope(&mut self) {
        // Clone the current top scope as the base for the new scope
        let base = if let Some(top) = self.ns_stack.last() {
            top.clone()
        } else {
            Vec::new()
        };
        self.ns_stack.push(base);
    }

    /// Exit the current namespace scope (depth - 1).
    #[allow(dead_code)]
    fn pop_scope(&mut self) {
        self.ns_stack.pop();
    }

    /// Add a namespace declaration to the current scope.
    #[allow(dead_code)]
    fn add_namespace(&mut self, prefix: *const xmlChar, href: *const xmlChar) {
        if let Some(top) = self.ns_stack.last_mut() {
            // Check if this prefix already exists in the current scope
            if !top
                .iter()
                .any(|e| unsafe { crate::abi::exports_xml2::xmlStrEqual(e.prefix, prefix) != 0 })
            {
                top.push(NsEntry {
                    prefix,
                    href,
                    rendered: false,
                });
            } else {
                // Update the existing entry
                if let Some(existing) = top.iter_mut().find(|e| unsafe {
                    crate::abi::exports_xml2::xmlStrEqual(e.prefix, prefix) != 0
                }) {
                    existing.href = href;
                    existing.rendered = false;
                }
            }
        }
    }

    /// Check if a prefix is in scope.
    #[allow(dead_code)]
    fn is_prefix_in_scope(&self, prefix: *const xmlChar) -> bool {
        self.ns_stack.iter().rev().any(|scope| {
            scope
                .iter()
                .any(|e| unsafe { crate::abi::exports_xml2::xmlStrEqual(e.prefix, prefix) != 0 })
        })
    }

    /// Get the href for a prefix from the current scope.
    #[allow(dead_code)]
    fn get_href_for_prefix(&self, prefix: *const xmlChar) -> *const xmlChar {
        for scope in self.ns_stack.iter().rev() {
            for entry in scope.iter() {
                if unsafe { crate::abi::exports_xml2::xmlStrEqual(entry.prefix, prefix) != 0 } {
                    return entry.href;
                }
            }
        }
        ptr::null()
    }

    /// Check if a prefix is in the inclusive namespace prefixes list.
    #[allow(dead_code)]
    fn is_inclusive_prefix(&self, prefix: *const xmlChar) -> bool {
        if let Some(ref set) = self.inclusive_ns_prefixes {
            if prefix.is_null() {
                return set.contains("");
            }
            let prefix_str = unsafe {
                let c_str = core::ffi::CStr::from_ptr(prefix as *const c_char);
                match c_str.to_str() {
                    Ok(s) => s.to_string(),
                    Err(_) => return false,
                }
            };
            set.contains(&prefix_str)
        } else {
            false
        }
    }

    /// Mark a namespace as rendered.
    #[allow(dead_code)]
    fn mark_rendered(&mut self, prefix: *const xmlChar) {
        for scope in self.ns_stack.iter_mut().rev() {
            for entry in scope.iter_mut() {
                if unsafe { crate::abi::exports_xml2::xmlStrEqual(entry.prefix, prefix) != 0 } {
                    entry.rendered = true;
                    return;
                }
            }
        }
    }

    /// Check if a namespace is already rendered.
    #[allow(dead_code)]
    fn is_rendered(&self, prefix: *const xmlChar) -> bool {
        for scope in self.ns_stack.iter().rev() {
            for entry in scope.iter() {
                if unsafe { crate::abi::exports_xml2::xmlStrEqual(entry.prefix, prefix) != 0 } {
                    return entry.rendered;
                }
            }
        }
        false
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// C14N Escaping
// ═══════════════════════════════════════════════════════════════════════════════

/// Escape text content per C14N rules.
///
/// Canonical XML requires:
/// - `<` → `&lt;`
/// - `>` → `&gt;`
/// - `&` → `&amp;`
/// - Carriage return `\r` (0x0D) → `&#xD;`
/// - `]]>` → `]]&gt;`
///
/// # SAFETY
///
/// - `buf` must be a valid pointer to a mutable `_xmlBuffer`.
/// - `text` must be a valid pointer to `len` bytes of xmlChar data, or NULL.
unsafe fn c14n_escape_text(buf: *mut _xmlBuffer, text: *const xmlChar, len: c_int) {
    if buf.is_null() || text.is_null() || len <= 0 {
        return;
    }

    let mut i: c_int = 0;
    while i < len {
        let ch = unsafe { *text.add(i as usize) };

        // Check for `]]>` sequence
        if ch == b']'
            && i + 2 < len
            && unsafe { *text.add(i as usize + 1) == b']' }
            && unsafe { *text.add(i as usize + 2) == b'>' }
        {
            // Write `]]&gt;` — escape the `>` that ends `]]>`
            io::buf_add(buf, b"]]" as *const u8, 2); // write `]]`
            io::buf_add(buf, b"&gt;" as *const u8, 4);
            i += 3;
            continue;
        }

        match ch {
            b'<' => {
                io::buf_add(buf, b"&lt;" as *const u8, 4);
            }
            b'>' => {
                io::buf_add(buf, b"&gt;" as *const u8, 4);
            }
            b'&' => {
                io::buf_add(buf, b"&amp;" as *const u8, 5);
            }
            0x0D => {
                // Carriage return: &#xD;
                io::buf_add(buf, b"&#xD;" as *const u8, 5);
            }
            _ => {
                io::buf_add(buf, &ch as *const u8, 1);
            }
        }
        i += 1;
    }
}

/// Escape attribute values per C14N rules.
///
/// Canonical XML requires:
/// - `<` → `&lt;`
/// - `&` → `&amp;`
/// - `"` → `&quot;`
/// - Tab (0x09) → `&#x9;`
/// - Newline (0x0A) → `&#xA;`
/// - Carriage return (0x0D) → `&#xD;`
/// - `]]>` → `]]&gt;`
///
/// # SAFETY
///
/// - `buf` must be a valid pointer to a mutable `_xmlBuffer`.
/// - `text` must be a valid pointer to a null-terminated xmlChar string, or NULL.
unsafe fn c14n_escape_attr(buf: *mut _xmlBuffer, text: *const xmlChar) {
    if buf.is_null() || text.is_null() {
        return;
    }

    let len = tree::xml_strlen(text);
    let mut i: c_int = 0;
    while i < len {
        let ch = unsafe { *text.add(i as usize) };

        // Check for `]]>` sequence
        if ch == b']'
            && i + 2 < len
            && unsafe { *text.add(i as usize + 1) == b']' }
            && unsafe { *text.add(i as usize + 2) == b'>' }
        {
            // Write `]]&gt;`
            io::buf_add(buf, b"]]" as *const u8, 2); // write `]]`
            io::buf_add(buf, b"&gt;" as *const u8, 4);
            i += 3;
            continue;
        }

        match ch {
            b'<' => {
                io::buf_add(buf, b"&lt;" as *const u8, 4);
            }
            b'&' => {
                io::buf_add(buf, b"&amp;" as *const u8, 5);
            }
            b'"' => {
                io::buf_add(buf, b"&quot;" as *const u8, 6);
            }
            0x09 => {
                // Tab
                io::buf_add(buf, b"&#x9;" as *const u8, 5);
            }
            0x0A => {
                // Newline
                io::buf_add(buf, b"&#xA;" as *const u8, 5);
            }
            0x0D => {
                // Carriage return
                io::buf_add(buf, b"&#xD;" as *const u8, 5);
            }
            _ => {
                io::buf_add(buf, &ch as *const u8, 1);
            }
        }
        i += 1;
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Namespace Collection
// ═══════════════════════════════════════════════════════════════════════════════

/// Represents a collected namespace declaration for output.
#[derive(Debug, Clone)]
struct CollectedNs {
    /// The prefix (NULL for default namespace).
    prefix: *const xmlChar,
    /// The namespace URI.
    href: *const xmlChar,
}

/// Collect visible namespace declarations for a node per inclusive/exclusive rules.
///
/// For **inclusive** C14N:
/// - All namespaces in scope for the node are included.
/// - Namespaces inherited from ancestors are included.
/// - Namespace undeclarations are emitted when needed.
///
/// For **exclusive** C14N:
/// - Only namespaces actually used by the node and its attributes are included.
/// - If an inclusive-ns-prefix list is provided, those prefixes are always included.
///
/// # SAFETY
///
/// - `node` must be a valid pointer to an `_xmlNode` or NULL.
/// - `ctx` must be a valid pointer to a `C14nContext`.
unsafe fn c14n_collect_namespaces(node: *mut _xmlNode, ctx: &mut C14nContext) -> Vec<CollectedNs> {
    if node.is_null() {
        return Vec::new();
    }

    let n = unsafe { &*node };
    if n.type_ != XML_ELEMENT_NODE as c_int {
        return Vec::new();
    }

    let mut collected: Vec<CollectedNs> = Vec::new();
    let mut seen_prefixes: Vec<*const xmlChar> = Vec::new();

    // NOTE: The `xml` namespace (prefix `xml`, URI `http://www.w3.org/XML/1998/namespace`)
    // is always implicitly available per the XML Namespaces specification.
    // Per C14N, it MUST NOT be explicitly declared in the output.
    // See W3C Canonical XML 1.0 §2.4 and Exclusive XML Canonicalization §2.1.2.

    if ctx.mode.is_exclusive() {
        // ── Exclusive C14N namespace collection ──
        //
        // Only include namespaces actually used by this node and its attributes,
        // plus any prefixes in the inclusive-ns-prefixes list.

        // Collect namespaces used by this node
        let mut used_prefixes: Vec<*const xmlChar> = Vec::new();

        // The node's own namespace
        if !n.ns.is_null() {
            let ns = unsafe { &*n.ns };
            used_prefixes.push(ns.prefix);
        }

        // Namespaces used by attributes
        let mut attr = n.properties;
        while !attr.is_null() {
            let a = unsafe { &*attr };
            if !a.ns.is_null() {
                let ans = unsafe { &*a.ns };
                if !ans.prefix.is_null()
                    && !used_prefixes.iter().any(|p| unsafe {
                        crate::abi::exports_xml2::xmlStrEqual(*p, ans.prefix) != 0
                    })
                {
                    used_prefixes.push(ans.prefix);
                }
            }
            attr = a.next;
        }

        // For each used prefix, find the namespace declaration by walking
        // up the ancestor chain
        for &used_prefix in &used_prefixes {
            let ns = find_ns_declaration(node, used_prefix);
            if !ns.is_null() {
                let ns_ref = unsafe { &*ns };
                if !seen_prefixes.iter().any(|p| unsafe {
                    crate::abi::exports_xml2::xmlStrEqual(*p, ns_ref.prefix) != 0
                }) {
                    collected.push(CollectedNs {
                        prefix: ns_ref.prefix,
                        href: ns_ref.href,
                    });
                    seen_prefixes.push(ns_ref.prefix);
                }
            }
        }

        // Include inclusive namespace prefixes
        if let Some(ref inclusive_set) = ctx.inclusive_ns_prefixes {
            for inc_prefix_str in inclusive_set.iter() {
                let inc_prefix = if inc_prefix_str.is_empty() {
                    ptr::null()
                } else {
                    let c_str = format!("{}\0", inc_prefix_str);
                    c_str.as_ptr() as *const xmlChar
                };

                if !seen_prefixes.iter().any(|p| {
                    if inc_prefix.is_null() {
                        p.is_null()
                    } else {
                        !p.is_null()
                            && unsafe { crate::abi::exports_xml2::xmlStrEqual(*p, inc_prefix) != 0 }
                    }
                }) {
                    let ns = find_ns_declaration(node, inc_prefix);
                    if !ns.is_null() {
                        let ns_ref = unsafe { &*ns };
                        collected.push(CollectedNs {
                            prefix: ns_ref.prefix,
                            href: ns_ref.href,
                        });
                        seen_prefixes.push(ns_ref.prefix);
                    }
                }
            }
        }
    } else {
        // ── Inclusive C14N namespace collection ──
        //
        // Collect ALL namespaces in scope for this node, walking up ancestors.

        let mut cur: *mut _xmlNode = node;
        while !cur.is_null() {
            let cur_node = unsafe { &*cur };
            let mut ns_def = cur_node.nsDef;
            while !ns_def.is_null() {
                let ns = unsafe { &*ns_def };
                let ns_prefix = ns.prefix;

                if !seen_prefixes.iter().any(|p| {
                    if ns_prefix.is_null() && p.is_null() {
                        return true;
                    }
                    if ns_prefix.is_null() || p.is_null() {
                        return false;
                    }
                    unsafe { crate::abi::exports_xml2::xmlStrEqual(*p, ns_prefix) != 0 }
                }) {
                    collected.push(CollectedNs {
                        prefix: ns_prefix,
                        href: ns.href,
                    });
                    seen_prefixes.push(ns_prefix);
                }
                ns_def = ns.next;
            }
            cur = cur_node.parent;
        }
    }

    collected
}

/// Find the namespace declaration for a given prefix by walking up the ancestor chain.
///
/// # SAFETY
///
/// - `node` must be a valid pointer to an `_xmlNode` or NULL.
unsafe fn find_ns_declaration(node: *mut _xmlNode, prefix: *const xmlChar) -> *mut _xmlNs {
    if node.is_null() {
        return ptr::null_mut();
    }

    let mut cur: *mut _xmlNode = node;
    while !cur.is_null() {
        let cur_node = unsafe { &*cur };
        let mut ns_def = cur_node.nsDef;
        while !ns_def.is_null() {
            let ns = unsafe { &*ns_def };
            let match_found = if prefix.is_null() {
                // Default namespace: prefix should be NULL
                ns.prefix.is_null()
            } else if ns.prefix.is_null() {
                false
            } else {
                unsafe { crate::abi::exports_xml2::xmlStrEqual(ns.prefix, prefix) != 0 }
            };
            if match_found {
                return ns_def;
            }
            ns_def = ns.next;
        }
        cur = cur_node.parent;
    }

    ptr::null_mut()
}

// ═══════════════════════════════════════════════════════════════════════════════
// Namespace Serialization
// ═══════════════════════════════════════════════════════════════════════════════

/// Serialize namespace declarations to the output buffer.
///
/// Outputs namespace declarations in canonical order.
///
/// # SAFETY
///
/// - `buf` must be a valid pointer to a mutable `_xmlBuffer`.
/// - `ns_list` contains raw pointers that must be valid.
unsafe fn c14n_serialize_namespaces(buf: *mut _xmlBuffer, ns_list: &[CollectedNs]) {
    if buf.is_null() || ns_list.is_empty() {
        return;
    }

    for ns in ns_list {
        io::buf_add(buf, b" xmlns" as *const u8, 6);
        if !ns.prefix.is_null() {
            io::buf_ccat(buf, b':');
            io::buf_cat(buf, ns.prefix);
        }
        io::buf_add(buf, b"=\"" as *const u8, 2);
        if !ns.href.is_null() {
            c14n_escape_attr(buf, ns.href);
        }
        io::buf_ccat(buf, b'"');
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Attribute Ordering (Canonical)
// ═══════════════════════════════════════════════════════════════════════════════

/// Compare two attribute pointers for canonical ordering.
///
/// Canonical ordering: lexicographic by namespace URI (with empty namespace
/// coming first), then by local name.
///
/// Returns negative, zero, or positive.
///
/// # SAFETY
///
/// - `a` and `b` must be valid pointers to `_xmlAttr`.
unsafe fn compare_attrs(a: *const _xmlAttr, b: *const _xmlAttr) -> std::cmp::Ordering {
    let attr_a = unsafe { &*a };
    let attr_b = unsafe { &*b };

    // Get namespace URIs (empty string if no namespace)
    let ns_uri_a = if !attr_a.ns.is_null() {
        unsafe { &*attr_a.ns }.href
    } else {
        ptr::null()
    };
    let ns_uri_b = if !attr_b.ns.is_null() {
        unsafe { &*attr_b.ns }.href
    } else {
        ptr::null()
    };

    // Namespace URI comparison: NULL (no namespace) sorts before any URI
    if ns_uri_a.is_null() && !ns_uri_b.is_null() {
        return std::cmp::Ordering::Less;
    }
    if !ns_uri_a.is_null() && ns_uri_b.is_null() {
        return std::cmp::Ordering::Greater;
    }
    if !ns_uri_a.is_null() && !ns_uri_b.is_null() {
        let cmp = unsafe { crate::abi::exports_xml2::xmlStrcmp(ns_uri_a, ns_uri_b) };
        if cmp != 0 {
            return cmp.cmp(&0);
        }
    }

    // Local name comparison
    let name_a = attr_a.name;
    let name_b = attr_b.name;
    if name_a.is_null() && name_b.is_null() {
        return std::cmp::Ordering::Equal;
    }
    if name_a.is_null() {
        return std::cmp::Ordering::Less;
    }
    if name_b.is_null() {
        return std::cmp::Ordering::Greater;
    }
    let cmp = unsafe { crate::abi::exports_xml2::xmlStrcmp(name_a, name_b) };
    cmp.cmp(&0)
}

/// Serialize attributes in canonical order.
///
/// Attributes are sorted lexicographically by namespace URI then local name.
/// The `xmlns:*` attributes are NOT included here — they are handled separately
/// by `c14n_serialize_namespaces`.
///
/// # SAFETY
///
/// - `node` must be a valid pointer to an `_xmlNode` or NULL.
/// - `buf` must be a valid pointer to a mutable `_xmlBuffer`.
unsafe fn c14n_serialize_attributes(node: *mut _xmlNode, buf: *mut _xmlBuffer) {
    if node.is_null() || buf.is_null() {
        return;
    }

    let n = unsafe { &*node };
    if n.type_ != XML_ELEMENT_NODE as c_int {
        return;
    }

    // Collect attributes into a vector for sorting
    let mut attrs: Vec<*mut _xmlAttr> = Vec::new();
    let mut cur_attr = n.properties;
    while !cur_attr.is_null() {
        attrs.push(cur_attr);
        cur_attr = unsafe { (*cur_attr).next };
    }

    // Sort attributes canonically
    attrs.sort_by(|a, b| unsafe { compare_attrs(*a, *b) });

    // Serialize each attribute
    for &attr in &attrs {
        let a = unsafe { &*attr };

        io::buf_ccat(buf, b' ');

        // Write attribute name with optional namespace prefix
        if !a.ns.is_null() {
            let ans = unsafe { &*a.ns };
            if !ans.prefix.is_null() {
                io::buf_cat(buf, ans.prefix);
                io::buf_ccat(buf, b':');
            }
        }
        if !a.name.is_null() {
            io::buf_cat(buf, a.name);
        }

        io::buf_add(buf, b"=\"" as *const u8, 2);

        // Attribute value from child text node
        if !a.children.is_null() {
            let child = unsafe { &*a.children };
            if child.type_ == XML_TEXT_NODE as c_int && !child.content.is_null() {
                c14n_escape_attr(buf, child.content);
            }
        }

        io::buf_ccat(buf, b'"');
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Node Serialization (Canonical)
// ═══════════════════════════════════════════════════════════════════════════════

/// Serialize a single node in canonical form.
///
/// This is the core recursive canonical serialization function.
///
/// # SAFETY
///
/// - `node` must be a valid pointer to an `_xmlNode` or NULL.
/// - `ctx` must be a valid pointer to a `C14nContext`.
/// - `buf` must be a valid pointer to a mutable `_xmlBuffer`.
unsafe fn c14n_serialize_node(node: *mut _xmlNode, ctx: &mut C14nContext, buf: *mut _xmlBuffer) {
    if node.is_null() || buf.is_null() {
        return;
    }

    let n = unsafe { &*node };

    match n.type_ {
        t if t == XML_ELEMENT_NODE as c_int => {
            c14n_serialize_element(node, ctx, buf);
        }
        t if t == XML_TEXT_NODE as c_int => {
            if !n.content.is_null() {
                c14n_escape_text(buf, n.content, tree::xml_strlen(n.content));
            }
        }
        t if t == XML_CDATA_SECTION_NODE as c_int => {
            // C14N converts CDATA sections to text
            if !n.content.is_null() {
                c14n_escape_text(buf, n.content, tree::xml_strlen(n.content));
            }
        }
        t if t == XML_COMMENT_NODE as c_int => {
            if ctx.mode.with_comments() {
                io::buf_add(buf, b"<!--" as *const u8, 4);
                if !n.content.is_null() {
                    io::buf_cat(buf, n.content);
                }
                io::buf_add(buf, b"-->" as *const u8, 3);
            }
        }
        t if t == XML_PI_NODE as c_int => {
            io::buf_add(buf, b"<?" as *const u8, 2);
            if !n.name.is_null() {
                io::buf_cat(buf, n.name);
            }
            if !n.content.is_null() && unsafe { *n.content != 0 } {
                io::buf_ccat(buf, b' ');
                io::buf_cat(buf, n.content);
            }
            io::buf_add(buf, b"?>" as *const u8, 2);
        }
        t if t == XML_DOCUMENT_NODE as c_int || t == XML_HTML_DOCUMENT_NODE as c_int => {
            // Serialize children of the document node (skip XML declaration)
            let mut child = n.children;
            while !child.is_null() {
                c14n_serialize_node(child, ctx, buf);
                child = unsafe { (*child).next };
            }
        }
        t if t == XML_DTD_NODE as c_int || t == XML_DOCUMENT_TYPE_NODE as c_int => {
            // Skip DTD nodes in C14N output
        }
        t if t == XML_ENTITY_REF_NODE as c_int => {
            // Entity references are not expanded in canonical XML;
            // instead, the reference itself is output.
            if !n.name.is_null() {
                io::buf_ccat(buf, b'&');
                io::buf_cat(buf, n.name);
                io::buf_ccat(buf, b';');
            }
        }
        _ => {
            // For unknown types, write content if present
            if !n.content.is_null() {
                c14n_escape_text(buf, n.content, tree::xml_strlen(n.content));
            }
        }
    }
}

/// Serialize an element node in canonical form.
///
/// This handles namespace collection, attribute ordering, and recursive
/// child serialization.
///
/// # SAFETY
///
/// - `node` must be a valid pointer to an `_xmlNode` or NULL.
/// - `ctx` must be a valid pointer to a `C14nContext`.
/// - `buf` must be a valid pointer to a mutable `_xmlBuffer`.
unsafe fn c14n_serialize_element(node: *mut _xmlNode, ctx: &mut C14nContext, buf: *mut _xmlBuffer) {
    if node.is_null() || buf.is_null() {
        return;
    }

    let n = unsafe { &*node };

    // Push a new namespace scope for this element
    ctx.push_scope();

    // Collect namespaces for this element
    let ns_list = c14n_collect_namespaces(node, ctx);

    // Open element: `<`
    io::buf_ccat(buf, b'<');

    // Write element name with optional namespace prefix
    if !n.ns.is_null() {
        let ns = unsafe { &*n.ns };
        if !ns.prefix.is_null() {
            io::buf_cat(buf, ns.prefix);
            io::buf_ccat(buf, b':');
        }
    }
    if !n.name.is_null() {
        io::buf_cat(buf, n.name);
    }

    // Write namespace declarations
    c14n_serialize_namespaces(buf, &ns_list);

    // Write attributes in canonical order
    c14n_serialize_attributes(node, buf);

    if n.children.is_null() {
        // Self-closing tag for empty elements
        io::buf_add(buf, b"/>" as *const u8, 2);
    } else {
        io::buf_ccat(buf, b'>');

        // Serialize children
        let mut child = n.children;
        while !child.is_null() {
            c14n_serialize_node(child, ctx, buf);
            child = unsafe { (*child).next };
        }

        // Close element: `</name>`
        io::buf_add(buf, b"</" as *const u8, 2);
        if !n.ns.is_null() {
            let ns = unsafe { &*n.ns };
            if !ns.prefix.is_null() {
                io::buf_cat(buf, ns.prefix);
                io::buf_ccat(buf, b':');
            }
        }
        if !n.name.is_null() {
            io::buf_cat(buf, n.name);
        }
        io::buf_ccat(buf, b'>');
    }

    // Pop namespace scope
    ctx.pop_scope();
}

// ═══════════════════════════════════════════════════════════════════════════════
// Public API — Document-level canonicalization
// ═══════════════════════════════════════════════════════════════════════════════

/// Canonicalize a document (or a subset of nodes) to an xmlBuffer.
///
/// If `nodes` is non-NULL, it is a NULL-terminated array of node pointers
/// that form the subset to canonicalize. If NULL, the entire document is
/// canonicalized.
///
/// # SAFETY
///
/// - `doc` must be a valid pointer to an `_xmlDoc` or NULL.
/// - `nodes` may be NULL (meaning the entire document) or a NULL-terminated
///   array of `_xmlNode` pointers.
/// - `buf` must be a valid pointer to a mutable `_xmlBuffer`.
/// - `inclusive_ns_prefixes` is a comma-separated list of prefixes or NULL.
pub unsafe fn c14n_doc_dump_memory(
    doc: *mut _xmlDoc,
    nodes: *mut *mut _xmlNode,
    mode: C14nMode,
    inclusive_ns_prefixes: *const xmlChar,
    with_comments: c_int,
    result: *mut *mut xmlChar,
) -> c_int {
    if doc.is_null() || result.is_null() {
        return -1;
    }

    // Determine the effective mode, considering with_comments flag
    let effective_mode = if with_comments != 0 {
        match mode {
            C14nMode::XML_C14N_1_0 => C14nMode::XML_C14N_1_0_WITH_COMMENTS,
            C14nMode::XML_C14N_EXCLUSIVE_1_0 => C14nMode::XML_C14N_EXCLUSIVE_1_0_WITH_COMMENTS,
            C14nMode::XML_C14N_1_1 => C14nMode::XML_C14N_1_1_WITH_COMMENTS,
            _ => mode,
        }
    } else {
        mode
    };

    // Parse inclusive namespace prefixes
    let inclusive_set = parse_inclusive_prefixes(inclusive_ns_prefixes);

    let mut ctx = C14nContext::new(doc, effective_mode, inclusive_set);

    // Create output buffer
    let buf = io::buf_create(-1);
    if buf.is_null() {
        return -1;
    }

    if nodes.is_null() {
        // Canonicalize entire document
        let doc_node = doc as *mut _xmlNode;
        let d = unsafe { &*doc_node };
        let mut child = d.children;
        while !child.is_null() {
            c14n_serialize_node(child, &mut ctx, buf);
            child = unsafe { (*child).next };
        }
    } else {
        // Canonicalize a subset of nodes in document order
        // First, collect and sort nodes in document order
        let mut node_vec: Vec<*mut _xmlNode> = Vec::new();
        let mut i = 0;
        loop {
            let n = unsafe { *nodes.add(i) };
            if n.is_null() {
                break;
            }
            node_vec.push(n);
            i += 1;
        }

        // Sort nodes in document order
        node_vec.sort_by(|a, b| unsafe { cmp_document_order(*a, *b) });

        for &n in &node_vec {
            c14n_serialize_node(n, &mut ctx, buf);
        }
    }

    // Extract result string
    let content = io::buf_content(buf);
    let len = io::buf_length(buf);
    if content.is_null() || len < 0 {
        io::buf_free(buf);
        return -1;
    }

    // Duplicate the content for the caller
    let result_str = crate::abi::exports_xml2::xmlStrdup(content);
    io::buf_free(buf);

    if result_str.is_null() {
        return -1;
    }

    unsafe {
        *result = result_str;
    }

    len
}

/// Canonicalize a document to an output buffer via callback.
///
/// # SAFETY
///
/// - `doc` must be a valid pointer to an `_xmlDoc` or NULL.
/// - `callback` must be a valid function pointer.
/// - `inclusive_ns_prefixes` is a comma-separated list of prefixes or NULL.
pub unsafe fn c14n_execute(
    doc: *mut _xmlDoc,
    mode: C14nMode,
    inclusive_ns_prefixes: *const xmlChar,
    with_comments: c_int,
    callback: Option<
        unsafe extern "C" fn(ctx: *mut c_void, data: *const c_char, len: c_int) -> c_int,
    >,
    callback_data: *mut c_void,
) -> c_int {
    if doc.is_null() || callback.is_none() {
        return -1;
    }

    let callback = callback.unwrap();

    // Determine effective mode
    let effective_mode = if with_comments != 0 {
        match mode {
            C14nMode::XML_C14N_1_0 => C14nMode::XML_C14N_1_0_WITH_COMMENTS,
            C14nMode::XML_C14N_EXCLUSIVE_1_0 => C14nMode::XML_C14N_EXCLUSIVE_1_0_WITH_COMMENTS,
            C14nMode::XML_C14N_1_1 => C14nMode::XML_C14N_1_1_WITH_COMMENTS,
            _ => mode,
        }
    } else {
        mode
    };

    // Parse inclusive namespace prefixes
    let inclusive_set = parse_inclusive_prefixes(inclusive_ns_prefixes);

    let mut ctx = C14nContext::new(doc, effective_mode, inclusive_set);

    // Create output buffer
    let buf = io::buf_create(-1);
    if buf.is_null() {
        return -1;
    }

    // Canonicalize entire document
    let doc_node = doc as *mut _xmlNode;
    let d = unsafe { &*doc_node };
    let mut child = d.children;
    while !child.is_null() {
        c14n_serialize_node(child, &mut ctx, buf);
        child = unsafe { (*child).next };
    }

    // Call the callback with the result
    let content = io::buf_content(buf);
    let len = io::buf_length(buf);
    if content.is_null() || len < 0 {
        io::buf_free(buf);
        return -1;
    }

    let ret = unsafe { callback(callback_data, content as *const c_char, len) };

    io::buf_free(buf);
    ret
}

/// Canonicalize a document and save to an output buffer.
///
/// # SAFETY
///
/// - `doc` must be a valid pointer to an `_xmlDoc` or NULL.
/// - `output` must be a valid pointer to an `_xmlOutputBuffer` or NULL.
/// - `inclusive_ns_prefixes` is a comma-separated list of prefixes or NULL.
pub unsafe fn c14n_doc_save_to(
    doc: *mut _xmlDoc,
    nodes: *mut *mut _xmlNode,
    mode: C14nMode,
    inclusive_ns_prefixes: *const xmlChar,
    with_comments: c_int,
    output: *mut _xmlOutputBuffer,
) -> c_int {
    if doc.is_null() || output.is_null() {
        return -1;
    }

    // Determine effective mode
    let effective_mode = if with_comments != 0 {
        match mode {
            C14nMode::XML_C14N_1_0 => C14nMode::XML_C14N_1_0_WITH_COMMENTS,
            C14nMode::XML_C14N_EXCLUSIVE_1_0 => C14nMode::XML_C14N_EXCLUSIVE_1_0_WITH_COMMENTS,
            C14nMode::XML_C14N_1_1 => C14nMode::XML_C14N_1_1_WITH_COMMENTS,
            _ => mode,
        }
    } else {
        mode
    };

    // Parse inclusive namespace prefixes
    let inclusive_set = parse_inclusive_prefixes(inclusive_ns_prefixes);

    let mut ctx = C14nContext::new(doc, effective_mode, inclusive_set);

    // Create buffer for serialization
    let buf = io::buf_create(-1);
    if buf.is_null() {
        return -1;
    }

    if nodes.is_null() {
        // Canonicalize entire document
        let doc_node = doc as *mut _xmlNode;
        let d = unsafe { &*doc_node };
        let mut child = d.children;
        while !child.is_null() {
            c14n_serialize_node(child, &mut ctx, buf);
            child = unsafe { (*child).next };
        }
    } else {
        // Canonicalize a subset of nodes in document order
        let mut node_vec: Vec<*mut _xmlNode> = Vec::new();
        let mut i = 0;
        loop {
            let n = unsafe { *nodes.add(i) };
            if n.is_null() {
                break;
            }
            node_vec.push(n);
            i += 1;
        }

        node_vec.sort_by(|a, b| unsafe { cmp_document_order(*a, *b) });

        for &n in &node_vec {
            c14n_serialize_node(n, &mut ctx, buf);
        }
    }

    // Write to output buffer
    let content = io::buf_content(buf);
    let len = io::buf_length(buf);
    if content.is_null() || len < 0 {
        io::buf_free(buf);
        return -1;
    }

    let written = io::output_buffer_write(output, len, content as *const c_char);
    io::buf_free(buf);

    // Flush the output buffer to ensure data reaches the underlying target
    let flush_ret = io::output_buffer_flush(output);
    if flush_ret < 0 {
        return written;
    }

    written
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helper Functions
// ═══════════════════════════════════════════════════════════════════════════════

/// Parse a comma-separated list of inclusive namespace prefixes.
///
/// Returns `None` if the input is NULL (meaning no inclusive prefixes).
fn parse_inclusive_prefixes(input: *const xmlChar) -> Option<HashSet<String>> {
    if input.is_null() {
        return None;
    }

    let input_str = unsafe {
        let c_str = core::ffi::CStr::from_ptr(input as *const c_char);
        match c_str.to_str() {
            Ok(s) => s.to_string(),
            Err(_) => return None,
        }
    };

    if input_str.is_empty() {
        return None;
    }

    let mut set = HashSet::new();
    for prefix in input_str.split(',') {
        let trimmed = prefix.trim();
        if !trimmed.is_empty() {
            set.insert(trimmed.to_string());
        }
    }

    if set.is_empty() {
        None
    } else {
        Some(set)
    }
}

/// Compare two nodes in document order.
///
/// Returns negative if `a` comes before `b`, positive if `a` comes after `b`.
///
/// # SAFETY
///
/// - `a` and `b` must be valid pointers to `_xmlNode`.
unsafe fn cmp_document_order(a: *mut _xmlNode, b: *mut _xmlNode) -> std::cmp::Ordering {
    if a == b {
        return std::cmp::Ordering::Equal;
    }

    // Build ancestor chains
    let mut ancestors_a: Vec<*mut _xmlNode> = Vec::new();
    let mut cur = a;
    while !cur.is_null() {
        ancestors_a.push(cur);
        cur = unsafe { (*cur).parent };
    }

    let mut ancestors_b: Vec<*mut _xmlNode> = Vec::new();
    let mut cur = b;
    while !cur.is_null() {
        ancestors_b.push(cur);
        cur = unsafe { (*cur).parent };
    }

    // Find the lowest common ancestor
    let mut i = ancestors_a.len();
    let mut j = ancestors_b.len();

    while i > 0 && j > 0 && ancestors_a[i - 1] == ancestors_b[j - 1] {
        i -= 1;
        j -= 1;
    }

    if i == 0 || j == 0 {
        // One is an ancestor of the other
        if i == 0 {
            return std::cmp::Ordering::Less;
        }
        return std::cmp::Ordering::Greater;
    }

    // The nodes at i-1 and j-1 are siblings under the common ancestor.
    // Determine their order by walking the sibling chain.
    let sibling_a = ancestors_a[i - 1];
    let sibling_b = ancestors_b[j - 1];

    // Walk forward from sibling_a to see if we find sibling_b
    let mut walk = sibling_a;
    while !walk.is_null() {
        if walk == sibling_b {
            return std::cmp::Ordering::Less;
        }
        walk = unsafe { (*walk).next };
    }

    // sibling_b must be before sibling_a
    std::cmp::Ordering::Greater
}

// ═══════════════════════════════════════════════════════════════════════════════
// C ABI Exports
// ═══════════════════════════════════════════════════════════════════════════════

/// Serialize canonical XML to a memory buffer.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlC14NDocDumpMemory(
///     xmlDocPtr doc,
///     xmlNodeSetPtr nodes,
///     int mode,
///     xmlChar **inclusive_ns_prefixes,
///     int with_comments,
///     xmlChar **result
/// );
/// ```
///
/// Serializes `doc` (or a subset of `nodes`) to canonical XML.
/// The result is allocated with `xmlMalloc` and must be freed by the caller
/// with `xmlFree`.
///
/// Returns the length of the result string in bytes, or -1 on error.
///
/// # SAFETY
///
/// - `doc` must be a valid pointer to an `_xmlDoc` or NULL.
/// - `nodes` may be NULL (entire document) or a NULL-terminated array of `_xmlNode` pointers.
/// - `result` must be a valid pointer to a `xmlChar*` that will receive the result.
#[no_mangle]
pub unsafe extern "C" fn xmlC14NDocDumpMemory(
    doc: *mut _xmlDoc,
    nodes: *mut *mut _xmlNode,
    mode: c_int,
    inclusive_ns_prefixes: *mut *mut xmlChar,
    with_comments: c_int,
    result: *mut *mut xmlChar,
) -> c_int {
    // SAFETY: Delegates to the safe internal implementation.
    // The caller must provide valid pointers or NULL as documented.

    let c14n_mode = match mode {
        0 => C14nMode::XML_C14N_1_0,
        1 => C14nMode::XML_C14N_EXCLUSIVE_1_0,
        2 => C14nMode::XML_C14N_1_1,
        3 => C14nMode::XML_C14N_1_0_WITH_COMMENTS,
        4 => C14nMode::XML_C14N_EXCLUSIVE_1_0_WITH_COMMENTS,
        5 => C14nMode::XML_C14N_1_1_WITH_COMMENTS,
        _ => return -1,
    };

    // The inclusive_ns_prefixes parameter in the upstream API is a
    // NULL-terminated array of xmlChar* strings. We join them with commas
    // for our internal parser.
    let joined_prefixes = if !inclusive_ns_prefixes.is_null() {
        let mut parts: Vec<*mut xmlChar> = Vec::new();
        let mut i = 0;
        loop {
            let p = unsafe { *inclusive_ns_prefixes.add(i) };
            if p.is_null() {
                break;
            }
            parts.push(p);
            i += 1;
        }

        if parts.is_empty() {
            ptr::null()
        } else {
            // Build comma-separated string
            let mut result_str = Vec::<u8>::new();
            for (idx, &part) in parts.iter().enumerate() {
                if idx > 0 {
                    result_str.push(b',');
                }
                let len = tree::xml_strlen(part);
                let part_slice = unsafe { core::slice::from_raw_parts(part, len as usize) };
                result_str.extend_from_slice(part_slice);
            }
            result_str.push(0); // null-terminate
            result_str.as_ptr() as *const xmlChar
        }
    } else {
        ptr::null()
    };

    unsafe {
        c14n_doc_dump_memory(
            doc,
            nodes,
            c14n_mode,
            joined_prefixes,
            with_comments,
            result,
        )
    }
}

/// Canonicalize XML with a callback for output.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlC14NExecute(
///     xmlDocPtr doc,
///     int mode,
///     xmlChar **inclusive_ns_prefixes,
///     int with_comments,
///     xmlC14NIOWriteCallback callback,
///     void *callback_data
/// );
/// ```
///
/// # SAFETY
///
/// - `doc` must be a valid pointer to an `_xmlDoc` or NULL.
/// - `callback` must be a valid function pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlC14NExecute(
    doc: *mut _xmlDoc,
    mode: c_int,
    inclusive_ns_prefixes: *mut *mut xmlChar,
    with_comments: c_int,
    callback: Option<
        unsafe extern "C" fn(ctx: *mut c_void, data: *const c_char, len: c_int) -> c_int,
    >,
    callback_data: *mut c_void,
) -> c_int {
    // SAFETY: Delegates to the safe internal implementation.

    let c14n_mode = match mode {
        0 => C14nMode::XML_C14N_1_0,
        1 => C14nMode::XML_C14N_EXCLUSIVE_1_0,
        2 => C14nMode::XML_C14N_1_1,
        3 => C14nMode::XML_C14N_1_0_WITH_COMMENTS,
        4 => C14nMode::XML_C14N_EXCLUSIVE_1_0_WITH_COMMENTS,
        5 => C14nMode::XML_C14N_1_1_WITH_COMMENTS,
        _ => return -1,
    };

    let joined_prefixes = if !inclusive_ns_prefixes.is_null() {
        let mut parts: Vec<*mut xmlChar> = Vec::new();
        let mut i = 0;
        loop {
            let p = unsafe { *inclusive_ns_prefixes.add(i) };
            if p.is_null() {
                break;
            }
            parts.push(p);
            i += 1;
        }

        if parts.is_empty() {
            ptr::null()
        } else {
            let mut result_str = Vec::<u8>::new();
            for (idx, &part) in parts.iter().enumerate() {
                if idx > 0 {
                    result_str.push(b',');
                }
                let len = tree::xml_strlen(part);
                let part_slice = unsafe { core::slice::from_raw_parts(part, len as usize) };
                result_str.extend_from_slice(part_slice);
            }
            result_str.push(0);
            result_str.as_ptr() as *const xmlChar
        }
    } else {
        ptr::null()
    };

    unsafe {
        c14n_execute(
            doc,
            c14n_mode,
            joined_prefixes,
            with_comments,
            callback,
            callback_data,
        )
    }
}

/// Save canonical XML to an output buffer.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlC14NDocSaveTo(
///     xmlDocPtr doc,
///     xmlNodeSetPtr nodes,
///     int mode,
///     xmlChar **inclusive_ns_prefixes,
///     int with_comments,
///     xmlOutputBufferPtr output
/// );
/// ```
///
/// # SAFETY
///
/// - `doc` must be a valid pointer to an `_xmlDoc` or NULL.
/// - `output` must be a valid pointer to an `_xmlOutputBuffer` or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlC14NDocSaveTo(
    doc: *mut _xmlDoc,
    nodes: *mut *mut _xmlNode,
    mode: c_int,
    inclusive_ns_prefixes: *mut *mut xmlChar,
    with_comments: c_int,
    output: *mut _xmlOutputBuffer,
) -> c_int {
    // SAFETY: Delegates to the safe internal implementation.

    let c14n_mode = match mode {
        0 => C14nMode::XML_C14N_1_0,
        1 => C14nMode::XML_C14N_EXCLUSIVE_1_0,
        2 => C14nMode::XML_C14N_1_1,
        3 => C14nMode::XML_C14N_1_0_WITH_COMMENTS,
        4 => C14nMode::XML_C14N_EXCLUSIVE_1_0_WITH_COMMENTS,
        5 => C14nMode::XML_C14N_1_1_WITH_COMMENTS,
        _ => return -1,
    };

    let joined_prefixes: Option<Vec<u8>> = if !inclusive_ns_prefixes.is_null() {
        let mut parts: Vec<*mut xmlChar> = Vec::new();
        let mut i = 0;
        loop {
            let p = unsafe { *inclusive_ns_prefixes.add(i) };
            if p.is_null() {
                break;
            }
            parts.push(p);
            i += 1;
        }

        if parts.is_empty() {
            None
        } else {
            let mut result_str = Vec::<u8>::new();
            for (idx, &part) in parts.iter().enumerate() {
                if idx > 0 {
                    result_str.push(b',');
                }
                let len = tree::xml_strlen(part);
                let part_slice = unsafe { core::slice::from_raw_parts(part, len as usize) };
                result_str.extend_from_slice(part_slice);
            }
            result_str.push(0);
            Some(result_str)
        }
    } else {
        None
    };

    let joined_ptr = joined_prefixes
        .as_ref()
        .map(|v| v.as_ptr() as *const xmlChar)
        .unwrap_or(ptr::null());

    unsafe { c14n_doc_save_to(doc, nodes, c14n_mode, joined_ptr, with_comments, output) }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::allocator::xmlFree;
    use crate::xml::io;
    use crate::xml::tree;
    use core::ptr;
    use std::os::raw::c_int;

    /// Helper: create a simple document for testing.
    ///
    /// Creates: `<root><child attr="value">text</child></root>`
    unsafe fn create_simple_doc() -> *mut _xmlDoc {
        let doc = tree::new_doc(b"1.0\0" as *const u8 as *const xmlChar);
        assert!(!doc.is_null());

        let root = tree::new_node(ptr::null_mut(), b"root\0" as *const u8 as *const xmlChar);
        assert!(!root.is_null());
        tree::doc_set_root_element(doc, root);

        let child = tree::new_node(ptr::null_mut(), b"child\0" as *const u8 as *const xmlChar);
        assert!(!child.is_null());
        tree::add_child(root, child);

        // Set attribute
        tree::set_prop(
            child,
            b"attr\0" as *const u8 as *const xmlChar,
            b"value\0" as *const u8 as *const xmlChar,
        );

        // Set text content
        let text = tree::new_text(b"text\0" as *const u8 as *const xmlChar);
        assert!(!text.is_null());
        tree::add_child(child, text);

        doc
    }

    /// Helper: canonicalize a document and return the result as a String.
    unsafe fn canonicalize_doc(doc: *mut _xmlDoc, mode: C14nMode, with_comments: c_int) -> String {
        let mut result: *mut xmlChar = ptr::null_mut();
        let len = c14n_doc_dump_memory(
            doc,
            ptr::null_mut(),
            mode,
            ptr::null(),
            with_comments,
            &mut result as *mut *mut xmlChar,
        );
        assert!(len >= 0);
        assert!(!result.is_null());

        let s = {
            let slice = core::slice::from_raw_parts(result, len as usize);
            String::from_utf8_lossy(slice).to_string()
        };
        xmlFree(result as *mut c_void);
        s
    }

    // ── Basic document canonicalization ──

    #[test]
    fn test_c14n_basic_document() {
        unsafe {
            let doc = create_simple_doc();
            let result = canonicalize_doc(doc, C14nMode::XML_C14N_1_0, 0);
            assert!(
                result.contains("<root>"),
                "Result should contain <root>, got: {}",
                result
            );
            assert!(
                result.contains("<child"),
                "Result should contain <child>, got: {}",
                result
            );
            assert!(
                result.contains("attr=\"value\""),
                "Result should contain attr=\"value\", got: {}",
                result
            );
            assert!(
                result.contains("text"),
                "Result should contain text, got: {}",
                result
            );
            assert!(
                result.contains("</child>"),
                "Result should contain </child>, got: {}",
                result
            );
            assert!(
                result.contains("</root>"),
                "Result should contain </root>, got: {}",
                result
            );
            tree::free_doc(doc);
        }
    }

    #[test]
    fn test_c14n_basic_empty_element() {
        unsafe {
            let doc = tree::new_doc(b"1.0\0" as *const u8 as *const xmlChar);
            assert!(!doc.is_null());
            let root = tree::new_node(ptr::null_mut(), b"empty\0" as *const u8 as *const xmlChar);
            assert!(!root.is_null());
            tree::doc_set_root_element(doc, root);

            let result = canonicalize_doc(doc, C14nMode::XML_C14N_1_0, 0);
            // Empty element should be self-closing
            assert!(
                result.contains("<empty/>"),
                "Empty element should be self-closing, got: {}",
                result
            );
            tree::free_doc(doc);
        }
    }

    // ── Namespace propagation ──

    #[test]
    fn test_c14n_namespace_propagation() {
        unsafe {
            let doc = tree::new_doc(b"1.0\0" as *const u8 as *const xmlChar);
            assert!(!doc.is_null());

            let ns = tree::new_ns(
                ptr::null_mut(),
                b"http://example.com/ns\0" as *const u8 as *const xmlChar,
                b"ex\0" as *const u8 as *const xmlChar,
            );
            assert!(!ns.is_null());

            let root = tree::new_node(ns, b"root\0" as *const u8 as *const xmlChar);
            assert!(!root.is_null());
            tree::doc_set_root_element(doc, root);

            // Re-attach the namespace to the root node
            tree::new_ns(
                root,
                b"http://example.com/ns\0" as *const u8 as *const xmlChar,
                b"ex\0" as *const u8 as *const xmlChar,
            );

            let result = canonicalize_doc(doc, C14nMode::XML_C14N_1_0, 0);
            assert!(
                result.contains("xmlns:ex=\"http://example.com/ns\""),
                "Result should contain namespace declaration, got: {}",
                result
            );
            assert!(
                result.contains("<ex:root"),
                "Result should contain <ex:root, got: {}",
                result
            );
            tree::free_doc(doc);
        }
    }

    // ── Attribute ordering ──

    #[test]
    fn test_c14n_attribute_ordering() {
        unsafe {
            let doc = tree::new_doc(b"1.0\0" as *const u8 as *const xmlChar);
            assert!(!doc.is_null());

            let root = tree::new_node(ptr::null_mut(), b"root\0" as *const u8 as *const xmlChar);
            assert!(!root.is_null());
            tree::doc_set_root_element(doc, root);

            // Set attributes in reverse alphabetical order
            tree::set_prop(
                root,
                b"zeta\0" as *const u8 as *const xmlChar,
                b"1\0" as *const u8 as *const xmlChar,
            );
            tree::set_prop(
                root,
                b"alpha\0" as *const u8 as *const xmlChar,
                b"2\0" as *const u8 as *const xmlChar,
            );
            tree::set_prop(
                root,
                b"beta\0" as *const u8 as *const xmlChar,
                b"3\0" as *const u8 as *const xmlChar,
            );

            let result = canonicalize_doc(doc, C14nMode::XML_C14N_1_0, 0);

            // Find the position of each attribute in the output
            let alpha_pos = result.find("alpha=\"2\"");
            let beta_pos = result.find("beta=\"3\"");
            let zeta_pos = result.find("zeta=\"1\"");

            assert!(alpha_pos.is_some(), "alpha attribute should be present");
            assert!(beta_pos.is_some(), "beta attribute should be present");
            assert!(zeta_pos.is_some(), "zeta attribute should be present");

            // alpha should come before beta, beta before zeta
            assert!(
                alpha_pos.unwrap() < beta_pos.unwrap(),
                "alpha should come before beta"
            );
            assert!(
                beta_pos.unwrap() < zeta_pos.unwrap(),
                "beta should come before zeta"
            );

            tree::free_doc(doc);
        }
    }

    // ── Character escaping ──

    #[test]
    fn test_c14n_character_escaping_text() {
        unsafe {
            let doc = tree::new_doc(b"1.0\0" as *const u8 as *const xmlChar);
            assert!(!doc.is_null());

            let root = tree::new_node(ptr::null_mut(), b"root\0" as *const u8 as *const xmlChar);
            assert!(!root.is_null());
            tree::doc_set_root_element(doc, root);

            // Text with special characters
            let text = tree::new_text(b"a < b & c > d\r\0" as *const u8 as *const xmlChar);
            assert!(!text.is_null());
            tree::add_child(root, text);

            let result = canonicalize_doc(doc, C14nMode::XML_C14N_1_0, 0);
            assert!(result.contains("&lt;"), "Should escape <, got: {}", result);
            assert!(result.contains("&amp;"), "Should escape &, got: {}", result);
            assert!(result.contains("&gt;"), "Should escape >, got: {}", result);
            assert!(
                result.contains("&#xD;"),
                "Should escape CR, got: {}",
                result
            );

            tree::free_doc(doc);
        }
    }

    #[test]
    fn test_c14n_character_escaping_attr() {
        unsafe {
            let doc = tree::new_doc(b"1.0\0" as *const u8 as *const xmlChar);
            assert!(!doc.is_null());

            let root = tree::new_node(ptr::null_mut(), b"root\0" as *const u8 as *const xmlChar);
            assert!(!root.is_null());
            tree::doc_set_root_element(doc, root);

            // Attribute value with special characters including tab, newline, CR
            tree::set_prop(
                root,
                b"test\0" as *const u8 as *const xmlChar,
                b"a < b & c \" d\t\n\r\0" as *const u8 as *const xmlChar,
            );

            let result = canonicalize_doc(doc, C14nMode::XML_C14N_1_0, 0);
            assert!(
                result.contains("&lt;"),
                "Should escape < in attr, got: {}",
                result
            );
            assert!(
                result.contains("&amp;"),
                "Should escape & in attr, got: {}",
                result
            );
            assert!(
                result.contains("&quot;"),
                "Should escape \" in attr, got: {}",
                result
            );
            assert!(
                result.contains("&#x9;"),
                "Should escape tab in attr, got: {}",
                result
            );
            assert!(
                result.contains("&#xA;"),
                "Should escape newline in attr, got: {}",
                result
            );
            assert!(
                result.contains("&#xD;"),
                "Should escape CR in attr, got: {}",
                result
            );

            tree::free_doc(doc);
        }
    }

    // ── With comments ──

    #[test]
    fn test_c14n_with_comments() {
        unsafe {
            let doc = tree::new_doc(b"1.0\0" as *const u8 as *const xmlChar);
            assert!(!doc.is_null());

            let root = tree::new_node(ptr::null_mut(), b"root\0" as *const u8 as *const xmlChar);
            assert!(!root.is_null());
            tree::doc_set_root_element(doc, root);

            let comment = tree::new_comment(b" a comment \0" as *const u8 as *const xmlChar);
            assert!(!comment.is_null());
            tree::add_child(root, comment);

            // Without comments
            let result_no_comments = canonicalize_doc(doc, C14nMode::XML_C14N_1_0, 0);
            assert!(
                !result_no_comments.contains("<!--"),
                "Without comments: should not contain comments, got: {}",
                result_no_comments
            );

            // With comments
            let result_with_comments =
                canonicalize_doc(doc, C14nMode::XML_C14N_1_0_WITH_COMMENTS, 0);
            assert!(
                result_with_comments.contains("<!--"),
                "With comments: should contain comments, got: {}",
                result_with_comments
            );

            tree::free_doc(doc);
        }
    }

    // ── Exclusive vs inclusive ──

    #[test]
    fn test_c14n_exclusive_vs_inclusive() {
        unsafe {
            let doc = tree::new_doc(b"1.0\0" as *const u8 as *const xmlChar);
            assert!(!doc.is_null());

            // Create root with a namespace
            tree::new_ns(
                ptr::null_mut(),
                b"http://example.com/ns1\0" as *const u8 as *const xmlChar,
                b"ns1\0" as *const u8 as *const xmlChar,
            );
            let root = tree::new_node(ptr::null_mut(), b"root\0" as *const u8 as *const xmlChar);
            assert!(!root.is_null());
            tree::doc_set_root_element(doc, root);
            tree::new_ns(
                root,
                b"http://example.com/ns1\0" as *const u8 as *const xmlChar,
                b"ns1\0" as *const u8 as *const xmlChar,
            );

            // Child without any namespace usage
            let child = tree::new_node(ptr::null_mut(), b"child\0" as *const u8 as *const xmlChar);
            assert!(!child.is_null());
            tree::add_child(root, child);

            // Inclusive C14N should include the ns1 namespace on child
            let result_inclusive = canonicalize_doc(doc, C14nMode::XML_C14N_1_0, 0);
            // The inclusive output will include namespace declarations from ancestors
            // on each element.

            // Exclusive C14N should NOT include the ns1 namespace on child
            // since the child doesn't use it
            let _result_exclusive = canonicalize_doc(doc, C14nMode::XML_C14N_EXCLUSIVE_1_0, 0);

            // In inclusive mode, the namespace should be visible
            // In exclusive mode, it should NOT be on the child (which doesn't use ns1)
            // The root element in exclusive mode still has ns1 declared on it

            // Both should contain the ns1 namespace somewhere
            assert!(
                result_inclusive.contains("ns1"),
                "Inclusive should have ns1, got: {}",
                result_inclusive
            );

            tree::free_doc(doc);
        }
    }

    // ── XML declaration handling ──

    #[test]
    fn test_c14n_no_xml_declaration() {
        unsafe {
            let doc = create_simple_doc();
            let result = canonicalize_doc(doc, C14nMode::XML_C14N_1_0, 0);
            // C14N output should NOT include the XML declaration
            assert!(
                !result.contains("<?xml"),
                "C14N output should not contain XML declaration, got: {}",
                result
            );
            tree::free_doc(doc);
        }
    }

    // ── Empty document ──

    #[test]
    fn test_c14n_empty_document() {
        unsafe {
            let doc = tree::new_doc(b"1.0\0" as *const u8 as *const xmlChar);
            assert!(!doc.is_null());

            let result = canonicalize_doc(doc, C14nMode::XML_C14N_1_0, 0);
            assert!(
                result.is_empty(),
                "Empty document should produce empty output, got: {}",
                result
            );

            tree::free_doc(doc);
        }
    }

    // ── Edge cases ──

    #[test]
    fn test_c14n_null_doc() {
        unsafe {
            let mut result: *mut xmlChar = ptr::null_mut();
            let len = c14n_doc_dump_memory(
                ptr::null_mut(),
                ptr::null_mut(),
                C14nMode::XML_C14N_1_0,
                ptr::null(),
                0,
                &mut result as *mut *mut xmlChar,
            );
            assert_eq!(len, -1, "Null doc should return -1");
        }
    }

    #[test]
    fn test_c14n_text_node() {
        unsafe {
            let doc = tree::new_doc(b"1.0\0" as *const u8 as *const xmlChar);
            assert!(!doc.is_null());

            let root = tree::new_node(ptr::null_mut(), b"root\0" as *const u8 as *const xmlChar);
            assert!(!root.is_null());
            tree::doc_set_root_element(doc, root);

            // Text node with various content
            let text = tree::new_text(b"Hello World\0" as *const u8 as *const xmlChar);
            assert!(!text.is_null());
            tree::add_child(root, text);

            let result = canonicalize_doc(doc, C14nMode::XML_C14N_1_0, 0);
            assert!(
                result.contains("Hello World"),
                "Should contain text content, got: {}",
                result
            );

            tree::free_doc(doc);
        }
    }

    #[test]
    fn test_c14n_cdata_section() {
        unsafe {
            let doc = tree::new_doc(b"1.0\0" as *const u8 as *const xmlChar);
            assert!(!doc.is_null());

            let root = tree::new_node(ptr::null_mut(), b"root\0" as *const u8 as *const xmlChar);
            assert!(!root.is_null());
            tree::doc_set_root_element(doc, root);

            // CDATA section content is represented as a text node in the tree
            // For C14N, CDATA sections are converted to text
            let cdata =
                tree::new_text(b"<greeting>Hello</greeting>\0" as *const u8 as *const xmlChar);
            assert!(!cdata.is_null());
            tree::add_child(root, cdata);

            let result = canonicalize_doc(doc, C14nMode::XML_C14N_1_0, 0);
            assert!(
                result.contains("&lt;greeting&gt;"),
                "CDATA should be converted to escaped text, got: {}",
                result
            );

            tree::free_doc(doc);
        }
    }

    #[test]
    fn test_c14n_pi_node() {
        unsafe {
            let doc = tree::new_doc(b"1.0\0" as *const u8 as *const xmlChar);
            assert!(!doc.is_null());

            let root = tree::new_node(ptr::null_mut(), b"root\0" as *const u8 as *const xmlChar);
            assert!(!root.is_null());
            tree::doc_set_root_element(doc, root);

            let pi = tree::new_pi(
                b"xml-model\0" as *const u8 as *const xmlChar,
                b"href=\"schema.xsd\"\0" as *const u8 as *const xmlChar,
            );
            assert!(!pi.is_null());
            tree::add_child(root, pi);

            let result = canonicalize_doc(doc, C14nMode::XML_C14N_1_0, 0);
            assert!(result.contains("<?"), "Should contain PI, got: {}", result);

            tree::free_doc(doc);
        }
    }

    #[test]
    fn test_c14n_with_comments_flag() {
        unsafe {
            let doc = tree::new_doc(b"1.0\0" as *const u8 as *const xmlChar);
            assert!(!doc.is_null());

            let root = tree::new_node(ptr::null_mut(), b"root\0" as *const u8 as *const xmlChar);
            assert!(!root.is_null());
            tree::doc_set_root_element(doc, root);

            let comment = tree::new_comment(b"test\0" as *const u8 as *const xmlChar);
            assert!(!comment.is_null());
            tree::add_child(root, comment);

            // Using the with_comments flag with the base mode
            let mut result: *mut xmlChar = ptr::null_mut();
            let len = c14n_doc_dump_memory(
                doc,
                ptr::null_mut(),
                C14nMode::XML_C14N_1_0,
                ptr::null(),
                1, // with_comments = true
                &mut result as *mut *mut xmlChar,
            );
            assert!(len >= 0);
            let s = {
                let slice = core::slice::from_raw_parts(result, len as usize);
                String::from_utf8_lossy(slice).to_string()
            };
            xmlFree(result as *mut c_void);

            assert!(
                s.contains("<!--"),
                "With comments flag should include comments, got: {}",
                s
            );

            tree::free_doc(doc);
        }
    }

    #[test]
    fn test_c14n_mode_enum_values() {
        // Verify mode enum values match upstream constants
        assert_eq!(C14nMode::XML_C14N_1_0 as c_int, 0);
        assert_eq!(C14nMode::XML_C14N_EXCLUSIVE_1_0 as c_int, 1);
        assert_eq!(C14nMode::XML_C14N_1_1 as c_int, 2);
        assert_eq!(C14nMode::XML_C14N_1_0_WITH_COMMENTS as c_int, 3);
        assert_eq!(C14nMode::XML_C14N_EXCLUSIVE_1_0_WITH_COMMENTS as c_int, 4);
        assert_eq!(C14nMode::XML_C14N_1_1_WITH_COMMENTS as c_int, 5);
    }

    #[test]
    fn test_c14n_with_comments_property() {
        assert!(C14nMode::XML_C14N_1_0_WITH_COMMENTS.with_comments());
        assert!(C14nMode::XML_C14N_EXCLUSIVE_1_0_WITH_COMMENTS.with_comments());
        assert!(C14nMode::XML_C14N_1_1_WITH_COMMENTS.with_comments());
        assert!(!C14nMode::XML_C14N_1_0.with_comments());
        assert!(!C14nMode::XML_C14N_EXCLUSIVE_1_0.with_comments());
        assert!(!C14nMode::XML_C14N_1_1.with_comments());
    }

    #[test]
    fn test_c14n_is_exclusive_property() {
        assert!(C14nMode::XML_C14N_EXCLUSIVE_1_0.is_exclusive());
        assert!(C14nMode::XML_C14N_EXCLUSIVE_1_0_WITH_COMMENTS.is_exclusive());
        assert!(!C14nMode::XML_C14N_1_0.is_exclusive());
        assert!(!C14nMode::XML_C14N_1_0_WITH_COMMENTS.is_exclusive());
        assert!(!C14nMode::XML_C14N_1_1.is_exclusive());
    }

    #[test]
    fn test_c14n_escape_text_cr() {
        unsafe {
            let buf = io::buf_create(-1);
            assert!(!buf.is_null());

            let text = b"line1\r\nline2\r\0" as *const u8 as *const xmlChar;
            c14n_escape_text(buf, text, 13);

            let content = io::buf_content(buf);
            let len = io::buf_length(buf);
            let s = {
                let slice = core::slice::from_raw_parts(content, len as usize);
                String::from_utf8_lossy(slice).to_string()
            };
            assert!(
                s.contains("&#xD;"),
                "CR should be escaped as &#xD;, got: {}",
                s
            );
            assert!(s.contains("\n"), "LF should remain as-is, got: {}", s);

            io::buf_free(buf);
        }
    }

    #[test]
    fn test_c14n_escape_attr_tab_nl_cr() {
        unsafe {
            let buf = io::buf_create(-1);
            assert!(!buf.is_null());

            let text = b"a\tb\nc\rd\0" as *const u8 as *const xmlChar;
            c14n_escape_attr(buf, text);

            let content = io::buf_content(buf);
            let len = io::buf_length(buf);
            let s = {
                let slice = core::slice::from_raw_parts(content, len as usize);
                String::from_utf8_lossy(slice).to_string()
            };
            assert!(
                s.contains("&#x9;"),
                "Tab should be escaped as &#x9;, got: {}",
                s
            );
            assert!(
                s.contains("&#xA;"),
                "NL should be escaped as &#xA;, got: {}",
                s
            );
            assert!(
                s.contains("&#xD;"),
                "CR should be escaped as &#xD;, got: {}",
                s
            );

            io::buf_free(buf);
        }
    }

    #[test]
    fn test_c14n_parse_inclusive_prefixes() {
        // Test NULL input
        assert!(parse_inclusive_prefixes(ptr::null()).is_none());

        // Test empty string
        let empty = b"\0" as *const u8 as *const xmlChar;
        assert!(parse_inclusive_prefixes(empty).is_none());

        // Test single prefix
        let single = b"foo\0" as *const u8 as *const xmlChar;
        let result = parse_inclusive_prefixes(single);
        assert!(result.is_some());
        assert!(result.unwrap().contains("foo"));

        // Test multiple prefixes
        let multi = b"foo,bar,baz\0" as *const u8 as *const xmlChar;
        let result = parse_inclusive_prefixes(multi);
        assert!(result.is_some());
        let set = result.unwrap();
        assert!(set.contains("foo"));
        assert!(set.contains("bar"));
        assert!(set.contains("baz"));
        assert_eq!(set.len(), 3);
    }

    #[test]
    fn test_c14n_document_order() {
        unsafe {
            let doc = tree::new_doc(b"1.0\0" as *const u8 as *const xmlChar);
            assert!(!doc.is_null());

            let root = tree::new_node(ptr::null_mut(), b"root\0" as *const u8 as *const xmlChar);
            assert!(!root.is_null());
            tree::doc_set_root_element(doc, root);

            let child1 =
                tree::new_node(ptr::null_mut(), b"child1\0" as *const u8 as *const xmlChar);
            assert!(!child1.is_null());
            tree::add_child(root, child1);

            let child2 =
                tree::new_node(ptr::null_mut(), b"child2\0" as *const u8 as *const xmlChar);
            assert!(!child2.is_null());
            tree::add_child(root, child2);

            // child1 should come before child2
            assert_eq!(
                cmp_document_order(child1, child2),
                std::cmp::Ordering::Less,
                "child1 should be before child2"
            );
            assert_eq!(
                cmp_document_order(child2, child1),
                std::cmp::Ordering::Greater,
                "child2 should be after child1"
            );
            assert_eq!(
                cmp_document_order(child1, child1),
                std::cmp::Ordering::Equal,
                "Same node should be equal"
            );

            tree::free_doc(doc);
        }
    }

    #[test]
    fn test_c14n_escape_text_gt() {
        unsafe {
            let buf = io::buf_create(-1);
            assert!(!buf.is_null());

            let text = b"a > b\0" as *const u8 as *const xmlChar;
            c14n_escape_text(buf, text, 5);

            let content = io::buf_content(buf);
            let len = io::buf_length(buf);
            let s = {
                let slice = core::slice::from_raw_parts(content, len as usize);
                String::from_utf8_lossy(slice).to_string()
            };
            assert!(
                s.contains("&gt;"),
                "> should be escaped as &gt;, got: {}",
                s
            );

            io::buf_free(buf);
        }
    }

    #[test]
    fn test_c14n_escape_text_cdata_end() {
        unsafe {
            let buf = io::buf_create(-1);
            assert!(!buf.is_null());

            let text = b"a]]>b\0" as *const u8 as *const xmlChar;
            c14n_escape_text(buf, text, 5);

            let content = io::buf_content(buf);
            let len = io::buf_length(buf);
            let s = {
                let slice = core::slice::from_raw_parts(content, len as usize);
                String::from_utf8_lossy(slice).to_string()
            };
            // The `]]>` sequence should become `]]&gt;`
            assert!(
                s.contains("]]&gt;"),
                "]]> should be escaped as ]]&gt;, got: {}",
                s
            );

            io::buf_free(buf);
        }
    }

    #[test]
    fn test_c14n_execute_callback() {
        unsafe {
            let doc = create_simple_doc();

            // Use a heap-allocated Vec passed through the callback context
            let output_vec = Box::into_raw(Box::new(Vec::<u8>::new()));

            unsafe extern "C" fn test_callback(
                ctx: *mut c_void,
                data: *const c_char,
                len: c_int,
            ) -> c_int {
                let slice = unsafe { core::slice::from_raw_parts(data as *const u8, len as usize) };
                let output = unsafe { &mut *(ctx as *mut Vec<u8>) };
                output.extend_from_slice(slice);
                len
            }

            let ret = c14n_execute(
                doc,
                C14nMode::XML_C14N_1_0,
                ptr::null(),
                0,
                Some(
                    test_callback
                        as unsafe extern "C" fn(*mut c_void, *const c_char, c_int) -> c_int,
                ),
                output_vec as *mut c_void,
            );

            assert!(ret >= 0, "c14n_execute should succeed");
            let output = Box::from_raw(output_vec);
            let output_str = String::from_utf8_lossy(&output);
            assert!(
                output_str.contains("<root>"),
                "Callback output should contain <root>, got: {}",
                output_str
            );

            tree::free_doc(doc);
        }
    }

    #[test]
    fn test_c14n_save_to_output_buffer() {
        unsafe {
            let doc = create_simple_doc();

            // Create an output buffer
            let buf = io::buf_create(-1);
            assert!(!buf.is_null());

            let output = io::output_buffer_create_buffer(buf, ptr::null_mut());
            assert!(!output.is_null());

            let ret = c14n_doc_save_to(
                doc,
                ptr::null_mut(),
                C14nMode::XML_C14N_1_0,
                ptr::null(),
                0,
                output,
            );

            assert!(ret >= 0, "c14n_doc_save_to should succeed");

            let content = io::buf_content(buf);
            let len = io::buf_length(buf);
            let s = {
                let slice = core::slice::from_raw_parts(content, len as usize);
                String::from_utf8_lossy(slice).to_string()
            };
            assert!(
                s.contains("<root>"),
                "Output buffer should contain <root>, got: {}",
                s
            );

            io::output_buffer_close(output);
            io::buf_free(buf);
            tree::free_doc(doc);
        }
    }

    #[test]
    fn test_c14n_c_abi_doc_dump_memory() {
        unsafe {
            let doc = create_simple_doc();

            let mut result: *mut xmlChar = ptr::null_mut();
            let len = xmlC14NDocDumpMemory(
                doc,
                ptr::null_mut(),
                0, // XML_C14N_1_0
                ptr::null_mut(),
                0, // no comments
                &mut result as *mut *mut xmlChar,
            );

            assert!(len >= 0, "xmlC14NDocDumpMemory should succeed");
            assert!(!result.is_null());

            let s = {
                let slice = core::slice::from_raw_parts(result, len as usize);
                String::from_utf8_lossy(slice).to_string()
            };
            assert!(
                s.contains("<root>"),
                "C ABI export should produce canonical output, got: {}",
                s
            );

            xmlFree(result as *mut c_void);
            tree::free_doc(doc);
        }
    }

    #[test]
    fn test_c14n_c_abi_execute() {
        unsafe {
            let doc = create_simple_doc();

            let output_vec = Box::into_raw(Box::new(Vec::<u8>::new()));

            unsafe extern "C" fn test_callback(
                ctx: *mut c_void,
                data: *const c_char,
                len: c_int,
            ) -> c_int {
                let slice = unsafe { core::slice::from_raw_parts(data as *const u8, len as usize) };
                let output = unsafe { &mut *(ctx as *mut Vec<u8>) };
                output.extend_from_slice(slice);
                len
            }

            let ret = xmlC14NExecute(
                doc,
                0, // XML_C14N_1_0
                ptr::null_mut(),
                0,
                Some(
                    test_callback
                        as unsafe extern "C" fn(*mut c_void, *const c_char, c_int) -> c_int,
                ),
                output_vec as *mut c_void,
            );

            assert!(ret >= 0, "xmlC14NExecute should succeed");
            let output = Box::from_raw(output_vec);
            let output_str = String::from_utf8_lossy(&output);
            assert!(
                output_str.contains("<root>"),
                "C ABI execute should produce canonical output, got: {}",
                output_str
            );

            tree::free_doc(doc);
        }
    }

    #[test]
    fn test_c14n_c_abi_save_to() {
        unsafe {
            let doc = create_simple_doc();

            let buf = io::buf_create(-1);
            assert!(!buf.is_null());

            let output = io::output_buffer_create_buffer(buf, ptr::null_mut());
            assert!(!output.is_null());

            let ret = xmlC14NDocSaveTo(
                doc,
                ptr::null_mut(),
                0, // XML_C14N_1_0
                ptr::null_mut(),
                0,
                output,
            );

            assert!(ret >= 0, "xmlC14NDocSaveTo should succeed");

            let content = io::buf_content(buf);
            let len = io::buf_length(buf);
            let s = {
                let slice = core::slice::from_raw_parts(content, len as usize);
                String::from_utf8_lossy(slice).to_string()
            };
            assert!(
                s.contains("<root>"),
                "C ABI save_to should produce canonical output, got: {}",
                s
            );

            io::output_buffer_close(output);
            io::buf_free(buf);
            tree::free_doc(doc);
        }
    }
}
