//! exports_xptr — the `xmlXPtr*` family of C ABI exports (family closure 11.1-I).
//!
//! These are the XPointer location-set / range / node-list functions that
//! libxml2 still exports from the shared object even though they were removed
//! from the public headers (they are gone from `xpointer.h` since 2.13/2.15;
//! the archaeology clone at `archaeology/libxml2-git/xpointer.c` only retains
//! `xmlXPtrNewContext` and `xmlXPtrEval`).
//!
//! # Signatures
//!
//! The exact signatures come from the historical public header
//! (`oracle/historical/prefix/libxml2-2.9.14/include/libxml2/libxml/xpointer.h`)
//! and the function definitions in `oracle/historical/src/libxml2-2.13.5/
//! xpointer.c`. All 19 symbols are exported by the oracle DSO
//! (`/usr/lib/libxml2-legacy/lib/libxml2.so.2`, versioned `LIBXML2_2.4.30`).
//!
//! ```c
//! xmlLocationSetPtr      xmlXPtrLocationSetCreate  (xmlXPathObjectPtr val);
//! void                   xmlXPtrFreeLocationSet    (xmlLocationSetPtr obj);
//! xmlLocationSetPtr      xmlXPtrLocationSetMerge   (xmlLocationSetPtr val1,
//!                                                   xmlLocationSetPtr val2);
//! xmlXPathObjectPtr      xmlXPtrNewRange           (xmlNodePtr start,
//!                                                   int startindex,
//!                                                   xmlNodePtr end,
//!                                                   int endindex);
//! xmlXPathObjectPtr      xmlXPtrNewRangePoints     (xmlXPathObjectPtr start,
//!                                                   xmlXPathObjectPtr end);
//! xmlXPathObjectPtr      xmlXPtrNewRangePointNode  (xmlXPathObjectPtr start,
//!                                                   xmlNodePtr end);
//! xmlXPathObjectPtr      xmlXPtrNewRangeNodePoint  (xmlNodePtr start,
//!                                                   xmlXPathObjectPtr end);
//! xmlXPathObjectPtr      xmlXPtrNewRangeNodes      (xmlNodePtr start,
//!                                                   xmlNodePtr end);
//! xmlXPathObjectPtr      xmlXPtrNewLocationSetNodes(xmlNodePtr start,
//!                                                   xmlNodePtr end);
//! xmlXPathObjectPtr      xmlXPtrNewLocationSetNodeSet(xmlNodeSetPtr set);
//! xmlXPathObjectPtr      xmlXPtrNewRangeNodeObject (xmlNodePtr start,
//!                                                   xmlXPathObjectPtr end);
//! xmlXPathObjectPtr      xmlXPtrNewCollapsedRange  (xmlNodePtr start);
//! void                   xmlXPtrLocationSetAdd     (xmlLocationSetPtr cur,
//!                                                   xmlXPathObjectPtr val);
//! xmlXPathObjectPtr      xmlXPtrWrapLocationSet    (xmlLocationSetPtr val);
//! void                   xmlXPtrLocationSetDel     (xmlLocationSetPtr cur,
//!                                                   xmlXPathObjectPtr val);
//! void                   xmlXPtrLocationSetRemove  (xmlLocationSetPtr cur,
//!                                                   int val);
//! xmlXPathContextPtr     xmlXPtrNewContext         (xmlDocPtr doc,
//!                                                   xmlNodePtr here,
//!                                                   xmlNodePtr origin);
//! void                   xmlXPtrRangeToFunction    (xmlXPathParserContextPtr ctxt,
//!                                                   int nargs);
//! xmlNodePtr             xmlXPtrBuildNodeList      (xmlXPathObjectPtr obj);
//! ```
//!
//! # Object layout notes
//!
//! - Range objects (`XPATH_RANGE`, type 6) store their start/end points in
//!   `user`/`index` and `user2`/`index2` (start node/start index, end node/end
//!   index).
//! - Location-set objects (`XPATH_LOCATIONSET`, type 7) store the
//!   `xmlLocationSetPtr` in `user`.
//! - Point objects (`XPATH_POINT`, type 5) store the node in `user` and the
//!   index in `index`.
//!
//! # Simplifications vs upstream
//!
//! - `xmlXPtrRangeToFunction` follows the 2.9.14/2.13.x sources and the oracle
//!   DSO (verified by disassembly: a tail call to `xmlXPathErr`): it only sets
//!   `XPATH_EXPR_ERROR` on the parser context. The pre-2.9 implementation that
//!   pushed a range over the context node-set is obsolete upstream.
//! - `xmlXPtrNewContext` sets `xptr = 1` / `here` / `origin` (as the task
//!   specifies and as 2.9–2.13 did under `LIBXML_XPTR_LOCS_ENABLED`) but does
//!   not register the `range()`/`here()`/… extension functions: the modern
//!   2.16 archaeology no longer registers them and those seven functions are
//!   not part of this export set.
//!
//! # Upstream contract
//!
//! Parity target is upstream `xpointer.c`/`xpath.c` — the location-set
//! surface that the oracle DSO still exports (versioned `LIBXML2_2.4.30`)
//! even though it was removed from the public headers since 2.13/2.15;
//! signatures come from the historical 2.9.14 header and the 2.13.5 source.
//! R-000166 (11.1-P) exercised the xpointer surface in the three-way
//! standards reconciliation.
//!
//! # Conceptual behavior
//!
//! This module implements the XPointer location-set/range/node-list ABI:
//! `xmlXPtrLocationSet*` management, the range constructors (`xmlXPtrNewRange*`,
//! `xmlXPtrNewCollapsedRange`, ...), node-set wrappers, `xmlXPtrNewContext` and
//! `xmlXPtrRangeToFunction`. Range objects store start/end in
//! `user`/`index`/`user2`/`index2`; location-set objects store the set in
//! `user` (layout notes above).
//!
//! # Ownership & safety invariants
//!
//! Location sets and range objects are XPath objects: created by the New*
//! constructors and freed with `xmlXPathFreeObject`; `xmlXPtrFreeLocationSet`
//! frees a set and its members; `xmlXPtrLocationSetAdd` takes ownership of the
//! added object (upstream transfers), `xmlXPtrLocationSetDel`/`Remove` drop
//! members. The node pointers inside are borrowed — never freed by the
//! location set.
//!
//! # Historical quirks & epochs
//!
//! The XPointer API dates to the 2.4/2.6 era and was pruned from the headers
//! in 2.13+ while the DSO kept the exports — the candidate mirrors the
//! exported DSO surface, not the modern header. SEC-0009 records the 2016
//! XPointer CVE fixes (commits `9ab01a27`/`c1d1f712`, 2016-06-28). R-000166
//! aligned `xmlXPtrRangeToFunction` with the 2.9.14/2.13.x behavior (tail call
//! to xmlXPathErr verified by disassembly).
//!
//! # Deliberate oddities
//!
//! `xmlXPtrRangeToFunction` only sets `XPATH_EXPR_ERROR` (the pre-2.9
//! range-push implementation is obsolete upstream — deliberate);
//! `xmlXPtrNewContext` sets xptr/here/origin but does not register the
//! range()/here() extension functions (the modern 2.16 archaeology no longer
//! registers them).
//!
//! # Proving courts
//!
//! The XINCLUDE, XPATH and XPOINTER court families, the C14N subset cases
//! (R-000166) and the DSO-LOADER court (versioned-symbol resolution) cover
//! this module; the xpointer unit tests run under cargo test.
//!
//! # Tempting simplifications that would break parity
//!
//! A tempting simplification is to implement `xmlXPtrLocationSetAdd` as a
//! borrow (no ownership transfer) to match Rust conventions — upstream
//! transfers ownership, and a double-free would follow when the set and the
//! caller both release the object. Another shortcut, dropping the legacy
//! location-set surface because the headers removed it, would break every
//! consumer that dlsyms these still-exported symbols.
//! - The internal document-order comparator is a private port of upstream
//!   `xmlXPathCmpNodes` with upstream return convention (1 if node1 < node2),
//!   kept for clarity; the public `crate::abi::exports_xml2::xmlXPathCmpNodes`
//!   now follows the same upstream convention.

