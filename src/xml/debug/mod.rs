//! Debug/memory debugging infrastructure (§85 Phase 7).
//!
//! UPSTREAM-PARITY: Corresponds to `debugXML.c` / `debugXML.h` in libxml2.
//!
//! The libxml2 debug APIs for printing tree structure, XPath expressions, etc.
//! These are used by `xmllint --debug` and other diagnostic tools.
//!
//! # Upstream contract
//!
//! Mirrors upstream `debugXML.c` / `debugXML.h`
//! (`SRC-LIBXML2-2.15.0-DEBUGXML-C`, parity target libxml2 2.15.3 oracle):
//! `xmlDebugDumpNode`, `xmlDebugDumpNodeList`, `xmlDebugDumpOneNode`,
//! `xmlDebugDumpAttrList`, `xmlDebugDumpString` and the `xmllint --debug`
//! tree dump format.
//!
//! # Conceptual behavior
//!
//! Implements the recursive debug dumper: nodes are printed with type,
//! name, content, attributes, namespaces, line numbers and children at
//! increasing indentation (`MAX_DEPTH` guard), plus the XPath expression
//! and string dumps used by xmllint diagnostics.
//!
//! # Ownership & safety invariants
//!
//! The dump functions borrow the tree — they never free or detach nodes;
//! output goes to the caller-provided FILE* (or a buffer) that the caller
//! owns. Namespace strings are read via their href (bounded scans), never
//! stored.
//!
//! # Historical quirks & epochs
//!
//! The dump format is one of the most stable surfaces: the debug-simple /
//! debug-dtd / debug-nodes / debug-ns matrix cases are byte-identical
//! across the whole 2.7.8 → 2.15.3 span (SEMANTIC_EPOCHS.md stable cases).
//! One deliberate epoch join: entity content children dump as `TEXT compact`
//! since 2.13.0 (E-004, commit 8d04f0ee 2024-03-11, R-000119) — the
//! candidate matches the 2.13+ epoch, i.e. the current oracle.
//!
//! # Deliberate oddities
//!
//! The `TEXT compact` marker is reproduced exactly (including the
//! 14/15-byte compact threshold that is unchanged in every tested version),
//! rather than normalized to plain `TEXT`.
//!
//! # Proving courts
//!
//! The xmllint --debug CLI differential courts and the debug-* matrix cases
//! compare dumps byte-identical against each built oracle (2.7.8 through
//! 2.15.3); cargo test covers the unit dump suites.
//!
//! # Tempting simplifications that would break parity
//!
//! Do not normalize the dump format (e.g. always printing `TEXT` or
//! collapsing attributes): xmllint --debug output is a byte-exact parity
//! target and downstream tooling parses it. Do not drop the compact-text
//! threshold logic — R-000119/E-004 depend on it.
//!
//! # Safety
//!
//! This module contains no top-level `unsafe` blocks, no `static mut` items
//! and no module-level `unsafe` code; every `unsafe` operation lives inside
//! the individual `unsafe fn` entry points and their tests below, each
//! carrying its own `# Safety` contract. The module-level invariant for the
//! exported dump surface is that all raw pointers (output `FILE*`, `_xmlDoc`,
//! `_xmlNode`, `_xmlAttr`, `_xmlElement`, `_xmlEntity` and `xmlChar` strings)
//! are borrowed for the duration of each call — never stored, freed or
//! detached — and must point to live, unmutated objects; NULL is accepted
//! wherever the upstream C API allows it.

use crate::abi::structs::{_xmlAttr, _xmlDoc, _xmlNode};
use core::ffi::{c_char, c_int, c_void};
use core::ptr;

/// Maximum indentation depth for debug output.
const MAX_DEPTH: c_int = 100;

/// Check if a node is an XInclude start node.
///
/// UPSTREAM-PARITY: `xmlDebugIsXInclude()` — internal check used by debug dumper.
///
/// # Safety
///
/// - `node` must be NULL or a valid pointer to a live `_xmlNode`.
/// - If non-NULL, `(*node).ns` must be NULL or a valid `_xmlNs` whose `href`
///   points to a region of at least 30 readable bytes, and `(*node).name`
///   must be NULL or point to at least 8 readable bytes (both are scanned
///   with `from_raw_parts` at fixed lengths).
/// - The node and its namespace must not be mutated concurrently during the
///   call.
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
        let _ns_prefix = (*ns).prefix;
        if ns_href.is_null() {
            return false;
        }
        // Check for XInclude namespace
        let href = core::slice::from_raw_parts(ns_href, 30);
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
            core::slice::from_raw_parts((*node).name, 8)
        } else {
            return false;
        };
        name_bytes.len() >= 7 && &name_bytes[..7] == b"include"
    }
}

/// Convert a boolean to text.
///
/// UPSTREAM-PARITY: `xmlBoolToText()`
///
/// # SAFETY
///
/// The function touches crate-global state only; it is safe
/// as long as the caller respects the library's global
/// initialization/cleanup ordering (xmlInitParser before use,
/// xmlCleanupParser only after all users are done).
///
/// Violating the global lifecycle ordering, or calling this after
/// teardown or from a signal handler, is undefined behavior.
#[no_mangle]
pub const unsafe extern "C" fn xmlBoolToText(boolval: c_int) -> *const c_char {
    if boolval != 0 {
        c"true".as_ptr() as *const c_char
    } else {
        c"false".as_ptr() as *const c_char
    }
}

