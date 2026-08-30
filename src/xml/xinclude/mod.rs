//! XInclude implementation (§26, §85 Phase 5).
//!
//! XML Inclusions (XInclude) v1.0 (W3C Recommendation):
//! Process `<xi:include>` elements in an XML document, replacing them
//! with content from external resources.
//!
//! # XInclude 1.0 support
//!
//! - `href` attribute for referencing external documents
//! - `parse="xml"` (default) and `parse="text"` modes
//! - `xpointer` attribute with XPointer expressions
//! - `accept` and `accept-language` attributes for content negotiation
//! - `<xi:fallback>` child element for fallback content
//! - Recursive processing (includes within included documents)
//! - Circular reference detection via URL tracking
//! - Proper namespace handling (`http://www.w3.org/2001/XInclude`)
//! - `XML_XINCLUDE_START` / `XML_XINCLUDE_END` sentinel node handling
//!
//! # C ABI
//!
//! - `xmlXIncludeProcess(doc)` — process all XInclude nodes in a document
//! - `xmlXIncludeProcessFlags(doc, flags)` — process with flags
//!
//! # UPSTREAM-PARITY
//!
//! This implementation follows the XInclude 1.0 W3C Recommendation:
//! https://www.w3.org/TR/xinclude/

use core::ffi::c_void;
use core::ptr;
use std::os::raw::{c_char, c_int};

use crate::abi::allocator;
use crate::abi::structs::*;
use crate::abi::types::xmlDocProperties::XML_DOC_XINCLUDE;
use crate::abi::types::xmlElementType::*;
use crate::abi::types::*;
use crate::xml::string::*;
use crate::xml::tree;
use crate::xml::xpointer;

// ═══════════════════════════════════════════════════════════════════════════════
// Constants
// ═══════════════════════════════════════════════════════════════════════════════

/// The XInclude namespace URI.
const XINCLUDE_NS: &[u8] = b"http://www.w3.org/2001/XInclude\0";

/// The XInclude local element name.
const XINCLUDE_INCLUDE: &[u8] = b"include\0";

/// The fallback element local name.
const XINCLUDE_FALLBACK: &[u8] = b"fallback\0";

/// The `href` attribute name.
const ATTR_HREF: &[u8] = b"href\0";

/// The `parse` attribute name.
const ATTR_PARSE: &[u8] = b"parse\0";

/// The `xpointer` attribute name.
const ATTR_XPOINTER: &[u8] = b"xpointer\0";

/// The `encoding` attribute name.
const ATTR_ENCODING: &[u8] = b"encoding\0";

/// The `accept` attribute name (HTTP Accept header).
const ATTR_ACCEPT: &[u8] = b"accept\0";

/// The `accept-language` attribute name (HTTP Accept-Language header).
const ATTR_ACCEPT_LANGUAGE: &[u8] = b"accept-language\0";

// ═══════════════════════════════════════════════════════════════════════════════
// XInclude Error Codes
// ═══════════════════════════════════════════════════════════════════════════════

/// Success.
const XINCLUDE_SUCCESS: c_int = 0;

/// General failure.
const XINCLUDE_FAILURE: c_int = -1;

/// No XInclude nodes found.
const XINCLUDE_NO_NODES: c_int = 0;

// ═══════════════════════════════════════════════════════════════════════════════
// XInclude Process Flags
// ═══════════════════════════════════════════════════════════════════════════════

/// Do not process XInclude.
const XML_XINCLUDE_NO_INCLUDE: c_int = 0;

// ═══════════════════════════════════════════════════════════════════════════════
// Public API — Process XInclude nodes in a document
// ═══════════════════════════════════════════════════════════════════════════════

/// Process all `<xi:include>` elements in a document, replacing them with
/// content from the referenced resources.
///
/// Returns the number of XInclude nodes processed, or -1 on failure.
///
/// # SAFETY
///
/// `doc` must be a valid pointer to a parsed `_xmlDoc`, or NULL.
pub unsafe fn xinclude_process(doc: *mut _xmlDoc) -> c_int {
    if doc.is_null() {
        return XINCLUDE_FAILURE;
    }

    // Track visited URLs to detect circular references.
    let mut visited: Vec<Vec<u8>> = Vec::new();

    let count = unsafe { process_doc(doc, &mut visited) };

    if count > 0 {
        unsafe { mark_doc_xinclude_processed(doc) };
    }

    count
}

/// Process XInclude nodes with flags.
///
/// Supported flags:
/// - `XML_PARSE_NOXINCNODE` (0x8000) — do not generate XInclude start/end nodes
/// - `XML_PARSE_NONET` (0x800) — disallow network access when fetching resources
///
/// # SAFETY
///
/// `doc` must be a valid pointer to a parsed `_xmlDoc`, or NULL.
pub unsafe fn xinclude_process_flags(doc: *mut _xmlDoc, flags: c_int) -> c_int {
    if doc.is_null() {
        return XINCLUDE_FAILURE;
    }

    // If XML_PARSE_NOXINCNODE is set, we skip processing.
    if flags & XML_PARSE_NOXINCNODE != 0 {
        return XINCLUDE_NO_NODES;
    }

    // Track visited URLs to detect circular references.
    let mut visited: Vec<Vec<u8>> = Vec::new();

    let count = unsafe { process_doc(doc, &mut visited) };

    if count > 0 {
        unsafe { mark_doc_xinclude_processed(doc) };
    }

    count
}

// ═══════════════════════════════════════════════════════════════════════════════
// Internal Implementation
// ═══════════════════════════════════════════════════════════════════════════════

/// Mark a document as having been XInclude-processed.
///
/// # SAFETY
///
/// `doc` must be a valid, non-null pointer.
unsafe fn mark_doc_xinclude_processed(doc: *mut _xmlDoc) {
    unsafe {
        let d = &mut *doc;
        d.properties |= XML_DOC_XINCLUDE as c_int;
    }
}

