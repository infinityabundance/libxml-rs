//! Debug/memory debugging infrastructure (§85 Phase 7).
//!
//! UPSTREAM-PARITY: Corresponds to `debugXML.c` / `debugXML.h` in libxml2.
//!
//! libxml2's debug APIs for printing tree structure, XPath expressions, etc.
//! These are used by `xmllint --debug` and other diagnostic tools.

use crate::abi::allocator::xmlFree;
use crate::abi::structs::{_xmlAttr, _xmlDoc, _xmlNode, _xmlNs};
use core::ffi::{c_char, c_int, c_void};
use core::ptr;

/// Maximum indentation depth for debug output.
const MAX_DEPTH: c_int = 100;

/// Check if a node is an XInclude start node.
///
/// UPSTREAM-PARITY: `xmlDebugIsXInclude()` — internal check used by debug dumper.
fn is_xinclude_node(node: *mut _xmlNode) -> bool {
    if node.is_null() {
        return false;
    }
    unsafe {
        let ns = (*node).ns;
        if ns.is_null() {
            return false;
        }
        let ns_href = (*ns).href;
        let ns_prefix = (*ns).prefix;
        if ns_href.is_null() {
            return false;
        }
        // Check for XInclude namespace
        let href = core::slice::from_raw_parts(ns_href as *const u8, 30);
        let xi_ns = b"http://www.w3.org/2001/XInclude\0";
        let mut matches = true;
        for i in 0..30 {
            if i >= href.len() || href[i] != xi_ns[i] {
                matches = false;
                break;
            }
        }
        if !matches {
            return false;
        }
        // Check for xi:include element
        let name_bytes = if !(*node).name.is_null() {
            core::slice::from_raw_parts((*node).name as *const u8, 8)
        } else {
            return false;
        };
        name_bytes.len() >= 7 && &name_bytes[..7] == b"include"
    }
}

/// Convert a boolean to text.
///
/// UPSTREAM-PARITY: `xmlBoolToText()`
#[no_mangle]
pub unsafe extern "C" fn xmlBoolToText(boolval: c_int) -> *const c_char {
    if boolval != 0 {
        b"true\0".as_ptr() as *const c_char
    } else {
        b"false\0".as_ptr() as *const c_char
    }
}

/// Dump a debug representation of an xmlChar string.
///
/// UPSTREAM-PARITY: `xmlDebugDumpString()`
#[no_mangle]
pub unsafe extern "C" fn xmlDebugDumpString(output: *mut _IO_FILE, str_val: *const u8) {
    if output.is_null() {
        return;
    }
    if str_val.is_null() {
        unsafe {
            libc::fprintf(output, b"(null)\0".as_ptr() as *const c_char);
        }
        return;
    }
    unsafe {
        let mut i = 0;
        loop {
            let c = *str_val.add(i);
            if c == 0 {
                break;
            }
            if c == b'\n' {
                libc::fprintf(output, b"\\n\0".as_ptr() as *const c_char);
            } else if c == b'\r' {
                libc::fprintf(output, b"\\r\0".as_ptr() as *const c_char);
            } else if c == b'\t' {
                libc::fprintf(output, b"\\t\0".as_ptr() as *const c_char);
            } else if c < 0x20 || c >= 0x7f {
                libc::fprintf(output, b"\\x%02x\0".as_ptr() as *const c_char, c as c_int);
            } else {
                libc::fprintf(output, b"%c\0".as_ptr() as *const c_char, c as c_int);
            }
            i += 1;
        }
    }
}

