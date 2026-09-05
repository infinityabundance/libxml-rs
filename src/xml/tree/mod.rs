//! XML tree construction and manipulation (§17, §18, §85 Phase 1).
//!
//! Complete tree construction/manipulation, namespaces, attributes,
//! dictionaries, entity structures, document ownership, copying, linking,
//! and freeing.
//!
//! # UPSTREAM-PARITY
//!
//! The libxml2 tree is an observable data structure. The pointer topology
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
//!
//! # Upstream contract
//!
//! Mirrors upstream tree.c and buf.c (SRC-LIBXML2-2.15.0, oracle tree
//! `oracle/historical/src/libxml2-2.15.0/`). The tree is an observable data
//! structure: pointer topology (parent, children, last, next, prev, doc, ns,
//! properties, nsDef) is part of the compatibility contract and must be
//! court-tested. Parity target: the system libxml2 2.15.3 oracle.
//!
//! # Conceptual behavior
//!
//! Complete tree construction/manipulation: namespaces, attributes,
//! dictionaries, entity structures, document ownership, copying, linking and
//! freeing. Nodes are C-layout mirrors (tree.h); copy/link/free semantics
//! follow xmlCopyNode / xmlAddChild / xmlFreeNodeList / xmlFreeDoc.
//!
//! # Ownership & safety invariants
//!
//! Documents own all their nodes; freeing the document frees the subtree.
//! node->parent, node->doc, node->ns, node->next/prev are borrowed pointers —
//! never freed by the reader. Allocator domain: xmlMalloc, freed with xmlFree
//! (atlas/OWNERSHIP_ATLAS.md). SAFETY: the Rust mirrors enforce layout exactly
//! (`#[repr(C)]`); `_xmlElement` is 104 bytes upstream and must stay that size
//! (R-000139: a 56-byte mirror under-allocated every element declaration).
//!
//! # Historical quirks & epochs
//!
//! QUIRK-0002 / LORE-0006: namespace nodes have no parent — a long-standing
//! divergence upstream was aware of since the c14n fix commit 044fc6b7
//! (2002). E-004: entity-content text nodes became TEXT compact at 2.13.0
//! (commit 8d04f0ee). The 11.1-N structural alignment (R-000164) pinned
//! doc->children DTD placement, CDATA node names, standalone=-2 and the
//! attribute hash (name,prefix,elem) key order.
//!
//! # Deliberate oddities
//!
//! Deliberate oddities preserved for parity: an xmlns= declaration with an
//! empty value yields href pointing at an empty string (not NULL), parsed
//! attributes keep atype=0, the DTD node joins doc->children before the first
//! element, and xmlGetLineNo returns long with the upstream -1 walk for
//! non-element nodes (all R-000164).
//!
//! # Proving courts
//!
//! OWNERSHIP and TREE-STRUCTURE court families; TREE-001 (27-block structural
//! fingerprint of 20 corpus docs x 8 option variants, byte-identical), ASan
//! full-suite runs, and `cargo test --lib` (counts generated into
//! atlas/TEST_COUNTS.json by tools/evidence/test_counts.py). Receipts under
//! courts/receipts/phase-11.
//!
//! # Tempting simplifications that would break parity
//!
//! A tempting simplification is a nicer Rust node type instead of the exact
//! `_xmlNode` / `_xmlElement` mirrors — it would break the C ABI layout
//! (R-000139 class) and every C consumer reading fields at upstream offsets.
//! Do not auto-maintain parent pointers for namespace nodes (QUIRK-0002); do
//! not drop the last/next/prev links — TREE-001 fingerprints them.
//!
//! # Safety
//!
//! - The unsafe entry points in this module accept raw pointers that must be
//!   valid, correctly typed, and live for the duration of the call: `_xmlDoc`,
//!   `_xmlNode`, `_xmlAttr`, `_xmlNs`, `_xmlDtd`, `_xmlBuffer`, and
//!   NUL-terminated `xmlChar` strings. NULL is permitted only where an
//!   individual function's contract explicitly allows it.
//! - Tree links (`parent`, `children`, `last`, `next`, `prev`, `properties`,
//!   `nsDef`) must form a consistent, live tree; documents own their node
//!   subtrees, so callers must not free a node that still belongs to a live
//!   document.

use core::ffi::c_void;
use core::ptr;
use std::os::raw::{c_char, c_int, c_long, c_ulong};

use crate::abi::allocator;
use crate::abi::constants::*;
use crate::abi::structs::*;
use crate::abi::types::xmlCharEncoding::XML_CHAR_ENCODING_UTF8;
use crate::abi::types::xmlDocProperties::XML_DOC_USERBUILT;
use crate::abi::types::xmlElementType::*;
use crate::abi::types::*;
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
        let buf = unsafe { allocator::xmlMallocImpl(1) as *mut xmlChar };
        if !buf.is_null() {
            unsafe { *buf = 0 };
        }
        return buf;
    }
    let buf = unsafe { allocator::xmlMallocImpl(len + 1) as *mut xmlChar };
    if !buf.is_null() {
        unsafe {
            ptr::copy_nonoverlapping(str, buf, len + 1);
        }
    }
    buf
}

/// Copy an xmlChar string into an already-allocated buffer, or return NULL.
#[allow(dead_code)]
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
///
/// # SAFETY
///
///
/// - `str` must point to valid NUL-terminated
///   strings (or NULL where the C contract allows) for the lifetime
///   of the call.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
pub const unsafe fn xml_strlen(str: *const xmlChar) -> c_int {
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
        (*doc).properties = XML_DOC_USERBUILT as c_int;
        (*doc).compression = -1; // not initialized (upstream xmlNewDoc)
        (*doc).charset = XML_CHAR_ENCODING_UTF8 as c_int;

        // Set version
        let ver = if version.is_null() {
            XML_DEFAULT_VERSION.as_ptr() as *const xmlChar
        } else {
            version
        };
        (*doc).version = dup_xml_str(ver);
    }

    // UPSTREAM-PARITY (tree.c xmlNewDoc): the document node is registered.
    crate::abi::data_globals::register_node_hook(doc as *mut _xmlNode);

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

    // UPSTREAM-PARITY (tree.c xmlFreeDoc): the deregister hook fires before
    // the document is torn down.
    crate::abi::data_globals::deregister_node_hook(doc as *mut _xmlNode);

    let d = unsafe { &mut *doc };

    // UPSTREAM-PARITY (tree.c xmlFreeDoc): the ID/REF tables are freed
    // BEFORE the tree walk — attribute frees must not need ID-table lookups
    // during document teardown (xmlRemoveID on a NULL table is a no-op).
    if !d.ids.is_null() {
        crate::xml::validation::free_id_table(d.ids as *mut crate::xml::hash::HashTable);
        d.ids = ptr::null_mut();
    }

    // UPSTREAM-PARITY (tree.c xmlFreeDoc): the subset DTD nodes are unlinked
    // from the child list before the tree is freed (they may be part of
    // doc->children after a parse).
    let dict = d.dict;
    let mut ext_subset = d.extSubset;
    let int_subset = d.intSubset;
    if !ext_subset.is_null() && ext_subset == int_subset {
        ext_subset = ptr::null_mut();
    }
    if !ext_subset.is_null() {
        unlink_node_internal(ext_subset as *mut _xmlNode, d as *mut _xmlDoc);
        d.extSubset = ptr::null_mut();
        free_dtd(ext_subset);
    }
    if !int_subset.is_null() {
        unlink_node_internal(int_subset as *mut _xmlNode, d as *mut _xmlDoc);
        d.intSubset = ptr::null_mut();
        free_dtd(int_subset);
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
        allocator::xmlFreeImpl(d.version as *mut c_void);
    }
    if !d.encoding.is_null() {
        allocator::xmlFreeImpl(d.encoding as *mut c_void);
    }
    if !d.URL.is_null() {
        allocator::xmlFreeImpl(d.URL as *mut c_void);
    }

    // Free the document itself
    allocator::xmlFreeImpl(doc as *mut c_void);

    // UPSTREAM-PARITY: the document holds a reference on its dictionary.
    if !dict.is_null() {
        crate::abi::exports_xml2::xmlDictFree(dict);
    }
}

/// Rebind the `ns` pointer of one element (and each of its attributes) to a
/// namespace declaration owned by `new_doc`'s tree, declaring it on `top`
/// when nothing in scope matches the prefix (upstream tree.c
/// `xmlStaticCopyNode`: `xmlSearchNsSafe(ret, node->ns->prefix, &ns)` then
/// "search it in the original tree and add it at the top of the new tree").
///
/// # Safety
///
/// - `new_doc`/`el`/`top` must be valid tree pointers in the same document.
unsafe fn rebind_copied_ns(new_doc: *mut _xmlDoc, top: *mut _xmlNode, el: *mut _xmlNode) {
    unsafe {
        let bind = |n: *mut _xmlNode| {
            let ns = (*n).ns;
            if ns.is_null() {
                return;
            }
            let prefix = (*ns).prefix;
            let mut found = search_ns(new_doc, el, prefix);
            if found.is_null() {
                found = new_ns(top, (*ns).href, prefix);
            }
            if !found.is_null() {
                (*n).ns = found;
            }
        };
        if (*el).type_ == XML_ELEMENT_NODE as c_int {
            bind(el);
            let mut a = (*el).properties;
            while !a.is_null() {
                if !(*a).ns.is_null() && a as *mut _xmlNode != el {
                    // Attributes resolve their prefix against the element's
                    // in-scope declarations.
                    let prefix = (*(*a).ns).prefix;
                    let mut found = search_ns(new_doc, el, prefix);
                    if found.is_null() {
                        found = new_ns(top, (*(*a).ns).href, prefix);
                    }
                    if !found.is_null() {
                        (*a).ns = found;
                    }
                }
                a = (*a).next;
            }
        }
    }
}

/// Rebind every element/attribute namespace in a freshly deep-copied
/// document to declarations owned by the copy (upstream xmlStaticCopyNode
/// semantics for `xmlCopyDoc`). The generic copy keeps the source pointers
/// verbatim; without this pass the copied tree's namespace pointers dangle
/// as soon as the source document is freed.
///
/// # Safety
///
/// - `new_doc` must be the fresh copy; its tree must be fully built.
pub(crate) unsafe fn reconcile_copied_tree_ns(new_doc: *mut _xmlDoc) {
    if new_doc.is_null() {
        return;
    }
    unsafe {
        // The top element is where new declarations are attached (upstream
        // adds missing namespaces "at the top of the new tree").
        let mut top: *mut _xmlNode = ptr::null_mut();
        let mut c = (*new_doc).children;
        while !c.is_null() {
            if (*c).type_ == XML_ELEMENT_NODE as c_int {
                top = c;
                break;
            }
            c = (*c).next;
        }
        if top.is_null() {
            return;
        }
        // Pre-order walk: parents are processed before children, so a
        // declaration attached to the top element is found by descendants.
        let mut stack: Vec<*mut _xmlNode> = vec![top];
        while let Some(el) = stack.pop() {
            rebind_copied_ns(new_doc, top, el);
            // Push children in reverse so they pop in document order.
            let mut kids: Vec<*mut _xmlNode> = Vec::new();
            let mut ch = (*el).children;
            while !ch.is_null() {
                if (*ch).type_ == XML_ELEMENT_NODE as c_int {
                    kids.push(ch);
                }
                ch = (*ch).next;
            }
            for k in kids.into_iter().rev() {
                stack.push(k);
            }
        }
    }
}