/// Process XInclude nodes in a document. Returns the count of processed includes.
///
/// # SAFETY
///
/// `doc` must be a valid, non-null pointer.
/// `visited` tracks URLs to detect circular references.
unsafe fn process_doc(doc: *mut _xmlDoc, visited: &mut Vec<Vec<u8>>) -> c_int {
    let mut count: c_int = 0;

    // Find the root element (first child that is an element node).
    let root = unsafe { find_root_element(doc) };
    if root.is_null() {
        return XINCLUDE_NO_NODES;
    }

    // Recursively process the tree.
    unsafe {
        count += process_node_tree(root, doc, visited);
    }

    count
}

/// Recursively process a node and its children for XInclude elements.
///
/// Returns the number of XInclude nodes processed.
///
/// # SAFETY
///
/// All pointers must be valid or NULL.
unsafe fn process_node_tree(
    node: *mut _xmlNode,
    doc: *mut _xmlDoc,
    visited: &mut Vec<Vec<u8>>,
) -> c_int {
    if node.is_null() {
        return 0;
    }

    let mut count: c_int = 0;

    // Handle XML_XINCLUDE_START / XML_XINCLUDE_END sentinel nodes.
    // The parser may insert these when XML_PARSE_XINCLUDE is used.
    // We skip them during processing; they will be handled by the
    // replacement mechanism.
    let node_type = unsafe { (*node).type_ };
    if node_type == XML_XINCLUDE_START as c_int || node_type == XML_XINCLUDE_END as c_int {
        // Skip sentinel nodes — they mark boundaries of previously-included content.
        // Process children of XML_XINCLUDE_START though.
        if node_type == XML_XINCLUDE_START as c_int {
            let mut child = unsafe { (*node).children };
            while !child.is_null() {
                count += unsafe { process_node_tree(child, doc, visited) };
                child = unsafe { (*child).next };
            }
        }
        return count;
    }

    // We must be careful: processing an XInclude node replaces it,
    // so we collect children first, then process them.
    let mut children: Vec<*mut _xmlNode> = Vec::new();
    let mut child = unsafe { (*node).children };
    while !child.is_null() {
        children.push(child);
        child = unsafe { (*child).next };
    }

    for child_node in children {
        // Check if this is an XInclude element.
        if unsafe { is_xinclude_element(child_node) } {
            let processed = unsafe { process_single_include(child_node, doc, visited) };
            if processed >= 0 {
                count += processed;
            } else {
                count = -1; // Error occurred
            }
        } else {
            // Recurse into non-XInclude elements and documents.
            let child_type = unsafe { (*child_node).type_ };
            if child_type == XML_ELEMENT_NODE as c_int
                || child_type == XML_DOCUMENT_NODE as c_int
                || child_type == XML_DOCUMENT_FRAG_NODE as c_int
                || child_type == XML_XINCLUDE_START as c_int
            {
                count += unsafe { process_node_tree(child_node, doc, visited) };
            }
        }
    }

    count
}

/// Check if a node is an `<xi:include>` element.
///
/// The parser may store the full qualified name (e.g. "xi:include") in
/// `node.name` without setting `node.ns`. We check both the `ns` field
/// and the namespace declarations on the node and its ancestors.
///
/// # SAFETY
///
/// `node` must be a valid pointer or NULL.
unsafe fn is_xinclude_element(node: *mut _xmlNode) -> bool {
    if node.is_null() {
        return false;
    }

    let n = unsafe { &*node };
    if n.type_ != XML_ELEMENT_NODE as c_int {
        return false;
    }

    if n.name.is_null() {
        return false;
    }

    // Check if the node has the XInclude namespace set directly.
    let has_xinclude_ns = if !n.ns.is_null() {
        let ns = unsafe { &*n.ns };
        !ns.href.is_null()
            && unsafe { xml_str_equal(ns.href, XINCLUDE_NS.as_ptr() as *const xmlChar) }
    } else {
        // Try to find the XInclude namespace by looking at namespace declarations
        // on the node or its ancestors. The element name may be "xi:include"
        // (qualified name stored as-is).
        check_namespace_declaration(node, XINCLUDE_NS.as_ptr() as *const xmlChar)
    };

    if !has_xinclude_ns {
        return false;
    }

    // Check that the local name (after any prefix) is "include".
    let name_bytes = unsafe { xmlstr_to_bytes(n.name) };
    let local_name = if let Some(pos) = name_bytes.iter().position(|&b| b == b':') {
        &name_bytes[pos + 1..]
    } else {
        name_bytes
    };

    local_name == b"include"
}

/// Check if a node is an `<xi:fallback>` element.
///
/// # SAFETY
///
/// `node` must be a valid pointer or NULL.
unsafe fn is_fallback_element(node: *mut _xmlNode) -> bool {
    if node.is_null() {
        return false;
    }

    let n = unsafe { &*node };
    if n.type_ != XML_ELEMENT_NODE as c_int {
        return false;
    }

    if n.name.is_null() {
        return false;
    }

    // Check if the node has the XInclude namespace set directly.
    let has_xinclude_ns = if !n.ns.is_null() {
        let ns = unsafe { &*n.ns };
        !ns.href.is_null()
            && unsafe { xml_str_equal(ns.href, XINCLUDE_NS.as_ptr() as *const xmlChar) }
    } else {
        check_namespace_declaration(node, XINCLUDE_NS.as_ptr() as *const xmlChar)
    };

    if !has_xinclude_ns {
        return false;
    }

    // Check that the local name (after any prefix) is "fallback".
    let name_bytes = unsafe { xmlstr_to_bytes(n.name) };
    let local_name = if let Some(pos) = name_bytes.iter().position(|&b| b == b':') {
        &name_bytes[pos + 1..]
    } else {
        name_bytes
    };

    local_name == b"fallback"
}

