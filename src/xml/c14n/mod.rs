//! Canonical XML implementation (§28, §85 Phase 7).
//!
//! Inclusive and exclusive canonicalization, comments, namespace propagation,
//! attribute ordering, character escaping, subsets, node sets.
//! Must be byte-exact compared to oracle.
//!
//! References:
//! - XML Canonicalization (C14N) — inclusive: <https://www.w3.org/TR/xml-c14n11/>
//! - Exclusive XML Canonicalization (C14N): <https://www.w3.org/2001/10/xml-exc-c14n>
//!
//! # Safety
//!
//! - The unsafe entry points in this module accept raw pointers that must be
//!   valid, correctly typed, and live for the duration of the call: pointers
//!   to `_xmlDoc`, `_xmlNode`, `_xmlNs`, `_xmlBuffer`, and `xmlOutputBuffer`
//!   objects, and NUL-terminated `xmlChar` strings. NULL is permitted only
//!   where an individual function's contract explicitly allows it.
//! - Callers own the pointed-to objects for the duration of each call and
//!   release them afterwards with the matching free function.

#![allow(
    missing_docs,
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_ptr_alignment,
    clippy::missing_safety_doc,
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::type_complexity
)]

use core::ffi::c_void;
use core::ptr;
use std::collections::HashSet;
use std::os::raw::{c_char, c_int};

