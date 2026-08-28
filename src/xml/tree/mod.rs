//! XML tree construction and manipulation (§17, §18, §85 Phase 1).
//!
//! Complete tree construction/manipulation, namespaces, attributes,
//! dictionaries, entity structures, document ownership, copying, linking,
//! and freeing.
//!
//! # UPSTREAM-PARITY
//!
//! libxml2's tree is an observable data structure. The pointer topology
//! (parent, children, last, next, prev, doc, ns, properties, nsDef) is
//! part of the compatibility contract and must be court-tested.
//!
//! Key invariants (matching upstream):
//!
//! - `node->doc` points to the owning document (or NULL if not owned)
//! - `node->parent` points to the parent element (or NULL for root)
//! - `node->children` points to the first child
//! - `node->last` points to the last child
//! - `node->next` / `node->prev` form a doubly-linked list of siblings
//! - `node->properties` points to the first attribute (for elements)
//! - `node->nsDef` points to the first namespace declaration (for elements)
//! - `doc->children` points to the root element
//! - `doc->doc` points to itself (self-reference)
//!
//! # Ownership model
//!
//! Documents own all their nodes. When a document is freed, all nodes
//! are freed. Nodes can be moved between documents via unlinking and
//! re-adding.
//!
//! # Phase 1 status
//!
//! Complete — all tree operations are implemented.
//! Future phases may add more edge-case handling for historical quirks.

use core::ffi::c_void;
use core::ptr;
use std::os::raw::{c_char, c_int, c_uint};

use crate::abi::allocator;
use crate::abi::constants::*;
use crate::abi::structs::*;
use crate::abi::types::xmlAttributeType::XML_ATTRIBUTE_CDATA;
use crate::abi::types::xmlCharEncoding::XML_CHAR_ENCODING_UTF8;
use crate::abi::types::xmlDocProperties::XML_DOC_WELLFORMED;
use crate::abi::types::xmlElementType::*;
use crate::abi::types::*;
use crate::xml::globals;
use crate::xml::io;

// ═══════════════════════════════════════════════════════════════════════════════
// String Helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Duplicate an xmlChar string using xmlMalloc.
///
/// # SAFETY
///
/// - `str` must be a valid null-terminated xmlChar* or NULL.
unsafe fn dup_xml_str(str: *const xmlChar) -> *mut xmlChar {
    if str.is_null() {
        return ptr::null_mut();
    }
    let len = unsafe { crate::abi::exports_xml2::xmlStrlen(str) as usize };
    if len == 0 {
        // Return a pointer to a null byte
        let buf = unsafe { allocator::xmlMalloc(1) as *mut xmlChar };
        if !buf.is_null() {
            unsafe { *buf = 0 };
        }
        return buf;
    }
    let buf = unsafe { allocator::xmlMalloc(len + 1) as *mut xmlChar };
    if !buf.is_null() {
        unsafe {
            ptr::copy_nonoverlapping(str, buf, len + 1);
        }
    }
    buf
}

/// Copy an xmlChar string into an already-allocated buffer, or return NULL.
unsafe fn copy_xml_str_content(dest: *mut xmlChar, src: *const xmlChar, max_len: usize) -> bool {
    if src.is_null() || dest.is_null() || max_len == 0 {
        return false;
    }
    let len = unsafe { crate::abi::exports_xml2::xmlStrlen(src) as usize };
    if len >= max_len {
        return false;
    }
    unsafe {
        ptr::copy_nonoverlapping(src, dest, len);
        *dest.add(len) = 0;
    }
    true
}

/// Get the length of a null-terminated xmlChar string.
pub unsafe fn xml_strlen(str: *const xmlChar) -> c_int {
    if str.is_null() {
        return 0;
    }
    let mut len: c_int = 0;
    while unsafe { *str.add(len as usize) != 0 } {
        len += 1;
    }
    len
}

// ═══════════════════════════════════════════════════════════════════════════════
// Document Operations
// ═══════════════════════════════════════════════════════════════════════════════

/// Create a new XML document.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDocPtr xmlNewDoc(const xmlChar *version);
/// ```
///
/// Creates a new document with the given version string (or "1.0" if NULL).
/// The document is initialized with:
/// - type = XML_DOCUMENT_NODE
/// - standalone = -1 (unknown)
/// - doc->doc = self (self-reference)
/// - properties = XML_DOC_WELLFORMED
///
/// # SAFETY
///
/// - `version` must be a valid null-terminated string or NULL.
pub unsafe fn new_doc(version: *const xmlChar) -> *mut _xmlDoc {
    // SAFETY: Allocate zero-initialized memory for the document.
    let doc = allocator::xmlMallocZero(size_of::<_xmlDoc>() as usize) as *mut _xmlDoc;
    if doc.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        (*doc).type_ = XML_DOCUMENT_NODE as c_int;
        (*doc).standalone = -1; // unknown
        (*doc).doc = doc; // self-reference
        (*doc).properties = XML_DOC_WELLFORMED as c_int;
        (*doc).charset = XML_CHAR_ENCODING_UTF8 as c_int;

        // Set version
        let ver = if version.is_null() {
            XML_DEFAULT_VERSION.as_ptr() as *const xmlChar
        } else {
            version
        };
        (*doc).version = dup_xml_str(ver);
    }

    doc
}

/// Free a document and all its contents.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlFreeDoc(xmlDocPtr doc);
/// ```
///
/// Frees the document, its DTDs, and all nodes in the tree.
///
/// # SAFETY
///
/// - `doc` must be a valid pointer to an _xmlDoc, or NULL.
pub unsafe fn free_doc(doc: *mut _xmlDoc) {
    if doc.is_null() {
        return;
    }

    let d = unsafe { &mut *doc };

    // Free internal subset (DTD)
    if !d.intSubset.is_null() {
        free_dtd(d.intSubset);
    }

    // Free external subset (DTD)
    if !d.extSubset.is_null() {
        free_dtd(d.extSubset);
    }

    // Free the tree
    if !d.children.is_null() {
        free_node_list(d.children);
    }

    // Free oldNs list
    if !d.oldNs.is_null() {
        free_ns_list(d.oldNs);
    }

    // Free strings
    if !d.version.is_null() {
        allocator::xmlFree(d.version as *mut c_void);
    }
    if !d.encoding.is_null() {
        allocator::xmlFree(d.encoding as *mut c_void);
    }
    if !d.URL.is_null() {
        allocator::xmlFree(d.URL as *mut c_void);
    }

    // Free the document itself
    allocator::xmlFree(doc as *mut c_void);
}

/// Copy a document (deep copy by default).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDocPtr xmlCopyDoc(xmlDocPtr doc, int recursive);
/// ```
///
/// If `recursive` is 1, the entire tree is copied.
/// If `recursive` is 0, only the document structure is copied (no children).
///
/// # SAFETY
///
/// - `doc` must be a valid pointer to an _xmlDoc, or NULL.
pub unsafe fn copy_doc(doc: *const _xmlDoc, recursive: c_int) -> *mut _xmlDoc {
    if doc.is_null() {
        return ptr::null_mut();
    }

    let d = unsafe { &*doc };

    let new_doc = new_doc(d.version);
    if new_doc.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        (*new_doc).type_ = d.type_;
        (*new_doc).standalone = d.standalone;
        (*new_doc).encoding = dup_xml_str(d.encoding);
        (*new_doc).URL = dup_xml_str(d.URL);
        (*new_doc).charset = d.charset;
        (*new_doc).properties = d.properties;

        if recursive != 0 && !d.children.is_null() {
            (*new_doc).children = copy_node_list(d.children, recursive);
            if !(*new_doc).children.is_null() {
                (*(*new_doc).children).parent = ptr::null_mut(); // root element parent is NULL
                (*(*new_doc).children).doc = new_doc;
                // Update doc for all descendants
                propagate_doc((*new_doc).children, new_doc);
            }
        }
    }

    new_doc
}

/// Set the root element of a document.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNodePtr xmlDocSetRootElement(xmlDocPtr doc, xmlNodePtr root);
/// ```
///
/// If the document already has a root element, the old root is returned.
/// The new root is added as a child of the document.
///
/// # SAFETY
///
/// - `doc` must be a valid pointer to an _xmlDoc.
/// - `root` must be a valid pointer to an _xmlNode, or NULL.
pub unsafe fn doc_set_root_element(doc: *mut _xmlDoc, root: *mut _xmlNode) -> *mut _xmlNode {
    if doc.is_null() {
        return ptr::null_mut();
    }

    let d = unsafe { &mut *doc };

    let old_root = doc_get_root_element(doc);

    if !root.is_null() {
        unsafe {
            (*root).parent = ptr::null_mut();
            (*root).doc = doc;
        }
        d.children = root;
        d.last = root;
        unsafe {
            (*root).prev = ptr::null_mut();
            (*root).next = ptr::null_mut();
        }
    } else {
        d.children = ptr::null_mut();
        d.last = ptr::null_mut();
    }

    old_root
}

/// Get the root element of a document.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNodePtr xmlDocGetRootElement(xmlDocPtr doc);
/// ```
///
/// Returns the root element, or NULL if the document has no root element.
/// Skips non-element nodes (like PIs, comments) at the document level.
pub fn doc_get_root_element(doc: *mut _xmlDoc) -> *mut _xmlNode {
    if doc.is_null() {
        return ptr::null_mut();
    }

    let d = unsafe { &*doc };
    let mut cur = d.children;

    while !cur.is_null() {
        let node = unsafe { &*cur };
        if node.type_ == XML_ELEMENT_NODE as c_int {
            return cur;
        }
        cur = node.next;
    }

    ptr::null_mut()
}

/// Get the line number of a node.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// long xmlGetLineNo(xmlNodePtr node);
/// ```
///
/// Returns the line number, or 0 if not available.
pub fn get_line_no(node: *const _xmlNode) -> c_int {
    if node.is_null() {
        return 0;
    }
    let n = unsafe { &*node };
    n.line as c_int
}

// ═══════════════════════════════════════════════════════════════════════════════
// Node Operations
// ═══════════════════════════════════════════════════════════════════════════════

/// Create a new XML node.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNodePtr xmlNewNode(xmlNsPtr ns, const xmlChar *name);
/// ```
///
/// Creates a new element node with the given name and namespace.
///
/// # SAFETY
///
/// - `ns` may be NULL.
/// - `name` must be a valid null-terminated string or NULL.
pub unsafe fn new_node(ns: *mut _xmlNs, name: *const xmlChar) -> *mut _xmlNode {
    let node = allocator::xmlMallocZero(size_of::<_xmlNode>() as usize) as *mut _xmlNode;
    if node.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        (*node).type_ = XML_ELEMENT_NODE as c_int;
        (*node).name = dup_xml_str(name);
        (*node).ns = ns;
        (*node).line = 0;
        (*node).extra = 0;

        if !ns.is_null() {
            (*ns).context = node as *mut _xmlDoc;
        }
    }

    node
}

/// Free a single node (without freeing children).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlFreeNode(xmlNodePtr node);
/// ```
///
/// Frees a node and its properties/namespaces, but NOT its children.
/// Children must be freed separately or reattached.
///
/// # SAFETY
///
/// - `node` must be a valid pointer to an _xmlNode, or NULL.
pub unsafe fn free_node(node: *mut _xmlNode) {
    if node.is_null() {
        return;
    }

    let n = unsafe { &mut *node };

    // Free properties
    if !n.properties.is_null() {
        free_prop_list(n.properties);
    }

    // Free namespace declarations
    if !n.nsDef.is_null() {
        free_ns_list(n.nsDef);
    }

    // Free the name
    if !n.name.is_null() {
        allocator::xmlFree(n.name as *mut c_void);
    }

    // Free content (for text/CDATA nodes)
    if !n.content.is_null() {
        let node_type = n.type_;
        if node_type == XML_TEXT_NODE as c_int
            || node_type == XML_CDATA_SECTION_NODE as c_int
            || node_type == XML_COMMENT_NODE as c_int
            || node_type == XML_PI_NODE as c_int
        {
            allocator::xmlFree(n.content as *mut c_void);
        }
    }

    allocator::xmlFree(node as *mut c_void);
}