/// Dump a debug representation of an attribute.
///
/// UPSTREAM-PARITY: `xmlDebugDumpAttr()`
#[no_mangle]
pub unsafe extern "C" fn xmlDebugDumpAttr(
    output: *mut _IO_FILE,
    attr: *mut _xmlAttr,
    depth: c_int,
) {
    if output.is_null() || attr.is_null() {
        return;
    }
    unsafe {
        for _ in 0..depth {
            libc::fprintf(output, b"  \0".as_ptr() as *const c_char);
        }
        libc::fprintf(output, b"ATTRIBUTE \0".as_ptr() as *const c_char);
        xmlDebugDumpString(output, (*attr).name as *const u8);
        if !(*attr).ns.is_null() && !(*(*attr).ns).prefix.is_null() {
            libc::fprintf(
                output,
                b":%s\0".as_ptr() as *const c_char,
                (*(*attr).ns).prefix,
            );
        }
        libc::fprintf(output, b"\n\0".as_ptr() as *const c_char);
        if !(*attr).children.is_null() {
            for _ in 0..(depth + 1) {
                libc::fprintf(output, b"  \0".as_ptr() as *const c_char);
            }
            libc::fprintf(output, b"VALUE: \0".as_ptr() as *const c_char);
            let text = (*attr).children;
            if !text.is_null() && !(*text).content.is_null() {
                xmlDebugDumpString(output, (*text).content as *const u8);
            }
            libc::fprintf(output, b"\n\0".as_ptr() as *const c_char);
        }
    }
}

/// Dump a debug representation of an attribute list.
///
/// UPSTREAM-PARITY: `xmlDebugDumpAttrList()`
#[no_mangle]
pub unsafe extern "C" fn xmlDebugDumpAttrList(
    output: *mut _IO_FILE,
    attr: *mut _xmlAttr,
    depth: c_int,
) {
    if output.is_null() {
        return;
    }
    let mut cur = attr;
    while !cur.is_null() {
        unsafe {
            xmlDebugDumpAttr(output, cur, depth);
            cur = (*cur).next;
        }
    }
}

/// Dump a single node for debug output.
///
/// UPSTREAM-PARITY: `xmlDebugDumpOneNode()`
#[no_mangle]
pub unsafe extern "C" fn xmlDebugDumpOneNode(
    output: *mut _IO_FILE,
    node: *mut _xmlNode,
    depth: c_int,
) {
    if output.is_null() || node.is_null() {
        return;
    }
    unsafe {
        // Indent
        for _ in 0..depth {
            libc::fprintf(output, b"  \0".as_ptr() as *const c_char);
        }

        // Print node type
        match (*node).type_ {
            1 => {
                // XML_ELEMENT_NODE
                libc::fprintf(output, b"ELEMENT \0".as_ptr() as *const c_char);
                xmlDebugDumpString(output, (*node).name as *const u8);
                if !(*node).ns.is_null() {
                    if !(*(*node).ns).prefix.is_null() {
                        libc::fprintf(
                            output,
                            b" (ns=%s)\0".as_ptr() as *const c_char,
                            (*(*node).ns).prefix,
                        );
                    }
                }
            }
            2 => {
                // XML_ATTRIBUTE_NODE
                libc::fprintf(output, b"ATTRIBUTE \0".as_ptr() as *const c_char);
                xmlDebugDumpString(output, (*node).name as *const u8);
            }
            3 => {
                // XML_TEXT_NODE
                libc::fprintf(output, b"TEXT\0".as_ptr() as *const c_char);
                if !(*node).content.is_null() {
                    libc::fprintf(output, b"\n\0".as_ptr() as *const c_char);
                    for _ in 0..(depth + 1) {
                        libc::fprintf(output, b"  \0".as_ptr() as *const c_char);
                    }
                    libc::fprintf(output, b"CONTENT: \0".as_ptr() as *const c_char);
                    xmlDebugDumpString(output, (*node).content as *const u8);
                }
            }
            4 => {
                // XML_CDATA_SECTION_NODE
                libc::fprintf(output, b"CDATA\0".as_ptr() as *const c_char);
            }
            5 => {
                // XML_ENTITY_REF_NODE
                libc::fprintf(output, b"ENTITY_REF \0".as_ptr() as *const c_char);
                xmlDebugDumpString(output, (*node).name as *const u8);
            }
            6 => {
                // XML_ENTITY_NODE
                libc::fprintf(output, b"ENTITY \0".as_ptr() as *const c_char);
                xmlDebugDumpString(output, (*node).name as *const u8);
            }
            7 => {
                // XML_PI_NODE
                libc::fprintf(output, b"PI \0".as_ptr() as *const c_char);
                xmlDebugDumpString(output, (*node).name as *const u8);
            }
            8 => {
                // XML_COMMENT_NODE
                libc::fprintf(output, b"COMMENT\0".as_ptr() as *const c_char);
            }
            9 => {
                // XML_DOCUMENT_NODE
                libc::fprintf(output, b"DOCUMENT\0".as_ptr() as *const c_char);
            }
            10 => {
                // XML_DOCUMENT_TYPE_NODE
                libc::fprintf(output, b"DOCTYPE\0".as_ptr() as *const c_char);
            }
            11 => {
                // XML_DOCUMENT_FRAG_NODE
                libc::fprintf(output, b"DOCUMENT_FRAG\0".as_ptr() as *const c_char);
            }
            13 => {
                // XML_NAMESPACE_DECL
                libc::fprintf(output, b"NAMESPACE\0".as_ptr() as *const c_char);
                if !(*node).ns.is_null() && !(*(*node).ns).prefix.is_null() {
                    libc::fprintf(
                        output,
                        b" %s=%s\0".as_ptr() as *const c_char,
                        (*(*node).ns).prefix,
                        (*(*node).ns).href,
                    );
                }
            }
            14 => {
                // XML_XINCLUDE_START
                if is_xinclude_node(node) {
                    libc::fprintf(output, b"XINCLUDE\0".as_ptr() as *const c_char);
                } else {
                    libc::fprintf(output, b"XINCLUDE_START\0".as_ptr() as *const c_char);
                }
            }
            15 => {
                // XML_XINCLUDE_END
                libc::fprintf(output, b"XINCLUDE_END\0".as_ptr() as *const c_char);
            }
            _ => {
                libc::fprintf(
                    output,
                    b"UNKNOWN (%d)\0".as_ptr() as *const c_char,
                    (*node).type_ as c_int,
                );
            }
        }
        libc::fprintf(output, b"\n\0".as_ptr() as *const c_char);

        // Print attributes
        if !(*node).properties.is_null() {
            xmlDebugDumpAttrList(output, (*node).properties, depth + 1);
        }
    }
}