/// Unlink a node from its parent's child list without freeing it
/// (upstream xmlUnlinkNodeInternal semantics; doc is used for ID/ref
/// bookkeeping which the candidate does not maintain on unlink).
unsafe fn unlink_node_internal(node: *mut _xmlNode, _doc: *mut _xmlDoc) {
    if node.is_null() {
        return;
    }
    let parent = (*node).parent;
    if parent.is_null() {
        return;
    }
    if (*node).prev.is_null() {
        (*parent).children = (*node).next;
    } else {
        (*(*node).prev).next = (*node).next;
    }
    if (*node).next.is_null() {
        (*parent).last = (*node).prev;
    } else {
        (*(*node).next).prev = (*node).prev;
    }
    (*node).next = ptr::null_mut();
    (*node).prev = ptr::null_mut();
    (*node).parent = ptr::null_mut();
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

        // UPSTREAM-PARITY (tree.c xmlCopyDoc + xmlStaticCopyNodeList): the
        // internal subset is copied FIRST (a plain DTD copy) and the SAME
        // node is then linked into the children list at the DTD child's
        // position — one shared DTD node serves both `intSubset` and the
        // DocumentType child (the copy is NOT duplicated). The generic node
        // copy must never be applied to a DTD child (its ExternalID/SystemID
        // live at offsets the node copy leaves untouched — the corrupt-clone
        // SEGV in DOMNode_isEqualNode / clone-of-doctype docs).
        if recursive != 0 {
            if !d.intSubset.is_null() {
                let dtd_copy = crate::xml::dtd::copy_dtd(d.intSubset);
                if !dtd_copy.is_null() {
                    (*new_doc).intSubset = dtd_copy;
                }
            }
        }

        if recursive != 0 && !d.children.is_null() {
            // DTD-aware children walk (upstream xmlStaticCopyNodeList): a DTD
            // child contributes the intSubset copy (created above, or a fresh
            // DTD copy when the source child is not the recorded subset) at
            // its original position; every other child is copied generically.
            let mut head: *mut _xmlNode = ptr::null_mut();
            let mut tail: *mut _xmlNode = ptr::null_mut();
            let mut cur = d.children;
            while !cur.is_null() {
                let ct = unsafe { (*cur).type_ };
                let copy: *mut _xmlNode = if ct == XML_DTD_NODE as c_int {
                    unsafe {
                        if (*new_doc).intSubset.is_null() {
                            let dc = crate::xml::dtd::copy_dtd(
                                cur as *const crate::abi::structs::_xmlDtd,
                            );
                            if dc.is_null() {
                                break;
                            }
                            (*new_doc).intSubset = dc;
                            dc as *mut _xmlNode
                        } else {
                            (*new_doc).intSubset as *mut _xmlNode
                        }
                    }
                } else {
                    copy_node(cur, recursive)
                };
                if copy.is_null() {
                    break;
                }
                unsafe {
                    if tail.is_null() {
                        head = copy;
                    } else {
                        (*tail).next = copy;
                        (*copy).prev = tail;
                    }
                    tail = copy;
                }
                cur = unsafe { (*cur).next };
            }
            (*new_doc).children = head;
            (*new_doc).last = tail;
            if !head.is_null() {
                // UPSTREAM-PARITY (tree.c xmlCopyDoc): every copied top-level
                // child keeps the new DOCUMENT node as its parent
                // (`xmlStaticCopyNodeList(doc->children, ret,
                // (xmlNodePtr)ret)`). The pre-fix NULL parent made PHP treat a
                // cloned document's root element as ownerless: its proxy
                // teardown (php_libxml_node_free_resource, `parent == NULL`
                // branch) freed the whole subtree while the cloned doc still
                // referenced it, so the doc teardown double-freed the root
                // (Phase 14.3 Bug-3 — DOMDocument clone + navigation).
                let mut child = head;
                while !child.is_null() {
                    (*child).parent = new_doc as *mut _xmlNode;
                    (*child).doc = new_doc;
                    child = (*child).next;
                }
                propagate_doc(head, new_doc);
                // UPSTREAM-PARITY (tree.c xmlStaticCopyNode): after a
                // CROSS-document deep copy every element/attribute namespace
                // pointer must reference namespace declarations owned by the
                // COPY (the generic copy keeps the original pointers
                // verbatim, which dangle once the source document is freed).
                // Resolve each against the copied tree and declare missing
                // namespaces on the top element — xmlCopyNode (same-doc)
                // keeps verbatim pointers, only xmlCopyDoc/xmlDocCopyNode
                // reconcile.
                reconcile_copied_tree_ns(new_doc);
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
    if doc.is_null() || root.is_null() || unsafe { (*root).type_ } == XML_NAMESPACE_DECL as c_int {
        return ptr::null_mut();
    }

    let old_root = doc_get_root_element(doc);
    if old_root == root {
        return old_root;
    }

    unsafe {
        // UPSTREAM-PARITY (tree.c xmlDocSetRootElement): unlink the node
        // from its current tree, move doc pointers, and set the parent to
        // the DOCUMENT NODE itself. A NULL parent here is observable: lxml
        // walks `parent` to decide whether a node is still in a document
        // (proxy.pxi getDeallocationTop) and would free an orphaned-looking
        // root directly, after which free_doc walks the doc children and
        // frees it again — a double free.
        if !(*root).parent.is_null() {
            unlink_node(root);
        }
        if (*root).doc != doc {
            propagate_doc(root, doc);
        }
        (*root).parent = doc as *mut _xmlNode;
        (*root).doc = doc;

        if old_root.is_null() {
            // No previous root element: append after the existing doc-level
            // nodes (PIs/comments may precede the root).
            if (*doc).children.is_null() {
                (*doc).children = root;
                (*doc).last = root;
                (*root).prev = ptr::null_mut();
                (*root).next = ptr::null_mut();
            } else {
                add_sibling((*doc).last, root);
            }
        } else {
            // Replace the old root in position (upstream xmlReplaceNode).
            if (*old_root).prev.is_null() {
                (*doc).children = root;
                (*root).prev = ptr::null_mut();
            } else {
                (*(*old_root).prev).next = root;
                (*root).prev = (*old_root).prev;
            }
            if (*old_root).next.is_null() {
                (*doc).last = root;
                (*root).next = ptr::null_mut();
            } else {
                (*(*old_root).next).prev = root;
                (*root).next = (*old_root).next;
            }
            (*old_root).parent = ptr::null_mut();
            (*old_root).prev = ptr::null_mut();
            (*old_root).next = ptr::null_mut();
        }
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
///
/// # Safety
///
/// - `doc` must be a valid pointer to an `_xmlDoc`, or NULL; when non-NULL it
///   is dereferenced and its `children` chain is walked.
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
///
/// # Safety
///
/// - `node` must be NULL or a valid pointer to an `_xmlNode`; it is forwarded
///   to `get_line_no_internal`, which walks the tree links.
pub fn get_line_no(node: *const _xmlNode) -> c_long {
    unsafe { get_line_no_internal(node, 0) }
}

/// UPSTREAM-PARITY (tree.c xmlGetLineNoInternal): element/text/comment/PI
/// nodes report their stored line; other node types (DTD nodes, entity
/// references, declarations, ...) walk to the nearest previous or ancestor
/// element-ish node and return -1 when none exists.
///
/// # Safety
///
/// - `node` must be NULL or a valid pointer to an `_xmlNode` in a consistent
///   tree: the `children`, `next`, `prev`, and `parent` links it follows must
///   themselves point to valid `_xmlNode` structs.
/// - `depth` bounds the recursion; callers start at 0.
unsafe fn get_line_no_internal(node: *const _xmlNode, depth: c_int) -> c_long {
    if depth >= 5 {
        return -1;
    }
    if node.is_null() {
        return -1;
    }
    let n = unsafe { &*node };
    if n.type_ == XML_ELEMENT_NODE as c_int
        || n.type_ == XML_TEXT_NODE as c_int
        || n.type_ == XML_COMMENT_NODE as c_int
        || n.type_ == XML_PI_NODE as c_int
    {
        if n.line == 65535 {
            // UPSTREAM-PARITY (tree.c xmlGetLineNoInternal): text nodes whose
            // real line exceeded USHRT_MAX at parse time store the line in
            // psvi (XML_PARSE_BIG_LINES / the PHP html5 lexbor bridge does
            // XML_INT_TO_PTR(line)); read it back (XML_PTR_TO_INT).
            if n.type_ == XML_TEXT_NODE as c_int && !n.psvi.is_null() {
                return n.psvi as usize as c_long;
            }
            if n.type_ == XML_ELEMENT_NODE as c_int && !n.children.is_null() {
                let r = unsafe { get_line_no_internal(n.children, depth + 1) };
                if r != -1 {
                    return r;
                }
            }
            if !n.next.is_null() {
                let r = unsafe { get_line_no_internal(n.next, depth + 1) };
                if r != -1 {
                    return r;
                }
            }
            if !n.prev.is_null() {
                let r = unsafe { get_line_no_internal(n.prev, depth + 1) };
                if r != -1 {
                    return r;
                }
            }
        }
        n.line as c_long
    } else if !n.prev.is_null()
        && (unsafe { (*n.prev).type_ } == XML_ELEMENT_NODE as c_int
            || unsafe { (*n.prev).type_ } == XML_TEXT_NODE as c_int
            || unsafe { (*n.prev).type_ } == XML_COMMENT_NODE as c_int
            || unsafe { (*n.prev).type_ } == XML_PI_NODE as c_int)
    {
        unsafe { get_line_no_internal(n.prev, depth + 1) }
    } else if !n.parent.is_null() && unsafe { (*n.parent).type_ } == XML_ELEMENT_NODE as c_int {
        unsafe { get_line_no_internal(n.parent, depth + 1) }
    } else {
        -1
    }
}

/// Get the content of a node, recursively concatenating child text.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlNodeGetContent(const xmlNode *cur);
/// ```
///
/// Oracle behavior (tree.c `xmlNodeGetContent`):
/// - For text/CDATA nodes: returns the content directly.
/// - For element nodes: recursively concatenates the string values of
///   children (text and CDATA; entity references are expanded via their
///   content when available).
/// - For attribute nodes: returns the attribute value (first child).
/// - For comments/PIs: returns the content field.
/// - For documents: returns content of the root element.
/// - Returns NULL on error, empty string for empty nodes.
///
/// Returns a newly allocated string; caller frees with `xmlFree`.
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
pub unsafe fn node_get_content(node: *mut _xmlNode) -> *mut xmlChar {
    if node.is_null() {
        return ptr::null_mut();
    }
    let typ = (*node).type_;
    let mut result: Vec<u8> = Vec::new();
    match typ {
        t if t == XML_TEXT_NODE as c_int
            || t == XML_CDATA_SECTION_NODE as c_int
            || t == XML_COMMENT_NODE as c_int
            || t == XML_PI_NODE as c_int =>
        {
            if !(*node).content.is_null() {
                let len = crate::abi::exports_xml2::xmlStrlen((*node).content);
                result
                    .extend_from_slice(core::slice::from_raw_parts((*node).content, len as usize));
            } else if t == XML_TEXT_NODE as c_int && !(*node).children.is_null() {
                // Non-compact text node (entity merge): content lives in the
                // child text nodes.
                let mut child = (*node).children;
                while !child.is_null() {
                    if !(*child).content.is_null() {
                        let len = crate::abi::exports_xml2::xmlStrlen((*child).content);
                        result.extend_from_slice(core::slice::from_raw_parts(
                            (*child).content,
                            len as usize,
                        ));
                    }
                    child = (*child).next;
                }
            }
        }
        t if t == XML_ATTRIBUTE_NODE as c_int => {
            // Attribute: the value is the concatenation of ALL children (text
            // runs plus expanded entity references) — upstream
            // xmlNodeGetContent(attr) walks the children via
            // xmlBufGetChildContent. A single text child is the common case
            // (php_libxml_attr_value fast path); values containing references
            // keep entity-REF children (`<root a="x&ent;x"/>` reads "xfoox"
            // while serialization round-trips `&ent;`).
            let mut child = (*node).children;
            while !child.is_null() {
                let ctype = (*child).type_;
                if ctype == XML_TEXT_NODE as c_int || ctype == XML_CDATA_SECTION_NODE as c_int {
                    if !(*child).content.is_null() {
                        let len = crate::abi::exports_xml2::xmlStrlen((*child).content);
                        result.extend_from_slice(core::slice::from_raw_parts(
                            (*child).content,
                            len as usize,
                        ));
                    }
                } else if ctype == XML_ENTITY_REF_NODE as c_int
                    || ctype == XML_ELEMENT_NODE as c_int
                {
                    let sub = node_get_content(child);
                    if !sub.is_null() {
                        let len = crate::abi::exports_xml2::xmlStrlen(sub);
                        result.extend_from_slice(core::slice::from_raw_parts(sub, len as usize));
                        allocator::xmlFreeImpl(sub as *mut c_void);
                    }
                }
                child = (*child).next;
            }
        }
        t if t == XML_ENTITY_REF_NODE as c_int => {
            // Entity reference: expand via the declaration its `children`
            // points at (xmlNewReference / the parser bind the entity decl
            // there), falling back to the document/predefined lookup.
            //
            // UPSTREAM-PARITY (tree.c xmlBufGetEntityRefContent): a
            // PREDEFINED entity contributes its `content`; any OTHER entity
            // contributes its CHILD content (the parsed replacement tree).
            // An internal entity declaration created from `<!ENTITY test
            // "...">` carries only `content` and NO child tree, so a
            // reference to it reads as "" — php delayed_freeing/
            // entity_reference expects exactly this (and
            // `new DOMEntityReference("amp")` reads "&").
            let mut ent = if (*node).children.is_null() {
                ptr::null_mut()
            } else {
                (*node).children as *mut _xmlEntity
            };
            if ent.is_null() {
                let name = (*node).name;
                if !name.is_null() {
                    ent = crate::xml::tree::get_doc_entity((*node).doc, name);
                }
            }
            if !ent.is_null() {
                let is_predef = (*ent).etype
                    == crate::abi::types::xmlEntityType::XML_INTERNAL_PREDEFINED_ENTITY as c_int;
                if is_predef {
                    if !(*ent).content.is_null() {
                        let len = crate::abi::exports_xml2::xmlStrlen((*ent).content);
                        result.extend_from_slice(core::slice::from_raw_parts(
                            (*ent).content,
                            len as usize,
                        ));
                    }
                } else if (*ent).flags & (1 << 3) == 0 {
                    // UPSTREAM-PARITY (tree.c xmlBufGetEntityRefContent): the
                    // XML_ENT_EXPANDING flag (candidate bit 1 << 3, parser
                    // convention) breaks self-referential loops while the
                    // declaration's replacement tree is walked.
                    (*ent).flags |= 1 << 3;
                    let mut child = (*ent).children;
                    while !child.is_null() {
                        let sub = node_get_content(child);
                        if !sub.is_null() {
                            let len = crate::abi::exports_xml2::xmlStrlen(sub);
                            result
                                .extend_from_slice(core::slice::from_raw_parts(sub, len as usize));
                            allocator::xmlFreeImpl(sub as *mut c_void);
                        }
                        child = (*child).next;
                    }
                    (*ent).flags &= !(1 << 3);
                }
            }
        }
        t if t == XML_DOCUMENT_NODE as c_int || t == XML_HTML_DOCUMENT_NODE as c_int => {
            let root = doc_get_root_element(node as *mut _xmlDoc);
            if !root.is_null() {
                let sub = node_get_content(root);
                if !sub.is_null() {
                    let len = crate::abi::exports_xml2::xmlStrlen(sub);
                    result.extend_from_slice(core::slice::from_raw_parts(sub, len as usize));
                    allocator::xmlFreeImpl(sub as *mut c_void);
                }
            }
        }
        t if t == XML_NAMESPACE_DECL as c_int => {
            // XPath namespace node (an independent `_xmlNs` copy cast to
            // `_xmlNode`, xmlXPathNodeSetDupNs semantics): its string-value is
            // the namespace URI. The copy has NO tree links — the fields that
            // _xmlNode accessors read (children at the `prefix` offset, etc.)
            // are ns data, so this must not fall into the element arm.
            // php's DOMXPath php:functionString relies on this conversion
            // (xmlXPathCastToString of a namespace node-set).
            let ns = node as *mut crate::abi::structs::_xmlNs;
            if !(*ns).href.is_null() {
                let len = crate::abi::exports_xml2::xmlStrlen((*ns).href);
                result.extend_from_slice(core::slice::from_raw_parts((*ns).href, len as usize));
            }
        }
        _ => {
            // Element and everything else: concatenate descendant text
            // content (XPath 1.0 string-value semantics — §4.2 / tree.c
            // xmlNodeGetContent, which walks the full subtree, not just
            // direct text children).
            let mut child = (*node).children;
            while !child.is_null() {
                let ctype = (*child).type_;
                if ctype == XML_TEXT_NODE as c_int || ctype == XML_CDATA_SECTION_NODE as c_int {
                    if !(*child).content.is_null() {
                        let len = crate::abi::exports_xml2::xmlStrlen((*child).content);
                        result.extend_from_slice(core::slice::from_raw_parts(
                            (*child).content,
                            len as usize,
                        ));
                    }
                } else if ctype == XML_ENTITY_REF_NODE as c_int
                    || ctype == XML_ELEMENT_NODE as c_int
                {
                    let sub = node_get_content(child);
                    if !sub.is_null() {
                        let len = crate::abi::exports_xml2::xmlStrlen(sub);
                        result.extend_from_slice(core::slice::from_raw_parts(sub, len as usize));
                        allocator::xmlFreeImpl(sub as *mut c_void);
                    }
                }
                child = (*child).next;
            }
        }
    }
    // Allocate the C string.
    let buf = allocator::xmlMallocImpl(result.len() + 1) as *mut xmlChar;
    if buf.is_null() {
        return ptr::null_mut();
    }
    if !result.is_empty() {
        ptr::copy_nonoverlapping(result.as_ptr(), buf, result.len());
    }
    *buf.add(result.len()) = 0;
    buf
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
/// - `name` must be a valid null-terminated string or NULL (NULL returns
///   NULL — upstream tree.c `xmlNewNode` rejects a NULL name up front,
///   HOSTILE-ABI A48).
pub unsafe fn new_node(ns: *mut _xmlNs, name: *const xmlChar) -> *mut _xmlNode {
    if name.is_null() {
        return ptr::null_mut();
    }
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

    // UPSTREAM-PARITY (tree.c): the node-registration hook fires after a
    // node is fully initialised.
    crate::abi::data_globals::register_node_hook(node);

    node
}

/// Create a new XML element node whose name is BORROWED (not duplicated).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNodePtr xmlNewDocNodeEatName(xmlDocPtr doc, xmlNsPtr ns,
///                                 const xmlChar *name, const xmlChar *content);
/// ```
///
/// The name pointer is stored as-is; the caller keeps ownership. The XML
/// parser uses this when `dictNames` is enabled: the name is an interned
/// dictionary string owned by the document dictionary, and consumers (lxml
/// objectify `_tagMatches`) rely on node names being pointer-identical to
/// `xmlDictLookup`/`xmlDictExists` results. `free_node` consults
/// `xmlDictOwns` (UPSTREAM-PARITY `DICT_FREE`) so borrowed dictionary names
/// are never freed.
///
/// # SAFETY
///
/// - `ns` may be NULL.
/// - `name` must be non-NULL and remain valid for the lifetime of the node.
pub unsafe fn new_node_eat_name(ns: *mut _xmlNs, name: *const xmlChar) -> *mut _xmlNode {
    if name.is_null() {
        return ptr::null_mut();
    }
    let node = allocator::xmlMallocZero(size_of::<_xmlNode>() as usize) as *mut _xmlNode;
    if node.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        (*node).type_ = XML_ELEMENT_NODE as c_int;
        (*node).name = name as *mut xmlChar;
        (*node).ns = ns;
        (*node).line = 0;
        (*node).extra = 0;

        if !ns.is_null() {
            (*ns).context = node as *mut _xmlDoc;
        }
    }

    // UPSTREAM-PARITY (tree.c): the node-registration hook fires after a
    // node is fully initialised.
    crate::abi::data_globals::register_node_hook(node);

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

    // UPSTREAM-PARITY (tree.c xmlFreeNode): declaration nodes and namespace
    // declarations are routed to their dedicated free functions (their
    // struct layouts diverge from _xmlNode).
    if n.type_ == XML_DTD_NODE as c_int {
        free_dtd(node as *mut _xmlDtd);
        return;
    } else if n.type_ == XML_NAMESPACE_DECL as c_int {
        free_ns(node as *mut _xmlNs);
        return;
    } else if n.type_ == XML_ATTRIBUTE_NODE as c_int {
        free_prop(node as *mut _xmlAttr);
        return;
    } else if n.type_ == XML_ELEMENT_DECL as c_int {
        crate::xml::dtd::free_element(node as *mut _xmlElement);
        return;
    } else if n.type_ == XML_ATTRIBUTE_DECL as c_int {
        crate::xml::dtd::free_attribute(node as *mut _xmlAttribute);
        return;
    } else if n.type_ == XML_ENTITY_DECL as c_int {
        crate::xml::entities::free_entity(node as *mut _xmlEntity);
        return;
    }

    // UPSTREAM-PARITY (tree.c xmlFreeNode): the deregister hook fires before
    // the node is torn down.
    crate::abi::data_globals::deregister_node_hook(node);

    // Free properties and namespace declarations. Only element nodes carry
    // them; compact text nodes store their inline content at the address of
    // the `properties` field (and the following `nsDef` field), so touching
    // these for other node types would read text bytes as pointers.
    let is_element = n.type_ == XML_ELEMENT_NODE as c_int;
    if is_element && !n.properties.is_null() {
        free_prop_list(n.properties);
    }

    if is_element && !n.nsDef.is_null() {
        free_ns_list(n.nsDef);
    }

    // UPSTREAM-PARITY (tree.c xmlFreeNode + DICT_FREE): node names and
    // content may live in the document dictionary — consumers (lxml
    // `_fixHtmlDictNodeNames`) intern HTML element/attribute names with
    // xmlDictLookup, so the same interned pointer is shared by every node
    // with that name. Dict-owned strings must NOT be freed here; the guard
    // is: free iff there is no dict, or the dict does not own the string.
    let dict = if n.doc.is_null() {
        ptr::null_mut()
    } else {
        unsafe { (*n.doc).dict }
    };

    // Free the name.
    // UPSTREAM-PARITY (tree.c xmlNewText / xmlNewComment + xmlCopyNode): text,
    // CDATA, comment and PI nodes store their `name` as one of the SHARED static
    // markers xmlStringText / xmlStringTextNoenc / xmlStringComment (never
    // dict- or heap-owned) and those must never be freed here — freeing one
    // aborts with "free(): invalid pointer" at teardown (PHP modern/spec
    // Node_isDefaultNamespace was the trigger). The only non-dict node names the
    // candidate attaches are these statics, so pointer-equality guards fully
    // close the leak/double-free.
    if !n.name.is_null() && !crate::abi::exports_hash::dict_owns_str(dict, n.name) {
        let sentinels = [
            crate::abi::data_globals::xmlStringText.as_ptr() as *const c_void,
            crate::abi::data_globals::xmlStringTextNoenc.as_ptr() as *const c_void,
            crate::abi::data_globals::xmlStringComment.as_ptr() as *const c_void,
        ];
        let is_sentinel = sentinels.iter().any(|&p| p == n.name as *const c_void);
        if !is_sentinel {
            allocator::xmlFreeImpl(n.name as *mut c_void);
        }
    }

    // Free content (for text/CDATA nodes). Compact text content lives inside
    // the node struct (at the `properties` field address) and must not be
    // freed separately. UPSTREAM-PARITY: entity-reference content is shared
    // with the entity declaration and must not be freed here.
    if !n.content.is_null() {
        let node_type = n.type_;
        if node_type == XML_TEXT_NODE as c_int
            || node_type == XML_CDATA_SECTION_NODE as c_int
            || node_type == XML_COMMENT_NODE as c_int
            || node_type == XML_PI_NODE as c_int
        {
            let inline_addr = std::ptr::addr_of_mut!((*node).properties) as *const c_void;
            if n.content as *const c_void != inline_addr
                && !crate::abi::exports_hash::dict_owns_str(dict, n.content)
            {
                allocator::xmlFreeImpl(n.content as *mut c_void);
            }
        }
    }

    allocator::xmlFreeImpl(node as *mut c_void);
}

/// Free a linked list of nodes.
///
/// Frees all nodes in the list and their children (depth-first, children
/// before parents) WITHOUT C recursion — upstream xmlFreeNodeList walks the
/// tree with an explicit depth counter (tree.c 2.15), and a 100k-deep
/// document must tear down without overflowing the stack (GH-22570: the
/// recursive version segv'd at php shutdown on the deep Dom\XMLDocument).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlFreeNodeList(xmlNodePtr node);
/// ```
///
/// Does NOT descend into `XML_ENTITY_REF_NODE` children (their child list
/// points at the shared entity declaration, owned by the DTD) nor free
/// `XML_DTD_NODE` children (a DTD node in the list is unlinked, not freed —
/// xmlFreeDtd owns the subset teardown).
///
/// # SAFETY
///
/// - `node` must be a valid pointer to an _xmlNode, or NULL.
pub unsafe fn free_node_list(mut cur: *mut _xmlNode) {
    // Explicit resume frames replace the old recursion: each frame is a node
    // whose children-subtree free is in progress, plus the node's next
    // sibling read before the descent (the struct is freed on resume, so the
    // `next` pointer must not be read afterwards).
    let mut frames: Vec<(*mut _xmlNode, *mut _xmlNode)> = Vec::new();
    loop {
        // Process the current sibling list; descend into the first node that
        // carries a freeable children subtree (post-order: children first).
        while !cur.is_null() {
            let next = unsafe { (*cur).next };
            let t = unsafe { (*cur).type_ };

            if t == XML_DTD_NODE as c_int {
                // UPSTREAM-PARITY: DTD nodes are unlinked but not freed here.
                unsafe {
                    (*cur).prev = ptr::null_mut();
                    (*cur).next = ptr::null_mut();
                }
                cur = next;
                continue;
            }

            // Free children first (entity-ref children are shared with the
            // entity declaration and are owned by the DTD).
            if t != XML_ENTITY_REF_NODE as c_int && !unsafe { (*cur).children }.is_null() {
                frames.push((cur, next));
                cur = unsafe { (*cur).children };
                continue;
            }

            free_node(cur);
            cur = next;
        }
        // Sibling list exhausted: resume the innermost descended parent.
        let Some((parent, next)) = frames.pop() else {
            break;
        };
        free_node(parent);
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
        free_prop(cur);
        cur = next;
    }
}

/// Free a single attribute (upstream `xmlFreeProp`).
///
/// # SAFETY
///
/// - `prop` must be a valid pointer to an _xmlAttr, or NULL.
unsafe fn free_prop(prop: *mut _xmlAttr) {
    if prop.is_null() {
        return;
    }

    // UPSTREAM-PARITY (tree.c xmlFreeProp): freeing an ID attribute removes
    // its entry from the document's ID table (xmlRemoveID), so xmlGetID stops
    // reporting it. A NULL doc->ids (document teardown) is a no-op.
    if !unsafe { (*prop).doc }.is_null() && !unsafe { (*prop).id }.is_null() {
        crate::xml::validation::remove_id(unsafe { (*prop).doc }, prop);
    }

    // Free children (text nodes with value)
    if !unsafe { (*prop).children }.is_null() {
        free_node_list(unsafe { (*prop).children });
    }

    // UPSTREAM-PARITY (tree.c xmlFreeProp + DICT_FREE): attribute names may
    // be dict-interned (lxml `_fixHtmlDictNodeNames`); interned names are
    // shared and must not be freed here.
    let dict = if unsafe { (*prop).doc }.is_null() {
        ptr::null_mut()
    } else {
        unsafe { (*(*prop).doc).dict }
    };

    // Free name
    if !unsafe { (*prop).name }.is_null()
        && !crate::abi::exports_hash::dict_owns_str(dict, unsafe { (*prop).name })
    {
        allocator::xmlFreeImpl(unsafe { (*prop).name } as *mut c_void);
    }

    allocator::xmlFreeImpl(prop as *mut c_void);
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
        free_ns(cur);
        cur = next;
    }
}

/// Free a single namespace declaration (upstream `xmlFreeNs`).
///
/// # SAFETY
///
/// - `ns` must be a valid pointer to an _xmlNs, or NULL.
unsafe fn free_ns(ns: *mut _xmlNs) {
    if ns.is_null() {
        return;
    }

    // Free href and prefix
    if !unsafe { (*ns).href }.is_null() {
        allocator::xmlFreeImpl(unsafe { (*ns).href } as *mut c_void);
    }
    if !unsafe { (*ns).prefix }.is_null() {
        allocator::xmlFreeImpl(unsafe { (*ns).prefix } as *mut c_void);
    }

    allocator::xmlFreeImpl(ns as *mut c_void);
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
        // UPSTREAM-PARITY (tree.c xmlStaticCopyNode): the line number is
        // copied for element nodes only; text/CDATA/comment/PI copies keep
        // line 0.
        if n.type_ == XML_ELEMENT_NODE as c_int {
            (*new_node).line = n.line;
        }
        (*new_node).extra = n.extra;
        (*new_node).psvi = n.psvi;
        // UPSTREAM-PARITY (tree.c xmlStaticCopyNode): _private is NOT copied
        // — the copy is a fresh zeroed node (memset 0). Consumers (PHP's
        // php_libxml_* wrappers) key their registrations on node->_private:
        // a copied node must look UNREGISTERED so the clone gets its own
        // wrapper and document binding (ext/simplexml bug63575 — the cloned
        // document's nodes inherited the original's php wrappers, so XPath
        // and mutations resolved into the ORIGINAL document).

        // Copy namespace pointer (NOT the ns declaration — just the reference)
        (*new_node).ns = n.ns;

        // Copy namespace declarations (element nodes only; compact text nodes
        // store inline content over the `properties`/`nsDef` fields).
        let is_element = n.type_ == XML_ELEMENT_NODE as c_int;
        if is_element && !n.nsDef.is_null() {
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

        // Copy properties (element nodes only).
        if is_element && !n.properties.is_null() {
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

        // Copy children if recursive (each copied child gets its parent and
        // document pointers; `last` follows the upstream link order).
        if recursive != 0 && !n.children.is_null() {
            (*new_node).children = copy_node_list(n.children, recursive);
            if !(*new_node).children.is_null() {
                let mut child = (*new_node).children;
                let mut last_child = child;
                while !child.is_null() {
                    (*child).parent = new_node;
                    (*child).doc = (*new_node).doc;
                    propagate_doc(child, (*new_node).doc);
                    if (*child).next.is_null() {
                        last_child = child;
                    }
                    child = (*child).next;
                }
                (*new_node).last = last_child;
            }
        }
    }

    new_node
}

/// Copy a linked list of nodes.
///
/// Returns the first node of the new list, or NULL on failure.
///
/// # Safety
///
/// - `node` must be NULL or a valid pointer to an `_xmlNode`; when non-NULL it
///   is copied with `copy_node` and its `next` chain is walked, so every node
///   reachable through `next` must be valid and alive.
/// - `recursive` is forwarded to `copy_node` and selects deep versus shallow
///   copy; deep copies require valid `children` subtrees.
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
///
/// # Safety
///
/// - `ns` must be NULL or a valid pointer to an `_xmlNs`; when non-NULL its
///   fields are read and its `next` chain is walked, so every reachable
///   `_xmlNs` must be valid and alive.
/// - `href` and `prefix` of each entry may be NULL or NUL-terminated `xmlChar`
///   strings; `dup_xml_str` reads them as C strings.
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
        // UPSTREAM-PARITY (tree.c xmlCopyNamespaceList): _private is NOT
        // copied (PHP tags some namespace declarations through ns->_private;
        // a copied declaration must not alias the original's registration).
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
            (*prev).next = new_cur;
        }
        prev = new_cur;
        cur = c.next;
    }

    new_ns
}

/// Copy a linked list of properties.
///
/// # Safety
///
/// - `prop` must be NULL or a valid pointer to an `_xmlAttr`; its fields are
///   read and its `next` chain is walked, so every reachable `_xmlAttr` must
///   be valid and alive.
/// - `children` of each attribute must be NULL or a valid node list; it is
///   copied recursively via `copy_node_list`. `name` may be NULL or a
///   NUL-terminated `xmlChar` string read by `dup_xml_str`.
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
///
/// # Safety
///
/// - `node` must be NULL or a valid pointer to an `_xmlNode`; the function
///   walks the whole subtree through `properties`, `children`, and `next`
///   links, so every reachable node and attribute must be a valid, live
///   struct.
/// - `doc` may be NULL or a valid pointer to an `_xmlDoc`; it is only stored
///   into `doc` fields, never dereferenced.
unsafe fn propagate_doc(node: *mut _xmlNode, doc: *mut _xmlDoc) {
    let mut cur = node;
    while !cur.is_null() {
        unsafe {
            // UPSTREAM-PARITY (tree.c xmlSetTreeDoc/xmlNodeSetDoc): a node
            // whose document actually changes must move its dict-owned
            // name/content into the destination document's dictionary (or
            // heap copies) and drop its ID-table entry, otherwise the source
            // doc's teardown frees strings the moved subtree still points at
            // (double free). No-op delegation when the doc pointer already
            // matches.
            if (*cur).doc != doc {
                crate::abi::exports_tree::node_set_doc_impl(cur, doc);
            }

            // Propagate to properties (element nodes only; other node types
            // never carry properties, and compact text nodes store inline
            // content at the `properties` field address).
            if (*cur).type_ == XML_ELEMENT_NODE as c_int {
                let mut prop = (*cur).properties;
                while !prop.is_null() {
                    if (*prop).doc != doc {
                        crate::abi::exports_tree::node_set_doc_impl(prop as *mut _xmlNode, doc);
                    }
                    if !(*prop).children.is_null() {
                        propagate_doc((*prop).children, doc);
                    }
                    prop = (*prop).next;
                }
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

    // UPSTREAM-PARITY (tree.c xmlUnlinkNode): an unlinked DTD node is
    // detached from the document's internal/external subset pointers so a
    // later xmlFreeDoc doesn't free it again (nokogiri pins an unlinked DTD
    // and frees it via xmlFreeDtd, then frees the doc — without clearing
    // doc->intSubset the doc double-frees the DTD).
    let doc = n.doc;
    if n.type_ == XML_DTD_NODE as c_int && !doc.is_null() {
        if (*doc).intSubset == node as *mut _xmlDtd {
            (*doc).intSubset = ptr::null_mut();
        }
        if (*doc).extSubset == node as *mut _xmlDtd {
            (*doc).extSubset = ptr::null_mut();
        }
    }

    // Fix up prev/next chain
    let prev = n.prev;
    let next = n.next;

    if !prev.is_null() {
        unsafe { (*prev).next = next };
    }
    if !next.is_null() {
        unsafe { (*next).prev = prev };
    }

    // Fix up parent's children/last pointers. UPSTREAM-PARITY
    // (tree.c xmlUnlinkNodeInternal): an attribute node is tracked by the
    // parent element's `properties` chain, not by children/last — when the
    // unlinked node is an attribute, advance the `properties` head past it so
    // the parent no longer references a (possibly freed) attribute. Without
    // this, nokogiri unlinks an attribute and later frees it while the element
    // still hangs off the stale `properties` slot, and an XPath attribute-axis
    // walk dereferences freed memory.
    let parent = n.parent;
    if !parent.is_null() {
        if n.type_ == XML_ATTRIBUTE_NODE as c_int {
            if unsafe { (*parent).properties } == node as *mut _xmlAttr {
                unsafe { (*parent).properties = next as *mut _xmlAttr };
            }
        } else {
            if unsafe { (*parent).children } == node {
                unsafe { (*parent).children = next };
            }
            if unsafe { (*parent).last } == node {
                unsafe { (*parent).last = prev };
            }
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

    // UPSTREAM-PARITY (tree.c xmlAddChild): attaching an ATTRIBUTE node routes
    // it into the parent element's PROPERTIES list, not its children list
    // (lxml/PHP `element->setAttributeNode(attr)` calls xmlAddChild(elem, attr)
    // literally). Without this branch the attribute was appended to `children`
    // and then serialized as a bogus child text node / doubly freed on teardown.
    // The attribute keeps its own name (already set by xmlNewProp) and is
    // appended after the existing properties, mirroring how set_prop links new
    // attributes so serialization, clone, and free all treat it as a real attr.
    if c.type_ == XML_ATTRIBUTE_NODE as c_int {
        c.parent = parent;
        c.prev = ptr::null_mut();
        c.next = ptr::null_mut();
        if p.properties.is_null() {
            p.properties = cur as *mut crate::abi::structs::_xmlAttr;
        } else {
            let mut last = p.properties;
            while !unsafe { (*last).next }.is_null() {
                last = unsafe { (*last).next };
            }
            unsafe { (*last).next = cur as *mut crate::abi::structs::_xmlAttr };
            c.prev = last as *mut _xmlNode;
        }
        // Re-parent into the element's document so the attribute and its text
        // value share the owner element's doc (propagate_doc also descends into
        // attribute text children).
        if !p.doc.is_null() && c.doc != p.doc {
            propagate_doc(cur, p.doc);
        }
        return cur;
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
    if !doc.is_null() && parent.is_null() && unsafe { (*doc).children } == cur {
        unsafe { (*doc).children = elem };
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
            let empty = allocator::xmlMallocImpl(1) as *mut xmlChar;
            if !empty.is_null() {
                *empty = 0;
            }
            empty
        } else {
            dup_xml_str(content)
        };
        (*node).line = 0;
    }

    // UPSTREAM-PARITY (tree.c): the node-registration hook fires after a
    // node is fully initialised.
    crate::abi::data_globals::register_node_hook(node);

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

    // UPSTREAM-PARITY (tree.c): the node-registration hook fires after a
    // node is fully initialised.
    crate::abi::data_globals::register_node_hook(node);

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
    // UPSTREAM-PARITY (tree.c xmlNewPI): a NULL target name is rejected up
    // front — HOSTILE-ABI A46.
    if name.is_null() {
        return ptr::null_mut();
    }
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

    // UPSTREAM-PARITY (tree.c): the node-registration hook fires after a
    // node is fully initialised.
    crate::abi::data_globals::register_node_hook(node);

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
        // UPSTREAM-PARITY (tree.c xmlNewCDataBlock): the name field is left
        // NULL (zero-initialised).
        (*node).doc = doc;

        if !content.is_null() && len > 0 {
            (*node).content = allocator::xmlMallocImpl((len + 1) as usize) as *mut xmlChar;
            if !(*node).content.is_null() {
                ptr::copy_nonoverlapping(content, (*node).content, len as usize);
                *((*node).content.add(len as usize)) = 0;
            }
        } else {
            let empty = allocator::xmlMallocImpl(1) as *mut xmlChar;
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

        // UPSTREAM-PARITY (tree.c xmlNewNs): a node may not declare the
        // same prefix twice — when an existing declaration on the node's
        // OWN nsDef chain shares the prefix and has a non-NULL href the new
        // declaration is rejected (freed, NULL returned). PHP's
        // setAttributeNS/createAttributeNS conflict resolution relies on
        // this NULL to allocate a FRESH prefix instead (dom_get_ns_unchecked
        // -> dom_get_ns_resolve_prefix_conflict: xmlns:default,
        // xmlns:default1, ...) — createAttributeNS_prefix_conflicts /
        // Element_setAttributeNS.
        if !node.is_null() {
            let n = &mut *node;
            let same_prefix = |a: *mut _xmlNs| {
                let other = &*a;
                // xmlStrEqual semantics: NULL == NULL (two default
                // declarations conflict), otherwise byte equality.
                match ((*ns).prefix, other.prefix) {
                    (x, y) if x == y => true,
                    (x, y) if x.is_null() || y.is_null() => false,
                    (x, y) => crate::abi::exports_xml2::xmlStrEqual(x, y) != 0,
                }
            };
            let conflict = |a: *mut _xmlNs| -> bool {
                let other = &*a;
                same_prefix(a) && !other.href.is_null()
            };
            if n.nsDef.is_null() {
                n.nsDef = ns;
            } else {
                // Mirror upstream's first-element check then the walk.
                let mut prev = n.nsDef;
                if conflict(prev) {
                    free_ns(ns);
                    return ptr::null_mut();
                }
                while !(*prev).next.is_null() {
                    prev = (*prev).next;
                    if conflict(prev) {
                        free_ns(ns);
                        return ptr::null_mut();
                    }
                }
                (*prev).next = ns;
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
pub unsafe fn get_ns_list(_doc: *mut _xmlDoc, node: *mut _xmlNode) -> *mut *mut _xmlNs {
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
    let arr = allocator::xmlMallocImpl((ns_ptrs.len() + 1) * size_of::<*mut _xmlNs>())
        as *mut *mut _xmlNs;
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
/// `IS_STR_XML` from upstream tree.c: the string is exactly "xml".
fn is_str_xml(s: *const xmlChar) -> bool {
    if s.is_null() {
        return false;
    }
    let b = unsafe { core::slice::from_raw_parts(s, 3) };
    b[0] == b'x' && b[1] == b'm' && b[2] == b'l'
}

/// UPSTREAM-PARITY (tree.c xmlNsInScope): walk from `node` up to (excluding)
/// `ancestor`, checking that no closer declaration binds the same `prefix`
/// (NULL prefix = the default namespace). Returns 1 when `ancestor`'s decl is
/// still in scope, 0 when it is shadowed, -1 when the walk cannot reach
/// `ancestor` or crosses an entity boundary.
unsafe fn ns_in_scope(
    node: *mut _xmlNode,
    ancestor: *mut _xmlNode,
    prefix: *const xmlChar,
) -> c_int {
    let mut cur = node;
    while !cur.is_null() && cur != ancestor {
        let t = unsafe { (*cur).type_ };
        if t == XML_ENTITY_REF_NODE as c_int || t == XML_ENTITY_DECL as c_int {
            return -1;
        }
        if t == XML_ELEMENT_NODE as c_int {
            let mut tst = unsafe { (*cur).nsDef };
            while !tst.is_null() {
                let ns = unsafe { &*tst };
                if ns.prefix.is_null() && prefix.is_null() {
                    return 0;
                }
                if !ns.prefix.is_null()
                    && !prefix.is_null()
                    && unsafe { crate::abi::exports_xml2::xmlStrEqual(ns.prefix, prefix) != 0 }
                {
                    return 0;
                }
                tst = unsafe { (*tst).next };
            }
        }
        cur = unsafe { (*cur).parent };
    }
    if cur != ancestor {
        return -1;
    }
    1
}

/// Ensure `doc->oldNs` holds the implicit XML namespace declaration (upstream
/// xmlTreeEnsureXMLDecl). The engine models `doc->oldNs` as a list whose HEAD is
/// always the xml declaration; parked/retired declarations live after it.
unsafe fn ensure_doc_xml_ns(doc: *mut _xmlDoc) -> *mut _xmlNs {
    if doc.is_null() {
        return ptr::null_mut();
    }
    if !(*doc).oldNs.is_null() {
        return (*doc).oldNs;
    }
    let ns = allocator::xmlMallocZero(size_of::<_xmlNs>()) as *mut _xmlNs;
    if ns.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        (*ns).type_ = XML_LOCAL_NAMESPACE as c_int;
        (*ns).href = crate::xml::string::xml_strdup(
            c"http://www.w3.org/XML/1998/namespace".as_ptr() as *const xmlChar,
        );
        (*ns).prefix = crate::xml::string::xml_strdup(c"xml".as_ptr() as *const xmlChar);
        (*doc).oldNs = ns;
    }
    ns
}

/// UPSTREAM-PARITY (tree.c xmlSearchNsSafe): search for a namespace bound to
/// the given PREFIX in scope of `node`. A NULL `name_space` searches the
/// default namespace. The walk only ever reads ELEMENT nodes' `nsDef` chains
/// (the document is NOT an element: its `oldNs` list must never be mistaken for
/// declarations), and a declaration with a NULL href does not bind its prefix.
pub unsafe fn search_ns(
    doc: *mut _xmlDoc,
    node: *mut _xmlNode,
    name_space: *const xmlChar,
) -> *mut _xmlNs {
    if node.is_null() || unsafe { (*node).type_ } == XML_NAMESPACE_DECL as c_int {
        return ptr::null_mut();
    }
    let orig = node;

    // The XML-1.0 namespace is implicitly bound to the prefix "xml" on every
    // document (xmlTreeEnsureXMLDecl keeps it on doc->oldNs).
    if !doc.is_null() && is_str_xml(name_space) {
        return unsafe { ensure_doc_xml_ns(doc) };
    }

    // Climb from a non-element node to its owning element, if any.
    let mut cur = node;
    while unsafe { (*cur).type_ } != XML_ELEMENT_NODE as c_int {
        cur = unsafe { (*cur).parent };
        if cur.is_null() {
            return ptr::null_mut();
        }
    }
    let parent = cur;

    // UPSTREAM-PARITY: `while ((node != NULL) && (node->type ==
    // XML_ELEMENT_NODE))` — a detached element (parent == NULL) terminates the
    // walk instead of dereferencing NULL.
    while !cur.is_null() && unsafe { (*cur).type_ } == XML_ELEMENT_NODE as c_int {
        let mut ns_def = unsafe { (*cur).nsDef };
        while !ns_def.is_null() {
            let ns = unsafe { &*ns_def };
            if unsafe { crate::abi::exports_xml2::xmlStrEqual(ns.prefix, name_space) != 0 }
                && !ns.href.is_null()
            {
                return ns_def;
            }
            ns_def = unsafe { (*ns_def).next };
        }
        if orig != cur {
            let el_ns = unsafe { (*cur).ns };
            if !el_ns.is_null()
                && unsafe {
                    crate::abi::exports_xml2::xmlStrEqual((*el_ns).prefix, name_space) != 0
                }
                && !(*el_ns).href.is_null()
            {
                return el_ns;
            }
        }
        cur = unsafe { (*cur).parent };
    }

    // No document but the node belongs to a doc-less tree: exceptionally create
    // the xml declaration on the nearest element (upstream tree.c).
    if doc.is_null() && is_str_xml(name_space) {
        let ns = allocator::xmlMallocZero(size_of::<_xmlNs>()) as *mut _xmlNs;
        if !ns.is_null() {
            unsafe {
                (*ns).type_ = XML_LOCAL_NAMESPACE as c_int;
                (*ns).href = crate::xml::string::xml_strdup(
                    c"http://www.w3.org/XML/1998/namespace".as_ptr() as *const xmlChar,
                );
                (*ns).prefix = crate::xml::string::xml_strdup(c"xml".as_ptr() as *const xmlChar);
                (*ns).next = (*parent).nsDef;
                (*parent).nsDef = ns;
            }
            return ns;
        }
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
/// UPSTREAM-PARITY (tree.c xmlSearchNsByHrefSafe): search for a namespace
/// declaration with the given URI in scope of `node`. Attributes are never in
/// the default namespace, so a prefix-less declaration cannot satisfy an
/// attribute search (`is_attr`). A declaration only counts when it is actually
/// in scope (not shadowed between `node` and the declaring element, and never
/// across entity boundaries). The walk only reads ELEMENT nodes.
pub unsafe fn search_ns_by_href(
    doc: *mut _xmlDoc,
    node: *mut _xmlNode,
    href: *const xmlChar,
) -> *mut _xmlNs {
    if node.is_null() || href.is_null() || unsafe { (*node).type_ } == XML_NAMESPACE_DECL as c_int {
        return ptr::null_mut();
    }

    let orig = node;

    // The XML-1.0 namespace is implicitly in scope everywhere via the prefix
    // "xml" (xmlTreeEnsureXMLDecl keeps it on doc->oldNs).
    let is_xml_ns_uri = unsafe {
        crate::abi::exports_xml2::xmlStrEqual(
            href,
            c"http://www.w3.org/XML/1998/namespace".as_ptr() as *const xmlChar,
        ) != 0
    };
    if is_xml_ns_uri && !doc.is_null() {
        return unsafe { ensure_doc_xml_ns(doc) };
    }

    let is_attr = unsafe { (*node).type_ } == XML_ATTRIBUTE_NODE as c_int;

    // Climb from a non-element node to its owning element, if any.
    let mut cur = node;
    while unsafe { (*cur).type_ } != XML_ELEMENT_NODE as c_int {
        cur = unsafe { (*cur).parent };
        if cur.is_null() {
            return ptr::null_mut();
        }
    }
    let parent = cur;

    // UPSTREAM-PARITY: `while ((node != NULL) && (node->type ==
    // XML_ELEMENT_NODE))` — a detached element (parent == NULL) terminates the
    // walk instead of dereferencing NULL.
    while !cur.is_null() && unsafe { (*cur).type_ } == XML_ELEMENT_NODE as c_int {
        let mut ns_def = unsafe { (*cur).nsDef };
        while !ns_def.is_null() {
            let ns = unsafe { &*ns_def };
            if !ns.href.is_null()
                && unsafe { crate::abi::exports_xml2::xmlStrEqual(ns.href, href) != 0 }
            {
                let ns_prefix = ns.prefix;
                if ((!is_attr) || !ns_prefix.is_null())
                    && unsafe { ns_in_scope(orig, cur, ns_prefix) } == 1
                {
                    return ns_def;
                }
            }
            ns_def = unsafe { (*ns_def).next };
        }
        if orig != cur {
            let el_ns = unsafe { (*cur).ns };
            if !el_ns.is_null() {
                let eh = (*el_ns).href;
                if !eh.is_null() && unsafe { crate::abi::exports_xml2::xmlStrEqual(eh, href) != 0 }
                {
                    let el_prefix = (*el_ns).prefix;
                    if ((!is_attr) || !el_prefix.is_null())
                        && unsafe { ns_in_scope(orig, cur, el_prefix) } == 1
                    {
                        return el_ns;
                    }
                }
            }
        }
        cur = unsafe { (*cur).parent };
    }

    // No document but the node belongs to a doc-less tree: exceptionally create
    // the xml declaration on the nearest element (upstream tree.c).
    if doc.is_null() && is_xml_ns_uri {
        let ns = allocator::xmlMallocZero(size_of::<_xmlNs>()) as *mut _xmlNs;
        if !ns.is_null() {
            unsafe {
                (*ns).type_ = XML_LOCAL_NAMESPACE as c_int;
                (*ns).href = crate::xml::string::xml_strdup(
                    c"http://www.w3.org/XML/1998/namespace".as_ptr() as *const xmlChar,
                );
                (*ns).prefix = crate::xml::string::xml_strdup(c"xml".as_ptr() as *const xmlChar);
                (*ns).next = (*parent).nsDef;
                (*parent).nsDef = ns;
            }
            return ns;
        }
    }

    ptr::null_mut()
}

// ═══════════════════════════════════════════════════════════════════════════════
// Attribute Operations
// ═══════════════════════════════════════════════════════════════════════════════

/// UPSTREAM-PARITY (tree.c xmlGetPropNodeInternal DTD arm): when an element has
/// no matching real attribute, an ATTLIST default/#FIXED declaration whose
/// `defaultValue` is non-NULL is reported as the attribute (returned as an
/// `XML_ATTRIBUTE_DECL` node). `ns_name` is the namespace URI: NULL selects
/// prefix-less declarations, the XML namespace selects the reserved "xml"
/// prefix, anything else is matched through the in-scope prefixes bound to that
/// URI. Returns the declaration cast to `*mut _xmlAttr`, or NULL.
unsafe fn dtd_default_decl_lookup(
    node: *mut _xmlNode,
    name: *const xmlChar,
    ns_name: *const xmlChar,
) -> *mut _xmlAttr {
    let n = unsafe { &*node };
    if n.doc.is_null() {
        return ptr::null_mut();
    }
    let doc = unsafe { &*(n.doc) };
    if doc.intSubset.is_null() && doc.extSubset.is_null() {
        return ptr::null_mut();
    }

    // We need the QName of the element for the DTD lookup.
    let ns = n.ns;
    let mut tmp: *mut xmlChar = ptr::null_mut();
    let elem_qname: *const xmlChar = if !ns.is_null() && !unsafe { (*ns).prefix.is_null() } {
        tmp = unsafe { crate::abi::exports_xml2::xmlStrdup((*ns).prefix) };
        if !tmp.is_null() {
            tmp = unsafe {
                crate::abi::exports_xml2::xmlStrcat(tmp, b":\0" as *const u8 as *const xmlChar)
            };
        }
        if !tmp.is_null() {
            tmp = unsafe { crate::abi::exports_xml2::xmlStrcat(tmp, n.name) };
        }
        if tmp.is_null() {
            return ptr::null_mut();
        }
        tmp
    } else {
        n.name
    };

    let mut attr_decl: *mut crate::abi::structs::_xmlAttribute = ptr::null_mut();
    let xml_ns_uri = b"http://www.w3.org/XML/1998/namespace\0";
    if ns_name.is_null() {
        attr_decl = crate::xml::validation::get_dtd_qattr_desc(
            doc.intSubset,
            elem_qname,
            name,
            ptr::null(),
        );
        if attr_decl.is_null() && !doc.extSubset.is_null() {
            attr_decl = crate::xml::validation::get_dtd_qattr_desc(
                doc.extSubset,
                elem_qname,
                name,
                ptr::null(),
            );
        }
    } else if unsafe {
        crate::abi::exports_xml2::xmlStrEqual(ns_name, xml_ns_uri.as_ptr() as *const xmlChar) != 0
    } {
        // The XML namespace must be bound to prefix 'xml'.
        let xml_prefix = b"xml\0";
        attr_decl = crate::xml::validation::get_dtd_qattr_desc(
            doc.intSubset,
            elem_qname,
            name,
            xml_prefix.as_ptr() as *const xmlChar,
        );
        if attr_decl.is_null() && !doc.extSubset.is_null() {
            attr_decl = crate::xml::validation::get_dtd_qattr_desc(
                doc.extSubset,
                elem_qname,
                name,
                xml_prefix.as_ptr() as *const xmlChar,
            );
        }
    } else {
        // The ugly case: search using the prefixes of in-scope ns-decls
        // corresponding to ns_name.
        let ns_list = unsafe { get_ns_list(n.doc, node) };
        if ns_list.is_null() {
            if !tmp.is_null() {
                allocator::xmlFreeImpl(tmp as *mut c_void);
            }
            return ptr::null_mut();
        }
        let mut cur = ns_list;
        while !unsafe { *cur }.is_null() {
            let d = unsafe { *cur };
            if !unsafe { (*d).href }.is_null()
                && unsafe { crate::abi::exports_xml2::xmlStrEqual((*d).href, ns_name) != 0 }
            {
                attr_decl = crate::xml::validation::get_dtd_qattr_desc(
                    doc.intSubset,
                    elem_qname,
                    name,
                    (*d).prefix,
                );
                if attr_decl.is_null() && !doc.extSubset.is_null() {
                    attr_decl = crate::xml::validation::get_dtd_qattr_desc(
                        doc.extSubset,
                        elem_qname,
                        name,
                        (*d).prefix,
                    );
                }
                if !attr_decl.is_null() {
                    break;
                }
            }
            cur = cur.add(1);
        }
        allocator::xmlFreeImpl(ns_list as *mut c_void);
    }
    if !tmp.is_null() {
        allocator::xmlFreeImpl(tmp as *mut c_void);
    }

    if !attr_decl.is_null() && !unsafe { (*attr_decl).defaultValue.is_null() } {
        return attr_decl as *mut _xmlAttr;
    }
    ptr::null_mut()
}

/// UPSTREAM-PARITY (tree.c xmlHasProp DTD arm): plain-element-name variant of
/// the default/#FIXED lookup (no QName prefix expansion, matching upstream
/// xmlHasProp's xmlGetDtdAttrDesc call).
unsafe fn dtd_default_decl_lookup_plain(
    node: *mut _xmlNode,
    name: *const xmlChar,
) -> *mut _xmlAttr {
    let n = unsafe { &*node };
    if n.doc.is_null() {
        return ptr::null_mut();
    }
    let doc = unsafe { &*(n.doc) };
    let mut attr_decl: *mut crate::abi::structs::_xmlAttribute = ptr::null_mut();
    if !doc.intSubset.is_null() {
        attr_decl = crate::xml::validation::get_dtd_attr_desc(doc.intSubset, n.name, name);
        if attr_decl.is_null() && !doc.extSubset.is_null() {
            attr_decl = crate::xml::validation::get_dtd_attr_desc(doc.extSubset, n.name, name);
        }
        if !attr_decl.is_null() && !unsafe { (*attr_decl).defaultValue.is_null() } {
            return attr_decl as *mut _xmlAttr;
        }
    }
    ptr::null_mut()
}

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
/// UPSTREAM-PARITY (tree.c xmlSetProp): set an attribute given its QName.
/// A prefixed name resolves the prefix through the in-scope namespace
/// declarations and delegates to `xmlSetNsProp` with the LOCAL name; when the
/// prefix is unbound (or the name is unprefixed) the attribute is set in no
/// namespace under its (raw) name — an unprefixed attribute is never in a
/// namespace, and an unbound prefix keeps the raw QName (matching the SAX2
/// tree-builder convention for undefined prefixes).
pub unsafe fn set_prop(
    node: *mut _xmlNode,
    name: *const xmlChar,
    value: *const xmlChar,
) -> *mut _xmlAttr {
    if node.is_null() || name.is_null() || unsafe { (*node).type_ } != XML_ELEMENT_NODE as c_int {
        return ptr::null_mut();
    }

    let mut prefix: *mut xmlChar = ptr::null_mut();
    let localname = crate::xml::validation::split_qname4(name, &mut prefix);
    if localname.is_null() {
        if !prefix.is_null() {
            allocator::xmlFreeImpl(prefix as *mut c_void);
        }
        return ptr::null_mut();
    }
    if !prefix.is_null() {
        let ns = unsafe { search_ns((*node).doc, node, prefix) };
        if !ns.is_null() {
            allocator::xmlFreeImpl(prefix as *mut c_void);
            return unsafe { set_ns_prop(node, ns, localname, value) };
        }
        allocator::xmlFreeImpl(prefix as *mut c_void);
        return unsafe { set_ns_prop(node, ptr::null_mut(), name, value) };
    }

    unsafe { set_ns_prop(node, ptr::null_mut(), name, value) }
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
    if node.is_null() || name.is_null() || unsafe { (*node).type_ } != XML_ELEMENT_NODE as c_int {
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

    // UPSTREAM-PARITY (tree.c xmlGetProp/xmlHasProp): when the element has no
    // matching real attribute, an ATTLIST default/#FIXED declaration value is
    // returned.
    let decl = unsafe { dtd_default_decl_lookup_plain(node, name) };
    if !decl.is_null() {
        let a = decl as *mut crate::abi::structs::_xmlAttribute;
        let dv = unsafe { (*a).defaultValue };
        if !dv.is_null() {
            return dup_xml_str(dv);
        }
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
    name_space: *const xmlChar,
) -> *mut xmlChar {
    if node.is_null() || name.is_null() || unsafe { (*node).type_ } != XML_ELEMENT_NODE as c_int {
        return ptr::null_mut();
    }
    // UPSTREAM-PARITY (tree.c xmlGetNsProp/xmlGetPropNodeInternal): match the
    // LOCAL name plus the namespace — a NULL nameSpace matches only
    // UNPREFIXED attributes; a non-NULL nameSpace matches only attributes
    // whose ns href equals it (an unprefixed attribute is NEVER in a
    // namespace, not even the element's default one). Returns the value (""
    // when empty) or NULL.
    let mut cur = unsafe { (*node).properties };
    while !cur.is_null() {
        let attr = unsafe { &*cur };
        if !attr.name.is_null()
            && unsafe { crate::abi::exports_xml2::xmlStrEqual(attr.name, name) != 0 }
        {
            let matches = if name_space.is_null() {
                attr.ns.is_null()
            } else if !attr.ns.is_null() && !(*attr.ns).href.is_null() {
                unsafe { crate::abi::exports_xml2::xmlStrEqual((*attr.ns).href, name_space) != 0 }
            } else {
                false
            };
            if matches {
                if !attr.children.is_null() {
                    let text = unsafe { &*attr.children };
                    if text.type_ == XML_TEXT_NODE as c_int && !text.content.is_null() {
                        return dup_xml_str(text.content);
                    }
                }
                return dup_xml_str(b"\0" as *const u8 as *const xmlChar);
            }
        }
        cur = unsafe { (*cur).next };
    }

    // UPSTREAM-PARITY (tree.c xmlGetPropNodeInternal useDTD arm): DTD
    // default/#FIXED declarations are reported as present attributes.
    let decl = unsafe { dtd_default_decl_lookup(node, name, name_space) };
    if !decl.is_null() {
        let a = decl as *mut crate::abi::structs::_xmlAttribute;
        let dv = unsafe { (*a).defaultValue };
        if !dv.is_null() {
            return dup_xml_str(dv);
        }
    }
    ptr::null_mut()
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
    ns: *mut _xmlNs,
    name: *const xmlChar,
    value: *const xmlChar,
) -> *mut _xmlAttr {
    if node.is_null() || name.is_null() {
        return ptr::null_mut();
    }
    let n = unsafe { &mut *node };

    // Namespace-aware variant of xmlSetProp (upstream xmlSetNsProp / tree.c
    // xmlSetNsPropInternal): find an existing attribute that shares name AND
    // namespace (href); when found, update its value, otherwise create a new
    // namespaced attribute bound to `ns`. The legacy Phase-1 stub ignored `ns`
    // and created an UNNAMESPACED attribute, which broke the modern DOM ns
    // mapper that materialises xmlns declarations as real XMLNS-ns attributes
    // (php dom_mark_namespaces_as_attributes_too -> xmlSetNsProp).

    let mut existing = n.properties;
    while !existing.is_null() {
        let attr = unsafe { &*existing };
        let same_ns = if !ns.is_null() {
            // Both declare the same href (prefix may legitimately come from
            // different mapper instances, so compare by href).
            if unsafe { (*existing).ns }.is_null() {
                false
            } else {
                let an = unsafe { &*(*existing).ns };
                let bn = unsafe { &*ns };
                (!an.href.is_null()
                    && !bn.href.is_null()
                    && unsafe {
                        crate::abi::exports_xml2::xmlStrEqual(
                            an.href as *const crate::abi::types::xmlChar,
                            bn.href as *const crate::abi::types::xmlChar,
                        ) != 0
                    })
                    || (an.href.is_null() && bn.href.is_null())
            }
        } else {
            unsafe { (*existing).ns }.is_null()
        };
        if !attr.name.is_null()
            && unsafe { crate::abi::exports_xml2::xmlStrEqual(attr.name, name) != 0 }
            && same_ns
        {
            // UPSTREAM-PARITY (tree.c xmlSetNsProp modify branch): the
            // attribute is rebound to the NEW namespace — the passed `ns`
            // may carry a different prefix for the same URI (the modern
            // DOM ns-mapper allocates a fresh prefix per qualified name,
            // so setAttributeNS("urn:a", "y:foo", ...) renames x:foo to
            // y:foo rather than keeping the stale prefix).
            let attr_mut = existing;
            unsafe { (*attr_mut).ns = ns };
            // UPSTREAM-PARITY (tree.c xmlSetNsProp modify branch): an
            // attribute whose current value is registered as an ID drops its
            // old entry and keeps its ID type, so the new value is
            // re-registered below (xml:id / HTML id value changes move the
            // doc->ids mapping — bug79701).
            let mut was_id = false;
            if !n.doc.is_null() && !attr.id.is_null() {
                crate::xml::validation::remove_id(n.doc, existing);
                was_id = true;
                let am = existing;
                unsafe {
                    (*am).atype = crate::abi::types::xmlAttributeType::XML_ATTRIBUTE_ID as c_int;
                }
            }
            // Update existing attribute value (mirror set_prop: free old
            // text children and set the new text value).
            if !attr.children.is_null() {
                free_node_list(attr.children);
                let attr_mut = existing;
                unsafe {
                    (*attr_mut).children = ptr::null_mut();
                    (*attr_mut).last = ptr::null_mut();
                }
            }
            if !value.is_null() {
                let text = new_text(value);
                if !text.is_null() {
                    let attr_mut = existing;
                    unsafe {
                        (*attr_mut).children = text;
                        (*attr_mut).last = text;
                        (*text).parent = existing as *mut _xmlNode;
                        (*text).doc = n.doc;
                    }
                }
                if was_id {
                    crate::xml::validation::add_id_safe(existing, value);
                }
            }
            return existing;
        }
        existing = unsafe { (*existing).next };
    }

    // Create a new namespaced attribute.
    let attr = allocator::xmlMallocZero(size_of::<_xmlAttr>() as usize) as *mut _xmlAttr;
    if attr.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        (*attr).type_ = XML_ATTRIBUTE_NODE as c_int;
        (*attr).name = dup_xml_str(name);
        (*attr).ns = ns;
        (*attr).parent = node;
        (*attr).doc = n.doc;
    }
    if !value.is_null() {
        let text = new_text(value);
        if !text.is_null() {
            unsafe {
                (*attr).children = text;
                (*attr).last = text;
                (*text).parent = attr as *mut _xmlNode;
                (*text).doc = n.doc;
            }
        }
        // UPSTREAM-PARITY (tree.c xmlNewPropInternal): a newly created
        // attribute whose name/type makes it an ID (HTML id, xml:id, DTD ID
        // declarations) is registered against the document's ID table
        // immediately.
        if !n.doc.is_null() {
            let res = crate::xml::validation::is_id(n.doc, node, attr);
            if res > 0 {
                crate::xml::validation::add_id_safe(attr, value);
            }
        }
    }
    // Attach to the node's property list.
    unsafe {
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

    // UPSTREAM-PARITY (tree.c xmlRemoveProp): a NULL parent means the
    // attribute is not attached to any element — upstream returns -1
    // without freeing (the caller keeps ownership).
    if a.parent.is_null() {
        return -1;
    }

    // Unlink from the parent's property list. Upstream scans the list for
    // `attr`; once found it rethreads prev/next and hands the attribute to
    // xmlFreeProp. When the attribute is not in the list, -1 is returned
    // and nothing is freed.
    let parent = a.parent;
    let p = unsafe { &mut *parent };
    if p.properties == attr {
        p.properties = a.next;
        if !a.next.is_null() {
            unsafe { (*a.next).prev = ptr::null_mut() };
        }
        free_prop(attr);
        return 0;
    }
    let mut tmp = p.properties;
    while !tmp.is_null() {
        let next = unsafe { (*tmp).next };
        if next == attr {
            unsafe { (*tmp).next = a.next };
            if !a.next.is_null() {
                unsafe { (*a.next).prev = tmp };
            }
            free_prop(attr);
            return 0;
        }
        tmp = next;
    }
    -1
}

/// Check whether a node has a property with the given name (upstream tree.c
/// `xmlHasProp`): returns the attribute pointer or NULL.
///
/// # SAFETY
///
/// - `node` must be a valid node pointer or NULL.
/// - `name` must be a valid null-terminated string.
pub unsafe fn has_prop(node: *mut _xmlNode, name: *const xmlChar) -> *mut _xmlAttr {
    if node.is_null() || name.is_null() || unsafe { (*node).type_ } != XML_ELEMENT_NODE as c_int {
        return ptr::null_mut();
    }
    let mut cur = unsafe { (*node).properties };
    while !cur.is_null() {
        let attr = unsafe { &*cur };
        // UPSTREAM-PARITY (tree.c xmlHasProp): the search matches the LOCAL
        // name only — namespaced attributes are found too (php's
        // setAttributeNode replacement lookup relies on this). The NULL-
        // namespace restriction belongs to xmlHasNsProp.
        if !attr.name.is_null()
            && unsafe { crate::abi::exports_xml2::xmlStrEqual(attr.name, name) != 0 }
        {
            return cur;
        }
        cur = unsafe { (*cur).next };
    }

    // UPSTREAM-PARITY (tree.c xmlHasProp): report ATTLIST default/#FIXED
    // declarations as present (php removeAttribute/toggleAttribute rely on the
    // returned XML_ATTRIBUTE_DECL being non-removable).
    unsafe { dtd_default_decl_lookup_plain(node, name) }
}

/// Check whether a node has a namespaced property (upstream tree.c
/// `xmlHasNsProp`): returns the attribute pointer or NULL. A NULL
/// `nameSpace` matches the no-namespace case.
///
/// # SAFETY
///
/// - `node` must be a valid node pointer or NULL.
/// - `name` must be a valid null-terminated string.
/// - `nameSpace` may be NULL.
pub unsafe fn has_ns_prop(
    node: *mut _xmlNode,
    name: *const xmlChar,
    name_space: *const xmlChar,
) -> *mut _xmlAttr {
    if node.is_null() || name.is_null() || unsafe { (*node).type_ } != XML_ELEMENT_NODE as c_int {
        return ptr::null_mut();
    }
    let mut cur = unsafe { (*node).properties };
    while !cur.is_null() {
        let attr = unsafe { &*cur };
        if !attr.name.is_null()
            && unsafe { crate::abi::exports_xml2::xmlStrEqual(attr.name, name) != 0 }
        {
            if name_space.is_null() {
                if attr.ns.is_null() {
                    return cur;
                }
            } else if !attr.ns.is_null()
                && !(*attr.ns).href.is_null()
                && unsafe {
                    crate::abi::exports_xml2::xmlStrEqual((*attr.ns).href, name_space) != 0
                }
            {
                return cur;
            }
        }
        cur = unsafe { (*cur).next };
    }

    // UPSTREAM-PARITY (tree.c xmlGetPropNodeInternal useDTD arm): report
    // ATTLIST default/#FIXED declarations as present (php's
    // setAttributeNode replacement lookup treats the returned
    // XML_ATTRIBUTE_DECL as "no existing attribute").
    unsafe { dtd_default_decl_lookup(node, name, name_space) }
}

/// Remove a property by name from a node (upstream tree.c `xmlUnsetProp`):
/// returns 0 on success, -1 if the property does not exist or arguments are
/// NULL.
///
/// # SAFETY
///
/// - `node` must be a valid node pointer or NULL.
/// - `name` must be a valid null-terminated string.
pub unsafe fn unset_prop(node: *mut _xmlNode, name: *const xmlChar) -> c_int {
    let attr = unsafe { has_prop(node, name) };
    if attr.is_null() {
        return -1;
    }
    unsafe { remove_prop(attr) }
}

/// Remove a namespaced property by name (upstream tree.c `xmlUnsetNsProp`).
///
/// # SAFETY
///
/// - `node` must be a valid node pointer or NULL.
/// - `name` must be a valid null-terminated string.
/// - `nameSpace` may be NULL.
pub unsafe fn unset_ns_prop(
    node: *mut _xmlNode,
    name: *const xmlChar,
    name_space: *const xmlChar,
) -> c_int {
    let attr = unsafe { has_ns_prop(node, name, name_space) };
    if attr.is_null() {
        return -1;
    }
    unsafe { remove_prop(attr) }
}

/// Return the first child ELEMENT of a node, or NULL (upstream tree.c
/// `xmlFirstElementChild`).
///
/// # SAFETY
///
/// - `node` must be a valid node pointer or NULL.
pub unsafe fn first_element_child(node: *mut _xmlNode) -> *mut _xmlNode {
    if node.is_null() {
        return ptr::null_mut();
    }
    let mut cur = unsafe { (*node).children };
    while !cur.is_null() {
        if unsafe { (*cur).type_ } == XML_ELEMENT_NODE as c_int {
            return cur;
        }
        cur = unsafe { (*cur).next };
    }
    ptr::null_mut()
}

/// Return the last child ELEMENT of a node, or NULL (upstream tree.c
/// `xmlLastElementChild`).
///
/// # SAFETY
///
/// - `node` must be a valid node pointer or NULL.
pub unsafe fn last_element_child(node: *mut _xmlNode) -> *mut _xmlNode {
    if node.is_null() {
        return ptr::null_mut();
    }
    let mut cur = unsafe { (*node).last };
    while !cur.is_null() {
        if unsafe { (*cur).type_ } == XML_ELEMENT_NODE as c_int {
            return cur;
        }
        cur = unsafe { (*cur).prev };
    }
    ptr::null_mut()
}

/// Return the next ELEMENT sibling of a node, or NULL (upstream tree.c
/// `xmlNextElementSibling`).
///
/// # SAFETY
///
/// - `node` must be a valid node pointer or NULL.
pub unsafe fn next_element_sibling(node: *mut _xmlNode) -> *mut _xmlNode {
    if node.is_null() {
        return ptr::null_mut();
    }
    let mut cur = unsafe { (*node).next };
    while !cur.is_null() {
        if unsafe { (*cur).type_ } == XML_ELEMENT_NODE as c_int {
            return cur;
        }
        cur = unsafe { (*cur).next };
    }
    ptr::null_mut()
}

/// Return the previous ELEMENT sibling of a node, or NULL (upstream tree.c
/// `xmlPreviousElementSibling`).
///
/// # SAFETY
///
/// - `node` must be a valid node pointer or NULL.
pub unsafe fn previous_element_sibling(node: *mut _xmlNode) -> *mut _xmlNode {
    if node.is_null() {
        return ptr::null_mut();
    }
    let mut cur = unsafe { (*node).prev };
    while !cur.is_null() {
        if unsafe { (*cur).type_ } == XML_ELEMENT_NODE as c_int {
            return cur;
        }
        cur = unsafe { (*cur).prev };
    }
    ptr::null_mut()
}

/// Count the child ELEMENT nodes of a node (upstream tree.c
/// `xmlChildElementCount`).
///
/// # SAFETY
///
/// - `node` must be a valid node pointer or NULL.
pub unsafe fn child_element_count(node: *mut _xmlNode) -> c_ulong {
    if node.is_null() {
        return 0;
    }
    let mut cur = unsafe { (*node).children };
    let mut count: c_ulong = 0;
    while !cur.is_null() {
        if unsafe { (*cur).type_ } == XML_ELEMENT_NODE as c_int {
            count += 1;
        }
        cur = unsafe { (*cur).next };
    }
    count
}

/// Concatenate text to a node's content (upstream tree.c `xmlTextConcat`):
/// appends `num` bytes of `str` to the node's text content. Returns 0 on
/// success, -1 on error.
///
/// # SAFETY
///
/// - `node` must be a valid text node or NULL.
/// - `str` must be a valid buffer of `num` bytes.
pub unsafe fn text_concat(node: *mut _xmlNode, str: *const xmlChar, num: c_int) -> c_int {
    if node.is_null() || str.is_null() || num <= 0 {
        return -1;
    }
    let cur = unsafe { &mut *node };
    if cur.content.is_null() {
        let p = unsafe { allocator::xmlMallocImpl(num as usize + 1) as *mut xmlChar };
        if p.is_null() {
            return -1;
        }
        unsafe {
            ptr::copy_nonoverlapping(str, p, num as usize);
            *p.add(num as usize) = 0;
        }
        cur.content = p;
        return 0;
    }
    let old_len = unsafe { crate::xml::string::xml_strlen(cur.content) };
    let p = unsafe {
        allocator::xmlReallocImpl(cur.content as *mut c_void, old_len + num as usize + 1)
            as *mut xmlChar
    };
    if p.is_null() {
        return -1;
    }
    unsafe {
        ptr::copy_nonoverlapping(str, p.add(old_len), num as usize);
        *p.add(old_len + num as usize) = 0;
    }
    cur.content = p;
    0
}

/// Merge the text content of two nodes (upstream tree.c `xmlTextMerge`):
/// appends `ntext`'s content to `text`'s content and frees `ntext`.
/// Returns the first node, or NULL on error.
///
/// # SAFETY
///
/// - `text` and `ntext` must be valid text nodes or NULL.
pub unsafe fn text_merge(text: *mut _xmlNode, ntext: *mut _xmlNode) -> *mut _xmlNode {
    if text.is_null() || ntext.is_null() {
        return ptr::null_mut();
    }
    if unsafe { (*ntext).content.is_null() } {
        unsafe { free_node(ntext) };
        return text;
    }
    let num = unsafe { crate::xml::string::xml_strlen((*ntext).content) };
    if unsafe { text_concat(text, (*ntext).content, num as c_int) } != 0 {
        return ptr::null_mut();
    }
    unsafe { free_node(ntext) };
    text
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
pub const fn get_int_subset(doc: *const _xmlDoc) -> *mut _xmlDtd {
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

    // UPSTREAM-PARITY (tree.c xmlNewDtd): a document that already has an
    // external subset cannot take another — upstream returns NULL.
    if !doc.is_null() && !(*doc).extSubset.is_null() {
        allocator::xmlFreeImpl(dtd as *mut c_void);
        return ptr::null_mut();
    }

    unsafe {
        (*dtd).type_ = XML_DTD_NODE as c_int;
        (*dtd).name = dup_xml_str(name);
        (*dtd).ExternalID = dup_xml_str(ExternalID);
        (*dtd).SystemID = dup_xml_str(SystemID);
        (*dtd).parent = doc;
        (*dtd).doc = doc;

        // UPSTREAM-PARITY (tree.c xmlNewDtd): a freshly created DTD node
        // becomes the document's EXTERNAL subset (doc->extSubset); the
        // internal subset is created via xmlCreateIntSubset.
        // nokogiri create_external_subset reads doc->extSubset back as
        // Document#external_subset.

        // Attach to document as the external subset
        if !doc.is_null() {
            (*doc).extSubset = dtd;
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

    let _d = unsafe { &mut *dtd };
    let d = &mut *dtd;

    // UPSTREAM-PARITY (tree.c xmlFreeDtd): element/attribute/entity
    // declaration nodes in the child list are owned by the hash tables and
    // are freed by the deallocators below; only non-declaration children
    // (comments, PIs) are unlinked and freed from the list here. This must
    // run BEFORE the hash tables are freed so the decl nodes are still alive
    // when their type is inspected.
    if !d.children.is_null() {
        let mut c = d.children;
        while !c.is_null() {
            let next = (*c).next;
            let t = (*c).type_;
            if t != XML_ELEMENT_DECL as c_int
                && t != XML_ATTRIBUTE_DECL as c_int
                && t != XML_ENTITY_DECL as c_int
            {
                unlink_node_internal(c, ptr::null_mut());
                free_node(c);
            }
            c = next;
        }
    }

    // Free name
    if !d.name.is_null() {
        allocator::xmlFreeImpl(d.name as *mut c_void);
    }
    if !d.ExternalID.is_null() {
        allocator::xmlFreeImpl(d.ExternalID as *mut c_void);
    }
    if !d.SystemID.is_null() {
        allocator::xmlFreeImpl(d.SystemID as *mut c_void);
    }

    // Free hash tables for declarations
    /// Hash-table deallocator shim that frees an `_xmlNotation` payload.
    ///
    /// # Safety
    ///
    /// - `payload` must be NULL or a valid pointer to an `_xmlNotation` owned
    ///   exclusively by the hash table being freed; it is freed with
    ///   `free_notation`.
    /// - `_name` is unused.
    unsafe extern "C" fn free_notation_wrapper(payload: *mut c_void, _name: *mut u8) {
        crate::xml::dtd::free_notation(payload as *mut _xmlNotation);
    }
    /// Hash-table deallocator shim that frees an `_xmlElement` payload.
    ///
    /// # Safety
    ///
    /// - `payload` must be NULL or a valid pointer to an `_xmlElement` owned
    ///   exclusively by the hash table being freed; it is freed with
    ///   `free_element`.
    /// - `_name` is unused.
    unsafe extern "C" fn free_element_wrapper(payload: *mut c_void, _name: *mut u8) {
        crate::xml::dtd::free_element(payload as *mut _xmlElement);
    }
    /// Hash-table deallocator shim that frees an `_xmlAttribute` payload.
    ///
    /// # Safety
    ///
    /// - `payload` must be NULL or a valid pointer to an `_xmlAttribute` owned
    ///   exclusively by the hash table being freed; it is freed with
    ///   `free_attribute`.
    /// - `_name` is unused.
    unsafe extern "C" fn free_attribute_wrapper(payload: *mut c_void, _name: *mut u8) {
        crate::xml::dtd::free_attribute(payload as *mut _xmlAttribute);
    }
    /// Hash-table deallocator shim that frees an `_xmlEntity` payload.
    ///
    /// # Safety
    ///
    /// - `payload` must be NULL or a valid pointer to an `_xmlEntity` owned
    ///   exclusively by the hash table being freed; it is freed with
    ///   `free_entity`.
    /// - `_name` is unused.
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

    allocator::xmlFreeImpl(dtd as *mut c_void);
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

/// Add an entity declaration to the document's internal subset (upstream
/// entities.c `xmlAddDocEntity`); creates the internal subset when absent.
///
/// # SAFETY
///
/// - `doc` must be a valid document pointer or NULL.
/// - `name` must be a valid null-terminated string.
pub unsafe fn add_doc_entity(
    doc: *mut _xmlDoc,
    name: *const xmlChar,
    etype: c_int,
    ExternalID: *const xmlChar,
    SystemID: *const xmlChar,
    content: *const xmlChar,
) -> *mut _xmlEntity {
    if doc.is_null() || name.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        let mut dtd = (*doc).intSubset;
        if dtd.is_null() {
            dtd = new_dtd(
                doc,
                c"internal".as_ptr() as *const xmlChar,
                ptr::null(),
                ptr::null(),
            );
            if dtd.is_null() {
                return ptr::null_mut();
            }
        }
        crate::xml::entities::add_entity(dtd, name, etype, ExternalID, SystemID, content)
    }
}

/// Add an entity declaration to the document's external subset (upstream
/// entities.c `xmlAddDtdEntity`); creates the external subset when absent.
///
/// # SAFETY
///
/// - `doc` must be a valid document pointer or NULL.
/// - `name` must be a valid null-terminated string.
pub unsafe fn add_dtd_entity(
    doc: *mut _xmlDoc,
    name: *const xmlChar,
    etype: c_int,
    ExternalID: *const xmlChar,
    SystemID: *const xmlChar,
    content: *const xmlChar,
) -> *mut _xmlEntity {
    if doc.is_null() || name.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        let mut dtd = (*doc).extSubset;
        if dtd.is_null() {
            dtd = new_dtd(
                doc,
                c"internal".as_ptr() as *const xmlChar,
                ptr::null(),
                ptr::null(),
            );
            if dtd.is_null() {
                return ptr::null_mut();
            }
            (*doc).extSubset = dtd;
        }
        crate::xml::entities::add_entity(dtd, name, etype, ExternalID, SystemID, content)
    }
}

/// Get an entity declaration from the internal or external subset (upstream
/// entities.c `xmlGetDtdEntity`).
///
/// # SAFETY
///
/// - `doc` must be a valid document pointer or NULL.
/// - `name` must be a valid null-terminated string.
pub unsafe fn get_dtd_entity(doc: *const _xmlDoc, name: *const xmlChar) -> *mut _xmlEntity {
    if doc.is_null() || name.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        if !(*doc).intSubset.is_null() {
            let e = crate::xml::entities::get_entity_from_dtd((*doc).intSubset, name);
            if !e.is_null() {
                return e;
            }
        }
        if !(*doc).extSubset.is_null() {
            return crate::xml::entities::get_entity_from_dtd((*doc).extSubset, name);
        }
        ptr::null_mut()
    }
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
#[allow(dead_code)]
const ENTITY_APOS: &[xmlChar] = b"&apos;";

/// Indentation string (libxml2's default `xmlTreeIndentString`).
const INDENT: &[xmlChar] = b"  ";

/// Maximum indent buffer size (libxml2 `MAX_INDENT` in xmlsave.c).
const MAX_INDENT: c_int = 60;

/// Serialize text content with XML escaping.
///
/// Decode one UTF-8 sequence whose first byte is at `bytes[i]` and write the
/// upstream `xmlSerializeHexCharRef` form (`&#x%X;`, uppercase hex, no
/// padding). Returns the number of input bytes consumed (1 on invalid
/// sequences — the upstream U+FFFD fallback advances one byte).
///
/// # Safety
///
/// - `bytes` must be readable for `len` bytes with `i < len`.
unsafe fn write_utf8_hex_char_ref(buf: *mut _xmlBuffer, bytes: &[u8], i: usize) -> usize {
    // SAFETY: caller guarantees i < len.
    let first = bytes[i];
    let (n, mut val): (usize, u32) = if first < 0x80 {
        (1, first as u32)
    } else if first < 0xE0 {
        if i + 1 < bytes.len() {
            (
                2,
                ((first & 0x1F) as u32) << 6 | (bytes[i + 1] & 0x3F) as u32,
            )
        } else {
            (1, 0xFFFD)
        }
    } else if first < 0xF0 {
        if i + 2 < bytes.len() {
            (
                3,
                ((first & 0x0F) as u32) << 12
                    | ((bytes[i + 1] & 0x3F) as u32) << 6
                    | (bytes[i + 2] & 0x3F) as u32,
            )
        } else {
            (1, 0xFFFD)
        }
    } else if first < 0xF8 {
        if i + 3 < bytes.len() {
            (
                4,
                ((first & 0x07) as u32) << 18
                    | ((bytes[i + 1] & 0x3F) as u32) << 12
                    | ((bytes[i + 2] & 0x3F) as u32) << 6
                    | (bytes[i + 3] & 0x3F) as u32,
            )
        } else {
            (1, 0xFFFD)
        }
    } else {
        (1, 0xFFFD)
    };
    if val == 0xFFFE || val == 0xFFFF || val > 0x10FFFF {
        val = 0xFFFD;
    }
    // SAFETY: writes the formatted reference (xmlSerializeHexCharRef).
    let hex = format!("&#x{:X};", val);
    io::buf_add(buf, hex.as_ptr(), hex.len() as c_int);
    n
}

/// Serialize text content with XML escaping.
///
/// # UPSTREAM-PARITY
///
/// Mirrors libxml2 2.15 `xmlSerializeText`. When `escape_non_ascii` is set
/// (upstream `XML_ESCAPE_NON_ASCII` — chosen by xmlsave.c `xmlSaveWriteText`
/// when the save context has NO output encoder, `ctxt->encoding == NULL`),
/// every non-ASCII byte is decoded as UTF-8 and written as a hexadecimal
/// character reference — `café` → `caf&#xE9;`, U+00A0 → `&#xA0;` (ext/dom
/// dom005's xml save of html-origin text, xmlsave oracle parity). Without the
/// flag (a non-NULL save encoding, as in the libxslt save path): `<` →
/// `&lt;`, `>` → `&gt;`, `&` → `&amp;`, `\r` → `&#13;`, other control
/// characters → hexadecimal character references, while `\n` and `\t` are
/// emitted literally and non-ASCII bytes are passed through.
///
/// # SAFETY
///
/// - `buf` must be a valid pointer to a mutable `_xmlBuffer`.
/// - `content` must be a valid pointer to `len` bytes of xmlChar data, or NULL.
pub(crate) unsafe fn serialize_text_flags(
    buf: *mut _xmlBuffer,
    content: *const xmlChar,
    len: c_int,
    escape_non_ascii: bool,
) {
    if buf.is_null() || content.is_null() || len <= 0 {
        return;
    }

    let bytes = core::slice::from_raw_parts(content, len as usize);
    let mut i: usize = 0;
    while i < bytes.len() {
        let ch = bytes[i];

        // Check for `]]>` sequence
        if ch == b']' && i + 2 < bytes.len() && bytes[i + 1] == b']' && bytes[i + 2] == b'>' {
            // Write `]]&gt;` — escape the `>` that ends `]]>`
            io::buf_add(buf, b"]]" as *const u8, 2);
            io::buf_add(buf, ENTITY_GT.as_ptr(), ENTITY_GT.len() as c_int);
            i += 3;
            continue;
        }

        if ch >= 0x80 && escape_non_ascii {
            // SAFETY: i < bytes.len() holds; the helper decodes the UTF-8
            // sequence and advances past it.
            i += unsafe { write_utf8_hex_char_ref(buf, bytes, i) };
            continue;
        }

        match ch {
            b'<' => {
                io::buf_add(buf, ENTITY_LT.as_ptr(), ENTITY_LT.len() as c_int);
            }
            b'&' => {
                io::buf_add(buf, ENTITY_AMP.as_ptr(), ENTITY_AMP.len() as c_int);
            }
            b'>' => {
                // UPSTREAM-PARITY: libxml2 escapes `>` to `&gt;` in text content.
                // While the XML spec only requires escaping `>` in `]]>`, libxml2's
                // serializer escapes all `>` characters.
                io::buf_add(buf, ENTITY_GT.as_ptr(), ENTITY_GT.len() as c_int);
            }
            b'\r' => {
                // Carriage return is not allowed literally in XML content.
                io::buf_add(buf, b"&#13;" as *const u8, 5);
            }
            0x01..=0x08 | 0x0B | 0x0C | 0x0E..=0x1F => {
                // Other control characters are emitted as hex character refs.
                let hex = format!("&#x{:X};", ch);
                io::buf_add(buf, hex.as_ptr(), hex.len() as c_int);
            }
            _ => {
                io::buf_add(buf, &ch as *const u8, 1);
            }
        }
        i += 1;
    }
}

/// Serialize an attribute value with XML escaping.
///
/// # UPSTREAM-PARITY
///
/// Mirrors libxml2 `xmlBufAttrSerializeTxtContent` (xmlsave.c):
/// `\n` → `&#10;`, `\r` → `&#13;`, `\t` → `&#9;`, `"` → `&quot;`,
/// `<` → `&lt;`, `>` → `&gt;`, `&` → `&amp;`.
///
/// # SAFETY
///
/// - `buf` must be a valid pointer to a mutable `_xmlBuffer`.
/// - `value` must be a valid null-terminated xmlChar string, or NULL.
pub(crate) unsafe fn serialize_attr_value(buf: *mut _xmlBuffer, value: *const xmlChar) {
    unsafe { serialize_attr_value_flags(buf, value, false) };
}

/// Like [`serialize_attr_value`] plus the upstream `XML_ESCAPE_NON_ASCII`
/// flag (xmlsave.c `xmlSaveWriteText` / `xmlBufAttrSerializeTxtContent`):
/// non-ASCII bytes become hexadecimal character references when the save
/// context has no output encoder.
///
/// # SAFETY
///
/// - `buf` must be a valid pointer to a mutable `_xmlBuffer`.
/// - `value` must be a valid null-terminated xmlChar string, or NULL.
pub(crate) unsafe fn serialize_attr_value_flags(
    buf: *mut _xmlBuffer,
    value: *const xmlChar,
    escape_non_ascii: bool,
) {
    if buf.is_null() || value.is_null() {
        return;
    }

    let len = xml_strlen(value);
    let bytes = core::slice::from_raw_parts(value, len as usize);
    let mut i: usize = 0;
    while i < bytes.len() {
        let ch = bytes[i];

        if ch >= 0x80 && escape_non_ascii {
            // SAFETY: i < bytes.len() holds; the helper decodes the UTF-8
            // sequence and advances past it.
            i += unsafe { write_utf8_hex_char_ref(buf, bytes, i) };
            continue;
        }

        match ch {
            b'\n' => {
                io::buf_add(buf, b"&#10;" as *const u8, 5);
            }
            b'\r' => {
                io::buf_add(buf, b"&#13;" as *const u8, 5);
            }
            b'\t' => {
                io::buf_add(buf, b"&#9;" as *const u8, 4);
            }
            b'<' => {
                io::buf_add(buf, ENTITY_LT.as_ptr(), ENTITY_LT.len() as c_int);
            }
            b'&' => {
                io::buf_add(buf, ENTITY_AMP.as_ptr(), ENTITY_AMP.len() as c_int);
            }
            b'"' => {
                io::buf_add(buf, ENTITY_QUOT.as_ptr(), ENTITY_QUOT.len() as c_int);
            }
            b'>' => {
                io::buf_add(buf, ENTITY_GT.as_ptr(), ENTITY_GT.len() as c_int);
            }
            _ => {
                io::buf_add(buf, &ch as *const u8, 1);
            }
        }
        i += 1;
    }
}

/// Write indentation.
///
/// # UPSTREAM-PARITY
///
/// Mirrors libxml2 `xmlSaveWriteIndent` (xmlsave.c 2.15): the level is
/// capped at `MAX_INDENT / indent_size` (= 30 with the default two-space
/// indent string).
///
/// # SAFETY
///
/// - `buf` must be a valid pointer to a mutable `_xmlBuffer`.
unsafe fn write_indent(
    buf: *mut _xmlBuffer,
    level: c_int,
    indent: *const xmlChar,
    indent_len: c_int,
) {
    if buf.is_null() || level <= 0 || indent.is_null() || indent_len <= 0 {
        return;
    }
    let indent_nr = MAX_INDENT / indent_len;
    let mut lvl = level;
    if lvl > indent_nr {
        lvl = indent_nr;
    }
    for _ in 0..lvl {
        io::buf_add(buf, indent, indent_len);
    }
}

/// Whether the save context should escape non-ASCII as hex references:
/// upstream xmlSaveWriteText sets XML_ESCAPE_NON_ASCII only when
/// `ctxt->encoding == NULL` after xmlSaveDocInternal fell back to the
/// document's own encoding and tried to switch an output encoder — a real
/// (non-native) document encoding gets a converter (no escape; the converter
/// produces the target bytes), while a NULL/UTF-8/US-ASCII encoding leaves
/// no encoder (escape). The candidate has no converters, so a declared
/// non-native encoding keeps the pre-existing pass-through.
///
/// # Safety
///
/// - `doc` may be NULL or a valid `_xmlDoc`.
pub(crate) fn save_escapes_non_ascii(save_encoding: *const xmlChar, doc: *mut _xmlDoc) -> bool {
    if !save_encoding.is_null() {
        return false;
    }
    if doc.is_null() || unsafe { (*doc).encoding }.is_null() {
        return true;
    }
    let enc = unsafe { (*doc).encoding };
    let name = unsafe { std::ffi::CStr::from_ptr(enc as *const c_char) }
        .to_string_lossy()
        .into_owned();
    let lower = name.to_ascii_lowercase();
    !(lower == "utf-8" || lower == "utf8" || lower == "us-ascii" || lower == "ascii")
}

/// True if the text node is marked as unescaped (`disable-output-escaping`).
///
/// # UPSTREAM-PARITY
///
/// Upstream compares `node->name == xmlStringTextNoenc` (pointer equality
/// against a static marker). Our trees carry the marker as a duplicated
/// `"textnoenc"` string, so we compare contents.
///
/// # Safety
///
/// - `node` must be NULL or a valid pointer to an `_xmlNode`.
/// - When `node.name` is non-NULL it must be a valid NUL-terminated `xmlChar`
///   string; `c_str_eq_bytes` scans it until the NUL byte.
unsafe fn is_noenc_text(node: *mut _xmlNode) -> bool {
    if node.is_null() {
        return false;
    }
    let n = unsafe { &*node };
    if n.name.is_null() {
        return false;
    }
    c_str_eq_bytes(n.name, b"textnoenc")
}

/// Compare a NUL-terminated xmlChar string with a byte slice.
///
/// # Safety
///
/// - `s` must be a valid pointer to a NUL-terminated `xmlChar` buffer; the
///   scan reads `b.len()` bytes plus the terminating NUL at `s[b.len()]`, so
///   the buffer must be at least `b.len() + 1` readable bytes.
const unsafe fn c_str_eq_bytes(s: *const xmlChar, b: &[u8]) -> bool {
    let mut i = 0usize;
    while i < b.len() {
        if unsafe { *s.add(i) } != b[i] {
            return false;
        }
        i += 1;
    }
    unsafe { *s.add(i) == 0 }
}

/// Serialization state mirroring the formatting state of libxml2's
/// `xmlSaveCtxt` (xmlsave.c 2.15).
#[derive(Clone, Copy)]
struct DumpState {
    /// `ctxt->format`: 0 = no formatting, 1 = XML_SAVE_FORMAT.
    format: c_int,
    /// The format value captured at dump entry; restored when leaving an
    /// element whose children disabled formatting (upstream local `format`).
    saved: c_int,
    /// The element whose children disabled formatting (upstream
    /// `unformattedNode`).
    unformatted: *mut _xmlNode,
    /// Per-context indent string (upstream `ctxt->indent`); NULL falls back
    /// to the default `xmlTreeIndentString`.
    indent: *const xmlChar,
    /// Byte length of `indent`.
    indent_len: c_int,
    /// Suppress the XML declaration (XML_SAVE_NO_DECL, upstream `no_decl`).
    no_decl: c_int,
    /// The encoding name for the XML declaration (upstream `ctxt->encoding`);
    /// NULL means "use the document's own encoding" (upstream
    /// `if (encoding == NULL) encoding = cur->encoding;`).
    encoding: *const xmlChar,
    /// True when this dump entered through the full save path
    /// (serialize_node_opts_enc_full — xmlSaveDoc / xmlNodeDumpOutput-style
    /// saves): only those honor the upstream XML_ESCAPE_NON_ASCII decision.
    /// Bare node dumps (xslt per-child output, debug helpers) keep the raw
    /// pass-through.
    explicit_save: bool,
    /// XHTML mode (upstream `xhtmlNodeDumpOutput`): the document's DTD is an
    /// XHTML DTD, so a bare `html` element gets
    /// `xmlns="http://www.w3.org/1999/xhtml"` and non-HTML-empty elements
    /// serialize as open/close instead of self-closing.
    xhtml: bool,
    /// HTML output mode (nokogiri SaveOptions::AS_HTML): empty HTML void
    /// elements stay `<br>`-style (no slash, no end tag) and other empty
    /// non-void elements serialize as `<a></a>`.
    as_html: bool,
    /// `XML_SAVE_NO_EMPTY`: empty non-void elements get an explicit end tag.
    no_empty: bool,
}

impl DumpState {
    const fn new(format: c_int) -> Self {
        let f = if format != 0 { 1 } else { 0 };
        DumpState {
            format: f,
            saved: f,
            unformatted: ptr::null_mut(),
            indent: INDENT.as_ptr(),
            indent_len: INDENT.len() as c_int,
            no_decl: 0,
            encoding: ptr::null(),
            xhtml: false,
            as_html: false,
            no_empty: false,
            explicit_save: false,
        }
    }

    /// Create a state with a custom indent string (xmlSaveSetIndentString),
    /// the XML_SAVE_NO_DECL option, and an encoding name for the XML
    /// declaration (upstream `ctxt->encoding`; NULL = use `doc->encoding`).
    ///
    /// When `indent` is NULL the caller's global `xmlTreeIndentString` is
    /// used (upstream xmlsave.c: `if (ctxt->indent == NULL) indent =
    /// xmlTreeIndentString`; nokogiri sets that global before xmlSaveToIO).
    ///
    /// # SAFETY
    ///
    /// - `indent` must be NULL or a valid NUL-terminated string that stays
    ///   alive for the whole dump.
    /// - `encoding` must be NULL or a valid NUL-terminated string that stays
    ///   alive for the whole dump.
    unsafe fn with_indent_enc(
        format: c_int,
        indent: *const xmlChar,
        no_decl: c_int,
        encoding: *const xmlChar,
    ) -> Self {
        let f = if format != 0 { 1 } else { 0 };
        let (ptr, len) = if indent.is_null() {
            let g = crate::xml::globals::get_tree_indent_string();
            if g.is_null() {
                (INDENT.as_ptr(), INDENT.len() as c_int)
            } else {
                let mut n = 0i32;
                while unsafe { *g.add(n as usize) } != 0 {
                    n += 1;
                }
                (g, n)
            }
        } else {
            let mut n = 0i32;
            while unsafe { *indent.add(n as usize) } != 0 {
                n += 1;
            }
            (indent, n)
        };
        DumpState {
            format: f,
            saved: f,
            unformatted: ptr::null_mut(),
            indent: ptr,
            indent_len: len,
            no_decl,
            encoding,
            as_html: false,
            no_empty: false,
            xhtml: false,
            explicit_save: false,
        }
    }
}

/// UPSTREAM-PARITY (tree.c xmlIsXHTML): whether the document's internal
/// subset is one of the XHTML 1.0 DTDs (strict, frameset or transitional).
/// The XML serializer (xmlsave.c xhtmlNodeDumpOutput) switches to XHTML mode
/// when this returns 1.
pub(crate) unsafe fn xml_is_xhtml(doc: *mut _xmlDoc) -> bool {
    unsafe {
        if doc.is_null() {
            return false;
        }
        let d = &*doc;
        if d.intSubset.is_null() {
            return false;
        }
        let dtd = &*d.intSubset;
        if !dtd.ExternalID.is_null()
            && (c_str_eq_bytes(dtd.ExternalID, b"-//W3C//DTD XHTML 1.0 Strict//EN")
                || c_str_eq_bytes(dtd.ExternalID, b"-//W3C//DTD XHTML 1.0 Frameset//EN")
                || c_str_eq_bytes(dtd.ExternalID, b"-//W3C//DTD XHTML 1.0 Transitional//EN"))
        {
            return true;
        }
        if !dtd.SystemID.is_null()
            && (c_str_eq_bytes(
                dtd.SystemID,
                b"http://www.w3.org/TR/xhtml1/DTD/xhtml1-strict.dtd",
            ) || c_str_eq_bytes(
                dtd.SystemID,
                b"http://www.w3.org/TR/xhtml1/DTD/xhtml1-frameset.dtd",
            ) || c_str_eq_bytes(
                dtd.SystemID,
                b"http://www.w3.org/TR/xhtml1/DTD/xhtml1-transitional.dtd",
            ))
        {
            return true;
        }
        false
    }
}

/// UPSTREAM-PARITY (xmlsave.c xhtmlIsEmpty): HTML empty elements which stay
/// self-closing in XHTML mode.
const unsafe fn xhtml_is_empty(name: *const xmlChar) -> bool {
    unsafe {
        if name.is_null() {
            return false;
        }
        c_str_eq_bytes(name, b"area")
            || c_str_eq_bytes(name, b"base")
            || c_str_eq_bytes(name, b"basefont")
            || c_str_eq_bytes(name, b"br")
            || c_str_eq_bytes(name, b"col")
            || c_str_eq_bytes(name, b"frame")
            || c_str_eq_bytes(name, b"hr")
            || c_str_eq_bytes(name, b"img")
            || c_str_eq_bytes(name, b"input")
            || c_str_eq_bytes(name, b"isindex")
            || c_str_eq_bytes(name, b"link")
            || c_str_eq_bytes(name, b"meta")
            || c_str_eq_bytes(name, b"param")
    }
}

/// Write an element/attribute name with its namespace prefix.
///
/// # SAFETY
///
/// - `buf` must be valid; `node` must be a valid element node.
unsafe fn write_qname(buf: *mut _xmlBuffer, node: *mut _xmlNode) {
    let n = unsafe { &*node };
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
}

/// Dump a local namespace definition (upstream `xmlNsDumpOutput`).
///
/// # SAFETY
///
/// - `buf` must be valid; `cur` must be a valid `_xmlNs`.
unsafe fn ns_dump_output(buf: *mut _xmlBuffer, cur: *mut _xmlNs) {
    if cur.is_null() || buf.is_null() {
        return;
    }
    let ns = unsafe { &*cur };
    if ns.type_ == XML_LOCAL_NAMESPACE as c_int && !ns.href.is_null() {
        // The xml namespace is implicit and never re-declared.
        if !ns.prefix.is_null() && c_str_eq_bytes(ns.prefix, b"xml") {
            return;
        }
        io::buf_ccat(buf, b' ');
        if !ns.prefix.is_null() {
            io::buf_add(buf, b"xmlns:" as *const u8, 6);
            io::buf_cat(buf, ns.prefix);
        } else {
            io::buf_add(buf, b"xmlns" as *const u8, 5);
        }
        io::buf_add(buf, b"=\"" as *const u8, 2);
        serialize_attr_value(buf, ns.href);
        io::buf_ccat(buf, b'"');
    }
}

/// Dump an attribute node (upstream `xmlAttrDumpOutput`).
///
/// `escape_non_ascii` mirrors xmlsave.c `xmlSaveWriteText`'s
/// `XML_ESCAPE_NON_ASCII` flag: attribute CONTENT is escaped as hex char
/// references when the save context has no output encoder.
///
/// # SAFETY
///
/// - `buf` must be valid; `cur` must be a valid `_xmlAttr`.
unsafe fn attr_dump_output(buf: *mut _xmlBuffer, cur: *mut _xmlAttr, escape_non_ascii: bool) {
    if cur.is_null() || buf.is_null() {
        return;
    }
    io::buf_ccat(buf, b' ');
    let a = unsafe { &*cur };
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
    // Attribute content: text children are escaped, entity references are
    // emitted as `&name;` (upstream `xmlSaveWriteAttrContent`).
    let mut child = a.children;
    while !child.is_null() {
        let ct = unsafe { (*child).type_ };
        if ct == XML_TEXT_NODE as c_int && !unsafe { (*child).content }.is_null() {
            serialize_attr_value_flags(buf, unsafe { (*child).content }, escape_non_ascii);
        } else if ct == XML_ENTITY_REF_NODE as c_int && !unsafe { (*child).name }.is_null() {
            io::buf_ccat(buf, b'&');
            io::buf_cat(buf, unsafe { (*child).name });
            io::buf_ccat(buf, b';');
        }
        child = unsafe { (*child).next };
    }
    io::buf_ccat(buf, b'"');
}

/// Dump a notation declaration (upstream `xmlBufDumpNotationDecl`).
///
/// # SAFETY
///
/// - `buf` must be valid; `nota` must be a valid `_xmlNotation`.
unsafe fn dump_notation_decl(buf: *mut _xmlBuffer, nota: *mut _xmlNotation) {
    let n = unsafe { &*nota };
    io::buf_add(buf, b"<!NOTATION " as *const u8, 11);
    if !n.name.is_null() {
        io::buf_cat(buf, n.name);
    }
    if !n.PublicID.is_null() {
        io::buf_add(buf, b" PUBLIC " as *const u8, 8);
        write_quoted_string(buf, n.PublicID);
        if !n.SystemID.is_null() {
            io::buf_ccat(buf, b' ');
            write_quoted_string(buf, n.SystemID);
        }
    } else {
        io::buf_add(buf, b" SYSTEM " as *const u8, 8);
        write_quoted_string(buf, n.SystemID);
    }
    io::buf_add(buf, b" >\n" as *const u8, 4);
}

/// Dump an occurrence operator (upstream `xmlBufDumpElementOccur`).
unsafe fn dump_element_occur(buf: *mut _xmlBuffer, ocur: c_int) {
    use crate::abi::types::xmlElementContentOccur::*;
    if ocur == XML_ELEMENT_CONTENT_OPT as c_int {
        io::buf_ccat(buf, b'?');
    } else if ocur == XML_ELEMENT_CONTENT_MULT as c_int {
        io::buf_ccat(buf, b'*');
    } else if ocur == XML_ELEMENT_CONTENT_PLUS as c_int {
        io::buf_ccat(buf, b'+');
    }
}

/// Dump an element content model (upstream `xmlBufDumpElementContent`).
///
/// # SAFETY
///
/// - `buf` must be valid; `content` must be a valid content tree or NULL.
unsafe fn dump_element_content(buf: *mut _xmlBuffer, content: *mut _xmlElementContent) {
    use crate::abi::types::xmlElementContentOccur::*;
    use crate::abi::types::xmlElementContentType::*;
    if content.is_null() {
        return;
    }
    io::buf_ccat(buf, b'(');
    let mut cur = content;
    loop {
        if cur.is_null() {
            return;
        }
        let c = unsafe { &*cur };
        match c.type_ {
            t if t == XML_ELEMENT_CONTENT_PCDATA as c_int => {
                io::buf_add(buf, b"#PCDATA" as *const u8, 7);
            }
            t if t == XML_ELEMENT_CONTENT_ELEMENT as c_int => {
                if !c.prefix.is_null() {
                    io::buf_cat(buf, c.prefix);
                    io::buf_ccat(buf, b':');
                }
                if !c.name.is_null() {
                    io::buf_cat(buf, c.name);
                }
            }
            t if t == XML_ELEMENT_CONTENT_SEQ as c_int || t == XML_ELEMENT_CONTENT_OR as c_int => {
                if cur != content
                    && !c.parent.is_null()
                    && (c.type_ != unsafe { (*c.parent).type_ }
                        || c.ocur != XML_ELEMENT_CONTENT_ONCE as c_int)
                {
                    io::buf_ccat(buf, b'(');
                }
                cur = c.c1;
                continue;
            }
            _ => {}
        }

        // Walk up until we find the next sibling to process.
        while cur != content {
            let ccur = unsafe { &*cur };
            let parent = ccur.parent;
            if parent.is_null() {
                return;
            }
            let p = unsafe { &*parent };
            if (ccur.type_ == XML_ELEMENT_CONTENT_OR as c_int
                || ccur.type_ == XML_ELEMENT_CONTENT_SEQ as c_int)
                && (ccur.type_ != p.type_ || ccur.ocur != XML_ELEMENT_CONTENT_ONCE as c_int)
            {
                io::buf_ccat(buf, b')');
            }
            dump_element_occur(buf, ccur.ocur);

            if cur == p.c1 {
                // UPSTREAM-PARITY (xmlsave.c xmlBufDumpElementContent): the
                // " , "/" | " separator belongs to the PARENT combinator and
                // is written when ascending from the c1 subtree to c2 — a
                // plain-element c1 must still separate (a sequence like
                // (title, author) previously lost its separators entirely).
                if p.type_ == XML_ELEMENT_CONTENT_SEQ as c_int {
                    io::buf_add(buf, b" , " as *const u8, 3);
                } else if p.type_ == XML_ELEMENT_CONTENT_OR as c_int {
                    io::buf_add(buf, b" | " as *const u8, 3);
                }
                cur = p.c2;
                break;
            }
            cur = parent;
        }
        if cur == content {
            break;
        }
    }
    io::buf_ccat(buf, b')');
    let cc = unsafe { &*content };
    dump_element_occur(buf, cc.ocur);
}

/// Dump an element declaration (upstream `xmlBufDumpElementDecl`).
///
/// # SAFETY
///
/// - `buf` must be valid; `elem` must be a valid `_xmlElement`.
unsafe fn dump_element_decl(buf: *mut _xmlBuffer, elem: *mut _xmlElement) {
    use crate::abi::types::xmlElementTypeVal::*;
    let e = unsafe { &*elem };
    io::buf_add(buf, b"<!ELEMENT " as *const u8, 10);
    if !e.prefix.is_null() {
        io::buf_cat(buf, e.prefix);
        io::buf_ccat(buf, b':');
    }
    if !e.name.is_null() {
        io::buf_cat(buf, e.name);
    }
    io::buf_ccat(buf, b' ');
    match e.etype {
        t if t == XML_ELEMENT_TYPE_EMPTY as c_int => {
            io::buf_add(buf, b"EMPTY" as *const u8, 5);
        }
        t if t == XML_ELEMENT_TYPE_ANY as c_int => {
            io::buf_add(buf, b"ANY" as *const u8, 3);
        }
        t if t == XML_ELEMENT_TYPE_MIXED as c_int || t == XML_ELEMENT_TYPE_ELEMENT as c_int => {
            dump_element_content(buf, e.content);
        }
        _ => {}
    }
    io::buf_add(buf, b">\n" as *const u8, 2);
}

/// Dump an enumeration (upstream `xmlBufDumpEnumeration`).
///
/// # SAFETY
///
/// - `buf` must be valid; `cur` must be a valid enumeration or NULL.
unsafe fn dump_enumeration(buf: *mut _xmlBuffer, cur: *mut _xmlEnumeration) {
    let mut e = cur;
    while !e.is_null() {
        let en = unsafe { &*e };
        if !en.name.is_null() {
            io::buf_cat(buf, en.name);
        }
        if !en.next.is_null() {
            io::buf_add(buf, b" | " as *const u8, 3);
        }
        e = en.next;
    }
    io::buf_ccat(buf, b')');
}

/// Dump an attribute declaration (upstream `xmlSaveWriteAttributeDecl`).
///
/// # SAFETY
///
/// - `buf` must be valid; `attr` must be a valid `_xmlAttribute` decl.
unsafe fn dump_attribute_decl(buf: *mut _xmlBuffer, attr: *mut _xmlAttribute) {
    use crate::abi::types::xmlAttributeDefault::*;
    use crate::abi::types::xmlAttributeType::*;
    let a = unsafe { &*attr };
    io::buf_add(buf, b"<!ATTLIST " as *const u8, 10);
    if !a.elem.is_null() {
        io::buf_cat(buf, a.elem);
    }
    io::buf_ccat(buf, b' ');
    if !a.prefix.is_null() {
        io::buf_cat(buf, a.prefix);
        io::buf_ccat(buf, b':');
    }
    if !a.name.is_null() {
        io::buf_cat(buf, a.name);
    }
    match a.atype {
        t if t == XML_ATTRIBUTE_CDATA as c_int => {
            io::buf_add(buf, b" CDATA" as *const u8, 6);
        }
        t if t == XML_ATTRIBUTE_ID as c_int => {
            io::buf_add(buf, b" ID" as *const u8, 3);
        }
        t if t == XML_ATTRIBUTE_IDREF as c_int => {
            io::buf_add(buf, b" IDREF" as *const u8, 6);
        }
        t if t == XML_ATTRIBUTE_IDREFS as c_int => {
            io::buf_add(buf, b" IDREFS" as *const u8, 7);
        }
        t if t == XML_ATTRIBUTE_ENTITY as c_int => {
            io::buf_add(buf, b" ENTITY" as *const u8, 7);
        }
        t if t == XML_ATTRIBUTE_ENTITIES as c_int => {
            io::buf_add(buf, b" ENTITIES" as *const u8, 9);
        }
        t if t == XML_ATTRIBUTE_NMTOKEN as c_int => {
            io::buf_add(buf, b" NMTOKEN" as *const u8, 8);
        }
        t if t == XML_ATTRIBUTE_NMTOKENS as c_int => {
            io::buf_add(buf, b" NMTOKENS" as *const u8, 9);
        }
        t if t == XML_ATTRIBUTE_ENUMERATION as c_int => {
            io::buf_add(buf, b" (" as *const u8, 2);
            dump_enumeration(buf, a.tree);
        }
        t if t == XML_ATTRIBUTE_NOTATION as c_int => {
            io::buf_add(buf, b" NOTATION (" as *const u8, 11);
            dump_enumeration(buf, a.tree);
        }
        _ => {}
    }
    match a.def {
        t if t == XML_ATTRIBUTE_REQUIRED as c_int => {
            io::buf_add(buf, b" #REQUIRED" as *const u8, 10);
        }
        t if t == XML_ATTRIBUTE_IMPLIED as c_int => {
            io::buf_add(buf, b" #IMPLIED" as *const u8, 9);
        }
        t if t == XML_ATTRIBUTE_FIXED as c_int => {
            io::buf_add(buf, b" #FIXED" as *const u8, 7);
        }
        _ => {}
    }
    if !a.defaultValue.is_null() {
        io::buf_add(buf, b" \"" as *const u8, 2);
        serialize_attr_value(buf, a.defaultValue);
        io::buf_ccat(buf, b'"');
    }
    io::buf_add(buf, b">\n" as *const u8, 2);
}

/// Write a quoted string (upstream `xmlOutputBufferWriteQuotedString`).
///
/// # SAFETY
///
/// - `buf` must be valid; `str` must be a valid NUL-terminated string.
unsafe fn write_quoted_string(buf: *mut _xmlBuffer, str: *const xmlChar) {
    if buf.is_null() {
        return;
    }
    io::buf_ccat(buf, b'"');
    if !str.is_null() {
        let mut i = 0usize;
        while unsafe { *str.add(i) != 0 } {
            let ch = unsafe { *str.add(i) };
            if ch == b'"' {
                io::buf_add(buf, b"&quot;" as *const u8, 6);
            } else {
                io::buf_add(buf, &ch as *const u8, 1);
            }
            i += 1;
        }
    }
    io::buf_ccat(buf, b'"');
}

/// Dump an entity declaration (upstream `xmlBufDumpEntityDecl`).
///
/// # SAFETY
///
/// - `buf` must be valid; `ent` must be a valid `_xmlEntity` decl.
unsafe fn dump_entity_decl(buf: *mut _xmlBuffer, ent: *mut _xmlEntity) {
    use crate::abi::types::xmlEntityType::*;
    let e = unsafe { &*ent };
    if e.etype == XML_INTERNAL_PARAMETER_ENTITY as c_int
        || e.etype == XML_EXTERNAL_PARAMETER_ENTITY as c_int
    {
        io::buf_add(buf, b"<!ENTITY % " as *const u8, 11);
    } else {
        io::buf_add(buf, b"<!ENTITY " as *const u8, 9);
    }
    if !e.name.is_null() {
        io::buf_cat(buf, e.name);
    }
    io::buf_ccat(buf, b' ');

    if e.etype == XML_EXTERNAL_GENERAL_PARSED_ENTITY as c_int
        || e.etype == XML_EXTERNAL_GENERAL_UNPARSED_ENTITY as c_int
        || e.etype == XML_EXTERNAL_PARAMETER_ENTITY as c_int
    {
        if !e.ExternalID.is_null() {
            io::buf_add(buf, b"PUBLIC " as *const u8, 7);
            write_quoted_string(buf, e.ExternalID);
            io::buf_ccat(buf, b' ');
        } else {
            io::buf_add(buf, b"SYSTEM " as *const u8, 7);
        }
        write_quoted_string(buf, e.SystemID);
    }

    if e.etype == XML_EXTERNAL_GENERAL_UNPARSED_ENTITY as c_int && !e.content.is_null() {
        io::buf_add(buf, b" NDATA " as *const u8, 7);
        if !e.orig.is_null() {
            io::buf_cat(buf, e.orig);
        } else if !e.content.is_null() {
            io::buf_cat(buf, e.content);
        }
    }

    if e.etype == XML_INTERNAL_GENERAL_ENTITY as c_int
        || e.etype == XML_INTERNAL_PARAMETER_ENTITY as c_int
    {
        if !e.orig.is_null() {
            write_quoted_string(buf, e.orig);
        } else {
            // Entity content is quoted, escaping `"` and `%`.
            io::buf_ccat(buf, b'"');
            if !e.content.is_null() {
                let mut i = 0usize;
                while unsafe { *e.content.add(i) != 0 } {
                    let ch = unsafe { *e.content.add(i) };
                    match ch {
                        b'"' => io::buf_add(buf, b"&quot;" as *const u8, 6),
                        b'%' => io::buf_add(buf, b"&#x25;" as *const u8, 6),
                        _ => io::buf_add(buf, &ch as *const u8, 1),
                    };
                    i += 1;
                }
            }
            io::buf_ccat(buf, b'"');
        }
    }
    io::buf_add(buf, b">\n" as *const u8, 2);
}

/// Dump a DTD node (upstream `xmlDtdDumpOutput`).
///
/// # SAFETY
///
/// - `buf` must be valid; `cur` must be a valid DTD node.
unsafe fn dtd_dump_output(
    buf: *mut _xmlBuffer,
    cur: *mut _xmlNode,
    state: &mut DumpState,
    level: &mut c_int,
) {
    let dtd = cur as *mut _xmlDtd;
    let d = unsafe { &*dtd };
    io::buf_add(buf, b"<!DOCTYPE " as *const u8, 10);
    if !d.name.is_null() {
        io::buf_cat(buf, d.name);
    }
    if !d.ExternalID.is_null() {
        io::buf_add(buf, b" PUBLIC " as *const u8, 8);
        write_quoted_string(buf, d.ExternalID);
        io::buf_ccat(buf, b' ');
        write_quoted_string(buf, d.SystemID);
    } else if !d.SystemID.is_null() {
        io::buf_add(buf, b" SYSTEM " as *const u8, 8);
        write_quoted_string(buf, d.SystemID);
    }
    // UPSTREAM-PARITY (xmlsave.c xmlDtdDumpOutput): the internal-subset
    // brackets are written only when a declaration table is non-NULL — an
    // empty DTD (all tables NULL) emits `>`. (hash_size() cannot be used
    // here: it returns -1 for NULL tables.)
    if d.entities.is_null()
        && d.elements.is_null()
        && d.attributes.is_null()
        && d.notations.is_null()
        && d.pentities.is_null()
    {
        io::buf_ccat(buf, b'>');
        return;
    }
    io::buf_add(buf, b" [\n" as *const u8, 3);
    // UPSTREAM-PARITY (xmlsave.c xmlDtdDumpOutput): the internal-subset
    // declarations are dumped from the DTD NODE's children list — the
    // declaration nodes are linked there in declaration order — NOT from the
    // hash tables (which upstream also keeps, but only for lookups; notations
    // are hash-only because they never join the child list). The old
    // hash-scan emitted declarations in hash-bucket order, which reversed
    // multi-declaration files (RESIDUAL R-DTD-DUMP-ORDER; ext/dom +
    // ext/simplexml xml_parsing_LIBXML_NO_XXE show `xxe` before `foo`).
    let format = state.format;
    let lvl = *level;
    state.format = 0;
    *level = -1;
    // Notations first: upstream xmlBufDumpNotationTable (hash-only table).
    if !d.notations.is_null() {
        crate::xml::hash::hash_scan(
            d.notations as *mut crate::xml::hash::HashTable,
            Some(dump_notation_decl_cb),
            buf as *mut c_void,
        );
    }
    // Declarations in child-list (declaration) order: element, attribute,
    // entity and parameter-entity declaration nodes.
    let mut decl = d.children;
    while !decl.is_null() {
        let dt = unsafe { (*decl).type_ };
        match dt {
            t if t == XML_ELEMENT_DECL as c_int => {
                dump_element_decl(buf, decl as *mut _xmlElement);
            }
            t if t == XML_ATTRIBUTE_DECL as c_int => {
                dump_attribute_decl(buf, decl as *mut _xmlAttribute);
            }
            t if t == XML_ENTITY_DECL as c_int => {
                dump_entity_decl(buf, decl as *mut _xmlEntity);
            }
            _ => {}
        }
        decl = unsafe { (*decl).next };
    }
    state.format = format;
    *level = lvl;
    io::buf_add(buf, b"]>" as *const u8, 2);
}

/// Hash-scan callback for notation declarations.
unsafe extern "C" fn dump_notation_decl_cb(
    payload: *mut c_void,
    data: *mut c_void,
    _name: *const crate::abi::types::xmlChar,
) {
    if !payload.is_null() && !data.is_null() {
        dump_notation_decl(data as *mut _xmlBuffer, payload as *mut _xmlNotation);
    }
}

/// Dump the content of a document (upstream `xmlSaveDocInternal`, XML path).
///
/// Writes the XML declaration (when not suppressed) followed by each child
/// separated by a newline.
///
/// # SAFETY
///
/// - `buf` must be valid; `cur` must be a valid document node.
unsafe fn doc_content_dump_output(
    buf: *mut _xmlBuffer,
    cur: *mut _xmlNode,
    state: &mut DumpState,
    level: &mut c_int,
) {
    let doc = cur as *mut _xmlDoc;
    let d = unsafe { &*doc };

    // XML declaration: `<?xml version="..."?>` plus the encoding and
    // standalone attributes. The encoding is the save-context encoding when
    // one is set (upstream `ctxt->encoding`), falling back to the
    // document's own encoding (upstream xmlsave.c xmlSaveDocInternal:
    // `if (encoding == NULL) encoding = cur->encoding;`). Suppressed by the
    // XML_SAVE_NO_DECL save option (upstream `no_decl`).
    if state.no_decl == 0 {
        io::buf_add(buf, b"<?xml version=\"" as *const u8, 15);
        if !d.version.is_null() {
            io::buf_cat(buf, d.version);
        } else {
            io::buf_add(buf, b"1.0" as *const u8, 3);
        }
        io::buf_ccat(buf, b'"');
        let enc = if state.encoding.is_null() {
            d.encoding
        } else {
            state.encoding
        };
        if !enc.is_null() {
            io::buf_add(buf, b" encoding=\"" as *const u8, 11);
            io::buf_cat(buf, enc);
            io::buf_ccat(buf, b'"');
        }
        match d.standalone {
            0 => {
                io::buf_add(buf, b" standalone=\"no\"" as *const u8, 16);
            }
            1 => {
                io::buf_add(buf, b" standalone=\"yes\"" as *const u8, 17);
            }
            _ => {}
        }
        io::buf_add(buf, b"?>\n" as *const u8, 3);
    }

    // UPSTREAM-PARITY (xmlsave.c xmlSaveDocInternal): the internal subset
    // DTD is a member of the children chain (xmlCreateIntSubset inserts it
    // before the first element), and the children loop below dumps it once.
    // Construction paths that keep the DTD only on doc->intSubset
    // (xmlCopyDoc, lazily-created subsets) need the explicit dump. Never
    // dump both — that double-prints <!DOCTYPE>. When doc->children is
    // EMPTY the DTD must NOT be dumped either: php's modern serializer
    // temporarily NULLs doc->children around xmlSaveDoc (to get a
    // declaration-only pass) and re-dumps the children itself — dumping the
    // intSubset there produced a duplicated <!DOCTYPE> (ext/dom +
    // ext/simplexml xml_parsing_LIBXML_NO_XXE).
    if !d.children.is_null() && !d.intSubset.is_null() {
        let mut in_chain = false;
        let mut c = d.children;
        while !c.is_null() {
            if c as *mut c_void == d.intSubset as *mut c_void {
                in_chain = true;
                break;
            }
            c = unsafe { (*c).next };
        }
        if !in_chain {
            let mut lvl = 0;
            dtd_dump_output(buf, d.intSubset as *mut _xmlNode, state, &mut lvl);
            io::buf_ccat(buf, b'\n');
        }
    }

    if !d.children.is_null() {
        let mut child = d.children;
        while !child.is_null() {
            *level = 0;
            node_dump_internal(buf, child, child, cur, state, level);
            let ct = unsafe { (*child).type_ };
            if ct != XML_XINCLUDE_START as c_int && ct != XML_XINCLUDE_END as c_int {
                io::buf_ccat(buf, b'\n');
            }
            child = unsafe { (*child).next };
        }
    }
}

/// Faithful port of libxml2's `xmlNodeDumpOutputInternal` (xmlsave.c 2.15).
///
/// Serializes `cur` and its descendants into `buf`. `root` is the node this
/// invocation started with: the root node itself is never indented, and no
/// trailing separator is emitted for it (the caller separates siblings).
/// `parent` is the expected parent of `cur`, used by the corrupted-tree
/// fallback.
///
/// # UPSTREAM-PARITY
///
/// - Indentation (two spaces per level, capped at 30 levels) is written
///   before every non-root element, PI and comment when formatting.
/// - An element whose children include a text, CDATA or entity-reference
///   node disables formatting for its whole content (the `unformattedNode`
///   mechanism); formatting is restored when its closing tag is emitted.
/// - `\n` separators between siblings are emitted after every child of a
///   formatted element (the upstream unwind loop).
///
/// # SAFETY
///
/// - `buf` must be a valid pointer to a mutable `_xmlBuffer`.
/// - `cur` must be a valid node pointer; `root`/`parent` must be stable
///   pointers into the same tree.
unsafe fn node_dump_internal(
    buf: *mut _xmlBuffer,
    cur: *mut _xmlNode,
    root: *mut _xmlNode,
    parent: *mut _xmlNode,
    state: &mut DumpState,
    level: &mut c_int,
) {
    if cur.is_null() || buf.is_null() {
        return;
    }
    let n = unsafe { &*cur };
    match n.type_ {
        t if t == XML_ELEMENT_NODE as c_int => {
            if cur != root && state.format == 1 {
                write_indent(buf, *level, state.indent, state.indent_len);
            }
            // Corrupted-tree fallback (upstream handles nodes passed with a
            // broken parent link by dumping the subtree as its own root).
            if !n.parent.is_null() && n.parent != parent && !n.children.is_null() {
                let mut sub = DumpState::new(state.format);
                let mut sub_level = *level;
                node_dump_internal(buf, cur, cur, n.parent, &mut sub, &mut sub_level);
                return;
            }
            // Start tag.
            io::buf_ccat(buf, b'<');
            write_qname(buf, cur);
            // UPSTREAM-PARITY (xmlsave.c xhtmlNodeDumpOutput): in XHTML mode
            // a bare <html> element (no ns, no nsDef) gets the XHTML default
            // namespace declaration.
            if state.xhtml
                && !n.name.is_null()
                && c_str_eq_bytes(n.name, b"html")
                && n.ns.is_null()
                && n.nsDef.is_null()
            {
                io::buf_add(
                    buf,
                    b" xmlns=\"http://www.w3.org/1999/xhtml\"" as *const u8,
                    37,
                );
            }
            let mut nsdef = n.nsDef;
            while !nsdef.is_null() {
                ns_dump_output(buf, nsdef);
                nsdef = unsafe { (*nsdef).next };
            }
            let mut attr = n.properties;
            while !attr.is_null() {
                attr_dump_output(
                    buf,
                    attr,
                    state.explicit_save
                        && !state.as_html
                        && save_escapes_non_ascii(state.encoding, unsafe { (*attr).doc }),
                );
                attr = unsafe { (*attr).next };
            }
            if n.children.is_null() {
                if state.as_html {
                    // UPSTREAM-PARITY (nokogiri to_html / AS_HTML save): in
                    // HTML output an empty HTML void element stays
                    // `<br>`-style (no slash, no end tag); any other empty
                    // element serializes as `<a></a>`.
                    if xhtml_is_empty(n.name) {
                        io::buf_ccat(buf, b'>');
                    } else {
                        io::buf_ccat(buf, b'>');
                        io::buf_add(buf, b"</" as *const u8, 2);
                        write_qname(buf, cur);
                        io::buf_ccat(buf, b'>');
                    }
                } else if state.no_empty {
                    // UPSTREAM-PARITY (xmlsave.c XML_SAVE_NO_EMPTY): empty
                    // elements get an explicit end tag.
                    io::buf_ccat(buf, b'>');
                    io::buf_add(buf, b"</" as *const u8, 2);
                    write_qname(buf, cur);
                    io::buf_ccat(buf, b'>');
                } else if state.xhtml && !xhtml_is_empty(n.name) {
                    // UPSTREAM-PARITY (xhtmlNodeDumpOutput C.2): in XHTML
                    // mode only the HTML-empty elements stay self-closing;
                    // everything else (e.g. <html>) serializes as
                    // open/close.
                    io::buf_ccat(buf, b'>');
                    io::buf_add(buf, b"</" as *const u8, 2);
                    write_qname(buf, cur);
                    io::buf_ccat(buf, b'>');
                } else {
                    io::buf_add(buf, b"/>" as *const u8, 2);
                }
            } else {
                if state.format == 1 {
                    // An element with text/CDATA/entity-ref children is
                    // serialized unformatted (upstream unformattedNode).
                    let mut tmp = n.children;
                    while !tmp.is_null() {
                        let tt = unsafe { (*tmp).type_ };
                        if tt == XML_TEXT_NODE as c_int
                            || tt == XML_CDATA_SECTION_NODE as c_int
                            || tt == XML_ENTITY_REF_NODE as c_int
                        {
                            state.format = 0;
                            state.unformatted = cur;
                            break;
                        }
                        tmp = unsafe { (*tmp).next };
                    }
                }
                io::buf_ccat(buf, b'>');
                if state.format == 1 {
                    io::buf_ccat(buf, b'\n');
                }
                if *level >= 0 {
                    *level += 1;
                }
                let mut child = n.children;
                while !child.is_null() {
                    node_dump_internal(buf, child, root, cur, state, level);
                    if state.format == 1 {
                        let ct = unsafe { (*child).type_ };
                        if ct != XML_XINCLUDE_START as c_int && ct != XML_XINCLUDE_END as c_int {
                            io::buf_ccat(buf, b'\n');
                        }
                    }
                    child = unsafe { (*child).next };
                }
                // Closing tag.
                if *level > 0 {
                    *level -= 1;
                }
                if state.format == 1 {
                    write_indent(buf, *level, state.indent, state.indent_len);
                }
                io::buf_add(buf, b"</" as *const u8, 2);
                write_qname(buf, cur);
                io::buf_ccat(buf, b'>');
                if cur == state.unformatted {
                    state.format = state.saved;
                    state.unformatted = ptr::null_mut();
                }
            }
        }
        t if t == XML_TEXT_NODE as c_int => {
            // UPSTREAM-PARITY (xmlsave.c xmlSaveWriteText): with no output
            // encoder on the save context, non-ASCII text is written as hex
            // character references. HTML-method output (XSLT method=html /
            // AS_HTML) writes raw like upstream's HTML serializer.
            let esc = state.explicit_save
                && !state.as_html
                && save_escapes_non_ascii(state.encoding, n.doc);
            if !n.content.is_null() {
                if is_noenc_text(cur) {
                    io::buf_cat(buf, n.content);
                } else {
                    serialize_text_flags(buf, n.content, xml_strlen(n.content), esc);
                }
            } else if !n.children.is_null() {
                // Non-compact text node (entity merge): content lives in a
                // child text node.
                let c = node_get_content(cur);
                if !c.is_null() {
                    if is_noenc_text(cur) {
                        io::buf_cat(buf, c);
                    } else {
                        serialize_text_flags(buf, c, xml_strlen(c), esc);
                    }
                    allocator::xmlFreeImpl(c as *mut c_void);
                }
            }
        }
        t if t == XML_CDATA_SECTION_NODE as c_int => {
            if n.content.is_null() || unsafe { *n.content == 0 } {
                io::buf_add(buf, b"<![CDATA[]]>" as *const u8, 12);
            } else {
                let len = xml_strlen(n.content) as usize;
                let bytes = core::slice::from_raw_parts(n.content, len);
                let mut i = 0usize;
                let mut seg_start = 0usize;
                while i < len {
                    if bytes[i] == b']'
                        && i + 2 < len
                        && bytes[i + 1] == b']'
                        && bytes[i + 2] == b'>'
                    {
                        io::buf_add(buf, b"<![CDATA[" as *const u8, 9);
                        io::buf_add(buf, n.content.add(seg_start), (i + 2 - seg_start) as c_int);
                        io::buf_add(buf, b"]]>" as *const u8, 3);
                        seg_start = i + 2;
                        i += 3;
                        continue;
                    }
                    i += 1;
                }
                if seg_start < len {
                    io::buf_add(buf, b"<![CDATA[" as *const u8, 9);
                    io::buf_add(buf, n.content.add(seg_start), (len - seg_start) as c_int);
                    io::buf_add(buf, b"]]>" as *const u8, 3);
                }
            }
        }
        t if t == XML_COMMENT_NODE as c_int => {
            if cur != root && state.format == 1 {
                write_indent(buf, *level, state.indent, state.indent_len);
            }
            if !n.content.is_null() {
                io::buf_add(buf, b"<!--" as *const u8, 4);
                io::buf_cat(buf, n.content);
                io::buf_add(buf, b"-->" as *const u8, 3);
            }
        }
        t if t == XML_PI_NODE as c_int => {
            if cur != root && state.format == 1 {
                write_indent(buf, *level, state.indent, state.indent_len);
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
        t if t == XML_ENTITY_REF_NODE as c_int => {
            io::buf_ccat(buf, b'&');
            if !n.name.is_null() {
                io::buf_cat(buf, n.name);
            }
            io::buf_ccat(buf, b';');
        }
        t if t == XML_DOCUMENT_FRAG_NODE as c_int => {
            // UPSTREAM-PARITY (xmlsave.c 2.15 xmlNodeDumpOutputInternal
            // `case XML_DOCUMENT_FRAG_NODE:`): a document fragment is a
            // transparent container. Upstream trampolines from the fragment
            // into its children (validated against `cur->parent`) and dumps
            // them as free siblings — the fragment emits no tags, no XML
            // declaration, and no indentation of its own. Children keep the
            // caller/level semantics of the enclosing context.
            let mut child = n.children;
            while !child.is_null() {
                // Each child is dumped as its own root so a fragment's first
                // child gets no leading indent and the caller separates
                // sibling content (mirrors doc children handling).
                let mut sublevel = *level;
                node_dump_internal(buf, child, child, cur, state, &mut sublevel);
                child = unsafe { (*child).next };
            }
        }
        t if t == XML_DOCUMENT_NODE as c_int => {
            doc_content_dump_output(buf, cur, state, level);
        }
        t if t == XML_HTML_DOCUMENT_NODE as c_int => {
            // UPSTREAM-PARITY (xmlsave.c xmlSaveDocInternal): an HTML
            // document is serialized by the HTML serializer only when an
            // HTML/XHTML save was requested. Saved as XML (PHP
            // DOMDocument::saveXML passes XML_SAVE_AS_XML), the document is
            // dumped by the XML serializer — XML declaration (with
            // doc->standalone), XML escaping — which is what makes a
            // loadHTML()'d document saveXml() print `<?xml version="1.0"
            // standalone="yes"?>` (ext/dom dom005/gh15670/gh16535/...).
            if state.as_html {
                crate::xml::html::serialize_node(cur, buf, state.format, *level);
            } else {
                doc_content_dump_output(buf, cur, state, level);
            }
        }
        t if t == XML_DTD_NODE as c_int => {
            dtd_dump_output(buf, cur, state, level);
        }
        t if t == XML_ATTRIBUTE_NODE as c_int => {
            attr_dump_output(
                buf,
                cur as *mut _xmlAttr,
                save_escapes_non_ascii(state.encoding, unsafe { (*cur).doc }),
            );
        }
        t if t == XML_NAMESPACE_DECL as c_int => {
            ns_dump_output(buf, cur as *mut _xmlNs);
        }
        t if t == XML_ELEMENT_DECL as c_int => {
            dump_element_decl(buf, cur as *mut _xmlElement);
        }
        t if t == XML_ATTRIBUTE_DECL as c_int => {
            dump_attribute_decl(buf, cur as *mut _xmlAttribute);
        }
        t if t == XML_ENTITY_DECL as c_int => {
            dump_entity_decl(buf, cur as *mut _xmlEntity);
        }
        _ => {}
    }
}

/// Recursively serialize a node tree to a buffer.
///
/// `buf` is an `_xmlBuffer*`, `format` controls indentation (non-zero = pretty-print).
///
/// # UPSTREAM-PARITY
///
/// Mirrors `xmlNodeDumpOutputInternal` (xmlsave.c 2.15): the node is treated
/// as the root of the dump (no leading indentation, no trailing separator).
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
    unsafe { serialize_node_opt(node, buf, format, level, ptr::null()) };
}

/// Like `serialize_node`, with a per-context indent string
/// (xmlSaveSetIndentString); NULL indent uses the default.
///
/// # SAFETY
///
/// - `indent` must be NULL or a valid NUL-terminated string that stays
///   alive for the whole dump.
pub(crate) unsafe fn serialize_node_opt(
    node: *mut _xmlNode,
    buf: *mut _xmlBuffer,
    format: c_int,
    level: c_int,
    indent: *const xmlChar,
) {
    unsafe { serialize_node_opts(node, buf, format, level, indent, 0) };
}

/// Like `serialize_node_opt`, plus the XML_SAVE_NO_DECL flag.
///
/// # SAFETY
///
/// - `indent` must be NULL or a valid NUL-terminated string that stays
///   alive for the whole dump.
pub(crate) unsafe fn serialize_node_opts(
    node: *mut _xmlNode,
    buf: *mut _xmlBuffer,
    format: c_int,
    level: c_int,
    indent: *const xmlChar,
    no_decl: c_int,
) {
    unsafe { serialize_node_opts_enc(node, buf, format, level, indent, no_decl, ptr::null()) };
}

/// Like `serialize_node_opts`, plus an encoding name for the XML
/// declaration (upstream `ctxt->encoding`; NULL = use `doc->encoding`).
///
/// # SAFETY
///
/// - `indent` must be NULL or a valid NUL-terminated string that stays
///   alive for the whole dump.
/// - `encoding` must be NULL or a valid NUL-terminated string that stays
///   alive for the whole dump.
pub(crate) unsafe fn serialize_node_opts_enc(
    node: *mut _xmlNode,
    buf: *mut _xmlBuffer,
    format: c_int,
    level: c_int,
    indent: *const xmlChar,
    no_decl: c_int,
    encoding: *const xmlChar,
) {
    unsafe {
        serialize_node_opts_xhtml(node, buf, format, level, indent, no_decl, encoding, false)
    };
}

/// Serialize a node with the full save-option set (node, buffer, format,
/// level, indent, no-declaration, no-empty-tags, HTML mode, encoding).
/// Mirrors the fields nokogiri's `xmlSaveToIO`/`xmlSaveTree` path threads so
/// HTML serialization (`SaveOptions::AS_HTML`) controls empty-element output.
///
/// # SAFETY
///
/// - `node` must be NULL or a valid `_xmlNode`; `buf` a valid `_xmlBuffer`;
///   `indent`/`encoding` NULL or valid NUL-terminated strings.
#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn serialize_node_opts_enc_full(
    node: *mut _xmlNode,
    buf: *mut _xmlBuffer,
    format: c_int,
    level: c_int,
    indent: *const xmlChar,
    no_decl: c_int,
    no_empty: c_int,
    as_html: c_int,
    encoding: *const xmlChar,
) {
    unsafe {
        if node.is_null() || buf.is_null() {
            return;
        }
        let parent = (*node).parent;
        let mut state = DumpState::with_indent_enc(format, indent, no_decl, encoding);
        state.no_empty = no_empty != 0;
        state.as_html = as_html != 0;
        state.explicit_save = true;
        let mut lvl = level;
        node_dump_internal(buf, node, node, parent, &mut state, &mut lvl);
    }
}

/// Serialize a node with full options plus XHTML mode (upstream
/// `xhtmlNodeDumpOutput`). When `xhtml` is set, a bare `<html>` element
/// receives the XHTML default namespace and non-HTML-empty elements are
/// serialized as open/close pairs.
///
/// # SAFETY
///
/// - `node` must be NULL or a valid `_xmlNode`; `buf` a valid `_xmlBuffer`;
///   `indent`/`encoding` NULL or valid NUL-terminated strings.
// The 8 parameters mirror the upstream xmlsave.c dump state (node, buffer,
// format, level, indent, no-declaration, encoding, xhtml mode) — the XHTML
// flag is threaded alongside the existing serializer state rather than
// through the DumpState struct to keep the xhtml gate visible at the call
// site.
#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn serialize_node_opts_xhtml(
    node: *mut _xmlNode,
    buf: *mut _xmlBuffer,
    format: c_int,
    level: c_int,
    indent: *const xmlChar,
    no_decl: c_int,
    encoding: *const xmlChar,
    xhtml: bool,
) {
    unsafe {
        if node.is_null() || buf.is_null() {
            return;
        }
        let parent = (*node).parent;
        let mut state = DumpState::with_indent_enc(format, indent, no_decl, encoding);
        state.xhtml = xhtml;
        let mut lvl = level;
        node_dump_internal(buf, node, node, parent, &mut state, &mut lvl);
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

/// Save a document to a file descriptor.
///
/// # SAFETY
///
/// - `doc` must be a valid pointer to an `_xmlDoc`, or NULL.
/// - `fd` must be a valid open file descriptor.
#[allow(dead_code)]
pub(crate) unsafe fn save_doc_to_fd(doc: *mut _xmlDoc, fd: c_int, _compression: c_int) -> c_int {
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
#[allow(dead_code)]
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
#[allow(dead_code)]
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
#[allow(dead_code)]
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
pub unsafe fn dump_doc(doc: *mut _xmlDoc) -> *mut xmlChar {
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
/// - `mem` must be a valid pointer to an xmlChar* that will receive the
///   allocated memory.
/// - `size` may be NULL (upstream tree.c xmlDocDumpFormatMemoryEnc only
///   writes the length when the pointer is given; `mem` is still produced —
///   the recover-cont-probe passes NULL for the length).
pub(crate) unsafe fn xmlDocDumpFormatMemory(
    doc: *mut _xmlDoc,
    mem: *mut *mut xmlChar,
    size: *mut c_int,
    format: c_int,
) {
    if doc.is_null() || mem.is_null() {
        return;
    }

    let buf = io::buf_create(-1);
    if buf.is_null() {
        unsafe {
            *mem = ptr::null_mut();
            if !size.is_null() {
                *size = 0;
            }
        }
        return;
    }

    serialize_node(doc as *mut _xmlNode, buf, format, 0);

    let content = io::buf_content(buf);
    let len = io::buf_length(buf);

    if !content.is_null() && len > 0 {
        // Allocate memory for the result (+1 for null terminator)
        let result = allocator::xmlMallocImpl((len + 1) as usize) as *mut xmlChar;
        if !result.is_null() {
            ptr::copy_nonoverlapping(content, result, len as usize);
            *result.add(len as usize) = 0;
            unsafe {
                *mem = result;
                if !size.is_null() {
                    *size = len;
                }
            }
        } else {
            unsafe {
                *mem = ptr::null_mut();
                if !size.is_null() {
                    *size = 0;
                }
            }
        }
    } else {
        unsafe {
            *mem = ptr::null_mut();
            if !size.is_null() {
                *size = 0;
            }
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
/// - `size` may be NULL (upstream tree.c xmlDocDumpFormatMemoryEnc).
pub(crate) unsafe fn xmlDocDumpMemory(doc: *mut _xmlDoc, mem: *mut *mut xmlChar, size: *mut c_int) {
    xmlDocDumpFormatMemory(doc, mem, size, 0)
}

/// Save a document to a file (ABI wrapper). Upstream tree.c 2.15:
/// `xmlSaveFile` = `xmlSaveFormatFileEnc(filename, cur, NULL, 0)`.
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
    unsafe { xmlSaveFormatFileEnc(filename, cur, ptr::null(), 0) }
}

/// Save a document to a file with encoding. Upstream tree.c 2.15:
/// `xmlSaveFileEnc` = `xmlSaveFormatFileEnc(filename, cur, encoding, 0)`.
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
    unsafe { xmlSaveFormatFileEnc(filename, cur, encoding, 0) }
}

/// Save a document to a file with format flag. Upstream tree.c 2.15:
/// `xmlSaveFormatFile` = `xmlSaveFormatFileEnc(filename, cur, NULL, format)`.
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
    unsafe { xmlSaveFormatFileEnc(filename, cur, ptr::null(), format) }
}

/// Save a document to a file with encoding and format flag.
///
/// Mirrors upstream xmlsave.c 2.15 `xmlSaveFormatFileEnc`: create the output
/// buffer ("-" maps to stdout like the oracle), serialize through the save
/// machinery (formatting + the encoding declaration), and return the close
/// result. The save context's encoding is emitted in the XML declaration and
/// drives the output-buffer encoder exactly like upstream xmlDocDumpInternal.
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
    if cur.is_null() {
        return -1;
    }
    let options = if format != 0 {
        crate::xml::save::XML_SAVE_FORMAT
    } else {
        0
    };
    let ctxt = crate::xml::save::xmlSaveToFilename(filename, encoding, options);
    if ctxt.is_null() {
        return -1;
    }
    crate::xml::save::xmlSaveDoc(ctxt, cur);
    crate::xml::save::xmlSaveClose(ctxt)
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

    /// Helper: allocate a NUL-terminated xmlChar copy of `s`.
    ///
    /// # Safety
    ///
    /// - The returned buffer is heap-allocated with `xmlMallocImpl` and must
    ///   be freed by the caller with `xmlFreeImpl`; it may be NULL on OOM, so
    ///   callers check it before use.
    fn c_str(s: &str) -> *const xmlChar {
        let bytes = s.as_bytes();
        let buf = unsafe { allocator::xmlMallocImpl(bytes.len() + 1) as *mut u8 };
        if !buf.is_null() {
            unsafe {
                ptr::copy_nonoverlapping(bytes.as_ptr(), buf, bytes.len());
                *buf.add(bytes.len()) = 0;
            }
        }
        buf as *const xmlChar
    }

    /// Verify creating and freeing a document.
    ///
    /// # Safety
    ///
    /// - `doc` from `new_doc` must be a valid `_xmlDoc` while its fields are
    ///   read; it is freed with `free_doc`.
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

    /// Verify creating a document with a version string.
    ///
    /// # Safety
    ///
    /// - The `c_str` buffer must be NUL-terminated and alive while `new_doc`
    ///   duplicates it; the buffer is freed with `xmlFreeImpl` and the doc
    ///   with `free_doc`.
    #[test]
    fn test_new_doc_with_version() {
        unsafe {
            let ver = c_str("2.0");
            let doc = new_doc(ver);
            assert!(!doc.is_null());
            let doc_ver = (*doc).version;
            assert!(!doc_ver.is_null());
            assert!(crate::abi::exports_xml2::xmlStrEqual(doc_ver, ver,) != 0);
            allocator::xmlFreeImpl(ver as *mut c_void);
            free_doc(doc);
        }
    }

    /// Verify creating a node.
    ///
    /// # Safety
    ///
    /// - The `c_str` buffer must be NUL-terminated and alive while `new_node`
    ///   duplicates it; the node is freed with `free_node` and the doc with
    ///   `free_doc`.
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

    /// Verify that `node_get_content` concatenates all descendant text.
    ///
    /// # Safety
    ///
    /// - The doc and nodes built by the tree helpers must be valid and linked
    ///   before `node_get_content` walks them; `content` is read with
    ///   `strlen` and freed with `xmlFreeImpl`, and the doc with `free_doc`.
    #[test]
    fn test_node_get_content_recurses_descendants() {
        // UPSTREAM-PARITY: xmlNodeGetContent (tree.c) concatenates ALL
        // descendant text, not just direct text children — the XPath 1.0
        // string-value of an element. Regression test for the Phase 9 fix
        // where <book><title>Rust</title></book> produced empty content.
        unsafe {
            let doc = new_doc(ptr::null());
            let root = new_node(ptr::null_mut(), c_str("library"));
            doc_set_root_element(doc, root);
            let book = new_child(root, ptr::null_mut(), c_str("book"));
            let title = new_child(book, ptr::null_mut(), c_str("title"));
            let text = new_text(c_str("Rust"));
            add_child(title, text);

            let content = node_get_content(book);
            assert!(!content.is_null());
            let s = core::slice::from_raw_parts(
                content,
                libc::strlen(content as *const libc::c_char) as usize,
            );
            assert_eq!(s, b"Rust", "descendant text not concatenated");
            allocator::xmlFreeImpl(content as *mut c_void);

            free_doc(doc);
        }
    }

    /// Verify setting the root element of a document.
    ///
    /// # Safety
    ///
    /// - `doc` and `root` must be valid pointers while `doc_set_root_element`
    ///   relinks them; the doc is freed with `free_doc`.
    #[test]
    fn test_doc_set_root_element() {
        unsafe {
            let doc = new_doc(ptr::null());
            let root = new_node(ptr::null_mut(), c_str("root"));
            let old = doc_set_root_element(doc, root);
            assert!(old.is_null());
            assert_eq!(doc_get_root_element(doc), root);
            assert_eq!((*doc).children, root);
            free_doc(doc);
        }
    }

    /// Verify adding children and siblings.
    ///
    /// # Safety
    ///
    /// - The doc and nodes built by the tree helpers must be valid and linked
    ///   before their `parent`, `children`, `last`, `next`, and `prev` fields
    ///   are read; the doc is freed with `free_doc`.
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

    /// Phase 14 PHP DOM regression (domattributes): attaching an ATTRIBUTE
    /// node with `xmlAddChild(element, attr)` must route it into the element's
    /// PROPERTIES list, not its children list. php's `element->setAttributeNode`
    /// (new DOMAttr + setAttributeNode) does exactly this via xmlAddChild. The
    /// pre-fix behaviour appended the attribute to `children` — the attribute
    /// serialized as a bogus child text node and doubly freed on teardown.
    ///
    /// # Safety
    ///
    /// - `root`/`attr` are built and linked as in upstream: an element under a
    ///   doc, and a standalone attr from `xmlNewProp(NULL, ...)`, then attached
    ///   with `add_child`. The doc is freed once with `free_doc` (proving no
    ///   double free of the attached attribute).
    #[test]
    fn test_add_child_attribute_goes_to_properties() {
        unsafe {
            let doc = new_doc(ptr::null());
            let root = new_node(ptr::null_mut(), c_str("chapter"));
            doc_set_root_element(doc, root);

            // Mirror php DOMAttr::__construct: a standalone (doc NULL,
            // unlinked) attribute with a text value.
            let attr = crate::abi::exports_tree::xmlNewProp(
                ptr::null_mut(),
                c"num".as_ptr() as *const crate::abi::types::xmlChar,
                c"1".as_ptr() as *const crate::abi::types::xmlChar,
            );
            assert!(!attr.is_null());
            assert_eq!((*attr).type_, XML_ATTRIBUTE_NODE as c_int);

            // xmlAddChild(element, attr) — upstream routes attrs to properties.
            let ret = add_child(root, attr as *mut _xmlNode);
            assert_eq!(ret, attr as *mut _xmlNode);
            assert_eq!((*root).properties, attr);
            assert!(
                (*root).children.is_null(),
                "attr must NOT become a child node"
            );
            assert!(!(*attr).children.is_null());
            assert_eq!((*(*attr).children).parent, attr as *mut _xmlNode);

            // Crucially: the doc teardown must not double-free the attribute.
            free_doc(doc);
        }
    }

    /// Phase 14 PHP DOM regression (modern/spec serialize_*_xmlns and the
    /// modern Dom\XMLDocument namespace mapper): xmlSetNsProp must bind the new
    /// attribute to the given `ns`, not drop it. php's modern DOM materialises
    /// xmlns declarations as real attributes in the XMLNS namespace via
    /// xmlSetNsProp(node, xmlnsNs, prefix|xmlns, href); the legacy stub ignored
    /// `ns` and created an UNNAMESPACED `a="urn:a"` attribute, so the doc
    /// serialized with a spurious bare attribute and a missing/duplicated
    /// namespace declaration.
    ///
    /// # Safety
    ///
    /// - The doc/root/ns are valid while `set_ns_prop` runs; the namespace is
    ///   a detached, dedicated declaration on `root`; the doc is freed with
    ///   `free_doc`.
    #[test]
    fn test_set_ns_prop_binds_namespace() {
        unsafe {
            let doc = new_doc(ptr::null());
            let root = new_node(ptr::null_mut(), c_str("root"));
            doc_set_root_element(doc, root);

            // xmlNewNs(node, href, prefix): declares xmlns:a="urn:a" on root.
            let ns = new_ns(root, c_str("urn:a"), c_str("a"));

            let attr = set_ns_prop(root, ns, c_str("a"), c_str("urn:a"));
            assert!(!attr.is_null());
            assert_eq!((*root).properties, attr);
            // CRITICAL: the attribute must carry the namespace, not be bare.
            assert_eq!((*attr).ns, ns);
            assert!(!(*attr).name.is_null());

            free_doc(doc);
        }
    }

    /// Phase 14 PHP DOM regression (gh12870_b): searching for the reserved XML
    /// namespace URI must bind prefix `xml` (it is implicitly in scope), even
    /// on a freshly built document with no xmlns:xml declaration. Without the
    /// doc->oldNs fallback php DOMDocument::createAttributeNS
    /// ('http://www.w3.org/XML/1998/namespace', 'xml') resolved to a synthetic
    /// "default" prefix instead of the fixed xml prefix.
    ///
    /// # Safety
    ///
    /// - The doc/root are valid while search_ns_by_href runs; the doc is freed
    ///   with `free_doc` (freeing the materialised doc-level xml declaration).
    #[test]
    fn test_search_ns_by_href_xml_namespace_returns_xml_prefix() {
        unsafe {
            let doc = new_doc(ptr::null());
            let root = new_node(ptr::null_mut(), c_str("root"));
            doc_set_root_element(doc, root);
            // root->doc is set by doc_set_root_element; documentElement::first
            // usage in php starts from root. Query must succeed.
            let found = search_ns_by_href(
                doc,
                root,
                c"http://www.w3.org/XML/1998/namespace".as_ptr() as *const xmlChar,
            );
            assert!(!found.is_null());
            assert!(!(*found).prefix.is_null());
            assert!(
                crate::abi::exports_xml2::xmlStrEqual(
                    (*found).prefix,
                    c"xml".as_ptr() as *const xmlChar
                ) != 0
            );
            free_doc(doc);
        }
    }

    /// Phase 14 PHP DOM regression (modern/spec Node_isDefaultNamespace): text,
    /// CDATA, comment and PI nodes may carry their `name` as the SHARED static
    /// marker `xmlStringText`/`xmlStringComment` (upstream tree.c xmlNewText/
    /// xmlNewComment). `free_node` must not `xmlFree` such a name or teardown
    /// aborts with `free(): invalid pointer`. Regression guard for the sentinel
    /// name-free skip.
    ///
    /// # Safety
    ///
    /// - The node is a freshly xmlMalloc'd text node, fully zero-initialised
    ///   except its type and sentinel `name`; it owns no other allocations, so
    ///   `free_node` releases only the struct.
    #[test]
    fn test_free_text_node_with_static_name_sentinel() {
        unsafe {
            let node = allocator::xmlMallocZero(size_of::<_xmlNode>()) as *mut _xmlNode;
            assert!(!node.is_null());
            (*node).type_ = XML_TEXT_NODE as c_int;
            // A text node whose name aliases the shared static marker text.
            (*node).name = crate::abi::data_globals::xmlStringText.as_ptr();
            // no children/content owned by the node.
            free_node(node); // must not free the static marker.
        }
    }

    /// Verify unlinking a node.
    ///
    /// # Safety
    ///
    /// - The doc and nodes must be valid while `unlink_node` rewrites the
    ///   sibling links; the unlinked node is freed with `free_node` and the
    ///   doc with `free_doc`.
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

    /// Verify creating text, comment, and PI nodes.
    ///
    /// # Safety
    ///
    /// - The `c_str` buffers must be NUL-terminated and alive while the
    ///   creators duplicate them; each node is freed with `free_node`.
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

    /// Verify setting and getting a property.
    ///
    /// # Safety
    ///
    /// - `doc` and `root` must be valid while `set_prop` and `get_prop` run;
    ///   the value returned by `get_prop` is freed with `xmlFreeImpl` and the
    ///   doc with `free_doc`.
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
            allocator::xmlFreeImpl(value as *mut c_void);

            free_doc(doc);
        }
    }

    /// Verify removing a property.
    ///
    /// # Safety
    ///
    /// - `doc` and `root` must be valid while `set_prop`, `get_prop`, and
    ///   `remove_prop` run; values returned by `get_prop` are freed with
    ///   `xmlFreeImpl` and the doc with `free_doc`.
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
            allocator::xmlFreeImpl(value as *mut c_void);

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

    /// Verify namespace operations: creation, binding, and search.
    ///
    /// # Safety
    ///
    /// - `doc`, `root`, and `ns` must be valid while the namespace helpers
    ///   run; the `c_str` buffers must be NUL-terminated and alive for the
    ///   calls. The doc is freed with `free_doc`.
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

    /// Verify creating a DTD and attaching it to a document.
    ///
    /// # Safety
    ///
    /// - `doc` and `dtd` must be valid while `new_dtd` and `get_int_subset`
    ///   run; the `c_str` buffers must be NUL-terminated and alive for the
    ///   calls. The doc is freed with `free_doc`.
    #[test]
    fn test_new_dtd() {
        unsafe {
            let doc = new_doc(ptr::null());
            let dtd = new_dtd(doc, c_str("root"), c_str("-//TEST//DTD"), c_str("test.dtd"));
            assert!(!dtd.is_null());
            assert_eq!((*dtd).type_, XML_DTD_NODE as c_int);
            // UPSTREAM-PARITY (tree.c xmlNewDtd): xmlNewDtd creates the
            // document's EXTERNAL subset, so the DTD is reachable as
            // doc->extSubset (the xmlCreateIntSubset call sets intSubset).
            assert_eq!((*doc).extSubset, dtd);
            free_doc(doc);
        }
    }

    /// Verify deep-copying a node.
    ///
    /// # Safety
    ///
    /// - `doc` and `root` must be valid while `copy_node` walks the subtree;
    ///   the copy is freed with `free_node` and the doc with `free_doc`.
    #[test]
    fn test_copy_node_deep() {
        unsafe {
            let doc = new_doc(ptr::null());
            let root = new_node(ptr::null_mut(), c_str("root"));
            doc_set_root_element(doc, root);
            let _child = new_child(root, ptr::null_mut(), c_str("child"));

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

    /// Verify creating a CDATA section node.
    ///
    /// # Safety
    ///
    /// - `doc` must be valid and `content` NUL-terminated and alive while
    ///   `new_cdata_block` runs; the node is freed with `free_node` and the
    ///   doc with `free_doc`.
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

    /// A half-million-deep element chain must free WITHOUT C recursion —
    /// upstream xmlFreeNodeList walks iteratively (tree.c 2.15) and the
    /// recursive version overflowed the stack at php shutdown on the deep
    /// Dom\XMLDocument (GH-22570: segv after saveXml's "Maximum call stack
    /// size" Error, which this test's process would hit too on a regression
    /// because the test thread stack is far below 500k frames).
    ///
    /// # Safety
    ///
    /// - doc/root/chain are built with the tree API and freed exactly once
    ///   through `free_doc` (children before parents, post-order).
    #[test]
    fn test_free_deeply_nested_chain_is_iterative() {
        unsafe {
            let doc = new_doc(c"1.0".as_ptr() as *const xmlChar);
            assert!(!doc.is_null());
            const DEPTH: usize = 500_000;
            let root = new_node(ptr::null_mut(), c_str("a"));
            assert!(!root.is_null());
            doc_set_root_element(doc, root);
            let mut child = root;
            for _ in 0..DEPTH {
                let next = new_node(ptr::null_mut(), c_str("a"));
                assert!(!next.is_null());
                assert!(!add_child(child, next).is_null());
                child = next;
            }
            free_doc(doc);
        }
    }

    /// Verify NULL handling in the tree API entry points.
    ///
    /// # Safety
    ///
    /// - NULL pointers passed to `new_doc`, `new_node`, `free_node`,
    ///   `free_doc`, `unlink_node`, `add_child`, and `add_sibling` must be
    ///   accepted without dereference (the test asserts this); all created
    ///   objects are freed.
    #[test]
    fn test_null_handling() {
        unsafe {
            assert!(!new_doc(ptr::null()).is_null()); // Should succeed with default version
            let doc = new_doc(ptr::null());
            // UPSTREAM-PARITY (tree.c xmlNewNode): a NULL name is rejected
            // up front (HOSTILE-ABI A48).
            assert!(new_node(ptr::null_mut(), ptr::null()).is_null());
            free_node(ptr::null_mut()); // Should not crash
            free_doc(ptr::null_mut()); // Should not crash
            unlink_node(ptr::null_mut()); // Should not crash
            assert!(add_child(ptr::null_mut(), ptr::null_mut()).is_null());
            assert!(add_sibling(ptr::null_mut(), ptr::null_mut()).is_null());
            free_doc(doc);
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // Serialization Tests
    // ═══════════════════════════════════════════════════════════════════

    /// Helper: compare a buffer's content to an expected string.
    ///
    /// # Safety
    ///
    /// - `buf` must be a valid pointer to an `_xmlBuffer`; its content and
    ///   length are read through `io::buf_content` and `io::buf_length`.
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

    /// Verify serializing an empty document.
    ///
    /// # Safety
    ///
    /// - `doc` must be a valid `_xmlDoc` and `buf` a valid `_xmlBuffer` while
    ///   `doc_dump` runs; `buf` is freed with `io::buf_free` and the doc with
    ///   `free_doc`.
    #[test]
    fn test_serialize_empty_document() {
        unsafe {
            let doc = new_doc(ptr::null());
            let buf = io::buf_create(-1);
            assert!(!buf.is_null());

            let ret = doc_dump(buf, doc);
            assert!(ret >= 0);

            // UPSTREAM-PARITY: xmlDocDump writes the declaration with no
            // encoding attribute (doc->encoding is NULL) and a trailing
            // newline after it.
            let expected = "<?xml version=\"1.0\"?>\n";
            assert!(buf_eq_str(buf, expected));

            io::buf_free(buf);
            free_doc(doc);
        }
    }

    /// Verify serializing an element with text content.
    ///
    /// # Safety
    ///
    /// - `doc` and `buf` must be valid while `doc_dump` runs; `buf` is freed
    ///   with `io::buf_free` and the doc with `free_doc`.
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

            let expected = "<?xml version=\"1.0\"?>\n<root>hello world</root>\n";
            assert!(buf_eq_str(buf, expected));

            io::buf_free(buf);
            free_doc(doc);
        }
    }

    /// Verify serializing an element with attributes.
    ///
    /// # Safety
    ///
    /// - `doc` and `buf` must be valid while `doc_dump` runs; `buf` is freed
    ///   with `io::buf_free` and the doc with `free_doc`.
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

            let expected = "<?xml version=\"1.0\"?>\n<root id=\"42\" name=\"test\"/>\n";
            assert!(buf_eq_str(buf, expected));

            io::buf_free(buf);
            free_doc(doc);
        }
    }

    /// Verify serializing nested elements.
    ///
    /// # Safety
    ///
    /// - `doc` and `buf` must be valid while `doc_dump` walks the tree; `buf`
    ///   is freed with `io::buf_free` and the doc with `free_doc`.
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

            let expected = "<?xml version=\"1.0\"?>\n<root><child><gc>text</gc></child></root>\n";
            assert!(buf_eq_str(buf, expected));

            io::buf_free(buf);
            free_doc(doc);
        }
    }

    /// Verify formatted serialization output.
    ///
    /// # Safety
    ///
    /// - `doc` and `buf` must be valid while `serialize_node` runs; `buf` is
    ///   freed with `io::buf_free` and the doc with `free_doc`.
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

            let expected = "<?xml version=\"1.0\"?>\n<root>\n  <child>text</child>\n</root>\n";
            assert!(buf_eq_str(buf, expected));

            io::buf_free(buf);
            free_doc(doc);
        }
    }

    /// Verify that `&` is escaped in serialized text.
    ///
    /// # Safety
    ///
    /// - `doc` and `buf` must be valid while `doc_dump` runs; `buf` is freed
    ///   with `io::buf_free` and the doc with `free_doc`.
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

            let expected = "<?xml version=\"1.0\"?>\n<root>a &amp; b</root>\n";
            assert!(buf_eq_str(buf, expected));

            io::buf_free(buf);
            free_doc(doc);
        }
    }

    /// Verify that angle brackets are escaped in serialized text.
    ///
    /// # Safety
    ///
    /// - `doc` and `buf` must be valid while `doc_dump` runs; `buf` is freed
    ///   with `io::buf_free` and the doc with `free_doc`.
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

            let expected = "<?xml version=\"1.0\"?>\n<root>x &lt; y &gt; z</root>\n";
            assert!(buf_eq_str(buf, expected));

            io::buf_free(buf);
            free_doc(doc);
        }
    }

    /// Verify serializing a comment node.
    ///
    /// # Safety
    ///
    /// - `doc` and `buf` must be valid while `doc_dump` runs; `buf` is freed
    ///   with `io::buf_free` and the doc with `free_doc`.
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

            let expected = "<?xml version=\"1.0\"?>\n<root><!--my comment--></root>\n";
            assert!(buf_eq_str(buf, expected));

            io::buf_free(buf);
            free_doc(doc);
        }
    }

    /// Verify serializing a processing instruction.
    ///
    /// # Safety
    ///
    /// - `doc` and `buf` must be valid while `doc_dump` runs; `buf` is freed
    ///   with `io::buf_free` and the doc with `free_doc`.
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

            let expected = "<?xml version=\"1.0\"?>\n<root><?xml-stylesheet href=\"style.xsl\" type=\"text/xsl\"?></root>\n";
            assert!(buf_eq_str(buf, expected));

            io::buf_free(buf);
            free_doc(doc);
        }
    }

    /// Verify serializing an empty element in self-closing form.
    ///
    /// # Safety
    ///
    /// - `doc` and `buf` must be valid while `doc_dump` runs; `buf` is freed
    ///   with `io::buf_free` and the doc with `free_doc`.
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

            let expected = "<?xml version=\"1.0\"?>\n<empty/>\n";
            assert!(buf_eq_str(buf, expected));

            io::buf_free(buf);
            free_doc(doc);
        }
    }

    /// Verify dumping a node to a string.
    ///
    /// # Safety
    ///
    /// - `node` must be a valid `_xmlNode` while `dump_node` serializes it;
    ///   `result` is read with `xml_strlen` and freed with `xmlFreeImpl`, and
    ///   the node with `free_node`.
    #[test]
    fn test_dump_node_to_string() {
        unsafe {
            let node = new_node(ptr::null_mut(), c_str("foo"));
            let text = new_text(c_str("bar"));
            add_child(node, text);

            let result = dump_node(node);
            assert!(!result.is_null());

            let len = xml_strlen(result);
            let slice = { core::slice::from_raw_parts(result, len as usize) };
            assert_eq!(slice, b"<foo>bar</foo>");

            allocator::xmlFreeImpl(result as *mut c_void);
            free_node(node);
        }
    }

    /// Verify dumping a document to a string.
    ///
    /// # Safety
    ///
    /// - `doc` must be a valid `_xmlDoc` while `dump_doc` serializes it;
    ///   `result` is read with `xml_strlen` and freed with `xmlFreeImpl`, and
    ///   the doc with `free_doc`.
    #[test]
    fn test_dump_doc_to_string() {
        unsafe {
            let doc = new_doc(ptr::null());
            let root = new_node(ptr::null_mut(), c_str("root"));
            doc_set_root_element(doc, root);

            let result = dump_doc(doc);
            assert!(!result.is_null());

            let len = xml_strlen(result);
            let slice = { core::slice::from_raw_parts(result, len as usize) };
            let expected = "<?xml version=\"1.0\"?>\n<root/>\n";
            assert_eq!(slice, expected.as_bytes());

            allocator::xmlFreeImpl(result as *mut c_void);
            free_doc(doc);
        }
    }

    /// Verify `xmlDocDumpFormatMemory`.
    ///
    /// # Safety
    ///
    /// - `doc` must be a valid `_xmlDoc` while the export runs; `mem` receives
    ///   a callee-owned buffer read as `size` bytes and freed with
    ///   `xmlFreeImpl`, and the doc with `free_doc`.
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

            let slice = { core::slice::from_raw_parts(mem, size as usize) };
            // UPSTREAM-PARITY: xmlDocDumpFormatMemory with a NULL encoding
            // writes no encoding attribute and a newline after each child.
            let expected = "<?xml version=\"1.0\"?>\n<root/>\n";
            assert_eq!(slice, expected.as_bytes());

            allocator::xmlFreeImpl(mem as *mut c_void);
            free_doc(doc);
        }
    }

    /// Verify escaping of special characters in attribute values.
    ///
    /// # Safety
    ///
    /// - `doc` and `buf` must be valid while `doc_dump` runs; `buf` is freed
    ///   with `io::buf_free` and the doc with `free_doc`.
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

            let expected =
                "<?xml version=\"1.0\"?>\n<root desc=\"a &lt; b &amp; c &quot;quoted&quot;\"/>\n";
            assert!(buf_eq_str(buf, expected));

            io::buf_free(buf);
            free_doc(doc);
        }
    }

    /// Phase 14.3 PHP regression (DOMParentNode_empty_argument): serializing a
    /// document-fragment node must emit its children (transparent container).
    /// The missing XML_DOCUMENT_FRAG_NODE arm returned empty output (and a
    /// downstream PHP double-destroy) where upstream xmlsave.c trampolines the
    /// fragment's children.
    ///
    /// # Safety
    ///
    /// - `frag`/`foo` are built under `doc`; `node_dump` walks them; the buffer
    ///   is freed with `io::buf_free` and the doc with `free_doc`.
    #[test]
    fn test_dump_document_fragment_serializes_children() {
        unsafe {
            let doc = new_doc(ptr::null());
            let frag = crate::abi::exports_tree::xmlNewDocFragment(doc);
            assert!(!frag.is_null());
            // add an element child `<foo/>` to the fragment
            let foo = new_child(frag, ptr::null_mut(), c_str("foo"));
            assert!(!foo.is_null());

            let buf = io::buf_create(-1);
            assert!(!buf.is_null());
            // Serialize the fragment itself (xmlNodeDump semantics, same
            // node_dump_internal path as xmlNodeDumpOutput).
            let ret = node_dump(buf, doc, frag, 0, 0);
            assert!(ret >= 0);
            assert!(buf_eq_str(buf, "<foo/>"));

            io::buf_free(buf);
            free_doc(doc);
        }
    }

    /// Phase 14.3 Bug-3 regression: `copy_doc` must link the copied
    /// top-level children with the NEW document node as their parent (and set
    /// `doc->last`), mirroring upstream xmlCopyDoc
    /// (`xmlStaticCopyNodeList(doc->children, ret, (xmlNodePtr)ret)`). The
    /// pre-fix NULL parent made PHP treat a cloned document's root element as
    /// ownerless: its proxy teardown (php_libxml_node_free_resource,
    /// `parent == NULL` branch) freed the whole subtree while the cloned doc
    /// still referenced it, so the doc teardown double-freed the root
    /// (DOMDocument clone + navigation crash).
    ///
    /// # Safety
    ///
    /// - doc/root are valid while copy_doc runs; both docs are freed with
    ///   `free_doc`.
    #[test]
    fn test_copy_doc_keeps_doc_as_root_parent() {
        unsafe {
            let doc = new_doc(ptr::null());
            let root = new_node(ptr::null_mut(), c_str("root"));
            doc_set_root_element(doc, root);
            let child = new_child(root, ptr::null_mut(), c_str("kid"));
            assert!(!child.is_null());

            let copy = copy_doc(doc, 1);
            assert!(!copy.is_null());
            let copied_root = (*copy).children;
            assert!(!copied_root.is_null());
            // UPSTREAM-PARITY: root's parent is the DOCUMENT node.
            assert_eq!((*copied_root).parent as *mut c_void, copy as *mut c_void);
            assert_eq!((*copied_root).doc as *mut c_void, copy as *mut c_void);
            // doc->last tracks the final child.
            assert_eq!((*copy).last as *mut c_void, copied_root as *mut c_void);
            // child keeps its element parent.
            let copied_child = (*copied_root).children;
            assert!(!copied_child.is_null());
            assert_eq!(
                (*copied_child).parent as *mut c_void,
                copied_root as *mut c_void
            );

            free_doc(copy);
            free_doc(doc);
        }
    }
}