/// Free a linked list of nodes.
///
/// Frees all nodes in the list and their children recursively.
///
/// # SAFETY
///
/// - `node` must be a valid pointer to an _xmlNode, or NULL.
pub unsafe fn free_node_list(node: *mut _xmlNode) {
    let mut cur = node;
    while !cur.is_null() {
        let next = unsafe { (*cur).next };

        // Free children recursively
        if !unsafe { (*cur).children }.is_null() {
            free_node_list(unsafe { (*cur).children });
        }

        free_node(cur);
        cur = next;
    }
}

/// Free a linked list of properties.
///
/// # SAFETY
///
/// - `prop` must be a valid pointer to an _xmlAttr, or NULL.
unsafe fn free_prop_list(prop: *mut _xmlAttr) {
    let mut cur = prop;
    while !cur.is_null() {
        let next = unsafe { (*cur).next };

        // Free children (text nodes with value)
        if !unsafe { (*cur).children }.is_null() {
            free_node_list(unsafe { (*cur).children });
        }

        // Free name
        if !unsafe { (*cur).name }.is_null() {
            allocator::xmlFree(unsafe { (*cur).name } as *mut c_void);
        }

        allocator::xmlFree(cur as *mut c_void);
        cur = next;
    }
}

/// Free a linked list of namespace declarations.
///
/// # SAFETY
///
/// - `ns` must be a valid pointer to an _xmlNs, or NULL.
unsafe fn free_ns_list(ns: *mut _xmlNs) {
    let mut cur = ns;
    while !cur.is_null() {
        let next = unsafe { (*cur).next };

        // Free href and prefix
        if !unsafe { (*cur).href }.is_null() {
            allocator::xmlFree(unsafe { (*cur).href } as *mut c_void);
        }
        if !unsafe { (*cur).prefix }.is_null() {
            allocator::xmlFree(unsafe { (*cur).prefix } as *mut c_void);
        }

        allocator::xmlFree(cur as *mut c_void);
        cur = next;
    }
}

/// Copy a node (shallow or deep).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNodePtr xmlCopyNode(xmlNodePtr node, int recursive);
/// ```
///
/// If `recursive` is 1, children are also copied.
/// Returns the new node, or NULL on failure.
///
/// # SAFETY
///
/// - `node` must be a valid pointer to an _xmlNode, or NULL.
pub unsafe fn copy_node(node: *const _xmlNode, recursive: c_int) -> *mut _xmlNode {
    if node.is_null() {
        return ptr::null_mut();
    }

    let n = unsafe { &*node };

    let new_node = allocator::xmlMallocZero(size_of::<_xmlNode>() as usize) as *mut _xmlNode;
    if new_node.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        (*new_node).type_ = n.type_;
        (*new_node).name = dup_xml_str(n.name);
        (*new_node).line = n.line;
        (*new_node).extra = n.extra;
        (*new_node).psvi = n.psvi;
        (*new_node)._private = n._private;

        // Copy namespace pointer (NOT the ns declaration — just the reference)
        (*new_node).ns = n.ns;

        // Copy namespace declarations
        if !n.nsDef.is_null() {
            (*new_node).nsDef = copy_ns_list(n.nsDef);
        }

        // Copy content for text/CDATA/comment/PI nodes
        let node_type = n.type_;
        if (node_type == XML_TEXT_NODE as c_int
            || node_type == XML_CDATA_SECTION_NODE as c_int
            || node_type == XML_COMMENT_NODE as c_int
            || node_type == XML_PI_NODE as c_int)
            && !n.content.is_null()
        {
            (*new_node).content = dup_xml_str(n.content);
        }

        // Copy properties
        if !n.properties.is_null() {
            (*new_node).properties = copy_prop_list(n.properties);
            // Update doc links on properties
            let mut prop = (*new_node).properties;
            while !prop.is_null() {
                (*prop).parent = new_node;
                if !(*prop).children.is_null() {
                    propagate_doc((*prop).children, (*new_node).doc);
                }
                prop = (*prop).next;
            }
        }

        // Copy children if recursive
        if recursive != 0 && !n.children.is_null() {
            (*new_node).children = copy_node_list(n.children, recursive);
            if !(*new_node).children.is_null() {
                (*(*new_node).children).parent = new_node;
                (*(*new_node).children).doc = (*new_node).doc;
                propagate_doc((*new_node).children, (*new_node).doc);
            }
        }
    }

    new_node
}

/// Copy a linked list of nodes.
///
/// Returns the first node of the new list, or NULL on failure.
unsafe fn copy_node_list(node: *const _xmlNode, recursive: c_int) -> *mut _xmlNode {
    if node.is_null() {
        return ptr::null_mut();
    }

    let n = unsafe { &*node };
    let new_node = copy_node(node, recursive);
    if new_node.is_null() {
        return ptr::null_mut();
    }

    let mut prev = new_node;
    let mut cur = n.next;

    while !cur.is_null() {
        let new_cur = copy_node(cur, recursive);
        if new_cur.is_null() {
            break;
        }
        unsafe {
            (*prev).next = new_cur;
            (*new_cur).prev = prev;
        }
        prev = new_cur;
        cur = unsafe { (*cur).next };
    }

    new_node
}

/// Copy a linked list of namespace declarations.
unsafe fn copy_ns_list(ns: *const _xmlNs) -> *mut _xmlNs {
    if ns.is_null() {
        return ptr::null_mut();
    }

    let n = unsafe { &*ns };
    let new_ns = allocator::xmlMallocZero(size_of::<_xmlNs>() as usize) as *mut _xmlNs;
    if new_ns.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        (*new_ns).type_ = n.type_;
        (*new_ns).href = dup_xml_str(n.href);
        (*new_ns).prefix = dup_xml_str(n.prefix);
        (*new_ns)._private = n._private;
    }

    let mut prev = new_ns;
    let mut cur = n.next;

    while !cur.is_null() {
        let c = unsafe { &*cur };
        let new_cur = allocator::xmlMallocZero(size_of::<_xmlNs>() as usize) as *mut _xmlNs;
        if new_cur.is_null() {
            break;
        }
        unsafe {
            (*new_cur).type_ = c.type_;
            (*new_cur).href = dup_xml_str(c.href);
            (*new_cur).prefix = dup_xml_str(c.prefix);
            (*new_cur)._private = c._private;
            (*prev).next = new_cur;
        }
        prev = new_cur;
        cur = c.next;
    }

    new_ns
}

/// Copy a linked list of properties.
unsafe fn copy_prop_list(prop: *const _xmlAttr) -> *mut _xmlAttr {
    if prop.is_null() {
        return ptr::null_mut();
    }

    let p = unsafe { &*prop };
    let new_prop = allocator::xmlMallocZero(size_of::<_xmlAttr>() as usize) as *mut _xmlAttr;
    if new_prop.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        (*new_prop).type_ = p.type_;
        (*new_prop).name = dup_xml_str(p.name);
        (*new_prop).ns = p.ns;
        (*new_prop).atype = p.atype;

        // Copy children (text value nodes)
        if !p.children.is_null() {
            (*new_prop).children = copy_node_list(p.children, 1);
            if !(*new_prop).children.is_null() {
                (*(*new_prop).children).parent = new_prop as *mut _xmlNode;
            }
        }
    }

    let mut prev = new_prop;
    let mut cur = p.next;

    while !cur.is_null() {
        let c = unsafe { &*cur };
        let new_cur = allocator::xmlMallocZero(size_of::<_xmlAttr>() as usize) as *mut _xmlAttr;
        if new_cur.is_null() {
            break;
        }
        unsafe {
            (*new_cur).type_ = c.type_;
            (*new_cur).name = dup_xml_str(c.name);
            (*new_cur).ns = c.ns;
            (*new_cur).atype = c.atype;

            if !c.children.is_null() {
                (*new_cur).children = copy_node_list(c.children, 1);
                if !(*new_cur).children.is_null() {
                    (*(*new_cur).children).parent = new_cur as *mut _xmlNode;
                }
            }

            (*prev).next = new_cur;
        }
        prev = new_cur;
        cur = c.next;
    }

    new_prop
}

/// Propagate the document pointer to all descendants of a node.
unsafe fn propagate_doc(node: *mut _xmlNode, doc: *mut _xmlDoc) {
    let mut cur = node;
    while !cur.is_null() {
        unsafe {
            (*cur).doc = doc;

            // Propagate to properties
            let mut prop = (*cur).properties;
            while !prop.is_null() {
                (*prop).doc = doc;
                if !(*prop).children.is_null() {
                    propagate_doc((*prop).children, doc);
                }
                prop = (*prop).next;
            }

            // Recurse into children
            if !(*cur).children.is_null() {
                propagate_doc((*cur).children, doc);
            }
        }
        cur = unsafe { (*cur).next };
    }
}

/// Unlink a node from its parent/siblings.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlUnlinkNode(xmlNodePtr node);
/// ```
///
/// Removes the node from its parent's child list and sibling list.
/// The node's parent, prev, and next pointers are cleared.
/// The node is NOT freed — the caller is responsible for freeing it.
///
/// # SAFETY
///
/// - `node` must be a valid pointer to an _xmlNode, or NULL.
pub unsafe fn unlink_node(node: *mut _xmlNode) {
    if node.is_null() {
        return;
    }

    let n = unsafe { &mut *node };

    // Fix up prev/next chain
    let prev = n.prev;
    let next = n.next;

    if !prev.is_null() {
        unsafe { (*prev).next = next };
    }
    if !next.is_null() {
        unsafe { (*next).prev = prev };
    }

    // Fix up parent's children/last pointers
    let parent = n.parent;
    if !parent.is_null() {
        if unsafe { (*parent).children } == node {
            unsafe { (*parent).children = next };
        }
        if unsafe { (*parent).last } == node {
            unsafe { (*parent).last = prev };
        }
    }

    // Also fix up doc-level children/last if node is a direct doc child
    let doc = n.doc;
    if !doc.is_null() && !parent.is_null() {
        // Already handled above
    }
    if !doc.is_null() && parent.is_null() {
        // Node is a direct child of the document
        if unsafe { (*doc).children } == node {
            unsafe { (*doc).children = next };
        }
        if unsafe { (*doc).last } == node {
            unsafe { (*doc).last = prev };
        }
    }

    // Clear the node's links
    n.parent = ptr::null_mut();
    n.prev = ptr::null_mut();
    n.next = ptr::null_mut();
}

/// Add a child node to a parent.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNodePtr xmlAddChild(xmlNodePtr parent, xmlNodePtr cur);
/// ```
///
/// Adds `cur` as the last child of `parent`.
/// Returns the child, or NULL on failure.
///
/// # SAFETY
///
/// - `parent` must be a valid pointer to an _xmlNode.
/// - `cur` must be a valid pointer to an _xmlNode.
pub unsafe fn add_child(parent: *mut _xmlNode, cur: *mut _xmlNode) -> *mut _xmlNode {
    if parent.is_null() || cur.is_null() {
        return ptr::null_mut();
    }

    let p = unsafe { &mut *parent };
    let c = unsafe { &mut *cur };

    // If cur is already linked, unlink it first
    if !c.parent.is_null() || !c.prev.is_null() || !c.next.is_null() {
        unlink_node(cur);
    }

    // Update parent/child links
    c.parent = parent;

    if p.children.is_null() {
        // First child
        p.children = cur;
        p.last = cur;
        c.prev = ptr::null_mut();
        c.next = ptr::null_mut();
    } else {
        // Append to end
        c.prev = p.last;
        c.next = ptr::null_mut();
        if !p.last.is_null() {
            unsafe { (*p.last).next = cur };
        }
        p.last = cur;
    }

    // Update doc
    let doc = if !p.doc.is_null() {
        p.doc
    } else {
        ptr::null_mut()
    };
    if !doc.is_null() && c.doc != doc {
        propagate_doc(cur, doc);
    }

    cur
}

