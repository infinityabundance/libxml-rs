//! exports_tree — tree construction/editing/content/property/namespace/free
//! family closure (11.1-I).
//!
//! C ABI exports for the libxml2 tree, entities, valid and writer families:
//! `xmlAddChildList`, `xmlAddNextSibling`, `xmlAddPrevSibling`,
//! `xmlReplaceNode`, node/property/namespace creation, content get/set/add,
//! `xml:lang`/`xml:space`/`xml:base` handling, `xmlSetTreeDoc`,
//! `xmlGetNsListSafe`, `xmlReconciliateNs`, `xmlFreeNs[List]`,
//! `xmlFreeProp[List]`, `xmlSetCompressMode`/`xmlGetCompressMode`, the
//! `xmlDOMWrap*` functions, the entities/valid tables
//! (`xmlCreateEntitiesTable`, `xmlFreeEntitiesTable`,
//! `xmlCreateEnumeration`, `xmlFreeEnumeration`,
//! `xmlFreeDocElementContent`, `xmlFreeAttributeTable`,
//! `xmlFreeElementTable`, `xmlFreeNotationTable`),
//! `xmlNewEntityInputStream`
//! and `xmlNewTextWriterPushParser`.
//!
//! # UPSTREAM-PARITY
//!
//! All semantics follow upstream libxml2 (`archaeology/libxml2-git/tree.c`,
//! `valid.c`, `entities.c`, `parserInternals.c`, `xmlwriter.c`).
//!
//! The internal helpers from `src/xml/tree/mod.rs` (`new_node`, `new_text`,
//! `copy_node`, `add_child`, `get_prop`, `set_prop`, `unset_prop`,
//! `node_get_content`, `get_ns_list`, `search_ns`, `search_ns_by_href`,
//! `unlink_node`, `free_node`, `free_node_list`, `new_ns`, `get_doc_entity`,
//! ...) are reused wherever they match upstream semantics; functions whose
//! upstream behavior is not covered by those helpers are ported locally.
//!
//! # Upstream contract
//!
//! Parity target is upstream `tree.c`, `buf.c`, `entities.c`, `valid.c` and
//! `parserInternals.c` (libxml2 2.15.3) with the `tree.h`/`entities.h`/
//! `valid.h` signatures. Residuals R-000164 (parser/tree structural parity)
//! and R-000165 (tree-family export gaps) both land here.
//!
//! # Conceptual behavior
//!
//! This module implements the tree-construction/editing ABI: child/sibling
//! linking, node/property/namespace creation, content get/set/add,
//! xml:lang/xml:space/xml:base handling, namespace reconciliation, free
//! functions, the `xmlDOMWrap*` family and the entities/valid tables. Internal
//! helpers from `src/xml/tree/mod.rs` are reused wherever their semantics
//! match upstream; the rest are ported locally.
//!
//! # Ownership & safety invariants
//!
//! Tree ownership per OWNERSHIP_ATLAS section 2: `xmlAddChild`/
//! `xmlAddNextSibling` transfer subtree ownership to the new parent;
//! `xmlUnlinkNode` detaches; `xmlFreeNodeList` frees a whole list; `xmlFreeNs`
//! frees only standalone `xmlNewNs` results (ns attached to nodes are freed
//! with the node); `xmlSetTreeDoc` re-parents docs across a subtree; borrowed
//! pointers (parent/doc/ns) are never freed by the reader.
//!
//! # Historical quirks & epochs
//!
//! LORE-0006/QUIRK-0002: namespace nodes have no parent pointer — a
//! long-standing upstream divergence (commit `044fc6b7`, 2002-03-04, fixing
//! #61290) that downstream XPath namespace-axis code depends on. R-000164
//! (11.1-N, TREE-001 byte-identical) aligned the parse-time DOM with upstream
//! including the namespace/decl handling and free paths.
//!
//! # Deliberate oddities
//!
//! `xmlSetCompressMode`/`xmlGetCompressMode` keep upstreams gzip-compression
//! global; the namespace-node parent gap (QUIRK-0002) is deliberately
//! preserved rather than fixed because it is observable surface; `xmlNewNodeEatName`
//! consumes its name argument per upstreams ownership transfer.
//!
//! # Proving courts
//!
//! The OWNERSHIP and TREE-STRUCTURE court families (TREE-001 probe
//! byte-identical, 0 mismatch lines) plus DSO-LOADER and HEADER-COMPILE cover
//! this module; the tree unit suite runs under cargo test.
//!
//! # Tempting simplifications that would break parity
//!
//! A tempting simplification is to give namespace nodes a parent pointer (it
//! would make Rust code simpler) — QUIRK-0002 records that upstream
//! deliberately has none and XPath namespace-axis semantics depend on it, so
//! the XPATH-NS courts would fail. Another shortcut, freeing ns objects from
//! `xmlFreeNode` unconditionally, would double-free standalone `xmlNewNs`
//! results; the ownership split must stay.

#![allow(
    missing_docs,
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals
)]
#![allow(clippy::missing_safety_doc)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]
#![allow(clippy::comparison_chain)]
#![allow(clippy::needless_range_loop)]
#![allow(clippy::manual_swap)]
// Ported goto-heavy C (tree.c) writes state variables before re-reading
// them; the dead first assignments mirror upstream exactly.
#![allow(unused_assignments)]
#![allow(missing_debug_implementations)]

// SAFETY-SCOPE: EXPORT-TREE-MECHANICAL-001
// (11.1-Z.3 proof scope, classified-generated) — this module is the
// mechanical extern-"C" export surface: every `unsafe` block in it is
// the documented indirection/registry-access pattern whose validity
// rests on the upstream C contract, and the exported signatures are
// machine-measured by the ABI-FUNCTION-SIGNATURE and DSO-LOADER
// courts and the C-API differential probes. The safety contract of
// each export is stated in its own doc comment; this scope covers the
// mechanical wrappers' unsafe blocks.

use core::ffi::c_void;
use core::ptr;
use std::os::raw::{c_char, c_int};
use std::sync::atomic::{AtomicI32, Ordering};

use crate::abi::allocator::{xmlFreeImpl, xmlMallocImpl, xmlMallocZero, xmlReallocImpl};
use crate::abi::constants::*;
use crate::abi::structs::*;
use crate::abi::types::xmlElementType::*;
use crate::abi::types::*;
use crate::xml::entities;
use crate::xml::hash;
use crate::xml::tree;

// ═══════════════════════════════════════════════════════════════════════════════
// String helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Duplicate a null-terminated xmlChar string (NULL-safe).
unsafe fn dup_str(s: *const xmlChar) -> *mut xmlChar {
    unsafe { crate::abi::exports_xml2::xmlStrdup(s) }
}

/// Length of a null-terminated xmlChar string (NULL → 0).
unsafe fn str_len(s: *const xmlChar) -> c_int {
    unsafe { crate::abi::exports_xml2::xmlStrlen(s) }
}

/// Copy the (NUL-terminated) prefix of `name` up to `max` bytes into an
/// owned, NUL-terminated buffer (upstream `snprintf(..., "%.*s", max, name)`).
unsafe fn str_prefix(name: *const xmlChar, max: usize) -> Vec<u8> {
    if name.is_null() {
        return vec![0];
    }
    let mut v = Vec::new();
    let mut i = 0usize;
    while i < max {
        let c = unsafe { *name.add(i) };
        if c == 0 {
            break;
        }
        v.push(c);
        i += 1;
    }
    v.push(0);
    v
}

/// Pointer-or-value string equality (upstream `(a == b) || xmlStrEqual(a, b)`).
unsafe fn str_eq_or_ptr(a: *const xmlChar, b: *const xmlChar) -> bool {
    if a == b {
        return true;
    }
    unsafe { crate::abi::exports_xml2::xmlStrEqual(a, b) != 0 }
}

/// Case-insensitive equality against a byte literal (e.g. `"html"`).
const unsafe fn strcasecmp_eq(a: *const xmlChar, lit: &[u8]) -> bool {
    if a.is_null() {
        return false;
    }
    let mut i = 0usize;
    loop {
        let ca = unsafe { *a.add(i) };
        if i >= lit.len() {
            return ca == 0;
        }
        let cb = lit[i];
        if ca == 0 || !ca.eq_ignore_ascii_case(&cb) {
            return false;
        }
        i += 1;
    }
}

/// `IS_STR_XML` from upstream tree.c — prefix is the `xml` prefix.
unsafe fn is_str_xml(s: *const xmlChar) -> bool {
    unsafe { crate::abi::exports_xml2::xmlStrEqual(s, c"xml".as_ptr() as *const xmlChar) != 0 }
}

/// `xmlStringText` — the canonical name of text nodes.
const TEXT_NAME: &[u8] = b"text\0";

/// UTF-8 encode `val` into `out` (upstream `xmlCopyCharMultiByte`).
unsafe fn copy_char_multibyte(mut out: *mut xmlChar, val: c_int) -> c_int {
    if out.is_null() || val < 0 {
        return 0;
    }
    if val >= 0x80 {
        let saved = out;
        let bits: c_int;
        if val < 0x800 {
            *out = ((val >> 6) | 0xC0) as u8;
            out = out.add(1);
            bits = 0;
        } else if val < 0x10000 {
            *out = ((val >> 12) | 0xE0) as u8;
            out = out.add(1);
            bits = 6;
        } else if val < 0x110000 {
            *out = ((val >> 18) | 0xF0) as u8;
            out = out.add(1);
            bits = 12;
        } else {
            return 0;
        }
        let mut b = bits;
        loop {
            *out = (((val >> b) & 0x3F) | 0x80) as u8;
            out = out.add(1);
            if b == 0 {
                break;
            }
            b -= 6;
        }
        return unsafe { out.offset_from(saved) } as c_int;
    }
    *out = val as u8;
    1
}

/// Copy a byte vector into a NUL-terminated xmlMalloc'd string.
unsafe fn vec_to_c_string(v: &[u8]) -> *mut xmlChar {
    let buf = unsafe { xmlMallocImpl(v.len() + 1) } as *mut xmlChar;
    if buf.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        ptr::copy_nonoverlapping(v.as_ptr(), buf, v.len());
        *buf.add(v.len()) = 0;
    }
    buf
}

/// Upstream `xmlTextSetContent`.
unsafe fn text_set_content(text: *mut _xmlNode, content: *mut xmlChar) {
    if !(*text).content.is_null() {
        let inline_addr = core::ptr::addr_of_mut!((*text).properties) as *const c_void;
        if (*text).content as *const c_void != inline_addr {
            unsafe { xmlFreeImpl((*text).content as *mut c_void) };
        }
    }
    (*text).content = content;
    (*text).properties = ptr::null_mut();
}

/// Upstream `xmlTextAddContent` — append `len` bytes (or the full string if
/// `len < 0`) to the content of a text node.
unsafe fn text_add_content(text: *mut _xmlNode, content: *const xmlChar, len: c_int) -> c_int {
    if content.is_null() {
        return 0;
    }
    let l = if len < 0 {
        unsafe { str_len(content) }
    } else {
        len
    };
    let merged = unsafe { crate::abi::exports_xml2::xmlStrncatNew((*text).content, content, l) };
    if merged.is_null() {
        return -1;
    }
    unsafe { text_set_content(text, merged) };
    0
}

/// Upstream `xmlNewDocText`.
unsafe fn new_doc_text(doc: *mut _xmlDoc, content: *const xmlChar) -> *mut _xmlNode {
    let n = unsafe { tree::new_text(content) };
    if !n.is_null() {
        (*n).doc = doc;
    }
    n
}

/// Upstream `xmlNewDocTextLen`.
unsafe fn new_doc_text_len(
    doc: *mut _xmlDoc,
    content: *const xmlChar,
    len: c_int,
) -> *mut _xmlNode {
    let node = unsafe { xmlMallocZero(size_of::<_xmlNode>()) } as *mut _xmlNode;
    if node.is_null() {
        return ptr::null_mut();
    }
    (*node).type_ = XML_TEXT_NODE as c_int;
    (*node).name = unsafe { dup_str(TEXT_NAME.as_ptr() as *const xmlChar) };
    (*node).doc = doc;
    if !content.is_null() {
        (*node).content = unsafe { crate::abi::exports_xml2::xmlStrndup(content, len) };
        if (*node).content.is_null() {
            unsafe { tree::free_node(node) };
            return ptr::null_mut();
        }
    }
    node
}

/// Upstream static `xmlNewEntityRef` — creates an ENTITY_REF node taking
/// ownership of `name`.
unsafe fn new_entity_ref(doc: *mut _xmlDoc, name: *mut xmlChar) -> *mut _xmlNode {
    let cur = unsafe { xmlMallocZero(size_of::<_xmlNode>()) } as *mut _xmlNode;
    if cur.is_null() {
        unsafe { xmlFreeImpl(name as *mut c_void) };
        return ptr::null_mut();
    }
    (*cur).type_ = XML_ENTITY_REF_NODE as c_int;
    (*cur).doc = doc;
    (*cur).name = name;
    cur
}

/// Look up the content of a predefined entity (lt/gt/amp/quot/apos), or NULL.
const unsafe fn predefined_entity_content(name: *const xmlChar) -> *const xmlChar {
    if name.is_null() {
        return ptr::null();
    }
    let c = unsafe { *name };
    let n1 = unsafe { *name.add(1) };
    let n2 = unsafe { *name.add(2) };
    match c {
        b'l' if n1 == b't' && n2 == 0 => c"<".as_ptr() as *const xmlChar,
        b'g' if n1 == b't' && n2 == 0 => c">".as_ptr() as *const xmlChar,
        b'a' if n1 == b'm' && n2 == b'p' && unsafe { *name.add(3) } == 0 => {
            c"&".as_ptr() as *const xmlChar
        }
        b'q' if n1 == b'u'
            && n2 == b'o'
            && unsafe { *name.add(3) } == b't'
            && unsafe { *name.add(4) } == 0 =>
        {
            c"\"".as_ptr() as *const xmlChar
        }
        b'a' if n1 == b'p'
            && n2 == b'o'
            && unsafe { *name.add(3) } == b's'
            && unsafe { *name.add(4) } == 0 =>
        {
            c"'".as_ptr() as *const xmlChar
        }
        _ => ptr::null(),
    }
}

/// Append a node to the tail of a list, updating `head`/`last` and the
/// sibling links (upstream inline list-append used by xmlNodeParseAttValue).
unsafe fn list_append(head: *mut *mut _xmlNode, last: *mut *mut _xmlNode, node: *mut _xmlNode) {
    if (*head).is_null() {
        *head = node;
    } else {
        (**last).next = node;
        (*node).prev = *last;
    }
    *last = node;
}

/// Upstream `xmlNodeParseAttValue` — parse `value` (up to `len` bytes, or the
/// whole string when `len == usize::MAX`) as an XML attribute value into a
/// list of text/entity-ref nodes, freeing and replacing `attr`'s children.
unsafe fn node_parse_att_value(
    doc: *mut _xmlDoc,
    attr: *mut _xmlNode,
    value: *const xmlChar,
    len: usize,
) -> c_int {
    let mut head: *mut _xmlNode = ptr::null_mut();
    let mut last: *mut _xmlNode = ptr::null_mut();
    let mut remaining = len;

    if !value.is_null() && *value != 0 {
        let mut cur = value;
        let mut q = cur;
        let mut buf: Vec<u8> = Vec::new();
        let mut failed = false;
        while remaining > 0 && *cur != 0 {
            if *cur == b'&' {
                let mut charval: c_int = 0;
                if cur != q {
                    buf.extend_from_slice(core::slice::from_raw_parts(
                        q,
                        cur.offset_from(q) as usize,
                    ));
                    q = cur;
                }
                if remaining > 2 && *cur.add(1) == b'#' && *cur.add(2) == b'x' {
                    cur = cur.add(3);
                    remaining -= 3;
                    while remaining > 0 {
                        let tmp = *cur;
                        if tmp == b';' {
                            break;
                        }
                        charval = match tmp {
                            b'0'..=b'9' => charval * 16 + (tmp - b'0') as c_int,
                            b'a'..=b'f' => charval * 16 + (tmp - b'a') as c_int + 10,
                            b'A'..=b'F' => charval * 16 + (tmp - b'A') as c_int + 10,
                            _ => {
                                charval = 0;
                                break;
                            }
                        };
                        if charval > 0x110000 {
                            charval = 0x110000;
                        }
                        cur = cur.add(1);
                        remaining -= 1;
                    }
                    if *cur == b';' {
                        cur = cur.add(1);
                        remaining -= 1;
                    }
                    q = cur;
                } else if remaining > 1 && *cur.add(1) == b'#' {
                    cur = cur.add(2);
                    remaining -= 2;
                    while remaining > 0 {
                        let tmp = *cur;
                        if tmp == b';' {
                            break;
                        }
                        charval = match tmp {
                            b'0'..=b'9' => charval * 10 + (tmp - b'0') as c_int,
                            _ => {
                                charval = 0;
                                break;
                            }
                        };
                        if charval > 0x110000 {
                            charval = 0x110000;
                        }
                        cur = cur.add(1);
                        remaining -= 1;
                    }
                    if *cur == b';' {
                        cur = cur.add(1);
                        remaining -= 1;
                    }
                    q = cur;
                } else {
                    cur = cur.add(1);
                    remaining -= 1;
                    q = cur;
                    while remaining > 0 && *cur != 0 && *cur != b';' {
                        cur = cur.add(1);
                        remaining -= 1;
                    }
                    // UPSTREAM-PARITY (tree.c xmlNodeParseAttValue): a bare
                    // '&' whose name never reaches a ';' does NOT fail the
                    // parse — the '&' is consumed and the REST of the value
                    // continues as plain text (the q..cur flush in the tail).
                    // Hard-failing here made xmlNewDocNode/xmlNewChild reject
                    // contents like "x & y" that the oracle accepts as
                    // "x  y".
                    if remaining == 0 || *cur == 0 {
                        break;
                    }
                    if cur != q {
                        let mut val = unsafe {
                            crate::abi::exports_xml2::xmlStrndup(q, (cur.offset_from(q)) as c_int)
                        };
                        if val.is_null() {
                            failed = true;
                            break;
                        }
                        let ent = unsafe { tree::get_doc_entity(doc, val) };
                        let is_predef = unsafe { predefined_entity_content(val) };
                        if !is_predef.is_null() {
                            // Predefined entities don't generate nodes.
                            let p = is_predef;
                            let mut i = 0usize;
                            while unsafe { *p.add(i) } != 0 {
                                buf.push(unsafe { *p.add(i) });
                                i += 1;
                            }
                        } else {
                            // Flush the buffer into a text node.
                            if !buf.is_empty() {
                                let node = unsafe { new_doc_text(doc, ptr::null()) };
                                if node.is_null() {
                                    unsafe { xmlFreeImpl(val as *mut c_void) };
                                    failed = true;
                                    break;
                                }
                                (*node).content = unsafe { vec_to_c_string(&buf) };
                                if (*node).content.is_null() {
                                    unsafe { tree::free_node(node) };
                                    unsafe { xmlFreeImpl(val as *mut c_void) };
                                    failed = true;
                                    break;
                                }
                                buf.clear();
                                (*node).parent = attr;
                                unsafe { list_append(&mut head, &mut last, node) };
                            }
                            // Create an entity-reference node.
                            let node = unsafe { new_entity_ref(doc, val) };
                            val = ptr::null_mut();
                            if node.is_null() {
                                failed = true;
                                break;
                            }
                            (*node).parent = attr;
                            (*node).last = ent as *mut _xmlNode;
                            if !ent.is_null() {
                                (*node).children = ent as *mut _xmlNode;
                                (*node).content = (*ent).content;
                            }
                            unsafe { list_append(&mut head, &mut last, node) };
                        }
                        if !val.is_null() {
                            unsafe { xmlFreeImpl(val as *mut c_void) };
                        }
                    }
                    cur = cur.add(1);
                    remaining -= 1;
                    q = cur;
                }
                if charval != 0 && !failed {
                    if charval >= 0x110000 {
                        charval = 0xFFFD; // replacement character
                    }
                    let mut buffer = [0u8; 8];
                    let l = unsafe { copy_char_multibyte(buffer.as_mut_ptr(), charval) };
                    if l > 0 {
                        buf.extend_from_slice(&buffer[..l as usize]);
                    }
                }
                if failed {
                    break;
                }
            } else {
                cur = cur.add(1);
                remaining -= 1;
            }
        }
        if !failed && cur != q {
            buf.extend_from_slice(core::slice::from_raw_parts(q, cur.offset_from(q) as usize));
        }
        if !failed {
            if !buf.is_empty() {
                let node = unsafe { new_doc_text(doc, ptr::null()) };
                if node.is_null() {
                    failed = true;
                } else {
                    (*node).content = unsafe { vec_to_c_string(&buf) };
                    if (*node).content.is_null() {
                        unsafe { tree::free_node(node) };
                        failed = true;
                    } else {
                        (*node).parent = attr;
                        unsafe { list_append(&mut head, &mut last, node) };
                    }
                }
            } else if head.is_null() {
                let node = unsafe { new_doc_text(doc, c"".as_ptr() as *const xmlChar) };
                if node.is_null() {
                    failed = true;
                } else {
                    (*node).parent = attr;
                    head = node;
                    last = node;
                }
            }
        }
        if failed {
            if !head.is_null() {
                unsafe { tree::free_node_list(head) };
            }
            return -1;
        }
    }

    if !attr.is_null() {
        if !(*attr).children.is_null() {
            unsafe { tree::free_node_list((*attr).children) };
        }
        (*attr).children = head;
        (*attr).last = last;
    }
    0
}