#![allow(
    missing_docs,
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals
)]
#![allow(unused_variables)]
#![allow(unused_assignments)] // faithful ports of upstream dead stores
#![allow(clippy::missing_safety_doc)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]

// SAFETY-SCOPE: EXPORT-XPTR-MECHANICAL-001
// (11.1-Z.3 proof scope, classified-generated) — this module is the
// mechanical extern-"C" export surface: every `unsafe` block in it is
// the documented indirection/registry-access pattern whose validity
// rests on the upstream C contract, and the exported signatures are
// machine-measured by the ABI-FUNCTION-SIGNATURE and DSO-LOADER
// courts and the C-API differential probes. The safety contract of
// each export is stated in its own doc comment; this scope covers the
// mechanical wrappers' unsafe blocks.

use core::ffi::c_void;
use core::mem::size_of;
use core::ptr;
use std::os::raw::c_int;

use crate::abi::allocator::*;
use crate::abi::structs::*;
use crate::abi::types::*;
use crate::xml::tree::{add_child, add_sibling, copy_node, new_text};
use crate::xml::xpath::parser_context::{pc_set_error, XmlXPathParserContext};

/// Initial capacity of a location-set's `locTab` array (upstream
/// `XML_RANGESET_DEFAULT`, xpointer.c).
const XML_RANGESET_DEFAULT: c_int = 10;

/// A location set (upstream `struct _xmlLocationSet`, historical xpointer.h).
///
/// The location-set API was removed from the public headers but the struct is
/// still produced/consumed by the exported functions, so it is re-declared
/// here with the exact upstream layout.
#[repr(C)]
#[derive(Debug)]
pub struct _xmlLocationSet {
    pub locNr: c_int,                      // number of locations in the set
    pub locMax: c_int,                     // size of the array as allocated
    pub locTab: *mut *mut _xmlXPathObject, // array of locations
}

/// Location-set pointer (upstream `xmlLocationSetPtr`).
pub type xmlLocationSetPtr = *mut _xmlLocationSet;

// ═══════════════════════════════════════════════════════════════════════════════
// Private helpers (ports of static functions from xpointer.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// Upstream `xmlXPtrCmpPoints` (xpointer.c): compare two points w.r.t document
/// order. Returns -2 on error, 1 if point1 < point2, 0 if same point,
/// -1 otherwise.
unsafe fn cmp_points(
    node1: *mut _xmlNode,
    index1: c_int,
    node2: *mut _xmlNode,
    index2: c_int,
) -> c_int {
    if node1.is_null() || node2.is_null() {
        return -2;
    }
    if node1 == node2 {
        if index1 < index2 {
            return 1;
        }
        if index1 > index2 {
            return -1;
        }
        return 0;
    }
    cmp_nodes(node1, node2)
}