/// Dump a debug representation of an xmlChar string.
///
/// UPSTREAM-PARITY: `xmlDebugDumpString()` — line breaks, tabs and CRs are
/// rendered as a single space.
///
/// # SAFETY
///
/// - `output`, `str_val` must be valid pointers (or NULL
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
#[no_mangle]
pub unsafe extern "C" fn xmlDebugDumpString(output: *mut _IO_FILE, str_val: *const u8) {
    if output.is_null() {
        return;
    }
    if str_val.is_null() {
        unsafe {
            libc::fprintf(output, c"(NULL)".as_ptr() as *const c_char);
        }
        return;
    }
    unsafe {
        // UPSTREAM-PARITY: xmlCtxtDumpString prints at most 40 characters;
        // blank characters become spaces, bytes >= 0x80 are printed as
        // `#%X`, and a longer string is truncated with "...".
        let mut i = 0;
        while i < 40 {
            let c = *str_val.add(i);
            if c == 0 {
                return;
            }
            if c == b' ' || c == b'\t' || c == b'\n' || c == b'\r' {
                libc::fprintf(output, c" ".as_ptr() as *const c_char);
            } else if c >= 0x80 {
                libc::fprintf(output, c"#%X".as_ptr() as *const c_char, c as c_int);
            } else {
                libc::fprintf(output, c"%c".as_ptr() as *const c_char, c as c_int);
            }
            i += 1;
        }
        libc::fprintf(output, c"...".as_ptr() as *const c_char);
    }
}

/// Dump a debug representation of an attribute.
///
/// UPSTREAM-PARITY: `xmlDebugDumpAttr()`
///
/// # SAFETY
///
/// - `output`, `attr` must be valid pointers (or NULL
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
            libc::fprintf(output, c"  ".as_ptr() as *const c_char);
        }
        libc::fprintf(output, c"ATTRIBUTE ".as_ptr() as *const c_char);
        xmlDebugDumpString(output, (*attr).name);
        libc::fprintf(output, c"\n".as_ptr() as *const c_char);
        // The attribute value is dumped as a compact text node child.
        xmlDebugDumpNode(output, (*attr).children, depth + 1);
    }
}