// ═══════════════════════════════════════════════════════════════════════════════
// Namespace helpers (xmlTreeEnsureXMLDecl, xmlSearchNsByHrefSafe subset, …)
// ═══════════════════════════════════════════════════════════════════════════════

/// Upstream `xmlTreeEnsureXMLDecl` — ensure `doc->oldNs` holds the XML ns.
unsafe fn ensure_xml_decl(doc: *mut _xmlDoc) -> *mut _xmlNs {
    if doc.is_null() {
        return ptr::null_mut();
    }
    if !(*doc).oldNs.is_null() {
        return (*doc).oldNs;
    }
    let ns = unsafe {
        tree::new_ns(
            ptr::null_mut(),
            XML_XML_NAMESPACE.as_ptr() as *const xmlChar,
            c"xml".as_ptr() as *const xmlChar,
        )
    };
    if ns.is_null() {
        return ptr::null_mut();
    }
    (*doc).oldNs = ns;
    ns
}

/// Upstream `xmlTreeNSListLookupByPrefix`.
unsafe fn ns_list_lookup_by_prefix(ns_list: *mut _xmlNs, prefix: *const xmlChar) -> *mut _xmlNs {
    if ns_list.is_null() {
        return ptr::null_mut();
    }
    let mut ns = ns_list;
    loop {
        if unsafe { str_eq_or_ptr(prefix, (*ns).prefix) } {
            return ns;
        }
        if (*ns).next.is_null() {
            break;
        }
        ns = (*ns).next;
    }
    ptr::null_mut()
}

// ═══════════════════════════════════════════════════════════════════════════════
// Attribute / namespace property helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Upstream `xmlFreeProp`.
unsafe fn free_prop_impl(cur: *mut _xmlAttr) {
    if cur.is_null() {
        return;
    }
    // UPSTREAM-PARITY (tree.c xmlFreeProp): freeing an ID attribute removes
    // its entry from the document's ID table (xmlRemoveID). A NULL doc->ids
    // (document teardown frees the table first) is a no-op.
    if !(*cur).doc.is_null() && !(*cur).id.is_null() {
        crate::xml::validation::remove_id((*cur).doc, cur);
    }
    if !(*cur).children.is_null() {
        unsafe { tree::free_node_list((*cur).children) };
    }
    // UPSTREAM-PARITY (tree.c xmlFreeProp + DICT_FREE): attribute names may
    // be dict-interned (parser dictNames / lxml `_fixHtmlDictNodeNames`), so
    // the name is freed only when the owning document has no dictionary or the
    // dictionary does not own the string. The unguarded free here double-freed
    // interned names: php_libxml_node_free calls xmlFreeProp for an attribute
    // node, and xmlDictFree at doc teardown then freed the same string (Phase
    // 14.3 Bug-2 — SimpleXML attribute set/unset double free).
    let dict = if (*cur).doc.is_null() {
        ptr::null_mut()
    } else {
        unsafe { (*(*cur).doc).dict as *mut c_void }
    };
    if !(*cur).name.is_null() && !crate::abi::exports_hash::dict_owns_str(dict, (*cur).name) {
        unsafe { xmlFreeImpl((*cur).name as *mut c_void) };
    }
    unsafe { xmlFreeImpl(cur as *mut c_void) };
}

/// Upstream `xmlFreeNs`.
unsafe fn free_ns_impl(cur: *mut _xmlNs) {
    if cur.is_null() {
        return;
    }
    if !(*cur).href.is_null() {
        unsafe { xmlFreeImpl((*cur).href as *mut c_void) };
    }
    if !(*cur).prefix.is_null() {
        unsafe { xmlFreeImpl((*cur).prefix as *mut c_void) };
    }
    unsafe { xmlFreeImpl(cur as *mut c_void) };
}

/// Upstream `xmlGetPropNodeInternal` (ns-name variant).
unsafe fn find_prop_node_internal(
    node: *mut _xmlNode,
    name: *const xmlChar,
    ns_href: *const xmlChar,
    _use_dtd: c_int,
) -> *mut _xmlAttr {
    if node.is_null() || (*node).type_ != XML_ELEMENT_NODE as c_int || name.is_null() {
        return ptr::null_mut();
    }
    if !(*node).properties.is_null() {
        let mut prop = (*node).properties;
        if ns_href.is_null() {
            loop {
                if (*prop).ns.is_null()
                    && !(*prop).name.is_null()
                    && unsafe { crate::abi::exports_xml2::xmlStrEqual((*prop).name, name) != 0 }
                {
                    return prop;
                }
                if (*prop).next.is_null() {
                    break;
                }
                prop = (*prop).next;
            }
        } else {
            loop {
                if !(*prop).ns.is_null()
                    && !(*prop).name.is_null()
                    && unsafe { crate::abi::exports_xml2::xmlStrEqual((*prop).name, name) != 0 }
                    && !(*(*prop).ns).href.is_null()
                    && unsafe {
                        crate::abi::exports_xml2::xmlStrEqual((*(*prop).ns).href, ns_href) != 0
                    }
                {
                    return prop;
                }
                if (*prop).next.is_null() {
                    break;
                }
                prop = (*prop).next;
            }
        }
    }
    ptr::null_mut()
}

/// Upstream `xmlGetPropNodeValueInternal` — dup of the attribute's text value.
unsafe fn get_prop_node_value_internal(prop: *mut _xmlAttr) -> *mut xmlChar {
    if (*prop).children.is_null() {
        return unsafe { dup_str(c"".as_ptr() as *const xmlChar) };
    }
    let text = (*prop).children;
    if (*text).type_ == XML_TEXT_NODE as c_int {
        if (*text).content.is_null() {
            return unsafe { dup_str(c"".as_ptr() as *const xmlChar) };
        }
        return unsafe { dup_str((*text).content) };
    }
    // Entity-ref children: fall back to the raw children content walk.
    unsafe { tree::node_get_content(prop as *mut _xmlNode) }
}

/// Upstream `xmlNodeGetAttrValue` — value of the attribute with `name` in
/// namespace `ns_uri` (NULL = no namespace).
unsafe fn get_attr_value(
    node: *mut _xmlNode,
    name: *const xmlChar,
    ns_uri: *const xmlChar,
) -> *mut xmlChar {
    let prop = unsafe { find_prop_node_internal(node, name, ns_uri, 0) };
    if prop.is_null() {
        return ptr::null_mut();
    }
    unsafe { get_prop_node_value_internal(prop) }
}

/// Upstream `xmlSetNsProp` — set (or create) a namespaced attribute.
unsafe fn set_ns_prop_impl(
    node: *mut _xmlNode,
    ns: *mut _xmlNs,
    name: *const xmlChar,
    value: *const xmlChar,
) -> *mut _xmlAttr {
    if node.is_null() || name.is_null() {
        return ptr::null_mut();
    }
    let ns_href = if ns.is_null() {
        ptr::null()
    } else {
        (*ns).href
    };
    let existing = unsafe { find_prop_node_internal(node, name, ns_href, 0) };
    if !existing.is_null() {
        if !(*existing).children.is_null() {
            unsafe { tree::free_node_list((*existing).children) };
            (*existing).children = ptr::null_mut();
            (*existing).last = ptr::null_mut();
        }
        if !value.is_null() {
            let text = unsafe { new_doc_text((*node).doc, value) };
            if !text.is_null() {
                (*existing).children = text;
                (*existing).last = text;
                (*text).parent = existing as *mut _xmlNode;
                (*text).doc = (*node).doc;
            }
        }
        return existing;
    }
    unsafe { new_prop_internal(node, ns, name, value, 0) }
}

/// Upstream `xmlNewPropInternal` (raw value path).
unsafe fn new_prop_internal(
    node: *mut _xmlNode,
    ns: *mut _xmlNs,
    name: *const xmlChar,
    value: *const xmlChar,
    eatname: c_int,
) -> *mut _xmlAttr {
    let doc: *mut _xmlDoc;
    if !node.is_null() && (*node).type_ != XML_ELEMENT_NODE as c_int {
        if eatname == 1 {
            unsafe { xmlFreeImpl(name as *mut c_void) };
        }
        return ptr::null_mut();
    }
    let cur = unsafe { xmlMallocZero(size_of::<_xmlAttr>()) } as *mut _xmlAttr;
    if cur.is_null() {
        if eatname == 1 {
            unsafe { xmlFreeImpl(name as *mut c_void) };
        }
        return ptr::null_mut();
    }
    (*cur).type_ = XML_ATTRIBUTE_NODE as c_int;
    (*cur).parent = node;
    if !node.is_null() {
        doc = (*node).doc;
        (*cur).doc = doc;
    } else {
        doc = ptr::null_mut();
    }
    (*cur).ns = ns;

    if eatname == 0 {
        (*cur).name = unsafe { dup_str(name) };
        if (*cur).name.is_null() {
            unsafe { free_prop_impl(cur) };
            return ptr::null_mut();
        }
    } else {
        (*cur).name = name;
    }

    if !value.is_null() {
        let text = unsafe { new_doc_text(doc, value) };
        if text.is_null() {
            unsafe { free_prop_impl(cur) };
            return ptr::null_mut();
        }
        (*cur).children = text;
        (*cur).last = ptr::null_mut();
        let mut tmp = text;
        while !tmp.is_null() {
            (*tmp).parent = cur as *mut _xmlNode;
            if (*tmp).next.is_null() {
                (*cur).last = tmp;
            }
            tmp = (*tmp).next;
        }
    }

    if !node.is_null() {
        if (*node).properties.is_null() {
            (*node).properties = cur;
        } else {
            let mut prev = (*node).properties;
            while !(*prev).next.is_null() {
                prev = (*prev).next;
            }
            (*prev).next = cur;
            (*cur).prev = prev;
        }
    }
    cur
}