/// Private port of upstream `xmlXPathCmpNodes` (xpath.c) with the upstream
/// return convention: -2 on error, 1 if node1 < node2 (document order),
/// 0 if same node, -1 otherwise.
///
/// The local `exports_xml2::xmlXPathCmpNodes` returns the inverted sign, so
/// this faithful port is used to keep `xmlXPtrRangeCheckOrder` correct.
unsafe fn cmp_nodes(mut node1: *mut _xmlNode, mut node2: *mut _xmlNode) -> c_int {
    if node1.is_null() || node2.is_null() {
        return -2;
    }
    if node1 == node2 {
        return 0;
    }
    let mut attr1 = 0;
    let mut attr2 = 0;
    let mut attr_node1: *mut _xmlAttr = ptr::null_mut();
    let mut attr_node2: *mut _xmlAttr = ptr::null_mut();
    if (*node1).type_ == xmlElementType::XML_ATTRIBUTE_NODE as c_int {
        attr1 = 1;
        attr_node1 = node1 as *mut _xmlAttr;
        node1 = (*node1).parent;
    }
    if (*node2).type_ == xmlElementType::XML_ATTRIBUTE_NODE as c_int {
        attr2 = 1;
        attr_node2 = node2 as *mut _xmlAttr;
        node2 = (*node2).parent;
    }
    if node1 == node2 {
        if attr1 == attr2 {
            // Not required, but we keep attributes in order.
            if attr1 != 0 {
                let mut cur = (*attr_node2).prev;
                while !cur.is_null() {
                    if cur == attr_node1 {
                        return 1;
                    }
                    cur = (*cur).prev;
                }
                return -1;
            }
            return 0;
        }
        if attr2 == 1 {
            return 1;
        }
        return -1;
    }
    if (*node1).type_ == xmlElementType::XML_NAMESPACE_DECL as c_int
        || (*node2).type_ == xmlElementType::XML_NAMESPACE_DECL as c_int
    {
        return 1;
    }
    if std::ptr::eq(node1, (*node2).prev) {
        return 1;
    }
    if std::ptr::eq(node1, (*node2).next) {
        return -1;
    }
    // Compute depth to root.
    let mut depth2 = 0;
    let mut cur = node2;
    while !(*cur).parent.is_null() {
        if std::ptr::eq((*cur).parent, node1) {
            return 1;
        }
        depth2 += 1;
        cur = (*cur).parent;
    }
    let root = cur;
    let mut depth1 = 0;
    cur = node1;
    while !(*cur).parent.is_null() {
        if std::ptr::eq((*cur).parent, node2) {
            return -1;
        }
        depth1 += 1;
        cur = (*cur).parent;
    }
    // Distinct document (or distinct entities) case.
    if root != cur {
        return -2;
    }
    // Get the nearest common ancestor.
    while depth1 > depth2 {
        depth1 -= 1;
        node1 = (*node1).parent;
    }
    while depth2 > depth1 {
        depth2 -= 1;
        node2 = (*node2).parent;
    }
    while !std::ptr::eq((*node1).parent, (*node2).parent) {
        node1 = (*node1).parent;
        node2 = (*node2).parent;
        // Should not happen but just in case...
        if node1.is_null() || node2.is_null() {
            return -2;
        }
    }
    // Find who's first.
    if std::ptr::eq(node1, (*node2).prev) {
        return 1;
    }
    if std::ptr::eq(node1, (*node2).next) {
        return -1;
    }
    cur = (*node1).next;
    while !cur.is_null() {
        if cur == node2 {
            return 1;
        }
        cur = (*cur).next;
    }
    -1 // assume there is no sibling list corruption
}

/// Upstream `xmlXPtrNewPoint` (xpointer.c): create a new XPATH_POINT object.
///
/// Ported for completeness of the family closure; no exported function in
/// this module constructs points directly (upstream only uses it from the
/// XPointer extension functions that are not part of this export set).
#[allow(dead_code)]
unsafe fn new_point(node: *mut _xmlNode, indx: c_int) -> *mut _xmlXPathObject {
    if node.is_null() || indx < 0 {
        return ptr::null_mut();
    }
    let ret = xmlMallocZero(size_of::<_xmlXPathObject>()) as *mut _xmlXPathObject;
    if ret.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        (*ret).type_ = xmlXPathObjectType::XPATH_POINT as c_int;
        (*ret).user = node as *mut c_void;
        (*ret).index = indx;
    }
    ret
}

/// Upstream `xmlXPtrRangeCheckOrder` (xpointer.c): make sure the points in a
/// range are in the right order.
unsafe fn range_check_order(range: *mut _xmlXPathObject) {
    if range.is_null() {
        return;
    }
    unsafe {
        if (*range).type_ != xmlXPathObjectType::XPATH_RANGE as c_int {
            return;
        }
        if (*range).user2.is_null() {
            return;
        }
        let tmp = cmp_points(
            (*range).user as *mut _xmlNode,
            (*range).index,
            (*range).user2 as *mut _xmlNode,
            (*range).index2,
        );
        if tmp == -1 {
            std::mem::swap(&mut (*range).user, &mut (*range).user2);
            std::mem::swap(&mut (*range).index, &mut (*range).index2);
        }
    }
}

/// Upstream `xmlXPtrRangesEqual` (xpointer.c): compare two ranges.
/// Returns 1 if equal, 0 otherwise.
unsafe fn ranges_equal(range1: *mut _xmlXPathObject, range2: *mut _xmlXPathObject) -> c_int {
    if range1 == range2 {
        return 1;
    }
    if range1.is_null() || range2.is_null() {
        return 0;
    }
    unsafe {
        if (*range1).type_ != (*range2).type_ {
            return 0;
        }
        if (*range1).type_ != xmlXPathObjectType::XPATH_RANGE as c_int {
            return 0;
        }
        if (*range1).user != (*range2).user {
            return 0;
        }
        if (*range1).index != (*range2).index {
            return 0;
        }
        if (*range1).user2 != (*range2).user2 {
            return 0;
        }
        if (*range1).index2 != (*range2).index2 {
            return 0;
        }
    }
    1
}

/// Upstream `xmlXPtrNewRangeInternal` (xpointer.c): internal constructor for
/// XPATH_RANGE objects. Namespace nodes are disallowed.
unsafe fn new_range_internal(
    start: *mut _xmlNode,
    startindex: c_int,
    end: *mut _xmlNode,
    endindex: c_int,
) -> *mut _xmlXPathObject {
    if !start.is_null() && (*start).type_ == xmlElementType::XML_NAMESPACE_DECL as c_int {
        return ptr::null_mut();
    }
    if !end.is_null() && (*end).type_ == xmlElementType::XML_NAMESPACE_DECL as c_int {
        return ptr::null_mut();
    }
    let ret = xmlMallocZero(size_of::<_xmlXPathObject>()) as *mut _xmlXPathObject;
    if ret.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        (*ret).type_ = xmlXPathObjectType::XPATH_RANGE as c_int;
        (*ret).user = start as *mut c_void;
        (*ret).index = startindex;
        (*ret).user2 = end as *mut c_void;
        (*ret).index2 = endindex;
    }
    ret
}

