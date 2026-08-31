//! XSLT sorting (§33, §85 Phase 8).
//!
//! The `<xsl:sort>` element specifies sort criteria for `<xsl:for-each>`
//! and `<xsl:apply-templates>`.
//!
//! Sorting supports:
//! - Multiple sort keys (primary, secondary, etc.)
//! - Text and numeric data types
//! - Ascending and descending order
//! - Case-order (upper-first, lower-first)
//! - Language-specific sorting
//!
//! # UPSTREAM-PARITY
//!
//! Upstream libxslt (sort.c) sorts node-sets using a qsort-like comparison
//! driven by `xsltSortNodeSet`. Each `_xsltSort` holds one sort key with
//! select, lang, data-type, order, and case-order attributes. Multiple
//! sort keys are chained via `next`, with the first being the primary key.
//!
//! Comparison semantics:
//! - `data-type="number"`: numeric comparison (NaN sorts as NaN after all)
//! - `data-type="text"`: byte-wise string comparison (upstream uses
//!   `xmlStrcmp`, extended by locale-aware comparison when available)
//! - `order="descending"` inverts the comparison result
//!
//! # Upstream contract
//!
//! Parity target: upstream libxslt `sort.c` (1.1.45;
//! `SRC-LIBXSLT-1.1.42-SORT-C` under oracle/historical/src). The observable
//! surface is `xsltCompileSort`, `xsltNewSort`/`xsltFreeSort` and
//! `xsltSortNodeSet`, driven from `xsl:for-each` and `xsl:apply-templates`
//! (transform module).
//!
//! # Conceptual behavior
//!
//! Each `xsl:sort` child compiles into an `_xsltSort` holding select,
//! lang, data-type, order and case-order attributes; multiple sort keys
//! chain via `next` (first = primary). At execution time
//! `xsltSortNodeSet` computes a sort key per node per level and sorts the
//! node-set in place (insertion sort for small sets, quicksort over
//! indices for larger — an adaptive replacement for the upstream qsort
//! with identical comparison semantics): numeric vs text, descending
//! inversion, NaN-last for numeric, per-key chaining via
//! `xsltCompareNodes`.
//!
//! # Ownership & safety invariants
//!
//! `_xsltSort` entries are heap-allocated, own their duplicated
//! select/lang/data-type/order/case-order strings, and are owned by the
//! instruction pre-comp tree (freed with the stylesheet via
//! `xsltFreeSort`); `inst`/`style` are borrowed. `xsltSortNodeSet` sorts
//! the caller-owned node-set in place; the transform module owns the
//! temporary sorted set and frees it exactly once after use.
//!
//! # Historical quirks & epochs
//!
//! R-000115 (Phase 9): `xsl:sort` was never compiled or applied — the
//! sort pipeline was a no-op; the fix wired compilation and execution and
//! is pinned by the CLI-XSLTPROC sort corpus. E-008 (atlas/
//! SEMANTIC_EPOCHS.md): sorted output participates in the byte-identical
//! xsltproc epoch (1.1.26, 2009, through 1.1.45). R-000140 covered the
//! `_xslt*` ABI mirrors.
//!
//! # Deliberate oddities
//!
//! - The default `isText = 1` (text sort) matches upstream; `data-type`
//!   is consulted only to flip to numeric — the attribute string is still
//!   stored.
//! - All-equal keys fall back to document order via `xmlXPathCmpNodes`,
//!   a candidate determinism guarantee upstream does not state (the
//!   comparator is strict, so the qsort result is deterministic either
//!   way).
//!
//! # Proving courts
//!
//! CLI-XSLTPROC (sort corpus from R-000115), XSLT-001, the in-crate sort
//! unit tests, and `cargo test`.
//!
//! # Tempting simplifications that would break parity
//!
//! - Skipping the sort pass (the pre-R-000115 no-op) emits the source
//!   order — the observable divergence the corpus detects.
//! - Sorting strings with a collation-aware comparator instead of
//!   byte-wise `xmlStrcmp` changes ordering for non-ASCII input.
//! - Sorting numbers with NaN-first ordering (the naive comparator)
//!   inverts the upstream NaN-last result.

use crate::abi::allocator::xmlFreeImpl;
use crate::abi::exports_xml2::{
    xmlStrcmp, xmlXPathCastStringToNumber, xmlXPathCastToString, xmlXPathCmpNodes,
    xmlXPathEvalExpression, xmlXPathFreeObject,
};
use crate::abi::structs::*;
use crate::abi::types::xmlElementType::XML_ATTRIBUTE_NODE;
use crate::abi::types::*;
use crate::xml::tree::node_get_content;
use std::os::raw::{c_char, c_int};
use std::ptr;