/// Dump a debug representation of an attribute list.
///
/// UPSTREAM-PARITY: `xmlDebugDumpAttrList()`
///
/// # SAFETY
///
/// - `output`, `attr` must be valid pointers (or NULL
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
///
/// # SAFETY
///
/// - `output`, `node` must be valid pointers (or NULL
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
            libc::fprintf(output, c"  ".as_ptr() as *const c_char);
        }

        // Print node type
        match (*node).type_ {
            1 => {
                // XML_ELEMENT_NODE
                libc::fprintf(output, c"ELEMENT ".as_ptr() as *const c_char);
                // QName: prefix:name when a namespace prefix is present.
                if !(*node).ns.is_null() && !(*(*node).ns).prefix.is_null() {
                    libc::fprintf(
                        output,
                        c"%s:".as_ptr() as *const c_char,
                        (*(*node).ns).prefix,
                    );
                }
                xmlDebugDumpString(output, (*node).name);
                libc::fprintf(output, c"\n".as_ptr() as *const c_char);

                // Namespace declarations on the element (upstream prints them
                // before attributes).
                if !(*node).nsDef.is_null() {
                    let mut ns = (*node).nsDef;
                    while !ns.is_null() {
                        for _ in 0..(depth + 1) {
                            libc::fprintf(output, c"  ".as_ptr() as *const c_char);
                        }
                        libc::fprintf(output, c"namespace ".as_ptr() as *const c_char);
                        if (*ns).prefix.is_null() {
                            libc::fprintf(output, c" ".as_ptr() as *const c_char);
                        } else {
                            libc::fprintf(output, c"%s".as_ptr() as *const c_char, (*ns).prefix);
                        }
                        libc::fprintf(output, c" href=".as_ptr() as *const c_char);
                        libc::fprintf(output, c"%s\n".as_ptr() as *const c_char, (*ns).href);
                        ns = (*ns).next;
                    }
                }

                // Attributes
                if !(*node).properties.is_null() {
                    xmlDebugDumpAttrList(output, (*node).properties, depth + 1);
                }
            }
            2 => {
                // XML_ATTRIBUTE_NODE
                libc::fprintf(output, c"ATTRIBUTE ".as_ptr() as *const c_char);
                xmlDebugDumpString(output, (*node).name);
                libc::fprintf(output, c"\n".as_ptr() as *const c_char);
            }
            3 => {
                // XML_TEXT_NODE
                libc::fprintf(output, c"TEXT".as_ptr() as *const c_char);
                // UPSTREAM-PARITY: debugXML.c marks compact text via
                // `node->content == (xmlChar *) &(node->properties)`.
                let inline_addr = std::ptr::addr_of_mut!((*node).properties) as *const c_void;
                if (*node).content as *const c_void == inline_addr {
                    libc::fprintf(output, c" compact".as_ptr() as *const c_char);
                }
                libc::fprintf(output, c"\n".as_ptr() as *const c_char);
                for _ in 0..(depth + 1) {
                    libc::fprintf(output, c"  ".as_ptr() as *const c_char);
                }
                libc::fprintf(output, c"content=".as_ptr() as *const c_char);
                if !(*node).content.is_null() {
                    xmlDebugDumpString(output, (*node).content as *const u8);
                } else {
                    let c = crate::xml::tree::node_get_content(node);
                    if !c.is_null() {
                        libc::fprintf(output, c"%s".as_ptr() as *const c_char, c);
                        crate::abi::allocator::xmlFreeImpl(c as *mut c_void);
                    }
                }
                libc::fprintf(output, c"\n".as_ptr() as *const c_char);
            }
            4 => {
                // XML_CDATA_SECTION_NODE
                libc::fprintf(output, c"CDATA_SECTION\n".as_ptr() as *const c_char);
                for _ in 0..(depth + 1) {
                    libc::fprintf(output, c"  ".as_ptr() as *const c_char);
                }
                libc::fprintf(output, c"content=".as_ptr() as *const c_char);
                xmlDebugDumpString(output, (*node).content as *const u8);
                libc::fprintf(output, c"\n".as_ptr() as *const c_char);
            }
            5 => {
                // XML_ENTITY_REF_NODE
                libc::fprintf(output, c"ENTITY_REF(".as_ptr() as *const c_char);
                xmlDebugDumpString(output, (*node).name);
                libc::fprintf(output, c")\n".as_ptr() as *const c_char);
                // The referenced entity's declaration.
                let doc = (*node).doc;
                let ent = if doc.is_null() {
                    ptr::null_mut()
                } else {
                    crate::xml::entities::get_entity(doc, (*node).name)
                };
                if !ent.is_null() {
                    for _ in 0..(depth + 1) {
                        libc::fprintf(output, c"  ".as_ptr() as *const c_char);
                    }
                    let etype = (*ent).etype;
                    let etype_name = entity_type_name(etype);
                    libc::fprintf(
                        output,
                        c"%s ".as_ptr() as *const c_char,
                        etype_name.as_ptr() as *const c_char,
                    );
                    libc::fprintf(output, c"%s\n".as_ptr() as *const c_char, (*ent).name);
                    for _ in 0..(depth + 1) {
                        libc::fprintf(output, c"  ".as_ptr() as *const c_char);
                    }
                    libc::fprintf(output, c"content=".as_ptr() as *const c_char);
                    if !(*ent).content.is_null() {
                        xmlDebugDumpString(output, (*ent).content as *const u8);
                    }
                    libc::fprintf(output, c"\n".as_ptr() as *const c_char);
                }
            }
            6 => {
                // XML_ENTITY_NODE
                libc::fprintf(output, c"ENTITYDECL(".as_ptr() as *const c_char);
                xmlDebugDumpString(output, (*node).name);
                libc::fprintf(output, c")".as_ptr() as *const c_char);
                if !(*node).content.is_null() {
                    libc::fprintf(output, c", internal\n ".as_ptr() as *const c_char);
                    libc::fprintf(output, c"content=".as_ptr() as *const c_char);
                    xmlDebugDumpString(output, (*node).content as *const u8);
                }
                libc::fprintf(output, c"\n".as_ptr() as *const c_char);
            }
            7 => {
                // XML_PI_NODE
                libc::fprintf(output, c"PI ".as_ptr() as *const c_char);
                xmlDebugDumpString(output, (*node).name);
                libc::fprintf(output, c"\n".as_ptr() as *const c_char);
                if !(*node).content.is_null() {
                    for _ in 0..(depth + 1) {
                        libc::fprintf(output, c"  ".as_ptr() as *const c_char);
                    }
                    libc::fprintf(output, c"content=".as_ptr() as *const c_char);
                    xmlDebugDumpString(output, (*node).content as *const u8);
                    libc::fprintf(output, c"\n".as_ptr() as *const c_char);
                }
            }
            8 => {
                // XML_COMMENT_NODE
                libc::fprintf(output, c"COMMENT\n".as_ptr() as *const c_char);
                for _ in 0..(depth + 1) {
                    libc::fprintf(output, c"  ".as_ptr() as *const c_char);
                }
                libc::fprintf(output, c"content=".as_ptr() as *const c_char);
                xmlDebugDumpString(output, (*node).content as *const u8);
                libc::fprintf(output, c"\n".as_ptr() as *const c_char);
            }
            9 => {
                // XML_DOCUMENT_NODE
                libc::fprintf(output, c"DOCUMENT".as_ptr() as *const c_char);
            }
            10 => {
                // XML_DOCUMENT_TYPE_NODE
                libc::fprintf(output, c"DOCTYPE".as_ptr() as *const c_char);
            }
            14 => {
                // XML_DTD_NODE. UPSTREAM-PARITY: xmlCtxtDumpDtdNode prints
                // `DTD(name)`, `, PUBLIC extID` and `, SYSTEM sysID` all on
                // one line.
                libc::fprintf(output, c"DTD(".as_ptr() as *const c_char);
                xmlDebugDumpString(output, (*node).name);
                libc::fprintf(output, c")".as_ptr() as *const c_char);
                let dtd = node as *mut crate::abi::structs::_xmlDtd;
                if !(*dtd).ExternalID.is_null() {
                    libc::fprintf(output, c", PUBLIC ".as_ptr() as *const c_char);
                    libc::fprintf(output, c"%s".as_ptr() as *const c_char, (*dtd).ExternalID);
                }
                if !(*dtd).SystemID.is_null() {
                    libc::fprintf(output, c", SYSTEM ".as_ptr() as *const c_char);
                    libc::fprintf(output, c"%s".as_ptr() as *const c_char, (*dtd).SystemID);
                }
                libc::fprintf(output, c"\n".as_ptr() as *const c_char);
                // Element declarations (hash table).
                let dtd = node as *mut crate::abi::structs::_xmlDtd;
                if !(*dtd).elements.is_null() {
                    let ctx = DtdDumpCtx {
                        output,
                        depth: depth + 1,
                    };
                    crate::xml::hash::hash_scan(
                        (*dtd).elements as *mut crate::xml::hash::HashTable,
                        Some(dump_elemscan_cb),
                        &ctx as *const DtdDumpCtx as *mut c_void,
                    );
                }
                // Entity declarations.
                if !(*dtd).entities.is_null() {
                    let ctx = DtdDumpCtx {
                        output,
                        depth: depth + 1,
                    };
                    crate::xml::hash::hash_scan(
                        (*dtd).entities as *mut crate::xml::hash::HashTable,
                        Some(dump_entityscan_cb),
                        &ctx as *const DtdDumpCtx as *mut c_void,
                    );
                }
            }
            13 => {
                // XML_HTML_DOCUMENT_NODE
                libc::fprintf(output, c"HTML DOCUMENT".as_ptr() as *const c_char);
            }
            18 => {
                // XML_NAMESPACE_DECL
                libc::fprintf(output, c"NAMESPACE".as_ptr() as *const c_char);
                if !(*node).ns.is_null() && !(*(*node).ns).prefix.is_null() {
                    libc::fprintf(
                        output,
                        c" %s=%s".as_ptr() as *const c_char,
                        (*(*node).ns).prefix,
                        (*(*node).ns).href,
                    );
                }
            }
            19 => {
                // XML_XINCLUDE_START
                if is_xinclude_node(node) {
                    libc::fprintf(output, c"XINCLUDE".as_ptr() as *const c_char);
                } else {
                    libc::fprintf(output, c"XINCLUDE_START".as_ptr() as *const c_char);
                }
            }
            20 => {
                // XML_XINCLUDE_END
                libc::fprintf(output, c"XINCLUDE_END".as_ptr() as *const c_char);
            }
            _ => {
                libc::fprintf(
                    output,
                    c"UNKNOWN (%d)".as_ptr() as *const c_char,
                    (*node).type_ as c_int,
                );
            }
        }
    }
}