/// Upstream `xmlRemoveProp`.
unsafe fn remove_prop_impl(cur: *mut _xmlAttr) -> c_int {
    if cur.is_null() {
        return -1;
    }
    if (*cur).parent.is_null() {
        return -1;
    }
    let mut tmp = (*(*cur).parent).properties;
    if tmp == cur {
        (*(*cur).parent).properties = (*cur).next;
        if !(*cur).next.is_null() {
            (*(*cur).next).prev = ptr::null_mut();
        }
        unsafe { free_prop_impl(cur) };
        return 0;
    }
    while !tmp.is_null() {
        if (*tmp).next == cur {
            (*tmp).next = (*cur).next;
            if !(*tmp).next.is_null() {
                (*(*tmp).next).prev = tmp;
            }
            unsafe { free_prop_impl(cur) };
            return 0;
        }
        tmp = (*tmp).next;
    }
    -1
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlNodeSetDoc / xmlSetListDoc / xmlSetTreeDoc
// ═══════════════════════════════════════════════════════════════════════════════

/// Upstream `xmlNodeSetDoc` — full libxml2 2.15 semantics.
///
/// When a node moves to a document with a different dictionary, names that
/// the OLD document dictionary owns are re-interned into the NEW document
/// dictionary (`xmlDictLookup`) or turned into free-standing heap copies
/// (`xmlStrdup`) when the destination has no dictionary. Text/CDATA content
/// owned by the old dictionary is duplicated to the heap. Without this a
/// node adopted across documents keeps pointers into the source doc's
/// dictionary and teardown double-frees once the source document dies —
/// PHP >= 2.13 relies on this (ext/dom `php_dom_adopt_node` only runs its
/// own fixup when `LIBXML_VERSION < 21300`; upstream libxml2 commits
/// 4bc3ebf3eaba352fbbce2ef70ad00a3c7752478a + bc7ab5a2e61e4b36accf6803c5b0e245c11154b1).
/// ID attributes are removed from the old document's ID table; entity
/// references are re-resolved against the destination document and DTD nodes
/// are detached from the old document's subsets.
///
/// Callers must only invoke this when `(*node).doc != doc` (the caller-side
/// guards in `set_tree_doc_impl` / `set_list_doc_impl` / `propagate_doc`
/// ensure that, mirroring upstream `xmlSetTreeDoc`/`xmlSetListDoc`).
pub(crate) unsafe fn node_set_doc_impl(node: *mut _xmlNode, doc: *mut _xmlDoc) -> c_int {
    let mut ret = 0;
    let old_doc = (*node).doc;
    let old_dict = if old_doc.is_null() {
        ptr::null_mut()
    } else {
        (*old_doc).dict
    };
    let new_dict = if doc.is_null() {
        ptr::null_mut()
    } else {
        (*doc).dict
    };

    // UPSTREAM-PARITY (tree.c xmlNodeSetDoc): move names/content out of the
    // old document dictionary before the old document can be freed.
    if !old_dict.is_null() && old_dict != new_dict {
        let t = (*node).type_;
        let is_name_type = t == XML_ELEMENT_NODE as c_int
            || t == XML_ATTRIBUTE_NODE as c_int
            || t == XML_PI_NODE as c_int
            || t == XML_ENTITY_REF_NODE as c_int;
        if is_name_type
            && !(*node).name.is_null()
            && crate::abi::exports_hash::dict_owns_str(old_dict, (*node).name)
        {
            let new_name = if !new_dict.is_null() {
                unsafe {
                    crate::xml::dictionary::dict_lookup(
                        new_dict as *mut crate::xml::dictionary::Dict,
                        (*node).name,
                        -1,
                    )
                }
            } else {
                unsafe { crate::xml::string::xml_strdup((*node).name) }
            };
            if new_name.is_null() {
                ret = -1;
            }
            (*node).name = new_name;
        }
        let is_content_type = t == XML_TEXT_NODE as c_int || t == XML_CDATA_SECTION_NODE as c_int;
        if is_content_type
            && !(*node).content.is_null()
            && crate::abi::exports_hash::dict_owns_str(old_dict, (*node).content)
        {
            (*node).content = unsafe { crate::xml::string::xml_strdup((*node).content) };
            if (*node).content.is_null() {
                ret = -1;
            }
        }
    }

    match (*node).type_ as u32 {
        t if t == XML_ATTRIBUTE_NODE as u32 => {
            // UPSTREAM-PARITY (tree.c xmlNodeSetDoc): remove the attribute's
            // ID entry from the OLD document's ID table (not re-added to the
            // new one — upstream TODO).
            let attr = node as *mut _xmlAttr;
            if !(*attr).id.is_null() {
                unsafe { crate::xml::validation::remove_id(old_doc, attr) };
            }
        }
        t if t == XML_ENTITY_REF_NODE as u32 => {
            (*node).children = ptr::null_mut();
            (*node).last = ptr::null_mut();
            (*node).content = ptr::null_mut();
            if !doc.is_null() && (!(*doc).intSubset.is_null() || !(*doc).extSubset.is_null()) {
                let ent = unsafe { tree::get_doc_entity(doc, (*node).name) };
                if !ent.is_null() {
                    (*node).children = ent as *mut _xmlNode;
                    (*node).last = ent as *mut _xmlNode;
                    (*node).content = (*ent).content;
                }
            }
        }
        t if t == XML_DTD_NODE as u32 && !old_doc.is_null() => {
            if (*old_doc).intSubset == node as *mut _xmlDtd {
                (*old_doc).intSubset = ptr::null_mut();
            }
            if (*old_doc).extSubset == node as *mut _xmlDtd {
                (*old_doc).extSubset = ptr::null_mut();
            }
        }
        _ => {}
    }
    (*node).doc = doc;
    ret
}

/// Upstream `xmlSetListDoc`.
unsafe fn set_list_doc_impl(list: *mut _xmlNode, doc: *mut _xmlDoc) -> c_int {
    let mut ret = 0;
    if list.is_null() || (*list).type_ == XML_NAMESPACE_DECL as c_int {
        return 0;
    }
    let mut cur = list;
    while !cur.is_null() {
        if (*cur).doc != doc && unsafe { set_tree_doc_impl(cur, doc) } < 0 {
            ret = -1;
        }
        cur = (*cur).next;
    }
    ret
}

/// Upstream `xmlSetTreeDoc`.
unsafe fn set_tree_doc_impl(tree: *mut _xmlNode, doc: *mut _xmlDoc) -> c_int {
    let mut ret = 0;
    if tree.is_null() || (*tree).type_ == XML_NAMESPACE_DECL as c_int {
        return 0;
    }
    if (*tree).doc == doc {
        return 0;
    }
    if (*tree).type_ == XML_ELEMENT_NODE as c_int {
        let mut prop = (*tree).properties;
        while !prop.is_null() {
            if !(*prop).children.is_null()
                && unsafe { set_list_doc_impl((*prop).children, doc) } < 0
            {
                ret = -1;
            }
            if unsafe { node_set_doc_impl(prop as *mut _xmlNode, doc) } < 0 {
                ret = -1;
            }
            prop = (*prop).next;
        }
    }
    if !(*tree).children.is_null()
        && (*tree).type_ != XML_ENTITY_REF_NODE as c_int
        && unsafe { set_list_doc_impl((*tree).children, doc) } < 0
    {
        ret = -1;
    }
    if unsafe { node_set_doc_impl(tree, doc) } < 0 {
        ret = -1;
    }
    ret
}

/// Upstream `xmlInsertProp` — insert an attribute at (prev, next), destroying
/// a same-named duplicate.
unsafe fn insert_prop(
    doc: *mut _xmlDoc,
    cur: *mut _xmlNode,
    parent: *mut _xmlNode,
    prev: *mut _xmlNode,
    next: *mut _xmlNode,
) -> *mut _xmlNode {
    if (!prev.is_null() && (*prev).type_ != XML_ATTRIBUTE_NODE as c_int)
        || (!next.is_null() && (*next).type_ != XML_ATTRIBUTE_NODE as c_int)
    {
        return ptr::null_mut();
    }
    let ns_href = if (*cur).ns.is_null() {
        ptr::null()
    } else {
        (*(*cur).ns).href
    };
    let attr = unsafe { find_prop_node_internal(parent, (*cur).name, ns_href, 0) };

    unsafe { tree::unlink_node(cur) };

    if (*cur).doc != doc && unsafe { set_tree_doc_impl(cur, doc) } < 0 {
        return ptr::null_mut();
    }
    (*cur).parent = parent;
    (*cur).prev = prev;
    (*cur).next = next;

    if prev.is_null() {
        if !parent.is_null() {
            (*parent).properties = cur as *mut _xmlAttr;
        }
    } else {
        (*prev).next = cur;
    }
    if !next.is_null() {
        (*next).prev = cur;
    }

    if !attr.is_null() && attr != cur as *mut _xmlAttr {
        // Different instance: destroy it (attributes must be unique).
        unsafe { remove_prop_impl(attr) };
    }
    cur
}

/// Upstream `xmlInsertNode` — unlink `cur` and insert it between `prev` and
/// `next` (coalescing adjacent text nodes when `coalesce != 0`).
unsafe fn insert_node(
    doc: *mut _xmlDoc,
    cur: *mut _xmlNode,
    parent: *mut _xmlNode,
    prev: *mut _xmlNode,
    next: *mut _xmlNode,
    coalesce: c_int,
) -> *mut _xmlNode {
    if (*cur).type_ == XML_ATTRIBUTE_NODE as c_int {
        return unsafe { insert_prop(doc, cur, parent, prev, next) };
    }

    if coalesce != 0 && (*cur).type_ == XML_TEXT_NODE as c_int {
        if !prev.is_null()
            && (*prev).type_ == XML_TEXT_NODE as c_int
            && unsafe { str_eq_or_ptr((*prev).name, (*cur).name) }
        {
            if unsafe { text_add_content(prev, (*cur).content, -1) } < 0 {
                return ptr::null_mut();
            }
            unsafe { tree::unlink_node(cur) };
            unsafe { tree::free_node(cur) };
            return prev;
        }
        if !next.is_null()
            && (*next).type_ == XML_TEXT_NODE as c_int
            && unsafe { str_eq_or_ptr((*next).name, (*cur).name) }
        {
            if !(*cur).content.is_null() {
                let l = unsafe { str_len((*next).content) };
                let merged = unsafe {
                    crate::abi::exports_xml2::xmlStrncatNew((*cur).content, (*next).content, l)
                };
                if merged.is_null() {
                    return ptr::null_mut();
                }
                unsafe { text_set_content(next, merged) };
            }
            unsafe { tree::unlink_node(cur) };
            unsafe { tree::free_node(cur) };
            return next;
        }
    }

    let old_parent = (*cur).parent;
    if !old_parent.is_null() {
        if (*old_parent).children == cur {
            (*old_parent).children = (*cur).next;
        }
        if (*old_parent).last == cur {
            (*old_parent).last = (*cur).prev;
        }
    }
    if !(*cur).next.is_null() {
        (*(*cur).next).prev = (*cur).prev;
    }
    if !(*cur).prev.is_null() {
        (*(*cur).prev).next = (*cur).next;
    }

    if (*cur).doc != doc && unsafe { set_tree_doc_impl(cur, doc) } < 0 {
        (*cur).parent = ptr::null_mut();
        (*cur).prev = ptr::null_mut();
        (*cur).next = ptr::null_mut();
        return ptr::null_mut();
    }

    (*cur).parent = parent;
    (*cur).prev = prev;
    (*cur).next = next;

    if prev.is_null() {
        if !parent.is_null() {
            (*parent).children = cur;
        }
    } else {
        (*prev).next = cur;
    }
    if next.is_null() {
        if !parent.is_null() {
            (*parent).last = cur;
        }
    } else {
        (*next).prev = cur;
    }
    cur
}

/// Upstream `xmlAddChild` (with text merging), used by xmlNodeAddContentLen.
unsafe fn add_child_coalesce(parent: *mut _xmlNode, cur: *mut _xmlNode) -> *mut _xmlNode {
    if parent.is_null()
        || (*parent).type_ == XML_NAMESPACE_DECL as c_int
        || cur.is_null()
        || (*cur).type_ == XML_NAMESPACE_DECL as c_int
        || parent == cur
    {
        return ptr::null_mut();
    }
    // Undocumented quirk: adding to a text node appends content.
    if (*parent).type_ == XML_TEXT_NODE as c_int {
        if unsafe { text_add_content(parent, (*cur).content, -1) } < 0 {
            return ptr::null_mut();
        }
        unsafe { tree::unlink_node(cur) };
        unsafe { tree::free_node(cur) };
        return parent;
    }
    let prev: *mut _xmlNode = if (*cur).type_ == XML_ATTRIBUTE_NODE as c_int {
        let mut p = (*parent).properties;
        if !p.is_null() {
            while !(*p).next.is_null() {
                p = (*p).next;
            }
        }
        p as *mut _xmlNode
    } else {
        (*parent).last
    };
    if cur == prev {
        return cur;
    }
    unsafe { insert_node((*parent).doc, cur, parent, prev, ptr::null_mut(), 1) }
}

/// Upstream `xmlNewElem` (static helper of xmlNewDocNode).
unsafe fn new_elem(
    doc: *mut _xmlDoc,
    ns: *mut _xmlNs,
    name: *const xmlChar,
    content: *const xmlChar,
) -> *mut _xmlNode {
    let cur = unsafe { xmlMallocZero(size_of::<_xmlNode>()) } as *mut _xmlNode;
    if cur.is_null() {
        return ptr::null_mut();
    }
    (*cur).type_ = XML_ELEMENT_NODE as c_int;
    (*cur).doc = doc;
    (*cur).name = name;
    (*cur).ns = ns;
    if !content.is_null() && unsafe { node_parse_att_value(doc, cur, content, usize::MAX) } < 0 {
        // Don't free name on error.
        unsafe { xmlFreeImpl(cur as *mut c_void) };
        return ptr::null_mut();
    }
    cur
}

/// Upstream `xmlNewDocNode`.
unsafe fn new_doc_node(
    doc: *mut _xmlDoc,
    ns: *mut _xmlNs,
    name: *const xmlChar,
    content: *const xmlChar,
) -> *mut _xmlNode {
    if name.is_null() {
        return ptr::null_mut();
    }
    let copy = unsafe { dup_str(name) };
    if copy.is_null() {
        return ptr::null_mut();
    }
    let cur = unsafe { new_elem(doc, ns, copy, content) };
    if cur.is_null() {
        unsafe { xmlFreeImpl(copy as *mut c_void) };
        return ptr::null_mut();
    }
    cur
}

/// Upstream `xmlNewDocNodeEatName`.
unsafe fn new_doc_node_eat_name(
    doc: *mut _xmlDoc,
    ns: *mut _xmlNs,
    name: *mut xmlChar,
    content: *const xmlChar,
) -> *mut _xmlNode {
    if name.is_null() {
        return ptr::null_mut();
    }
    let cur = unsafe { new_elem(doc, ns, name, content) };
    if cur.is_null() {
        unsafe { xmlFreeImpl(name as *mut c_void) };
        return ptr::null_mut();
    }
    cur
}

/// Upstream `xmlNewDocRawNode`.
unsafe fn new_doc_raw_node(
    doc: *mut _xmlDoc,
    ns: *mut _xmlNs,
    name: *const xmlChar,
    content: *const xmlChar,
) -> *mut _xmlNode {
    let cur = unsafe { new_doc_node(doc, ns, name, ptr::null()) };
    if cur.is_null() {
        return ptr::null_mut();
    }
    (*cur).doc = doc;
    if !content.is_null() {
        let text = unsafe { new_doc_text(doc, content) };
        if text.is_null() {
            unsafe { tree::free_node(cur) };
            return ptr::null_mut();
        }
        (*cur).children = text;
        (*cur).last = text;
        (*text).parent = cur;
    }
    cur
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlDOMWrap internal infrastructure (ns-map, upstream tree.c §DOMWrap)
// ═══════════════════════════════════════════════════════════════════════════════

/// `xmlDOMWrapAcquireNsFunction` from tree.h.
pub type xmlDOMWrapAcquireNsFunction = unsafe extern "C" fn(
    ctxt: *mut _xmlDOMWrapCtxt,
    node: *mut _xmlNode,
    nsName: *const xmlChar,
    nsPrefix: *const xmlChar,
) -> *mut _xmlNs;

/// Upstream `struct _xmlDOMWrapCtxt`.
#[repr(C)]
pub struct _xmlDOMWrapCtxt {
    pub _private: *mut c_void,
    pub type_: c_int,
    pub namespaceMap: *mut c_void,
    pub getNsForNodeFunc: Option<xmlDOMWrapAcquireNsFunction>,
}

const XML_TREE_NSMAP_PARENT: c_int = -1;
const XML_TREE_NSMAP_DOC: c_int = -3;
const XML_TREE_NSMAP_CUSTOM: c_int = -4;

const XML_DOM_RECONNS_REMOVEREDUND: c_int = 1 << 0;

/// Upstream `struct xmlNsMapItem`.
#[repr(C)]
struct NsMapItem {
    next: *mut NsMapItem,
    prev: *mut NsMapItem,
    oldNs: *mut _xmlNs,
    newNs: *mut _xmlNs,
    shadowDepth: c_int,
    depth: c_int,
}

/// Upstream `struct xmlNsMap` (with a pool of detached items).
#[repr(C)]
struct NsMap {
    first: *mut NsMapItem,
    last: *mut NsMapItem,
    pool: *mut NsMapItem,
}

unsafe fn ns_map_not_empty(m: *mut NsMap) -> bool {
    !m.is_null() && !(*m).first.is_null()
}

/// Upstream `xmlDOMWrapNsMapFree`.
unsafe fn ns_map_free(nsmap: *mut NsMap) {
    if nsmap.is_null() {
        return;
    }
    let mut cur = (*nsmap).pool;
    while !cur.is_null() {
        let next = (*cur).next;
        unsafe { xmlFreeImpl(cur as *mut c_void) };
        cur = next;
    }
    cur = (*nsmap).first;
    while !cur.is_null() {
        let next = (*cur).next;
        unsafe { xmlFreeImpl(cur as *mut c_void) };
        cur = next;
    }
    unsafe { xmlFreeImpl(nsmap as *mut c_void) };
}

/// Upstream `xmlDOMWrapNsMapAddItem`.
unsafe fn ns_map_add_item(
    nsmap: *mut *mut NsMap,
    position: c_int,
    oldNs: *mut _xmlNs,
    newNs: *mut _xmlNs,
    depth: c_int,
) -> *mut NsMapItem {
    if nsmap.is_null() {
        return ptr::null_mut();
    }
    if position != -1 && position != 0 {
        return ptr::null_mut();
    }
    let mut map = *nsmap;
    if map.is_null() {
        map = unsafe { xmlMallocZero(size_of::<NsMap>()) } as *mut NsMap;
        if map.is_null() {
            return ptr::null_mut();
        }
        *nsmap = map;
    }
    let ret = unsafe { xmlMallocZero(size_of::<NsMapItem>()) } as *mut NsMapItem;
    if ret.is_null() {
        return ptr::null_mut();
    }
    if (*map).first.is_null() {
        (*map).first = ret;
        (*map).last = ret;
    } else if position == -1 {
        (*ret).prev = (*map).last;
        (*(*map).last).next = ret;
        (*map).last = ret;
    } else {
        (*(*map).first).prev = ret;
        (*ret).next = (*map).first;
        (*map).first = ret;
    }
    (*ret).oldNs = oldNs;
    (*ret).newNs = newNs;
    (*ret).shadowDepth = -1;
    (*ret).depth = depth;
    ret
}

/// Upstream `XML_NSMAP_POP`.
unsafe fn ns_map_pop(map: *mut NsMap) -> *mut NsMapItem {
    let i = (*map).last;
    (*map).last = (*i).prev;
    if (*map).last.is_null() {
        (*map).first = ptr::null_mut();
    } else {
        (*(*map).last).next = ptr::null_mut();
    }
    (*i).prev = ptr::null_mut();
    (*i).next = (*map).pool;
    (*map).pool = i;
    i
}

/// Upstream `xmlDOMWrapStoreNs`.
unsafe fn store_ns(
    doc: *mut _xmlDoc,
    ns_name: *const xmlChar,
    prefix: *const xmlChar,
) -> *mut _xmlNs {
    if doc.is_null() {
        return ptr::null_mut();
    }
    let mut ns = unsafe { ensure_xml_decl(doc) };
    if ns.is_null() {
        return ptr::null_mut();
    }
    if !(*ns).next.is_null() {
        ns = (*ns).next;
        loop {
            if unsafe { str_eq_or_ptr((*ns).prefix, prefix) }
                && !(*ns).href.is_null()
                && !ns_name.is_null()
                && unsafe { crate::abi::exports_xml2::xmlStrEqual((*ns).href, ns_name) != 0 }
            {
                return ns;
            }
            if (*ns).next.is_null() {
                break;
            }
            ns = (*ns).next;
        }
    }
    if !ns.is_null() {
        let n = unsafe { tree::new_ns(ptr::null_mut(), ns_name, prefix) };
        if n.is_null() {
            return ptr::null_mut();
        }
        (*ns).next = n;
        return n;
    }
    ptr::null_mut()
}

/// Upstream `xmlDOMWrapNSNormGatherInScopeNs`.
unsafe fn gather_in_scope_ns(map: *mut *mut NsMap, node: *mut _xmlNode) -> c_int {
    if map.is_null() || !(*map).is_null() {
        return -1;
    }
    if node.is_null() || (*node).type_ == XML_NAMESPACE_DECL as c_int {
        return -1;
    }
    let mut cur = node;
    while !cur.is_null() && cur != (*cur).doc as *mut _xmlNode {
        if (*cur).type_ == XML_ELEMENT_NODE as c_int && !(*cur).nsDef.is_null() {
            let mut ns = (*cur).nsDef;
            loop {
                let mut shadowed = 0;
                if unsafe { ns_map_not_empty(*map) } {
                    let mut mi = (*(*map)).first;
                    while !mi.is_null() {
                        if !(*mi).newNs.is_null()
                            && unsafe { str_eq_or_ptr((*ns).prefix, (*(*mi).newNs).prefix) }
                        {
                            shadowed = 1;
                            break;
                        }
                        mi = (*mi).next;
                    }
                }
                let mi =
                    unsafe { ns_map_add_item(map, 0, ptr::null_mut(), ns, XML_TREE_NSMAP_PARENT) };
                if mi.is_null() {
                    return -1;
                }
                if shadowed != 0 {
                    (*mi).shadowDepth = 0;
                }
                if (*ns).next.is_null() {
                    break;
                }
                ns = (*ns).next;
            }
        }
        cur = (*cur).parent;
    }
    0
}

/// Upstream `xmlDOMWrapNSNormAddNsMapItem2`.
unsafe fn ns_norm_add_ns_map_item2(
    list: *mut *mut *mut _xmlNs,
    size: *mut c_int,
    number: *mut c_int,
    old_ns: *mut _xmlNs,
    new_ns: *mut _xmlNs,
) -> c_int {
    if *number >= *size {
        let new_size = if *size <= 0 { 6 } else { (*size) * 2 };
        let tmp = unsafe {
            xmlReallocImpl(
                *list as *mut c_void,
                (new_size as usize) * 2 * size_of::<*mut _xmlNs>(),
            )
        } as *mut *mut _xmlNs;
        if tmp.is_null() {
            return -1;
        }
        *list = tmp;
        *size = new_size;
    }
    let arr = *list;
    unsafe {
        arr.add((2 * *number) as usize).write(old_ns);
        arr.add((2 * *number + 1) as usize).write(new_ns);
    }
    *number += 1;
    0
}

/// Upstream `xmlSearchNsByPrefixStrict`.
unsafe fn search_ns_by_prefix_strict(
    doc: *mut _xmlDoc,
    node: *mut _xmlNode,
    prefix: *const xmlChar,
    ret_ns: *mut *mut _xmlNs,
) -> c_int {
    if doc.is_null() || node.is_null() || (*node).type_ == XML_NAMESPACE_DECL as c_int {
        return -1;
    }
    if !ret_ns.is_null() {
        *ret_ns = ptr::null_mut();
    }
    if unsafe { is_str_xml(prefix) } {
        if !ret_ns.is_null() {
            let ns = unsafe { ensure_xml_decl(doc) };
            if ns.is_null() {
                return -1;
            }
            *ret_ns = ns;
        }
        return 1;
    }
    let mut cur = node;
    loop {
        if (*cur).type_ == XML_ELEMENT_NODE as c_int {
            if !(*cur).nsDef.is_null() {
                let mut ns = (*cur).nsDef;
                loop {
                    if unsafe { str_eq_or_ptr(prefix, (*ns).prefix) } {
                        // Disabled namespaces, e.g. xmlns:abc="".
                        if (*ns).href.is_null() {
                            return 0;
                        }
                        if !ret_ns.is_null() {
                            *ret_ns = ns;
                        }
                        return 1;
                    }
                    if (*ns).next.is_null() {
                        break;
                    }
                    ns = (*ns).next;
                }
            }
        } else if (*cur).type_ == XML_ENTITY_DECL as c_int {
            return 0;
        }
        cur = (*cur).parent;
        if cur.is_null() || (*cur).doc == cur as *mut _xmlDoc {
            break;
        }
    }
    0
}

/// Upstream `xmlSearchNsByNamespaceStrict`.
unsafe fn search_ns_by_namespace_strict(
    doc: *mut _xmlDoc,
    node: *mut _xmlNode,
    ns_name: *const xmlChar,
    ret_ns: *mut *mut _xmlNs,
    prefixed: c_int,
) -> c_int {
    if doc.is_null() || ns_name.is_null() || ret_ns.is_null() {
        return -1;
    }
    if node.is_null() || (*node).type_ == XML_NAMESPACE_DECL as c_int {
        return -1;
    }
    *ret_ns = ptr::null_mut();
    if unsafe {
        crate::abi::exports_xml2::xmlStrEqual(ns_name, XML_XML_NAMESPACE.as_ptr() as *const xmlChar)
            != 0
    } {
        let ns = unsafe { ensure_xml_decl(doc) };
        if ns.is_null() {
            return -1;
        }
        *ret_ns = ns;
        return 1;
    }
    let mut cur = node;
    let mut prev: *mut _xmlNode = ptr::null_mut();
    let mut out: *mut _xmlNode = ptr::null_mut();
    loop {
        if (*cur).type_ == XML_ELEMENT_NODE as c_int {
            if !(*cur).nsDef.is_null() {
                let mut ns = (*cur).nsDef;
                loop {
                    if prefixed != 0 && (*ns).prefix.is_null() {
                        if (*ns).next.is_null() {
                            break;
                        }
                        ns = (*ns).next;
                        continue;
                    }
                    if !prev.is_null() {
                        // Check the last level of ns-decls for a shadowing prefix.
                        let mut prevns = (*prev).nsDef;
                        let mut shadowed: *mut _xmlNs = ptr::null_mut();
                        while !prevns.is_null() {
                            if unsafe { str_eq_or_ptr((*prevns).prefix, (*ns).prefix) } {
                                shadowed = prevns;
                                break;
                            }
                            prevns = (*prevns).next;
                        }
                        if !shadowed.is_null() {
                            if (*ns).next.is_null() {
                                break;
                            }
                            ns = (*ns).next;
                            continue;
                        }
                    }
                    if unsafe { crate::abi::exports_xml2::xmlStrEqual(ns_name, (*ns).href) != 0 } {
                        if !out.is_null() {
                            // The prefix might be shadowed at the 3rd level.
                            if unsafe {
                                search_ns_by_prefix_strict(doc, node, (*ns).prefix, ptr::null_mut())
                            } == 0
                            {
                                if (*ns).next.is_null() {
                                    break;
                                }
                                ns = (*ns).next;
                                continue;
                            }
                        }
                        *ret_ns = ns;
                        return 1;
                    }
                    if (*ns).next.is_null() {
                        break;
                    }
                    ns = (*ns).next;
                }
                out = prev;
                prev = cur;
            }
        } else if (*cur).type_ == XML_ENTITY_DECL as c_int {
            return 0;
        }
        cur = (*cur).parent;
        if cur.is_null() || (*cur).doc == cur as *mut _xmlDoc {
            break;
        }
    }
    0
}

/// Upstream `xmlDOMWrapNSNormDeclareNsForced`.
unsafe fn declare_ns_forced(
    doc: *mut _xmlDoc,
    elem: *mut _xmlNode,
    ns_name: *const xmlChar,
    prefix: *const xmlChar,
    check_shadow: c_int,
) -> *mut _xmlNs {
    if doc.is_null() || elem.is_null() || (*elem).type_ != XML_ELEMENT_NODE as c_int {
        return ptr::null_mut();
    }
    let mut counter: c_int = 0;
    let mut buf: Vec<u8> = Vec::new();
    let mut cur_prefix: *const xmlChar = prefix;
    loop {
        let mut used = false;
        if !(*elem).nsDef.is_null()
            && !unsafe { ns_list_lookup_by_prefix((*elem).nsDef, cur_prefix) }.is_null()
        {
            used = true;
        }
        if !used
            && check_shadow != 0
            && !(*elem).parent.is_null()
            && unsafe {
                search_ns_by_prefix_strict(doc, (*elem).parent, cur_prefix, ptr::null_mut())
            } == 1
        {
            used = true;
        }
        if !used {
            let ret = unsafe { tree::new_ns(ptr::null_mut(), ns_name, cur_prefix) };
            if ret.is_null() {
                return ptr::null_mut();
            }
            if (*elem).nsDef.is_null() {
                (*elem).nsDef = ret;
            } else {
                let mut ns2 = (*elem).nsDef;
                while !(*ns2).next.is_null() {
                    ns2 = (*ns2).next;
                }
                (*ns2).next = ret;
            }
            return ret;
        }
        counter += 1;
        if counter > 1000 {
            return ptr::null_mut();
        }
        if prefix.is_null() {
            let s = format!("ns_{}", counter);
            buf = s.into_bytes();
            buf.push(0);
        } else {
            let p = unsafe { str_prefix(prefix, 30) };
            let p_str = String::from_utf8_lossy(&p[..p.len().saturating_sub(1)]);
            let s = format!("{}_{}", p_str, counter);
            buf = s.into_bytes();
            buf.push(0);
        }
        cur_prefix = buf.as_ptr() as *const xmlChar;
    }
}

/// Upstream `xmlDOMWrapNSNormAcquireNormalizedNs`.
#[allow(clippy::too_many_arguments)]
unsafe fn acquire_normalized_ns(
    doc: *mut _xmlDoc,
    elem: *mut _xmlNode,
    ns: *mut _xmlNs,
    ret_ns: *mut *mut _xmlNs,
    ns_map: *mut *mut NsMap,
    depth: c_int,
    ancestors_only: c_int,
    prefixed: c_int,
) -> c_int {
    if doc.is_null() || ns.is_null() || ret_ns.is_null() || ns_map.is_null() {
        return -1;
    }
    *ret_ns = ptr::null_mut();
    if unsafe { is_str_xml((*ns).prefix) } {
        let xml_ns = unsafe { ensure_xml_decl(doc) };
        if xml_ns.is_null() {
            return -1;
        }
        *ret_ns = xml_ns;
        return 0;
    }
    if unsafe { ns_map_not_empty(*ns_map) } && !(ancestors_only != 0 && elem.is_null()) {
        let mut mi = (*(*ns_map)).first;
        while !mi.is_null() {
            if (*mi).depth >= XML_TREE_NSMAP_PARENT
                && ((ancestors_only == 0) || (*mi).depth == XML_TREE_NSMAP_PARENT)
                && (*mi).shadowDepth == -1
                && !(*mi).newNs.is_null()
                && !(*(*mi).newNs).href.is_null()
                && *(*(*mi).newNs).href != 0
                && ((prefixed == 0) || !(*(*mi).newNs).prefix.is_null())
                && ((*ns).href == (*(*mi).newNs).href
                    || unsafe {
                        crate::abi::exports_xml2::xmlStrEqual((*ns).href, (*(*mi).newNs).href) != 0
                    })
            {
                (*mi).oldNs = ns;
                *ret_ns = (*mi).newNs;
                return 0;
            }
            mi = (*mi).next;
        }
    }
    if elem.is_null() {
        let tmpns = unsafe { store_ns(doc, (*ns).href, (*ns).prefix) };
        if tmpns.is_null() {
            return -1;
        }
        if unsafe { ns_map_add_item(ns_map, -1, ns, tmpns, XML_TREE_NSMAP_DOC) }.is_null() {
            return -1;
        }
        *ret_ns = tmpns;
    } else {
        let tmpns = unsafe { declare_ns_forced(doc, elem, (*ns).href, (*ns).prefix, 0) };
        if tmpns.is_null() {
            return -1;
        }
        if unsafe { ns_map_not_empty(*ns_map) } {
            let mut mi = (*(*ns_map)).first;
            while !mi.is_null() {
                if (*mi).depth < depth
                    && (*mi).shadowDepth == -1
                    && !(*mi).newNs.is_null()
                    && unsafe { str_eq_or_ptr((*ns).prefix, (*(*mi).newNs).prefix) }
                {
                    (*mi).shadowDepth = depth;
                    break;
                }
                mi = (*mi).next;
            }
        }
        if unsafe { ns_map_add_item(ns_map, -1, ns, tmpns, depth) }.is_null() {
            return -1;
        }
        *ret_ns = tmpns;
    }
    0
}

/// Pop the ns-map entries at/under `depth` and unshadow, when leaving an
/// element node (upstream tail of xmlDOMWrapReconcileNamespaces /
/// xmlDOMWrapAdoptBranch / xmlDOMWrapCloneNode).
unsafe fn ns_map_pop_depth(ns_map: *mut NsMap, depth: c_int) {
    if ns_map.is_null() {
        return;
    }
    if unsafe { ns_map_not_empty(ns_map) } {
        while !(*ns_map).last.is_null() && (*(*ns_map).last).depth >= depth {
            unsafe { ns_map_pop(ns_map) };
        }
        let mut mi = (*ns_map).first;
        while !mi.is_null() {
            if (*mi).shadowDepth >= depth {
                (*mi).shadowDepth = -1;
            }
            mi = (*mi).next;
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Namespace free functions
// ═══════════════════════════════════════════════════════════════════════════════

/// Free an xmlNs object.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlFreeNs(xmlNs *cur);
/// ```
///
/// # SAFETY
///
/// - `cur` must be a valid pointer to an _xmlNs, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlFreeNs(cur: *mut _xmlNs) {
    // QUIRK-0002/LORE-0006: namespace nodes have no parent (commit `044fc6b7`);
    // this frees only standalone xmlNewNs results — ns attached to nodes are
    // freed with the node, never via this path.
    unsafe { free_ns_impl(cur) };
}

/// Free a list of xmlNs objects.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlFreeNsList(xmlNs *cur);
/// ```
///
/// # SAFETY
///
/// - `cur` must be a valid pointer to the first _xmlNs, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlFreeNsList(cur: *mut _xmlNs) {
    let mut c = cur;
    while !c.is_null() {
        let next = (*c).next;
        unsafe { free_ns_impl(c) };
        c = next;
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Attribute free functions
// ═══════════════════════════════════════════════════════════════════════════════

/// Free an attribute including all children.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlFreeProp(xmlAttr *cur);
/// ```
///
/// # SAFETY
///
/// - `cur` must be a valid pointer to an _xmlAttr, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlFreeProp(cur: *mut _xmlAttr) {
    unsafe { free_prop_impl(cur) };
}

/// Free an attribute list including all children.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlFreePropList(xmlAttr *cur);
/// ```
///
/// # SAFETY
///
/// - `cur` must be a valid pointer to the first _xmlAttr, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlFreePropList(cur: *mut _xmlAttr) {
    let mut c = cur;
    while !c.is_null() {
        let next = (*c).next;
        unsafe { free_prop_impl(c) };
        c = next;
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Creation functions (elements, fragments, properties, references, texts)
// ═══════════════════════════════════════════════════════════════════════════════

/// Create an element node, eating the `name` string.
///
/// Like #xmlNewNode, but the `name` string will be used directly
/// without making a copy. Takes ownership of `name` which will also
/// be freed on error.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNode *xmlNewNodeEatName(xmlNs *ns, xmlChar *name);
/// ```
///
/// # SAFETY
///
/// - `name` must be a valid xmlMalloc'd string (ownership transferred).
#[no_mangle]
pub unsafe extern "C" fn xmlNewNodeEatName(ns: *mut _xmlNs, name: *mut xmlChar) -> *mut _xmlNode {
    unsafe { new_doc_node_eat_name(ptr::null_mut(), ns, name, ptr::null()) }
}

/// Create an element node.
///
/// If provided, `content` is expected to be a valid XML attribute value
/// possibly containing character and entity references. Syntax errors
/// and references to undeclared entities are ignored silently.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNode *xmlNewDocNode(xmlDoc *doc, xmlNs *ns,
///                        const xmlChar *name, const xmlChar *content);
/// ```
///
/// # SAFETY
///
/// - `name` must be a valid null-terminated string.
#[no_mangle]
pub unsafe extern "C" fn xmlNewDocNode(
    doc: *mut _xmlDoc,
    ns: *mut _xmlNs,
    name: *const xmlChar,
    content: *const xmlChar,
) -> *mut _xmlNode {
    unsafe { new_doc_node(doc, ns, name, content) }
}

/// Create an element node, eating the `name` string.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNode *xmlNewDocNodeEatName(xmlDoc *doc, xmlNs *ns,
///                               xmlChar *name, const xmlChar *content);
/// ```
///
/// # SAFETY
///
/// - `name` must be a valid xmlMalloc'd string (ownership transferred).
#[no_mangle]
pub unsafe extern "C" fn xmlNewDocNodeEatName(
    doc: *mut _xmlDoc,
    ns: *mut _xmlNs,
    name: *mut xmlChar,
    content: *const xmlChar,
) -> *mut _xmlNode {
    unsafe { new_doc_node_eat_name(doc, ns, name, content) }
}

/// Create an element node with raw (unescaped) text content.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNode *xmlNewDocRawNode(xmlDoc *doc, xmlNs *ns,
///                           const xmlChar *name, const xmlChar *content);
/// ```
///
/// # SAFETY
///
/// - `name` must be a valid null-terminated string.
#[no_mangle]
pub unsafe extern "C" fn xmlNewDocRawNode(
    doc: *mut _xmlDoc,
    ns: *mut _xmlNs,
    name: *const xmlChar,
    content: *const xmlChar,
) -> *mut _xmlNode {
    unsafe { new_doc_raw_node(doc, ns, name, content) }
}

/// Create a document fragment node.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNode *xmlNewDocFragment(xmlDoc *doc);
/// ```
///
/// # SAFETY
///
/// - `doc` must be a valid pointer to an _xmlDoc, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlNewDocFragment(doc: *mut _xmlDoc) -> *mut _xmlNode {
    let cur = unsafe { xmlMallocZero(size_of::<_xmlNode>()) } as *mut _xmlNode;
    if cur.is_null() {
        return ptr::null_mut();
    }
    (*cur).type_ = XML_DOCUMENT_FRAG_NODE as c_int;
    (*cur).doc = doc;
    cur
}

/// Create an attribute node (raw value).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlAttr *xmlNewProp(xmlNode *node, const xmlChar *name, const xmlChar *value);
/// ```
///
/// # SAFETY
///
/// - `name` must be a valid null-terminated string.
#[no_mangle]
pub unsafe extern "C" fn xmlNewProp(
    node: *mut _xmlNode,
    name: *const xmlChar,
    value: *const xmlChar,
) -> *mut _xmlAttr {
    if name.is_null() {
        return ptr::null_mut();
    }
    unsafe { new_prop_internal(node, ptr::null_mut(), name, value, 0) }
}

/// Create a namespaced attribute node (raw value).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlAttr *xmlNewNsProp(xmlNode *node, xmlNs *ns,
///                       const xmlChar *name, const xmlChar *value);
/// ```
///
/// # SAFETY
///
/// - `name` must be a valid null-terminated string.
#[no_mangle]
pub unsafe extern "C" fn xmlNewNsProp(
    node: *mut _xmlNode,
    ns: *mut _xmlNs,
    name: *const xmlChar,
    value: *const xmlChar,
) -> *mut _xmlAttr {
    if name.is_null() {
        return ptr::null_mut();
    }
    unsafe { new_prop_internal(node, ns, name, value, 0) }
}

/// Create a namespaced attribute node, eating the `name` string.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlAttr *xmlNewNsPropEatName(xmlNode *node, xmlNs *ns,
///                              xmlChar *name, const xmlChar *value);
/// ```
///
/// # SAFETY
///
/// - `name` must be a valid xmlMalloc'd string (ownership transferred).
#[no_mangle]
pub unsafe extern "C" fn xmlNewNsPropEatName(
    node: *mut _xmlNode,
    ns: *mut _xmlNs,
    name: *mut xmlChar,
    value: *const xmlChar,
) -> *mut _xmlAttr {
    if name.is_null() {
        return ptr::null_mut();
    }
    unsafe { new_prop_internal(node, ns, name, value, 1) }
}

/// Create an attribute node; `value` may contain XML character/entity
/// references.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlAttr *xmlNewDocProp(xmlDoc *doc, const xmlChar *name, const xmlChar *value);
/// ```
///
/// # SAFETY
///
/// - `name` must be a valid null-terminated string.
#[no_mangle]
pub unsafe extern "C" fn xmlNewDocProp(
    doc: *mut _xmlDoc,
    name: *const xmlChar,
    value: *const xmlChar,
) -> *mut _xmlAttr {
    if name.is_null() {
        return ptr::null_mut();
    }
    let cur = unsafe { xmlMallocZero(size_of::<_xmlAttr>()) } as *mut _xmlAttr;
    if cur.is_null() {
        return ptr::null_mut();
    }
    (*cur).type_ = XML_ATTRIBUTE_NODE as c_int;
    (*cur).name = unsafe { dup_str(name) };
    if (*cur).name.is_null() {
        unsafe { xmlFreeImpl(cur as *mut c_void) };
        return ptr::null_mut();
    }
    (*cur).doc = doc;
    if !value.is_null()
        && unsafe { node_parse_att_value(doc, cur as *mut _xmlNode, value, usize::MAX) } < 0
    {
        unsafe { free_prop_impl(cur) };
        return ptr::null_mut();
    }
    cur
}

/// Create a new entity reference node, linking the result with the
/// entity in `doc` if found.
///
/// Entity names like `&entity;` are handled as well.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNode *xmlNewReference(const xmlDoc *doc, const xmlChar *name);
/// ```
///
/// # SAFETY
///
/// - `name` must be a valid null-terminated string.
#[no_mangle]
pub unsafe extern "C" fn xmlNewReference(
    doc: *const _xmlDoc,
    name: *const xmlChar,
) -> *mut _xmlNode {
    if name.is_null() {
        return ptr::null_mut();
    }
    let cur = unsafe { xmlMallocZero(size_of::<_xmlNode>()) } as *mut _xmlNode;
    if cur.is_null() {
        return ptr::null_mut();
    }
    (*cur).type_ = XML_ENTITY_REF_NODE as c_int;
    (*cur).doc = doc as *mut _xmlDoc;
    let mut nm = name;
    if *nm == b'&' {
        nm = nm.add(1);
        let len = unsafe { str_len(nm) } as usize;
        (*cur).name = if len > 0 && unsafe { *nm.add(len - 1) } == b';' {
            unsafe { crate::abi::exports_xml2::xmlStrndup(nm, (len - 1) as c_int) }
        } else {
            unsafe { crate::abi::exports_xml2::xmlStrndup(nm, len as c_int) }
        };
    } else {
        (*cur).name = unsafe { dup_str(nm) };
    }
    if (*cur).name.is_null() {
        unsafe { tree::free_node(cur) };
        return ptr::null_mut();
    }
    let ent = unsafe { tree::get_doc_entity(doc as *mut _xmlDoc, (*cur).name) };
    if !ent.is_null() {
        (*cur).content = (*ent).content;
        (*cur).children = ent as *mut _xmlNode;
        (*cur).last = ent as *mut _xmlNode;
    }
    cur
}

/// Create a new child element with raw text content and append it to a
/// parent element.
///
/// If `ns` is NULL, the newly created element inherits the namespace
/// of the parent.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNode *xmlNewTextChild(xmlNode *parent, xmlNs *ns,
///                          const xmlChar *name, const xmlChar *content);
/// ```
///
/// # SAFETY
///
/// - `parent` must be a valid pointer to an _xmlNode.
/// - `name` must be a valid null-terminated string.
#[no_mangle]
pub unsafe extern "C" fn xmlNewTextChild(
    parent: *mut _xmlNode,
    ns: *mut _xmlNs,
    name: *const xmlChar,
    content: *const xmlChar,
) -> *mut _xmlNode {
    if parent.is_null() || name.is_null() {
        return ptr::null_mut();
    }
    let mut ns = ns;
    match (*parent).type_ as u32 {
        t if t == XML_DOCUMENT_NODE as u32
            || t == XML_HTML_DOCUMENT_NODE as u32
            || t == XML_DOCUMENT_FRAG_NODE as u32 => {}
        t if t == XML_ELEMENT_NODE as u32 => {
            if ns.is_null() {
                ns = (*parent).ns;
            }
        }
        _ => return ptr::null_mut(),
    }
    let cur = unsafe { new_doc_raw_node((*parent).doc, ns, name, content) };
    if cur.is_null() {
        return ptr::null_mut();
    }
    (*cur).parent = parent;
    if (*parent).children.is_null() {
        (*parent).children = cur;
        (*parent).last = cur;
    } else {
        let prev = (*parent).last;
        (*prev).next = cur;
        (*cur).prev = prev;
        (*parent).last = cur;
    }
    cur
}

/// Create a new text node.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNode *xmlNewTextLen(const xmlChar *content, int len);
/// ```
///
/// # SAFETY
///
/// - `content` must point to at least `len` bytes, or be NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlNewTextLen(content: *const xmlChar, len: c_int) -> *mut _xmlNode {
    let cur = unsafe { xmlMallocZero(size_of::<_xmlNode>()) } as *mut _xmlNode;
    if cur.is_null() {
        return ptr::null_mut();
    }
    (*cur).type_ = XML_TEXT_NODE as c_int;
    (*cur).name = unsafe { dup_str(TEXT_NAME.as_ptr() as *const xmlChar) };
    if !content.is_null() {
        (*cur).content = unsafe { crate::abi::exports_xml2::xmlStrndup(content, len) };
        if (*cur).content.is_null() {
            unsafe { tree::free_node(cur) };
            return ptr::null_mut();
        }
    }
    cur
}

// ═══════════════════════════════════════════════════════════════════════════════
// Content get/set/add
// ═══════════════════════════════════════════════════════════════════════════════

/// Replace the text content of a node.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlNodeSetContent(xmlNode *cur, const xmlChar *content);
/// ```
///
/// # SAFETY
///
/// - `cur` must be a valid pointer to an _xmlNode, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlNodeSetContent(cur: *mut _xmlNode, content: *const xmlChar) -> c_int {
    unsafe { node_set_content_internal(cur, content, -1) }
}

/// See #xmlNodeSetContent.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlNodeSetContentLen(xmlNode *cur, const xmlChar *content, int len);
/// ```
///
/// # SAFETY
///
/// - `cur` must be a valid pointer to an _xmlNode, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlNodeSetContentLen(
    cur: *mut _xmlNode,
    content: *const xmlChar,
    len: c_int,
) -> c_int {
    unsafe { node_set_content_internal(cur, content, len) }
}

/// Upstream `xmlNodeSetContentInternal`.
unsafe fn node_set_content_internal(
    cur: *mut _xmlNode,
    content: *const xmlChar,
    len: c_int,
) -> c_int {
    if cur.is_null() {
        return 1;
    }
    match (*cur).type_ as u32 {
        t if t == XML_DOCUMENT_FRAG_NODE as u32
            || t == XML_ELEMENT_NODE as u32
            || t == XML_ATTRIBUTE_NODE as u32 =>
        {
            let max_size = if len < 0 { usize::MAX } else { len as usize };
            if unsafe { node_parse_att_value((*cur).doc, cur, content, max_size) } < 0 {
                return -1;
            }
        }
        t if t == XML_TEXT_NODE as u32
            || t == XML_CDATA_SECTION_NODE as u32
            || t == XML_PI_NODE as u32
            || t == XML_COMMENT_NODE as u32 =>
        {
            let mut copy: *mut xmlChar = ptr::null_mut();
            if !content.is_null() {
                copy = if len < 0 {
                    unsafe { dup_str(content) }
                } else {
                    unsafe { crate::abi::exports_xml2::xmlStrndup(content, len) }
                };
                if copy.is_null() {
                    return -1;
                }
            }
            unsafe { text_set_content(cur, copy) };
        }
        _ => {}
    }
    0
}

/// Append the extra substring to the node content.
///
/// NOTE: In contrast to #xmlNodeSetContentLen, `content` is supposed
/// to be raw text, so unescaped XML special chars are allowed.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlNodeAddContent(xmlNode *cur, const xmlChar *content);
/// ```
///
/// # SAFETY
///
/// - `cur` must be a valid pointer to an _xmlNode, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlNodeAddContent(cur: *mut _xmlNode, content: *const xmlChar) -> c_int {
    let len = unsafe { str_len(content) };
    unsafe { xmlNodeAddContentLen(cur, content, len) }
}

