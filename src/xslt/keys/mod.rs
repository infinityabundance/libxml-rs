//! XSLT keys and the key() function (§33, §85 Phase 8).
//!
//! Keys provide a way to index nodes by specific criteria.
//! Defined with `<xsl:key>` and accessed with the `key()` function.
//!
//! The key() function can be used in:
//! - Match patterns in `<xsl:template match="key('name', 'value')">`
//! - XPath expressions in select attributes
//! - Predicates
//!
//! # UPSTREAM-PARITY
//!
//! Upstream libxslt (keys.c) builds key tables at the start of a
//! transformation. Each `_xsltKeyDef` has a name, match pattern, and use
//! expression. The key table maps values → node-sets. The `key()` function
//! looks up values in the table for the current document.
//!
//! Table building: for each node matching the match pattern, the use
//! expression is evaluated. Each resulting string value maps to the node.
//! A node can appear under multiple values.

use crate::abi::allocator::xmlFree;
use crate::abi::exports_xml2::{
    xmlXPathCastToString, xmlXPathEvalExpression, xmlXPathFreeObject, xmlXPathNewNodeSet,
    xmlXPathNodeSetCreate,
};
use crate::abi::structs::*;
use crate::abi::types::*;
use crate::xml::tree::doc_get_root_element;
use crate::xslt::patterns::{_xsltPattern, xsltCompilePattern, xsltFreePattern, xsltTestPattern};
use std::ffi::c_void;
use std::os::raw::c_int;
use std::ptr;

/// Add a key definition to a stylesheet.
///
/// # SAFETY
///
/// - `style` must be a valid `_xsltStylesheet`.
/// - `name`, `match_pattern`, `use_expr` must be valid NUL-terminated strings.
/// - `inst` must be a valid `xsl:key` element node.
pub unsafe fn xsltAddKeyDef(
    style: *mut _xsltStylesheet,
    name: *const xmlChar,
    match_pattern: *const xmlChar,
    use_expr: *const xmlChar,
    inst: *mut _xmlNode,
) -> c_int {
    if style.is_null() || name.is_null() || match_pattern.is_null() || use_expr.is_null() {
        return -1;
    }
    let def = libc::calloc(1, core::mem::size_of::<_xsltKeyDef>()) as *mut _xsltKeyDef;
    if def.is_null() {
        return -1;
    }
    (*def).style = style;
    (*def).inst = inst;
    (*def).name = name;
    (*def).r#match = match_pattern;
    (*def).r#use = use_expr;
    // Prepend to the stylesheet's key list (upstream appends; order is
    // preserved for matching semantics).
    (*def).next = (*style).keys;
    (*style).keys = def;
    0
}

/// Free a single key definition.
///
/// # SAFETY
///
/// - `key_def` must be a valid `_xsltKeyDef` allocated by this library.
pub unsafe fn xsltFreeKeyDef(key_def: *mut _xsltKeyDef) {
    if key_def.is_null() {
        return;
    }
    // name/match/use are dictionary-owned strings (borrowed), do not free.
    (*key_def).next = ptr::null_mut();
    xmlFree(key_def as *mut libc::c_void);
}

/// Free key definitions in a stylesheet.
///
/// # SAFETY
///
/// - `style` must be a valid `_xsltStylesheet`.
pub unsafe fn xsltFreeKeys(style: *mut _xsltStylesheet) {
    if style.is_null() {
        return;
    }
    let mut cur = (*style).keys;
    (*style).keys = ptr::null_mut();
    while !cur.is_null() {
        let next = (*cur).next;
        xsltFreeKeyDef(cur);
        cur = next;
    }
}

/// Free a single key table entry.
///
/// # SAFETY
///
/// - `key_table` must be a valid `_xsltKeyTable` allocated by this library.
pub unsafe fn xsltFreeKeyTable(key_table: *mut _xsltKeyTable) {
    if key_table.is_null() {
        return;
    }
    if !(*key_table).table.is_null() {
        let mut i = 0;
        while i < (*key_table).nb {
            let obj = *(*key_table).table.offset(i as isize);
            if !obj.is_null() {
                xmlXPathFreeObject(obj);
            }
            i += 1;
        }
        libc::free((*key_table).table as *mut libc::c_void);
    }
    if !(*key_table).name.is_null() {
        libc::free((*key_table).name as *mut libc::c_void);
    }
    (*key_table).next = ptr::null_mut();
    xmlFree(key_table as *mut libc::c_void);
}