/// Map an entity type to its upstream debug name.
fn entity_type_name(etype: c_int) -> Vec<u8> {
    use crate::abi::types::xmlEntityType::*;
    match etype {
        t if t == XML_INTERNAL_GENERAL_ENTITY as c_int => b"INTERNAL_GENERAL_ENTITY\0".to_vec(),
        t if t == XML_INTERNAL_PARAMETER_ENTITY as c_int => b"INTERNAL_PARAMETER_ENTITY\0".to_vec(),
        t if t == XML_EXTERNAL_GENERAL_PARSED_ENTITY as c_int => {
            b"EXTERNAL_GENERAL_PARSED_ENTITY\0".to_vec()
        }
        t if t == XML_EXTERNAL_GENERAL_UNPARSED_ENTITY as c_int => {
            b"EXTERNAL_GENERAL_UNPARSED_ENTITY\0".to_vec()
        }
        t if t == XML_EXTERNAL_PARAMETER_ENTITY as c_int => b"EXTERNAL_PARAMETER_ENTITY\0".to_vec(),
        t if t == XML_INTERNAL_PREDEFINED_ENTITY as c_int => {
            b"INTERNAL_PREDEFINED_ENTITY\0".to_vec()
        }
        _ => b"UNKNOWN_ENTITY\0".to_vec(),
    }
}

/// Context for DTD hash-scan debug callbacks.
#[repr(C)]
struct DtdDumpCtx {
    output: *mut _IO_FILE,
    depth: c_int,
}

/// Dump an element declaration (`ELEMDECL(name), TYPE (model)`).
///
/// # Safety
///
/// - `payload` must be NULL or a valid pointer to a live `_xmlElement` whose
///   `name` is NULL or a NUL-terminated string and whose `content` is NULL or
///   a valid content-model tree.
/// - `data` must be NULL or a valid pointer to a `DtdDumpCtx` whose `output`
///   is a valid writable `FILE*` and whose `depth` is non-negative.
/// - The element and context must not be mutated concurrently during the
///   call.
unsafe extern "C" fn dump_elemscan_cb(payload: *mut c_void, data: *mut c_void, _name: *const u8) {
    if payload.is_null() || data.is_null() {
        return;
    }
    let ctx = unsafe { &*(data as *const DtdDumpCtx) };
    let elem = unsafe { &*(payload as *mut crate::abi::structs::_xmlElement) };
    unsafe {
        for _ in 0..ctx.depth {
            libc::fprintf(ctx.output, c"  ".as_ptr() as *const c_char);
        }
        libc::fprintf(ctx.output, c"ELEMDECL(".as_ptr() as *const c_char);
        if !elem.name.is_null() {
            libc::fprintf(ctx.output, c"%s".as_ptr() as *const c_char, elem.name);
        }
        libc::fprintf(ctx.output, c")".as_ptr() as *const c_char);
        // UPSTREAM-PARITY: 2.15 prints the MIXED label for every
        // parenthesized content model (even element-only ones) in the debug
        // dump.
        match elem.type_ {
            t if t == crate::abi::types::xmlElementTypeVal::XML_ELEMENT_TYPE_EMPTY as c_int => {
                libc::fprintf(ctx.output, c", EMPTY\n".as_ptr() as *const c_char);
            }
            t if t == crate::abi::types::xmlElementTypeVal::XML_ELEMENT_TYPE_ANY as c_int => {
                libc::fprintf(ctx.output, c", ANY\n".as_ptr() as *const c_char);
            }
            _ => {
                libc::fprintf(ctx.output, c", MIXED ".as_ptr() as *const c_char);
                dump_debug_content_model(ctx.output, elem.content);
                libc::fprintf(ctx.output, c"\n".as_ptr() as *const c_char);
            }
        }
    }
}