/// Append the extra substring to the node content.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlNodeAddContentLen(xmlNode *cur, const xmlChar *content, int len);
/// ```
///
/// # SAFETY
///
/// - `cur` must be a valid pointer to an _xmlNode, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlNodeAddContentLen(
    cur: *mut _xmlNode,
    content: *const xmlChar,
    len: c_int,
) -> c_int {
    if cur.is_null() {
        return 1;
    }
    if content.is_null() || len <= 0 {
        return 0;
    }
    match (*cur).type_ as u32 {
        t if t == XML_DOCUMENT_FRAG_NODE as u32
            || t == XML_ELEMENT_NODE as u32
            || t == XML_ATTRIBUTE_NODE as u32 =>
        {
            let new_node = unsafe { new_doc_text_len((*cur).doc, content, len) };
            if new_node.is_null() {
                return -1;
            }
            let tmp = unsafe { add_child_coalesce(cur, new_node) };
            if tmp.is_null() {
                unsafe { tree::free_node(new_node) };
                return -1;
            }
        }
        t if t == XML_TEXT_NODE as u32
            || t == XML_CDATA_SECTION_NODE as u32
            || t == XML_PI_NODE as u32
            || t == XML_COMMENT_NODE as u32 =>
        {
            return unsafe { text_add_content(cur, content, len) };
        }
        _ => {}
    }
    0
}