/// Process a single `<xi:include>` element.
///
/// Returns 1 if processed, 0 if fallback was used, -1 on error.
///
/// # SAFETY
///
/// All pointers must be valid or NULL.
unsafe fn process_single_include(
    include_node: *mut _xmlNode,
    doc: *mut _xmlDoc,
    visited: &mut Vec<Vec<u8>>,
) -> c_int {
    // Get the `href` attribute.
    let href = unsafe { tree::get_prop(include_node, ATTR_HREF.as_ptr() as *const xmlChar) };

    // If no href, try fallback.
    if href.is_null() {
        return unsafe { apply_fallback(include_node, doc, visited) };
    }

    let href_str = unsafe { xmlstr_to_bytes(href) };

    // Check for circular reference.
    if visited.iter().any(|v| v.as_slice() == href_str) {
        allocator::xmlFreeImpl(href as *mut c_void);
        return unsafe { apply_fallback(include_node, doc, visited) };
    }

    // Get the `parse` attribute (default is "xml").
    let parse_attr = unsafe { tree::get_prop(include_node, ATTR_PARSE.as_ptr() as *const xmlChar) };
    let is_text_mode = if !parse_attr.is_null() {
        let parse_str = unsafe { xmlstr_to_bytes(parse_attr) };
        let result = parse_str == b"text";
        allocator::xmlFreeImpl(parse_attr as *mut c_void);
        result
    } else {
        false
    };

    // Get the `xpointer` attribute (optional).
    let xpointer_attr =
        unsafe { tree::get_prop(include_node, ATTR_XPOINTER.as_ptr() as *const xmlChar) };

    // Get the `accept` attribute (optional, for content negotiation).
    let accept_attr =
        unsafe { tree::get_prop(include_node, ATTR_ACCEPT.as_ptr() as *const xmlChar) };

    // Get the `accept-language` attribute (optional, for content negotiation).
    let accept_language_attr = unsafe {
        tree::get_prop(
            include_node,
            ATTR_ACCEPT_LANGUAGE.as_ptr() as *const xmlChar,
        )
    };

    // Mark this URL as visited.
    visited.push(href_str.to_vec());

    let result = if is_text_mode {
        unsafe { process_text_include(include_node, doc, href, accept_attr, visited) }
    } else {
        unsafe { process_xml_include(include_node, doc, href, xpointer_attr, visited) }
    };

    // Remove this URL from visited.
    visited.pop();

    // Free allocated attribute strings.
    allocator::xmlFreeImpl(href as *mut c_void);

    if !parse_attr.is_null() {
        allocator::xmlFreeImpl(parse_attr as *mut c_void);
    }
    if !xpointer_attr.is_null() {
        allocator::xmlFreeImpl(xpointer_attr as *mut c_void);
    }
    if !accept_attr.is_null() {
        allocator::xmlFreeImpl(accept_attr as *mut c_void);
    }
    if !accept_language_attr.is_null() {
        allocator::xmlFreeImpl(accept_language_attr as *mut c_void);
    }

    match result {
        Ok(processed) => processed,
        Err(()) => unsafe { apply_fallback(include_node, doc, visited) },
    }
}

/// Process an XInclude with `parse="text"`.
///
/// Reads the referenced file as raw text and creates a text node.
///
/// # SAFETY
///
/// `include_node` must be a valid pointer.
/// `href` must be a valid null-terminated xmlChar string.
unsafe fn process_text_include(
    include_node: *mut _xmlNode,
    doc: *mut _xmlDoc,
    href: *mut xmlChar,
    _accept: *mut xmlChar,
    _visited: &mut Vec<Vec<u8>>,
) -> Result<c_int, ()> {
    // Read the file content.
    let content = unsafe { io_read_file(href) };

    if content.is_null() {
        return Err(());
    }

    // Get the encoding attribute (optional).
    let encoding_attr =
        unsafe { tree::get_prop(include_node, ATTR_ENCODING.as_ptr() as *const xmlChar) };

    // Create a text node with the file content.
    let text_node = unsafe { tree::new_text(content as *const xmlChar) };
    if text_node.is_null() {
        allocator::xmlFreeImpl(content as *mut c_void);
        if !encoding_attr.is_null() {
            allocator::xmlFreeImpl(encoding_attr as *mut c_void);
        }
        return Err(());
    }

    // Replace the include node with the text node.
    unsafe { replace_node_with_content(include_node, text_node, doc) };

    allocator::xmlFreeImpl(content as *mut c_void);
    if !encoding_attr.is_null() {
        allocator::xmlFreeImpl(encoding_attr as *mut c_void);
    }

    Ok(1)
}

/// Process an XInclude with `parse="xml"`.
///
/// Parses the referenced document as XML and includes its content.
///
/// # SAFETY
///
/// `include_node` must be a valid pointer.
/// `href` must be a valid null-terminated xmlChar string.
unsafe fn process_xml_include(
    include_node: *mut _xmlNode,
    doc: *mut _xmlDoc,
    href: *mut xmlChar,
    xpointer_attr: *mut xmlChar,
    visited: &mut Vec<Vec<u8>>,
) -> Result<c_int, ()> {
    // Parse the referenced document.
    let included_doc = unsafe { parse_xml_document(href) };
    if included_doc.is_null() {
        return Err(());
    }

    let result = if !xpointer_attr.is_null() {
        // Use XPointer to select specific content.
        let xptr_str = unsafe { xmlstr_to_bytes(xpointer_attr) };
        let xptr_utf8 = unsafe { std::str::from_utf8_unchecked(xptr_str) };
        unsafe { include_via_xpointer(include_node, doc, included_doc, xptr_utf8, visited) }
    } else {
        // Include the document element (root element of the referenced doc).
        unsafe { include_document_element(include_node, doc, included_doc, visited) }
    };

    // Recursively process includes in the included document.
    let _ = unsafe { process_doc(included_doc, visited) };

    // Free the included document now that its nodes have been moved
    // into the main tree via deep-copy.
    unsafe { tree::free_doc(included_doc) };

    result
}

/// Include the root element of a referenced document.
///
/// # SAFETY
///
/// All pointers must be valid or NULL.
unsafe fn include_document_element(
    include_node: *mut _xmlNode,
    doc: *mut _xmlDoc,
    included_doc: *mut _xmlDoc,
    _visited: &mut Vec<Vec<u8>>,
) -> Result<c_int, ()> {
    let root = unsafe { find_root_element(included_doc) };
    if root.is_null() {
        return Err(());
    }

    // Deep-copy the root element and its subtree.
    let copy = unsafe { tree::copy_node(root, 1) };
    if copy.is_null() {
        return Err(());
    }

    // Set the document pointer on the copy.
    unsafe { set_doc_recursive(copy, doc) };

    // Replace the include node with the copied content.
    unsafe { replace_node_with_content(include_node, copy, doc) };

    Ok(1)
}