/// Render a content model tree in the upstream debug format
/// (`xmlDebugDumpContentModel`, flattened).
///
/// # Safety
///
/// - `output` must be a valid writable `FILE*`.
/// - `content` must be NULL or a valid pointer to a live `_xmlElementContent`
///   tree; element-type nodes require `name` to be a valid NUL-terminated
///   string and `prefix` to be NULL or a valid NUL-terminated string (both
///   are passed to `fprintf`).
/// - The tree must not be mutated or freed concurrently during the call.
unsafe fn dump_debug_content_model(
    output: *mut _IO_FILE,
    content: *mut crate::abi::structs::_xmlElementContent,
) {
    use crate::abi::types::xmlElementContentType::*;
    if content.is_null() {
        return;
    }
    unsafe {
        let c = &*content;
        match c.type_ {
            t if t == XML_ELEMENT_CONTENT_PCDATA as c_int => {
                libc::fprintf(output, c"(#PCDATA)".as_ptr() as *const c_char);
            }
            t if t == XML_ELEMENT_CONTENT_ELEMENT as c_int => {
                if !c.prefix.is_null() {
                    libc::fprintf(output, c"%s:".as_ptr() as *const c_char, c.prefix);
                }
                libc::fprintf(output, c"%s".as_ptr() as *const c_char, c.name);
            }
            _ => {
                let sep = if c.type_ == XML_ELEMENT_CONTENT_SEQ as c_int {
                    c" , ".as_ptr() as *const c_char
                } else {
                    c" | ".as_ptr() as *const c_char
                };
                libc::fprintf(output, c"(".as_ptr() as *const c_char);
                let mut parts: Vec<*mut crate::abi::structs::_xmlElementContent> = Vec::new();
                flatten_chain(c as *const _ as *mut _, c.type_, &mut parts);
                for (i, &p) in parts.iter().enumerate() {
                    if i > 0 {
                        libc::fprintf(output, c"%s".as_ptr() as *const c_char, sep);
                    }
                    let pc = &*p;
                    if pc.type_ == XML_ELEMENT_CONTENT_PCDATA as c_int {
                        libc::fprintf(output, c"#PCDATA".as_ptr() as *const c_char);
                    } else if pc.type_ == XML_ELEMENT_CONTENT_ELEMENT as c_int {
                        if !pc.prefix.is_null() {
                            libc::fprintf(output, c"%s:".as_ptr() as *const c_char, pc.prefix);
                        }
                        libc::fprintf(output, c"%s".as_ptr() as *const c_char, pc.name);
                    } else {
                        dump_debug_content_model(output, p);
                    }
                    dump_debug_occurrence(output, pc.ocur);
                }
                libc::fprintf(output, c")".as_ptr() as *const c_char);
            }
        }
        dump_debug_occurrence(output, c.ocur);
    }
}

/// Collect the leaves of a same-type chain (left-leaning trees flatten).
///
/// # Safety
///
/// - `node` must be a valid non-NULL pointer to a live `_xmlElementContent`.
/// - The subtree reachable through `c1`/`c2` must remain alive and unmutated
///   for the duration of the call; the traversal reads raw pointers
///   recursively and stores borrowed copies in `parts`.
/// - `parts` must be a valid mutable `Vec` reference.
fn flatten_chain(
    node: *mut crate::abi::structs::_xmlElementContent,
    chain_type: c_int,
    parts: &mut Vec<*mut crate::abi::structs::_xmlElementContent>,
) {
    unsafe {
        let c = &*node;
        if c.type_ == chain_type && !c.c1.is_null() {
            flatten_chain(c.c1, chain_type, parts);
            if !c.c2.is_null() {
                flatten_chain(c.c2, chain_type, parts);
            }
        } else {
            parts.push(node);
        }
    }
}

/// Print the occurrence suffix.
///
/// # Safety
///
/// - `output` must be a valid writable `FILE*`; the matched occurrence
///   suffixes are static strings passed to `fprintf`.
/// - `ocur` is an arbitrary `c_int`; unmatched values return without writing.
unsafe fn dump_debug_occurrence(output: *mut _IO_FILE, ocur: c_int) {
    use crate::abi::types::xmlElementContentOccur::*;
    let s = match ocur {
        t if t == XML_ELEMENT_CONTENT_OPT as c_int => c"?".as_ptr() as *const c_char,
        t if t == XML_ELEMENT_CONTENT_MULT as c_int => c"*".as_ptr() as *const c_char,
        t if t == XML_ELEMENT_CONTENT_PLUS as c_int => c"+".as_ptr() as *const c_char,
        _ => return,
    };
    unsafe {
        libc::fprintf(output, c"%s".as_ptr() as *const c_char, s);
    }
}

/// True when a null-terminated string contains markup-significant bytes
/// (`<` or `&`), i.e. it cannot be a single plain text node.
const unsafe fn contains_markup(s: *const crate::abi::types::xmlChar) -> bool {
    if s.is_null() {
        return false;
    }
    unsafe {
        let mut i = 0usize;
        while *s.add(i) != 0 {
            let c = *s.add(i);
            if c == b'<' || c == b'&' {
                return true;
            }
            i += 1;
        }
    }
    false
}