/// Upstream `xmlXPtrGetNthChild` (xpointer.c): return the `no`'th element
/// child of `cur` (1-based) or NULL.
unsafe fn get_nth_child(mut cur: *mut _xmlNode, no: c_int) -> *mut _xmlNode {
    if cur.is_null() || (*cur).type_ == xmlElementType::XML_NAMESPACE_DECL as c_int {
        return cur;
    }
    unsafe {
        cur = (*cur).children;
        let mut i: c_int = 0;
        while i <= no {
            if cur.is_null() {
                return cur;
            }
            let t = (*cur).type_;
            if t == xmlElementType::XML_ELEMENT_NODE as c_int
                || t == xmlElementType::XML_DOCUMENT_NODE as c_int
                || t == xmlElementType::XML_HTML_DOCUMENT_NODE as c_int
            {
                i += 1;
                if i == no {
                    break;
                }
            }
            cur = (*cur).next;
        }
    }
    cur
}

/// Upstream `xmlXPtrAdvanceNode` (xpointer.c): advance to the next element or
/// text node in document order, skipping non-content node types. Returns the
/// next node or NULL at the end of the tree.
unsafe fn advance_node(mut cur: *mut _xmlNode, _level: *mut c_int) -> *mut _xmlNode {
    unsafe {
        // ---- next: ----
        if cur.is_null() || (*cur).type_ == xmlElementType::XML_NAMESPACE_DECL as c_int {
            return ptr::null_mut();
        }
        if !(*cur).children.is_null() {
            cur = (*cur).children;
        } else {
            // ---- skip: ----
            if !(*cur).next.is_null() {
                cur = (*cur).next;
            } else {
                loop {
                    cur = (*cur).parent;
                    if cur.is_null() {
                        return ptr::null_mut();
                    }
                    if !(*cur).next.is_null() {
                        cur = (*cur).next;
                        break;
                    }
                }
            }
        }
        // ---- found: ----
        loop {
            let t = (*cur).type_;
            if t == xmlElementType::XML_ELEMENT_NODE as c_int
                || t == xmlElementType::XML_TEXT_NODE as c_int
                || t == xmlElementType::XML_DOCUMENT_NODE as c_int
                || t == xmlElementType::XML_HTML_DOCUMENT_NODE as c_int
                || t == xmlElementType::XML_CDATA_SECTION_NODE as c_int
            {
                return cur;
            }
            if t == xmlElementType::XML_ENTITY_REF_NODE as c_int {
                // goto skip: should not happen, but handle like upstream.
                if !(*cur).next.is_null() {
                    cur = (*cur).next;
                } else {
                    loop {
                        cur = (*cur).parent;
                        if cur.is_null() {
                            return ptr::null_mut();
                        }
                        if !(*cur).next.is_null() {
                            cur = (*cur).next;
                            break;
                        }
                    }
                }
                continue; // re-check found on the new node
            }
            // goto next
            if !(*cur).children.is_null() {
                cur = (*cur).children;
                continue;
            }
            if !(*cur).next.is_null() {
                cur = (*cur).next;
            } else {
                loop {
                    cur = (*cur).parent;
                    if cur.is_null() {
                        return ptr::null_mut();
                    }
                    if !(*cur).next.is_null() {
                        cur = (*cur).next;
                        break;
                    }
                }
            }
        }
    }
}

/// Upstream `xmlNewTextLen` (tree.c): create a text node holding `len` bytes
/// of `content` (no null terminator copy; content may be NULL).
unsafe fn new_text_len(content: *const xmlChar, len: c_int) -> *mut _xmlNode {
    let node = xmlMallocZero(size_of::<_xmlNode>()) as *mut _xmlNode;
    if node.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        (*node).type_ = xmlElementType::XML_TEXT_NODE as c_int;
        (*node).name =
            crate::abi::exports_xml2::xmlStrdup(b"text\0" as *const u8 as *const xmlChar);
        if !content.is_null() {
            let copied = if len == 0 {
                let empty = xmlMallocImpl(1) as *mut xmlChar;
                if !empty.is_null() {
                    *empty = 0;
                }
                empty
            } else {
                crate::abi::exports_xml2::xmlStrndup(content, len)
            };
            if copied.is_null() {
                xmlFreeImpl(node as *mut c_void);
                return ptr::null_mut();
            }
            (*node).content = copied;
        }
    }
    node
}