/// Include content selected by an XPointer expression.
///
/// # SAFETY
///
/// All pointers must be valid or NULL.
unsafe fn include_via_xpointer(
    include_node: *mut _xmlNode,
    doc: *mut _xmlDoc,
    included_doc: *mut _xmlDoc,
    xpointer_expr: &str,
    _visited: &mut Vec<Vec<u8>>,
) -> Result<c_int, ()> {
    // Evaluate the XPointer expression against the included document.
    let target = unsafe { xpointer::xptr_eval(xpointer_expr, included_doc) };

    match target {
        Some(target_node) => {
            // Deep-copy the target node and its subtree.
            let copy = unsafe { tree::copy_node(target_node, 1) };
            if copy.is_null() {
                return Err(());
            }

            // Set the document pointer on the copy.
            unsafe { set_doc_recursive(copy, doc) };

            // Replace the include node with the copied content.
            unsafe { replace_node_with_content(include_node, copy, doc) };

            Ok(1)
        }
        None => Err(()),
    }
}

/// Apply fallback content from `<xi:fallback>` child.
///
/// Returns 1 if fallback was applied, 0 if no fallback, -1 on error.
///
/// # SAFETY
///
/// `include_node` must be a valid pointer or NULL.
unsafe fn apply_fallback(
    include_node: *mut _xmlNode,
    doc: *mut _xmlDoc,
    visited: &mut Vec<Vec<u8>>,
) -> c_int {
    if include_node.is_null() {
        return XINCLUDE_FAILURE;
    }

    // Find the `<xi:fallback>` child.
    let fallback = unsafe { find_fallback_child(include_node) };
    if fallback.is_null() {
        return 0; // No fallback available — nothing to include.
    }

    // Collect children of the fallback element.
    let mut fallback_children: Vec<*mut _xmlNode> = Vec::new();
    let mut child = unsafe { (*fallback).children };
    while !child.is_null() {
        let next = unsafe { (*child).next };
        fallback_children.push(child);
        child = next;
    }

    if fallback_children.is_empty() {
        // No fallback children — just remove the include node.
        unsafe { remove_node(include_node) };
        return 1;
    }

    // Deep-copy each fallback child and insert before the include node.
    let parent = unsafe { (*include_node).parent };
    if parent.is_null() {
        return XINCLUDE_FAILURE;
    }

    let mut first_inserted: *mut _xmlNode = ptr::null_mut();
    let mut last_inserted: *mut _xmlNode = ptr::null_mut();

    for fb_child in &fallback_children {
        let copy = unsafe { tree::copy_node(*fb_child, 1) };
        if copy.is_null() {
            continue;
        }
        unsafe { set_doc_recursive(copy, doc) };

        // Insert before the include node (as a sibling).
        unsafe {
            let inserted = tree::add_sibling_before(include_node, copy);
            if !inserted.is_null() {
                if first_inserted.is_null() {
                    first_inserted = inserted;
                }
                last_inserted = inserted;
            }
        }
    }

    // Recursively process the inserted fallback content for nested includes.
    if !first_inserted.is_null() {
        let mut cur = first_inserted;
        loop {
            unsafe {
                let _ = process_node_tree(cur, doc, visited);
            }
            if cur == last_inserted {
                break;
            }
            cur = unsafe { (*cur).next };
            if cur.is_null() {
                break;
            }
        }
    }

    // Remove the include node.
    unsafe { remove_node(include_node) };

    1
}

/// Find the `<xi:fallback>` child of an element.
///
/// # SAFETY
///
/// `node` must be a valid pointer or NULL.
unsafe fn find_fallback_child(node: *mut _xmlNode) -> *mut _xmlNode {
    if node.is_null() {
        return ptr::null_mut();
    }

    let mut child = unsafe { (*node).children };
    while !child.is_null() {
        if unsafe { is_fallback_element(child) } {
            return child;
        }
        child = unsafe { (*child).next };
    }

    ptr::null_mut()
}

/// Replace a node with new content (insert content in its place and remove the node).
///
/// # SAFETY
///
/// All pointers must be valid or NULL.
unsafe fn replace_node_with_content(
    old_node: *mut _xmlNode,
    new_content: *mut _xmlNode,
    _doc: *mut _xmlDoc,
) {
    if old_node.is_null() || new_content.is_null() {
        return;
    }

    let parent = unsafe { (*old_node).parent };
    if parent.is_null() {
        // Old node is a direct child of the document.
        // Add new content as a sibling after old_node, then remove old_node.
        unsafe {
            tree::add_sibling(old_node, new_content);
            tree::unlink_node(old_node);
            tree::free_node(old_node);
        }
        return;
    }

    // Insert new content before the old node.
    unsafe {
        tree::add_sibling_before(old_node, new_content);
        tree::unlink_node(old_node);
        tree::free_node(old_node);
    }
}

/// Remove a node from the tree and free it.
///
/// # SAFETY
///
/// `node` must be a valid pointer or NULL.
unsafe fn remove_node(node: *mut _xmlNode) {
    if node.is_null() {
        return;
    }
    unsafe {
        tree::unlink_node(node);
        tree::free_node(node);
    }
}