/// Get the string value of a node (caller frees with xmlFree).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlNodeGetContent(const xmlNode *cur);
/// ```
///
/// # SAFETY
///
/// - `cur` must be a valid pointer to an _xmlNode, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlNodeGetContent(cur: *const _xmlNode) -> *mut xmlChar {
    unsafe { tree::node_get_content(cur as *mut _xmlNode) }
}

// ═══════════════════════════════════════════════════════════════════════════════
// xml:lang / xml:space / xml:base / name handling
// ═══════════════════════════════════════════════════════════════════════════════

/// Set the `xml:lang` attribute of a node.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlNodeSetLang(xmlNode *cur, const xmlChar *lang);
/// ```
///
/// # SAFETY
///
/// - `cur` must be a valid pointer to an element node.
#[no_mangle]
pub unsafe extern "C" fn xmlNodeSetLang(cur: *mut _xmlNode, lang: *const xmlChar) -> c_int {
    if cur.is_null() || (*cur).type_ != XML_ELEMENT_NODE as c_int {
        return 1;
    }
    let ns = unsafe { ensure_xml_decl((*cur).doc) };
    if ns.is_null() {
        return -1;
    }
    let attr = unsafe { set_ns_prop_impl(cur, ns, c"lang".as_ptr() as *const xmlChar, lang) };
    if attr.is_null() {
        return -1;
    }
    0
}

/// Find the `xml:lang` of a node (nearest ancestor-or-self).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlNodeGetLang(const xmlNode *cur);
/// ```
///
/// # SAFETY
///
/// - `cur` must be a valid pointer to an _xmlNode, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlNodeGetLang(cur: *const _xmlNode) -> *mut xmlChar {
    if cur.is_null() || (*cur).type_ == XML_NAMESPACE_DECL as c_int {
        return ptr::null_mut();
    }
    let mut c = cur;
    while !c.is_null() {
        let lang = unsafe {
            get_attr_value(
                c as *mut _xmlNode,
                c"lang".as_ptr() as *const xmlChar,
                XML_XML_NAMESPACE.as_ptr() as *const xmlChar,
            )
        };
        if !lang.is_null() {
            return lang;
        }
        c = (*c).parent;
    }
    ptr::null_mut()
}

/// Set the `xml:space` attribute of a node.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlNodeSetSpacePreserve(xmlNode *cur, int val);
/// ```
///
/// # SAFETY
///
/// - `cur` must be a valid pointer to an element node.
#[no_mangle]
pub unsafe extern "C" fn xmlNodeSetSpacePreserve(cur: *mut _xmlNode, val: c_int) -> c_int {
    if cur.is_null() || (*cur).type_ != XML_ELEMENT_NODE as c_int {
        return 1;
    }
    let ns = unsafe { ensure_xml_decl((*cur).doc) };
    if ns.is_null() {
        return -1;
    }
    let string: &[u8] = if val == 0 {
        b"default\0"
    } else {
        b"preserve\0"
    };
    let attr = unsafe {
        set_ns_prop_impl(
            cur,
            ns,
            c"space".as_ptr() as *const xmlChar,
            string.as_ptr() as *const xmlChar,
        )
    };
    if attr.is_null() {
        return -1;
    }
    0
}

/// Find the `xml:space` of a node.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlNodeGetSpacePreserve(const xmlNode *cur);
/// ```
///
/// # SAFETY
///
/// - `cur` must be a valid pointer to an element node, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlNodeGetSpacePreserve(cur: *const _xmlNode) -> c_int {
    if cur.is_null() || (*cur).type_ != XML_ELEMENT_NODE as c_int {
        return -1;
    }
    let mut c = cur;
    while !c.is_null() {
        let space = unsafe {
            get_attr_value(
                c as *mut _xmlNode,
                c"space".as_ptr() as *const xmlChar,
                XML_XML_NAMESPACE.as_ptr() as *const xmlChar,
            )
        };
        if !space.is_null() {
            if unsafe {
                crate::abi::exports_xml2::xmlStrEqual(space, c"preserve".as_ptr() as *const xmlChar)
                    != 0
            } {
                unsafe { xmlFreeImpl(space as *mut c_void) };
                return 1;
            }
            if unsafe {
                crate::abi::exports_xml2::xmlStrEqual(space, c"default".as_ptr() as *const xmlChar)
                    != 0
            } {
                unsafe { xmlFreeImpl(space as *mut c_void) };
                return 0;
            }
            unsafe { xmlFreeImpl(space as *mut c_void) };
        }
        c = (*c).parent;
    }
    -1
}

/// Set (or reset) the name of a node.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlNodeSetName(xmlNode *cur, const xmlChar *name);
/// ```
///
/// # SAFETY
///
/// - `cur` must be a valid pointer to an _xmlNode, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlNodeSetName(cur: *mut _xmlNode, name: *const xmlChar) {
    if cur.is_null() || name.is_null() {
        return;
    }
    match (*cur).type_ as u32 {
        t if t == XML_ELEMENT_NODE as u32
            || t == XML_ATTRIBUTE_NODE as u32
            || t == XML_PI_NODE as u32
            || t == XML_ENTITY_REF_NODE as u32 => {}
        _ => return,
    }
    let copy = unsafe { dup_str(name) };
    if copy.is_null() {
        return;
    }
    let old = (*cur).name;
    (*cur).name = copy;
    if !old.is_null() {
        unsafe { xmlFreeImpl(old as *mut c_void) };
    }
}

/// Set (or reset) the base URI of a node, i.e. the value of the
/// `xml:base` attribute.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlNodeSetBase(xmlNode *cur, const xmlChar* uri);
/// ```
///
/// # SAFETY
///
/// - `cur` must be a valid pointer to an _xmlNode, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlNodeSetBase(cur: *mut _xmlNode, uri: *const xmlChar) -> c_int {
    if cur.is_null() {
        return -1;
    }
    match (*cur).type_ as u32 {
        t if t == XML_ELEMENT_NODE as u32 || t == XML_ATTRIBUTE_NODE as u32 => {}
        t if t == XML_DOCUMENT_NODE as u32 || t == XML_HTML_DOCUMENT_NODE as u32 => {
            let doc = cur as *mut _xmlDoc;
            if !(*doc).URL.is_null() {
                unsafe { xmlFreeImpl((*doc).URL as *mut c_void) };
            }
            if uri.is_null() {
                (*doc).URL = ptr::null_mut();
            } else {
                (*doc).URL = unsafe { crate::abi::exports_uri::xmlPathToURI(uri as *const c_char) };
                if (*doc).URL.is_null() {
                    return -1;
                }
            }
            return 0;
        }
        _ => return -1,
    }
    let ns = unsafe { ensure_xml_decl((*cur).doc) };
    if ns.is_null() {
        return -1;
    }
    let fixed = unsafe { crate::abi::exports_uri::xmlPathToURI(uri as *const c_char) };
    if fixed.is_null() {
        return -1;
    }
    let attr = unsafe { set_ns_prop_impl(cur, ns, c"base".as_ptr() as *const xmlChar, fixed) };
    if attr.is_null() {
        unsafe { xmlFreeImpl(fixed as *mut c_void) };
        return -1;
    }
    unsafe { xmlFreeImpl(fixed as *mut c_void) };
    0
}

/// Searches for the base URI of a node (RFC 2396 sections 5.1.1/5.1.2).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlNodeGetBaseSafe(const xmlDoc *doc, const xmlNode *cur, xmlChar **baseOut);
/// ```
///
/// # SAFETY
///
/// - `doc`/`cur` must be valid pointers or NULL; `baseOut` must be valid.
#[no_mangle]
pub unsafe extern "C" fn xmlNodeGetBaseSafe(
    doc: *const _xmlDoc,
    cur: *const _xmlNode,
    baseOut: *mut *mut xmlChar,
) -> c_int {
    if baseOut.is_null() {
        return 1;
    }
    *baseOut = ptr::null_mut();
    if cur.is_null() && doc.is_null() {
        return 1;
    }
    if !cur.is_null() && (*cur).type_ == XML_NAMESPACE_DECL as c_int {
        return 1;
    }
    let mut doc = doc;
    if doc.is_null() {
        doc = (*cur).doc;
    }
    let mut ret: *mut xmlChar = ptr::null_mut();

    if !doc.is_null() && (*doc).type_ == XML_HTML_DOCUMENT_NODE as c_int {
        let mut c = (*doc).children;
        while !c.is_null() {
            if (*c).type_ != XML_ELEMENT_NODE as c_int {
                c = (*c).next;
                continue;
            }
            if unsafe { strcasecmp_eq((*c).name, b"html") } {
                c = (*c).children;
                continue;
            }
            if unsafe { strcasecmp_eq((*c).name, b"head") } {
                c = (*c).children;
                continue;
            }
            if unsafe { strcasecmp_eq((*c).name, b"base") } {
                ret = unsafe { get_attr_value(c, c"href".as_ptr() as *const xmlChar, ptr::null()) };
                if ret.is_null() {
                    return 1;
                }
                *baseOut = ret;
                return 0;
            }
            c = (*c).next;
        }
        return 0;
    }

    let mut c = cur;
    while !c.is_null() {
        if (*c).type_ == XML_ENTITY_DECL as c_int {
            let ent = c as *const _xmlEntity as *mut _xmlEntity;
            if (*ent).URI.is_null() {
                break;
            }
            if !ret.is_null() {
                unsafe { xmlFreeImpl(ret as *mut c_void) };
            }
            ret = unsafe { dup_str((*ent).URI) };
            if ret.is_null() {
                return -1;
            }
            *baseOut = ret;
            return 0;
        }
        if (*c).type_ == XML_ELEMENT_NODE as c_int {
            let base = unsafe {
                get_attr_value(
                    c as *mut _xmlNode,
                    c"base".as_ptr() as *const xmlChar,
                    XML_XML_NAMESPACE.as_ptr() as *const xmlChar,
                )
            };
            if !base.is_null() {
                if !ret.is_null() {
                    let mut newbase: *mut xmlChar = ptr::null_mut();
                    let res = unsafe {
                        crate::abi::exports_uri::xmlBuildURISafe(
                            ret as *const c_char,
                            base as *const c_char,
                            &mut newbase,
                        )
                    };
                    unsafe { xmlFreeImpl(ret as *mut c_void) };
                    unsafe { xmlFreeImpl(base as *mut c_void) };
                    if res != 0 {
                        return res;
                    }
                    ret = newbase;
                } else {
                    ret = base;
                }
                if !ret.is_null()
                    && (unsafe { *ret } == b'h'
                        || unsafe { *ret } == b'f'
                        || unsafe { *ret } == b'u')
                    && (unsafe {
                        crate::abi::exports_xml2::xmlStrncmp(
                            ret,
                            c"http://".as_ptr() as *const xmlChar,
                            7,
                        ) == 0
                    } || unsafe {
                        crate::abi::exports_xml2::xmlStrncmp(
                            ret,
                            c"ftp://".as_ptr() as *const xmlChar,
                            6,
                        ) == 0
                    } || unsafe {
                        crate::abi::exports_xml2::xmlStrncmp(
                            ret,
                            c"urn:".as_ptr() as *const xmlChar,
                            4,
                        ) == 0
                    })
                {
                    *baseOut = ret;
                    return 0;
                }
            }
        }
        c = (*c).parent;
    }

    if !doc.is_null() && !(*doc).URL.is_null() {
        if ret.is_null() {
            ret = unsafe { dup_str((*doc).URL) };
            if ret.is_null() {
                return -1;
            }
        } else {
            let mut newbase: *mut xmlChar = ptr::null_mut();
            let res = unsafe {
                crate::abi::exports_uri::xmlBuildURISafe(
                    ret as *const c_char,
                    (*doc).URL as *const c_char,
                    &mut newbase,
                )
            };
            unsafe { xmlFreeImpl(ret as *mut c_void) };
            if res != 0 {
                return res;
            }
            ret = newbase;
        }
    }
    *baseOut = ret;
    0
}

/// See #xmlNodeGetBaseSafe. Returns NULL if no base is found (memory
/// allocation failures are indistinguishable).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlNodeGetBase(const xmlDoc *doc, const xmlNode *cur);
/// ```
///
/// # SAFETY
///
/// - `doc`/`cur` must be valid pointers or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlNodeGetBase(doc: *const _xmlDoc, cur: *const _xmlNode) -> *mut xmlChar {
    let mut base: *mut xmlChar = ptr::null_mut();
    unsafe { xmlNodeGetBaseSafe(doc, cur, &mut base) };
    base
}

/// Check whether the node is a text node.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlNodeIsText(const xmlNode *node);
/// ```
///
/// # SAFETY
///
/// - `node` must be a valid pointer to an _xmlNode, or NULL.
#[no_mangle]
pub const unsafe extern "C" fn xmlNodeIsText(node: *const _xmlNode) -> c_int {
    if node.is_null() {
        return 0;
    }
    if (*node).type_ == XML_TEXT_NODE as c_int {
        1
    } else {
        0
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tree manipulation: xmlSetTreeDoc, xmlAddNextSibling, xmlAddPrevSibling,
// xmlAddChildList, xmlReplaceNode
// ═══════════════════════════════════════════════════════════════════════════════

/// Associate all nodes in a tree with a new document.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlSetTreeDoc(xmlNode *tree, xmlDoc *doc);
/// ```
///
/// # SAFETY
///
/// - `tree` must be the root of an unlinked subtree, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlSetTreeDoc(tree: *mut _xmlNode, doc: *mut _xmlDoc) -> c_int {
    unsafe { set_tree_doc_impl(tree, doc) }
}

/// Unlink `cur` and insert it as next sibling after `prev`.
///
/// Unlike #xmlAddChild this function does not merge text nodes.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNode *xmlAddNextSibling(xmlNode *prev, xmlNode *cur);
/// ```
///
/// # SAFETY
///
/// - `prev`/`cur` must be valid pointers to _xmlNode, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlAddNextSibling(
    prev: *mut _xmlNode,
    cur: *mut _xmlNode,
) -> *mut _xmlNode {
    if prev.is_null()
        || (*prev).type_ == XML_NAMESPACE_DECL as c_int
        || cur.is_null()
        || (*cur).type_ == XML_NAMESPACE_DECL as c_int
        || cur == prev
    {
        return ptr::null_mut();
    }
    if cur == (*prev).next {
        return cur;
    }
    unsafe { insert_node((*prev).doc, cur, (*prev).parent, prev, (*prev).next, 0) }
}

/// Unlink `cur` and insert it as previous sibling before `next`.
///
/// Unlike #xmlAddChild this function does not merge text nodes.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNode *xmlAddPrevSibling(xmlNode *next, xmlNode *cur);
/// ```
///
/// # SAFETY
///
/// - `next`/`cur` must be valid pointers to _xmlNode, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlAddPrevSibling(
    next: *mut _xmlNode,
    cur: *mut _xmlNode,
) -> *mut _xmlNode {
    if next.is_null()
        || (*next).type_ == XML_NAMESPACE_DECL as c_int
        || cur.is_null()
        || (*cur).type_ == XML_NAMESPACE_DECL as c_int
        || cur == next
    {
        return ptr::null_mut();
    }
    if cur == (*next).prev {
        return cur;
    }
    unsafe { insert_node((*next).doc, cur, (*next).parent, (*next).prev, next, 0) }
}

/// Append a node list to another node.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNode *xmlAddChildList(xmlNode *parent, xmlNode *cur);
/// ```
///
/// # SAFETY
///
/// - `parent`/`cur` must be valid pointers to _xmlNode, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlAddChildList(
    parent: *mut _xmlNode,
    cur: *mut _xmlNode,
) -> *mut _xmlNode {
    if parent.is_null() || (*parent).type_ == XML_NAMESPACE_DECL as c_int {
        return ptr::null_mut();
    }
    if cur.is_null() || (*cur).type_ == XML_NAMESPACE_DECL as c_int {
        return ptr::null_mut();
    }

    let mut oom = 0;
    let mut iter = cur;
    while !iter.is_null() {
        if (*iter).doc != (*parent).doc && unsafe { set_tree_doc_impl(iter, (*parent).doc) } < 0 {
            oom = 1;
        }
        iter = (*iter).next;
    }
    if oom != 0 {
        return ptr::null_mut();
    }

    let mut cur = cur;
    if (*parent).children.is_null() {
        (*parent).children = cur;
    } else {
        let prev = (*parent).last;
        if (*cur).type_ == XML_TEXT_NODE as c_int
            && (*prev).type_ == XML_TEXT_NODE as c_int
            && unsafe { str_eq_or_ptr((*cur).name, (*prev).name) }
        {
            if unsafe { text_add_content(prev, (*cur).content, -1) } < 0 {
                return ptr::null_mut();
            }
            let next = (*cur).next;
            unsafe { tree::free_node(cur) };
            if next.is_null() {
                return prev;
            }
            cur = next;
        }
        (*prev).next = cur;
        (*cur).prev = prev;
    }
    while !(*cur).next.is_null() {
        (*cur).parent = parent;
        cur = (*cur).next;
    }
    (*cur).parent = parent;
    (*parent).last = cur;
    cur
}

/// Unlink the old node; if `cur` is provided, it is unlinked and
/// inserted in place of `old`.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNode *xmlReplaceNode(xmlNode *old, xmlNode *cur);
/// ```
///
/// # SAFETY
///
/// - `old`/`cur` must be valid pointers to _xmlNode, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlReplaceNode(old: *mut _xmlNode, cur: *mut _xmlNode) -> *mut _xmlNode {
    if old == cur {
        return ptr::null_mut();
    }
    if old.is_null() || (*old).type_ == XML_NAMESPACE_DECL as c_int || (*old).parent.is_null() {
        return ptr::null_mut();
    }
    if cur.is_null() || (*cur).type_ == XML_NAMESPACE_DECL as c_int {
        // Don't route through xmlUnlinkNodeInternal to handle DTDs.
        unsafe { tree::unlink_node(old) };
        return old;
    }
    if (*old).type_ == XML_ATTRIBUTE_NODE as c_int && (*cur).type_ != XML_ATTRIBUTE_NODE as c_int {
        return old;
    }
    if (*cur).type_ == XML_ATTRIBUTE_NODE as c_int && (*old).type_ != XML_ATTRIBUTE_NODE as c_int {
        return old;
    }
    unsafe { tree::unlink_node(cur) };
    if unsafe { set_tree_doc_impl(cur, (*old).doc) } < 0 {
        return ptr::null_mut();
    }
    (*cur).parent = (*old).parent;
    (*cur).next = (*old).next;
    if !(*cur).next.is_null() {
        (*(*cur).next).prev = cur;
    }
    (*cur).prev = (*old).prev;
    if !(*cur).prev.is_null() {
        (*(*cur).prev).next = cur;
    }
    if !(*cur).parent.is_null() {
        if (*cur).type_ == XML_ATTRIBUTE_NODE as c_int {
            if (*(*cur).parent).properties == old as *mut _xmlAttr {
                (*(*cur).parent).properties = cur as *mut _xmlAttr;
            }
        } else {
            if (*(*cur).parent).children == old {
                (*(*cur).parent).children = cur;
            }
            if (*(*cur).parent).last == old {
                (*(*cur).parent).last = cur;
            }
        }
    }
    (*old).next = ptr::null_mut();
    (*old).prev = ptr::null_mut();
    (*old).parent = ptr::null_mut();
    old
}