/// Upstream `xmlXPtrBuildRangeNodeList` (xpointer.c): build a node-list tree
/// copy of an XPATH_RANGE object.
unsafe fn build_range_node_list(range: *mut _xmlXPathObject) -> *mut _xmlNode {
    // Pointers to generated nodes.
    let mut list: *mut _xmlNode = ptr::null_mut();
    let mut last: *mut _xmlNode = ptr::null_mut();
    let mut parent: *mut _xmlNode = ptr::null_mut();
    let mut tmp: *mut _xmlNode;

    if range.is_null() {
        return ptr::null_mut();
    }
    if (*range).type_ != xmlXPathObjectType::XPATH_RANGE as c_int {
        return ptr::null_mut();
    }
    let start = (*range).user as *mut _xmlNode;
    if start.is_null() || (*start).type_ == xmlElementType::XML_NAMESPACE_DECL as c_int {
        return ptr::null_mut();
    }
    let mut end = (*range).user2 as *mut _xmlNode;
    if end.is_null() {
        return copy_node(start, 1);
    }
    if (*end).type_ == xmlElementType::XML_NAMESPACE_DECL as c_int {
        return ptr::null_mut();
    }

    let mut cur = start;
    let mut index1 = (*range).index;
    let mut index2 = (*range).index2;
    while !cur.is_null() {
        unsafe {
            if cur == end {
                if (*cur).type_ == xmlElementType::XML_TEXT_NODE as c_int {
                    let content = (*cur).content;
                    if content.is_null() {
                        tmp = new_text_len(ptr::null(), 0);
                    } else {
                        let mut content2 = content;
                        let mut len = index2;
                        if cur == start && index1 > 1 {
                            content2 = content.add((index1 - 1) as usize);
                            len -= index1 - 1;
                            index1 = 0;
                        }
                        tmp = new_text_len(content2, len);
                    }
                    // Single sub text node selection.
                    if list.is_null() {
                        return tmp;
                    }
                    // Prune and return full set.
                    if !last.is_null() {
                        add_sibling(last, tmp);
                    } else {
                        add_child(parent, tmp);
                    }
                    return list;
                }
                tmp = copy_node(cur, 0);
                if list.is_null() {
                    list = tmp;
                    parent = tmp;
                } else if !last.is_null() {
                    parent = add_sibling(last, tmp);
                } else {
                    parent = add_child(parent, tmp);
                }
                last = ptr::null_mut();
                if index2 > 1 {
                    end = get_nth_child(cur, index2 - 1);
                    index2 = 0;
                }
                if cur == start && index1 > 1 {
                    cur = get_nth_child(cur, index1 - 1);
                    index1 = 0;
                } else {
                    cur = (*cur).children;
                }
                continue;
            }
            if cur == start && list.is_null() {
                let t = (*cur).type_;
                if t == xmlElementType::XML_TEXT_NODE as c_int
                    || t == xmlElementType::XML_CDATA_SECTION_NODE as c_int
                {
                    let content = (*cur).content;
                    if content.is_null() {
                        tmp = new_text_len(ptr::null(), 0);
                    } else {
                        let content2 = if index1 > 1 {
                            content.add((index1 - 1) as usize)
                        } else {
                            content
                        };
                        tmp = new_text(content2);
                    }
                    last = tmp;
                    list = tmp;
                } else {
                    if index1 > 1 {
                        tmp = copy_node(cur, 0);
                        list = tmp;
                        parent = tmp;
                        last = ptr::null_mut();
                        cur = get_nth_child(cur, index1 - 1);
                        index1 = 0;
                        continue;
                    }
                    tmp = copy_node(cur, 1);
                    list = tmp;
                    parent = ptr::null_mut();
                    last = tmp;
                }
            } else {
                tmp = ptr::null_mut();
                match (*cur).type_ {
                    t if t == xmlElementType::XML_DTD_NODE as c_int
                        || t == xmlElementType::XML_ELEMENT_DECL as c_int
                        || t == xmlElementType::XML_ATTRIBUTE_DECL as c_int
                        || t == xmlElementType::XML_ENTITY_NODE as c_int =>
                    {
                        // Do not copy DTD information.
                    }
                    t if t == xmlElementType::XML_ENTITY_DECL as c_int => {
                        // TODO: handle crossing entities -> stack needed.
                    }
                    t if t == xmlElementType::XML_XINCLUDE_START as c_int
                        || t == xmlElementType::XML_XINCLUDE_END as c_int =>
                    {
                        // Don't consider it part of the tree content.
                    }
                    t if t == xmlElementType::XML_ATTRIBUTE_NODE as c_int => {
                        // Humm, should not happen!
                    }
                    _ => {
                        tmp = copy_node(cur, 1);
                    }
                }
                if !tmp.is_null() {
                    if list.is_null() || (last.is_null() && parent.is_null()) {
                        return ptr::null_mut();
                    }
                    if !last.is_null() {
                        add_sibling(last, tmp);
                    } else {
                        last = add_child(parent, tmp);
                    }
                }
            }
            // Skip to next node in document order.
            if list.is_null() || (last.is_null() && parent.is_null()) {
                return ptr::null_mut();
            }
            cur = advance_node(cur, ptr::null_mut());
        }
    }
    list
}

// ═══════════════════════════════════════════════════════════════════════════════
// Location set management
// ═══════════════════════════════════════════════════════════════════════════════

/// Upstream `xmlXPtrLocationSetCreate` (xpointer.c): create a new location
/// set, optionally seeded with one object.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlLocationSetPtr xmlXPtrLocationSetCreate(xmlXPathObjectPtr val);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlXPtrLocationSetCreate(val: *mut _xmlXPathObject) -> xmlLocationSetPtr {
    let ret = xmlMallocZero(size_of::<_xmlLocationSet>()) as xmlLocationSetPtr;
    if ret.is_null() {
        return ptr::null_mut();
    }
    if !val.is_null() {
        let tab = xmlMallocImpl((XML_RANGESET_DEFAULT as usize) * size_of::<*mut _xmlXPathObject>())
            as *mut *mut _xmlXPathObject;
        if tab.is_null() {
            xmlFreeImpl(ret as *mut c_void);
            return ptr::null_mut();
        }
        unsafe {
            ptr::write_bytes(tab, 0, XML_RANGESET_DEFAULT as usize);
            (*ret).locMax = XML_RANGESET_DEFAULT;
            *tab = val;
            (*ret).locNr = 1;
            (*ret).locTab = tab;
        }
    }
    ret
}

/// Upstream `xmlXPtrFreeLocationSet` (xpointer.c): free a location set and
/// every object it holds.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlXPtrFreeLocationSet(xmlLocationSetPtr obj);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlXPtrFreeLocationSet(obj: xmlLocationSetPtr) {
    if obj.is_null() {
        return;
    }
    unsafe {
        let tab = (*obj).locTab;
        if !tab.is_null() {
            for i in 0..(*obj).locNr as usize {
                crate::abi::exports_xml2::xmlXPathFreeObject(*tab.add(i));
            }
            xmlFreeImpl(tab as *mut c_void);
        }
        xmlFreeImpl(obj as *mut c_void);
    }
}