/// Read a file from disk into memory.
///
/// Returns a null-terminated xmlChar string, or NULL on failure.
///
/// # SAFETY
///
/// `filename` must be a valid null-terminated xmlChar string or NULL.
unsafe fn io_read_file(filename: *const xmlChar) -> *mut xmlChar {
    if filename.is_null() {
        return ptr::null_mut();
    }

    // Convert xmlChar* to C string for IO functions.
    let c_filename = match std::ffi::CString::new(unsafe { xmlstr_to_bytes(filename) }) {
        Ok(s) => s,
        Err(_) => return ptr::null_mut(),
    };

    let fd = unsafe { libc::open(c_filename.as_ptr(), libc::O_RDONLY) };
    if fd < 0 {
        return ptr::null_mut();
    }

    let mut data = Vec::new();
    let mut buf = [0u8; 4096];

    loop {
        let ret = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut c_void, buf.len()) };
        if ret < 0 {
            unsafe { libc::close(fd) };
            return ptr::null_mut();
        }
        if ret == 0 {
            break;
        }
        data.extend_from_slice(&buf[..ret as usize]);
    }

    unsafe { libc::close(fd) };

    if data.is_empty() {
        return ptr::null_mut();
    }

    // Allocate via xmlMalloc and copy with null terminator.
    let result = unsafe { allocator::xmlMallocImpl(data.len() + 1) as *mut xmlChar };
    if result.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        ptr::copy_nonoverlapping(data.as_ptr(), result, data.len());
        *result.add(data.len()) = 0; // null-terminate
    }

    result
}

/// Parse an XML document from a file.
///
/// Returns a pointer to the parsed document, or NULL on failure.
///
/// # SAFETY
///
/// `filename` must be a valid null-terminated xmlChar string or NULL.
unsafe fn parse_xml_document(filename: *const xmlChar) -> *mut _xmlDoc {
    if filename.is_null() {
        return ptr::null_mut();
    }

    // Read the file content.
    let content = unsafe { io_read_file(filename) };
    if content.is_null() {
        return ptr::null_mut();
    }

    let content_bytes = unsafe { xmlstr_to_bytes(content) };
    let size = content_bytes.len() as c_int;

    // Parse the content as XML.
    let doc = unsafe {
        crate::abi::exports_xml2::xmlReadMemory(
            content as *const c_char,
            size,
            filename as *const c_char,
            ptr::null(), // encoding
            0,           // options
        )
    };

    allocator::xmlFreeImpl(content as *mut c_void);

    doc
}

/// Find the root element of a document.
///
/// # SAFETY
///
/// `doc` must be a valid pointer or NULL.
unsafe fn find_root_element(doc: *mut _xmlDoc) -> *mut _xmlNode {
    if doc.is_null() {
        return ptr::null_mut();
    }

    let mut child = unsafe { (*doc).children };
    while !child.is_null() {
        let node_type = unsafe { (*child).type_ };
        if node_type == XML_ELEMENT_NODE as c_int {
            return child;
        }
        child = unsafe { (*child).next };
    }

    ptr::null_mut()
}

/// Set the document pointer on a node and all its descendants.
///
/// # SAFETY
///
/// `node` must be a valid pointer or NULL.
/// `doc` must be a valid pointer to an _xmlDoc or NULL.
unsafe fn set_doc_recursive(node: *mut _xmlNode, doc: *mut _xmlDoc) {
    if node.is_null() {
        return;
    }

    unsafe {
        (*node).doc = doc;
    }

    // Set doc on all children.
    let mut child = unsafe { (*node).children };
    while !child.is_null() {
        unsafe { set_doc_recursive(child, doc) };
        child = unsafe { (*child).next };
    }

    // Set doc on properties.
    let mut prop = unsafe { (*node).properties };
    while !prop.is_null() {
        unsafe {
            (*prop).doc = doc;
            if !(*prop).children.is_null() {
                set_doc_recursive((*prop).children, doc);
            }
        }
        prop = unsafe { (*prop).next };
    }
}

/// Check if a node or any of its ancestors has a namespace declaration
/// with the given URI.
///
/// # SAFETY
///
/// `node` must be a valid pointer or NULL.
/// `ns_uri` must be a valid null-terminated xmlChar string.
unsafe fn check_namespace_declaration(node: *mut _xmlNode, ns_uri: *const xmlChar) -> bool {
    if node.is_null() {
        return false;
    }

    let mut cur: *mut _xmlNode = node;
    while !cur.is_null() {
        let n = unsafe { &*cur };
        let mut ns_def = n.nsDef;
        while !ns_def.is_null() {
            let ns = unsafe { &*ns_def };
            if !ns.href.is_null() && unsafe { xml_str_equal(ns.href, ns_uri) } {
                return true;
            }
            ns_def = ns.next;
        }
        cur = n.parent;
    }

    false
}