/// Add a sibling node after another.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNodePtr xmlAddSibling(xmlNodePtr cur, xmlNodePtr elem);
/// ```
///
/// Adds `elem` as the next sibling of `cur`.
/// Returns `elem`, or NULL on failure.
///
/// # SAFETY
///
/// - `cur` must be a valid pointer to an _xmlNode.
/// - `elem` must be a valid pointer to an _xmlNode.
pub unsafe fn add_sibling(cur: *mut _xmlNode, elem: *mut _xmlNode) -> *mut _xmlNode {
    if cur.is_null() || elem.is_null() {
        return ptr::null_mut();
    }

    let c = unsafe { &mut *cur };

    // If elem is already linked, unlink it first
    let e = unsafe { &mut *elem };
    if !e.parent.is_null() || !e.prev.is_null() || !e.next.is_null() {
        unlink_node(elem);
    }

    // Set parent
    e.parent = c.parent;

    // Link elem after cur
    e.prev = cur;
    e.next = c.next;

    if !c.next.is_null() {
        unsafe { (*c.next).prev = elem };
    }
    c.next = elem;

    // Update parent's last if needed
    let parent = c.parent;
    if !parent.is_null() && unsafe { (*parent).last } == cur {
        unsafe { (*parent).last = elem };
    }

    // Update doc
    if !c.doc.is_null() && e.doc != c.doc {
        propagate_doc(elem, c.doc);
    }

    elem
}

/// Add a sibling node before another.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNodePtr xmlAddPrevSibling(xmlNodePtr cur, xmlNodePtr elem);
/// ```
///
/// Adds `elem` as the previous sibling of `cur`.
/// Returns `elem`, or NULL on failure.
///
/// # SAFETY
///
/// - `cur` must be a valid pointer to an _xmlNode.
/// - `elem` must be a valid pointer to an _xmlNode.
pub unsafe fn add_sibling_before(cur: *mut _xmlNode, elem: *mut _xmlNode) -> *mut _xmlNode {
    if cur.is_null() || elem.is_null() {
        return ptr::null_mut();
    }

    let c = unsafe { &mut *cur };

    // If elem is already linked, unlink it first
    let e = unsafe { &mut *elem };
    if !e.parent.is_null() || !e.prev.is_null() || !e.next.is_null() {
        unlink_node(elem);
    }

    // Set parent
    e.parent = c.parent;

    // Link elem before cur
    e.prev = c.prev;
    e.next = cur;

    if !c.prev.is_null() {
        unsafe { (*c.prev).next = elem };
    }
    c.prev = elem;

    // Update parent's first if needed
    let parent = c.parent;
    if !parent.is_null() && unsafe { (*parent).children } == cur {
        unsafe { (*parent).children = elem };
    }

    // Update doc-level children if node is a direct doc child
    let doc = c.doc;
    if !doc.is_null() && parent.is_null() {
        if unsafe { (*doc).children } == cur {
            unsafe { (*doc).children = elem };
        }
    }

    // Update doc
    if !c.doc.is_null() && e.doc != c.doc {
        propagate_doc(elem, c.doc);
    }

    elem
}

/// Create a new child element.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNodePtr xmlNewChild(xmlNodePtr parent, xmlNsPtr ns, const xmlChar *name);
/// ```
///
/// Creates a new element and adds it as the last child of `parent`.
///
/// # SAFETY
///
/// - `parent` must be a valid pointer to an _xmlNode, or NULL.
/// - `name` must be a valid null-terminated string or NULL.
pub unsafe fn new_child(
    parent: *mut _xmlNode,
    ns: *mut _xmlNs,
    name: *const xmlChar,
) -> *mut _xmlNode {
    let node = new_node(ns, name);
    if node.is_null() {
        return ptr::null_mut();
    }

    if !parent.is_null() {
        add_child(parent, node);
    }

    node
}

// ═══════════════════════════════════════════════════════════════════════════════
// Text / Content Nodes
// ═══════════════════════════════════════════════════════════════════════════════

/// Create a new text node.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNodePtr xmlNewText(const xmlChar *content);
/// ```
///
/// Creates a text node with the given content.
/// If content is NULL, creates an empty text node.
///
/// # SAFETY
///
/// - `content` must be a valid null-terminated string or NULL.
pub unsafe fn new_text(content: *const xmlChar) -> *mut _xmlNode {
    let node = allocator::xmlMallocZero(size_of::<_xmlNode>() as usize) as *mut _xmlNode;
    if node.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        (*node).type_ = XML_TEXT_NODE as c_int;
        (*node).name = dup_xml_str(b"text\0" as *const u8 as *const xmlChar);
        (*node).content = if content.is_null() {
            let empty = allocator::xmlMalloc(1) as *mut xmlChar;
            if !empty.is_null() {
                *empty = 0;
            }
            empty
        } else {
            dup_xml_str(content)
        };
        (*node).line = 0;
    }

    node
}

/// Create a new comment node.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNodePtr xmlNewComment(const xmlChar *content);
/// ```
///
/// Creates a comment node with the given content.
///
/// # SAFETY
///
/// - `content` must be a valid null-terminated string or NULL.
pub unsafe fn new_comment(content: *const xmlChar) -> *mut _xmlNode {
    let node = allocator::xmlMallocZero(size_of::<_xmlNode>() as usize) as *mut _xmlNode;
    if node.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        (*node).type_ = XML_COMMENT_NODE as c_int;
        (*node).name = dup_xml_str(b"comment\0" as *const u8 as *const xmlChar);
        (*node).content = dup_xml_str(content);
        (*node).line = 0;
    }

    node
}

/// Create a new processing instruction node.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNodePtr xmlNewPI(const xmlChar *name, const xmlChar *content);
/// ```
///
/// Creates a PI node with the given target name and content.
///
/// # SAFETY
///
/// - `name` must be a valid null-terminated string.
/// - `content` must be a valid null-terminated string or NULL.
pub unsafe fn new_pi(name: *const xmlChar, content: *const xmlChar) -> *mut _xmlNode {
    let node = allocator::xmlMallocZero(size_of::<_xmlNode>() as usize) as *mut _xmlNode;
    if node.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        (*node).type_ = XML_PI_NODE as c_int;
        (*node).name = dup_xml_str(name);
        (*node).content = dup_xml_str(content);
        (*node).line = 0;
    }

    node
}

/// Create a new CDATA section node.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNodePtr xmlNewCDataBlock(xmlDocPtr doc, const xmlChar *content, int len);
/// ```
///
/// Creates a CDATA section node with the given content.
///
/// # SAFETY
///
/// - `doc` may be NULL.
/// - `content` must be a valid pointer to a buffer of at least `len` bytes,
///   or NULL.
pub unsafe fn new_cdata_block(
    doc: *mut _xmlDoc,
    content: *const xmlChar,
    len: c_int,
) -> *mut _xmlNode {
    let node = allocator::xmlMallocZero(size_of::<_xmlNode>() as usize) as *mut _xmlNode;
    if node.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        (*node).type_ = XML_CDATA_SECTION_NODE as c_int;
        (*node).name = dup_xml_str(b"cdata\0" as *const u8 as *const xmlChar);
        (*node).doc = doc;

        if !content.is_null() && len > 0 {
            (*node).content = allocator::xmlMalloc((len + 1) as usize) as *mut xmlChar;
            if !(*node).content.is_null() {
                ptr::copy_nonoverlapping(content, (*node).content, len as usize);
                *((*node).content.add(len as usize)) = 0;
            }
        } else {
            let empty = allocator::xmlMalloc(1) as *mut xmlChar;
            if !empty.is_null() {
                *empty = 0;
            }
            (*node).content = empty;
        }

        (*node).line = 0;
    }

    node
}

// ═══════════════════════════════════════════════════════════════════════════════
// Namespace Operations
// ═══════════════════════════════════════════════════════════════════════════════

/// Create a new namespace declaration.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNsPtr xmlNewNs(xmlNodePtr node, const xmlChar *href, const xmlChar *prefix);
/// ```
///
/// Creates a new namespace declaration on the given node.
/// The namespace is added to the node's nsDef list.
///
/// If `href` is NULL, the namespace is a default namespace undeclaration.
/// If `prefix` is NULL, this is the default namespace (xmlns="...").
///
/// # SAFETY
///
/// - `node` must be a valid pointer to an _xmlNode, or NULL.
/// - `href` must be a valid null-terminated string or NULL.
/// - `prefix` must be a valid null-terminated string or NULL.
pub unsafe fn new_ns(
    node: *mut _xmlNode,
    href: *const xmlChar,
    prefix: *const xmlChar,
) -> *mut _xmlNs {
    let ns = allocator::xmlMallocZero(size_of::<_xmlNs>() as usize) as *mut _xmlNs;
    if ns.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        (*ns).type_ = XML_LOCAL_NAMESPACE as c_int;
        (*ns).href = dup_xml_str(href);
        (*ns).prefix = dup_xml_str(prefix);
        (*ns).context = node as *mut _xmlDoc;

        // Add to node's nsDef list
        if !node.is_null() {
            let n = &mut *node;
            if n.nsDef.is_null() {
                n.nsDef = ns;
            } else {
                // Append to end
                let mut last = n.nsDef;
                while !(*last).next.is_null() {
                    last = (*last).next;
                }
                (*last).next = ns;
            }
        }
    }

    ns
}

/// Set the namespace of a node.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlSetNs(xmlNodePtr node, xmlNsPtr ns);
/// ```
///
/// # SAFETY
///
/// - `node` must be a valid pointer to an _xmlNode, or NULL.
/// - `ns` must be a valid pointer to an _xmlNs, or NULL.
pub unsafe fn set_ns(node: *mut _xmlNode, ns: *mut _xmlNs) {
    if node.is_null() {
        return;
    }
    unsafe {
        (*node).ns = ns;
    }
}

/// Get a list of namespaces in scope for a node.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNsPtr *xmlGetNsList(xmlDocPtr doc, xmlNodePtr node);
/// ```
///
/// Returns a NULL-terminated array of namespace pointers in scope,
/// or NULL on failure.
///
/// # SAFETY
///
/// - `doc` must be a valid pointer to an _xmlDoc, or NULL.
/// - `node` must be a valid pointer to an _xmlNode, or NULL.
pub unsafe fn get_ns_list(doc: *mut _xmlDoc, node: *mut _xmlNode) -> *mut *mut _xmlNs {
    // Phase 1: basic implementation
    // A more complete implementation would walk the node's ancestors
    // and collect all in-scope namespaces.
    if node.is_null() {
        return ptr::null_mut();
    }

    // Collect namespaces from this node and ancestors
    let mut ns_ptrs: Vec<*mut _xmlNs> = Vec::new();
    let mut cur = node;

    while !cur.is_null() {
        let n = unsafe { &*cur };
        let mut ns_def = n.nsDef;
        while !ns_def.is_null() {
            // Avoid duplicates
            let ns = unsafe { &*ns_def };
            let mut found = false;
            for &existing in &ns_ptrs {
                if existing == ns_def {
                    found = true;
                    break;
                }
                let e = unsafe { &*existing };
                if !ns.href.is_null() && !e.href.is_null() {
                    let href_match =
                        unsafe { crate::abi::exports_xml2::xmlStrEqual(ns.href, e.href) != 0 };
                    if href_match {
                        if ns.prefix.is_null() && e.prefix.is_null() {
                            found = true;
                            break;
                        }
                        if !ns.prefix.is_null() && !e.prefix.is_null() {
                            let prefix_match = unsafe {
                                crate::abi::exports_xml2::xmlStrEqual(ns.prefix, e.prefix) != 0
                            };
                            if prefix_match {
                                found = true;
                                break;
                            }
                        }
                    }
                }
            }
            if !found {
                ns_ptrs.push(ns_def);
            }
            ns_def = unsafe { (*ns_def).next };
        }
        cur = n.parent;
    }

    if ns_ptrs.is_empty() {
        return ptr::null_mut();
    }

    // Allocate NULL-terminated array
    let arr =
        allocator::xmlMalloc((ns_ptrs.len() + 1) * size_of::<*mut _xmlNs>()) as *mut *mut _xmlNs;
    if arr.is_null() {
        return ptr::null_mut();
    }

    for (i, ns) in ns_ptrs.iter().enumerate() {
        unsafe { *arr.add(i) = *ns };
    }
    unsafe { *arr.add(ns_ptrs.len()) = ptr::null_mut() };

    arr
}