/// Upstream `xmlXPtrLocationSetAdd` (xpointer.c): add an object to a location
/// set. If an equal range is already present, `val` is freed.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlXPtrLocationSetAdd(xmlLocationSetPtr cur, xmlXPathObjectPtr val);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlXPtrLocationSetAdd(cur: xmlLocationSetPtr, val: *mut _xmlXPathObject) {
    if cur.is_null() || val.is_null() {
        return;
    }
    unsafe {
        // Check against duplicates.
        for i in 0..(*cur).locNr as usize {
            if ranges_equal(*(*cur).locTab.add(i), val) != 0 {
                crate::abi::exports_xml2::xmlXPathFreeObject(val);
                return;
            }
        }
        // Grow locTab if needed.
        if (*cur).locMax == 0 {
            let tab =
                xmlMallocImpl((XML_RANGESET_DEFAULT as usize) * size_of::<*mut _xmlXPathObject>())
                    as *mut *mut _xmlXPathObject;
            if tab.is_null() {
                return;
            }
            ptr::write_bytes(tab, 0, XML_RANGESET_DEFAULT as usize);
            (*cur).locTab = tab;
            (*cur).locMax = XML_RANGESET_DEFAULT;
        } else if (*cur).locNr == (*cur).locMax {
            (*cur).locMax *= 2;
            let temp = xmlReallocImpl(
                (*cur).locTab as *mut c_void,
                ((*cur).locMax as usize) * size_of::<*mut _xmlXPathObject>(),
            ) as *mut *mut _xmlXPathObject;
            if temp.is_null() {
                return;
            }
            (*cur).locTab = temp;
        }
        let nr = (*cur).locNr as usize;
        ptr::write((*cur).locTab.add(nr), val);
        (*cur).locNr = nr as c_int + 1;
    }
}

/// Upstream `xmlXPtrLocationSetMerge` (xpointer.c): merge `val2` into `val1`
/// (adding only non-duplicate ranges). Returns `val1`, or NULL on error.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlLocationSetPtr xmlXPtrLocationSetMerge(xmlLocationSetPtr val1,
///                                           xmlLocationSetPtr val2);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlXPtrLocationSetMerge(
    val1: xmlLocationSetPtr,
    val2: xmlLocationSetPtr,
) -> xmlLocationSetPtr {
    if val1.is_null() {
        return ptr::null_mut();
    }
    if val2.is_null() {
        return val1;
    }
    unsafe {
        for i in 0..(*val2).locNr as usize {
            xmlXPtrLocationSetAdd(val1, *(*val2).locTab.add(i));
        }
    }
    val1
}

/// Upstream `xmlXPtrLocationSetDel` (xpointer.c): remove a specific object
/// pointer from a location set (no free).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlXPtrLocationSetDel(xmlLocationSetPtr cur, xmlXPathObjectPtr val);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlXPtrLocationSetDel(cur: xmlLocationSetPtr, val: *mut _xmlXPathObject) {
    if cur.is_null() || val.is_null() {
        return;
    }
    unsafe {
        let nr = (*cur).locNr;
        let mut i = 0;
        while i < nr {
            if *(*cur).locTab.add(i as usize) == val {
                break;
            }
            i += 1;
        }
        if i >= nr {
            return;
        }
        (*cur).locNr -= 1;
        let mut k = i;
        while k < (*cur).locNr {
            *(*cur).locTab.add(k as usize) = *(*cur).locTab.add(k as usize + 1);
            k += 1;
        }
        *(*cur).locTab.add((*cur).locNr as usize) = ptr::null_mut();
    }
}

/// Upstream `xmlXPtrLocationSetRemove` (xpointer.c): remove the entry at the
/// given index from a location set (no free).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlXPtrLocationSetRemove(xmlLocationSetPtr cur, int val);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlXPtrLocationSetRemove(cur: xmlLocationSetPtr, val: c_int) {
    if cur.is_null() {
        return;
    }
    unsafe {
        if val >= (*cur).locNr {
            return;
        }
        (*cur).locNr -= 1;
        let mut i = val;
        while i < (*cur).locNr {
            *(*cur).locTab.add(i as usize) = *(*cur).locTab.add(i as usize + 1);
            i += 1;
        }
        *(*cur).locTab.add((*cur).locNr as usize) = ptr::null_mut();
    }
}

/// Upstream `xmlXPtrWrapLocationSet` (xpointer.c): wrap a location set in an
/// XPATH_LOCATIONSET object (the set is owned by the object).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlXPathObjectPtr xmlXPtrWrapLocationSet(xmlLocationSetPtr val);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlXPtrWrapLocationSet(val: xmlLocationSetPtr) -> *mut _xmlXPathObject {
    let ret = xmlMallocZero(size_of::<_xmlXPathObject>()) as *mut _xmlXPathObject;
    if ret.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        (*ret).type_ = xmlXPathObjectType::XPATH_LOCATIONSET as c_int;
        (*ret).user = val as *mut c_void;
    }
    ret
}

// ═══════════════════════════════════════════════════════════════════════════════
// Range constructors
// ═══════════════════════════════════════════════════════════════════════════════

/// Upstream `xmlXPtrNewRange` (xpointer.c): create a range object from two
/// nodes and two indices.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlXPathObjectPtr xmlXPtrNewRange(xmlNodePtr start, int startindex,
///                                   xmlNodePtr end, int endindex);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlXPtrNewRange(
    start: *mut _xmlNode,
    startindex: c_int,
    end: *mut _xmlNode,
    endindex: c_int,
) -> *mut _xmlXPathObject {
    if start.is_null() || end.is_null() || startindex < 0 || endindex < 0 {
        return ptr::null_mut();
    }
    let ret = new_range_internal(start, startindex, end, endindex);
    range_check_order(ret);
    ret
}