/// Dump a node and its subtree.
///
/// UPSTREAM-PARITY: `xmlDebugDumpNode()`
#[no_mangle]
pub unsafe extern "C" fn xmlDebugDumpNode(
    output: *mut _IO_FILE,
    node: *mut _xmlNode,
    depth: c_int,
) {
    if output.is_null() || node.is_null() || depth > MAX_DEPTH {
        return;
    }
    unsafe {
        xmlDebugDumpOneNode(output, node, depth);

        // Dump children
        if !(*node).children.is_null() {
            let mut child = (*node).children;
            while !child.is_null() {
                xmlDebugDumpNode(output, child, depth + 1);
                child = (*child).next;
            }
        }
    }
}

/// Dump a node list.
///
/// UPSTREAM-PARITY: `xmlDebugDumpNodeList()`
#[no_mangle]
pub unsafe extern "C" fn xmlDebugDumpNodeList(
    output: *mut _IO_FILE,
    node: *mut _xmlNode,
    depth: c_int,
) {
    if output.is_null() {
        return;
    }
    let mut cur = node;
    while !cur.is_null() {
        unsafe {
            xmlDebugDumpNode(output, cur, depth);
            cur = (*cur).next;
        }
    }
}

/// Dump an entire document.
///
/// UPSTREAM-PARITY: `xmlDebugDumpDocument()`
#[no_mangle]
pub unsafe extern "C" fn xmlDebugDumpDocument(output: *mut _IO_FILE, doc: *mut _xmlDoc) {
    if output.is_null() || doc.is_null() {
        return;
    }
    unsafe {
        libc::fprintf(output, b"DOCUMENT\0".as_ptr() as *const c_char);
        if !(*doc).name.is_null() {
            libc::fprintf(output, b" %s\0".as_ptr() as *const c_char, (*doc).name);
        }
        if !(*doc).URL.is_null() {
            libc::fprintf(output, b" URL=%s\0".as_ptr() as *const c_char, (*doc).URL);
        }
        libc::fprintf(output, b"\n\0".as_ptr() as *const c_char);

        // Dump children of doc
        if !(*doc).children.is_null() {
            let mut child = (*doc).children;
            while !child.is_null() {
                xmlDebugDumpNode(output, child, 1);
                child = (*child).next;
            }
        }
    }
}