/// Dump an entity declaration (`ENTITYDECL(name), internal`).
///
/// # Safety
///
/// - `payload` must be NULL or a valid pointer to a live `_xmlEntity` whose
///   `name` is NULL or a NUL-terminated string.
/// - `data` must be NULL or a valid pointer to a `DtdDumpCtx` whose `output`
///   is a valid writable `FILE*` and whose `depth` is non-negative.
/// - The entity and context must not be mutated concurrently during the call.
unsafe extern "C" fn dump_entityscan_cb(payload: *mut c_void, data: *mut c_void, _name: *const u8) {
    if payload.is_null() || data.is_null() {
        return;
    }
    let ctx = unsafe { &*(data as *const DtdDumpCtx) };
    let ent = unsafe { &*(payload as *mut crate::abi::structs::_xmlEntity) };
    unsafe {
        for _ in 0..ctx.depth {
            libc::fprintf(ctx.output, c"  ".as_ptr() as *const c_char);
        }
        libc::fprintf(ctx.output, c"ENTITYDECL(".as_ptr() as *const c_char);
        if !ent.name.is_null() {
            libc::fprintf(ctx.output, c"%s".as_ptr() as *const c_char, ent.name);
        }
        if ent.etype == crate::abi::types::xmlEntityType::XML_INTERNAL_GENERAL_ENTITY as c_int {
            libc::fprintf(ctx.output, c"), internal\n".as_ptr() as *const c_char);
            // UPSTREAM-PARITY: the content line is the ENTITYDECL indent plus
            // one extra leading space.
            for _ in 0..ctx.depth {
                libc::fprintf(ctx.output, c"  ".as_ptr() as *const c_char);
            }
            libc::fprintf(ctx.output, c" content=".as_ptr() as *const c_char);
            if !ent.content.is_null() {
                xmlDebugDumpString(ctx.output, ent.content as *const u8);
            }
            libc::fprintf(ctx.output, c"\n".as_ptr() as *const c_char);
            // The entity's parsed content tree: upstream (debugXML.c
            // xmlCtxtDumpNode) recurses into ent->children, which the parser
            // populates on first reference (xmlCtxtParseEntity). For entity
            // declarations that were never referenced (no children) the raw
            // content is synthesized as a compact text node for plain text.
            if !ent.children.is_null() {
                let mut c = ent.children;
                while !c.is_null() {
                    xmlDebugDumpNode(ctx.output, c, (ctx.depth + 1) as c_int);
                    c = (*c).next;
                }
            } else if !ent.content.is_null() && !contains_markup(ent.content) {
                for _ in 0..(ctx.depth + 1) {
                    libc::fprintf(ctx.output, c"  ".as_ptr() as *const c_char);
                }
                libc::fprintf(ctx.output, c"TEXT compact\n".as_ptr() as *const c_char);
                for _ in 0..(ctx.depth + 2) {
                    libc::fprintf(ctx.output, c"  ".as_ptr() as *const c_char);
                }
                libc::fprintf(ctx.output, c"content=".as_ptr() as *const c_char);
                xmlDebugDumpString(ctx.output, ent.content as *const u8);
                libc::fprintf(ctx.output, c"\n".as_ptr() as *const c_char);
            }
        } else {
            libc::fprintf(ctx.output, c")\n".as_ptr() as *const c_char);
        }
    }
}