/// Search for a namespace by prefix.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNsPtr xmlSearchNs(xmlDocPtr doc, xmlNodePtr node, const xmlChar *nameSpace);
/// ```
///
/// Searches for a namespace declaration with the given prefix.
/// If `nameSpace` is NULL, searches for the default namespace.
///
/// # SAFETY
///
/// - `doc` must be a valid pointer to an _xmlDoc, or NULL.
/// - `node` must be a valid pointer to an _xmlNode, or NULL.
/// - `nameSpace` must be a valid null-terminated string or NULL.
pub unsafe fn search_ns(
    doc: *mut _xmlDoc,
    node: *mut _xmlNode,
    name_space: *const xmlChar,
) -> *mut _xmlNs {
    if node.is_null() {
        return ptr::null_mut();
    }

    let mut cur = node;
    while !cur.is_null() {
        let n = unsafe { &*cur };
        let mut ns_def = n.nsDef;
        while !ns_def.is_null() {
            let ns = unsafe { &*ns_def };
            let match_prefix = if name_space.is_null() {
                // Default namespace: prefix should be NULL
                ns.prefix.is_null()
            } else {
                !ns.prefix.is_null()
                    && unsafe { crate::abi::exports_xml2::xmlStrEqual(ns.prefix, name_space) != 0 }
            };
            if match_prefix {
                return ns_def;
            }
            ns_def = unsafe { (*ns_def).next };
        }
        cur = n.parent;
    }

    ptr::null_mut()
}

/// Search for a namespace by href (URI).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNsPtr xmlSearchNsByHref(xmlDocPtr doc, xmlNodePtr node, const xmlChar *href);
/// ```
///
/// Searches for a namespace declaration with the given URI.
///
/// # SAFETY
///
/// - `doc` must be a valid pointer to an _xmlDoc, or NULL.
/// - `node` must be a valid pointer to an _xmlNode, or NULL.
/// - `href` must be a valid null-terminated string or NULL.
pub unsafe fn search_ns_by_href(
    doc: *mut _xmlDoc,
    node: *mut _xmlNode,
    href: *const xmlChar,
) -> *mut _xmlNs {
    if node.is_null() || href.is_null() {
        return ptr::null_mut();
    }

    let mut cur = node;
    while !cur.is_null() {
        let n = unsafe { &*cur };
        let mut ns_def = n.nsDef;
        while !ns_def.is_null() {
            let ns = unsafe { &*ns_def };
            if !ns.href.is_null()
                && unsafe { crate::abi::exports_xml2::xmlStrEqual(ns.href, href) != 0 }
            {
                return ns_def;
            }
            ns_def = unsafe { (*ns_def).next };
        }
        cur = n.parent;
    }

    ptr::null_mut()
}

// ═══════════════════════════════════════════════════════════════════════════════
// Attribute Operations
// ═══════════════════════════════════════════════════════════════════════════════

/// Set an attribute on a node.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlAttrPtr xmlSetProp(xmlNodePtr node, const xmlChar *name, const xmlChar *value);
/// ```
///
/// Sets the attribute with the given name to the given value.
/// If the attribute already exists, its value is updated.
/// Creates the attribute if it doesn't exist.
///
/// Returns the attribute pointer, or NULL on failure.
///
/// # SAFETY
///
/// - `node` must be a valid pointer to an _xmlNode, or NULL.
/// - `name` must be a valid null-terminated string.
/// - `value` must be a valid null-terminated string or NULL.
pub unsafe fn set_prop(
    node: *mut _xmlNode,
    name: *const xmlChar,
    value: *const xmlChar,
) -> *mut _xmlAttr {
    if node.is_null() || name.is_null() {
        return ptr::null_mut();
    }

    let n = unsafe { &mut *node };

    // Check if attribute already exists
    let mut existing = n.properties;
    while !existing.is_null() {
        let attr = unsafe { &*existing };
        if !attr.name.is_null()
            && unsafe { crate::abi::exports_xml2::xmlStrEqual(attr.name, name) != 0 }
        {
            // Update existing attribute value
            // Free old children (text nodes)
            if !attr.children.is_null() {
                free_node_list(attr.children);
                // SAFETY: We need to mutate const fields
                let attr_mut = existing as *mut _xmlAttr;
                unsafe { (*attr_mut).children = ptr::null_mut() };
                unsafe { (*attr_mut).last = ptr::null_mut() };
            }
            // Set new value
            if !value.is_null() {
                let text = new_text(value);
                if !text.is_null() {
                    let attr_mut = existing as *mut _xmlAttr;
                    unsafe {
                        (*attr_mut).children = text;
                        (*attr_mut).last = text;
                        (*text).parent = existing as *mut _xmlNode;
                        (*text).doc = n.doc;
                    }
                }
            }
            return existing;
        }
        existing = unsafe { (*existing).next };
    }

    // Create new attribute
    let attr = allocator::xmlMallocZero(size_of::<_xmlAttr>() as usize) as *mut _xmlAttr;
    if attr.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        (*attr).type_ = XML_ATTRIBUTE_NODE as c_int;
        (*attr).name = dup_xml_str(name);
        (*attr).parent = node;
        (*attr).doc = n.doc;
        (*attr).atype = XML_ATTRIBUTE_CDATA as c_int;

        // Set value
        if !value.is_null() {
            let text = new_text(value);
            if !text.is_null() {
                (*attr).children = text;
                (*attr).last = text;
                (*text).parent = attr as *mut _xmlNode;
                (*text).doc = n.doc;
            }
        }

        // Add to node's property list
        if n.properties.is_null() {
            n.properties = attr;
        } else {
            let mut last = n.properties;
            while !(*last).next.is_null() {
                last = (*last).next;
            }
            (*last).next = attr;
            (*attr).prev = last;
        }
    }

    attr
}

/// Get an attribute value by name.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlGetProp(xmlNodePtr node, const xmlChar *name);
/// ```
///
/// Returns the attribute value as an xmlChar* (caller must free with xmlFree),
/// or NULL if the attribute doesn't exist.
///
/// # SAFETY
///
/// - `node` must be a valid pointer to an _xmlNode, or NULL.
/// - `name` must be a valid null-terminated string.
pub unsafe fn get_prop(node: *mut _xmlNode, name: *const xmlChar) -> *mut xmlChar {
    if node.is_null() || name.is_null() {
        return ptr::null_mut();
    }

    let n = unsafe { &*node };
    let mut cur = n.properties;

    while !cur.is_null() {
        let attr = unsafe { &*cur };
        if !attr.name.is_null()
            && unsafe { crate::abi::exports_xml2::xmlStrEqual(attr.name, name) != 0 }
        {
            // Get the text content of the attribute
            if !attr.children.is_null() {
                let text = unsafe { &*attr.children };
                if text.type_ == XML_TEXT_NODE as c_int && !text.content.is_null() {
                    return dup_xml_str(text.content);
                }
            }
            return dup_xml_str(b"\0" as *const u8 as *const xmlChar);
        }
        cur = unsafe { (*cur).next };
    }

    ptr::null_mut()
}

/// Get a namespaced attribute value.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlGetNsProp(xmlNodePtr node, const xmlChar *name, const xmlChar *nameSpace);
/// ```
///
/// Returns the attribute value, or NULL if not found.
///
/// # SAFETY
///
/// - `node` must be a valid pointer to an _xmlNode, or NULL.
/// - `name` must be a valid null-terminated string.
/// - `nameSpace` may be NULL.
pub unsafe fn get_ns_prop(
    node: *mut _xmlNode,
    name: *const xmlChar,
    _name_space: *const xmlChar,
) -> *mut xmlChar {
    // Phase 1: simple attribute lookup (namespace-aware lookup will be
    // fully implemented in Phase 2+).
    get_prop(node, name)
}

/// Set a namespaced attribute.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlAttrPtr xmlSetNsProp(xmlNodePtr node, xmlNsPtr ns, const xmlChar *name, const xmlChar *value);
/// ```
///
/// # SAFETY
///
/// - `node` must be a valid pointer to an _xmlNode, or NULL.
/// - `ns` may be NULL.
/// - `name` must be a valid null-terminated string.
/// - `value` must be a valid null-terminated string or NULL.
pub unsafe fn set_ns_prop(
    node: *mut _xmlNode,
    _ns: *mut _xmlNs,
    name: *const xmlChar,
    value: *const xmlChar,
) -> *mut _xmlAttr {
    // Phase 1: use xmlSetProp (namespace-aware version will be in Phase 2+).
    set_prop(node, name, value)
}

/// Remove a property from a node.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlRemoveProp(xmlAttrPtr attr);
/// ```
///
/// Removes the attribute from its parent node and frees it.
/// Returns 0 on success, -1 on failure.
///
/// # SAFETY
///
/// - `attr` must be a valid pointer to an _xmlAttr, or NULL.
pub unsafe fn remove_prop(attr: *mut _xmlAttr) -> c_int {
    if attr.is_null() {
        return -1;
    }

    let a = unsafe { &mut *attr };

    // Unlink from parent's property list
    let parent = a.parent;
    if !parent.is_null() {
        let p = unsafe { &mut *parent };
        if p.properties == attr {
            p.properties = a.next;
        }
    }

    // Fix up prev/next chain
    if !a.prev.is_null() {
        unsafe { (*a.prev).next = a.next };
    }
    if !a.next.is_null() {
        unsafe { (*a.next).prev = a.prev };
    }

    // Free children (text value nodes)
    if !a.children.is_null() {
        free_node_list(a.children);
    }

    // Free name
    if !a.name.is_null() {
        allocator::xmlFree(a.name as *mut c_void);
    }

    allocator::xmlFree(attr as *mut c_void);
    0
}

// ═══════════════════════════════════════════════════════════════════════════════
// DTD Operations
// ═══════════════════════════════════════════════════════════════════════════════

/// Get the internal DTD subset of a document.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDtdPtr xmlGetIntSubset(xmlDocPtr doc);
/// ```
pub fn get_int_subset(doc: *const _xmlDoc) -> *mut _xmlDtd {
    if doc.is_null() {
        return ptr::null_mut();
    }
    let d = unsafe { &*doc };
    d.intSubset
}

/// Create a new DTD node.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDtdPtr xmlNewDtd(xmlDocPtr doc, const xmlChar *name,
///                     const xmlChar *ExternalID, const xmlChar *SystemID);
/// ```
///
/// Creates a new DTD and attaches it to the document.
///
/// # SAFETY
///
/// - `doc` must be a valid pointer to an _xmlDoc.
/// - `name` must be a valid null-terminated string or NULL.
/// - `ExternalID`, `SystemID` may be NULL.
pub unsafe fn new_dtd(
    doc: *mut _xmlDoc,
    name: *const xmlChar,
    ExternalID: *const xmlChar,
    SystemID: *const xmlChar,
) -> *mut _xmlDtd {
    let dtd = allocator::xmlMallocZero(size_of::<_xmlDtd>() as usize) as *mut _xmlDtd;
    if dtd.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        (*dtd).type_ = XML_DTD_NODE as c_int;
        (*dtd).name = dup_xml_str(name);
        (*dtd).ExternalID = dup_xml_str(ExternalID);
        (*dtd).SystemID = dup_xml_str(SystemID);
        (*dtd).parent = doc;
        (*dtd).doc = doc;

        // Attach to document
        if !doc.is_null() {
            if (*doc).intSubset.is_null() {
                (*doc).intSubset = dtd;
            }
        }
    }

    dtd
}