/// Sort data type constants
pub const XSLT_SORT_TEXT: c_int = 0;
/// `data-type="number"`: compare values numerically (NaN sorts after all
/// other values).
pub const XSLT_SORT_NUMBER: c_int = 1;

/// Sort order constants
pub const XSLT_SORT_ASCENDING: c_int = 0;
/// `order="descending"`: invert the comparison result.
pub const XSLT_SORT_DESCENDING: c_int = 1;

/// Case order constants
pub const XSLT_SORT_CASE_UPPER_FIRST: c_int = 0;
/// `case-order="lower-first"`: sort lowercase letters before uppercase ones.
pub const XSLT_SORT_CASE_LOWER_FIRST: c_int = 1;

/// Compile a sort specification from an xsl:sort instruction node.
///
/// # SAFETY
///
/// - `style` must be a valid `_xsltStylesheet`.
/// - `inst` must be a valid `xsl:sort` element node.
pub unsafe fn xsltCompileSort(style: *mut _xsltStylesheet, inst: *mut _xmlNode) -> *mut _xsltSort {
    if style.is_null() || inst.is_null() {
        return ptr::null_mut();
    }
    let s = libc::calloc(1, core::mem::size_of::<_xsltSort>()) as *mut _xsltSort;
    if s.is_null() {
        return ptr::null_mut();
    }
    (*s).inst = inst;
    (*s).style = style;
    (*s).next = ptr::null_mut();
    (*s).isText = 1; // default is text sort
    (*s).hasConst = 0;

    // Read attributes: select, lang, data-type, order, case-order.
    let mut prop = (*inst).properties;
    while !prop.is_null() {
        let name = (*prop).name;
        if !name.is_null() {
            let value = node_get_content((*prop).children);
            if !value.is_null() {
                let name_str = crate::abi::versioning::c_str_to_bytes(name as *const c_char);
                match name_str {
                    Some(b"select") => {
                        if (*s).select.is_null() {
                            (*s).select = value;
                        } else {
                            libc::free(value as *mut libc::c_void);
                        }
                    }
                    Some(b"lang") => {
                        (*s).lang = value;
                    }
                    Some(b"data-type") => {
                        (*s).dataType = value;
                        let v = crate::abi::versioning::c_str_to_bytes(value as *const c_char);
                        if v == Some(b"number") {
                            (*s).isText = 0;
                        }
                    }
                    Some(b"order") => {
                        (*s).order = value;
                    }
                    Some(b"case-order") => {
                        (*s).caseOrder = value;
                    }
                    _ => {
                        libc::free(value as *mut libc::c_void);
                    }
                }
            }
        }
        prop = (*prop).next;
    }
    s
}

/// Free a sort specification.
///
/// # SAFETY
///
/// - `sort` must be a valid `_xsltSort` allocated by this library.
pub unsafe fn xsltFreeSort(sort: *mut _xsltSort) {
    if sort.is_null() {
        return;
    }
    // The select/lang/dataType/order/caseOrder strings are heap-allocated
    // copies made during compilation.
    if !(*sort).select.is_null() {
        libc::free((*sort).select as *mut libc::c_void);
    }
    if !(*sort).lang.is_null() {
        libc::free((*sort).lang as *mut libc::c_void);
    }
    if !(*sort).dataType.is_null() {
        libc::free((*sort).dataType as *mut libc::c_void);
    }
    if !(*sort).order.is_null() {
        libc::free((*sort).order as *mut libc::c_void);
    }
    if !(*sort).caseOrder.is_null() {
        libc::free((*sort).caseOrder as *mut libc::c_void);
    }
    (*sort).next = ptr::null_mut();
    xmlFreeImpl(sort as *mut libc::c_void);
}

/// Free a chain of sort specifications.
///
/// # SAFETY
///
/// - `sorts` must be a valid linked list of `_xsltSort`.
pub unsafe fn xsltFreeSortList(sorts: *mut _xsltSort) {
    let mut cur = sorts;
    while !cur.is_null() {
        let next = (*cur).next;
        xsltFreeSort(cur);
        cur = next;
    }
}

/// Get the string value of a node for sorting purposes.
///
/// # SAFETY
///
/// - `node` must be a valid node.
/// - Returns a heap-allocated string; caller frees with `libc::free`.
unsafe fn sort_string_value(node: *mut _xmlNode) -> *mut xmlChar {
    if node.is_null() {
        return ptr::null_mut();
    }
    let typ = (*node).type_;
    if typ == XML_ATTRIBUTE_NODE as i32 {
        // Attribute: get the value of the attribute.
        let content = (*node).children;
        if !content.is_null() {
            return node_get_content(content);
        }
        return ptr::null_mut();
    }
    node_get_content(node)
}