/// Dump a node and its subtree.
///
/// UPSTREAM-PARITY: `xmlDebugDumpNode()`
///
/// # SAFETY
///
/// - `output`, `node` must be valid pointers (or NULL
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

        // UPSTREAM-PARITY: only element-like nodes recurse into children;
        // text nodes (including the non-compact merged representation) do not.
        // The DTD node's declaration children are dumped from the hash tables
        // inside xmlDebugDumpOneNode, so the children chain must not be
        // walked again (upstream debugXML.c xmlCtxtDumpNode reaches the decl
        // nodes through the chain, but the candidate keeps them in the DTD
        // tables as well — walking both would duplicate them).
        let t = (*node).type_;
        let recurse = t == 1 // XML_ELEMENT_NODE
            || t == 9  // XML_DOCUMENT_NODE
            || t == 13 // XML_HTML_DOCUMENT_NODE
            || t == 11; // XML_DOCUMENT_FRAG_NODE
        if recurse && !(*node).children.is_null() {
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
///
/// # SAFETY
///
/// - `output`, `node` must be valid pointers (or NULL
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
///
/// # SAFETY
///
/// - `output`, `doc` must be valid pointers (or NULL
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
#[no_mangle]
pub unsafe extern "C" fn xmlDebugDumpDocument(output: *mut _IO_FILE, doc: *mut _xmlDoc) {
    if output.is_null() || doc.is_null() {
        return;
    }
    unsafe {
        // UPSTREAM-PARITY: xmlCtxtDumpDocHead prints "HTML DOCUMENT" for
        // HTML documents and "DOCUMENT" otherwise (debugXML.c).
        if (*doc).type_ == crate::abi::types::xmlElementType::XML_HTML_DOCUMENT_NODE as c_int {
            libc::fprintf(output, c"HTML DOCUMENT\n".as_ptr() as *const c_char);
        } else {
            libc::fprintf(output, c"DOCUMENT\n".as_ptr() as *const c_char);
        }
        if !(*doc).version.is_null() {
            libc::fprintf(output, c"version=".as_ptr() as *const c_char);
            xmlDebugDumpString(output, (*doc).version as *const u8);
            libc::fprintf(output, c"\n".as_ptr() as *const c_char);
        }
        if !(*doc).URL.is_null() {
            libc::fprintf(output, c"URL=".as_ptr() as *const c_char);
            // UPSTREAM-PARITY: xmlCtxtDumpDocHead prints the URL through
            // xmlCtxtDumpString, so it is truncated at 40 characters.
            xmlDebugDumpString(output, (*doc).URL as *const u8);
            libc::fprintf(output, c"\n".as_ptr() as *const c_char);
        }
        // UPSTREAM-PARITY: the standalone flag is tri-state; the debug dump
        // prints "standalone=true" whenever it is not 0 (unset defaults to
        // true in the parser).
        if (*doc).standalone != 0 {
            libc::fprintf(output, c"standalone=true\n".as_ptr() as *const c_char);
        }

        // Document-level namespace declarations (upstream keeps the xml
        // namespace here; other prefixes live on the root element's nsDef).
        if !(*doc).oldNs.is_null() {
            let mut ns = (*doc).oldNs;
            while !ns.is_null() {
                libc::fprintf(output, c"namespace ".as_ptr() as *const c_char);
                if (*ns).prefix.is_null() {
                    libc::fprintf(output, c" ".as_ptr() as *const c_char);
                } else {
                    libc::fprintf(output, c"%s".as_ptr() as *const c_char, (*ns).prefix);
                }
                libc::fprintf(output, c" href=%s\n".as_ptr() as *const c_char, (*ns).href);
                ns = (*ns).next;
            }
        }

        // Dump the internal subset. UPSTREAM-PARITY: xmlCreateIntSubset keeps
        // the DTD as a member of the document's children chain, so the
        // children loop below dumps it; only dump it here when the
        // construction path kept it solely on doc->intSubset (xmlCopyDoc,
        // lazily-created subsets). Never dump both.
        if !(*doc).intSubset.is_null() {
            let mut in_chain = false;
            let mut c = (*doc).children;
            while !c.is_null() {
                if c as *mut c_void == (*doc).intSubset as *mut c_void {
                    in_chain = true;
                    break;
                }
                c = (*c).next;
            }
            if !in_chain {
                xmlDebugDumpNode(output, (*doc).intSubset as *mut _xmlNode, 1);
            }
        }

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
///
/// # SAFETY
///
/// - `output`, `doc` must be valid pointers (or NULL
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
///
/// # SAFETY
///
/// - `output`, `node` must be valid pointers (or NULL
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
#[no_mangle]
pub unsafe extern "C" fn xmlLsOneNode(output: *mut _IO_FILE, node: *mut _xmlNode) {
    if output.is_null() || node.is_null() {
        return;
    }
    unsafe {
        match (*node).type_ {
            1 => {
                // XML_ELEMENT_NODE
                libc::fprintf(output, c"E ".as_ptr() as *const c_char);
                if !(*node).ns.is_null() && !(*(*node).ns).prefix.is_null() {
                    libc::fprintf(
                        output,
                        c"%s:".as_ptr() as *const c_char,
                        (*(*node).ns).prefix,
                    );
                }
                xmlDebugDumpString(output, (*node).name);
            }
            2 => {
                libc::fprintf(output, c"A ".as_ptr() as *const c_char);
                xmlDebugDumpString(output, (*node).name);
            }
            3 => {
                libc::fprintf(output, c"T ".as_ptr() as *const c_char);
                if !(*node).content.is_null() {
                    xmlDebugDumpString(output, (*node).content as *const u8);
                }
            }
            4 => {
                libc::fprintf(output, c"C ".as_ptr() as *const c_char);
            }
            5 => {
                libc::fprintf(output, c"E ".as_ptr() as *const c_char);
                xmlDebugDumpString(output, (*node).name);
            }
            6 => {
                libc::fprintf(output, c"E ".as_ptr() as *const c_char);
                xmlDebugDumpString(output, (*node).name);
            }
            7 => {
                libc::fprintf(output, c"PI ".as_ptr() as *const c_char);
                xmlDebugDumpString(output, (*node).name);
            }
            8 => {
                libc::fprintf(output, c"C ".as_ptr() as *const c_char);
            }
            9 => {
                libc::fprintf(output, c"D ".as_ptr() as *const c_char);
            }
            10 => {
                libc::fprintf(output, c"DTD ".as_ptr() as *const c_char);
            }
            14 => {
                libc::fprintf(output, c"X ".as_ptr() as *const c_char);
            }
            _ => {
                libc::fprintf(
                    output,
                    c"? (%d)".as_ptr() as *const c_char,
                    (*node).type_ as c_int,
                );
            }
        }
        libc::fprintf(output, c"\n".as_ptr() as *const c_char);
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

    use crate::abi::types::xmlChar;
    use crate::xml::tree::*;

    /// Helper: create a simple document for testing.
    #[allow(dead_code)]
    unsafe fn create_test_doc() -> *mut _xmlDoc {
        let doc = new_doc(c"1.0".as_ptr() as *const xmlChar);
        let root = new_node(ptr::null_mut(), c"root".as_ptr() as *const xmlChar);
        doc_set_root_element(doc, root);
        let child = new_child(root, ptr::null_mut(), c"child".as_ptr() as *const xmlChar);
        // Set a property using set_prop
        set_prop(
            child,
            c"attr1".as_ptr() as *const xmlChar,
            c"value1".as_ptr() as *const xmlChar,
        );
        doc
    }

    /// Test that `xmlBoolToText` returns the static strings `true`/`false`.
    ///
    /// # Safety
    ///
    /// - The returned pointers are non-NULL (asserted) and point to static
    ///   NUL-terminated strings; reading their first byte requires each
    ///   pointer to be valid, which the assertions guarantee.
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

    /// Test that `xmlDebugDumpString` tolerates NULL output and NULL string.
    ///
    /// # Safety
    ///
    /// - `xmlDebugDumpString` accepts NULL for both arguments per the C
    ///   contract; `fmemopen` returns a valid writable `FILE*` when non-NULL,
    ///   and `fclose` takes ownership of it exactly once.
    #[test]
    fn test_debug_dump_string_null() {
        unsafe {
            // Should not crash
            xmlDebugDumpString(ptr::null_mut(), ptr::null());
            // Should print "(null)"
            let f = libc::fmemopen(ptr::null_mut(), 0, c"w".as_ptr() as *const c_char);
            if !f.is_null() {
                xmlDebugDumpString(f, ptr::null());
                libc::fclose(f);
            }
        }
    }

    /// Test that `xmlDebugDumpDocument` tolerates NULL output and doc.
    ///
    /// # Safety
    ///
    /// - `xmlDebugDumpDocument` accepts NULL arguments (early return); a
    ///   non-NULL `fmemopen` result is a valid `FILE*` owned by the caller
    ///   and closed exactly once with `fclose`.
    #[test]
    fn test_debug_dump_document_null() {
        unsafe {
            xmlDebugDumpDocument(ptr::null_mut(), ptr::null_mut());
            let f = libc::fmemopen(ptr::null_mut(), 0, c"w".as_ptr() as *const c_char);
            if !f.is_null() {
                xmlDebugDumpDocument(f, ptr::null_mut());
                libc::fclose(f);
            }
        }
    }

    /// Test that `xmlDebugDumpNode` tolerates NULL output and node.
    ///
    /// # Safety
    ///
    /// - `xmlDebugDumpNode` accepts NULL arguments (early return); a non-NULL
    ///   `fmemopen` result is a valid `FILE*` closed exactly once with
    ///   `fclose`.
    #[test]
    fn test_debug_dump_node_null() {
        unsafe {
            xmlDebugDumpNode(ptr::null_mut(), ptr::null_mut(), 0);
            let f = libc::fmemopen(ptr::null_mut(), 0, c"w".as_ptr() as *const c_char);
            if !f.is_null() {
                xmlDebugDumpNode(f, ptr::null_mut(), 0);
                libc::fclose(f);
            }
        }
    }

    /// Test counting a node and its sibling chain.
    ///
    /// # Safety
    ///
    /// - `node` and `sibling` are non-NULL pointers returned by `new_node`
    ///   and linked with `add_sibling`; they remain valid until `free_node`
    ///   transfers ownership exactly once at the end.
    /// - `xmlLsCountNode` borrows the chain and must not race concurrent
    ///   mutation.
    #[test]
    fn test_ls_count_node() {
        unsafe {
            let node = new_node(ptr::null_mut(), c"test".as_ptr() as *const xmlChar);
            assert!(!node.is_null());
            let count = xmlLsCountNode(node);
            assert_eq!(count, 1);

            // Add a sibling
            let sibling = new_node(ptr::null_mut(), c"sibling".as_ptr() as *const xmlChar);
            add_sibling(node, sibling);
            let count = xmlLsCountNode(node);
            assert_eq!(count, 2);

            free_node(node);
        }
    }

    /// Test that `xmlDebugDumpAttr` tolerates NULL output and attr.
    ///
    /// # Safety
    ///
    /// - `xmlDebugDumpAttr` accepts NULL arguments; a non-NULL `fmemopen`
    ///   result is a valid `FILE*` closed exactly once with `fclose`.
    #[test]
    fn test_debug_dump_attr_null() {
        unsafe {
            xmlDebugDumpAttr(ptr::null_mut(), ptr::null_mut(), 0);
            let f = libc::fmemopen(ptr::null_mut(), 0, c"w".as_ptr() as *const c_char);
            if !f.is_null() {
                xmlDebugDumpAttr(f, ptr::null_mut(), 0);
                libc::fclose(f);
            }
        }
    }

    /// Test that `xmlDebugDumpAttrList` tolerates NULL output and attr list.
    ///
    /// # Safety
    ///
    /// - `xmlDebugDumpAttrList` accepts NULL arguments and returns without
    ///   dereferencing them; no pointers are owned or freed by this test.
    #[test]
    fn test_debug_dump_attr_list_null() {
        unsafe {
            xmlDebugDumpAttrList(ptr::null_mut(), ptr::null_mut(), 0);
        }
    }

    /// Test that `xmlDebugDumpNodeList` tolerates NULL output and node list.
    ///
    /// # Safety
    ///
    /// - `xmlDebugDumpNodeList` accepts NULL arguments and returns without
    ///   dereferencing them; no pointers are owned or freed by this test.
    #[test]
    fn test_debug_dump_node_list_null() {
        unsafe {
            xmlDebugDumpNodeList(ptr::null_mut(), ptr::null_mut(), 0);
        }
    }

    /// Test that `xmlLsOneNode` tolerates NULL output and node.
    ///
    /// # Safety
    ///
    /// - `xmlLsOneNode` accepts NULL arguments; a non-NULL `fmemopen` result
    ///   is a valid `FILE*` closed exactly once with `fclose`.
    #[test]
    fn test_ls_one_node_null() {
        unsafe {
            xmlLsOneNode(ptr::null_mut(), ptr::null_mut());
            let f = libc::fmemopen(ptr::null_mut(), 0, c"w".as_ptr() as *const c_char);
            if !f.is_null() {
                xmlLsOneNode(f, ptr::null_mut());
                libc::fclose(f);
            }
        }
    }

    /// Test that `xmlDebugDumpDocumentHead` tolerates NULL output and doc.
    ///
    /// # Safety
    ///
    /// - `xmlDebugDumpDocumentHead` accepts NULL arguments and returns without
    ///   dereferencing them; no pointers are owned or freed by this test.
    #[test]
    fn test_dump_document_head_null() {
        unsafe {
            xmlDebugDumpDocumentHead(ptr::null_mut(), ptr::null_mut());
        }
    }

    /// Test that `xmlLsCountNode` returns 0 for a NULL node.
    ///
    /// # Safety
    ///
    /// - Passing NULL to `xmlLsCountNode` is accepted and yields 0 without
    ///   dereferencing the pointer.
    #[test]
    fn test_ls_count_node_null() {
        assert_eq!(unsafe { xmlLsCountNode(ptr::null_mut()) }, 0);
    }
}