/// Dump the document head (first few nodes).
///
/// UPSTREAM-PARITY: `xmlDebugDumpDocumentHead()`
#[no_mangle]
pub unsafe extern "C" fn xmlDebugDumpDocumentHead(output: *mut _IO_FILE, doc: *mut _xmlDoc) {
    if output.is_null() || doc.is_null() {
        return;
    }
    unsafe {
        xmlDebugDumpDocument(output, doc);
    }
}

/// Count the number of nodes in a list reachable via next pointers.
///
/// UPSTREAM-PARITY: `xmlLsCountNode()`
#[no_mangle]
pub unsafe extern "C" fn xmlLsCountNode(node: *mut _xmlNode) -> c_int {
    if node.is_null() {
        return 0;
    }
    let mut count: c_int = 0;
    let mut cur = node;
    while !cur.is_null() {
        count += 1;
        unsafe {
            cur = (*cur).next;
        }
    }
    count
}

/// Dump a single node summary (like `ls -l` for nodes).
///
/// UPSTREAM-PARITY: `xmlLsOneNode()`
#[no_mangle]
pub unsafe extern "C" fn xmlLsOneNode(output: *mut _IO_FILE, node: *mut _xmlNode) {
    if output.is_null() || node.is_null() {
        return;
    }
    unsafe {
        match (*node).type_ {
            1 => {
                // XML_ELEMENT_NODE
                libc::fprintf(output, b"E \0".as_ptr() as *const c_char);
                if !(*node).ns.is_null() && !(*(*node).ns).prefix.is_null() {
                    libc::fprintf(
                        output,
                        b"%s:\0".as_ptr() as *const c_char,
                        (*(*node).ns).prefix,
                    );
                }
                xmlDebugDumpString(output, (*node).name as *const u8);
            }
            2 => {
                libc::fprintf(output, b"A \0".as_ptr() as *const c_char);
                xmlDebugDumpString(output, (*node).name as *const u8);
            }
            3 => {
                libc::fprintf(output, b"T \0".as_ptr() as *const c_char);
                if !(*node).content.is_null() {
                    xmlDebugDumpString(output, (*node).content as *const u8);
                }
            }
            4 => {
                libc::fprintf(output, b"C \0".as_ptr() as *const c_char);
            }
            5 => {
                libc::fprintf(output, b"E \0".as_ptr() as *const c_char);
                xmlDebugDumpString(output, (*node).name as *const u8);
            }
            6 => {
                libc::fprintf(output, b"E \0".as_ptr() as *const c_char);
                xmlDebugDumpString(output, (*node).name as *const u8);
            }
            7 => {
                libc::fprintf(output, b"PI \0".as_ptr() as *const c_char);
                xmlDebugDumpString(output, (*node).name as *const u8);
            }
            8 => {
                libc::fprintf(output, b"C \0".as_ptr() as *const c_char);
            }
            9 => {
                libc::fprintf(output, b"D \0".as_ptr() as *const c_char);
            }
            10 => {
                libc::fprintf(output, b"DTD \0".as_ptr() as *const c_char);
            }
            14 => {
                libc::fprintf(output, b"X \0".as_ptr() as *const c_char);
            }
            _ => {
                libc::fprintf(
                    output,
                    b"? (%d)\0".as_ptr() as *const c_char,
                    (*node).type_ as c_int,
                );
            }
        }
        libc::fprintf(output, b"\n\0".as_ptr() as *const c_char);
    }
}