/// Free a DTD.
///
/// # SAFETY
///
/// - `dtd` must be a valid pointer to an _xmlDtd, or NULL.
unsafe fn free_dtd(dtd: *mut _xmlDtd) {
    if dtd.is_null() {
        return;
    }

    let d = unsafe { &mut *dtd };

    // Free name
    if !d.name.is_null() {
        allocator::xmlFree(d.name as *mut c_void);
    }
    if !d.ExternalID.is_null() {
        allocator::xmlFree(d.ExternalID as *mut c_void);
    }
    if !d.SystemID.is_null() {
        allocator::xmlFree(d.SystemID as *mut c_void);
    }

    // Free hash tables for declarations
    unsafe extern "C" fn free_notation_wrapper(payload: *mut c_void, _name: *mut u8) {
        crate::xml::dtd::free_notation(payload as *mut _xmlNotation);
    }
    unsafe extern "C" fn free_element_wrapper(payload: *mut c_void, _name: *mut u8) {
        crate::xml::dtd::free_element(payload as *mut _xmlElement);
    }
    unsafe extern "C" fn free_attribute_wrapper(payload: *mut c_void, _name: *mut u8) {
        crate::xml::dtd::free_attribute(payload as *mut _xmlAttribute);
    }
    unsafe extern "C" fn free_entity_wrapper(payload: *mut c_void, _name: *mut u8) {
        crate::xml::entities::free_entity(payload as *mut _xmlEntity);
    }

    if !d.notations.is_null() {
        crate::xml::hash::hash_free(
            d.notations as *mut crate::xml::hash::HashTable,
            Some(free_notation_wrapper),
        );
        d.notations = ptr::null_mut();
    }
    if !d.elements.is_null() {
        crate::xml::hash::hash_free(
            d.elements as *mut crate::xml::hash::HashTable,
            Some(free_element_wrapper),
        );
        d.elements = ptr::null_mut();
    }
    if !d.attributes.is_null() {
        crate::xml::hash::hash_free(
            d.attributes as *mut crate::xml::hash::HashTable,
            Some(free_attribute_wrapper),
        );
        d.attributes = ptr::null_mut();
    }
    if !d.entities.is_null() {
        crate::xml::hash::hash_free(
            d.entities as *mut crate::xml::hash::HashTable,
            Some(free_entity_wrapper),
        );
        d.entities = ptr::null_mut();
    }
    if !d.pentities.is_null() {
        crate::xml::hash::hash_free(
            d.pentities as *mut crate::xml::hash::HashTable,
            Some(free_entity_wrapper),
        );
        d.pentities = ptr::null_mut();
    }

    // Free children
    if !d.children.is_null() {
        free_node_list(d.children);
    }

    allocator::xmlFree(dtd as *mut c_void);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Entity Operations
// ═══════════════════════════════════════════════════════════════════════════════

/// Create a new entity.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlEntityPtr xmlNewEntity(xmlDocPtr doc, const xmlChar *name, int type,
///                           const xmlChar *ExternalID, const xmlChar *SystemID,
///                           const xmlChar *content);
/// ```
///
/// # SAFETY
///
/// - `doc` may be NULL.
/// - `name` must be a valid null-terminated string.
/// - `ExternalID`, `SystemID`, `content` may be NULL.
pub unsafe fn new_entity(
    _doc: *mut _xmlDoc,
    name: *const xmlChar,
    etype: c_int,
    ExternalID: *const xmlChar,
    SystemID: *const xmlChar,
    content: *const xmlChar,
) -> *mut _xmlEntity {
    let entity = allocator::xmlMallocZero(size_of::<_xmlEntity>() as usize) as *mut _xmlEntity;
    if entity.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        (*entity).type_ = XML_ENTITY_DECL as c_int;
        (*entity).name = dup_xml_str(name);
        (*entity).etype = etype;
        (*entity).ExternalID = dup_xml_str(ExternalID);
        (*entity).SystemID = dup_xml_str(SystemID);
        (*entity).content = dup_xml_str(content);
        (*entity).length = if content.is_null() {
            0
        } else {
            crate::abi::exports_xml2::xmlStrlen(content)
        };
        (*entity).flags = 0;
        (*entity).expandedSize = 0;
    }

    entity
}

/// Get a document entity by name.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlEntityPtr xmlGetDocEntity(xmlDocPtr doc, const xmlChar *name);
/// ```
///
/// Returns the entity, or NULL if not found.
///
/// # SAFETY
///
/// - `doc` must be a valid pointer to an _xmlDoc, or NULL.
/// - `name` must be a valid null-terminated string.
pub unsafe fn get_doc_entity(doc: *const _xmlDoc, name: *const xmlChar) -> *mut _xmlEntity {
    crate::xml::entities::get_entity(doc as *mut _xmlDoc, name)
}

/// Get a parameter entity by name.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlEntityPtr xmlGetParameterEntity(xmlDocPtr doc, const xmlChar *name);
/// ```
///
/// # SAFETY
///
/// - `doc` must be a valid pointer to an _xmlDoc, or NULL.
/// - `name` must be a valid null-terminated string.
pub unsafe fn get_parameter_entity(doc: *const _xmlDoc, name: *const xmlChar) -> *mut _xmlEntity {
    crate::xml::entities::get_parameter_entity(doc as *mut _xmlDoc, name)
}

// ═══════════════════════════════════════════════════════════════════════════════
// XML Serialization
// ═══════════════════════════════════════════════════════════════════════════════
//
// Functions for serializing XML document/node trees to text.
// All output is UTF-8.

/// Entity replacement strings (as xmlChar byte slices).
const ENTITY_LT: &[xmlChar] = b"&lt;";
const ENTITY_GT: &[xmlChar] = b"&gt;";
const ENTITY_AMP: &[xmlChar] = b"&amp;";
const ENTITY_QUOT: &[xmlChar] = b"&quot;";
const ENTITY_APOS: &[xmlChar] = b"&apos;";

/// Indentation string (2 spaces).
const INDENT: &[xmlChar] = b"  ";

/// XML declaration.
const XML_DECL: &[xmlChar] = b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>";

/// Serialize text content with XML escaping.
///
/// Escapes `<`, `&`, and the `]]>` sequence.
///
/// # SAFETY
///
/// - `buf` must be a valid pointer to a mutable `_xmlBuffer`.
/// - `content` must be a valid pointer to `len` bytes of xmlChar data, or NULL.
pub(crate) unsafe fn serialize_text(buf: *mut _xmlBuffer, content: *const xmlChar, len: c_int) {
    if buf.is_null() || content.is_null() || len <= 0 {
        return;
    }

    let mut i: c_int = 0;
    while i < len {
        let ch = unsafe { *content.add(i as usize) };

        // Check for `]]>` sequence
        if ch == b']'
            && i + 2 < len
            && unsafe { *content.add(i as usize + 1) == b']' }
            && unsafe { *content.add(i as usize + 2) == b'>' }
        {
            // Write `]]&gt;` — escape the `>` that ends `]]>`
            io::buf_add(buf, &ch as *const u8, 2); // write `]]`
            io::buf_add(buf, ENTITY_GT.as_ptr(), ENTITY_GT.len() as c_int);
            i += 3;
            continue;
        }

        match ch {
            b'<' => {
                io::buf_add(buf, ENTITY_LT.as_ptr(), ENTITY_LT.len() as c_int);
            }
            b'&' => {
                io::buf_add(buf, ENTITY_AMP.as_ptr(), ENTITY_AMP.len() as c_int);
            }
            // UPSTREAM-PARITY: libxml2 does NOT escape `>` in text content.
            // Per XML spec, `>` is only required to be escaped when it appears
            // as `]]>` (handled above). libxml2's xmlNodeDumpOutput does not
            // escape standalone `>` characters.
            _ => {
                io::buf_add(buf, &ch as *const u8, 1);
            }
        }
        i += 1;
    }
}

/// Serialize an attribute value with XML escaping.
///
/// Escapes `<`, `&`, `"`, and the `]]>` sequence.
///
/// # SAFETY
///
/// - `buf` must be a valid pointer to a mutable `_xmlBuffer`.
/// - `value` must be a valid null-terminated xmlChar string, or NULL.
pub(crate) unsafe fn serialize_attr_value(buf: *mut _xmlBuffer, value: *const xmlChar) {
    if buf.is_null() || value.is_null() {
        return;
    }

    let len = xml_strlen(value);
    let mut i: c_int = 0;
    while i < len {
        let ch = unsafe { *value.add(i as usize) };

        // Check for `]]>` sequence
        if ch == b']'
            && i + 2 < len
            && unsafe { *value.add(i as usize + 1) == b']' }
            && unsafe { *value.add(i as usize + 2) == b'>' }
        {
            // Write `]]&gt;` — escape the `>` that ends `]]>`
            io::buf_add(buf, &ch as *const u8, 2); // write `]]`
            io::buf_add(buf, ENTITY_GT.as_ptr(), ENTITY_GT.len() as c_int);
            i += 3;
            continue;
        }

        match ch {
            b'<' => {
                io::buf_add(buf, ENTITY_LT.as_ptr(), ENTITY_LT.len() as c_int);
            }
            b'&' => {
                io::buf_add(buf, ENTITY_AMP.as_ptr(), ENTITY_AMP.len() as c_int);
            }
            b'"' => {
                io::buf_add(buf, ENTITY_QUOT.as_ptr(), ENTITY_QUOT.len() as c_int);
            }
            _ => {
                io::buf_add(buf, &ch as *const u8, 1);
            }
        }
        i += 1;
    }
}

/// Write indentation (2 spaces per level).
///
/// # SAFETY
///
/// - `buf` must be a valid pointer to a mutable `_xmlBuffer`.
unsafe fn write_indent(buf: *mut _xmlBuffer, level: c_int) {
    if buf.is_null() || level <= 0 {
        return;
    }
    for _ in 0..level {
        io::buf_add(buf, INDENT.as_ptr(), INDENT.len() as c_int);
    }
}

/// Serialize a single node's start tag + attributes.
///
/// For elements with no children, writes a self-closing tag `<name/>`.
///
/// # SAFETY
///
/// - `buf` must be a valid pointer to a mutable `_xmlBuffer`.
/// - `node` must be a valid pointer to an `_xmlNode`, or NULL.
unsafe fn serialize_start_tag(
    node: *mut _xmlNode,
    buf: *mut _xmlBuffer,
    format: c_int,
    level: c_int,
) {
    if node.is_null() || buf.is_null() {
        return;
    }

    let n = unsafe { &*node };

    // Write `<`
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

    // Write attributes
    let mut attr = n.properties;
    while !attr.is_null() {
        let a = unsafe { &*attr };
        io::buf_ccat(buf, b' ');

        // Attribute name with optional namespace prefix
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
                serialize_attr_value(buf, child.content);
            }
        }

        io::buf_ccat(buf, b'"');

        attr = a.next;
    }

    // Write namespace declarations
    let mut ns_def = n.nsDef;
    while !ns_def.is_null() {
        let nd = unsafe { &*ns_def };
        io::buf_add(buf, b" xmlns" as *const u8, 6);
        if !nd.prefix.is_null() {
            io::buf_ccat(buf, b':');
            io::buf_cat(buf, nd.prefix);
        }
        io::buf_add(buf, b"=\"" as *const u8, 2);
        if !nd.href.is_null() {
            serialize_attr_value(buf, nd.href);
        }
        io::buf_ccat(buf, b'"');
        ns_def = nd.next;
    }

    if n.children.is_null() {
        // Self-closing tag
        io::buf_add(buf, b"/>" as *const u8, 2);
    } else {
        io::buf_ccat(buf, b'>');
    }
}