/// Upstream `xmlXPtrNewRangePoints` (xpointer.c): create a range object from
/// two XPATH_POINT objects.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlXPathObjectPtr xmlXPtrNewRangePoints(xmlXPathObjectPtr start,
///                                         xmlXPathObjectPtr end);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlXPtrNewRangePoints(
    start: *mut _xmlXPathObject,
    end: *mut _xmlXPathObject,
) -> *mut _xmlXPathObject {
    if start.is_null() || end.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        if (*start).type_ != xmlXPathObjectType::XPATH_POINT as c_int
            || (*end).type_ != xmlXPathObjectType::XPATH_POINT as c_int
        {
            return ptr::null_mut();
        }
        let ret = new_range_internal(
            (*start).user as *mut _xmlNode,
            (*start).index,
            (*end).user as *mut _xmlNode,
            (*end).index,
        );
        range_check_order(ret);
        ret
    }
}

/// Upstream `xmlXPtrNewRangePointNode` (xpointer.c): create a range object
/// from a point and a node.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlXPathObjectPtr xmlXPtrNewRangePointNode(xmlXPathObjectPtr start,
///                                            xmlNodePtr end);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlXPtrNewRangePointNode(
    start: *mut _xmlXPathObject,
    end: *mut _xmlNode,
) -> *mut _xmlXPathObject {
    if start.is_null() || end.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        if (*start).type_ != xmlXPathObjectType::XPATH_POINT as c_int {
            return ptr::null_mut();
        }
        let ret = new_range_internal((*start).user as *mut _xmlNode, (*start).index, end, -1);
        range_check_order(ret);
        ret
    }
}

/// Upstream `xmlXPtrNewRangeNodePoint` (xpointer.c): create a range object
/// from a node and a point.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlXPathObjectPtr xmlXPtrNewRangeNodePoint(xmlNodePtr start,
///                                            xmlXPathObjectPtr end);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlXPtrNewRangeNodePoint(
    start: *mut _xmlNode,
    end: *mut _xmlXPathObject,
) -> *mut _xmlXPathObject {
    if start.is_null() || end.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        if (*end).type_ != xmlXPathObjectType::XPATH_POINT as c_int {
            return ptr::null_mut();
        }
        let ret = new_range_internal(start, -1, (*end).user as *mut _xmlNode, (*end).index);
        range_check_order(ret);
        ret
    }
}

/// Upstream `xmlXPtrNewRangeNodes` (xpointer.c): create a range object from
/// two nodes.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlXPathObjectPtr xmlXPtrNewRangeNodes(xmlNodePtr start, xmlNodePtr end);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlXPtrNewRangeNodes(
    start: *mut _xmlNode,
    end: *mut _xmlNode,
) -> *mut _xmlXPathObject {
    if start.is_null() || end.is_null() {
        return ptr::null_mut();
    }
    let ret = new_range_internal(start, -1, end, -1);
    range_check_order(ret);
    ret
}

/// Upstream `xmlXPtrNewCollapsedRange` (xpointer.c): create a collapsed range
/// (start == end) from a single node.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlXPathObjectPtr xmlXPtrNewCollapsedRange(xmlNodePtr start);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlXPtrNewCollapsedRange(start: *mut _xmlNode) -> *mut _xmlXPathObject {
    if start.is_null() {
        return ptr::null_mut();
    }
    new_range_internal(start, -1, ptr::null_mut(), -1)
}

/// Upstream `xmlXPtrNewRangeNodeObject` (xpointer.c): create a range object
/// from a node and a point/range/nodeset object.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlXPathObjectPtr xmlXPtrNewRangeNodeObject(xmlNodePtr start,
///                                             xmlXPathObjectPtr end);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlXPtrNewRangeNodeObject(
    start: *mut _xmlNode,
    end: *mut _xmlXPathObject,
) -> *mut _xmlXPathObject {
    if start.is_null() || end.is_null() {
        return ptr::null_mut();
    }
    let mut end_node: *mut _xmlNode = ptr::null_mut();
    let mut end_index: c_int = -1;
    unsafe {
        match (*end).type_ {
            t if t == xmlXPathObjectType::XPATH_POINT as c_int => {
                end_node = (*end).user as *mut _xmlNode;
                end_index = (*end).index;
            }
            t if t == xmlXPathObjectType::XPATH_RANGE as c_int => {
                end_node = (*end).user2 as *mut _xmlNode;
                end_index = (*end).index2;
            }
            t if t == xmlXPathObjectType::XPATH_NODESET as c_int => {
                // Empty set...
                let ns = (*end).nodesetval as *mut _xmlNodeSet;
                if ns.is_null() || (*ns).nodeNr <= 0 {
                    return ptr::null_mut();
                }
                end_node = *(*ns).nodeTab.add((*ns).nodeNr as usize - 1);
                end_index = -1;
            }
            _ => {
                // TODO
                return ptr::null_mut();
            }
        }
    }
    let ret = new_range_internal(start, -1, end_node, end_index);
    range_check_order(ret);
    ret
}

// ═══════════════════════════════════════════════════════════════════════════════
// Location-set object constructors
// ═══════════════════════════════════════════════════════════════════════════════

/// Upstream `xmlXPtrNewLocationSetNodes` (xpointer.c): create a
/// XPATH_LOCATIONSET object holding a single range built from `start`..`end`
/// (or a collapsed range at `start` when `end` is NULL).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlXPathObjectPtr xmlXPtrNewLocationSetNodes(xmlNodePtr start,
///                                              xmlNodePtr end);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlXPtrNewLocationSetNodes(
    start: *mut _xmlNode,
    end: *mut _xmlNode,
) -> *mut _xmlXPathObject {
    let ret = xmlMallocZero(size_of::<_xmlXPathObject>()) as *mut _xmlXPathObject;
    if ret.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        (*ret).type_ = xmlXPathObjectType::XPATH_LOCATIONSET as c_int;
        let set = if end.is_null() {
            xmlXPtrLocationSetCreate(xmlXPtrNewCollapsedRange(start))
        } else {
            xmlXPtrLocationSetCreate(xmlXPtrNewRangeNodes(start, end))
        };
        (*ret).user = set as *mut c_void;
    }
    ret
}