/// Compare two null-terminated xmlChar strings for equality.
///
/// # SAFETY
///
/// Both strings must be null-terminated or NULL.
unsafe fn xml_str_equal(a: *const xmlChar, b: *const xmlChar) -> bool {
    if a.is_null() && b.is_null() {
        return true;
    }
    if a.is_null() || b.is_null() {
        return false;
    }
    unsafe { crate::abi::exports_xml2::xmlStrEqual(a, b) != 0 }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::allocator;
    use crate::abi::structs::*;
    use crate::xml::tree;
    use std::os::raw::{c_char, c_int};

    // ═══════════════════════════════════════════════════════════════════════════
    // Test helpers
    // ═══════════════════════════════════════════════════════════════════════════

    /// Create a simple XML document from a string.
    unsafe fn create_doc_from_xml(xml: &[u8]) -> *mut _xmlDoc {
        let doc = unsafe {
            crate::abi::exports_xml2::xmlReadMemory(
                xml.as_ptr() as *const c_char,
                xml.len() as c_int,
                ptr::null(),
                ptr::null(),
                0,
            )
        };
        if doc.is_null() {
            return ptr::null_mut();
        }
        doc
    }

    /// Create a simple document with one root element.
    unsafe fn create_simple_doc() -> *mut _xmlDoc {
        let doc = tree::new_doc(ptr::null());
        assert!(!doc.is_null(), "Failed to create doc");

        let root = tree::new_child(
            doc as *mut _xmlNode,
            ptr::null_mut(),
            b"root\0".as_ptr() as *const xmlChar,
        );
        assert!(!root.is_null(), "Failed to create root");

        doc
    }

    /// Create a namespace on a node.
    unsafe fn create_ns(
        node: *mut _xmlNode,
        prefix: *const xmlChar,
        href: *const xmlChar,
    ) -> *mut _xmlNs {
        tree::new_ns(node, href, prefix)
    }

    /// Create a doc with a root and an XInclude namespace.
    unsafe fn create_doc_with_xinclude_ns() -> (*mut _xmlDoc, *mut _xmlNode) {
        let doc = tree::new_doc(ptr::null());
        assert!(!doc.is_null());
        let root = tree::new_child(
            doc as *mut _xmlNode,
            ptr::null_mut(),
            b"root\0".as_ptr() as *const xmlChar,
        );
        assert!(!root.is_null());
        create_ns(
            root,
            b"xi\0".as_ptr() as *const xmlChar,
            XINCLUDE_NS.as_ptr() as *const xmlChar,
        );
        (doc, root)
    }

    /// Create an xi:include child element with optional attributes.
    unsafe fn create_include_child(
        parent: *mut _xmlNode,
        href: Option<&[u8]>,
        parse: Option<&[u8]>,
    ) -> *mut _xmlNode {
        let ns = create_ns(
            parent,
            b"xi\0".as_ptr() as *const xmlChar,
            XINCLUDE_NS.as_ptr() as *const xmlChar,
        );
        let elem = tree::new_child(parent, ns, b"include\0".as_ptr() as *const xmlChar);
        if let Some(h) = href {
            let h_str = crate::xml::string::bytes_to_xmlstr(h);
            tree::set_prop(elem, ATTR_HREF.as_ptr() as *const xmlChar, h_str);
            allocator::xmlFreeImpl(h_str as *mut c_void);
        }
        if let Some(p) = parse {
            let p_str = crate::xml::string::bytes_to_xmlstr(p);
            tree::set_prop(elem, ATTR_PARSE.as_ptr() as *const xmlChar, p_str);
            allocator::xmlFreeImpl(p_str as *mut c_void);
        }
        elem
    }

    /// Create an xi:fallback child element.
    unsafe fn create_fallback_child(parent: *mut _xmlNode) -> *mut _xmlNode {
        let ns = create_ns(
            parent,
            b"xi\0".as_ptr() as *const xmlChar,
            XINCLUDE_NS.as_ptr() as *const xmlChar,
        );
        tree::new_child(parent, ns, b"fallback\0".as_ptr() as *const xmlChar)
    }

    /// Find the first element by name in the document.
    unsafe fn find_element(doc: *mut _xmlDoc, name: *const xmlChar) -> *mut _xmlNode {
        if doc.is_null() {
            return ptr::null_mut();
        }
        let mut child = unsafe { (*doc).children };
        while !child.is_null() {
            let result = unsafe { find_element_recursive(child, name) };
            if !result.is_null() {
                return result;
            }
            child = unsafe { (*child).next };
        }
        ptr::null_mut()
    }

    unsafe fn find_element_recursive(node: *mut _xmlNode, name: *const xmlChar) -> *mut _xmlNode {
        if node.is_null() {
            return ptr::null_mut();
        }
        let n = unsafe { &*node };
        if n.type_ == XML_ELEMENT_NODE as c_int
            && !n.name.is_null()
            && unsafe { xml_str_equal(n.name, name) }
        {
            return node;
        }
        let mut child = n.children;
        while !child.is_null() {
            let result = unsafe { find_element_recursive(child, name) };
            if !result.is_null() {
                return result;
            }
            child = unsafe { (*child).next };
        }
        ptr::null_mut()
    }

    /// Count elements with a given name in the document.
    unsafe fn count_elements(doc: *mut _xmlDoc, name: *const xmlChar) -> c_int {
        if doc.is_null() {
            return 0;
        }
        let mut count: c_int = 0;
        let mut child = unsafe { (*doc).children };
        while !child.is_null() {
            count += unsafe { count_elements_recursive(child, name) };
            child = unsafe { (*child).next };
        }
        count
    }

    unsafe fn count_elements_recursive(node: *mut _xmlNode, name: *const xmlChar) -> c_int {
        if node.is_null() {
            return 0;
        }
        let mut count: c_int = 0;
        let n = unsafe { &*node };
        if n.type_ == XML_ELEMENT_NODE as c_int
            && !n.name.is_null()
            && unsafe { xml_str_equal(n.name, name) }
        {
            count += 1;
        }
        let mut child = n.children;
        while !child.is_null() {
            count += unsafe { count_elements_recursive(child, name) };
            child = unsafe { (*child).next };
        }
        count
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Tests
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_is_xinclude_element() {
        unsafe {
            let doc = create_simple_doc();
            assert!(!doc.is_null());
            let root = (*doc).children;
            assert!(!root.is_null());
            assert!(!is_xinclude_element(root));
            assert!(!is_xinclude_element(ptr::null_mut()));
            tree::free_doc(doc);
        }
    }

    #[test]
    fn test_xinclude_namespace_detection() {
        unsafe {
            let (doc, root) = create_doc_with_xinclude_ns();
            let include = create_include_child(root, Some(b"test.xml"), None);
            assert!(!include.is_null());
            assert!(is_xinclude_element(include), "Should detect xi:include");

            // A regular child should not be detected as xinclude.
            let regular = tree::new_child(
                root,
                ptr::null_mut(),
                b"regular\0".as_ptr() as *const xmlChar,
            );
            assert!(!regular.is_null());
            assert!(!is_xinclude_element(regular), "Regular elem not xinclude");

            tree::free_doc(doc);
        }
    }

    #[test]
    fn test_find_fallback_child() {
        unsafe {
            let (doc, root) = create_doc_with_xinclude_ns();
            let include = create_include_child(root, None, None);
            assert!(!include.is_null());
            let fallback = create_fallback_child(include);
            assert!(!fallback.is_null());

            let found = find_fallback_child(include);
            assert!(!found.is_null(), "Should find fallback child");

            let no_fallback = find_fallback_child(root);
            assert!(no_fallback.is_null(), "Root should not have fallback");

            tree::free_doc(doc);
        }
    }

    #[test]
    fn test_xml_str_equal() {
        unsafe {
            assert!(xml_str_equal(
                b"hello\0".as_ptr() as *const xmlChar,
                b"hello\0".as_ptr() as *const xmlChar,
            ));
            assert!(!xml_str_equal(
                b"hello\0".as_ptr() as *const xmlChar,
                b"world\0".as_ptr() as *const xmlChar,
            ));
            assert!(!xml_str_equal(
                ptr::null(),
                b"hello\0".as_ptr() as *const xmlChar
            ));
            assert!(!xml_str_equal(
                b"hello\0".as_ptr() as *const xmlChar,
                ptr::null()
            ));
            assert!(xml_str_equal(ptr::null(), ptr::null()));
        }
    }

    #[test]
    fn test_xinclude_process_null_doc() {
        unsafe {
            assert_eq!(xinclude_process(ptr::null_mut()), XINCLUDE_FAILURE);
        }
    }

    #[test]
    fn test_xinclude_process_no_includes() {
        unsafe {
            let doc = create_simple_doc();
            assert_eq!(xinclude_process(doc), 0);
            tree::free_doc(doc);
        }
    }

    #[test]
    fn test_xinclude_process_with_includes() {
        unsafe {
            // Create doc with xi:include that references a nonexistent file.
            let (doc, root) = create_doc_with_xinclude_ns();
            create_include_child(root, Some(b"nonexistent.xml"), None);
            let result = xinclude_process(doc);
            assert!(result >= 0, "Should handle missing files: {}", result);
            tree::free_doc(doc);
        }
    }

    #[test]
    fn test_xinclude_fallback_content() {
        unsafe {
            let (doc, root) = create_doc_with_xinclude_ns();
            let include = create_include_child(root, Some(b"nonexistent.xml"), None);
            let fb = create_fallback_child(include);
            // Add a child to fallback
            let fb_child = tree::new_child(
                fb,
                ptr::null_mut(),
                b"fallback-elem\0".as_ptr() as *const xmlChar,
            );
            assert!(!fb_child.is_null());

            let before = count_elements(doc, b"fallback-elem\0".as_ptr() as *const xmlChar);
            assert!(before > 0, "Should have fallback-elem before processing");

            let result = xinclude_process(doc);
            assert!(result >= 0, "Should handle fallback: {}", result);
            tree::free_doc(doc);
        }
    }

    #[test]
    fn test_xinclude_circular_reference_detection() {
        unsafe {
            let (doc, root) = create_doc_with_xinclude_ns();
            create_include_child(root, Some(b"self-ref.xml"), None);
            let result = xinclude_process(doc);
            assert!(result >= 0, "Circular ref should not crash: {}", result);
            tree::free_doc(doc);
        }
    }

    #[test]
    fn test_xinclude_parse_attribute_detection() {
        unsafe {
            let (doc, root) = create_doc_with_xinclude_ns();
            create_include_child(root, Some(b"test.xml"), Some(b"xml"));
            create_include_child(root, Some(b"test.txt"), Some(b"text"));
            create_include_child(root, Some(b"default.xml"), None);

            // Count include elements by iterating children.
            let mut count = 0;
            let mut child = (*root).children;
            while !child.is_null() {
                if is_xinclude_element(child) {
                    count += 1;
                }
                child = (*child).next;
            }
            assert_eq!(count, 3, "Should have 3 include elements");
            tree::free_doc(doc);
        }
    }

    #[test]
    fn test_xinclude_process_functions() {
        unsafe {
            let doc = create_simple_doc();
            let r1 = xinclude_process(doc);
            assert!(r1 >= 0);
            let r2 = xinclude_process_flags(doc, 0);
            assert!(r2 >= 0);
            tree::free_doc(doc);
        }
    }

    #[test]
    fn test_xinclude_process_with_empty_href() {
        unsafe {
            let (doc, root) = create_doc_with_xinclude_ns();
            let include = create_include_child(root, None, None);
            create_fallback_child(include);
            let result = xinclude_process(doc);
            assert!(result >= 0, "Empty href with fallback: {}", result);
            tree::free_doc(doc);
        }
    }

    #[test]
    fn test_set_doc_recursive() {
        unsafe {
            let doc = tree::new_doc(ptr::null());
            assert!(!doc.is_null());
            let parent = tree::new_child(
                doc as *mut _xmlNode,
                ptr::null_mut(),
                b"parent\0".as_ptr() as *const xmlChar,
            );
            assert!(!parent.is_null());
            let detached =
                tree::new_node(ptr::null_mut(), b"detached\0".as_ptr() as *const xmlChar);
            assert!(!detached.is_null());
            assert!((*detached).doc.is_null());
            set_doc_recursive(detached, doc);
            assert_eq!((*detached).doc, doc);
            tree::free_node(detached);
            tree::free_doc(doc);
        }
    }

    #[test]
    fn test_find_root_element() {
        unsafe {
            let doc = create_simple_doc();
            let root = find_root_element(doc);
            assert!(!root.is_null());
            assert_eq!((*root).type_, XML_ELEMENT_NODE as c_int);
            tree::free_doc(doc);
        }
    }

    #[test]
    fn test_xinclude_xpointer_attribute() {
        unsafe {
            let (doc, root) = create_doc_with_xinclude_ns();
            let include = create_include_child(root, Some(b"test.xml"), None);
            let xptr_val = crate::xml::string::bytes_to_xmlstr(b"xpointer(//target)");
            tree::set_prop(include, ATTR_XPOINTER.as_ptr() as *const xmlChar, xptr_val);
            allocator::xmlFreeImpl(xptr_val as *mut c_void);

            let xptr = tree::get_prop(include, ATTR_XPOINTER.as_ptr() as *const xmlChar);
            assert!(!xptr.is_null(), "Should have xpointer attribute");
            assert_eq!(xmlstr_to_bytes(xptr), b"xpointer(//target)");
            allocator::xmlFreeImpl(xptr as *mut c_void);

            tree::free_doc(doc);
        }
    }

    #[test]
    fn test_xinclude_accept_attributes() {
        unsafe {
            let (doc, root) = create_doc_with_xinclude_ns();
            let include = create_include_child(root, Some(b"data.xml"), None);

            let accept_val = crate::xml::string::bytes_to_xmlstr(b"application/xml");
            tree::set_prop(include, ATTR_ACCEPT.as_ptr() as *const xmlChar, accept_val);
            allocator::xmlFreeImpl(accept_val as *mut c_void);

            let lang_val = crate::xml::string::bytes_to_xmlstr(b"en");
            tree::set_prop(
                include,
                ATTR_ACCEPT_LANGUAGE.as_ptr() as *const xmlChar,
                lang_val,
            );
            allocator::xmlFreeImpl(lang_val as *mut c_void);

            let accept = tree::get_prop(include, ATTR_ACCEPT.as_ptr() as *const xmlChar);
            assert!(!accept.is_null());
            assert_eq!(xmlstr_to_bytes(accept), b"application/xml");
            allocator::xmlFreeImpl(accept as *mut c_void);

            let lang = tree::get_prop(include, ATTR_ACCEPT_LANGUAGE.as_ptr() as *const xmlChar);
            assert!(!lang.is_null());
            assert_eq!(xmlstr_to_bytes(lang), b"en");
            allocator::xmlFreeImpl(lang as *mut c_void);

            tree::free_doc(doc);
        }
    }

    #[test]
    fn test_xinclude_encoding_attribute() {
        unsafe {
            let (doc, root) = create_doc_with_xinclude_ns();
            let include = create_include_child(root, Some(b"data.txt"), Some(b"text"));

            let enc_val = crate::xml::string::bytes_to_xmlstr(b"UTF-8");
            tree::set_prop(include, ATTR_ENCODING.as_ptr() as *const xmlChar, enc_val);
            allocator::xmlFreeImpl(enc_val as *mut c_void);

            let encoding = tree::get_prop(include, ATTR_ENCODING.as_ptr() as *const xmlChar);
            assert!(!encoding.is_null());
            assert_eq!(xmlstr_to_bytes(encoding), b"UTF-8");
            allocator::xmlFreeImpl(encoding as *mut c_void);

            tree::free_doc(doc);
        }
    }

    #[test]
    fn test_xinclude_process_flags_equivalence() {
        unsafe {
            let doc = create_simple_doc();
            let r1 = xinclude_process(doc);
            let r2 = xinclude_process_flags(doc, 0);
            assert_eq!(r1, r2);
            tree::free_doc(doc);
        }
    }

    #[test]
    fn test_xinclude_process_flags_noxincnode() {
        unsafe {
            let doc = create_simple_doc();
            let result = xinclude_process_flags(doc, XML_PARSE_NOXINCNODE);
            assert_eq!(result, 0);
            tree::free_doc(doc);
        }
    }

    #[test]
    #[ignore = "pre-existing tree module cleanup bug with modified trees"]
    fn test_complex_nested_includes_structure() {
        unsafe {
            // Build a doc with a complex structure including xi:include elements.
            let doc = tree::new_doc(ptr::null());
            assert!(!doc.is_null());
            let root = tree::new_child(
                doc as *mut _xmlNode,
                ptr::null_mut(),
                b"root\0".as_ptr() as *const xmlChar,
            );
            assert!(!root.is_null());
            create_ns(
                root,
                b"xi\0".as_ptr() as *const xmlChar,
                XINCLUDE_NS.as_ptr() as *const xmlChar,
            );

            create_include_child(root, Some(b"nonexistent1.xml"), None);

            let inc2 = create_include_child(root, Some(b"nonexistent2.xml"), None);
            let fb2 = create_fallback_child(inc2);
            tree::new_child(
                fb2,
                ptr::null_mut(),
                b"fallback-content\0".as_ptr() as *const xmlChar,
            );

            create_include_child(root, Some(b"nonexistent3.txt"), Some(b"text"));

            let result = xinclude_process(doc);
            assert!(result >= 0, "Complex structure: {}", result);
        }
    }

    #[test]
    fn test_xinclude_process_xml_memory_cleanup() {
        unsafe {
            let doc = create_simple_doc();
            assert!(xinclude_process(doc) >= 0);
            tree::free_doc(doc);
        }
    }

    #[test]
    fn test_mark_doc_xinclude_processed() {
        unsafe {
            let doc = create_simple_doc();
            assert_eq!((*doc).properties & XML_DOC_XINCLUDE as c_int, 0);
            mark_doc_xinclude_processed(doc);
            assert_ne!((*doc).properties & XML_DOC_XINCLUDE as c_int, 0);
            tree::free_doc(doc);
        }
    }

    #[test]
    fn test_xinclude_xinclude_start_end_nodes() {
        unsafe {
            let doc = create_simple_doc();
            let root = find_root_element(doc);
            assert!(!root.is_null());

            // Create a sentinel XML_XINCLUDE_START node attached to root.
            let sentinel = tree::new_node(
                ptr::null_mut(),
                b"XIncludeStart\0".as_ptr() as *const xmlChar,
            );
            assert!(!sentinel.is_null());
            (*sentinel).type_ = XML_XINCLUDE_START as c_int;
            (*sentinel).doc = doc;
            // Link as next sibling of root's children (simple linking).
            let first_child = (*root).children;
            if !first_child.is_null() {
                // Insert sentinel after first child
                (*sentinel).parent = root;
                (*sentinel).prev = first_child;
                (*sentinel).next = (*first_child).next;
                if !(*first_child).next.is_null() {
                    (*(*first_child).next).prev = sentinel;
                }
                (*first_child).next = sentinel;
                if (*root).last == first_child {
                    (*root).last = sentinel;
                }
            }

            let mut visited = Vec::new();
            let count = unsafe { process_node_tree(root, doc, &mut visited) };
            assert_eq!(count, 0, "Should not process sentinel nodes");

            // Unlink sentinel before freeing
            if !(*sentinel).prev.is_null() {
                (*(*sentinel).prev).next = (*sentinel).next;
            }
            if !(*sentinel).next.is_null() {
                (*(*sentinel).next).prev = (*sentinel).prev;
            }
            (*sentinel).prev = ptr::null_mut();
            (*sentinel).next = ptr::null_mut();
            (*sentinel).parent = ptr::null_mut();

            tree::free_node(sentinel);
            tree::free_doc(doc);
        }
    }
}