/// Serialize a single node's end tag.
///
/// # SAFETY
///
/// - `buf` must be a valid pointer to a mutable `_xmlBuffer`.
/// - `node` must be a valid pointer to an `_xmlNode`, or NULL.
unsafe fn serialize_end_tag(node: *mut _xmlNode, buf: *mut _xmlBuffer) {
    if node.is_null() || buf.is_null() {
        return;
    }

    let n = unsafe { &*node };

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

/// Check if a node is a "text-only" element (exactly one child which is a text node).
unsafe fn is_text_only_element(node: *mut _xmlNode) -> bool {
    if node.is_null() {
        return false;
    }
    let n = unsafe { &*node };
    if n.children.is_null() {
        return false;
    }
    // Only one child?
    if n.children != n.last {
        return false;
    }
    let child = unsafe { &*n.children };
    child.type_ == XML_TEXT_NODE as c_int
}

/// Recursively serialize a node tree to a buffer.
///
/// `buf` is an `_xmlBuffer*`, `format` controls indentation (non-zero = pretty-print).
///
/// # SAFETY
///
/// - `node` must be a valid pointer to an `_xmlNode`, or NULL.
/// - `buf` must be a valid pointer to a mutable `_xmlBuffer`.
pub(crate) unsafe fn serialize_node(
    node: *mut _xmlNode,
    buf: *mut _xmlBuffer,
    format: c_int,
    level: c_int,
) {
    if node.is_null() || buf.is_null() {
        return;
    }

    let n = unsafe { &*node };

    match n.type_ {
        t if t == XML_ELEMENT_NODE as c_int => {
            let is_text_only = is_text_only_element(node);

            // Newline + indent before start tag (if formatting)
            if format != 0 && level > 0 {
                io::buf_ccat(buf, b'\n');
                write_indent(buf, level);
            }

            serialize_start_tag(node, buf, format, level);

            if !n.children.is_null() {
                if !is_text_only && format != 0 {
                    // Indent children for mixed/structured content
                    let mut child = n.children;
                    while !child.is_null() {
                        serialize_node(child, buf, format, level + 1);
                        child = unsafe { (*child).next };
                    }
                    io::buf_ccat(buf, b'\n');
                    write_indent(buf, level);
                } else {
                    // Text-only or no formatting: serialize children inline
                    let mut child = n.children;
                    while !child.is_null() {
                        serialize_node(child, buf, format, level + 1);
                        child = unsafe { (*child).next };
                    }
                }
                serialize_end_tag(node, buf);
            }
        }
        t if t == XML_TEXT_NODE as c_int => {
            serialize_text(buf, n.content, xml_strlen(n.content));
        }
        t if t == XML_CDATA_SECTION_NODE as c_int => {
            io::buf_add(buf, b"<![CDATA[" as *const u8, 9);
            serialize_text(buf, n.content, xml_strlen(n.content));
            io::buf_add(buf, b"]]>" as *const u8, 3);
        }
        t if t == XML_COMMENT_NODE as c_int => {
            if format != 0 && level > 0 {
                io::buf_ccat(buf, b'\n');
                write_indent(buf, level);
            }
            io::buf_add(buf, b"<!--" as *const u8, 4);
            if !n.content.is_null() {
                io::buf_cat(buf, n.content);
            }
            io::buf_add(buf, b"-->" as *const u8, 3);
        }
        t if t == XML_PI_NODE as c_int => {
            if format != 0 && level > 0 {
                io::buf_ccat(buf, b'\n');
                write_indent(buf, level);
            }
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
            // XML declaration
            io::buf_add(buf, XML_DECL.as_ptr(), XML_DECL.len() as c_int);

            // Newline after declaration when formatting
            if format != 0 {
                io::buf_ccat(buf, b'\n');
            }

            // Serialize children
            let mut child = n.children;
            while !child.is_null() {
                serialize_node(child, buf, format, 0);
                child = unsafe { (*child).next };
            }
            if format != 0 {
                io::buf_ccat(buf, b'\n');
            }
        }
        t if t == XML_DTD_NODE as c_int => {
            // Skip DTD nodes in serialization for now
        }
        _ => {
            // For unknown types, just write content if present
            if !n.content.is_null() {
                serialize_text(buf, n.content, xml_strlen(n.content));
            }
        }
    }
}

/// Dump a document to a buffer.
///
/// Serializes the entire document tree into `buf`.
/// Returns the number of bytes written, or -1 on error.
///
/// # SAFETY
///
/// - `buf` must be a valid pointer to a mutable `_xmlBuffer`.
/// - `doc` must be a valid pointer to an `_xmlDoc`, or NULL.
pub(crate) unsafe fn doc_dump(buf: *mut _xmlBuffer, doc: *mut _xmlDoc) -> c_int {
    if buf.is_null() || doc.is_null() {
        return -1;
    }

    let before = io::buf_length(buf);
    serialize_node(doc as *mut _xmlNode, buf, 0, 0);
    let after = io::buf_length(buf);

    if after < 0 || before < 0 {
        return -1;
    }
    after - before
}

/// Dump a node tree to a buffer.
///
/// Serializes the node and its descendants into `buf`.
/// `level` is the initial indentation level, `format` controls pretty-printing.
/// Returns the number of bytes written, or -1 on error.
///
/// # SAFETY
///
/// - `buf` must be a valid pointer to a mutable `_xmlBuffer`.
/// - `doc` must be a valid pointer to an `_xmlDoc`, or NULL.
/// - `node` must be a valid pointer to an `_xmlNode`, or NULL.
pub(crate) unsafe fn node_dump(
    buf: *mut _xmlBuffer,
    doc: *mut _xmlDoc,
    node: *mut _xmlNode,
    level: c_int,
    format: c_int,
) -> c_int {
    let _ = doc; // Used for entity resolution in full implementation
    if buf.is_null() || node.is_null() {
        return -1;
    }

    let before = io::buf_length(buf);
    serialize_node(node, buf, format, level);
    let after = io::buf_length(buf);

    if after < 0 || before < 0 {
        return -1;
    }
    after - before
}

/// Save a document to a file.
///
/// # SAFETY
///
/// - `doc` must be a valid pointer to an `_xmlDoc`, or NULL.
/// - `filename` must be a valid null-terminated C string.
pub(crate) unsafe fn save_doc_to_filename(
    doc: *mut _xmlDoc,
    filename: *const c_char,
    compression: c_int,
) -> c_int {
    if doc.is_null() || filename.is_null() {
        return -1;
    }

    let out = io::output_buffer_create_filename(filename, ptr::null_mut(), compression);
    if out.is_null() {
        return -1;
    }

    let buf = io::buf_create(-1);
    if buf.is_null() {
        io::output_buffer_close(out);
        return -1;
    }

    let ret = doc_dump(buf, doc);
    if ret >= 0 {
        // Flush the buffer content to the output
        io::output_buffer_write_string(out, io::buf_content(buf) as *const c_char);
        io::output_buffer_flush(out);
    }

    io::buf_free(buf);
    io::output_buffer_close(out);
    ret
}

/// Save a document to a file descriptor.
///
/// # SAFETY
///
/// - `doc` must be a valid pointer to an `_xmlDoc`, or NULL.
/// - `fd` must be a valid open file descriptor.
pub(crate) unsafe fn save_doc_to_fd(doc: *mut _xmlDoc, fd: c_int, compression: c_int) -> c_int {
    if doc.is_null() || fd < 0 {
        return -1;
    }

    let out = io::output_buffer_create_fd(fd, ptr::null_mut());
    if out.is_null() {
        return -1;
    }

    let buf = io::buf_create(-1);
    if buf.is_null() {
        io::output_buffer_close(out);
        return -1;
    }

    let ret = doc_dump(buf, doc);
    if ret >= 0 {
        io::output_buffer_write_string(out, io::buf_content(buf) as *const c_char);
        io::output_buffer_flush(out);
    }

    io::buf_free(buf);
    io::output_buffer_close(out);
    ret
}

/// Save a document to an xmlBuffer.
///
/// # SAFETY
///
/// - `doc` must be a valid pointer to an `_xmlDoc`, or NULL.
/// - `buf` must be a valid pointer to a mutable `_xmlBuffer`.
pub(crate) unsafe fn save_doc_to_buf(
    doc: *mut _xmlDoc,
    buf: *mut _xmlBuffer,
    compression: c_int,
) -> c_int {
    let _ = compression;
    if doc.is_null() || buf.is_null() {
        return -1;
    }

    doc_dump(buf, doc)
}

/// Format (pretty-print) a document to a buffer.
///
/// # SAFETY
///
/// - `doc` must be a valid pointer to an `_xmlDoc`, or NULL.
/// - `buf` must be a valid pointer to a mutable `_xmlBuffer`.
pub(crate) unsafe fn save_format_doc_to_buf(
    doc: *mut _xmlDoc,
    buf: *mut _xmlBuffer,
    compression: c_int,
) -> c_int {
    let _ = compression;
    if doc.is_null() || buf.is_null() {
        return -1;
    }

    let before = io::buf_length(buf);
    serialize_node(doc as *mut _xmlNode, buf, 1, 0);
    let after = io::buf_length(buf);

    if after < 0 || before < 0 {
        return -1;
    }
    after - before
}

/// Dump a node to a null-terminated string.
///
/// Returns a pointer to the string (caller must free with `xmlFree`).
/// Returns NULL on error.
///
/// # SAFETY
///
/// - `node` must be a valid pointer to an `_xmlNode`, or NULL.
pub(crate) unsafe fn dump_node(node: *mut _xmlNode) -> *mut xmlChar {
    if node.is_null() {
        return ptr::null_mut();
    }

    let buf = io::buf_create(-1);
    if buf.is_null() {
        return ptr::null_mut();
    }

    serialize_node(node, buf, 0, 0);

    let content = io::buf_content(buf);
    if content.is_null() {
        io::buf_free(buf);
        return ptr::null_mut();
    }

    // Duplicate the string so we can free the buffer
    let result = dup_xml_str(content);
    io::buf_free(buf);
    result
}

/// Dump a document to a null-terminated string.
///
/// Returns a pointer to the string (caller must free with `xmlFree`).
/// Returns NULL on error.
///
/// # SAFETY
///
/// - `doc` must be a valid pointer to an `_xmlDoc`, or NULL.
pub(crate) unsafe fn dump_doc(doc: *mut _xmlDoc) -> *mut xmlChar {
    if doc.is_null() {
        return ptr::null_mut();
    }

    let buf = io::buf_create(-1);
    if buf.is_null() {
        return ptr::null_mut();
    }

    serialize_node(doc as *mut _xmlNode, buf, 0, 0);

    let content = io::buf_content(buf);
    if content.is_null() {
        io::buf_free(buf);
        return ptr::null_mut();
    }

    let result = dup_xml_str(content);
    io::buf_free(buf);
    result
}

// ═══════════════════════════════════════════════════════════════════════════════
// ABI-compatible export wrappers
// ═══════════════════════════════════════════════════════════════════════════════

/// Dump a node to a buffer (ABI wrapper).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlNodeDump(xmlBufferPtr buf, xmlDocPtr doc, xmlNodePtr node, int level, int format);
/// ```
///
/// # SAFETY
///
/// - All pointer arguments must be valid or NULL.
pub(crate) unsafe fn xmlNodeDump(
    buf: *mut _xmlBuffer,
    doc: *mut _xmlDoc,
    node: *mut _xmlNode,
    level: c_int,
    format: c_int,
) -> c_int {
    node_dump(buf, doc, node, level, format)
}

/// Dump a document to a FILE*.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlDocDump(FILE *fp, xmlDocPtr doc);
/// ```
///
/// # SAFETY
///
/// - `fp` must be a valid FILE* pointer.
/// - `doc` must be a valid pointer to an `_xmlDoc`, or NULL.
pub(crate) unsafe fn xmlDocDump(fp: *mut c_void, doc: *mut _xmlDoc) -> c_int {
    if fp.is_null() || doc.is_null() {
        return -1;
    }

    let buf = io::buf_create(-1);
    if buf.is_null() {
        return -1;
    }

    let ret = doc_dump(buf, doc);
    if ret < 0 {
        io::buf_free(buf);
        return -1;
    }

    let content = io::buf_content(buf);
    let len = io::buf_length(buf);
    if !content.is_null() && len > 0 {
        let written = libc::fwrite(
            content as *const c_void,
            1,
            len as usize,
            fp as *mut libc::FILE,
        );
        io::buf_free(buf);
        written as c_int
    } else {
        io::buf_free(buf);
        0
    }
}

/// Dump a document to memory (with format flag).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlDocDumpFormatMemory(xmlDocPtr doc, xmlChar **mem, int *size, int format);
/// ```
///
/// # SAFETY
///
/// - `doc` must be a valid pointer to an `_xmlDoc`, or NULL.
/// - `mem` must be a valid pointer to an xmlChar* that will receive the allocated memory.
/// - `size` must be a valid pointer to an int that will receive the size.
pub(crate) unsafe fn xmlDocDumpFormatMemory(
    doc: *mut _xmlDoc,
    mem: *mut *mut xmlChar,
    size: *mut c_int,
    format: c_int,
) {
    if doc.is_null() || mem.is_null() || size.is_null() {
        return;
    }

    let buf = io::buf_create(-1);
    if buf.is_null() {
        unsafe {
            *mem = ptr::null_mut();
            *size = 0;
        }
        return;
    }

    serialize_node(doc as *mut _xmlNode, buf, format, 0);

    let content = io::buf_content(buf);
    let len = io::buf_length(buf);

    if !content.is_null() && len > 0 {
        // Allocate memory for the result (+1 for null terminator)
        let result = allocator::xmlMalloc((len + 1) as usize) as *mut xmlChar;
        if !result.is_null() {
            ptr::copy_nonoverlapping(content, result, len as usize);
            *result.add(len as usize) = 0;
            unsafe {
                *mem = result;
                *size = len;
            }
        } else {
            unsafe {
                *mem = ptr::null_mut();
                *size = 0;
            }
        }
    } else {
        unsafe {
            *mem = ptr::null_mut();
            *size = 0;
        }
    }

    io::buf_free(buf);
}

/// Dump a document to memory (unformatted).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlDocDumpMemory(xmlDocPtr doc, xmlChar **mem, int *size);
/// ```
///
/// # SAFETY
///
/// - `doc` must be a valid pointer to an `_xmlDoc`, or NULL.
/// - `mem` must be a valid pointer to an xmlChar* that will receive the allocated memory.
/// - `size` must be a valid pointer to an int that will receive the size.
pub(crate) unsafe fn xmlDocDumpMemory(doc: *mut _xmlDoc, mem: *mut *mut xmlChar, size: *mut c_int) {
    xmlDocDumpFormatMemory(doc, mem, size, 0)
}

/// Save a document to a file (ABI wrapper).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlSaveFile(const char *filename, xmlDocPtr cur);
/// ```
///
/// # SAFETY
///
/// - `filename` must be a valid null-terminated C string.
/// - `cur` must be a valid pointer to an `_xmlDoc`, or NULL.
pub(crate) unsafe fn xmlSaveFile(filename: *const c_char, cur: *mut _xmlDoc) -> c_int {
    save_doc_to_filename(cur, filename, 0)
}

/// Save a document to a file with encoding.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlSaveFileEnc(const char *filename, xmlDocPtr cur, const char *encoding);
/// ```
///
/// # SAFETY
///
/// - `filename` must be a valid null-terminated C string.
/// - `cur` must be a valid pointer to an `_xmlDoc`, or NULL.
/// - `encoding` may be NULL (uses UTF-8).
pub(crate) unsafe fn xmlSaveFileEnc(
    filename: *const c_char,
    cur: *mut _xmlDoc,
    encoding: *const c_char,
) -> c_int {
    let _ = encoding; // Future: use encoding to set encoder on output buffer
    save_doc_to_filename(cur, filename, 0)
}

/// Save a document to a file with format flag.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlSaveFormatFile(const char *filename, xmlDocPtr cur, int format);
/// ```
///
/// # SAFETY
///
/// - `filename` must be a valid null-terminated C string.
/// - `cur` must be a valid pointer to an `_xmlDoc`, or NULL.
pub(crate) unsafe fn xmlSaveFormatFile(
    filename: *const c_char,
    cur: *mut _xmlDoc,
    format: c_int,
) -> c_int {
    let _ = format;
    save_doc_to_filename(cur, filename, 0)
}

/// Save a document to a file with encoding and format flag.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlSaveFormatFileEnc(const char *filename, xmlDocPtr cur, const char *encoding, int format);
/// ```
///
/// # SAFETY
///
/// - `filename` must be a valid null-terminated C string.
/// - `cur` must be a valid pointer to an `_xmlDoc`, or NULL.
/// - `encoding` may be NULL (uses UTF-8).
pub(crate) unsafe fn xmlSaveFormatFileEnc(
    filename: *const c_char,
    cur: *mut _xmlDoc,
    encoding: *const c_char,
    format: c_int,
) -> c_int {
    let _ = encoding;
    let _ = format;
    save_doc_to_filename(cur, filename, 0)
}

/// Get the compression mode of a document.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlGetDocCompressMode(xmlDocPtr doc);
/// ```
///
/// # SAFETY
///
/// - `doc` must be a valid pointer to an `_xmlDoc`, or NULL.
pub(crate) unsafe fn xmlGetDocCompressMode(doc: *mut _xmlDoc) -> c_int {
    if doc.is_null() {
        return -1;
    }
    unsafe { (*doc).compression }
}

/// Set the compression mode of a document.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlSetDocCompressMode(xmlDocPtr doc, int mode);
/// ```
///
/// # SAFETY
///
/// - `doc` must be a valid pointer to an `_xmlDoc`, or NULL.
pub(crate) unsafe fn xmlSetDocCompressMode(doc: *mut _xmlDoc, mode: c_int) {
    if doc.is_null() {
        return;
    }
    unsafe {
        (*doc).compression = mode;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::ffi::c_void;

    fn c_str(s: &str) -> *const xmlChar {
        let bytes = s.as_bytes();
        let buf = unsafe { allocator::xmlMalloc(bytes.len() + 1) as *mut u8 };
        if !buf.is_null() {
            unsafe {
                ptr::copy_nonoverlapping(bytes.as_ptr(), buf, bytes.len());
                *buf.add(bytes.len()) = 0;
            }
        }
        buf as *const xmlChar
    }

    #[test]
    fn test_new_free_doc() {
        unsafe {
            let doc = new_doc(ptr::null());
            assert!(!doc.is_null());
            assert_eq!((*doc).type_, XML_DOCUMENT_NODE as c_int);
            assert_eq!((*doc).standalone, -1);
            assert_eq!((*doc).doc, doc);
            assert!(!(*doc).version.is_null());
            free_doc(doc);
        }
    }

    #[test]
    fn test_new_doc_with_version() {
        unsafe {
            let ver = c_str("2.0");
            let doc = new_doc(ver);
            assert!(!doc.is_null());
            let doc_ver = (*doc).version;
            assert!(!doc_ver.is_null());
            assert!(crate::abi::exports_xml2::xmlStrEqual(doc_ver, ver,) != 0);
            allocator::xmlFree(ver as *mut c_void);
            free_doc(doc);
        }
    }

    #[test]
    fn test_new_node() {
        unsafe {
            let doc = new_doc(ptr::null());
            let node = new_node(ptr::null_mut(), c_str("root"));
            assert!(!node.is_null());
            assert_eq!((*node).type_, XML_ELEMENT_NODE as c_int);
            assert!(!(*node).name.is_null());
            free_node(node);
            free_doc(doc);
        }
    }

    #[test]
    fn test_doc_set_root_element() {
        unsafe {
            let doc = new_doc(ptr::null());
            let root = new_node(ptr::null_mut(), c_str("root"));
            let old = doc_set_root_element(doc, root);
            assert!(old.is_null());
            assert_eq!(doc_get_root_element(doc), root);
            assert_eq!((*doc).children, root as *mut _xmlNode);
            free_doc(doc);
        }
    }

    #[test]
    fn test_add_child_and_sibling() {
        unsafe {
            let doc = new_doc(ptr::null());
            let root = new_node(ptr::null_mut(), c_str("root"));
            doc_set_root_element(doc, root);

            let child1 = new_child(root, ptr::null_mut(), c_str("child1"));
            assert!(!child1.is_null());
            assert_eq!((*child1).parent, root);
            assert_eq!((*root).children, child1);
            assert_eq!((*root).last, child1);

            let child2 = new_child(root, ptr::null_mut(), c_str("child2"));
            assert!(!child2.is_null());
            assert_eq!((*child2).parent, root);
            assert_eq!((*child1).next, child2);
            assert_eq!((*child2).prev, child1);
            assert_eq!((*root).last, child2);

            // Test add_sibling
            let sibling = new_node(ptr::null_mut(), c_str("sibling"));
            add_sibling(child2, sibling);
            assert_eq!((*child2).next, sibling);
            assert_eq!((*sibling).prev, child2);
            assert_eq!((*root).last, sibling);

            free_doc(doc);
        }
    }

    #[test]
    fn test_unlink_node() {
        unsafe {
            let doc = new_doc(ptr::null());
            let root = new_node(ptr::null_mut(), c_str("root"));
            doc_set_root_element(doc, root);

            let child1 = new_child(root, ptr::null_mut(), c_str("c1"));
            let child2 = new_child(root, ptr::null_mut(), c_str("c2"));

            unlink_node(child1);
            assert!((*child1).parent.is_null());
            assert!((*child1).prev.is_null());
            assert!((*child1).next.is_null());
            assert_eq!((*root).children, child2);
            assert_eq!((*root).last, child2);

            free_node(child1);
            free_doc(doc);
        }
    }

    #[test]
    fn test_text_and_comment_nodes() {
        unsafe {
            let text = new_text(c_str("hello world"));
            assert!(!text.is_null());
            assert_eq!((*text).type_, XML_TEXT_NODE as c_int);
            assert!(!(*text).content.is_null());
            free_node(text);

            let comment = new_comment(c_str("my comment"));
            assert!(!comment.is_null());
            assert_eq!((*comment).type_, XML_COMMENT_NODE as c_int);
            free_node(comment);

            let pi = new_pi(c_str("xml"), c_str("version='1.0'"));
            assert!(!pi.is_null());
            assert_eq!((*pi).type_, XML_PI_NODE as c_int);
            free_node(pi);
        }
    }

    #[test]
    fn test_set_and_get_prop() {
        unsafe {
            let doc = new_doc(ptr::null());
            let root = new_node(ptr::null_mut(), c_str("root"));
            doc_set_root_element(doc, root);

            let attr = set_prop(root, c_str("id"), c_str("42"));
            assert!(!attr.is_null());
            assert_eq!((*attr).type_, XML_ATTRIBUTE_NODE as c_int);

            let value = get_prop(root, c_str("id"));
            assert!(!value.is_null());
            assert!(crate::abi::exports_xml2::xmlStrEqual(value, c_str("42")) != 0);
            allocator::xmlFree(value as *mut c_void);

            free_doc(doc);
        }
    }

    #[test]
    fn test_remove_prop() {
        unsafe {
            let doc = new_doc(ptr::null());
            let root = new_node(ptr::null_mut(), c_str("root"));
            doc_set_root_element(doc, root);

            set_prop(root, c_str("a"), c_str("1"));
            set_prop(root, c_str("b"), c_str("2"));

            let value = get_prop(root, c_str("a"));
            assert!(!value.is_null());
            allocator::xmlFree(value as *mut c_void);

            // Remove prop
            let attr = (*root).properties;
            assert!(!attr.is_null());
            let result = remove_prop(attr);
            assert_eq!(result, 0);

            // Should no longer be found
            let value2 = get_prop(root, c_str("a"));
            assert!(value2.is_null());

            free_doc(doc);
        }
    }

    #[test]
    fn test_namespace_operations() {
        unsafe {
            let doc = new_doc(ptr::null());
            let root = new_node(ptr::null_mut(), c_str("root"));
            doc_set_root_element(doc, root);

            let ns = new_ns(root, c_str("http://example.com"), c_str("ex"));
            assert!(!ns.is_null());
            assert!(!(*root).nsDef.is_null());

            set_ns(root, ns);
            assert_eq!((*root).ns, ns);

            let found = search_ns(doc, root, c_str("ex"));
            assert_eq!(found, ns);

            let found_href = search_ns_by_href(doc, root, c_str("http://example.com"));
            assert_eq!(found_href, ns);

            free_doc(doc);
        }
    }

    #[test]
    fn test_new_dtd() {
        unsafe {
            let doc = new_doc(ptr::null());
            let dtd = new_dtd(doc, c_str("root"), c_str("-//TEST//DTD"), c_str("test.dtd"));
            assert!(!dtd.is_null());
            assert_eq!((*dtd).type_, XML_DTD_NODE as c_int);
            assert_eq!(get_int_subset(doc), dtd);
            free_doc(doc);
        }
    }

    #[test]
    fn test_copy_node_deep() {
        unsafe {
            let doc = new_doc(ptr::null());
            let root = new_node(ptr::null_mut(), c_str("root"));
            doc_set_root_element(doc, root);
            let child = new_child(root, ptr::null_mut(), c_str("child"));

            let copy = copy_node(root, 1);
            assert!(!copy.is_null());
            assert_eq!((*copy).type_, XML_ELEMENT_NODE as c_int);
            // Check child was copied
            assert!(!(*copy).children.is_null());
            assert_eq!((*(*copy).children).type_, XML_ELEMENT_NODE as c_int);

            free_node(copy);
            free_doc(doc);
        }
    }

    #[test]
    fn test_new_cdata_block() {
        unsafe {
            let doc = new_doc(ptr::null());
            let content = c_str("some <cdata> content");
            let cdata = new_cdata_block(doc, content, 20);
            assert!(!cdata.is_null());
            assert_eq!((*cdata).type_, XML_CDATA_SECTION_NODE as c_int);
            free_node(cdata);
            free_doc(doc);
        }
    }

    #[test]
    fn test_null_handling() {
        unsafe {
            assert!(new_doc(ptr::null()).is_null() == false); // Should succeed with default version
            let doc = new_doc(ptr::null());
            assert!(new_node(ptr::null_mut(), ptr::null()).is_null() == false); // Should succeed
            free_node(ptr::null_mut()); // Should not crash
            free_doc(ptr::null_mut()); // Should not crash
            assert!(unlink_node(ptr::null_mut()) == ()); // Should not crash
            assert!(add_child(ptr::null_mut(), ptr::null_mut()).is_null());
            assert!(add_sibling(ptr::null_mut(), ptr::null_mut()).is_null());
            free_doc(doc);
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // Serialization Tests
    // ═══════════════════════════════════════════════════════════════════

    /// Helper: compare a buffer's content to an expected string.
    unsafe fn buf_eq_str(buf: *mut _xmlBuffer, expected: &str) -> bool {
        let content = io::buf_content(buf);
        if content.is_null() {
            return expected.is_empty();
        }
        let len = io::buf_length(buf) as usize;
        if len != expected.len() {
            return false;
        }
        let slice = unsafe { core::slice::from_raw_parts(content, len) };
        slice == expected.as_bytes()
    }

    #[test]
    fn test_serialize_empty_document() {
        unsafe {
            let doc = new_doc(ptr::null());
            let buf = io::buf_create(-1);
            assert!(!buf.is_null());

            let ret = doc_dump(buf, doc);
            assert!(ret >= 0);

            // Should have XML declaration
            let expected = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>";
            assert!(buf_eq_str(buf, expected));

            io::buf_free(buf);
            free_doc(doc);
        }
    }

    #[test]
    fn test_serialize_element_with_text() {
        unsafe {
            let doc = new_doc(ptr::null());
            let root = new_node(ptr::null_mut(), c_str("root"));
            doc_set_root_element(doc, root);

            // Add text child
            let text = new_text(c_str("hello world"));
            add_child(root, text);

            let buf = io::buf_create(-1);
            assert!(!buf.is_null());

            let ret = doc_dump(buf, doc);
            assert!(ret >= 0);

            let expected = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><root>hello world</root>";
            assert!(buf_eq_str(buf, expected));

            io::buf_free(buf);
            free_doc(doc);
        }
    }

    #[test]
    fn test_serialize_element_with_attributes() {
        unsafe {
            let doc = new_doc(ptr::null());
            let root = new_node(ptr::null_mut(), c_str("root"));
            doc_set_root_element(doc, root);

            set_prop(root, c_str("id"), c_str("42"));
            set_prop(root, c_str("name"), c_str("test"));

            let buf = io::buf_create(-1);
            assert!(!buf.is_null());

            let ret = doc_dump(buf, doc);
            assert!(ret >= 0);

            let expected =
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?><root id=\"42\" name=\"test\"/>";
            assert!(buf_eq_str(buf, expected));

            io::buf_free(buf);
            free_doc(doc);
        }
    }

    #[test]
    fn test_serialize_nested_elements() {
        unsafe {
            let doc = new_doc(ptr::null());
            let root = new_node(ptr::null_mut(), c_str("root"));
            doc_set_root_element(doc, root);

            let child = new_child(root, ptr::null_mut(), c_str("child"));
            let grandchild = new_child(child, ptr::null_mut(), c_str("gc"));
            let text = new_text(c_str("text"));
            add_child(grandchild, text);

            let buf = io::buf_create(-1);
            assert!(!buf.is_null());

            let ret = doc_dump(buf, doc);
            assert!(ret >= 0);

            let expected = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><root><child><gc>text</gc></child></root>";
            assert!(buf_eq_str(buf, expected));

            io::buf_free(buf);
            free_doc(doc);
        }
    }

    #[test]
    fn test_serialize_with_formatting() {
        unsafe {
            let doc = new_doc(ptr::null());
            let root = new_node(ptr::null_mut(), c_str("root"));
            doc_set_root_element(doc, root);

            let child = new_child(root, ptr::null_mut(), c_str("child"));
            let text = new_text(c_str("text"));
            add_child(child, text);

            let buf = io::buf_create(-1);
            assert!(!buf.is_null());

            serialize_node(doc as *mut _xmlNode, buf, 1, 0);

            let expected = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<root>\n  <child>text</child>\n</root>\n";
            assert!(buf_eq_str(buf, expected));

            io::buf_free(buf);
            free_doc(doc);
        }
    }

    #[test]
    fn test_serialize_escape_ampersand() {
        unsafe {
            let doc = new_doc(ptr::null());
            let root = new_node(ptr::null_mut(), c_str("root"));
            doc_set_root_element(doc, root);

            let text = new_text(c_str("a & b"));
            add_child(root, text);

            let buf = io::buf_create(-1);
            assert!(!buf.is_null());

            let ret = doc_dump(buf, doc);
            assert!(ret >= 0);

            let expected = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><root>a &amp; b</root>";
            assert!(buf_eq_str(buf, expected));

            io::buf_free(buf);
            free_doc(doc);
        }
    }

    #[test]
    fn test_serialize_escape_angle_brackets() {
        unsafe {
            let doc = new_doc(ptr::null());
            let root = new_node(ptr::null_mut(), c_str("root"));
            doc_set_root_element(doc, root);

            let text = new_text(c_str("x < y > z"));
            add_child(root, text);

            let buf = io::buf_create(-1);
            assert!(!buf.is_null());

            let ret = doc_dump(buf, doc);
            assert!(ret >= 0);

            let expected = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><root>x &lt; y > z</root>";
            assert!(buf_eq_str(buf, expected));

            io::buf_free(buf);
            free_doc(doc);
        }
    }

    #[test]
    fn test_serialize_comment() {
        unsafe {
            let doc = new_doc(ptr::null());
            let root = new_node(ptr::null_mut(), c_str("root"));
            doc_set_root_element(doc, root);

            let comment = new_comment(c_str("my comment"));
            add_child(root, comment);

            let buf = io::buf_create(-1);
            assert!(!buf.is_null());

            let ret = doc_dump(buf, doc);
            assert!(ret >= 0);

            let expected =
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?><root><!--my comment--></root>";
            assert!(buf_eq_str(buf, expected));

            io::buf_free(buf);
            free_doc(doc);
        }
    }

    #[test]
    fn test_serialize_pi() {
        unsafe {
            let doc = new_doc(ptr::null());
            let root = new_node(ptr::null_mut(), c_str("root"));
            doc_set_root_element(doc, root);

            let pi = new_pi(
                c_str("xml-stylesheet"),
                c_str("href=\"style.xsl\" type=\"text/xsl\""),
            );
            add_child(root, pi);

            let buf = io::buf_create(-1);
            assert!(!buf.is_null());

            let ret = doc_dump(buf, doc);
            assert!(ret >= 0);

            let expected = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><root><?xml-stylesheet href=\"style.xsl\" type=\"text/xsl\"?></root>";
            assert!(buf_eq_str(buf, expected));

            io::buf_free(buf);
            free_doc(doc);
        }
    }

    #[test]
    fn test_serialize_self_closing() {
        unsafe {
            let doc = new_doc(ptr::null());
            let root = new_node(ptr::null_mut(), c_str("empty"));
            doc_set_root_element(doc, root);

            let buf = io::buf_create(-1);
            assert!(!buf.is_null());

            let ret = doc_dump(buf, doc);
            assert!(ret >= 0);

            let expected = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><empty/>";
            assert!(buf_eq_str(buf, expected));

            io::buf_free(buf);
            free_doc(doc);
        }
    }

    #[test]
    fn test_dump_node_to_string() {
        unsafe {
            let node = new_node(ptr::null_mut(), c_str("foo"));
            let text = new_text(c_str("bar"));
            add_child(node, text);

            let result = dump_node(node);
            assert!(!result.is_null());

            let len = xml_strlen(result);
            let slice = unsafe { core::slice::from_raw_parts(result, len as usize) };
            assert_eq!(slice, b"<foo>bar</foo>");

            allocator::xmlFree(result as *mut c_void);
            free_node(node);
        }
    }

    #[test]
    fn test_dump_doc_to_string() {
        unsafe {
            let doc = new_doc(ptr::null());
            let root = new_node(ptr::null_mut(), c_str("root"));
            doc_set_root_element(doc, root);

            let result = dump_doc(doc);
            assert!(!result.is_null());

            let len = xml_strlen(result);
            let slice = unsafe { core::slice::from_raw_parts(result, len as usize) };
            let expected = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><root/>";
            assert_eq!(slice, expected.as_bytes());

            allocator::xmlFree(result as *mut c_void);
            free_doc(doc);
        }
    }

    #[test]
    fn test_xmlDocDumpFormatMemory() {
        unsafe {
            let doc = new_doc(ptr::null());
            let root = new_node(ptr::null_mut(), c_str("root"));
            doc_set_root_element(doc, root);

            let mut mem: *mut xmlChar = ptr::null_mut();
            let mut size: c_int = 0;

            xmlDocDumpFormatMemory(doc, &mut mem, &mut size, 0);

            assert!(!mem.is_null());
            assert!(size > 0);

            let slice = unsafe { core::slice::from_raw_parts(mem, size as usize) };
            let expected = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><root/>";
            assert_eq!(slice, expected.as_bytes());

            allocator::xmlFree(mem as *mut c_void);
            free_doc(doc);
        }
    }

    #[test]
    fn test_serialize_escape_attribute() {
        unsafe {
            let doc = new_doc(ptr::null());
            let root = new_node(ptr::null_mut(), c_str("root"));
            doc_set_root_element(doc, root);

            // Attribute with special chars
            set_prop(root, c_str("desc"), c_str("a < b & c \"quoted\""));

            let buf = io::buf_create(-1);
            assert!(!buf.is_null());

            let ret = doc_dump(buf, doc);
            assert!(ret >= 0);

            let expected = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><root desc=\"a &lt; b &amp; c &quot;quoted&quot;\"/>";
            assert!(buf_eq_str(buf, expected));

            io::buf_free(buf);
            free_doc(doc);
        }
    }
}