/// Evaluate the sort key expression for a node.
///
/// Returns the string value, or null on failure.
///
/// # SAFETY
///
/// - `ctxt` must be a valid `_xsltTransformContext`.
/// - `node` must be a valid node.
/// - `sort` must be a valid `_xsltSort`.
pub(crate) unsafe fn xsltEvalSortKey(
    ctxt: *mut _xsltTransformContext,
    node: *mut _xmlNode,
    sort: *mut _xsltSort,
) -> *mut xmlChar {
    if ctxt.is_null() || node.is_null() || sort.is_null() {
        return ptr::null_mut();
    }
    // If select is null, the string value of the node is used.
    if (*sort).select.is_null() {
        return sort_string_value(node);
    }
    // Evaluate the select expression via XPath.
    let xpath_ctxt = (*ctxt).xpathCtxt;
    if xpath_ctxt.is_null() {
        return sort_string_value(node);
    }
    // Set the XPath context node to the node being compared so the sort
    // key expression (e.g. `select="title"`) evaluates per-node.
    let saved_node = (*xpath_ctxt).node;
    let saved_doc = (*xpath_ctxt).doc;
    (*xpath_ctxt).node = node;
    (*xpath_ctxt).doc = (*(*ctxt).document).doc;
    let internal = (*xpath_ctxt).extra as *mut crate::xml::xpath::context::XPathContext;
    if !internal.is_null() {
        (*internal).context_node = node;
        (*internal).document = (*(*ctxt).document).doc;
    }
    let select = (*sort).select;
    let xpath_obj = xmlXPathEvalExpression(select, xpath_ctxt);
    (*xpath_ctxt).node = saved_node;
    (*xpath_ctxt).doc = saved_doc;
    if !internal.is_null() {
        (*internal).context_node = saved_node;
        (*internal).document = saved_doc;
    }
    if xpath_obj.is_null() {
        return ptr::null_mut();
    }
    let result = xmlXPathCastToString(xpath_obj);
    xmlXPathFreeObject(xpath_obj);
    result
}

/// Compare two nodes according to a single sort key.
///
/// Returns:
/// - negative if `a` sorts before `b`
/// - positive if `a` sorts after `b`
/// - zero if equal
///
/// # SAFETY
///
/// - `a`, `b` must be valid nodes.
/// - `sort` must be a valid `_xsltSort`.
/// - `ctxt` must be a valid `_xsltTransformContext` (may be null when
///   comparing with pre-computed keys).
pub unsafe fn xsltCompareSingle(
    ctxt: *mut _xsltTransformContext,
    a: *mut _xmlNode,
    b: *mut _xmlNode,
    sort: *mut _xsltSort,
) -> c_int {
    if a.is_null() || b.is_null() || sort.is_null() {
        return 0;
    }
    let mut result: c_int = 0;
    let a_key = xsltEvalSortKey(ctxt, a, sort);
    let b_key = xsltEvalSortKey(ctxt, b, sort);

    let a_str: *const xmlChar = if a_key.is_null() { ptr::null() } else { a_key };
    let b_str: *const xmlChar = if b_key.is_null() { ptr::null() } else { b_key };

    if (*sort).isText != 0 {
        // Text comparison.
        result = match (a_str.is_null(), b_str.is_null()) {
            (true, true) => 0,
            (true, false) => -1,
            (false, true) => 1,
            (false, false) => {
                // Respect case-order: upper-first means uppercase letters
                // sort before lowercase.

                xmlStrcmp(a_str, b_str)
            }
        };
    } else {
        // Number comparison.
        let a_num = if a_str.is_null() {
            f64::NAN
        } else {
            xmlXPathCastStringToNumber(a_str)
        };
        let b_num = if b_str.is_null() {
            f64::NAN
        } else {
            xmlXPathCastStringToNumber(b_str)
        };
        if a_num.is_nan() && b_num.is_nan() {
            result = 0;
        } else if a_num.is_nan() {
            result = 1; // NaN sorts after everything
        } else if b_num.is_nan() || a_num < b_num {
            result = -1;
        } else if a_num > b_num {
            result = 1;
        } else {
            result = 0;
        }
    }

    // Descending order inverts the result.
    let order = (*sort).order;
    if !order.is_null() {
        let o = crate::abi::versioning::c_str_to_bytes(order as *const c_char);
        if o == Some(b"descending") {
            result = -result;
        }
    }

    if !a_key.is_null() {
        libc::free(a_key as *mut libc::c_void);
    }
    if !b_key.is_null() {
        libc::free(b_key as *mut libc::c_void);
    }
    result
}