// ═══════════════════════════════════════════════════════════════════════════════
// Namespace list + reconciliation
// ═══════════════════════════════════════════════════════════════════════════════

/// Find all in-scope namespaces of a node. `out` returns a NULL
/// terminated array of namespace pointers that must be freed by
/// the caller.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlGetNsListSafe(const xmlDoc *doc, const xmlNode *node, xmlNs ***out);
/// ```
///
/// # SAFETY
///
/// - `out` must be a valid pointer; `node` must be valid or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlGetNsListSafe(
    doc: *const _xmlDoc,
    node: *const _xmlNode,
    out: *mut *mut *mut _xmlNs,
) -> c_int {
    let _ = doc;
    if out.is_null() {
        return 1;
    }
    *out = ptr::null_mut();
    if node.is_null() || (*node).type_ == XML_NAMESPACE_DECL as c_int {
        return 1;
    }
    let mut namespaces: *mut *mut _xmlNs = ptr::null_mut();
    let mut nbns: c_int = 0;
    let mut maxns: c_int = 0;

    let mut n = node;
    while !n.is_null() {
        if (*n).type_ == XML_ELEMENT_NODE as c_int {
            let mut cur = (*n).nsDef;
            while !cur.is_null() {
                let mut i = 0;
                let mut found = false;
                while i < nbns {
                    if unsafe {
                        str_eq_or_ptr((*cur).prefix, (*(*namespaces).add(i as usize)).prefix)
                    } {
                        found = true;
                        break;
                    }
                    i += 1;
                }
                if !found {
                    if nbns >= maxns {
                        let new_size = if maxns <= 0 { 10 } else { maxns * 2 };
                        let tmp = unsafe {
                            xmlReallocImpl(
                                namespaces as *mut c_void,
                                ((new_size + 1) as usize) * size_of::<*mut _xmlNs>(),
                            )
                        } as *mut *mut _xmlNs;
                        if tmp.is_null() {
                            unsafe { xmlFreeImpl(namespaces as *mut c_void) };
                            return -1;
                        }
                        namespaces = tmp;
                        maxns = new_size;
                    }
                    *namespaces.add(nbns as usize) = cur;
                    nbns += 1;
                    *namespaces.add(nbns as usize) = ptr::null_mut();
                }
                cur = (*cur).next;
            }
        }
        n = (*n).parent;
    }

    *out = namespaces;
    if namespaces.is_null() {
        1
    } else {
        0
    }
}

/// Upstream `xmlNewReconciledNs` — locate a namespace definition in the tree
/// ancestors or create a new one similar to `ns`, reusing the prefix when
/// possible.
unsafe fn new_reconciled_ns(tree: *mut _xmlNode, ns: *mut _xmlNs) -> *mut _xmlNs {
    if tree.is_null() || (*tree).type_ != XML_ELEMENT_NODE as c_int {
        return ptr::null_mut();
    }
    if ns.is_null() || (*ns).type_ != XML_NAMESPACE_DECL as c_int {
        return ptr::null_mut();
    }
    // Search an existing namespace definition inherited.
    let def = unsafe { tree::search_ns_by_href(ptr::null_mut(), tree, (*ns).href) };
    if !def.is_null() {
        return def;
    }
    // Find a close prefix which is not already in use (strip > 20 chars).
    let prefix_base: Vec<u8> = if (*ns).prefix.is_null() {
        b"default\0".to_vec()
    } else {
        unsafe { str_prefix((*ns).prefix, 20) }
    };
    let mut counter: c_int = 1;
    let mut buf: Vec<u8> = prefix_base.clone();
    loop {
        let res = unsafe { tree::search_ns(ptr::null_mut(), tree, buf.as_ptr() as *const xmlChar) };
        if res.is_null() {
            break;
        }
        if counter > 1000 {
            return ptr::null_mut();
        }
        let base = &prefix_base[..prefix_base.len().saturating_sub(1)];
        let s = format!("{}{}", String::from_utf8_lossy(base), counter);
        buf = s.into_bytes();
        buf.push(0);
        counter += 1;
    }
    unsafe { tree::new_ns(tree, (*ns).href, buf.as_ptr() as *const xmlChar) }
}

/// This function checks that all the namespaces declared within the given
/// tree are properly declared.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlReconciliateNs(xmlDoc *doc, xmlNode *tree);
/// ```
///
/// # SAFETY
///
/// - `tree` must be an element node of `doc`, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlReconciliateNs(doc: *mut _xmlDoc, tree: *mut _xmlNode) -> c_int {
    let mut cache: Vec<(*mut _xmlNs, *mut _xmlNs)> = Vec::new();
    let mut ret = 0;

    if tree.is_null() || (*tree).type_ != XML_ELEMENT_NODE as c_int {
        return -1;
    }
    if (*tree).doc != doc {
        return -1;
    }
    let mut node = tree;
    loop {
        // Reconciliate the node namespace.
        if !(*node).ns.is_null() {
            let mut i = 0;
            let mut found = false;
            while i < cache.len() {
                if cache[i].0 == (*node).ns {
                    (*node).ns = cache[i].1;
                    found = true;
                    break;
                }
                i += 1;
            }
            if !found {
                let n = unsafe { new_reconciled_ns(tree, (*node).ns) };
                if n.is_null() {
                    ret = -1;
                } else {
                    cache.push(((*node).ns, n));
                }
                (*node).ns = n;
            }
        }
        // Check namespaces held by attributes.
        if (*node).type_ == XML_ELEMENT_NODE as c_int {
            let mut attr = (*node).properties;
            while !attr.is_null() {
                if !(*attr).ns.is_null() {
                    let mut i = 0;
                    let mut found = false;
                    while i < cache.len() {
                        if cache[i].0 == (*attr).ns {
                            (*attr).ns = cache[i].1;
                            found = true;
                            break;
                        }
                        i += 1;
                    }
                    if !found {
                        let n = unsafe { new_reconciled_ns(tree, (*attr).ns) };
                        if n.is_null() {
                            ret = -1;
                        } else {
                            cache.push(((*attr).ns, n));
                        }
                        (*attr).ns = n;
                    }
                }
                attr = (*attr).next;
            }
        }
        // Browse the full subtree, deep first.
        if !(*node).children.is_null() && (*node).type_ != XML_ENTITY_REF_NODE as c_int {
            node = (*node).children;
        } else if node != tree && !(*node).next.is_null() {
            node = (*node).next;
        } else if node != tree {
            // Go up to parents->next if needed (upstream `while (node != tree)`
            // re-checks the condition on every climb).
            loop {
                if node == tree {
                    break;
                }
                if !(*node).parent.is_null() {
                    node = (*node).parent;
                }
                if node != tree && !(*node).next.is_null() {
                    node = (*node).next;
                    break;
                }
                if (*node).parent.is_null() {
                    node = ptr::null_mut();
                    break;
                }
            }
            if node == tree {
                node = ptr::null_mut();
            }
        } else {
            break;
        }
        if node.is_null() {
            break;
        }
    }
    ret
}

// ═══════════════════════════════════════════════════════════════════════════════
// Compression mode
// ═══════════════════════════════════════════════════════════════════════════════

static XML_COMPRESS_MODE: AtomicI32 = AtomicI32::new(0);

/// Get the global compression level, ZLIB based.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlGetCompressMode(void);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlGetCompressMode() -> c_int {
    XML_COMPRESS_MODE.load(Ordering::Relaxed)
}

/// Set the global compression level, ZLIB based.
///
/// Correct values: 0 (uncompressed) to 9 (max compression)
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlSetCompressMode(int mode);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlSetCompressMode(mode: c_int) {
    let m = mode.clamp(0, 9);
    XML_COMPRESS_MODE.store(m, Ordering::Relaxed);
}

// ═══════════════════════════════════════════════════════════════════════════════
// DOM wrapper
// ═══════════════════════════════════════════════════════════════════════════════

/// Allocates and initializes a new DOM-wrapper context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDOMWrapCtxt *xmlDOMWrapNewCtxt(void);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlDOMWrapNewCtxt() -> *mut _xmlDOMWrapCtxt {
    unsafe { xmlMallocZero(size_of::<_xmlDOMWrapCtxt>()) as *mut _xmlDOMWrapCtxt }
}

/// Frees the DOM-wrapper context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlDOMWrapFreeCtxt(xmlDOMWrapCtxt *ctxt);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be a valid pointer returned by xmlDOMWrapNewCtxt, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlDOMWrapFreeCtxt(ctxt: *mut _xmlDOMWrapCtxt) {
    if ctxt.is_null() {
        return;
    }
    if !(*ctxt).namespaceMap.is_null() {
        unsafe { ns_map_free((*ctxt).namespaceMap as *mut NsMap) };
    }
    unsafe { xmlFreeImpl(ctxt as *mut c_void) };
}

/// Unlinks the given node from its owner, substituting ns-references to
/// `node->nsDef` for ns-references to `doc->oldNs`.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlDOMWrapRemoveNode(xmlDOMWrapCtxt *ctxt, xmlDoc *doc,
///                          xmlNode *node, int options);
/// ```
///
/// # SAFETY
///
/// - `doc`/`node` must be valid pointers.
#[no_mangle]
pub unsafe extern "C" fn xmlDOMWrapRemoveNode(
    ctxt: *mut _xmlDOMWrapCtxt,
    doc: *mut _xmlDoc,
    node: *mut _xmlNode,
    options: c_int,
) -> c_int {
    let _ = options;
    let mut list: *mut *mut _xmlNs = ptr::null_mut();
    let mut size_list: c_int = 0;
    let mut nb_list: c_int = 0;
    let mut ret = 0;

    if node.is_null() || doc.is_null() || (*node).doc != doc {
        return -1;
    }
    if (*node).parent.is_null() {
        return 0;
    }
    match (*node).type_ as u32 {
        t if t == XML_TEXT_NODE as u32
            || t == XML_CDATA_SECTION_NODE as u32
            || t == XML_ENTITY_REF_NODE as u32
            || t == XML_PI_NODE as u32
            || t == XML_COMMENT_NODE as u32 =>
        {
            unsafe { tree::unlink_node(node) };
            return 0;
        }
        t if t == XML_ELEMENT_NODE as u32 || t == XML_ATTRIBUTE_NODE as u32 => {}
        _ => return 1,
    }
    unsafe { tree::unlink_node(node) };

    let mut cur = node;
    'outer: loop {
        let node_type = (*cur).type_;
        match node_type as u32 {
            t if t == XML_ELEMENT_NODE as u32 => {
                if ctxt.is_null() && !(*cur).nsDef.is_null() {
                    let mut ns = (*cur).nsDef;
                    loop {
                        if unsafe {
                            ns_norm_add_ns_map_item2(
                                &mut list,
                                &mut size_list,
                                &mut nb_list,
                                ns,
                                ns,
                            )
                        } == -1
                        {
                            ret = -1;
                        }
                        if (*ns).next.is_null() {
                            break;
                        }
                        ns = (*ns).next;
                    }
                }
                if !(*cur).ns.is_null() {
                    let mut mapped = false;
                    if !list.is_null() {
                        let mut i = 0;
                        let mut j = 0;
                        while i < nb_list {
                            if (*cur).ns == *list.add(j as usize) {
                                (*cur).ns = *list.add((j + 1) as usize);
                                mapped = true;
                                break;
                            }
                            i += 1;
                            j += 2;
                        }
                    }
                    if !mapped {
                        let mut ns: *mut _xmlNs = ptr::null_mut();
                        if ctxt.is_null() {
                            ns = unsafe { store_ns(doc, (*(*cur).ns).href, (*(*cur).ns).prefix) };
                            if ns.is_null() {
                                ret = -1;
                            }
                        }
                        if !ns.is_null()
                            && unsafe {
                                ns_norm_add_ns_map_item2(
                                    &mut list,
                                    &mut size_list,
                                    &mut nb_list,
                                    (*cur).ns,
                                    ns,
                                )
                            } == -1
                        {
                            ret = -1;
                        }
                        (*cur).ns = ns;
                    }
                }
                if !(*cur).properties.is_null() {
                    cur = (*cur).properties as *mut _xmlNode;
                    continue 'outer;
                }
            }
            t if t == XML_ATTRIBUTE_NODE as u32 && !(*cur).ns.is_null() => {
                let mut mapped = false;
                if !list.is_null() {
                    let mut i = 0;
                    let mut j = 0;
                    while i < nb_list {
                        if (*cur).ns == *list.add(j as usize) {
                            (*cur).ns = *list.add((j + 1) as usize);
                            mapped = true;
                            break;
                        }
                        i += 1;
                        j += 2;
                    }
                }
                if !mapped {
                    let mut ns: *mut _xmlNs = ptr::null_mut();
                    if ctxt.is_null() {
                        ns = unsafe { store_ns(doc, (*(*cur).ns).href, (*(*cur).ns).prefix) };
                        if ns.is_null() {
                            ret = -1;
                        }
                    }
                    if !ns.is_null()
                        && unsafe {
                            ns_norm_add_ns_map_item2(
                                &mut list,
                                &mut size_list,
                                &mut nb_list,
                                (*cur).ns,
                                ns,
                            )
                        } == -1
                    {
                        ret = -1;
                    }
                    (*cur).ns = ns;
                }
            }
            _ => {}
        }
        // Descend into children.
        if node_type == XML_ELEMENT_NODE as c_int && !(*cur).children.is_null() {
            cur = (*cur).children;
            continue 'outer;
        }
        // Advance: next sibling, or up.
        loop {
            if cur.is_null() {
                break 'outer;
            }
            if !(*cur).next.is_null() {
                cur = (*cur).next;
                break;
            }
            let t = (*cur).type_;
            cur = (*cur).parent;
            if t == XML_ATTRIBUTE_NODE as c_int && !cur.is_null() && !(*cur).children.is_null() {
                cur = (*cur).children;
                break;
            }
            // else goto next_sibling again
            if cur.is_null() {
                break 'outer;
            }
        }
    }

    if !list.is_null() {
        unsafe { xmlFreeImpl(list as *mut c_void) };
    }
    ret
}

/// Fix up namespaces: ensures ns-references point to ns-decls held on
/// element-nodes and creates additional ns-decls where needed.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlDOMWrapReconcileNamespaces(xmlDOMWrapCtxt *ctxt, xmlNode *elem, int options);
/// ```
///
/// # SAFETY
///
/// - `elem` must be an element node, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlDOMWrapReconcileNamespaces(
    ctxt: *mut _xmlDOMWrapCtxt,
    elem: *mut _xmlNode,
    options: c_int,
) -> c_int {
    let _ = ctxt;
    let mut depth: c_int = -1;
    let mut adoptns: c_int = 0;
    let mut parnsdone: c_int = 0;
    let mut ns_map: *mut NsMap = ptr::null_mut();
    let ancestors_only: c_int = 0;
    let opt_remove_redundant = (options & XML_DOM_RECONNS_REMOVEREDUND) != 0;
    let mut list_redund: *mut *mut _xmlNs = ptr::null_mut();
    let mut size_redund: c_int = 0;
    let mut nb_redund: c_int = 0;
    let mut ret = 0;

    if elem.is_null() || (*elem).doc.is_null() || (*elem).type_ != XML_ELEMENT_NODE as c_int {
        return -1;
    }
    let doc = (*elem).doc;
    let mut cur = elem;
    let mut cur_elem: *mut _xmlNode = ptr::null_mut();

    'outer: loop {
        match (*cur).type_ as u32 {
            t if t == XML_ELEMENT_NODE as u32 => {
                adoptns = 1;
                cur_elem = cur;
                depth += 1;
                if !(*cur).nsDef.is_null() {
                    let mut prevns: *mut _xmlNs = ptr::null_mut();
                    let mut ns = (*cur).nsDef;
                    loop {
                        if parnsdone == 0 {
                            if !(*elem).parent.is_null()
                                && (*elem).parent != (*(*elem).parent).doc as *mut _xmlNode
                                && unsafe { gather_in_scope_ns(&mut ns_map, (*elem).parent) } == -1
                            {
                                ret = -1;
                            }
                            parnsdone = 1;
                        }
                        // Remove redundant ns-decls.
                        if opt_remove_redundant && unsafe { ns_map_not_empty(ns_map) } {
                            let mut mi = (*ns_map).first;
                            let mut removed = false;
                            while !mi.is_null() {
                                if (*mi).depth >= XML_TREE_NSMAP_PARENT
                                    && (*mi).shadowDepth == -1
                                    && !(*mi).newNs.is_null()
                                    && unsafe { str_eq_or_ptr((*ns).prefix, (*(*mi).newNs).prefix) }
                                    && unsafe { str_eq_or_ptr((*ns).href, (*(*mi).newNs).href) }
                                {
                                    if unsafe {
                                        ns_norm_add_ns_map_item2(
                                            &mut list_redund,
                                            &mut size_redund,
                                            &mut nb_redund,
                                            ns,
                                            (*mi).newNs,
                                        )
                                    } == -1
                                    {
                                        ret = -1;
                                    } else {
                                        let next_ns = (*ns).next;
                                        if !prevns.is_null() {
                                            (*prevns).next = next_ns;
                                        } else {
                                            (*cur).nsDef = next_ns;
                                        }
                                        ns = next_ns;
                                        removed = true;
                                        break;
                                    }
                                }
                                mi = (*mi).next;
                            }
                            if removed {
                                if ns.is_null() {
                                    break;
                                }
                                continue;
                            }
                        }
                        // Skip ns-references handling if the referenced ns-decl
                        // is declared on the same element.
                        if !(*cur).ns.is_null() && adoptns != 0 && (*cur).ns == ns {
                            adoptns = 0;
                        }
                        // Does it shadow any ns-decl?
                        if unsafe { ns_map_not_empty(ns_map) } {
                            let mut mi = (*ns_map).first;
                            while !mi.is_null() {
                                if (*mi).depth >= XML_TREE_NSMAP_PARENT
                                    && (*mi).shadowDepth == -1
                                    && !(*mi).newNs.is_null()
                                    && unsafe { str_eq_or_ptr((*ns).prefix, (*(*mi).newNs).prefix) }
                                {
                                    (*mi).shadowDepth = depth;
                                }
                                mi = (*mi).next;
                            }
                        }
                        // Push mapping.
                        if unsafe { ns_map_add_item(&mut ns_map, -1, ns, ns, depth) }.is_null() {
                            ret = -1;
                        }
                        prevns = ns;
                        if (*ns).next.is_null() {
                            break;
                        }
                        ns = (*ns).next;
                    }
                }
                if adoptns == 0 {
                    // goto ns_end
                } else {
                    // Falls through to the ns-reference handling.
                    let _ = unsafe {
                        reconcile_ns_reference(
                            doc,
                            elem,
                            cur,
                            cur_elem,
                            &mut ns_map,
                            depth,
                            ancestors_only,
                            &mut ret,
                            &mut parnsdone,
                            &mut list_redund,
                            &mut size_redund,
                            &mut nb_redund,
                        )
                    };
                }
                // ns_end:
                if (*cur).type_ == XML_ELEMENT_NODE as c_int && !(*cur).properties.is_null() {
                    cur = (*cur).properties as *mut _xmlNode;
                    continue 'outer;
                }
            }
            t if t == XML_ATTRIBUTE_NODE as u32 => {
                let _ = unsafe {
                    reconcile_ns_reference(
                        doc,
                        elem,
                        cur,
                        cur_elem,
                        &mut ns_map,
                        depth,
                        ancestors_only,
                        &mut ret,
                        &mut parnsdone,
                        &mut list_redund,
                        &mut size_redund,
                        &mut nb_redund,
                    )
                };
            }
            _ => {
                // goto next_sibling
            }
        }
        // into_content
        if (*cur).type_ == XML_ELEMENT_NODE as c_int && !(*cur).children.is_null() {
            cur = (*cur).children;
            continue 'outer;
        }
        // next_sibling
        if cur == elem {
            break;
        }
        if (*cur).type_ == XML_ELEMENT_NODE as c_int {
            unsafe { ns_map_pop_depth(ns_map, depth) };
            depth -= 1;
        }
        if !(*cur).next.is_null() {
            cur = (*cur).next;
        } else {
            if (*cur).type_ == XML_ATTRIBUTE_NODE as c_int {
                cur = (*cur).parent;
                // goto into_content
                if (*cur).type_ == XML_ELEMENT_NODE as c_int && !(*cur).children.is_null() {
                    cur = (*cur).children;
                    continue 'outer;
                }
                // fall through to next_sibling
                if cur == elem {
                    break;
                }
                if (*cur).type_ == XML_ELEMENT_NODE as c_int {
                    unsafe { ns_map_pop_depth(ns_map, depth) };
                    depth -= 1;
                }
                if !(*cur).next.is_null() {
                    cur = (*cur).next;
                } else {
                    cur = (*cur).parent;
                    if cur.is_null() {
                        break;
                    }
                }
                continue 'outer;
            }
            cur = (*cur).parent;
            if cur.is_null() {
                break;
            }
            // goto next_sibling (loop around)
        }
    }

    if !list_redund.is_null() {
        let mut i = 0;
        let mut j = 0;
        while i < nb_redund {
            unsafe { free_ns_impl(*list_redund.add(j as usize)) };
            i += 1;
            j += 2;
        }
        unsafe { xmlFreeImpl(list_redund as *mut c_void) };
    }
    if !ns_map.is_null() {
        unsafe { ns_map_free(ns_map) };
    }
    ret
}