/// Upstream `xmlXPtrNewLocationSetNodeSet` (xpointer.c): create a
/// XPATH_LOCATIONSET object holding a collapsed range for every node of the
/// node-set.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlXPathObjectPtr xmlXPtrNewLocationSetNodeSet(xmlNodeSetPtr set);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlXPtrNewLocationSetNodeSet(
    set: *mut _xmlNodeSet,
) -> *mut _xmlXPathObject {
    let ret = xmlMallocZero(size_of::<_xmlXPathObject>()) as *mut _xmlXPathObject;
    if ret.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        (*ret).type_ = xmlXPathObjectType::XPATH_LOCATIONSET as c_int;
        if !set.is_null() {
            let newset = xmlXPtrLocationSetCreate(ptr::null_mut());
            if newset.is_null() {
                return ret;
            }
            for i in 0..(*set).nodeNr as usize {
                let node = *(*set).nodeTab.add(i);
                xmlXPtrLocationSetAdd(newset, xmlXPtrNewCollapsedRange(node));
            }
            (*ret).user = newset as *mut c_void;
        }
    }
    ret
}

// ═══════════════════════════════════════════════════════════════════════════════
// Context / evaluation
// ═══════════════════════════════════════════════════════════════════════════════

/// Upstream `xmlXPtrNewContext` (xpointer.c): create a new XPointer-aware
/// XPath context. Sets the `xptr` flag and the `here`/`origin` anchors used by
/// the `here()`/`origin()` XPointer functions.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlXPathContextPtr xmlXPtrNewContext(xmlDocPtr doc, xmlNodePtr here,
///                                      xmlNodePtr origin);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlXPtrNewContext(
    doc: *mut _xmlDoc,
    here: *mut _xmlNode,
    origin: *mut _xmlNode,
) -> *mut _xmlXPathContext {
    let ret = crate::abi::exports_xml2::xmlXPathNewContext(doc);
    if ret.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        (*ret).xptr = 1;
        (*ret).here = here;
        (*ret).origin = origin;
    }
    ret
}

/// Upstream `xmlXPtrRangeToFunction` (xpointer.c): the XPointer range-to()
/// extension function.
///
/// Since libxml2 2.9 this is obsolete (range-to is handled as a location
/// step in xpath.c) and the function only reports an `XPATH_EXPR_ERROR`. The
/// oracle DSO (2.13.9) matches: the export is a tail call to `xmlXPathErr`.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlXPtrRangeToFunction(xmlXPathParserContextPtr ctxt, int nargs);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlXPtrRangeToFunction(ctxt: *mut c_void, _nargs: c_int) {
    if ctxt.is_null() {
        return;
    }
    unsafe {
        pc_set_error(
            ctxt as *mut XmlXPathParserContext,
            XPATH_EXPR_ERROR as c_int,
        )
    };
}

// ═══════════════════════════════════════════════════════════════════════════════
// Node-list construction
// ═══════════════════════════════════════════════════════════════════════════════

/// Upstream `xmlXPtrBuildNodeList` (xpointer.c): build a node-list tree copy
/// of the XPointer result (nodeset / locationset / range / point). Attributes
/// and namespace declarations are dropped.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNodePtr xmlXPtrBuildNodeList(xmlXPathObjectPtr obj);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlXPtrBuildNodeList(obj: *mut _xmlXPathObject) -> *mut _xmlNode {
    let mut list: *mut _xmlNode = ptr::null_mut();
    let mut last: *mut _xmlNode = ptr::null_mut();
    if obj.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        match (*obj).type_ {
            t if t == xmlXPathObjectType::XPATH_NODESET as c_int => {
                let set = (*obj).nodesetval as *mut _xmlNodeSet;
                if set.is_null() {
                    return ptr::null_mut();
                }
                for i in 0..(*set).nodeNr as usize {
                    let node = *(*set).nodeTab.add(i);
                    if node.is_null() {
                        continue;
                    }
                    match (*node).type_ {
                        t2 if t2 == xmlElementType::XML_TEXT_NODE as c_int
                            || t2 == xmlElementType::XML_CDATA_SECTION_NODE as c_int
                            || t2 == xmlElementType::XML_ELEMENT_NODE as c_int
                            || t2 == xmlElementType::XML_ENTITY_REF_NODE as c_int
                            || t2 == xmlElementType::XML_ENTITY_NODE as c_int
                            || t2 == xmlElementType::XML_PI_NODE as c_int
                            || t2 == xmlElementType::XML_COMMENT_NODE as c_int
                            || t2 == xmlElementType::XML_DOCUMENT_NODE as c_int
                            || t2 == xmlElementType::XML_HTML_DOCUMENT_NODE as c_int
                            || t2 == xmlElementType::XML_XINCLUDE_START as c_int
                            || t2 == xmlElementType::XML_XINCLUDE_END as c_int =>
                        {
                            if last.is_null() {
                                list = copy_node(node, 1);
                                last = list;
                            } else {
                                add_sibling(last, copy_node(node, 1));
                                if !(*last).next.is_null() {
                                    last = (*last).next;
                                }
                            }
                        }
                        _ => {
                            // Attributes, namespaces, DTD internals, etc.
                            continue;
                        }
                    }
                }
            }
            t if t == xmlXPathObjectType::XPATH_LOCATIONSET as c_int => {
                let set = (*obj).user as xmlLocationSetPtr;
                if set.is_null() {
                    return ptr::null_mut();
                }
                for i in 0..(*set).locNr as usize {
                    if last.is_null() {
                        list = xmlXPtrBuildNodeList(*(*set).locTab.add(i));
                        last = list;
                    } else {
                        add_sibling(last, xmlXPtrBuildNodeList(*(*set).locTab.add(i)));
                    }
                    if !last.is_null() {
                        while !(*last).next.is_null() {
                            last = (*last).next;
                        }
                    }
                }
            }
            t if t == xmlXPathObjectType::XPATH_RANGE as c_int => {
                return build_range_node_list(obj);
            }
            t if t == xmlXPathObjectType::XPATH_POINT as c_int => {
                return copy_node((*obj).user as *mut _xmlNode, 0);
            }
            _ => {}
        }
    }
    list
}