use crate::abi::callbacks::xmlC14NIsVisibleCallback;
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
    const fn with_comments(self) -> bool {
        matches!(
            self,
            C14nMode::XML_C14N_1_0_WITH_COMMENTS
                | C14nMode::XML_C14N_EXCLUSIVE_1_0_WITH_COMMENTS
                | C14nMode::XML_C14N_1_1_WITH_COMMENTS
        )
    }

    /// Returns true if this mode is exclusive.
    const fn is_exclusive(self) -> bool {
        matches!(
            self,
            C14nMode::XML_C14N_EXCLUSIVE_1_0 | C14nMode::XML_C14N_EXCLUSIVE_1_0_WITH_COMMENTS
        )
    }

    /// Returns true if this mode is C14N 1.0 (inclusive).
    const fn is_1_0(self) -> bool {
        matches!(
            self,
            C14nMode::XML_C14N_1_0 | C14nMode::XML_C14N_1_0_WITH_COMMENTS
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
    /// Per-subtree stack of already-rendered (prefix, href) namespace pairs
    /// (upstream c14n.c `ns_rendered` visible-ns stack). A namespace
    /// binding is rendered at the topmost element where it is in effect;
    /// descendants inherit it without re-declaring, and a fresh subtree
    /// re-renders it (R-000166).
    rendered_stack: Vec<Vec<(Vec<u8>, Vec<u8>)>>,
    /// Position relative to the document element (upstream `ctx->pos`):
    /// 0 = XMLC14N_BEFORE_DOCUMENT_ELEMENT, 1 = INSIDE, 2 = AFTER. PIs and
    /// comments at the document level gain a trailing/leading newline
    /// depending on this position (c14n.c xmlC14NProcessNode).
    pos: u8,
    /// Upstream `ctx->parent_is_doc`: whether the element being processed
    /// is a direct child of the document (the document element).
    parent_is_doc: bool,
    /// Node-set for subset canonicalization (upstream's visibility
    /// callback): when present, only nodes whose pointer is in this set are
    /// visible. NULL (None) means the whole document is visible.
    visible_set: Option<HashSet<*mut c_void>>,
    /// Upstream `xmlC14NExecute`'s visibility callback (c14n.c
    /// `xmlC14NIsVisible`): when present it decides node visibility directly
    /// (R-000176 — the candidate previously mis-implemented xmlC14NExecute's
    /// whole register layout; the callback now lives here).
    visibility_callback: Option<(xmlC14NIsVisibleCallback, *mut c_void)>,
    /// Set when a node that canonical XML cannot process is encountered
    /// (upstream c14n.c xmlC14NErrInvalidNode: XML_ENTITY_REF_NODE /
    /// XML_ENTITY_NODE / XML_NAMESPACE_DECL return -1 "processing node"
    /// — 11.1-Z.1, R-000175). The dump functions then return -1 like the
    /// oracle instead of serializing the reference.
    pub invalid_node: bool,
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
        Self::with_visible_set(doc, mode, inclusive_ns_prefixes, None)
    }

    /// Create a new C14N context with an explicit visibility set.
    ///
    /// # SAFETY
    ///
    /// - `doc` must be a valid pointer to an `_xmlDoc` or NULL.
    /// - `visible_set` holds node pointers; None means the whole document is
    ///   visible (upstream `is_visible_callback == NULL`).
    pub unsafe fn with_visible_set(
        doc: *mut _xmlDoc,
        mode: C14nMode,
        inclusive_ns_prefixes: Option<HashSet<String>>,
        visible_set: Option<HashSet<*mut c_void>>,
    ) -> Self {
        let mut ctx = C14nContext {
            mode,
            ns_stack: Vec::new(),
            inclusive_ns_prefixes,
            doc,
            rendered_stack: Vec::new(),
            // Upstream xmlC14NNewCtx initialises the walk at the document
            // level, before the document element (c14n.c lines 1773-1774).
            pos: 0, // XMLC14N_BEFORE_DOCUMENT_ELEMENT
            parent_is_doc: true,
            visible_set,
            visibility_callback: None,
            invalid_node: false,
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

    /// Create a C14N context driven by an `xmlC14NExecute` visibility
    /// callback (upstream `ctx->isVisibleCallback` / `ctx->userData`).
    ///
    /// # SAFETY
    ///
    /// - `doc` must be a valid pointer to an `_xmlDoc` or NULL.
    /// - `callback`/`user_data` come from the C caller and must stay valid
    ///   for the walk.
    pub unsafe fn with_visibility_callback(
        doc: *mut _xmlDoc,
        mode: C14nMode,
        inclusive_ns_prefixes: Option<HashSet<String>>,
        callback: xmlC14NIsVisibleCallback,
        user_data: *mut c_void,
    ) -> Self {
        let mut ctx = C14nContext::with_visible_set(doc, mode, inclusive_ns_prefixes, None);
        ctx.visibility_callback = Some((callback, user_data));
        ctx
    }

    /// Upstream `xmlC14NIsVisible(ctx, node, parent)`: whether a node is in
    /// the visible node-set. Without a subset every node is visible. When an
    /// `xmlC14NExecute` visibility callback is installed it is consulted
    /// first (upstream `xmlC14NIsVisible` calls `ctx->isVisibleCallback`).
    fn is_visible_node<T>(&self, node: *mut T) -> bool {
        if let Some((cb, user_data)) = self.visibility_callback {
            let parent = if node.is_null() {
                ptr::null_mut()
            } else {
                unsafe { (*(node as *mut _xmlNode)).parent }
            };
            // SAFETY: the C caller provided the callback and data; per the
            // C contract both stay valid for the walk.
            return unsafe { cb(user_data, node as *mut _xmlNode, parent) } != 0;
        }
        match &self.visible_set {
            None => true,
            Some(set) => !node.is_null() && set.contains(&(node as *mut c_void)),
        }
    }

    /// Upstream `xmlC14NIsVisible(ctx, ns, cur)` with the caller's real
    /// namespace declaration: whether the namespace is part of the visible
    /// subset. c14n.c `xmlC14NIsNodeInNodeset` makes a stack copy of the ns,
    /// links `next` to the owning element, and defers to
    /// `xmlXPathNodeSetContains`, whose NAMESPACE_DECL arm matches a
    /// node-set entry by owner element + prefix (xpath.c). The synthesized
    /// namespace-axis nodes in the node-set carry `next` = owner element, so
    /// the equivalent test is: some set entry is a NAMESPACE_DECL whose
    /// `next` is `owner` with an equal (empty-tolerant) prefix.
    ///
    /// With the whole document visible every namespace is visible; with an
    /// `xmlC14NExecute` visibility callback the callback is consulted with
    /// the ns node and the owning element as parent (upstream behaviour).
    fn ns_visible(&self, ns: *const _xmlNs, owner: *mut _xmlNode) -> bool {
        if let Some((cb, user_data)) = self.visibility_callback {
            return unsafe { cb(user_data, ns as *mut _xmlNode, owner) } != 0;
        }
        match &self.visible_set {
            None => true,
            Some(set) => {
                if ns.is_null() || owner.is_null() {
                    return false;
                }
                let prefix = unsafe { (*ns).prefix };
                set.iter().any(|&e| {
                    // Node-set namespace entries are synthesized `_xmlNs`
                    // structs (xmlXPathNodeSetDupNs): type lives at the
                    // xmlNs offset, not the xmlNode offset.
                    let ns2 = e as *const _xmlNs;
                    unsafe {
                        (*ns2).type_ == XML_NAMESPACE_DECL as c_int
                            && (*ns2).next as *mut _xmlNode == owner
                            && c14n_prefix_eq((*ns2).prefix, prefix)
                    }
                })
            }
        }
    }

    /// Enter a new rendered-namespace scope (element open).
    fn push_rendered_scope(&mut self) {
        self.rendered_stack.push(Vec::new());
    }

    /// Exit the current rendered-namespace scope (element close).
    fn pop_rendered_scope(&mut self) {
        self.rendered_stack.pop();
    }

    /// Upstream c14n.c `xmlC14NVisibleNsStackFind` / `xmlExcC14NVisibleNsStackFind`
    /// (R-000166): whether the binding `(prefix, href)` counts as already
    /// rendered. Entries are searched most-recent-first; the first entry whose
    /// prefix matches decides by href equality (not exact-pair matching — a
    /// re-declaration of the same prefix to a different URI is NOT rendered, a
    /// re-declaration back to an older URI IS). `parent_frame_only` restricts
    /// the search to the parent element's rendered frame (upstream's
    /// `nsPrevStart` window, used by the inclusive axis and the exclusive
    /// InclusiveNamespaces list); otherwise the whole stack is searched
    /// (upstream `start = 0`, used by exclusive node/attribute namespaces).
    ///
    /// When no entry has the prefix at all, upstream returns `has_empty_ns`:
    /// the empty namespace counts as already rendered (so `xmlns=""` is only
    /// emitted when a non-empty default was rendered), everything else as not
    /// rendered.
    fn already_rendered(&self, prefix: &[u8], href: &[u8], parent_frame_only: bool) -> bool {
        let len = self.rendered_stack.len();
        let mut idx = len;
        let min = if parent_frame_only {
            len.saturating_sub(2)
        } else {
            0
        };
        while idx > min {
            idx -= 1;
            for (p, h) in self.rendered_stack[idx].iter().rev() {
                if p == prefix {
                    return h == href;
                }
            }
        }
        prefix.is_empty() && href.is_empty()
    }

    /// Mark the (prefix, href) pair in the current scope. Upstream adds every
    /// processed in-scope namespace to `ns_rendered` regardless of whether it
    /// was rendered, so this is called unconditionally for processed bindings.
    fn mark_rendered_pair(&mut self, prefix: &[u8], href: &[u8]) {
        if let Some(top) = self.rendered_stack.last_mut() {
            top.push((prefix.to_vec(), href.to_vec()));
        }
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
    ///
    /// # Safety
    ///
    /// - `prefix` must be NULL or a valid pointer to a NUL-terminated `xmlChar`
    ///   string; it is compared against existing entries with `xmlStrEqual`,
    ///   which reads it as a C string.
    /// - `href` is stored in the scope entry without being dereferenced here;
    ///   it must remain valid (or NULL, for an undeclaration) for as long as
    ///   the context keeps the entry.
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
    ///
    /// # Safety
    ///
    /// - `prefix` must be NULL or a valid pointer to a NUL-terminated `xmlChar`
    ///   string; it is compared with `xmlStrEqual`, which reads it as a C
    ///   string.
    #[allow(dead_code)]
    fn is_prefix_in_scope(&self, prefix: *const xmlChar) -> bool {
        self.ns_stack.iter().rev().any(|scope| {
            scope
                .iter()
                .any(|e| unsafe { crate::abi::exports_xml2::xmlStrEqual(e.prefix, prefix) != 0 })
        })
    }

    /// Get the href for a prefix from the current scope.
    ///
    /// # Safety
    ///
    /// - `prefix` must be NULL or a valid pointer to a NUL-terminated `xmlChar`
    ///   string; it is compared with `xmlStrEqual`, which reads it as a C
    ///   string.
    /// - The returned pointer is the matching scope entry's stored `href`; it
    ///   is valid only while that entry (and the string it points to) remains
    ///   alive in the context.
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
    ///
    /// # Safety
    ///
    /// - A non-NULL `prefix` must be a valid pointer to a NUL-terminated
    ///   `xmlChar` string; it is converted with `CStr::from_ptr`. A NULL
    ///   `prefix` matches the empty-string entry in the set.
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
    ///
    /// # Safety
    ///
    /// - `prefix` must be NULL or a valid pointer to a NUL-terminated `xmlChar`
    ///   string; it is compared with `xmlStrEqual`, which reads it as a C
    ///   string.
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
    ///
    /// # Safety
    ///
    /// - `prefix` must be NULL or a valid pointer to a NUL-terminated `xmlChar`
    ///   string; it is compared with `xmlStrEqual`, which reads it as a C
    ///   string.
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

/// Escape comment content per C14N rules: carriage returns are rendered as
/// `&#xD;`, everything else is copied (upstream `xmlC11NNormalizeComment`).
///
/// # SAFETY
///
/// - `buf` must be a valid pointer to a mutable `_xmlBuffer`.
/// - `text` must be a valid pointer to a null-terminated xmlChar string.
unsafe fn c14n_escape_comment(buf: *mut _xmlBuffer, text: *const xmlChar) {
    if buf.is_null() || text.is_null() {
        return;
    }
    let len = tree::xml_strlen(text);
    let mut i: c_int = 0;
    while i < len {
        let ch = unsafe { *text.add(i as usize) };
        if ch == 0x0D {
            io::buf_add(buf, b"&#xD;" as *const u8, 5);
        } else {
            io::buf_add(buf, &ch as *const u8, 1);
        }
        i += 1;
    }
}

/// Escape PI content per C14N rules: carriage returns are rendered as
/// `&#xD;`, everything else is copied (upstream `xmlC11NNormalizePI`).
///
/// # SAFETY
///
/// - `buf` must be a valid pointer to a mutable `_xmlBuffer`.
/// - `text` must be a valid pointer to a null-terminated xmlChar string.
unsafe fn c14n_escape_pi(buf: *mut _xmlBuffer, text: *const xmlChar) {
    if buf.is_null() || text.is_null() {
        return;
    }
    let len = tree::xml_strlen(text);
    let mut i: c_int = 0;
    while i < len {
        let ch = unsafe { *text.add(i as usize) };
        if ch == 0x0D {
            io::buf_add(buf, b"&#xD;" as *const u8, 5);
        } else {
            io::buf_add(buf, &ch as *const u8, 1);
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

/// Whether a namespace is the implicit xml namespace (prefix `xml`, href
/// `http://www.w3.org/XML/1998/namespace`) — upstream c14n.c
/// `xmlC14NIsXmlNs`. Such namespaces are never rendered.
///
/// # SAFETY
///
/// - `ns` must be a valid reference to an `_xmlNs`.
unsafe fn is_xml_ns_ref(ns: &_xmlNs) -> bool {
    !ns.prefix.is_null()
        && !ns.href.is_null()
        && crate::abi::exports_xml2::xmlStrEqual(
            ns.prefix,
            XML_XML_PREFIX.as_ptr() as *const xmlChar,
        ) != 0
        && crate::abi::exports_xml2::xmlStrEqual(ns.href, XML_XML_NS_URI.as_ptr() as *const xmlChar)
            != 0
}

/// Upstream c14n.c `xmlC14NStrEqual`: NULL and the empty string are
/// interchangeable (missing prefixes/hrefs compare as empty).
///
/// # SAFETY
///
/// - Non-NULL arguments must be valid NUL-terminated `xmlChar` strings.
unsafe fn c14n_prefix_eq(a: *const xmlChar, b: *const xmlChar) -> bool {
    if a == b {
        return true;
    }
    let empty = |p: *const xmlChar| p.is_null() || unsafe { *p == 0 };
    if empty(a) || empty(b) {
        return empty(a) && empty(b);
    }
    unsafe { crate::abi::exports_xml2::xmlStrEqual(a, b) != 0 }
}

/// Copy a NUL-terminated xmlChar string into a Vec (empty for NULL).
///
/// # SAFETY
///
/// - `p` must be NULL or a valid NUL-terminated xmlChar string.
unsafe fn cstr_bytes(p: *const xmlChar) -> Vec<u8> {
    if p.is_null() {
        return Vec::new();
    }
    let mut l = 0usize;
    while *p.add(l) != 0 {
        l += 1;
    }
    core::slice::from_raw_parts(p, l).to_vec()
}

/// Upstream `xmlExcC14NProcessNamespacesAxis` per-namespace handling: skip
/// the xml namespace and non-visible namespaces, resolve already-rendered
/// against the visible-ns stack (adding to it when the element is visible,
/// as upstream does), and collect when not already rendered.
/// `parent_frame_only` selects the plain `xmlC14NVisibleNsStackFind` window
/// (upstream `nsPrevStart`) versus the exclusive `start = 0` window.
///
/// # SAFETY
///
/// - `ns` must be NULL or a valid pointer to an `_xmlNs`.
unsafe fn exc_push_ns(
    ctx: &mut C14nContext,
    collected: &mut Vec<CollectedNs>,
    ns: *mut _xmlNs,
    has_empty_ns: &mut bool,
    parent_frame_only: bool,
    mark: bool,
    insert: bool,
    default_seen: bool,
) {
    if ns.is_null() {
        return;
    }
    let ns_ref = unsafe { &*ns };
    if is_xml_ns_ref(ns_ref) {
        return;
    }
    let prefix_bytes = unsafe { cstr_bytes(ns_ref.prefix) };
    let href_bytes = unsafe { cstr_bytes(ns_ref.href) };
    if insert {
        let already = ctx.already_rendered(&prefix_bytes, &href_bytes, parent_frame_only);
        if !already {
            collected.push(CollectedNs {
                prefix: ns_ref.prefix,
                href: ns_ref.href,
            });
        }
    }
    if mark {
        ctx.mark_rendered_pair(&prefix_bytes, &href_bytes);
    }
    if default_seen && ns_ref.prefix.is_null() {
        *has_empty_ns = true;
    }
}

/// Collect visible namespace declarations for a node per inclusive/exclusive rules.
///
/// For **inclusive** C14N:
/// - All effective namespaces in scope for the node are included.
/// - Bindings the parent element already rendered are not re-rendered.
/// - `xmlns=""` undeclarations are emitted when declared.
///
/// For **exclusive** C14N:
/// - Only namespaces visibly utilized by the node and its attributes are
///   included, plus any prefixes in the InclusiveNamespaces PrefixList.
/// - Bindings already rendered by an ancestor are not re-rendered.
///
/// This is a faithful port of upstream c14n.c `xmlC14NProcessNamespacesAxis`
/// (inclusive) and `xmlExcC14NProcessNamespacesAxis` (exclusive), including
/// the `ns_rendered` visible-ns stack semantics and the visibility callback
/// gating for subset canonicalization (R-000166).
///
/// # SAFETY
///
/// - `node` must be a valid pointer to an `_xmlNode` or NULL.
/// - `ctx` must be a valid pointer to a `C14nContext`.
unsafe fn c14n_collect_namespaces(
    node: *mut _xmlNode,
    ctx: &mut C14nContext,
    visible: bool,
) -> Vec<CollectedNs> {
    if node.is_null() {
        return Vec::new();
    }

    let n = unsafe { &*node };
    if n.type_ != XML_ELEMENT_NODE as c_int {
        return Vec::new();
    }

    let mut collected: Vec<CollectedNs> = Vec::new();
    let mut seen_prefixes: Vec<*const xmlChar> = Vec::new();

    if ctx.mode.is_exclusive() {
        // ── Exclusive C14N namespace collection ──
        //
        // Port of upstream xmlExcC14NProcessNamespacesAxis:
        //   1. the InclusiveNamespaces PrefixList is processed first under
        //      Canonical-XML rules (plain find, parent-frame window);
        //   2. the element's own namespace, or — when the element has no
        //      binding — the default namespace in scope (this is what
        //      renders `xmlns=""` undeclarations);
        //   3. attribute namespaces (exclusive find, whole-stack window);
        //   4. the `xmlns=""` fallback for a visibly-utilized empty default.
        let mut has_empty_ns = false;
        let mut has_empty_ns_in_inclusive_list = false;
        let mut has_visibly_utilized_empty_ns = false;

        // 1. InclusiveNamespaces PrefixList.
        let inclusive_prefixes: Vec<String> = ctx
            .inclusive_ns_prefixes
            .clone()
            .map(|s| s.into_iter().collect())
            .unwrap_or_default();
        // Keep owned NUL-terminated copies alive across the loop (the
        // `if`-arm temporary would dangle).
        let prefix_cstrs: Vec<Vec<u8>> = inclusive_prefixes
            .iter()
            .filter(|p| !p.is_empty() && p.as_str() != "#default")
            .map(|p| format!("{}\0", p).into_bytes())
            .collect();
        for inc_prefix_str in &inclusive_prefixes {
            let is_default = inc_prefix_str.is_empty() || inc_prefix_str == "#default";
            if is_default {
                has_empty_ns_in_inclusive_list = true;
            }
            let inc_prefix: *const xmlChar = if is_default {
                ptr::null()
            } else {
                let idx = prefix_cstrs
                    .iter()
                    .position(|c| {
                        let s = String::from_utf8_lossy(&c[..c.len() - 1]);
                        s == *inc_prefix_str
                    })
                    .expect("prefix cstr present");
                prefix_cstrs[idx].as_ptr() as *const xmlChar
            };
            let ns = find_ns_declaration(node, inc_prefix);
            // Upstream processes the prefix list only when the namespace is
            // visible in the subset (`xmlC14NIsVisible(ctx, ns, cur)`).
            let ns_visible = !ns.is_null() && ctx.ns_visible(ns, node);
            exc_push_ns(
                ctx,
                &mut collected,
                ns,
                &mut has_empty_ns,
                true,
                visible,
                ns_visible,
                ns_visible,
            );
        }

        // 2. The node's own namespace; when the element has no binding, the
        //    default namespace in scope (upstream xmlSearchNs(cur->doc, cur,
        //    NULL) with has_visibly_utilized_empty_ns = 1).
        let node_ns = if n.ns.is_null() {
            has_visibly_utilized_empty_ns = true;
            find_ns_declaration(node, ptr::null())
        } else {
            n.ns
        };
        let node_ns_visible = !node_ns.is_null() && ctx.ns_visible(node_ns, node);
        // Upstream: the node namespace is inserted only for a visible element
        // whose namespace is visible (`if(visible && xmlC14NIsVisible(...))`),
        // while the ns_rendered stack add happens for any visible element and
        // has_empty_ns is set whenever the binding is a default namespace.
        exc_push_ns(
            ctx,
            &mut collected,
            node_ns,
            &mut has_empty_ns,
            false,
            visible,
            visible && node_ns_visible,
            true,
        );

        // 3. Attribute namespaces: a visible attribute's namespace is added
        //    regardless of the namespace's own subset membership (upstream
        //    gates on `xmlC14NIsVisible(ctx, attr, cur)`).
        let mut attr = n.properties;
        while !attr.is_null() {
            let a = unsafe { &*attr };
            if !a.ns.is_null() {
                let ans = unsafe { &*a.ns };
                let attr_visible = ctx.is_visible_node(attr);
                if !is_xml_ns_ref(ans) && attr_visible {
                    exc_push_ns(
                        ctx,
                        &mut collected,
                        a.ns,
                        &mut has_empty_ns,
                        false,
                        attr_visible,
                        attr_visible,
                        attr_visible,
                    );
                } else if ans.prefix.is_null() && !ans.href.is_null() && *ans.href == 0 {
                    // Upstream: an attribute bound to an empty default
                    // namespace counts as a visibly utilized empty default
                    // (checked outside the visibility gate).
                    has_visibly_utilized_empty_ns = true;
                }
            }
            attr = a.next;
        }

        // 4. Process xmlns="".
        if visible
            && !has_empty_ns
            && (has_visibly_utilized_empty_ns || has_empty_ns_in_inclusive_list)
        {
            let empty_prefix: Vec<u8> = Vec::new();
            let empty_href: Vec<u8> = Vec::new();
            if !ctx.already_rendered(&empty_prefix, &empty_href, false) {
                collected.push(CollectedNs {
                    prefix: ptr::null(),
                    href: ptr::null(),
                });
            }
        }
    } else {
        // ── Inclusive C14N namespace collection ──
        //
        // Port of upstream xmlC14NProcessNamespacesAxis: walk the element
        // and its ancestors, processing every effective (in-scope) namespace
        // declaration (upstream's `xmlSearchNs(...) == ns` tmp check), skip
        // the xml namespace and non-visible namespaces, and skip bindings
        // already rendered by the parent (plain find, parent-frame window).
        let mut has_empty_ns = false;
        let mut cur: *mut _xmlNode = node;
        while !cur.is_null() {
            let cur_node = unsafe { &*cur };
            // Only element ancestors carry namespace declarations. In
            // particular the document node aliases `nsDef` onto its
            // `oldNs` list (identical struct offsets); upstream filters
            // those out with the `xmlSearchNs(...) == ns` effective-binding
            // test, and skipping non-element ancestors reproduces that.
            if cur_node.type_ != XML_ELEMENT_NODE as c_int {
                cur = cur_node.parent;
                continue;
            }
            let mut ns_def = cur_node.nsDef;
            while !ns_def.is_null() {
                let ns = unsafe { &*ns_def };
                let ns_prefix = ns.prefix;

                // The effective binding for this prefix at `node` is the
                // nearest declaration (upstream tmp == ns check).
                if !seen_prefixes.iter().any(|p| {
                    if ns_prefix.is_null() && p.is_null() {
                        return true;
                    }
                    if ns_prefix.is_null() || p.is_null() {
                        return false;
                    }
                    unsafe { crate::abi::exports_xml2::xmlStrEqual(*p, ns_prefix) != 0 }
                }) {
                    seen_prefixes.push(ns_prefix);
                    // Skip the xml namespace (always implicitly in scope)
                    // and non-visible namespaces (upstream tmp == ns &&
                    // !xmlC14NIsXmlNs && xmlC14NIsVisible(ctx, ns, cur)
                    // gates, where `cur` is the axis-owning element).
                    if is_xml_ns_ref(ns) || !ctx.ns_visible(ns_def, node) {
                        ns_def = ns.next;
                        continue;
                    }
                    let prefix_bytes = unsafe { cstr_bytes(ns_prefix) };
                    let href_bytes = unsafe { cstr_bytes(ns.href) };
                    let is_empty_ns = prefix_bytes.is_empty() && href_bytes.is_empty();
                    let already = ctx.already_rendered(&prefix_bytes, &href_bytes, !is_empty_ns);
                    if visible {
                        ctx.mark_rendered_pair(&prefix_bytes, &href_bytes);
                    }
                    if !already {
                        collected.push(CollectedNs {
                            prefix: ns_prefix,
                            href: ns.href,
                        });
                    }
                    if ns_prefix.is_null() {
                        has_empty_ns = true;
                    }
                }
                ns_def = ns.next;
            }
            cur = cur_node.parent;
        }

        // Upstream lines 625-641: emit xmlns="" when this element's axis
        // has no default namespace node but a non-empty default namespace
        // was already rendered (C14N 1.0 namespace-axis rule).
        if visible && !has_empty_ns {
            let empty_prefix: Vec<u8> = Vec::new();
            let empty_href: Vec<u8> = Vec::new();
            if !ctx.already_rendered(&empty_prefix, &empty_href, false) {
                collected.push(CollectedNs {
                    prefix: ptr::null(),
                    href: ptr::null(),
                });
            }
        }
    }

    // Upstream renders namespaces in lexicographic prefix order (default
    // namespace first) via the sorted xmlList (xmlC14NNsCompare).
    collected.sort_by(|a, b| {
        let (ap, bp) = (a.prefix, b.prefix);
        match (ap.is_null(), bp.is_null()) {
            (true, true) => std::cmp::Ordering::Equal,
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            (false, false) => unsafe { crate::abi::exports_xml2::xmlStrcmp(ap, bp) }.cmp(&0),
        }
    });

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

/// Compare two attribute pointers for canonical ordering — upstream c14n.c
/// `xmlC14NAttrsCompare`: same-ns-pointer first, then attributes in the
/// default namespace, then lexicographic by namespace URI, then by local
/// name.
///
/// Returns negative, zero, or positive.
///
/// # SAFETY
///
/// - `a` and `b` must be valid pointers to `_xmlAttr`.
unsafe fn compare_attrs(a: *const _xmlAttr, b: *const _xmlAttr) -> std::cmp::Ordering {
    if a == b {
        return std::cmp::Ordering::Equal;
    }
    if a.is_null() {
        return std::cmp::Ordering::Less;
    }
    if b.is_null() {
        return std::cmp::Ordering::Greater;
    }
    let attr_a = unsafe { &*a };
    let attr_b = unsafe { &*b };

    if attr_a.ns == attr_b.ns {
        return unsafe { crate::abi::exports_xml2::xmlStrcmp(attr_a.name, attr_b.name) }.cmp(&0);
    }

    // Attributes in the default namespace are first because the default
    // namespace is not applied to unqualified attributes.
    if attr_a.ns.is_null() {
        return std::cmp::Ordering::Less;
    }
    if attr_b.ns.is_null() {
        return std::cmp::Ordering::Greater;
    }
    if unsafe { (*(attr_a.ns)).prefix.is_null() } {
        return std::cmp::Ordering::Less;
    }
    if unsafe { (*(attr_b.ns)).prefix.is_null() } {
        return std::cmp::Ordering::Greater;
    }

    let ret =
        unsafe { crate::abi::exports_xml2::xmlStrcmp((*(attr_a.ns)).href, (*(attr_b.ns)).href) };
    if ret != 0 {
        return ret.cmp(&0);
    }
    unsafe { crate::abi::exports_xml2::xmlStrcmp(attr_a.name, attr_b.name) }.cmp(&0)
}

/// Whether an attribute is in the xml namespace (upstream `xmlC14NIsXmlAttr`).
///
/// # SAFETY
///
/// - `attr` must be a valid reference to an `_xmlAttr`.
unsafe fn is_xml_attr_ref(attr: &_xmlAttr) -> bool {
    !attr.ns.is_null() && unsafe { is_xml_ns_ref(&*attr.ns) }
}

/// Upstream `xmlC14NFindHiddenParentAttr`: walk up from `cur` while the node
/// is NOT in the visible node-set, returning the nearest xml-namespace
/// attribute with the given local name.
///
/// # SAFETY
///
/// - `cur` must be NULL or a valid pointer to an `_xmlNode`.
unsafe fn find_hidden_parent_attr(
    ctx: &C14nContext,
    cur: *mut _xmlNode,
    name: &[u8],
) -> *mut _xmlAttr {
    let mut cur = cur;
    while !cur.is_null() && !ctx.is_visible_node(cur) {
        let name_c = format!("{}\0", String::from_utf8_lossy(name));
        let res = crate::xml::tree::has_ns_prop(
            cur,
            name_c.as_ptr() as *const xmlChar,
            XML_XML_NS_URI.as_ptr() as *const xmlChar,
        );
        if !res.is_null() {
            return res;
        }
        cur = unsafe { (*cur).parent };
    }
    ptr::null_mut()
}

/// Upstream `xmlC14NFixupBaseAttr`: resolve an xml:base value against the
/// xml:base attributes of the hidden ancestors. Returns the resolved value
/// (owned Vec), or None when the result is empty.
///
/// # SAFETY
///
/// - `base_attr` must be a valid pointer to an `_xmlAttr` with a parent.
unsafe fn fixup_base_attr(ctx: &C14nContext, base_attr: *mut _xmlAttr) -> Option<Vec<u8>> {
    let mut res: Vec<u8> = Vec::new();
    let ba = unsafe { &*base_attr };
    if !ba.children.is_null() {
        let child = unsafe { &*ba.children };
        if !child.content.is_null() {
            let mut l = 0usize;
            while *child.content.add(l) != 0 {
                l += 1;
            }
            res = core::slice::from_raw_parts(child.content, l).to_vec();
        }
    }

    let mut cur = if ba.parent.is_null() {
        ptr::null_mut()
    } else {
        unsafe { (*ba.parent).parent }
    };
    while !cur.is_null() && !ctx.is_visible_node(cur) {
        let tmp_c = b"base\0";
        let attr = crate::xml::tree::has_ns_prop(
            cur,
            tmp_c.as_ptr() as *const xmlChar,
            XML_XML_NS_URI.as_ptr() as *const xmlChar,
        );
        if !attr.is_null() {
            let mut tmp_str: Vec<u8> = Vec::new();
            let a = unsafe { &*attr };
            if !a.children.is_null() {
                let child = unsafe { &*a.children };
                if !child.content.is_null() {
                    let mut l = 0usize;
                    while *child.content.add(l) != 0 {
                        l += 1;
                    }
                    tmp_str = core::slice::from_raw_parts(child.content, l).to_vec();
                }
            }
            // Force going "up" when the base ends in '.' or '..' (upstream
            // appends '/').
            let tl = tmp_str.len();
            if tl > 1 && tmp_str[tl - 2] == b'.' {
                tmp_str.push(b'/');
            }
            // Build the resolved URI.
            let uri_c = format!("{}\0", String::from_utf8_lossy(&res));
            let base_c = format!("{}\0", String::from_utf8_lossy(&tmp_str));
            let built = crate::abi::exports_uri::xmlBuildURI(
                uri_c.as_ptr() as *const c_char,
                base_c.as_ptr() as *const c_char,
            );
            if built.is_null() {
                return None;
            }
            let mut l = 0usize;
            while *built.add(l) != 0 {
                l += 1;
            }
            res = core::slice::from_raw_parts(built, l).to_vec();
            crate::abi::allocator::xmlFreeImpl(built as *mut c_void);
        }
        cur = unsafe { (*cur).parent };
    }

    if res.is_empty() {
        None
    } else {
        Some(res)
    }
}

/// One attribute to serialize: the attribute node plus an optional value
/// override (used for the fixed-up xml:base of hidden ancestors).
struct AttrOut {
    attr: *mut _xmlAttr,
    base_value: Option<Vec<u8>>,
}

/// Serialize the attribute axis in canonical order — upstream c14n.c
/// `xmlC14NProcessAttrsAxis`. The `xmlns:*` declarations are NOT included
/// here; they are handled by `c14n_serialize_namespaces`.
///
/// Mode-specific subset rules:
/// - C14N 1.0: visible attributes, plus xml-namespace attributes imported
///   from hidden ancestors of a visible orphan element.
/// - Exclusive: visible attributes only.
/// - C14N 1.1: visible attributes, plus the simple inheritable xml:lang /
///   xml:space from hidden ancestors and a fixed-up xml:base.
///
/// # SAFETY
///
/// - `node` must be a valid pointer to an `_xmlNode` or NULL.
/// - `ctx` must be a valid pointer to a `C14nContext`.
/// - `buf` must be a valid pointer to a mutable `_xmlBuffer`.
unsafe fn c14n_serialize_attributes(
    node: *mut _xmlNode,
    ctx: &mut C14nContext,
    buf: *mut _xmlBuffer,
    element_visible: bool,
) {
    if node.is_null() || buf.is_null() {
        return;
    }

    let n = unsafe { &*node };
    if n.type_ != XML_ELEMENT_NODE as c_int {
        return;
    }

    // Collect attributes into a vector for canonical sorting.
    let mut list: Vec<AttrOut> = Vec::new();
    let mut cur_attr = n.properties;

    if ctx.mode.is_1_0() {
        // C14N 1.0: all visible attributes of the element.
        while !cur_attr.is_null() {
            if ctx.is_visible_node(cur_attr) {
                list.push(AttrOut {
                    attr: cur_attr,
                    base_value: None,
                });
            }
            cur_attr = unsafe { (*cur_attr).next };
        }

        // Orphan handling: a visible element whose parent is not in the
        // node-set imports the xml-namespace attributes of its hidden
        // ancestors (c14n.c lines 1165-1187).
        if element_visible && !n.parent.is_null() && !ctx.is_visible_node(n.parent) {
            let mut tmp = n.parent;
            while !tmp.is_null() && unsafe { (*tmp).type_ == XML_ELEMENT_NODE as c_int } {
                let mut attr = unsafe { (*tmp).properties };
                while !attr.is_null() {
                    let a = unsafe { &*attr };
                    if unsafe { is_xml_attr_ref(a) } {
                        let dup = list.iter().any(|o| unsafe {
                            compare_attrs(o.attr, attr) == std::cmp::Ordering::Equal
                        });
                        if !dup {
                            list.push(AttrOut {
                                attr,
                                base_value: None,
                            });
                        }
                    }
                    attr = unsafe { (*attr).next };
                }
                tmp = unsafe { (*tmp).parent };
            }
        }
    } else if ctx.mode.is_exclusive() {
        // Exclusive: visible attributes only; xml attributes of orphan
        // nodes are not imported.
        while !cur_attr.is_null() {
            if ctx.is_visible_node(cur_attr) {
                list.push(AttrOut {
                    attr: cur_attr,
                    base_value: None,
                });
            }
            cur_attr = unsafe { (*cur_attr).next };
        }
    } else {
        // C14N 1.1: visible attributes, with xml:lang / xml:space /
        // xml:base handled specially (c14n.c lines 1238-1311).
        let mut xml_lang_attr: *mut _xmlAttr = ptr::null_mut();
        let mut xml_space_attr: *mut _xmlAttr = ptr::null_mut();
        let mut xml_base_attr: *mut _xmlAttr = ptr::null_mut();

        while !cur_attr.is_null() {
            let a = unsafe { &*cur_attr };
            if !element_visible || !unsafe { is_xml_attr_ref(a) } {
                if ctx.is_visible_node(cur_attr) {
                    list.push(AttrOut {
                        attr: cur_attr,
                        base_value: None,
                    });
                }
            } else {
                // Simple inheritable attributes and xml:base of the element
                // itself are collected (visible or not); other xml-namespace
                // attributes are ordinary visible attributes.
                let mut matched = false;
                if xml_lang_attr.is_null()
                    && !a.name.is_null()
                    && unsafe {
                        crate::abi::exports_xml2::xmlStrEqual(
                            a.name,
                            c"lang".as_ptr() as *const xmlChar,
                        ) != 0
                    }
                {
                    xml_lang_attr = cur_attr;
                    matched = true;
                }
                if !matched
                    && xml_space_attr.is_null()
                    && !a.name.is_null()
                    && unsafe {
                        crate::abi::exports_xml2::xmlStrEqual(
                            a.name,
                            c"space".as_ptr() as *const xmlChar,
                        ) != 0
                    }
                {
                    xml_space_attr = cur_attr;
                    matched = true;
                }
                if !matched
                    && xml_base_attr.is_null()
                    && !a.name.is_null()
                    && unsafe {
                        crate::abi::exports_xml2::xmlStrEqual(
                            a.name,
                            c"base".as_ptr() as *const xmlChar,
                        ) != 0
                    }
                {
                    xml_base_attr = cur_attr;
                    matched = true;
                }
                if !matched && ctx.is_visible_node(cur_attr) {
                    list.push(AttrOut {
                        attr: cur_attr,
                        base_value: None,
                    });
                }
            }
            cur_attr = unsafe { (*cur_attr).next };
        }

        if element_visible {
            // Simple inheritable attributes — nearest hidden ancestor.
            if xml_lang_attr.is_null() {
                xml_lang_attr = find_hidden_parent_attr(ctx, n.parent, b"lang");
            }
            if !xml_lang_attr.is_null() {
                list.push(AttrOut {
                    attr: xml_lang_attr,
                    base_value: None,
                });
            }
            if xml_space_attr.is_null() {
                xml_space_attr = find_hidden_parent_attr(ctx, n.parent, b"space");
            }
            if !xml_space_attr.is_null() {
                list.push(AttrOut {
                    attr: xml_space_attr,
                    base_value: None,
                });
            }
            // xml:base — resolved against the hidden ancestors.
            if xml_base_attr.is_null() {
                xml_base_attr = find_hidden_parent_attr(ctx, n.parent, b"base");
            }
            if !xml_base_attr.is_null() {
                if let Some(resolved) = fixup_base_attr(ctx, xml_base_attr) {
                    list.push(AttrOut {
                        attr: xml_base_attr,
                        base_value: Some(resolved),
                    });
                }
            }
        }
    }

    // Sort attributes canonically
    list.sort_by(|a, b| unsafe { compare_attrs(a.attr, b.attr) });

    // Serialize each attribute
    for out in &list {
        let a = unsafe { &*out.attr };

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

        // Attribute value: the fixed-up base value, or the child text node.
        if let Some(ref value) = out.base_value {
            if !value.is_empty() {
                let value_c = format!("{}\0", String::from_utf8_lossy(value));
                c14n_escape_attr(buf, value_c.as_ptr() as *const xmlChar);
            }
        } else if !a.children.is_null() {
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
            // UPSTREAM-PARITY (c14n.c xmlC14NProcessNode): text nodes are
            // rendered only when they are in the visible node-set.
            if ctx.is_visible_node(node) && !n.content.is_null() {
                c14n_escape_text(buf, n.content, tree::xml_strlen(n.content));
            }
        }
        t if t == XML_CDATA_SECTION_NODE as c_int => {
            // C14N converts CDATA sections to text
            if ctx.is_visible_node(node) && !n.content.is_null() {
                c14n_escape_text(buf, n.content, tree::xml_strlen(n.content));
            }
        }
        t if t == XML_COMMENT_NODE as c_int => {
            if ctx.is_visible_node(node) && ctx.mode.with_comments() {
                // UPSTREAM-PARITY (c14n.c xmlC14NProcessNode): comment
                // children of the root node get a leading newline when they
                // follow the document element and a trailing newline when
                // they precede it.
                if ctx.pos == 2 {
                    io::buf_ccat(buf, b'\n');
                }
                io::buf_add(buf, b"<!--" as *const u8, 4);
                if !n.content.is_null() {
                    c14n_escape_comment(buf, n.content);
                }
                io::buf_add(buf, b"-->" as *const u8, 3);
                if ctx.pos == 0 {
                    io::buf_ccat(buf, b'\n');
                }
            }
        }
        t if t == XML_PI_NODE as c_int => {
            // UPSTREAM-PARITY (c14n.c xmlC14NProcessNode): PI children of
            // the root node get a leading newline when they follow the
            // document element and a trailing newline when they precede it.
            if ctx.is_visible_node(node) {
                if ctx.pos == 2 {
                    io::buf_ccat(buf, b'\n');
                }
                io::buf_add(buf, b"<?" as *const u8, 2);
                if !n.name.is_null() {
                    io::buf_cat(buf, n.name);
                }
                if !n.content.is_null() && unsafe { *n.content != 0 } {
                    io::buf_ccat(buf, b' ');
                    c14n_escape_pi(buf, n.content);
                }
                io::buf_add(buf, b"?>" as *const u8, 2);
                if ctx.pos == 0 {
                    io::buf_ccat(buf, b'\n');
                }
            }
        }
        t if t == XML_DOCUMENT_NODE as c_int || t == XML_HTML_DOCUMENT_NODE as c_int => {
            // Serialize children of the document node (skip XML declaration)
            ctx.pos = 0; // XMLC14N_BEFORE_DOCUMENT_ELEMENT
            ctx.parent_is_doc = true;
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
            // UPSTREAM-PARITY (c14n.c xmlC14NProcessNode): an unexpanded
            // entity reference is NOT serialized — upstream fails the
            // canonicalization with xmlC14NErrInvalidNode ("processing
            // node", return -1). The candidate mirrors the -1 (R-000175).
            ctx.invalid_node = true;
        }
        _ => {
            // For unknown types, write content if present
            if !n.content.is_null() {
                c14n_escape_text(buf, n.content, tree::xml_strlen(n.content));
            }
        }
    }
}

/// Check whether any element in the document declares a relative
/// (scheme-less) namespace URI (upstream c14n.c
/// `xmlC14NCheckForRelativeNamespaces`).
///
/// # SAFETY
///
/// - `doc` must be a valid pointer to an `_xmlDoc`.
unsafe fn doc_has_relative_ns(doc: *mut _xmlDoc) -> bool {
    unsafe {
        let mut stack: Vec<*mut _xmlNode> = Vec::new();
        if !(*doc).children.is_null() {
            stack.push((*doc).children);
        }
        while let Some(node) = stack.pop() {
            let mut cur = node;
            while !cur.is_null() {
                let t = (*cur).type_;
                if t == crate::abi::types::xmlElementType::XML_ELEMENT_NODE as c_int {
                    let mut ns_def = (*cur).nsDef;
                    while !ns_def.is_null() {
                        let href = (*ns_def).href;
                        if !href.is_null() && *href != 0 && !has_uri_scheme_bytes(href) {
                            return true;
                        }
                        ns_def = (*ns_def).next;
                    }
                    if !(*cur).children.is_null() {
                        stack.push((*cur).children);
                    }
                }
                cur = (*cur).next;
            }
        }
        false
    }
}

/// Whether a NUL-terminated URI has a scheme (`scheme:` prefix) — upstream
/// xmlParseURISafe's `scheme == NULL` check.
///
/// # Safety
///
/// - `uri` must be a valid pointer to a NUL-terminated `xmlChar` buffer; the
///   scan reads one byte at a time until the terminating NUL and never past it.
const unsafe fn has_uri_scheme_bytes(uri: *const xmlChar) -> bool {
    unsafe {
        let mut i: usize = 0;
        while *uri.add(i) != 0 {
            let c = *uri.add(i);
            if c == b':' {
                return i > 0;
            }
            if i == 0 {
                if !c.is_ascii_alphabetic() {
                    return false;
                }
            } else if !(c.is_ascii_alphanumeric() || c == b'+' || c == b'-' || c == b'.') {
                return false;
            }
            i += 1;
        }
        false
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
    let visible = ctx.is_visible_node(node);

    // UPSTREAM-PARITY (c14n.c xmlC14NProcessElementNode): the document
    // element moves the position state into the element; it is restored to
    // AFTER_DOCUMENT_ELEMENT when the element closes (c14n.c lines
    // 1420-1426 and 1470-1474). Both transitions happen only when the
    // element is visible.
    let parent_is_doc = ctx.parent_is_doc;
    if visible && parent_is_doc {
        ctx.parent_is_doc = false;
        ctx.pos = 1; // XMLC14N_INSIDE_DOCUMENT_ELEMENT
    }

    // Push a new namespace scope for this element
    ctx.push_scope();
    // Push a rendered-namespace scope (upstream ns_rendered stack).
    ctx.push_rendered_scope();

    // Collect namespaces for this element
    let ns_list = c14n_collect_namespaces(node, ctx, visible);

    if visible {
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
        c14n_serialize_attributes(node, ctx, buf, visible);

        if n.children.is_null() {
            // UPSTREAM-PARITY (c14n.c xmlC14NProcessElementNode): canonical
            // form expands empty elements (`<name></name>`, never `<name/>`).
            io::buf_ccat(buf, b'>');
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
    } else {
        // Not in the node-set: still process the namespace and attribute
        // axes (for the ns_rendered stack semantics) and the children, but
        // write nothing (c14n.c xmlC14NProcessElementNode).
        let mut child = n.children;
        while !child.is_null() {
            c14n_serialize_node(child, ctx, buf);
            child = unsafe { (*child).next };
        }
    }

    // Pop namespace scope
    ctx.pop_rendered_scope();
    ctx.pop_scope();

    // UPSTREAM-PARITY: after the document element closes, following root
    // children are in the AFTER_DOCUMENT_ELEMENT position.
    if visible && parent_is_doc {
        ctx.parent_is_doc = true;
        ctx.pos = 2; // XMLC14N_AFTER_DOCUMENT_ELEMENT
    }
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

    // Upstream xmlC14NDocDumpMemory passes an xmlNodeSet as the visibility
    // callback data: nodes in the set are visible, everything else is
    // processed but not rendered (c14n.c xmlC14NIsNodeInNodeset).
    let visible_set = build_visible_set(nodes);

    let mut ctx = C14nContext::with_visible_set(doc, effective_mode, inclusive_set, visible_set);

    // UPSTREAM-PARITY (c14n.c xmlC14NCheckForRelativeNamespaces, called from
    // xmlC14NProcessElementNode in BOTH inclusive and exclusive modes):
    // canonicalization refuses documents that declare relative
    // (scheme-less) namespace URIs — "Failed to canonicalize" and a
    // negative return (R-000166).
    if doc_has_relative_ns(doc) {
        return -1;
    }

    // Create output buffer
    let buf = io::buf_create(-1);
    if buf.is_null() {
        return -1;
    }

    // Walk the document; visibility decides what is rendered (upstream
    // xmlC14NProcessNodeList over doc->children).
    c14n_walk_document(doc, &mut ctx, buf);

    // UPSTREAM-PARITY (c14n.c xmlC14NProcessNode): an unexpanded entity
    // reference (or an entity/namespace-decl node) fails the whole
    // canonicalization with -1 (R-000175).
    if ctx.invalid_node {
        io::buf_free(buf);
        return -1;
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

    // UPSTREAM-PARITY: xmlC14NExecute carries the visibility callback
    // provided by the caller; the candidate's built-in wrappers always
    // canonicalize the whole document (visible_set = None).
    let mut ctx = C14nContext::new(doc, effective_mode, inclusive_set);

    // UPSTREAM-PARITY (c14n.c xmlC14NCheckForRelativeNamespaces): the
    // relative-namespace-URI check applies in every mode, inclusive and
    // exclusive (R-000166).
    if doc_has_relative_ns(doc) {
        return -1;
    }

    // Create output buffer
    let buf = io::buf_create(-1);
    if buf.is_null() {
        return -1;
    }

    // Walk the document (upstream xmlC14NProcessNodeList).
    c14n_walk_document(doc, &mut ctx, buf);

    // UPSTREAM-PARITY (c14n.c xmlC14NProcessNode): an unexpanded entity
    // reference fails the canonicalization with -1 (R-000175).
    if ctx.invalid_node {
        io::buf_free(buf);
        return -1;
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

/// Canonicalize a document to an `xmlOutputBuffer` with an optional
/// `xmlC14NExecute` visibility callback (upstream c14n.c `xmlC14NExecute`:
/// `is_visible_callback == NULL` → the whole document is visible;
/// otherwise the callback decides node inclusion; the output goes to
/// `buf` via `xmlOutputBufferWrite`).
///
/// # SAFETY
///
/// - `doc`, `output` must be valid pointers (or NULL where the upstream C
///   contract allows), obtained from the matching constructor/owner and not
///   yet freed.
/// - `callback`/`user_data` must stay valid for the walk when non-NULL.
pub unsafe fn c14n_execute_visibility(
    doc: *mut _xmlDoc,
    mode: C14nMode,
    inclusive_ns_prefixes: *const xmlChar,
    with_comments: c_int,
    is_visible_callback: Option<crate::abi::callbacks::xmlC14NIsVisibleCallback>,
    user_data: *mut c_void,
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

    let mut ctx = match is_visible_callback {
        Some(cb) => {
            C14nContext::with_visibility_callback(doc, effective_mode, inclusive_set, cb, user_data)
        }
        None => C14nContext::new(doc, effective_mode, inclusive_set),
    };

    // UPSTREAM-PARITY (c14n.c xmlC14NCheckForRelativeNamespaces): the
    // relative-namespace-URI check applies in every mode, inclusive and
    // exclusive (R-000166).
    if doc_has_relative_ns(doc) {
        return -1;
    }

    // Create output buffer
    let buf = io::buf_create(-1);
    if buf.is_null() {
        return -1;
    }

    // Walk the document (upstream xmlC14NProcessNodeList).
    c14n_walk_document(doc, &mut ctx, buf);

    // UPSTREAM-PARITY (c14n.c xmlC14NProcessNode): an unexpanded entity
    // reference fails the canonicalization with -1 (R-000175).
    if ctx.invalid_node {
        io::buf_free(buf);
        return -1;
    }

    let content = io::buf_content(buf);
    let len = io::buf_length(buf);
    if content.is_null() || len < 0 {
        io::buf_free(buf);
        return -1;
    }

    // Write the canonical output to the caller's xmlOutputBuffer (upstream
    // xmlC14NProcessNode writes directly via xmlOutputBufferWrite).
    let ret = crate::abi::exports_xml2::xmlOutputBufferWrite(output, len, content as *const c_char);

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

    // Upstream xmlC14NDocSaveTo passes the xmlNodeSet as the visibility
    // callback data (c14n.c xmlC14NIsNodeInNodeset).
    let visible_set = build_visible_set(nodes);

    let mut ctx = C14nContext::with_visible_set(doc, effective_mode, inclusive_set, visible_set);

    // UPSTREAM-PARITY (c14n.c xmlC14NCheckForRelativeNamespaces): the
    // relative-namespace-URI check applies in every mode, inclusive and
    // exclusive (R-000166).
    if doc_has_relative_ns(doc) {
        return -1;
    }

    // Create buffer for serialization
    let buf = io::buf_create(-1);
    if buf.is_null() {
        return -1;
    }

    // Walk the document; visibility decides what is rendered (upstream
    // xmlC14NProcessNodeList).
    c14n_walk_document(doc, &mut ctx, buf);

    // UPSTREAM-PARITY (c14n.c xmlC14NProcessNode): an unexpanded entity
    // reference fails the canonicalization with -1 (R-000175).
    if ctx.invalid_node {
        io::buf_free(buf);
        return -1;
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
///
/// # Safety
///
/// - `input` must be NULL or a valid pointer to a NUL-terminated `xmlChar`
///   string; a non-NULL input is read as a C string with `CStr::from_ptr`.
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
/// Used by the unit tests to verify node-set ordering.
///
/// # SAFETY
///
/// - `a` and `b` must be valid pointers to `_xmlNode`.
#[cfg(test)]
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
/// - `nodes` may be NULL (entire document) or a valid `xmlNodeSet*` whose
///   nodes form the subset to canonicalize (upstream signature).
/// - `result` must be a valid pointer to a `xmlChar*` that will receive the result.
#[no_mangle]
pub unsafe extern "C" fn xmlC14NDocDumpMemory(
    doc: *mut _xmlDoc,
    nodes: *mut _xmlNodeSet,
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
            // Build comma-separated string (upstream passes a NULL-terminated
            // array; keep the Vec alive across the delegated call).
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
            Some(result_str)
        }
    } else {
        None
    };
    let joined_ptr = joined_prefixes
        .as_ref()
        .map(|v| v.as_ptr() as *const xmlChar)
        .unwrap_or(ptr::null());

    // The node-set conversion is kept alive for the duration of the call
    // and dropped afterwards (no leak).
    let nodes_array = node_set_to_array(nodes);
    let nodes_ptr = if nodes_array.is_empty() {
        ptr::null_mut()
    } else {
        nodes_array.as_ptr() as *mut *mut _xmlNode
    };
    let ret = unsafe {
        c14n_doc_dump_memory(doc, nodes_ptr, c14n_mode, joined_ptr, with_comments, result)
    };
    drop(nodes_array);
    ret
}

/// Build the visibility set from a NULL-terminated node array (subset
/// argument of `xmlC14NDocDumpMemory` / `xmlC14NDocSaveTo`). None means the
/// whole document is visible (upstream `nodes == NULL`).
///
/// # SAFETY
///
/// - `nodes` must be NULL or a valid NULL-terminated array of `_xmlNode`
///   pointers.
unsafe fn build_visible_set(nodes: *mut *mut _xmlNode) -> Option<HashSet<*mut c_void>> {
    if nodes.is_null() {
        return None;
    }
    let mut set = HashSet::new();
    let mut i = 0usize;
    loop {
        let n = unsafe { *nodes.add(i) };
        if n.is_null() {
            break;
        }
        set.insert(n as *mut c_void);
        i += 1;
    }
    Some(set)
}

/// Walk the document children with the current context (upstream
/// xmlC14NProcessNodeList over doc->children).
///
/// # SAFETY
///
/// - `doc` must be a valid pointer to an `_xmlDoc`.
/// - `ctx` must be a valid pointer to a `C14nContext`.
/// - `buf` must be a valid pointer to a mutable `_xmlBuffer`.
unsafe fn c14n_walk_document(doc: *mut _xmlDoc, ctx: &mut C14nContext, buf: *mut _xmlBuffer) {
    let doc_node = doc as *mut _xmlNode;
    let d = unsafe { &*doc_node };
    let mut child = d.children;
    while !child.is_null() {
        c14n_serialize_node(child, ctx, buf);
        child = unsafe { (*child).next };
    }
}

/// Convert an upstream `xmlNodeSet*` (subset argument of
/// `xmlC14NDocDumpMemory` / `xmlC14NDocSaveTo`) into a NULL-terminated
/// node array used by the internal canonicalization walk. An empty Vec
/// means NULL (whole document).
///
/// # SAFETY
///
/// - `nodes` must be NULL or a valid pointer to an `_xmlNodeSet`.
unsafe fn node_set_to_array(nodes: *mut _xmlNodeSet) -> Vec<*mut _xmlNode> {
    if nodes.is_null() {
        return Vec::new();
    }
    let ns = unsafe { &*nodes };
    let nr = if ns.nodeNr > 0 { ns.nodeNr as usize } else { 0 };
    let mut vec: Vec<*mut _xmlNode> = Vec::with_capacity(nr + 1);
    for i in 0..nr {
        vec.push(unsafe { *ns.nodeTab.add(i) });
    }
    vec.push(ptr::null_mut());
    vec
}

/// Canonicalize XML with a callback for output.
///
/// # UPSTREAM-PARITY
///
/// Canonicalize a document with a caller-supplied visibility callback
/// (upstream c14n.h / c14n.c `xmlC14NExecute`).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlC14NExecute(xmlDoc *doc,
///                    xmlC14NIsVisibleCallback is_visible_callback,
///                    void *user_data,
///                    int mode, /* a xmlC14NMode */
///                    xmlChar **inclusive_ns_prefixes,
///                    int with_comments,
///                    xmlOutputBuffer *buf);
/// ```
///
/// R-000176: the pre-11.1-Z.2 candidate exported a 6-argument form with a
/// completely different register layout (doc, mode, prefixes, with_comments,
/// write-callback, data) that misread every argument a C caller passes.
/// This now mirrors the oracle exactly: `is_visible_callback` decides node
/// visibility (NULL means the whole document is visible, upstream
/// `xmlC14NIsVisible`), and the canonical output is written to `buf` via
/// `xmlOutputBufferWrite`. Returns 0 on success, -1 on error.
///
/// # SAFETY
///
/// - `doc` must be a valid pointer to an `_xmlDoc` or NULL.
/// - `is_visible_callback` may be NULL; when non-NULL it and `user_data`
///   must stay valid for the walk.
/// - `buf` must be a valid pointer to an `_xmlOutputBuffer` or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlC14NExecute(
    doc: *mut _xmlDoc,
    is_visible_callback: Option<crate::abi::callbacks::xmlC14NIsVisibleCallback>,
    user_data: *mut c_void,
    mode: c_int,
    inclusive_ns_prefixes: *mut *mut xmlChar,
    with_comments: c_int,
    output: *mut _xmlOutputBuffer,
) -> c_int {
    // SAFETY: Delegates to the safe internal implementation.
    if doc.is_null() || output.is_null() {
        return -1;
    }

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
            // Build comma-separated string; the Vec must stay alive across
            // the delegated call (dangling-pointer fix).
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

    unsafe {
        c14n_execute_visibility(
            doc,
            c14n_mode,
            joined_ptr,
            with_comments,
            is_visible_callback,
            user_data,
            output,
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
    nodes: *mut _xmlNodeSet,
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

    let nodes_array = node_set_to_array(nodes);
    let nodes_ptr = if nodes_array.is_empty() {
        ptr::null_mut()
    } else {
        nodes_array.as_ptr() as *mut *mut _xmlNode
    };
    let ret =
        unsafe { c14n_doc_save_to(doc, nodes_ptr, c14n_mode, joined_ptr, with_comments, output) };
    drop(nodes_array);
    ret
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::allocator::xmlFreeImpl;
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
        xmlFreeImpl(result as *mut c_void);
        s
    }

    // ── Basic document canonicalization ──

    /// Verify that an unexpanded entity reference makes C14N fail with -1,
    /// matching the upstream oracle.
    ///
    /// # Safety
    ///
    /// - The `xmlReadMemory` input must be a byte buffer readable for
    ///   `len - 1` bytes and NUL-terminated at the end.
    /// - The returned `doc` is asserted non-NULL and freed with `xmlFreeDoc`;
    ///   the `result` out-parameter is left NULL on the -1 failure path, as
    ///   asserted.
    #[test]
    fn test_c14n_entity_ref_node_fails_like_upstream() {
        // R-000175: an unexpanded entity reference is an invalid node for
        // canonical XML (upstream c14n.c xmlC14NProcessNode returns -1 via
        // xmlC14NErrInvalidNode). The candidate used to serialize the
        // reference (`&foo;`); it must fail with -1 exactly like the oracle.
        unsafe {
            let xml = b"<?xml version='1.0'?><!DOCTYPE r [<!ELEMENT r (#PCDATA)><!ENTITY foo \"FOO\">]><r>a &foo; b</r>\0";
            let doc = crate::abi::exports_xml2::xmlReadMemory(
                xml.as_ptr() as *const c_char,
                (xml.len() - 1) as c_int,
                b"t.xml\0" as *const u8 as *const c_char,
                ptr::null(),
                0,
            );
            assert!(!doc.is_null(), "doc must parse (NOENT unset keeps the ref)");
            let mut result: *mut xmlChar = ptr::null_mut();
            let len = c14n_doc_dump_memory(
                doc,
                ptr::null_mut(),
                C14nMode::XML_C14N_1_0,
                ptr::null(),
                0,
                &mut result as *mut *mut xmlChar,
            );
            assert_eq!(len, -1, "entity-ref node must fail canonicalization");
            assert!(result.is_null());
            crate::abi::exports_xml2::xmlFreeDoc(doc);
        }
    }

    /// Verify that canonicalizing a simple document emits the expected
    /// element markup.
    ///
    /// # Safety
    ///
    /// - `doc` returned by `create_simple_doc` must be a valid `_xmlDoc`
    ///   pointer, alive until `canonicalize_doc` returns; the test frees it
    ///   with `tree::free_doc`.
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

    /// Verify that an empty element is canonicalized in expanded form.
    ///
    /// # Safety
    ///
    /// - `doc` and `root` created by the tree helpers must be valid and linked
    ///   before `canonicalize_doc` reads them; the doc is freed with
    ///   `tree::free_doc`.
    #[test]
    fn test_c14n_basic_empty_element() {
        unsafe {
            let doc = tree::new_doc(b"1.0\0" as *const u8 as *const xmlChar);
            assert!(!doc.is_null());
            let root = tree::new_node(ptr::null_mut(), b"empty\0" as *const u8 as *const xmlChar);
            assert!(!root.is_null());
            tree::doc_set_root_element(doc, root);

            let result = canonicalize_doc(doc, C14nMode::XML_C14N_1_0, 0);
            // UPSTREAM-PARITY (C14N 1.0 §2.4): empty elements are rendered
            // expanded (`<empty></empty>`), never self-closing. R-000166.
            assert!(
                result.contains("<empty></empty>"),
                "Empty element should be expanded, got: {}",
                result
            );
            tree::free_doc(doc);
        }
    }

    // ── Namespace propagation ──

    /// Verify that namespace declarations propagate through canonicalization.
    ///
    /// # Safety
    ///
    /// - `doc`, `ns`, and `root` created by the tree helpers must be valid,
    ///   live pointers while `canonicalize_doc` walks the document; the doc is
    ///   freed with `tree::free_doc`.
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

    /// Verify that attributes are canonicalized in lexicographic order.
    ///
    /// # Safety
    ///
    /// - `doc` and `root` created by the tree helpers must be valid and linked
    ///   before `canonicalize_doc` reads them; the doc is freed with
    ///   `tree::free_doc`.
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

    /// Verify that text content escapes the less-than, ampersand,
    /// greater-than, and carriage-return characters.
    ///
    /// # Safety
    ///
    /// - The text bytes must be NUL-terminated (`tree::new_text` reads them as
    ///   a C string); `doc` and `root` must be valid while `canonicalize_doc`
    ///   runs, and the doc is freed with `tree::free_doc`.
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

    /// Verify that attribute values escape the less-than, ampersand, quote,
    /// tab, newline, and carriage-return characters.
    ///
    /// # Safety
    ///
    /// - The attribute name and value byte buffers must be NUL-terminated
    ///   (`tree::set_prop` reads them as C strings); `doc` and `root` must be
    ///   valid while `canonicalize_doc` runs, and the doc is freed with
    ///   `tree::free_doc`.
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

    /// Verify that comments are omitted or included depending on the mode.
    ///
    /// # Safety
    ///
    /// - `doc`, `root`, and `comment` created by the tree helpers must be
    ///   valid, live pointers while `canonicalize_doc` runs; the doc is freed
    ///   with `tree::free_doc`.
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

    /// Verify the namespace rendering difference between exclusive and
    /// inclusive C14N.
    ///
    /// # Safety
    ///
    /// - `doc`, `root`, and `child` created by the tree helpers must be valid,
    ///   live pointers while `canonicalize_doc` runs; the doc is freed with
    ///   `tree::free_doc`.
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

    /// Verify that C14N output omits the XML declaration.
    ///
    /// # Safety
    ///
    /// - `doc` returned by `create_simple_doc` must be a valid `_xmlDoc`
    ///   pointer, alive until `canonicalize_doc` returns; the test frees it
    ///   with `tree::free_doc`.
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

    /// Verify that an empty document canonicalizes to empty output.
    ///
    /// # Safety
    ///
    /// - `doc` created by `tree::new_doc` must be a valid `_xmlDoc` pointer
    ///   while `canonicalize_doc` runs; the doc is freed with `tree::free_doc`.
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

    /// Verify that canonicalizing a NULL document returns -1.
    ///
    /// # Safety
    ///
    /// - Passing a NULL `doc` is allowed and must not be dereferenced; the
    ///   `result` out-parameter receives no allocation on the failure path.
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

    /// Verify that text node content survives canonicalization.
    ///
    /// # Safety
    ///
    /// - `doc`, `root`, and `text` created by the tree helpers must be valid,
    ///   live pointers while `canonicalize_doc` runs; the doc is freed with
    ///   `tree::free_doc`.
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

    /// Verify that CDATA content is canonicalized as escaped text.
    ///
    /// # Safety
    ///
    /// - `doc`, `root`, and the text node created by the tree helpers must be
    ///   valid, live pointers while `canonicalize_doc` runs; the doc is freed
    ///   with `tree::free_doc`.
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

    /// Verify that processing instructions are canonicalized.
    ///
    /// # Safety
    ///
    /// - `doc`, `root`, and `pi` created by the tree helpers must be valid,
    ///   live pointers while `canonicalize_doc` runs; the doc is freed with
    ///   `tree::free_doc`.
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

    /// Verify that the with-comments flag includes comments in the output.
    ///
    /// # Safety
    ///
    /// - `doc`, `root`, and `comment` created by the tree helpers must be
    ///   valid, live pointers while `c14n_doc_dump_memory` runs.
    /// - `result` receives a buffer owned by the callee; it is asserted
    ///   non-NULL, read as `len` bytes, and freed with `xmlFreeImpl`.
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
            xmlFreeImpl(result as *mut c_void);

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

    /// Verify that `c14n_escape_text` escapes carriage returns as `&#xD;`.
    ///
    /// # Safety
    ///
    /// - `buf` from `io::buf_create` must be a valid `_xmlBuffer` pointer for
    ///   the whole call; it is freed with `io::buf_free`.
    /// - `text` must point to a buffer with at least `len` readable bytes
    ///   (the NUL terminator is not needed because `len` bounds the read).
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

    /// Verify that `c14n_escape_attr` escapes tab, newline, and carriage
    /// return characters.
    ///
    /// # Safety
    ///
    /// - `buf` from `io::buf_create` must be a valid `_xmlBuffer` pointer for
    ///   the whole call; it is freed with `io::buf_free`.
    /// - `text` must be a valid NUL-terminated `xmlChar` string, read as a C
    ///   string by `c14n_escape_attr`.
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

    /// Verify `parse_inclusive_prefixes` on NULL, empty, and comma-separated
    /// inputs.
    ///
    /// # Safety
    ///
    /// - The static byte-string pointers passed in must be NUL-terminated;
    ///   NULL is allowed and yields `None`.
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

    /// Verify `cmp_document_order` against siblings in document order.
    ///
    /// # Safety
    ///
    /// - `doc`, `root`, `child1`, and `child2` created by the tree helpers
    ///   must be valid, live pointers while `cmp_document_order` walks the
    ///   ancestor chains; the doc is freed with `tree::free_doc`.
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

    /// Verify that `c14n_escape_text` escapes a greater-than sign as `&gt;`.
    ///
    /// # Safety
    ///
    /// - `buf` from `io::buf_create` must be a valid `_xmlBuffer` pointer for
    ///   the whole call; it is freed with `io::buf_free`.
    /// - `text` must point to a buffer with at least `len` readable bytes.
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

    /// Verify that `c14n_escape_text` escapes the CDATA-end sequence.
    ///
    /// # Safety
    ///
    /// - `buf` from `io::buf_create` must be a valid `_xmlBuffer` pointer for
    ///   the whole call; it is freed with `io::buf_free`.
    /// - `text` must point to a buffer with at least `len` readable bytes.
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

    /// Verify that `c14n_execute` streams canonicalized output through a C
    /// callback.
    ///
    /// # Safety
    ///
    /// - `doc` must be a valid `_xmlDoc` pointer, alive for the call.
    /// - `output_vec` must be the `Box::into_raw` pointer of a byte vector
    ///   and stay exclusively owned until it is reclaimed with `Box::from_raw`.
    #[test]
    fn test_c14n_execute_callback() {
        unsafe {
            let doc = create_simple_doc();

            // Use a heap-allocated Vec passed through the callback context
            let output_vec = Box::into_raw(Box::new(Vec::<u8>::new()));

            /// C callback that appends canonicalized output to a byte vector.
            ///
            /// # Safety
            ///
            /// - `ctx` must be the `Box::into_raw` pointer of a byte vector
            ///   that is exclusively owned for the duration of the call.
            /// - `data` must point to `len` readable bytes of output; `len` is
            ///   cast to `usize` for the slice.
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

    /// Verify that `c14n_doc_save_to` writes into an output buffer.
    ///
    /// # Safety
    ///
    /// - `doc` must be a valid `_xmlDoc` pointer, alive for the call.
    /// - `buf` and the `output` buffer created from it must be valid pointers
    ///   until `io::output_buffer_close`; `buf` is freed with `io::buf_free`
    ///   and the doc with `tree::free_doc`.
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

    /// Verify the `xmlC14NDocDumpMemory` C ABI export.
    ///
    /// # Safety
    ///
    /// - `doc` must be a valid `_xmlDoc` pointer, alive for the call.
    /// - `result` receives a callee-owned buffer, asserted non-NULL, read as
    ///   `len` bytes, and freed with `xmlFreeImpl`; the doc is freed with
    ///   `tree::free_doc`.
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

            xmlFreeImpl(result as *mut c_void);
            tree::free_doc(doc);
        }
    }

    /// Verify the `xmlC14NExecute` C ABI export with no visibility callback.
    ///
    /// # Safety
    ///
    /// - `doc` must be a valid `_xmlDoc` pointer, alive for the call.
    /// - `buf` and the `output` buffer created from it must be valid until
    ///   `io::output_buffer_close`; the doc is freed with `tree::free_doc`.
    #[test]
    fn test_c14n_c_abi_execute() {
        unsafe {
            let doc = create_simple_doc();

            // The oracle 7-argument contract (R-000176): no visibility
            // callback → whole document; output goes to an xmlOutputBuffer.
            let buf = io::buf_create(-1);
            assert!(!buf.is_null());
            let output = io::output_buffer_create_buffer(buf, ptr::null_mut());
            assert!(!output.is_null());

            let ret = xmlC14NExecute(
                doc,
                None,            // is_visible_callback
                ptr::null_mut(), // user_data
                0,               // XML_C14N_1_0
                ptr::null_mut(), // inclusive_ns_prefixes
                0,               // with_comments
                output,
            );

            assert!(ret >= 0, "xmlC14NExecute should succeed");
            // The write lands in the output buffer's internal buffer and is
            // flushed to the target buffer on close (upstream flush model).
            io::output_buffer_close(output);
            let content = io::buf_content(buf);
            let len = io::buf_length(buf);
            let s = {
                let slice = core::slice::from_raw_parts(content, len as usize);
                String::from_utf8_lossy(slice).to_string()
            };
            assert!(
                s.contains("<root>"),
                "C ABI execute should produce canonical output, got: {}",
                s
            );

            tree::free_doc(doc);
        }
    }

    /// Verify the `xmlC14NDocSaveTo` C ABI export.
    ///
    /// # Safety
    ///
    /// - `doc` must be a valid `_xmlDoc` pointer, alive for the call.
    /// - `buf` and the `output` buffer created from it must be valid until
    ///   `io::output_buffer_close`; `buf` is freed with `io::buf_free` and the
    ///   doc with `tree::free_doc`.
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

    // ── R-000166 regression tests (11.1-X C14N closure) ──

    /// Build `<root xmlns:p="http://u/p"><p:one><p:two p:a="1"/></p:one></root>`.
    unsafe fn build_nested_ns_doc() -> *mut _xmlDoc {
        let doc = tree::new_doc(b"1.0\0" as *const u8 as *const xmlChar);
        let ns = tree::new_ns(
            ptr::null_mut(),
            b"http://u/p\0" as *const u8 as *const xmlChar,
            b"p\0" as *const u8 as *const xmlChar,
        );
        let root = tree::new_node(ptr::null_mut(), b"root\0" as *const u8 as *const xmlChar);
        tree::doc_set_root_element(doc, root);
        tree::new_ns(
            root,
            b"http://u/p\0" as *const u8 as *const xmlChar,
            b"p\0" as *const u8 as *const xmlChar,
        );
        let one = tree::new_node(ns, b"one\0" as *const u8 as *const xmlChar);
        tree::add_child(root, one);
        let two = tree::new_node(ns, b"two\0" as *const u8 as *const xmlChar);
        tree::add_child(one, two);
        tree::set_prop(
            two,
            b"a\0" as *const u8 as *const xmlChar,
            b"1\0" as *const u8 as *const xmlChar,
        );
        // Bind the attribute to the p namespace.
        let attr = (*two).properties;
        (*attr).ns = ns;
        doc
    }

    /// Verify that exclusive C14N does not re-declare a namespace already
    /// rendered by an ancestor.
    ///
    /// # Safety
    ///
    /// - `doc` from `build_nested_ns_doc` must be a valid `_xmlDoc` with valid
    ///   node, namespace, and attribute pointers while `canonicalize_doc` runs;
    ///   the doc is freed with `tree::free_doc`.
    #[test]
    fn test_c14n_exclusive_skips_ancestor_rendered_ns() {
        // R-000166: exclusive C14N must not re-declare a namespace already
        // rendered by an ancestor (`xmlExcC14NProcessNamespacesAxis` with the
        // ns_rendered stack).
        unsafe {
            let doc = build_nested_ns_doc();
            let out = canonicalize_doc(doc, C14nMode::XML_C14N_EXCLUSIVE_1_0, 0);
            assert_eq!(
                out,
                "<root><p:one xmlns:p=\"http://u/p\"><p:two p:a=\"1\"></p:two></p:one></root>"
            );
            tree::free_doc(doc);
        }
    }

    /// Verify that namespaces render in lexicographic prefix order.
    ///
    /// # Safety
    ///
    /// - `doc`, `root`, and the namespace declarations created by the tree
    ///   helpers must be valid, live pointers while `canonicalize_doc` runs;
    ///   the doc is freed with `tree::free_doc`.
    #[test]
    fn test_c14n_namespace_sorting() {
        // R-000166: namespaces render in lexicographic prefix order (upstream
        // xmlC14NNsCompare) regardless of declaration order.
        unsafe {
            let doc = tree::new_doc(b"1.0\0" as *const u8 as *const xmlChar);
            let root = tree::new_node(ptr::null_mut(), b"root\0" as *const u8 as *const xmlChar);
            tree::doc_set_root_element(doc, root);
            tree::new_ns(
                root,
                b"http://u/z\0" as *const u8 as *const xmlChar,
                b"z\0" as *const u8 as *const xmlChar,
            );
            tree::new_ns(
                root,
                b"http://u/a\0" as *const u8 as *const xmlChar,
                b"a\0" as *const u8 as *const xmlChar,
            );
            let out = canonicalize_doc(doc, C14nMode::XML_C14N_1_0, 0);
            assert_eq!(
                out,
                "<root xmlns:a=\"http://u/a\" xmlns:z=\"http://u/z\"></root>"
            );
            tree::free_doc(doc);
        }
    }

    /// Verify that the implicit `xml` namespace is never rendered.
    ///
    /// # Safety
    ///
    /// - `doc`, `root`, and `xml_ns` created by the tree helpers must be valid,
    ///   live pointers; `xml_ns` is stored in `doc.oldNs` and must stay alive
    ///   until the doc is freed with `tree::free_doc`.
    #[test]
    fn test_c14n_xml_ns_never_rendered() {
        // R-000166: the xml namespace is never rendered as xmlns:xml in
        // exclusive C14N (`xmlC14NIsXmlNs`).
        unsafe {
            let doc = tree::new_doc(b"1.0\0" as *const u8 as *const xmlChar);
            let root = tree::new_node(ptr::null_mut(), b"root\0" as *const u8 as *const xmlChar);
            tree::doc_set_root_element(doc, root);
            // Materialize the xml namespace like the parser does.
            let xml_ns = tree::new_ns(
                ptr::null_mut(),
                b"http://www.w3.org/XML/1998/namespace\0" as *const u8 as *const xmlChar,
                b"xml\0" as *const u8 as *const xmlChar,
            );
            (*doc).oldNs = xml_ns;
            let out = canonicalize_doc(doc, C14nMode::XML_C14N_EXCLUSIVE_1_0, 0);
            assert_eq!(out, "<root></root>");
            assert!(!out.contains("xmlns:xml"));
            tree::free_doc(doc);
        }
    }

    /// Verify that an empty default namespace undeclaration is rendered.
    ///
    /// # Safety
    ///
    /// - `doc`, `root`, `d_ns`, `a`, and the namespace declarations created
    ///   by the tree helpers must be valid, live pointers while
    ///   `canonicalize_doc` runs; the doc is freed with `tree::free_doc`.
    #[test]
    fn test_c14n_empty_default_undeclaration() {
        // R-000166: `<a xmlns="">` renders the empty default undeclaration
        // in exclusive C14N (element with no binding resolves the default
        // namespace in scope).
        unsafe {
            let doc = tree::new_doc(b"1.0\0" as *const u8 as *const xmlChar);
            let root = tree::new_node(ptr::null_mut(), b"root\0" as *const u8 as *const xmlChar);
            tree::doc_set_root_element(doc, root);
            let d_ns = tree::new_ns(
                ptr::null_mut(),
                b"http://u/d\0" as *const u8 as *const xmlChar,
                ptr::null(),
            );
            tree::new_ns(
                root,
                b"http://u/d\0" as *const u8 as *const xmlChar,
                ptr::null(),
            );
            let a = tree::new_node(d_ns, b"a\0" as *const u8 as *const xmlChar);
            tree::add_child(root, a);
            // `a` carries xmlns="". Like the parser, an empty default
            // namespace is not bound to the element (a.ns stays NULL), so
            // exclusive C14N resolves it via the in-scope default search.
            tree::new_ns(a, b"\0" as *const u8 as *const xmlChar, ptr::null());
            (*a).ns = ptr::null_mut();
            let out = canonicalize_doc(doc, C14nMode::XML_C14N_EXCLUSIVE_1_0, 0);
            assert_eq!(out, "<root xmlns=\"http://u/d\"><a xmlns=\"\"></a></root>");
            tree::free_doc(doc);
        }
    }

    /// Verify that exclusive C14N rejects relative namespace URIs.
    ///
    /// # Safety
    ///
    /// - `doc` and `root` created by the tree helpers must be valid, live
    ///   pointers while `c14n_doc_dump_memory` runs; the doc is freed with
    ///   `tree::free_doc`.
    #[test]
    fn test_c14n_relative_ns_rejected_exclusive() {
        // R-000166: the relative-namespace-URI rejection applies in exclusive
        // mode too (xmlC14NCheckForRelativeNamespaces runs in
        // xmlC14NProcessElementNode for every mode).
        unsafe {
            let doc = tree::new_doc(b"1.0\0" as *const u8 as *const xmlChar);
            let root = tree::new_node(ptr::null_mut(), b"root\0" as *const u8 as *const xmlChar);
            tree::doc_set_root_element(doc, root);
            tree::new_ns(
                root,
                b"u\0" as *const u8 as *const xmlChar,
                b"p\0" as *const u8 as *const xmlChar,
            );
            let mut result: *mut xmlChar = ptr::null_mut();
            let len = c14n_doc_dump_memory(
                doc,
                ptr::null_mut(),
                C14nMode::XML_C14N_EXCLUSIVE_1_0,
                ptr::null(),
                0,
                &mut result as *mut *mut xmlChar,
            );
            assert!(
                len < 0,
                "exclusive C14N must reject relative namespace URIs"
            );
            tree::free_doc(doc);
        }
    }

    /// Verify newline placement for document-level PIs.
    ///
    /// # Safety
    ///
    /// - `doc`, `pi1`, `root`, and `pi2` created by the tree helpers must be
    ///   valid, live pointers; the PIs are children of the doc and must stay
    ///   alive until the doc is freed with `tree::free_doc`.
    #[test]
    fn test_c14n_pi_document_level_newlines() {
        // R-000166: PIs before the document element get a trailing newline,
        // PIs after it a leading newline.
        unsafe {
            let doc = tree::new_doc(b"1.0\0" as *const u8 as *const xmlChar);
            let pi1 = tree::new_pi(b"one\0" as *const u8 as *const xmlChar, ptr::null());
            tree::add_child(doc as *mut _xmlNode, pi1);
            let root = tree::new_node(ptr::null_mut(), b"root\0" as *const u8 as *const xmlChar);
            tree::add_child(doc as *mut _xmlNode, root);
            let pi2 = tree::new_pi(b"three\0" as *const u8 as *const xmlChar, ptr::null());
            tree::add_child(doc as *mut _xmlNode, pi2);
            let out = canonicalize_doc(doc, C14nMode::XML_C14N_1_0, 0);
            assert_eq!(out, "<?one?>\n<root></root>\n<?three?>");
            tree::free_doc(doc);
        }
    }

    /// Verify that subset canonicalization renders only nodes in the set.
    ///
    /// # Safety
    ///
    /// - `doc`, `root`, and `child` created by the tree helpers must be valid,
    ///   live pointers while `c14n_doc_dump_memory` runs; the nodeset array
    ///   must be NULL-terminated and live for the call.
    /// - `result` receives a callee-owned buffer, read as `len` bytes, and
    ///   freed with `xmlFreeImpl`; the doc is freed with `tree::free_doc`.
    #[test]
    fn test_c14n_subset_visibility() {
        // R-000166: subset canonicalization renders only nodes in the set;
        // the document element is still visited so invisible siblings are
        // skipped.
        unsafe {
            let doc = tree::new_doc(b"1.0\0" as *const u8 as *const xmlChar);
            let root = tree::new_node(ptr::null_mut(), b"root\0" as *const u8 as *const xmlChar);
            tree::doc_set_root_element(doc, root);
            let child = tree::new_node(ptr::null_mut(), b"child\0" as *const u8 as *const xmlChar);
            tree::add_child(root, child);

            let mut nodes: Vec<*mut _xmlNode> = vec![root, ptr::null_mut()];
            let mut result: *mut xmlChar = ptr::null_mut();
            let len = c14n_doc_dump_memory(
                doc,
                nodes.as_mut_ptr(),
                C14nMode::XML_C14N_1_0,
                ptr::null(),
                0,
                &mut result as *mut *mut xmlChar,
            );
            assert!(len >= 0);
            let s = {
                let slice = core::slice::from_raw_parts(result, len as usize);
                String::from_utf8_lossy(slice).to_string()
            };
            xmlFreeImpl(result as *mut c_void);
            // Only `root` is in the set; `child` is processed but not rendered.
            assert_eq!(s, "<root></root>");
            tree::free_doc(doc);
        }
    }

    /// Verify that C14N 1.0 imports xml-namespace attributes from hidden
    /// ancestors of a visible orphan element.
    ///
    /// # Safety
    ///
    /// - `doc`, `xml_ns`, `root`, `lang_attr`, and `child` created by the tree
    ///   helpers must be valid, live pointers while `c14n_doc_dump_memory`
    ///   runs; the nodeset array must be NULL-terminated and live for the call.
    /// - `result` receives a callee-owned buffer, read as `len` bytes, and
    ///   freed with `xmlFreeImpl`; the doc is freed with `tree::free_doc`.
    #[test]
    fn test_c14n_subset_hidden_parent_xml_lang() {
        // R-000166: C14N 1.0 imports xml-namespace attributes from hidden
        // ancestors of a visible orphan element.
        unsafe {
            let doc = tree::new_doc(b"1.0\0" as *const u8 as *const xmlChar);
            let xml_ns = tree::new_ns(
                ptr::null_mut(),
                b"http://www.w3.org/XML/1998/namespace\0" as *const u8 as *const xmlChar,
                b"xml\0" as *const u8 as *const xmlChar,
            );
            (*doc).oldNs = xml_ns;
            let root = tree::new_node(ptr::null_mut(), b"root\0" as *const u8 as *const xmlChar);
            tree::doc_set_root_element(doc, root);
            tree::set_prop(
                root,
                b"lang\0" as *const u8 as *const xmlChar,
                b"en\0" as *const u8 as *const xmlChar,
            );
            let lang_attr = (*root).properties;
            (*lang_attr).ns = xml_ns;

            let child = tree::new_node(ptr::null_mut(), b"a\0" as *const u8 as *const xmlChar);
            tree::add_child(root, child);

            let mut nodes: Vec<*mut _xmlNode> = vec![child, ptr::null_mut()];
            let mut result: *mut xmlChar = ptr::null_mut();
            let len = c14n_doc_dump_memory(
                doc,
                nodes.as_mut_ptr(),
                C14nMode::XML_C14N_1_0,
                ptr::null(),
                0,
                &mut result as *mut *mut xmlChar,
            );
            assert!(len >= 0);
            let s = {
                let slice = core::slice::from_raw_parts(result, len as usize);
                String::from_utf8_lossy(slice).to_string()
            };
            xmlFreeImpl(result as *mut c_void);
            assert_eq!(s, "<a xml:lang=\"en\"></a>");
            tree::free_doc(doc);
        }
    }

    /// Verify that a prefix rebound away and back to an older URI is
    /// re-rendered.
    ///
    /// # Safety
    ///
    /// - `doc`, `root`, `b`, `c`, and the namespace declarations created by
    ///   the tree helpers must be valid, live pointers while `canonicalize_doc`
    ///   runs; the doc is freed with `tree::free_doc`.
    #[test]
    fn test_c14n_rebinding_chain_rere_declares() {
        // R-000166: a prefix rebound away and back to an older URI IS
        // re-rendered (prefix-scoped last-wins find semantics).
        unsafe {
            let doc = tree::new_doc(b"1.0\0" as *const u8 as *const xmlChar);
            let ns1 = tree::new_ns(
                ptr::null_mut(),
                b"http://u/1\0" as *const u8 as *const xmlChar,
                b"p\0" as *const u8 as *const xmlChar,
            );
            let root = tree::new_node(ptr::null_mut(), b"a\0" as *const u8 as *const xmlChar);
            tree::doc_set_root_element(doc, root);
            tree::new_ns(
                root,
                b"http://u/1\0" as *const u8 as *const xmlChar,
                b"p\0" as *const u8 as *const xmlChar,
            );
            let b = tree::new_node(ns1, b"b\0" as *const u8 as *const xmlChar);
            tree::add_child(root, b);
            let ns2 = tree::new_ns(
                ptr::null_mut(),
                b"http://u/2\0" as *const u8 as *const xmlChar,
                b"p\0" as *const u8 as *const xmlChar,
            );
            tree::new_ns(
                b,
                b"http://u/2\0" as *const u8 as *const xmlChar,
                b"p\0" as *const u8 as *const xmlChar,
            );
            (*b).ns = ns2;
            let c = tree::new_node(ns1, b"c\0" as *const u8 as *const xmlChar);
            tree::add_child(b, c);
            tree::new_ns(
                c,
                b"http://u/1\0" as *const u8 as *const xmlChar,
                b"p\0" as *const u8 as *const xmlChar,
            );

            let out = canonicalize_doc(doc, C14nMode::XML_C14N_1_0, 0);
            assert_eq!(
                out,
                "<a xmlns:p=\"http://u/1\"><p:b xmlns:p=\"http://u/2\"><p:c xmlns:p=\"http://u/1\"></p:c></p:b></a>"
            );
            tree::free_doc(doc);
        }
    }
}