/// Shared ns-reference handling of xmlDOMWrapReconcileNamespaces (the
/// `XML_ATTRIBUTE_NODE` case body, also reached by element fall-through).
#[allow(clippy::too_many_arguments)]
unsafe fn reconcile_ns_reference(
    doc: *mut _xmlDoc,
    elem: *mut _xmlNode,
    cur: *mut _xmlNode,
    cur_elem: *mut _xmlNode,
    ns_map: *mut *mut NsMap,
    depth: c_int,
    ancestors_only: c_int,
    ret: &mut c_int,
    parnsdone: &mut c_int,
    list_redund: *mut *mut *mut _xmlNs,
    _size_redund: *mut c_int,
    nb_redund: *mut c_int,
) -> c_int {
    if (*cur).ns.is_null() {
        return 0;
    }
    if *parnsdone == 0 {
        if !elem.is_null()
            && !(*elem).parent.is_null()
            && (*elem).parent != (*(*elem).parent).doc as *mut _xmlNode
            && unsafe { gather_in_scope_ns(ns_map, (*elem).parent) } == -1
        {
            *ret = -1;
        }
        *parnsdone = 1;
    }
    // Adjust the reference if this was a redundant ns-decl.
    if !list_redund.is_null() && !(*list_redund).is_null() && *nb_redund > 0 {
        let list = *list_redund;
        let mut i = 0;
        let mut j = 0;
        while i < *nb_redund {
            if (*cur).ns == *list.add(j as usize) {
                (*cur).ns = *list.add((j + 1) as usize);
                return 0;
            }
            i += 1;
            j += 2;
        }
    }
    // Adopt ns-references.
    if unsafe { ns_map_not_empty(*ns_map) } {
        let mut mi = (*(*ns_map)).first;
        while !mi.is_null() {
            if (*mi).shadowDepth == -1 && (*cur).ns == (*mi).oldNs {
                (*cur).ns = (*mi).newNs;
                return 0;
            }
            mi = (*mi).next;
        }
    }
    // Acquire a normalized ns-decl and add it to the map.
    let mut ns: *mut _xmlNs = ptr::null_mut();
    if unsafe {
        acquire_normalized_ns(
            doc,
            cur_elem,
            (*cur).ns,
            &mut ns,
            ns_map,
            depth,
            ancestors_only,
            if (*cur).type_ == XML_ATTRIBUTE_NODE as c_int {
                1
            } else {
                0
            },
        )
    } == -1
    {
        *ret = -1;
    }
    (*cur).ns = ns;
    0
}

/// Upstream `xmlDOMWrapAdoptBranch` — fix up namespaces and move `node` to
/// `destDoc`, declaring namespaces on `destParent` when given.
unsafe fn domwrap_adopt_branch(
    ctxt: *mut _xmlDOMWrapCtxt,
    node: *mut _xmlNode,
    dest_doc: *mut _xmlDoc,
    dest_parent: *mut _xmlNode,
) -> c_int {
    let mut ret = 0;
    let mut cur = node;
    let mut cur_elem: *mut _xmlNode = ptr::null_mut();
    let mut ns_map: *mut NsMap = ptr::null_mut();
    let mut ns: *mut _xmlNs = ptr::null_mut();
    let mut depth: c_int = -1;
    let mut parnsdone: c_int;
    let ancestors_only: c_int = 0;
    let mut leave = false;

    if !ctxt.is_null() {
        ns_map = (*ctxt).namespaceMap as *mut NsMap;
    }
    // Disable search for ns-decls in the parent-axis of the destination
    // element if there's no destination parent or custom handling is used.
    if dest_parent.is_null() || (!ctxt.is_null() && (*ctxt).getNsForNodeFunc.is_some()) {
        parnsdone = 1;
    } else {
        parnsdone = 0;
    }

    'outer: loop {
        if !leave {
            if (*cur).doc != dest_doc && unsafe { node_set_doc_impl(cur, dest_doc) } < 0 {
                ret = -1;
            }
            match (*cur).type_ as u32 {
                t if t == XML_XINCLUDE_START as u32 || t == XML_XINCLUDE_END as u32 => {
                    ret = -1;
                    leave = true;
                }
                t if t == XML_ELEMENT_NODE as u32 => {
                    cur_elem = cur;
                    depth += 1;
                    if !(*cur).nsDef.is_null()
                        && (ctxt.is_null() || (*ctxt).getNsForNodeFunc.is_none())
                    {
                        if parnsdone == 0 {
                            if unsafe { gather_in_scope_ns(&mut ns_map, dest_parent) } == -1 {
                                ret = -1;
                            }
                            parnsdone = 1;
                        }
                        let mut ns2 = (*cur).nsDef;
                        loop {
                            if unsafe { ns_map_not_empty(ns_map) } {
                                let mut mi = (*ns_map).first;
                                while !mi.is_null() {
                                    if (*mi).depth >= XML_TREE_NSMAP_PARENT
                                        && (*mi).shadowDepth == -1
                                        && !(*mi).newNs.is_null()
                                        && unsafe {
                                            str_eq_or_ptr((*ns2).prefix, (*(*mi).newNs).prefix)
                                        }
                                    {
                                        (*mi).shadowDepth = depth;
                                    }
                                    mi = (*mi).next;
                                }
                            }
                            if unsafe { ns_map_add_item(&mut ns_map, -1, ns2, ns2, depth) }
                                .is_null()
                            {
                                ret = -1;
                            }
                            if (*ns2).next.is_null() {
                                break;
                            }
                            ns2 = (*ns2).next;
                        }
                    }
                    if !(*cur).ns.is_null() {
                        if parnsdone == 0 {
                            if unsafe { gather_in_scope_ns(&mut ns_map, dest_parent) } == -1 {
                                ret = -1;
                            }
                            parnsdone = 1;
                        }
                        let mut mapped = false;
                        if unsafe { ns_map_not_empty(ns_map) } {
                            let mut mi = (*ns_map).first;
                            while !mi.is_null() {
                                if (*mi).shadowDepth == -1 && (*cur).ns == (*mi).oldNs {
                                    (*cur).ns = (*mi).newNs;
                                    mapped = true;
                                    break;
                                }
                                mi = (*mi).next;
                            }
                        }
                        if !mapped {
                            if !ctxt.is_null() && (*ctxt).getNsForNodeFunc.is_some() {
                                let f = (*ctxt).getNsForNodeFunc.unwrap();
                                ns =
                                    unsafe { f(ctxt, cur, (*(*cur).ns).href, (*(*cur).ns).prefix) };
                                unsafe {
                                    ns_map_add_item(
                                        &mut ns_map,
                                        -1,
                                        (*cur).ns,
                                        ns,
                                        XML_TREE_NSMAP_CUSTOM,
                                    )
                                };
                                (*cur).ns = ns;
                            } else {
                                if unsafe {
                                    acquire_normalized_ns(
                                        dest_doc,
                                        if dest_parent.is_null() {
                                            ptr::null_mut()
                                        } else {
                                            cur_elem
                                        },
                                        (*cur).ns,
                                        &mut ns,
                                        &mut ns_map,
                                        depth,
                                        ancestors_only,
                                        0,
                                    )
                                } == -1
                                {
                                    ret = -1;
                                }
                                (*cur).ns = ns;
                            }
                        }
                    }
                    (*cur).psvi = ptr::null_mut();
                    (*cur).line = 0;
                    (*cur).extra = 0;
                    if !(*cur).properties.is_null() {
                        cur = (*cur).properties as *mut _xmlNode;
                        continue 'outer;
                    }
                }
                t if t == XML_ATTRIBUTE_NODE as u32 => {
                    if !(*cur).ns.is_null() {
                        if parnsdone == 0 {
                            if unsafe { gather_in_scope_ns(&mut ns_map, dest_parent) } == -1 {
                                ret = -1;
                            }
                            parnsdone = 1;
                        }
                        let mut mapped = false;
                        if unsafe { ns_map_not_empty(ns_map) } {
                            let mut mi = (*ns_map).first;
                            while !mi.is_null() {
                                if (*mi).shadowDepth == -1 && (*cur).ns == (*mi).oldNs {
                                    (*cur).ns = (*mi).newNs;
                                    mapped = true;
                                    break;
                                }
                                mi = (*mi).next;
                            }
                        }
                        if !mapped {
                            if !ctxt.is_null() && (*ctxt).getNsForNodeFunc.is_some() {
                                let f = (*ctxt).getNsForNodeFunc.unwrap();
                                ns =
                                    unsafe { f(ctxt, cur, (*(*cur).ns).href, (*(*cur).ns).prefix) };
                                unsafe {
                                    ns_map_add_item(
                                        &mut ns_map,
                                        -1,
                                        (*cur).ns,
                                        ns,
                                        XML_TREE_NSMAP_CUSTOM,
                                    )
                                };
                                (*cur).ns = ns;
                            } else {
                                if unsafe {
                                    acquire_normalized_ns(
                                        dest_doc,
                                        if dest_parent.is_null() {
                                            ptr::null_mut()
                                        } else {
                                            cur_elem
                                        },
                                        (*cur).ns,
                                        &mut ns,
                                        &mut ns_map,
                                        depth,
                                        ancestors_only,
                                        1,
                                    )
                                } == -1
                                {
                                    ret = -1;
                                }
                                (*cur).ns = ns;
                            }
                        }
                    }
                }
                t if t == XML_TEXT_NODE as u32
                    || t == XML_CDATA_SECTION_NODE as u32
                    || t == XML_PI_NODE as u32
                    || t == XML_COMMENT_NODE as u32
                    || t == XML_ENTITY_REF_NODE as u32 =>
                {
                    leave = true;
                }
                _ => {
                    ret = -1;
                    leave = true;
                }
            }
            if !leave {
                if !(*cur).children.is_null() {
                    cur = (*cur).children;
                    continue 'outer;
                }
                leave = true;
            }
        }
        // === leave_node ===
        leave = false;
        if cur == node {
            break;
        }
        if (*cur).type_ == XML_ELEMENT_NODE as c_int
            || (*cur).type_ == XML_XINCLUDE_START as c_int
            || (*cur).type_ == XML_XINCLUDE_END as c_int
        {
            unsafe { ns_map_pop_depth(ns_map, depth) };
            depth -= 1;
        }
        if !(*cur).next.is_null() {
            cur = (*cur).next;
        } else if (*cur).type_ == XML_ATTRIBUTE_NODE as c_int
            && !(*cur).parent.is_null()
            && !(*(*cur).parent).children.is_null()
        {
            cur = (*(*cur).parent).children;
        } else {
            cur = (*cur).parent;
            if cur.is_null() {
                break;
            }
            // goto leave_node
            leave = true;
        }
    }

    if !ns_map.is_null() {
        if !ctxt.is_null() && (*ctxt).namespaceMap == ns_map as *mut c_void {
            // Just cleanup the map but don't free.
            if !(*ns_map).first.is_null() {
                if !(*ns_map).pool.is_null() {
                    (*(*ns_map).last).next = (*ns_map).pool;
                }
                (*ns_map).pool = (*ns_map).first;
                (*ns_map).first = ptr::null_mut();
            }
        } else {
            unsafe { ns_map_free(ns_map) };
        }
    }
    ret
}

/// Upstream `xmlDOMWrapAdoptAttr`.
unsafe fn domwrap_adopt_attr(
    ctxt: *mut _xmlDOMWrapCtxt,
    attr: *mut _xmlAttr,
    dest_doc: *mut _xmlDoc,
    dest_parent: *mut _xmlNode,
) -> c_int {
    let _ = ctxt;
    let mut ret = 0;
    if attr.is_null() || dest_doc.is_null() {
        return -1;
    }
    if (*attr).doc != dest_doc && unsafe { set_tree_doc_impl(attr as *mut _xmlNode, dest_doc) } < 0
    {
        ret = -1;
    }
    if !(*attr).ns.is_null() {
        let mut ns: *mut _xmlNs = ptr::null_mut();
        if unsafe { is_str_xml((*(*attr).ns).prefix) } {
            ns = unsafe { ensure_xml_decl(dest_doc) };
        } else if dest_parent.is_null() {
            ns = unsafe { store_ns(dest_doc, (*(*attr).ns).href, (*(*attr).ns).prefix) };
        } else {
            if unsafe {
                search_ns_by_namespace_strict(dest_doc, dest_parent, (*(*attr).ns).href, &mut ns, 1)
            } == -1
            {
                ret = -1;
            }
            if ns.is_null() {
                ns = unsafe {
                    declare_ns_forced(
                        dest_doc,
                        dest_parent,
                        (*(*attr).ns).href,
                        (*(*attr).ns).prefix,
                        1,
                    )
                };
            }
        }
        if ns.is_null() {
            ret = -1;
        }
        (*attr).ns = ns;
    }
    ret
}

/// Fix up namespaces before moving a node.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlDOMWrapAdoptNode(xmlDOMWrapCtxt *ctxt, xmlDoc *sourceDoc,
///                         xmlNode *node, xmlDoc *destDoc,
///                         xmlNode *destParent, int options);
/// ```
///
/// # SAFETY
///
/// - `node`/`destDoc` must be valid pointers.
#[no_mangle]
pub unsafe extern "C" fn xmlDOMWrapAdoptNode(
    ctxt: *mut _xmlDOMWrapCtxt,
    sourceDoc: *mut _xmlDoc,
    node: *mut _xmlNode,
    destDoc: *mut _xmlDoc,
    destParent: *mut _xmlNode,
    options: c_int,
) -> c_int {
    let _ = options;
    let mut ret = 0;
    if node.is_null()
        || (*node).type_ == XML_NAMESPACE_DECL as c_int
        || destDoc.is_null()
        || (!destParent.is_null() && (*destParent).doc != destDoc)
    {
        return -1;
    }
    let mut sourceDoc = sourceDoc;
    if sourceDoc.is_null() {
        sourceDoc = (*node).doc;
    } else if (*node).doc != sourceDoc {
        return -1;
    }
    if sourceDoc == destDoc {
        return -1;
    }
    match (*node).type_ as u32 {
        t if t == XML_ELEMENT_NODE as u32
            || t == XML_ATTRIBUTE_NODE as u32
            || t == XML_TEXT_NODE as u32
            || t == XML_CDATA_SECTION_NODE as u32
            || t == XML_ENTITY_REF_NODE as u32
            || t == XML_PI_NODE as u32
            || t == XML_COMMENT_NODE as u32 => {}
        t if t == XML_DOCUMENT_FRAG_NODE as u32 => return 2,
        _ => return 1,
    }
    // Unlink only if @node was not already added to @destParent.
    if !(*node).parent.is_null() && destParent != (*node).parent {
        unsafe { tree::unlink_node(node) };
    }
    if (*node).type_ == XML_ELEMENT_NODE as c_int {
        return unsafe { domwrap_adopt_branch(ctxt, node, destDoc, destParent) };
    } else if (*node).type_ == XML_ATTRIBUTE_NODE as c_int {
        return unsafe { domwrap_adopt_attr(ctxt, node as *mut _xmlAttr, destDoc, destParent) };
    } else {
        if (*node).doc != destDoc && unsafe { node_set_doc_impl(node, destDoc) } < 0 {
            ret = -1;
        }
    }
    ret
}