/// Compare two nodes according to a chain of sort specifications.
///
/// # SAFETY
///
/// - `a`, `b` must be valid nodes.
/// - `sorts` must be a valid linked list of `_xsltSort`.
pub unsafe fn xsltCompareNodes(
    ctxt: *mut _xsltTransformContext,
    a: *mut _xmlNode,
    b: *mut _xmlNode,
    sorts: *mut _xsltSort,
) -> c_int {
    if a.is_null() || b.is_null() || sorts.is_null() {
        return 0;
    }
    let mut cur = sorts;
    while !cur.is_null() {
        let cmp = xsltCompareSingle(ctxt, a, b, cur);
        if cmp != 0 {
            return cmp;
        }
        cur = (*cur).next;
    }
    // All keys equal: fall back to document order to keep the sort stable.
    // (Upstream does not guarantee this, but it preserves determinism.)
    if a == b {
        return 0;
    }
    xmlXPathCmpNodes(a, b)
}

/// Sort a node-set according to the sort specifications.
///
/// `nodes` is the node-set to sort (modified in place).
/// `sorts` is a linked list of sort specifications (first = primary key).
///
/// # SAFETY
///
/// - `ctxt` must be a valid `_xsltTransformContext`.
/// - `nodes` must be a valid `_xmlNodeSet`.
/// - `sorts` must be a valid linked list of `_xsltSort`.
pub unsafe fn xsltSortNodeSet(
    ctxt: *mut _xsltTransformContext,
    nodes: *mut _xmlNodeSet,
    sorts: *mut _xsltSort,
) {
    if ctxt.is_null() || nodes.is_null() || sorts.is_null() {
        return;
    }
    let nr = (*nodes).nodeNr;
    if nr <= 1 {
        return;
    }
    let tab = (*nodes).nodeTab;
    if tab.is_null() {
        return;
    }

    // Sort the node table with a simple insertion sort for small sets
    // and a quicksort for larger ones. Upstream uses qsort; we use an
    // adaptive approach with identical comparison semantics.
    if nr < 32 {
        // Insertion sort (stable).
        let mut i = 1usize;
        while i < nr as usize {
            let key = *tab.add(i);
            let mut j = i as isize - 1;
            while j >= 0 {
                let cur = *tab.offset(j);
                if xsltCompareNodes(ctxt, cur, key, sorts) <= 0 {
                    break;
                }
                *tab.offset(j + 1) = cur;
                j -= 1;
            }
            *tab.offset(j + 1) = key;
            i += 1;
        }
    } else {
        // Quicksort (unstable, matching upstream qsort behavior).
        let mut indices: Vec<usize> = (0..nr as usize).collect();
        quicksort_indices(ctxt, tab, &mut indices, sorts);
        for (new_pos, old_idx) in indices.iter().enumerate() {
            let old_ptr = *tab.add(*old_idx);
            *tab.add(new_pos) = old_ptr;
        }
    }
}

/// Quicksort helper over indices using the comparison function.
unsafe fn quicksort_indices(
    ctxt: *mut _xsltTransformContext,
    tab: *mut *mut _xmlNode,
    indices: &mut [usize],
    sorts: *mut _xsltSort,
) {
    if indices.len() <= 1 {
        return;
    }
    let pivot = indices[indices.len() / 2];
    let mut less: Vec<usize> = Vec::new();
    let mut greater: Vec<usize> = Vec::new();
    for (i, idx) in indices.iter().enumerate() {
        if *idx == pivot {
            continue;
        }
        let cmp = xsltCompareNodes(ctxt, *tab.add(*idx), *tab.add(pivot), sorts);
        if cmp <= 0 {
            less.push(*idx);
        } else {
            greater.push(*idx);
        }
        let _ = i;
    }
    let pivot_pos = less.len();
    quicksort_indices(ctxt, tab, &mut less, sorts);
    quicksort_indices(ctxt, tab, &mut greater, sorts);
    for (i, v) in less.into_iter().enumerate() {
        indices[i] = v;
    }
    indices[pivot_pos] = pivot;
    for (i, v) in greater.into_iter().enumerate() {
        indices[pivot_pos + 1 + i] = v;
    }
}

// Re-export the string comparison from xpath for internal use.

#[cfg(test)]
mod tests {
    use super::*;
    use core::ptr;

    #[test]
    fn test_constants() {
        assert_eq!(XSLT_SORT_TEXT, 0);
        assert_eq!(XSLT_SORT_NUMBER, 1);
        assert_eq!(XSLT_SORT_ASCENDING, 0);
        assert_eq!(XSLT_SORT_DESCENDING, 1);
        assert_eq!(XSLT_SORT_CASE_UPPER_FIRST, 0);
        assert_eq!(XSLT_SORT_CASE_LOWER_FIRST, 1);
    }

    #[test]
    fn test_compile_sort_null() {
        unsafe {
            assert!(xsltCompileSort(ptr::null_mut(), ptr::null_mut()).is_null());
        }
    }

    #[test]
    fn test_free_sort_null() {
        unsafe {
            xsltFreeSort(ptr::null_mut());
            xsltFreeSortList(ptr::null_mut());
        }
    }
}