/// Re-export _IO_FILE type for C ABI compatibility.
///
/// This is typically `FILE` in C. On Linux with libc, `_IO_FILE` is the struct
/// behind `FILE *`. We use `*mut _IO_FILE` to match the upstream signature.
pub type _IO_FILE = libc::FILE;

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::allocator::xmlMalloc;
    use crate::abi::structs::*;
    use crate::abi::types::xmlChar;
    use crate::xml::tree::*;

    /// Helper: create a simple document for testing.
    unsafe fn create_test_doc() -> *mut _xmlDoc {
        let doc = new_doc(b"1.0\0".as_ptr() as *const xmlChar);
        let root = new_node(ptr::null_mut(), b"root\0".as_ptr() as *const xmlChar);
        doc_set_root_element(doc, root);
        let child = new_child(root, ptr::null_mut(), b"child\0".as_ptr() as *const xmlChar);
        // Set a property using set_prop
        set_prop(
            child,
            b"attr1\0".as_ptr() as *const xmlChar,
            b"value1\0".as_ptr() as *const xmlChar,
        );
        doc
    }

    #[test]
    fn test_xml_bool_to_text() {
        unsafe {
            let t = xmlBoolToText(1);
            assert!(!t.is_null());
            let f = xmlBoolToText(0);
            assert!(!f.is_null());
            // Check that the strings are correct by comparing first byte
            assert_eq!(*t as u8, b't');
            assert_eq!(*f as u8, b'f');
        }
    }

    #[test]
    fn test_debug_dump_string_null() {
        unsafe {
            // Should not crash
            xmlDebugDumpString(ptr::null_mut(), ptr::null());
            // Should print "(null)"
            let f = libc::fmemopen(ptr::null_mut(), 0, b"w\0".as_ptr() as *const c_char);
            if !f.is_null() {
                xmlDebugDumpString(f, ptr::null());
                libc::fclose(f);
            }
        }
    }

    #[test]
    fn test_debug_dump_document_null() {
        unsafe {
            xmlDebugDumpDocument(ptr::null_mut(), ptr::null_mut());
            let f = libc::fmemopen(ptr::null_mut(), 0, b"w\0".as_ptr() as *const c_char);
            if !f.is_null() {
                xmlDebugDumpDocument(f, ptr::null_mut());
                libc::fclose(f);
            }
        }
    }

    #[test]
    fn test_debug_dump_node_null() {
        unsafe {
            xmlDebugDumpNode(ptr::null_mut(), ptr::null_mut(), 0);
            let f = libc::fmemopen(ptr::null_mut(), 0, b"w\0".as_ptr() as *const c_char);
            if !f.is_null() {
                xmlDebugDumpNode(f, ptr::null_mut(), 0);
                libc::fclose(f);
            }
        }
    }

    #[test]
    fn test_ls_count_node() {
        unsafe {
            let node = new_node(ptr::null_mut(), b"test\0".as_ptr() as *const xmlChar);
            assert!(!node.is_null());
            let count = xmlLsCountNode(node);
            assert_eq!(count, 1);

            // Add a sibling
            let sibling = new_node(ptr::null_mut(), b"sibling\0".as_ptr() as *const xmlChar);
            add_sibling(node, sibling);
            let count = xmlLsCountNode(node);
            assert_eq!(count, 2);

            free_node(node);
        }
    }

    #[test]
    fn test_debug_dump_attr_null() {
        unsafe {
            xmlDebugDumpAttr(ptr::null_mut(), ptr::null_mut(), 0);
            let f = libc::fmemopen(ptr::null_mut(), 0, b"w\0".as_ptr() as *const c_char);
            if !f.is_null() {
                xmlDebugDumpAttr(f, ptr::null_mut(), 0);
                libc::fclose(f);
            }
        }
    }

    #[test]
    fn test_debug_dump_attr_list_null() {
        unsafe {
            xmlDebugDumpAttrList(ptr::null_mut(), ptr::null_mut(), 0);
        }
    }

    #[test]
    fn test_debug_dump_node_list_null() {
        unsafe {
            xmlDebugDumpNodeList(ptr::null_mut(), ptr::null_mut(), 0);
        }
    }

    #[test]
    fn test_ls_one_node_null() {
        unsafe {
            xmlLsOneNode(ptr::null_mut(), ptr::null_mut());
            let f = libc::fmemopen(ptr::null_mut(), 0, b"w\0".as_ptr() as *const c_char);
            if !f.is_null() {
                xmlLsOneNode(f, ptr::null_mut());
                libc::fclose(f);
            }
        }
    }

    #[test]
    fn test_dump_document_head_null() {
        unsafe {
            xmlDebugDumpDocumentHead(ptr::null_mut(), ptr::null_mut());
        }
    }

    #[test]
    fn test_ls_count_node_null() {
        assert_eq!(unsafe { xmlLsCountNode(ptr::null_mut()) }, 0);
    }
}