/// Clone a node and fix namespaces.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlDOMWrapCloneNode(xmlDOMWrapCtxt *ctxt, xmlDoc *sourceDoc,
///                         xmlNode *node, xmlNode **clonedNode,
///                         xmlDoc *destDoc, xmlNode *destParent,
///                         int deep, int options);
/// ```
///
/// # SAFETY
///
/// - `node`/`resNode`/`destDoc` must be valid pointers.
#[no_mangle]
pub unsafe extern "C" fn xmlDOMWrapCloneNode(
    ctxt: *mut _xmlDOMWrapCtxt,
    sourceDoc: *mut _xmlDoc,
    node: *mut _xmlNode,
    resNode: *mut *mut _xmlNode,
    destDoc: *mut _xmlDoc,
    destParent: *mut _xmlNode,
    deep: c_int,
    options: c_int,
) -> c_int {
    let _ = options;
    const ST_NORMAL: u8 = 0;
    const ST_INTO_CONTENT: u8 = 1;
    const ST_LEAVE: u8 = 2;
    let mut ret = 0;
    let mut cur = node;
    let mut clone_elem: *mut _xmlNode = ptr::null_mut();
    let mut ns_map: *mut NsMap = ptr::null_mut();
    let mut depth: c_int = -1;
    let mut parnsdone: c_int = 0;
    let ancestors_only: c_int = 0;
    let mut result_clone: *mut _xmlNode = ptr::null_mut();
    let mut clone: *mut _xmlNode = ptr::null_mut();
    let mut parent_clone: *mut _xmlNode = ptr::null_mut();
    let mut prev_clone: *mut _xmlNode = ptr::null_mut();
    let mut clone_ns: *mut _xmlNs = ptr::null_mut();
    let mut clone_ns_def_slot: *mut *mut _xmlNs = ptr::null_mut();
    let mut state: u8 = ST_NORMAL;

    if node.is_null()
        || resNode.is_null()
        || destDoc.is_null()
        || (!destParent.is_null() && (*destParent).doc != destDoc)
    {
        return -1;
    }
    if (*node).type_ != XML_ELEMENT_NODE as c_int {
        return 1;
    }
    if !(*node).doc.is_null() && !sourceDoc.is_null() && (*node).doc != sourceDoc {
        return -1;
    }
    let mut sourceDoc = sourceDoc;
    if sourceDoc.is_null() {
        sourceDoc = (*node).doc;
    }
    if sourceDoc.is_null() {
        return -1;
    }
    if !ctxt.is_null() {
        ns_map = (*ctxt).namespaceMap as *mut NsMap;
    }
    *resNode = ptr::null_mut();

    'outer: loop {
        if state == ST_NORMAL {
            if (*cur).doc != sourceDoc {
                // We'll assume XIncluded nodes if the doc differs.
                ret = -1;
                break;
            }
            let cur_type = (*cur).type_;
            match cur_type as u32 {
                t if t == XML_XINCLUDE_START as u32 || t == XML_XINCLUDE_END as u32 => {
                    ret = -1;
                    break;
                }
                t if t == XML_ELEMENT_NODE as u32
                    || t == XML_TEXT_NODE as u32
                    || t == XML_CDATA_SECTION_NODE as u32
                    || t == XML_COMMENT_NODE as u32
                    || t == XML_PI_NODE as u32
                    || t == XML_DOCUMENT_FRAG_NODE as u32
                    || t == XML_ENTITY_REF_NODE as u32 =>
                {
                    clone = unsafe { xmlMallocZero(size_of::<_xmlNode>()) } as *mut _xmlNode;
                    if clone.is_null() {
                        ret = -1;
                        break;
                    }
                    if !result_clone.is_null() {
                        (*clone).parent = parent_clone;
                        if !prev_clone.is_null() {
                            (*prev_clone).next = clone;
                            (*clone).prev = prev_clone;
                        } else {
                            (*parent_clone).children = clone;
                        }
                        (*parent_clone).last = clone;
                    } else {
                        result_clone = clone;
                    }
                }
                t if t == XML_ATTRIBUTE_NODE as u32 => {
                    clone = unsafe { xmlMallocZero(size_of::<_xmlAttr>()) } as *mut _xmlNode;
                    if clone.is_null() {
                        ret = -1;
                        break;
                    }
                    if !result_clone.is_null() {
                        (*clone).parent = parent_clone;
                        if !prev_clone.is_null() {
                            (*prev_clone).next = clone;
                            (*clone).prev = prev_clone;
                        } else {
                            (*parent_clone).properties = clone as *mut _xmlAttr;
                        }
                    } else {
                        result_clone = clone;
                    }
                }
                _ => {
                    ret = -1;
                    break;
                }
            }
            if ret == -1 {
                break;
            }
            (*clone).type_ = cur_type;
            (*clone).doc = destDoc;
            // Clone the name of the node if any.
            if !(*cur).name.is_null() {
                (*clone).name = unsafe { dup_str((*cur).name) };
                if (*clone).name.is_null() {
                    ret = -1;
                    break;
                }
            }
            match cur_type as u32 {
                t if t == XML_ELEMENT_NODE as u32 => {
                    clone_elem = clone;
                    depth += 1;
                    if !(*cur).nsDef.is_null() {
                        if parnsdone == 0 {
                            if !destParent.is_null()
                                && ctxt.is_null()
                                && unsafe { gather_in_scope_ns(&mut ns_map, destParent) } == -1
                            {
                                ret = -1;
                                break;
                            }
                            parnsdone = 1;
                        }
                        // Clone namespace declarations.
                        clone_ns_def_slot = core::ptr::addr_of_mut!((*clone).nsDef);
                        let mut ns = (*cur).nsDef;
                        loop {
                            clone_ns = unsafe { xmlMallocZero(size_of::<_xmlNs>()) } as *mut _xmlNs;
                            if clone_ns.is_null() {
                                ret = -1;
                                break;
                            }
                            (*clone_ns).type_ = XML_LOCAL_NAMESPACE as c_int;
                            if !(*ns).href.is_null() {
                                (*clone_ns).href = unsafe { dup_str((*ns).href) };
                                if (*clone_ns).href.is_null() {
                                    unsafe { free_ns_impl(clone_ns) };
                                    ret = -1;
                                    break;
                                }
                            }
                            if !(*ns).prefix.is_null() {
                                (*clone_ns).prefix = unsafe { dup_str((*ns).prefix) };
                                if (*clone_ns).prefix.is_null() {
                                    unsafe { free_ns_impl(clone_ns) };
                                    ret = -1;
                                    break;
                                }
                            }
                            *clone_ns_def_slot = clone_ns;
                            clone_ns_def_slot = core::ptr::addr_of_mut!((*clone_ns).next);
                            if ctxt.is_null() || (*ctxt).getNsForNodeFunc.is_none() {
                                if unsafe { ns_map_not_empty(ns_map) } {
                                    let mut mi = (*ns_map).first;
                                    while !mi.is_null() {
                                        if (*mi).depth >= XML_TREE_NSMAP_PARENT
                                            && (*mi).shadowDepth == -1
                                            && !(*mi).newNs.is_null()
                                            && unsafe {
                                                str_eq_or_ptr((*ns).prefix, (*(*mi).newNs).prefix)
                                            }
                                        {
                                            (*mi).shadowDepth = depth;
                                        }
                                        mi = (*mi).next;
                                    }
                                }
                                if unsafe { ns_map_add_item(&mut ns_map, -1, ns, clone_ns, depth) }
                                    .is_null()
                                {
                                    ret = -1;
                                    break;
                                }
                            }
                            if (*ns).next.is_null() {
                                break;
                            }
                            ns = (*ns).next;
                        }
                        if ret == -1 {
                            break;
                        }
                    }
                }
                t if t == XML_PI_NODE as u32
                    || t == XML_COMMENT_NODE as u32
                    || t == XML_TEXT_NODE as u32
                    || t == XML_CDATA_SECTION_NODE as u32 =>
                {
                    if !(*cur).content.is_null() {
                        (*clone).content = unsafe { dup_str((*cur).content) };
                        if (*clone).content.is_null() {
                            ret = -1;
                            break;
                        }
                    }
                    state = ST_LEAVE;
                    continue 'outer;
                }
                t if t == XML_ENTITY_REF_NODE as u32 => {
                    if sourceDoc != destDoc {
                        if !(*destDoc).intSubset.is_null() || !(*destDoc).extSubset.is_null() {
                            let ent = unsafe { tree::get_doc_entity(destDoc, (*cur).name) };
                            if !ent.is_null() {
                                (*clone).content = (*ent).content;
                                (*clone).children = ent as *mut _xmlNode;
                                (*clone).last = ent as *mut _xmlNode;
                            }
                        }
                    } else {
                        (*clone).content = (*cur).content;
                        (*clone).children = (*cur).children;
                        (*clone).last = (*cur).last;
                    }
                    state = ST_LEAVE;
                    continue 'outer;
                }
                _ => {
                    ret = -1;
                    break;
                }
            }
            if ret == -1 {
                break;
            }
            // ns-reference handling (element and attribute nodes).
            if !(*cur).ns.is_null() {
                if parnsdone == 0 {
                    if !destParent.is_null()
                        && ctxt.is_null()
                        && unsafe { gather_in_scope_ns(&mut ns_map, destParent) } == -1
                    {
                        ret = -1;
                        break;
                    }
                    parnsdone = 1;
                }
                let mut mapped = false;
                if unsafe { ns_map_not_empty(ns_map) } {
                    let mut mi = (*ns_map).first;
                    while !mi.is_null() {
                        if (*mi).shadowDepth == -1 && (*cur).ns == (*mi).oldNs {
                            (*clone).ns = (*mi).newNs;
                            mapped = true;
                            break;
                        }
                        mi = (*mi).next;
                    }
                }
                if !mapped {
                    if !ctxt.is_null() && (*ctxt).getNsForNodeFunc.is_some() {
                        let f = (*ctxt).getNsForNodeFunc.unwrap();
                        let ns = unsafe { f(ctxt, cur, (*(*cur).ns).href, (*(*cur).ns).prefix) };
                        unsafe {
                            ns_map_add_item(&mut ns_map, -1, (*cur).ns, ns, XML_TREE_NSMAP_CUSTOM)
                        };
                        (*clone).ns = ns;
                    } else {
                        let mut ns: *mut _xmlNs = ptr::null_mut();
                        if unsafe {
                            acquire_normalized_ns(
                                destDoc,
                                if destParent.is_null() {
                                    ptr::null_mut()
                                } else {
                                    clone_elem
                                },
                                (*cur).ns,
                                &mut ns,
                                &mut ns_map,
                                depth,
                                ancestors_only,
                                if (*cur).type_ == XML_ATTRIBUTE_NODE as c_int {
                                    1
                                } else {
                                    0
                                },
                            )
                        } == -1
                        {
                            ret = -1;
                            break;
                        }
                        (*clone).ns = ns;
                    }
                }
            }
            // Walk the element's attributes before descending into child-nodes.
            if (*cur).type_ == XML_ELEMENT_NODE as c_int && !(*cur).properties.is_null() {
                prev_clone = ptr::null_mut();
                parent_clone = clone;
                cur = (*cur).properties as *mut _xmlNode;
                continue 'outer; // state stays ST_NORMAL
            }
            state = ST_INTO_CONTENT;
            continue 'outer;
        } else if state == ST_INTO_CONTENT {
            if !(*cur).children.is_null()
                && (deep != 0 || (*cur).type_ == XML_ATTRIBUTE_NODE as c_int)
            {
                prev_clone = ptr::null_mut();
                parent_clone = clone;
                cur = (*cur).children;
                state = ST_NORMAL;
                continue 'outer;
            }
            state = ST_LEAVE;
        }
        // === leave_node ===
        if cur == node {
            break;
        }
        if (*cur).type_ == XML_ELEMENT_NODE as c_int
            || (*cur).type_ == XML_XINCLUDE_START as c_int
            || (*cur).type_ == XML_XINCLUDE_END as c_int
        {
            unsafe { ns_map_pop_depth(ns_map, depth) };
            depth -= 1;
        }
        if !(*cur).next.is_null() {
            prev_clone = clone;
            cur = (*cur).next;
            state = ST_NORMAL;
        } else if (*cur).type_ != XML_ATTRIBUTE_NODE as c_int {
            clone = (*clone).parent;
            if !clone.is_null() {
                parent_clone = (*clone).parent;
            }
            cur = (*cur).parent;
            if cur.is_null() {
                break;
            }
            // goto leave_node
            state = ST_LEAVE;
        } else {
            // This is for attributes only.
            clone = (*clone).parent;
            parent_clone = (*clone).parent;
            cur = (*cur).parent;
            if cur.is_null() {
                break;
            }
            // goto into_content
            state = ST_INTO_CONTENT;
        }
    }

    // Cleanup.
    if !ns_map.is_null() {
        if !ctxt.is_null() && (*ctxt).namespaceMap == ns_map as *mut c_void {
            // Just cleanup the map but don't free.
            if !(*ns_map).first.is_null() {
                if !(*ns_map).pool.is_null() {
                    (*(*ns_map).last).next = (*ns_map).pool;
                }
                (*ns_map).pool = (*ns_map).first;
                (*ns_map).first = ptr::null_mut();
            }
        } else {
            unsafe { ns_map_free(ns_map) };
        }
    }
    *resNode = result_clone;
    ret
}

// ═══════════════════════════════════════════════════════════════════════════════
// Entities tables
// ═══════════════════════════════════════════════════════════════════════════════

/// Create an empty entities hash table.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlEntitiesTable *xmlCreateEntitiesTable(void);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlCreateEntitiesTable() -> *mut c_void {
    hash::hash_create(0) as *mut c_void
}

unsafe extern "C" fn free_entity_wrapper(payload: *mut c_void, _name: *mut xmlChar) {
    if !payload.is_null() {
        unsafe { entities::free_entity(payload as *mut _xmlEntity) };
    }
}

/// Free an entities hash table.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlFreeEntitiesTable(xmlEntitiesTable *table);
/// ```
///
/// # SAFETY
///
/// - `table` must be a valid entities table or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlFreeEntitiesTable(table: *mut c_void) {
    unsafe { hash::hash_free(table as *mut hash::HashTable, Some(free_entity_wrapper)) };
}

/// Create a new input stream based on an xmlEntity.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlParserInput *xmlNewEntityInputStream(xmlParserCtxt *ctxt, xmlEntity *entity);
/// ```
///
/// # SAFETY
///
/// - `ctxt`/`entity` must be valid pointers or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlNewEntityInputStream(
    ctxt: *mut _xmlParserCtxt,
    entity: *mut _xmlEntity,
) -> *mut _xmlParserInput {
    if ctxt.is_null() || entity.is_null() {
        return ptr::null_mut();
    }
    if !(*entity).content.is_null() {
        let input = unsafe { xmlMallocZero(size_of::<_xmlParserInput>()) } as *mut _xmlParserInput;
        if input.is_null() {
            return ptr::null_mut();
        }
        let len = unsafe { str_len((*entity).content) } as usize;
        (*input).line = 1;
        (*input).col = 1;
        (*input).base = (*entity).content;
        (*input).cur = (*entity).content;
        (*input).end = (*entity).content.add(len);
        (*input).length = len as c_int;
        (*input).entity = entity;
        (*input).buf = ptr::null_mut();
        return input;
    }
    // External entities require the resource loader (xmlLoadResource); not
    // ported here — upstream returns NULL in this case as well when the
    // resource cannot be loaded.
    if !(*entity).URI.is_null() {
        return ptr::null_mut();
    }
    ptr::null_mut()
}

// ═══════════════════════════════════════════════════════════════════════════════
// valid.h: enumerations and content models
// ═══════════════════════════════════════════════════════════════════════════════

/// Create an enumeration value.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlEnumeration *xmlCreateEnumeration(const xmlChar *name);
/// ```
///
/// # SAFETY
///
/// - `name` must be a valid null-terminated string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlCreateEnumeration(name: *const xmlChar) -> *mut _xmlEnumeration {
    let ret = unsafe { xmlMallocZero(size_of::<_xmlEnumeration>()) } as *mut _xmlEnumeration;
    if ret.is_null() {
        return ptr::null_mut();
    }
    if !name.is_null() {
        (*ret).name = unsafe { dup_str(name) };
        if (*ret).name.is_null() {
            unsafe { xmlFreeImpl(ret as *mut c_void) };
            return ptr::null_mut();
        }
    }
    ret
}

/// Free an enumeration list.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlFreeEnumeration(xmlEnumeration *cur);
/// ```
///
/// # SAFETY
///
/// - `cur` must be a valid enumeration or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlFreeEnumeration(cur: *mut _xmlEnumeration) {
    let mut c = cur;
    while !c.is_null() {
        let next = (*c).next;
        if !(*c).name.is_null() {
            unsafe { xmlFreeImpl((*c).name as *mut c_void) };
        }
        unsafe { xmlFreeImpl(c as *mut c_void) };
        c = next;
    }
}

/// Free a content model tree (iterative; shared by xmlFreeDocElementContent).
unsafe fn free_elem_content_internal(cur: *mut _xmlElementContent) {
    if cur.is_null() {
        return;
    }
    let mut depth: usize = 0;
    let mut cur = cur;
    loop {
        while !(*cur).c1.is_null() || !(*cur).c2.is_null() {
            cur = if !(*cur).c1.is_null() {
                (*cur).c1
            } else {
                (*cur).c2
            };
            depth += 1;
        }
        if !(*cur).name.is_null() {
            unsafe { xmlFreeImpl((*cur).name as *mut c_void) };
        }
        if !(*cur).prefix.is_null() {
            unsafe { xmlFreeImpl((*cur).prefix as *mut c_void) };
        }
        let parent = (*cur).parent;
        if depth == 0 || parent.is_null() {
            unsafe { xmlFreeImpl(cur as *mut c_void) };
            break;
        }
        if cur == (*parent).c1 {
            (*parent).c1 = ptr::null_mut();
        } else {
            (*parent).c2 = ptr::null_mut();
        }
        unsafe { xmlFreeImpl(cur as *mut c_void) };
        if !(*parent).c2.is_null() {
            cur = (*parent).c2;
        } else {
            depth -= 1;
            cur = parent;
        }
    }
}

/// Free an element content model.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlFreeDocElementContent(xmlDoc *doc, xmlElementContent *cur);
/// ```
///
/// # SAFETY
///
/// - `cur` must be a valid content model or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlFreeDocElementContent(doc: *mut _xmlDoc, cur: *mut _xmlElementContent) {
    let _ = doc;
    unsafe { free_elem_content_internal(cur) };
}

// ═══════════════════════════════════════════════════════════════════════════════
// valid.h: declaration tables
// ═══════════════════════════════════════════════════════════════════════════════

unsafe extern "C" fn free_attribute_table_entry(payload: *mut c_void, _name: *mut xmlChar) {
    if !payload.is_null() {
        unsafe { crate::xml::dtd::free_attribute(payload as *mut _xmlAttribute) };
    }
}

/// Free an attribute declaration table.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlFreeAttributeTable(xmlAttributeTable *table);
/// ```
///
/// # SAFETY
///
/// - `table` must be a valid attribute table or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlFreeAttributeTable(table: *mut c_void) {
    unsafe {
        hash::hash_free(
            table as *mut hash::HashTable,
            Some(free_attribute_table_entry),
        )
    };
}

unsafe extern "C" fn free_element_table_entry(payload: *mut c_void, _name: *mut xmlChar) {
    if !payload.is_null() {
        unsafe { crate::xml::dtd::free_element(payload as *mut _xmlElement) };
    }
}

/// Free an element declaration table.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlFreeElementTable(xmlElementTable *table);
/// ```
///
/// # SAFETY
///
/// - `table` must be a valid element table or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlFreeElementTable(table: *mut c_void) {
    unsafe {
        hash::hash_free(
            table as *mut hash::HashTable,
            Some(free_element_table_entry),
        )
    };
}

unsafe extern "C" fn free_notation_table_entry(payload: *mut c_void, _name: *mut xmlChar) {
    if !payload.is_null() {
        unsafe { crate::xml::dtd::free_notation(payload as *mut _xmlNotation) };
    }
}

/// Free a notation declaration table.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlFreeNotationTable(xmlNotationTable *table);
/// ```
///
/// # SAFETY
///
/// - `table` must be a valid notation table or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlFreeNotationTable(table: *mut c_void) {
    unsafe {
        hash::hash_free(
            table as *mut hash::HashTable,
            Some(free_notation_table_entry),
        )
    };
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlwriter.h
// ═══════════════════════════════════════════════════════════════════════════════

unsafe extern "C" fn writer_push_write_cb(
    ctx: *mut c_void,
    buffer: *const c_char,
    len: c_int,
) -> c_int {
    let ctxt = ctx as *mut _xmlParserCtxt;
    if ctxt.is_null() || buffer.is_null() {
        return -1;
    }
    let rc = unsafe { crate::abi::exports_xml2::xmlParseChunk(ctxt, buffer, len, 0) };
    if rc != 0 {
        return -1;
    }
    len
}

unsafe extern "C" fn writer_push_close_cb(ctx: *mut c_void) -> c_int {
    let ctxt = ctx as *mut _xmlParserCtxt;
    if ctxt.is_null() {
        return -1;
    }
    let rc = unsafe { crate::abi::exports_xml2::xmlParseChunk(ctxt, ptr::null(), 0, 1) };
    if rc != 0 {
        return -1;
    }
    0
}

/// Create a new xmlTextWriter structure with `ctxt` as output.
///
/// NOTE: the `ctxt` context will be freed with the resulting writer
/// (if the call succeeds).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlTextWriter *xmlNewTextWriterPushParser(xmlParserCtxt *ctxt, int compression);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be a valid parser context.
#[no_mangle]
pub unsafe extern "C" fn xmlNewTextWriterPushParser(
    ctxt: *mut _xmlParserCtxt,
    compression: c_int,
) -> *mut crate::xml::writer::XmlTextWriter {
    let _ = compression;
    if ctxt.is_null() {
        return ptr::null_mut();
    }
    let out = crate::xml::io::output_buffer_create_io(
        Some(writer_push_write_cb),
        Some(writer_push_close_cb),
        ctxt as *mut c_void,
        ptr::null_mut(),
    );
    if out.is_null() {
        return ptr::null_mut();
    }
    let ret = crate::xml::writer::xmlNewTextWriter(out);
    if ret.is_null() {
        crate::xml::io::output_buffer_close(out);
        return ptr::null_mut();
    }
    ret
}

// ═══════════════════════════════════════════════════════════════════════════════
// Legacy
// ═══════════════════════════════════════════════════════════════════════════════

/// Creation of a Namespace, the old way using PI and without scoping.
///
/// DEPRECATED: the functionality was removed upstream; this returns NULL.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNsPtr xmlNewGlobalNs(xmlDocPtr doc, const xmlChar *href, const xmlChar *prefix);
/// ```
///
/// # SAFETY
///
/// - All arguments are unused.
#[no_mangle]
pub const unsafe extern "C" fn xmlNewGlobalNs(
    _doc: *mut _xmlDoc,
    _href: *const xmlChar,
    _prefix: *const xmlChar,
) -> *mut _xmlNs {
    ptr::null_mut()
}