/// Free all key tables in a transform context.
///
/// # SAFETY
///
/// - `ctxt` must be a valid `_xsltTransformContext`.
pub unsafe fn xsltFreeKeyTables(ctxt: *mut _xsltTransformContext) {
    if ctxt.is_null() {
        return;
    }
    let mut cur = (*ctxt).keyTables as *mut _xsltKeyTable;
    (*ctxt).keyTables = ptr::null_mut();
    while !cur.is_null() {
        let next = (*cur).next;
        xsltFreeKeyTable(cur);
        cur = next;
    }
}

/// Initialize key tables for a transformation.
///
/// Evaluates the key definitions against the source document and builds
/// the lookup tables.
///
/// # SAFETY
///
/// - `ctxt` must be a valid `_xsltTransformContext`.
/// - `style` must be a valid `_xsltStylesheet`.
pub unsafe fn xsltInitKeys(ctxt: *mut _xsltTransformContext, style: *mut _xsltStylesheet) -> c_int {
    if ctxt.is_null() || style.is_null() {
        return -1;
    }
    let doc = (*ctxt).document;
    if doc.is_null() {
        return 0;
    }
    let root = doc_get_root_element(doc);
    if root.is_null() {
        return 0;
    }

    let mut def = (*style).keys;
    while !def.is_null() {
        // Compile the match pattern and the use expression.
        let pat = xsltCompilePattern((*def).r#match, doc);
        if pat.is_null() {
            def = (*def).next;
            continue;
        }
        let use_expr = (*def).r#use;
        // Create a key table for this key name.
        let table = libc::calloc(1, core::mem::size_of::<_xsltKeyTable>()) as *mut _xsltKeyTable;
        if table.is_null() {
            xsltFreePattern(pat);
            def = (*def).next;
            continue;
        }
        let name_len = libc::strlen((*def).name as *const libc::c_char);
        let cname = libc::malloc(name_len + 1) as *mut xmlChar;
        if !cname.is_null() {
            libc::memcpy(
                cname as *mut libc::c_void,
                (*def).name as *const libc::c_void,
                name_len,
            );
            *cname.add(name_len) = 0;
        }
        (*table).name = cname;
        (*table).depth = (*def).depth;
        (*table).nb = 0;
        (*table).max = 0;
        (*table).table = ptr::null_mut();

        // Walk the document tree and build the index.
        build_key_table(ctxt, doc, root, pat, use_expr, table);

        xsltFreePattern(pat);

        // Prepend the table to the context's key table list.
        (*table).next = (*ctxt).keyTables as *mut _xsltKeyTable;
        (*ctxt).keyTables = table as *mut c_void;

        def = (*def).next;
    }
    0
}

/// Build the key table for a document by walking the tree.
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn build_key_table(
    ctxt: *mut _xsltTransformContext,
    doc: *mut _xmlDoc,
    node: *mut _xmlNode,
    pattern: *mut _xsltPattern,
    use_expr: *const xmlChar,
    table: *mut _xsltKeyTable,
) {
    if node.is_null() {
        return;
    }
    // Test the match pattern against this node.
    if xsltTestPattern(ctxt, pattern, node) != 0 {
        // Evaluate the use expression with this node as context.
        let xpath_ctxt = (*ctxt).xpathCtxt;
        if !xpath_ctxt.is_null() && !use_expr.is_null() {
            let saved_node = (*xpath_ctxt).node;
            let saved_doc = (*xpath_ctxt).doc;
            let saved_pos = (*xpath_ctxt).proximityPosition;
            let saved_size = (*xpath_ctxt).contextSize;
            (*xpath_ctxt).node = node;
            (*xpath_ctxt).doc = doc;
            (*xpath_ctxt).proximityPosition = 1;
            (*xpath_ctxt).contextSize = 1;
            let obj = xmlXPathEvalExpression(use_expr, xpath_ctxt);
            (*xpath_ctxt).node = saved_node;
            (*xpath_ctxt).doc = saved_doc;
            (*xpath_ctxt).proximityPosition = saved_pos;
            (*xpath_ctxt).contextSize = saved_size;
            if !obj.is_null() {
                // Each string value in the result maps to this node.
                let strv = xmlXPathCastToString(obj);
                if !strv.is_null() {
                    add_key_entry(table, strv, node);
                    libc::free(strv as *mut libc::c_void);
                }
                xmlXPathFreeObject(obj);
            }
        }
    }
    // Recurse into children.
    let mut child = (*node).children;
    while !child.is_null() {
        let next = (*child).next;
        build_key_table(ctxt, doc, child, pattern, use_expr, table);
        child = next;
    }
    // Recurse into attributes (keys can match attribute values via use).
    let mut prop = (*node).properties;
    while !prop.is_null() {
        let next = (*prop).next;
        build_key_table(ctxt, doc, prop as *mut _xmlNode, pattern, use_expr, table);
        prop = next;
    }
}

/// Add an entry to the key table: value → node.
///
/// # SAFETY
///
/// - `table` must be valid.
/// - `value` must be a NUL-terminated string.
/// - `node` must be a valid node.
unsafe fn add_key_entry(table: *mut _xsltKeyTable, value: *const xmlChar, node: *mut _xmlNode) {
    if table.is_null() || value.is_null() || node.is_null() {
        return;
    }
    // Look for an existing entry with this value.
    let mut i = 0;
    while i < (*table).nb {
        let obj = *(*table).table.offset(i as isize);
        if !obj.is_null()
            && (*obj).stringval != ptr::null_mut()
            && libc::strcmp(
                (*obj).stringval as *const libc::c_char,
                value as *const libc::c_char,
            ) == 0
        {
            // Append the node to this entry's node-set.
            let ns = (*obj).nodesetval as *mut _xmlNodeSet;
            if !ns.is_null() {
                if (*ns).nodeNr >= (*ns).nodeMax {
                    let new_max = if (*ns).nodeMax == 0 {
                        8
                    } else {
                        (*ns).nodeMax * 2
                    };
                    let new_tab = libc::realloc(
                        (*ns).nodeTab as *mut libc::c_void,
                        (new_max as usize) * core::mem::size_of::<*mut _xmlNode>(),
                    ) as *mut *mut _xmlNode;
                    if new_tab.is_null() {
                        return;
                    }
                    (*ns).nodeTab = new_tab;
                    (*ns).nodeMax = new_max;
                }
                *(*ns).nodeTab.offset((*ns).nodeNr as isize) = node;
                (*ns).nodeNr += 1;
            }
            return;
        }
        i += 1;
    }
    // Create a new entry.
    let obj = xmlXPathNewNodeSet(node);
    if obj.is_null() {
        return;
    }
    // Copy the value string.
    let vlen = libc::strlen(value as *const libc::c_char);
    let vcopy = libc::malloc(vlen + 1) as *mut xmlChar;
    if vcopy.is_null() {
        xmlXPathFreeObject(obj);
        return;
    }
    libc::memcpy(
        vcopy as *mut libc::c_void,
        value as *const libc::c_void,
        vlen,
    );
    *vcopy.add(vlen) = 0;
    (*obj).stringval = vcopy;

    // Grow the table if needed.
    if (*table).nb >= (*table).max {
        let new_max = if (*table).max == 0 {
            16
        } else {
            (*table).max * 2
        };
        let new_tab = libc::realloc(
            (*table).table as *mut libc::c_void,
            (new_max as usize) * core::mem::size_of::<*mut _xmlXPathObject>(),
        ) as *mut *mut _xmlXPathObject;
        if new_tab.is_null() {
            xmlXPathFreeObject(obj);
            return;
        }
        (*table).table = new_tab;
        (*table).max = new_max;
    }
    *(*table).table.offset((*table).nb as isize) = obj;
    (*table).nb += 1;
}

/// Evaluate the key() function.
///
/// Returns a new node-set of nodes matching the key criteria.
///
/// # SAFETY
///
/// - `ctxt` must be a valid `_xsltTransformContext`.
/// - `name` and `value` must be valid NUL-terminated strings.
pub unsafe fn xsltEvalKeyFunction(
    ctxt: *mut _xsltTransformContext,
    name: *const xmlChar,
    value: *const xmlChar,
) -> *mut _xmlNodeSet {
    if ctxt.is_null() || name.is_null() || value.is_null() {
        return ptr::null_mut();
    }
    // Find the key table for this name.
    let mut table = (*ctxt).keyTables as *mut _xsltKeyTable;
    while !table.is_null() {
        if !(*table).name.is_null()
            && libc::strcmp(
                (*table).name as *const libc::c_char,
                name as *const libc::c_char,
            ) == 0
        {
            // Look up the value.
            let mut i = 0;
            while i < (*table).nb {
                let obj = *(*table).table.offset(i as isize);
                if !obj.is_null()
                    && !(*obj).stringval.is_null()
                    && libc::strcmp(
                        (*obj).stringval as *const libc::c_char,
                        value as *const libc::c_char,
                    ) == 0
                {
                    // Copy the node-set.
                    let ns = (*obj).nodesetval as *mut _xmlNodeSet;
                    if ns.is_null() {
                        return ptr::null_mut();
                    }
                    // Copy the node-set via a fresh node-set built from the
                    // source entries.
                    let copy = xmlXPathNodeSetCreate(ptr::null_mut());
                    if copy.is_null() {
                        return ptr::null_mut();
                    }
                    let mut k = 0;
                    while k < (*ns).nodeNr {
                        let node = *(*ns).nodeTab.offset(k as isize);
                        add_to_node_set(copy, node);
                        k += 1;
                    }
                    return copy;
                }
                i += 1;
            }
            return ptr::null_mut();
        }
        table = (*table).next;
    }
    ptr::null_mut()
}

/// Append a node to a node-set (deduplicating).
///
/// # SAFETY
///
/// - `ns` must be a valid `_xmlNodeSet` with capacity to grow.
/// - `node` must be a valid node.
unsafe fn add_to_node_set(ns: *mut _xmlNodeSet, node: *mut _xmlNode) {
    if ns.is_null() || node.is_null() {
        return;
    }
    // Deduplicate.
    let mut i = 0;
    while i < (*ns).nodeNr {
        if *(*ns).nodeTab.offset(i as isize) == node {
            return;
        }
        i += 1;
    }
    if (*ns).nodeNr >= (*ns).nodeMax {
        let new_max = if (*ns).nodeMax == 0 {
            8
        } else {
            (*ns).nodeMax * 2
        };
        let new_tab = libc::realloc(
            (*ns).nodeTab as *mut libc::c_void,
            (new_max as usize) * core::mem::size_of::<*mut _xmlNode>(),
        ) as *mut *mut _xmlNode;
        if new_tab.is_null() {
            return;
        }
        (*ns).nodeTab = new_tab;
        (*ns).nodeMax = new_max;
    }
    *(*ns).nodeTab.offset((*ns).nodeNr as isize) = node;
    (*ns).nodeNr += 1;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::structs::*;
    use core::ptr;

    #[test]
    fn test_add_key_def_null() {
        unsafe {
            assert_eq!(
                xsltAddKeyDef(
                    ptr::null_mut(),
                    ptr::null(),
                    ptr::null(),
                    ptr::null(),
                    ptr::null_mut()
                ),
                -1
            );
        }
    }

    #[test]
    fn test_free_null() {
        unsafe {
            xsltFreeKeys(ptr::null_mut());
            xsltFreeKeyDef(ptr::null_mut());
            xsltFreeKeyTable(ptr::null_mut());
            xsltFreeKeyTables(ptr::null_mut());
        }
    }
}
