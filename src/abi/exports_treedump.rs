//! exports_treedump — copy/dump/serialize/debug/nodelist C ABI family (§11.1-I).
//!
//! Fills the copy / dump / serialize / debug / node-list closure of the
//! `tree.h`, `valid.h`, `entities.h`, `parserInternals.h` and `debugXML.h`
//! C ABI, with exact upstream signatures:
//!
//! - tree.h: `xmlAttrSerializeTxtContent`, `xmlCopyDtd`, `xmlCopyNamespace`,
//!   `xmlCopyNamespaceList`, `xmlCopyNodeList`, `xmlCopyProp`,
//!   `xmlCopyPropList`, `xmlDocCopyNode`, `xmlDocCopyNodeList`,
//!   `xmlDocDumpFormatMemoryEnc`, `xmlDocDumpMemoryEnc`, `xmlDocFormatDump`,
//!   `xmlElemDump`, `xmlNodeDumpOutput`, `xmlNodeListGetRawString`,
//!   `xmlNodeListGetString`, `xmlStringGetNodeList`
//! - parserInternals.h: `xmlCopyChar`, `xmlCopyCharMultiByte`
//! - valid.h: `xmlCopyAttributeTable`, `xmlCopyDocElementContent`,
//!   `xmlCopyElementTable`, `xmlCopyEnumeration`, `xmlCopyNotationTable`,
//!   `xmlDumpAttributeDecl`, `xmlDumpAttributeTable`, `xmlDumpElementDecl`,
//!   `xmlDumpElementTable`, `xmlDumpNotationDecl`, `xmlDumpNotationTable`
//! - entities.h: `xmlCopyEntitiesTable`, `xmlDumpEntitiesTable`,
//!   `xmlDumpEntityDecl`
//! - debugXML.h: `xmlDebugCheckDocument`, `xmlDebugDumpDTD`,
//!   `xmlDebugDumpEntities`
//!
//! Semantics follow archaeology/libxml2-git (tree.c, valid.c, entities.c,
//! xmlsave.c, debugXML.c, parserInternals.c).
//!
//! `xmlStringLenGetNodeList` is already exported by the string workstream
//! (`src/abi/exports_string.rs`) and is deliberately NOT re-exported here.
//! `xmlCopyNode` is exported by `src/abi/exports_xml2.rs` (same internal
//! `xmlCopyNodeList` is the copy-list member of this family.
//!
//! # Upstream contract
//!
//! Parity target is upstream `debugXML.c`, `tree.c`, `valid.c`, `entities.c`,
//! `xmlsave.c` and `parserInternals.c` (libxml2 2.15.3) with the `tree.h`/
//! `valid.h`/`entities.h`/`debugXML.h` signatures; R-000164 (11.1-N) exercised
//! the copy/dump paths in the TREE-001 structural probe.
//!
//! # Conceptual behavior
//!
//! This module implements the copy/dump/serialize/debug/node-list family:
//! node/prop/namespace/Dtd/table copy functions, `xmlNodeDumpOutput` and the
//! doc dump helpers, `xmlAttrSerializeTxtContent`, the node-list string
//! conversions (`xmlNodeListGetString`/`RawString`, `xmlStringGetNodeList`),
//! and the `xmlDebug*` dump entry points.
//!
//! # Ownership & safety invariants
//!
//! Copy functions return fresh caller-owned objects (freed with the matching
//! free function — `xmlFreeNodeList` for `xmlCopyNodeList`, `xmlFreeProp` for
//! `xmlCopyProp`, `xmlFreeDtd` for `xmlCopyDtd`); `xmlNodeListGetString`
//! returns an xml-allocator string the caller frees with `xmlFree`;
//! `xmlCopyChar`/`xmlCopyCharMultiByte` write into caller-provided buffers.
//!
//! # Historical quirks & epochs
//!
//! E-004 (SEMANTIC_EPOCHS): `--debug --noent` dumps changed in 2.13.0 (commit
//! `8d04f0ee`, 2024-03-11, tree text-node refactor) from `TEXT` to
//! `TEXT compact` — the debug-dump formatting this module feeds. R-000164:
//! copy_node parent/last/line handling was aligned with upstream (line
//! preserved only for elements, text copies keep line 0).
//!
//! # Deliberate oddities
//!
//! `xmlStringLenGetNodeList` is deliberately not re-exported here (it lives in
//! exports_string — single owner per symbol); `xmlCopyNode` is exported from
//! exports_xml2 with the same internal helper, so this module only carries the
//! list member.
//!
//! # Proving courts
//!
//! The TREE-STRUCTURE court family (TREE-001 byte-identical) plus the
//! C14N/CLI-XMLLINT debug-dump cases and DSO-LOADER/HEADER-COMPILE cover this
//! module; the copy/dump unit tests run under cargo test.
//!
//! # Tempting simplifications that would break parity
//!
//! A tempting simplification is to implement the copy functions with shallow
//! pointer copies — upstream copies deep (children, namespaces, properties),
//! and R-000164s TREE-001 probe fingerprint would diverge (the copy_node
//! parent/last/line defects were exactly that class). Another shortcut, skipping
//! the `TEXT compact` threshold in dumps, would fail the E-004 epoch comparison
//! against the 2.13+ oracle.

#![allow(
    missing_docs,
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals
)]
#![allow(unused_variables)]
#![allow(private_interfaces)]
#![allow(unused_assignments)]
#![allow(unused_unsafe)]
#![allow(clippy::missing_safety_doc)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]

// SAFETY-SCOPE: EXPORT-TREEDUMP-MECHANICAL-001
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
use std::mem::size_of;
use std::os::raw::{c_char, c_int};

use crate::abi::allocator::{xmlFreeImpl, xmlMallocImpl, xmlMallocZero};
use crate::abi::structs::*;
use crate::abi::types::xmlChar;
use crate::abi::types::xmlElementType::*;
use crate::abi::types::xmlEntityType::*;
use crate::xml::debug::_IO_FILE;
use crate::xml::io;
use crate::xml::string::{check_utf8, xml_strdup, xml_strlen, xml_strndup};
use crate::xml::tree::{
    copy_node, free_node_list, get_doc_entity, new_ns, new_text, node_get_content, search_ns,
    search_ns_by_href, serialize_attr_value, serialize_node_opts, serialize_node_opts_xhtml,
    xml_is_xhtml,
};

// The FILE* is opaque at the ABI boundary and is passed as *mut c_void; the
// `stdout` data symbol is used by the debug dumpers' NULL-output fallback
// (upstream `if (output == NULL) output = stdout;`).
extern "C" {
    /// The libc `FILE *stdout` variable.
    static mut stdout: *mut c_void;
}

// ═══════════════════════════════════════════════════════════════════════════════
// Shared internal helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Entity flags (include/private/entities.h).
const XML_ENT_PARSED: c_int = 1 << 0;
const XML_ENT_EXPANDING: c_int = 1 << 3;

/// Debug check error codes (include/libxml/xmlerror.h, XML_CHECK_* range).
const XML_CHECK_FOUND_ELEMENT: c_int = 5000;
const XML_CHECK_FOUND_ATTRIBUTE: c_int = 5001;
const XML_CHECK_FOUND_TEXT: c_int = 5002;
const XML_CHECK_FOUND_CDATA: c_int = 5003;
const XML_CHECK_FOUND_ENTITYREF: c_int = 5004;
const XML_CHECK_FOUND_ENTITY: c_int = 5005;
const XML_CHECK_FOUND_PI: c_int = 5006;
const XML_CHECK_FOUND_COMMENT: c_int = 5007;
const XML_CHECK_FOUND_DOCTYPE: c_int = 5008;
const XML_CHECK_FOUND_FRAGMENT: c_int = 5009;
const XML_CHECK_FOUND_NOTATION: c_int = 5010;
const XML_CHECK_UNKNOWN_NODE: c_int = 5011;
const XML_CHECK_ENTITY_TYPE: c_int = 5012;
const XML_CHECK_NO_PARENT: c_int = 5013;
const XML_CHECK_NO_DOC: c_int = 5014;
const XML_CHECK_NO_NAME: c_int = 5015;
const XML_CHECK_NO_ELEM: c_int = 5016;
const XML_CHECK_WRONG_DOC: c_int = 5017;
const XML_CHECK_NO_PREV: c_int = 5018;
const XML_CHECK_WRONG_PREV: c_int = 5019;
const XML_CHECK_NO_NEXT: c_int = 5020;
const XML_CHECK_WRONG_NEXT: c_int = 5021;
const XML_CHECK_NOT_DTD: c_int = 5022;
const XML_CHECK_NOT_ATTR_DECL: c_int = 5024;
const XML_CHECK_NOT_ELEM_DECL: c_int = 5025;
const XML_CHECK_NOT_ENTITY_DECL: c_int = 5026;
const XML_CHECK_NOT_NS_DECL: c_int = 5027;
const XML_CHECK_NO_HREF: c_int = 5028;
const XML_CHECK_WRONG_PARENT: c_int = 5029;
const XML_CHECK_NS_SCOPE: c_int = 5030;
const XML_CHECK_NS_ANCESTOR: c_int = 5031;
const XML_CHECK_NOT_UTF8: c_int = 5032;
const XML_CHECK_NOT_NCNAME: c_int = 5034;
const XML_CHECK_WRONG_NAME: c_int = 5036;
const XML_CHECK_NAME_NOT_NULL: c_int = 5037;

/// Escape flags (include/private/io.h).
const XML_ESCAPE_ATTR: c_int = 1 << 0;
const XML_ESCAPE_NON_ASCII: c_int = 1 << 1;
const XML_ESCAPE_HTML: c_int = 1 << 2;
const XML_ESCAPE_QUOT: c_int = 1 << 3;

/// Compare a NUL-terminated xmlChar string with a byte slice.
///
/// # SAFETY
///
/// - `s` must be a valid NUL-terminated string or NULL (NULL never matches).
const unsafe fn c_str_eq_bytes(s: *const xmlChar, b: &[u8]) -> bool {
    if s.is_null() {
        return false;
    }
    let mut i = 0usize;
    while i < b.len() {
        if unsafe { *s.add(i) } != b[i] {
            return false;
        }
        i += 1;
    }
    unsafe { *s.add(i) == 0 }
}

/// Upstream `xmlGetUTF8Char` (xmlstring.c): decode the UTF-8 character
/// starting at `utf`; sets `*len` to the number of bytes consumed and
/// returns the code point, or -1 (with `*len = 0`) on error.
///
/// # SAFETY
///
/// - `utf` must point to at least `*len` readable bytes.
unsafe fn get_utf8_char(utf: *const xmlChar, len: *mut c_int) -> c_int {
    if utf.is_null() || len.is_null() || unsafe { *len } < 1 {
        return -1;
    }
    unsafe {
        let u0 = *utf;
        if u0 < 0x80 {
            *len = 1;
            return u0 as c_int;
        }
        if *len < 2 {
            return -1;
        }
        if (u0 & 0xE0) == 0xC0 {
            let u1 = *utf.add(1);
            if (u1 & 0xC0) != 0x80 {
                return -1;
            }
            if (u0 & 0x1F) < 2 {
                return -1;
            }
            *len = 2;
            return (((u0 & 0x1F) as c_int) << 6) | ((u1 & 0x3F) as c_int);
        }
        if *len < 3 {
            return -1;
        }
        if (u0 & 0xF0) == 0xE0 {
            let u1 = *utf.add(1);
            let u2 = *utf.add(2);
            if (u1 & 0xC0) != 0x80 || (u2 & 0xC0) != 0x80 {
                return -1;
            }
            if (u0 & 0x0F) == 0 && (u1 & 0x20) == 0 {
                return -1;
            }
            *len = 3;
            return (((u0 & 0x0F) as c_int) << 12)
                | (((u1 & 0x3F) as c_int) << 6)
                | ((u2 & 0x3F) as c_int);
        }
        if *len < 4 {
            return -1;
        }
        if (u0 & 0xF8) == 0xF0 {
            let u1 = *utf.add(1);
            let u2 = *utf.add(2);
            let u3 = *utf.add(3);
            if (u1 & 0xC0) != 0x80 || (u2 & 0xC0) != 0x80 || (u3 & 0xC0) != 0x80 {
                return -1;
            }
            if (u0 & 0x07) == 0 && (u1 & 0x30) == 0 {
                return -1;
            }
            *len = 4;
            return (((u0 & 0x07) as c_int) << 18)
                | (((u1 & 0x3F) as c_int) << 12)
                | (((u2 & 0x3F) as c_int) << 6)
                | ((u3 & 0x3F) as c_int);
        }
    }
    -1
}

/// Upstream `xmlNewDocText` (tree.c): a text node associated with `doc`
/// (NULL allowed). Names are heap-allocated copies throughout this crate.
///
/// # SAFETY
///
/// - `doc` must be a valid `xmlDoc*` or NULL.
/// - `content` must be a valid NUL-terminated string or NULL.
unsafe fn new_doc_text(doc: *const _xmlDoc, content: *const xmlChar) -> *mut _xmlNode {
    if !doc.is_null() {
        let t = unsafe { (*doc).type_ };
        if t != XML_DOCUMENT_NODE as c_int && t != XML_HTML_DOCUMENT_NODE as c_int {
            return ptr::null_mut();
        }
    }
    let node = new_text(content);
    if node.is_null() {
        return ptr::null_mut();
    }
    if !doc.is_null() {
        unsafe { (*node).doc = doc as *mut _xmlDoc };
    }
    node
}

/// Upstream `xmlNewEntityReference` (tree.c): an `XML_ENTITY_REF_NODE`
/// carrying the entity's name.
///
/// # SAFETY
///
/// - `doc` must be a valid `xmlDoc*` or NULL.
/// - `name` must be a valid NUL-terminated string.
unsafe fn new_entity_ref(doc: *const _xmlDoc, name: *const xmlChar) -> *mut _xmlNode {
    if name.is_null() {
        return ptr::null_mut();
    }
    if !doc.is_null() {
        let t = unsafe { (*doc).type_ };
        if t != XML_DOCUMENT_NODE as c_int && t != XML_HTML_DOCUMENT_NODE as c_int {
            return ptr::null_mut();
        }
    }
    let node = xmlMallocZero(size_of::<_xmlNode>()) as *mut _xmlNode;
    if node.is_null() {
        return ptr::null_mut();
    }
    let name_copy = xml_strdup(name);
    if name_copy.is_null() {
        xmlFreeImpl(node as *mut c_void);
        return ptr::null_mut();
    }
    unsafe {
        (*node).type_ = XML_ENTITY_REF_NODE as c_int;
        (*node).name = name_copy;
        if !doc.is_null() {
            (*node).doc = doc as *mut _xmlDoc;
        }
    }
    node
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlCopyChar / xmlCopyCharMultiByte (parserInternals.h)
// ═══════════════════════════════════════════════════════════════════════════════

/// Append the UTF-8 encoding of `val` to `out` (upstream parserInternals.c
/// `xmlCopyCharMultiByte`). Returns the number of xmlChar written, or 0 on
/// invalid input / out-of-range code point.
///
/// # SAFETY
///
/// - `out` must point to a buffer with room for at least 6 xmlChar.
#[no_mangle]
pub unsafe extern "C" fn xmlCopyCharMultiByte(mut out: *mut xmlChar, val: c_int) -> c_int {
    if out.is_null() || val < 0 {
        return 0;
    }
    if val >= 0x80 {
        let savedout = out;
        let mut bits: c_int;
        if val < 0x800 {
            unsafe { *out = ((val >> 6) | 0xC0) as xmlChar };
            out = unsafe { out.add(1) };
            bits = 0;
        } else if val < 0x10000 {
            unsafe { *out = ((val >> 12) | 0xE0) as xmlChar };
            out = unsafe { out.add(1) };
            bits = 6;
        } else if val < 0x110000 {
            unsafe { *out = ((val >> 18) | 0xF0) as xmlChar };
            out = unsafe { out.add(1) };
            bits = 12;
        } else {
            return 0;
        }
        while bits >= 0 {
            unsafe {
                *out = (((val >> bits) & 0x3F) | 0x80) as xmlChar;
            }
            out = unsafe { out.add(1) };
            bits -= 6;
        }
        return unsafe { out.offset_from(savedout) } as c_int;
    }
    unsafe { *out = val as xmlChar };
    1
}

/// Append the char value in the array; the `len` parameter is ignored for
/// compatibility (upstream parserInternals.c `xmlCopyChar`).
///
/// # SAFETY
///
/// - `out` must point to a buffer with room for at least 6 xmlChar.
#[no_mangle]
pub unsafe extern "C" fn xmlCopyChar(_len: c_int, out: *mut xmlChar, val: c_int) -> c_int {
    if out.is_null() || val < 0 {
        return 0;
    }
    if val >= 0x80 {
        return xmlCopyCharMultiByte(out, val);
    }
    unsafe { *out = val as xmlChar };
    1
}

// ═══════════════════════════════════════════════════════════════════════════════
// Escaping (upstream xmlEscapeText, xmlIO.c + codegen/escape.inc)
// ═══════════════════════════════════════════════════════════════════════════════

/// Escape `str` with the upstream `xmlEscapeText` flag semantics
/// (xmlIO.c 2.15, codegen/escape.inc). Returns a heap-allocated
/// NUL-terminated string, or NULL on allocation failure.
///
/// Escape sets (byte → replacement):
/// - default: CR → `&#13;`, `&` → `&amp;`, `<` → `&lt;`, `>` → `&gt;`
/// - `XML_ESCAPE_QUOT`: default plus `"` → `&quot;`
/// - `XML_ESCAPE_ATTR`: tab → `&#9;`, LF → `&#10;`, CR → `&#13;`,
///   `"` → `&quot;`, `&` → `&amp;`, `<` → `&lt;`, `>` → `&gt;`
/// - `XML_ESCAPE_HTML` (with or without ATTR): only `&`/`<`/`>` (plus
///   `"` in ATTR mode)
/// - `XML_ESCAPE_NON_ASCII`: bytes ≥ 0x80 are decoded as UTF-8 and emitted
///   as `&#xHH;` character references (0xFFFD for invalid sequences).
///
/// # SAFETY
///
/// - `str` must be a valid NUL-terminated string.
unsafe fn escape_text_flags(str: *const xmlChar, flags: c_int) -> *mut xmlChar {
    if str.is_null() {
        return ptr::null_mut();
    }
    let mut out: Vec<u8> = Vec::with_capacity(64);
    let mut cur = str;
    unsafe {
        loop {
            if *cur == 0 {
                break;
            }
            let base = cur;
            let mut offset: c_int = -1;
            let mut c: u8 = 0;
            loop {
                c = *cur;
                if c == 0 {
                    // NUL terminates the scan; upstream tab[0] == 0
                    // selects an empty replacement.
                    offset = 0;
                    break;
                }
                if c < 0x80 {
                    offset = escape_tab_offset(c, flags);
                    if offset >= 0 {
                        break;
                    }
                } else if (flags & XML_ESCAPE_NON_ASCII) != 0 {
                    break;
                }
                cur = cur.add(1);
            }
            // Copy the unescaped run verbatim.
            let run_len = cur.offset_from(base) as usize;
            out.extend_from_slice(core::slice::from_raw_parts(base, run_len));
            if offset >= 0 {
                if c != 0 {
                    out.extend_from_slice(escape_replacement(offset));
                    cur = cur.add(1);
                }
            } else {
                // NON_ASCII: decode a UTF-8 sequence into a hex char ref.
                let mut len: c_int = 4;
                let mut val = get_utf8_char(cur, &mut len);
                if val < 0 {
                    val = 0xFFFD;
                    cur = cur.add(1);
                } else {
                    if val == 0xFFFE || val == 0xFFFF {
                        val = 0xFFFD;
                    }
                    cur = cur.add(len as usize);
                }
                let hex = format!("&#x{:X};", val as u32);
                out.extend_from_slice(hex.as_bytes());
            }
        }
    }
    out.push(0);
    let p = xmlMallocImpl(out.len()) as *mut xmlChar;
    if p.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        ptr::copy_nonoverlapping(out.as_ptr(), p, out.len());
    }
    p
}

/// Offset into the escape-content table for byte `c` under `flags`
/// (upstream codegen/escape.inc `xmlEscapeTab*`), or -1 when the byte is
/// emitted verbatim.
const fn escape_tab_offset(c: u8, flags: c_int) -> c_int {
    if (flags & XML_ESCAPE_HTML) != 0 {
        return match c {
            b'&' => 33,
            b'<' => 39,
            b'>' => 44,
            b'"' if (flags & XML_ESCAPE_ATTR) != 0 => 26,
            _ => -1,
        };
    }
    if (flags & XML_ESCAPE_QUOT) != 0 {
        return match c {
            b'\r' => 20,
            b'"' => 26,
            b'&' => 33,
            b'<' => 39,
            b'>' => 44,
            _ => -1,
        };
    }
    if (flags & XML_ESCAPE_ATTR) != 0 {
        return match c {
            b'\t' => 9,
            b'\n' => 14,
            b'\r' => 20,
            b'"' => 26,
            b'&' => 33,
            b'<' => 39,
            b'>' => 44,
            _ => -1,
        };
    }
    match c {
        b'\r' => 20,
        b'&' => 33,
        b'<' => 39,
        b'>' => 44,
        _ => -1,
    }
}

/// Replacement string for an escape-content offset (upstream
/// `xmlEscapeContent`).
const fn escape_replacement(offset: c_int) -> &'static [u8] {
    match offset {
        9 => b"&#9;",
        14 => b"&#10;",
        20 => b"&#13;",
        26 => b"&quot;",
        33 => b"&amp;",
        39 => b"&lt;",
        44 => b"&gt;",
        _ => b"",
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlNodeListGetString / xmlNodeListGetRawString (tree.h)
// ═══════════════════════════════════════════════════════════════════════════════

/// Port of upstream `xmlNodeListGetStringInternal` (tree.c).
///
/// `escape == 0` concatenates raw text content and substitutes entity
/// references with the referenced entity's content; `escape != 0` escapes
/// text with `flags` and keeps entity references as `&name;`.
///
/// # SAFETY
///
/// - `node` must be a valid node pointer or NULL.
unsafe fn node_list_get_string_internal(
    node: *const _xmlNode,
    escape: c_int,
    flags: c_int,
) -> *mut xmlChar {
    if node.is_null() {
        // UPSTREAM-PARITY: c"" is a real NUL-terminated static (the b""
        // idiom would hand a dangling 0x1 pointer to xml_strdup).
        return xml_strdup(c"".as_ptr() as *const xmlChar);
    }
    unsafe {
        let n = &*node;
        if escape == 0
            && (n.type_ == XML_TEXT_NODE as c_int || n.type_ == XML_CDATA_SECTION_NODE as c_int)
            && n.next.is_null()
        {
            if n.content.is_null() {
                return xml_strdup(c"".as_ptr() as *const xmlChar);
            }
            return xml_strdup(n.content);
        }
    }

    let buf = io::buf_create(50);
    if buf.is_null() {
        return ptr::null_mut();
    }

    let mut cur = node;
    while !cur.is_null() {
        unsafe {
            let t = (*cur).type_;
            if t == XML_TEXT_NODE as c_int || t == XML_CDATA_SECTION_NODE as c_int {
                if !(*cur).content.is_null() {
                    if escape == 0 {
                        io::buf_cat(buf, (*cur).content);
                    } else {
                        let encoded = escape_text_flags((*cur).content, flags);
                        if encoded.is_null() {
                            io::buf_free(buf);
                            return ptr::null_mut();
                        }
                        io::buf_cat(buf, encoded);
                        xmlFreeImpl(encoded as *mut c_void);
                    }
                }
            } else if t == XML_ENTITY_REF_NODE as c_int {
                if escape == 0 {
                    let content = node_get_content(cur as *mut _xmlNode);
                    if !content.is_null() {
                        io::buf_cat(buf, content);
                        xmlFreeImpl(content as *mut c_void);
                    }
                } else {
                    io::buf_add(buf, b"&" as *const u8, 1);
                    if !(*cur).name.is_null() {
                        io::buf_cat(buf, (*cur).name);
                    }
                    io::buf_add(buf, b";" as *const u8, 1);
                }
            }
            cur = (*cur).next;
        }
    }

    let content = io::buf_content(buf);
    let len = io::buf_length(buf);
    let ret = if !content.is_null() && len > 0 {
        xml_strdup(content)
    } else {
        xml_strdup(c"".as_ptr() as *const xmlChar)
    };
    io::buf_free(buf);
    ret
}

/// Serialize the children of an attribute into a string.
///
/// If `inLine` is true, entity references are substituted; otherwise
/// entity references are kept and special characters are escaped
/// (upstream tree.c `xmlNodeListGetString`).
///
/// # SAFETY
///
/// - `doc` must be a valid `xmlDoc*` or NULL.
/// - `list` must be a valid node pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlNodeListGetString(
    doc: *mut _xmlDoc,
    list: *const _xmlNode,
    inLine: c_int,
) -> *mut xmlChar {
    let mut flags: c_int = 0;
    let mut escape: c_int = 0;

    /* backward compatibility */
    if list.is_null() {
        return ptr::null_mut();
    }

    if inLine == 0 {
        escape = 1;
        if !doc.is_null() && (*doc).type_ == XML_HTML_DOCUMENT_NODE as c_int {
            flags |= XML_ESCAPE_HTML;
        } else if doc.is_null() || (*doc).encoding.is_null() {
            flags |= XML_ESCAPE_NON_ASCII;
        }
        if !list.is_null() {
            let parent = unsafe { (*list).parent };
            if !parent.is_null() && (*parent).type_ == XML_ATTRIBUTE_NODE as c_int {
                flags |= XML_ESCAPE_ATTR;
            }
        }
    }

    node_list_get_string_internal(list, escape, flags)
}

/// Serialize the children of an attribute into a string, keeping entity
/// references and escaping with the XML_ESCAPE_QUOT rule set (upstream
/// tree.c `xmlNodeListGetRawString`).
///
/// # SAFETY
///
/// - `doc` must be a valid `xmlDoc*` or NULL (unused upstream).
/// - `list` must be a valid node pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlNodeListGetRawString(
    _doc: *const _xmlDoc,
    list: *const _xmlNode,
    inLine: c_int,
) -> *mut xmlChar {
    let mut escape: c_int = 0;
    let mut flags: c_int = 0;

    /* backward compatibility */
    if list.is_null() {
        return ptr::null_mut();
    }

    if inLine == 0 {
        escape = 1;
        flags = XML_ESCAPE_QUOT;
    }

    node_list_get_string_internal(list, escape, flags)
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlStringGetNodeList (tree.h)
// ═══════════════════════════════════════════════════════════════════════════════

/// Port of upstream `xmlNodeParseAttValue` (tree.c): parse an attribute
/// value into a list of text nodes and entity reference nodes. `attr` is
/// the entity whose `children`/`last` receive the parsed list during
/// recursive entity content parsing (NULL for the top-level call). The
/// node list is returned through `list_ptr` (may be NULL); returns 0 on
/// success, -1 on allocation failure.
///
/// # SAFETY
///
/// - `doc` must be a valid `xmlDoc*` or NULL.
/// - `value` must be a valid NUL-terminated string of at least `len` bytes
///   or NULL.
/// - `list_ptr` must be a valid `xmlNode**` or NULL.
unsafe fn node_parse_att_value(
    doc: *const _xmlDoc,
    attr: *mut _xmlNode,
    value: *const xmlChar,
    len: usize,
    list_ptr: *mut *mut _xmlNode,
) -> c_int {
    let mut head: *mut _xmlNode = ptr::null_mut();
    let mut last: *mut _xmlNode = ptr::null_mut();

    if !list_ptr.is_null() {
        unsafe { *list_ptr = ptr::null_mut() };
    }

    if value.is_null() || unsafe { *value } == 0 {
        return 0;
    }

    let mut buf: Vec<u8> = Vec::new();
    let mut cur = value;
    let mut q = cur;
    let mut remaining = len;

    'scan: loop {
        if remaining == 0 || unsafe { *cur } == 0 {
            break 'scan;
        }
        if unsafe { *cur } == b'&' {
            let mut charval: u32 = 0;

            /* Save the current text. */
            if cur != q {
                unsafe {
                    buf.extend_from_slice(core::slice::from_raw_parts(
                        q,
                        cur.offset_from(q) as usize,
                    ));
                }
            }

            if remaining > 2 && unsafe { *cur.add(1) } == b'#' && unsafe { *cur.add(2) } == b'x' {
                /* hex character reference */
                let mut tmp: u8 = 0;
                unsafe {
                    cur = cur.add(3);
                }
                remaining -= 3;
                loop {
                    if remaining == 0 {
                        break;
                    }
                    tmp = unsafe { *cur };
                    if tmp == b';' {
                        break;
                    }
                    let digit: u32 = match tmp {
                        b'0'..=b'9' => (tmp - b'0') as u32,
                        b'a'..=b'f' => (tmp - b'a' + 10) as u32,
                        b'A'..=b'F' => (tmp - b'A' + 10) as u32,
                        _ => {
                            charval = 0;
                            break;
                        }
                    };
                    charval = charval.wrapping_mul(16).wrapping_add(digit);
                    if charval > 0x110000 {
                        charval = 0x110000;
                    }
                    unsafe {
                        cur = cur.add(1);
                    }
                    remaining -= 1;
                }
                if tmp == b';' {
                    unsafe {
                        cur = cur.add(1);
                    }
                    remaining -= 1;
                }
                q = cur;
            } else if remaining > 1 && unsafe { *cur.add(1) } == b'#' {
                /* decimal character reference */
                let mut tmp: u8 = 0;
                unsafe {
                    cur = cur.add(2);
                }
                remaining -= 2;
                loop {
                    if remaining == 0 {
                        break;
                    }
                    tmp = unsafe { *cur };
                    if tmp == b';' {
                        break;
                    }
                    if !tmp.is_ascii_digit() {
                        charval = 0;
                        break;
                    }
                    charval = charval.wrapping_mul(10).wrapping_add((tmp - b'0') as u32);
                    if charval > 0x110000 {
                        charval = 0x110000;
                    }
                    unsafe {
                        cur = cur.add(1);
                    }
                    remaining -= 1;
                }
                if tmp == b';' {
                    unsafe {
                        cur = cur.add(1);
                    }
                    remaining -= 1;
                }
                q = cur;
            } else {
                /* read the entity name */
                unsafe {
                    cur = cur.add(1);
                }
                remaining -= 1;
                q = cur;
                while remaining > 0 && unsafe { *cur } != 0 && unsafe { *cur } != b';' {
                    unsafe {
                        cur = cur.add(1);
                    }
                    remaining -= 1;
                }
                if remaining == 0 || unsafe { *cur } == 0 {
                    break 'scan;
                }
                if cur != q {
                    let name = unsafe { xml_strndup(q, cur.offset_from(q) as usize) };
                    if name.is_null() {
                        free_node_list(head);
                        return -1;
                    }
                    let ent = get_doc_entity(doc, name);
                    if !ent.is_null() && (*ent).etype == XML_INTERNAL_PREDEFINED_ENTITY as c_int {
                        /* predefined entities don't generate nodes */
                        let content = (*ent).content;
                        let clen = xml_strlen(content);
                        unsafe {
                            buf.extend_from_slice(core::slice::from_raw_parts(content, clen));
                        }
                    } else if ent.is_null() || ((*ent).flags & XML_ENT_EXPANDING) == 0 {
                        /* flush the buffer so far */
                        if !buf.is_empty() {
                            buf.push(0); /* NUL-terminate for the text-node dup */
                            let node = new_doc_text(doc, buf.as_ptr() as *const xmlChar);
                            buf.pop();
                            if node.is_null() {
                                xmlFreeImpl(name as *mut c_void);
                                free_node_list(head);
                                return -1;
                            }
                            (*node).parent = attr;
                            if last.is_null() {
                                head = node;
                            } else {
                                (*last).next = node;
                                (*node).prev = last;
                            }
                            last = node;
                            buf.clear();
                        }

                        /* parse the entity content if not parsed yet */
                        if !ent.is_null()
                            && ((*ent).flags & XML_ENT_PARSED) == 0
                            && !(*ent).content.is_null()
                        {
                            (*ent).flags |= XML_ENT_EXPANDING;
                            let res = node_parse_att_value(
                                doc,
                                ent as *mut _xmlNode,
                                (*ent).content,
                                usize::MAX,
                                ptr::null_mut(),
                            );
                            (*ent).flags &= !XML_ENT_EXPANDING;
                            if res < 0 {
                                xmlFreeImpl(name as *mut c_void);
                                free_node_list(head);
                                return -1;
                            }
                            (*ent).flags |= XML_ENT_PARSED;
                        }

                        /* create a new REFERENCE_REF node */
                        let node = new_entity_ref(doc, name);
                        if node.is_null() {
                            xmlFreeImpl(name as *mut c_void);
                            free_node_list(head);
                            return -1;
                        }
                        (*node).parent = attr;
                        (*node).last = ent as *mut _xmlNode;
                        if !ent.is_null() {
                            (*node).children = ent as *mut _xmlNode;
                            (*node).content = (*ent).content;
                        }
                        if last.is_null() {
                            head = node;
                        } else {
                            (*last).next = node;
                            (*node).prev = last;
                        }
                        last = node;
                    }
                    xmlFreeImpl(name as *mut c_void);
                }
                unsafe {
                    cur = cur.add(1);
                }
                remaining -= 1;
                q = cur;
            }
            if charval != 0 {
                if charval >= 0x110000 {
                    charval = 0xFFFD; /* replacement character */
                }
                let mut buffer = [0u8; 10];
                let l = xmlCopyCharMultiByte(buffer.as_mut_ptr(), charval as c_int);
                buf.extend_from_slice(&buffer[..l as usize]);
            }
        } else {
            unsafe {
                cur = cur.add(1);
            }
            remaining -= 1;
        }
    }

    /* handle the last piece of text */
    if cur != q {
        unsafe {
            buf.extend_from_slice(core::slice::from_raw_parts(q, cur.offset_from(q) as usize));
        }
    }

    if !buf.is_empty() {
        buf.push(0); /* NUL-terminate for the text-node dup */
        let node = new_doc_text(doc, buf.as_ptr() as *const xmlChar);
        buf.pop();
        if node.is_null() {
            free_node_list(head);
            return -1;
        }
        (*node).parent = attr;
        if last.is_null() {
            head = node;
        } else {
            (*last).next = node;
            (*node).prev = last;
        }
        last = node;
    } else if head.is_null() {
        head = new_doc_text(doc, c"".as_ptr() as *const xmlChar);
        if head.is_null() {
            return -1;
        }
        (*head).parent = attr;
        last = head;
    }

    if !attr.is_null() {
        (*attr).children = head;
        (*attr).last = last;
    }
    if !list_ptr.is_null() {
        unsafe { *list_ptr = head };
    }
    0
}

/// Build a node list (text and entity reference nodes) from an attribute
/// value (upstream tree.c `xmlStringGetNodeList`). Predefined entity
/// references are expanded into text; other declared entities produce
/// entity reference nodes; undeclared references produce entity reference
/// nodes without content.
///
/// Returns the head of the linked list, or NULL for a NULL/empty value or
/// on allocation failure.
///
/// # SAFETY
///
/// - `doc` must be a valid `xmlDoc*` or NULL.
/// - `value` must be a valid NUL-terminated string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlStringGetNodeList(
    doc: *const _xmlDoc,
    value: *const xmlChar,
) -> *mut _xmlNode {
    let mut ret: *mut _xmlNode = ptr::null_mut();
    unsafe {
        node_parse_att_value(doc, ptr::null_mut(), value, usize::MAX, &mut ret);
    }
    ret
}

// ═══════════════════════════════════════════════════════════════════════════════
// Copy operations (tree.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// Set the document pointer of a subtree (upstream tree.c `xmlSetTreeDoc`).
///
/// # SAFETY
///
/// - `tree` must be a valid node pointer or NULL.
unsafe fn set_tree_doc(tree: *mut _xmlNode, doc: *mut _xmlDoc) {
    if tree.is_null() {
        return;
    }
    unsafe {
        let t = (*tree).type_;
        if t == XML_NAMESPACE_DECL as c_int {
            return;
        }
        if (*tree).doc != doc {
            if t == XML_ELEMENT_NODE as c_int {
                let mut prop = (*tree).properties;
                while !prop.is_null() {
                    if (*prop).type_ == XML_ATTRIBUTE_NODE as c_int {
                        set_tree_doc(prop as *mut _xmlNode, doc);
                    }
                    prop = (*prop).next;
                }
            }
            if !(*tree).children.is_null() && t != XML_ENTITY_REF_NODE as c_int {
                set_tree_doc((*tree).children, doc);
            }
            (*tree).doc = doc;
        }
    }
}

/// Reconcile a namespace for a tree (upstream tree.c `xmlNewReconciledNs`):
/// reuse an in-scope declaration with the same prefix, else one with the
/// same href, else declare the namespace on the tree.
///
/// # SAFETY
///
/// - `doc` must be a valid `xmlDoc*` or NULL.
/// - `tree` must be a valid node pointer or NULL.
/// - `ns` must be a valid namespace pointer.
unsafe fn new_reconciled_ns(
    doc: *mut _xmlDoc,
    tree: *mut _xmlNode,
    ns: *mut _xmlNs,
) -> *mut _xmlNs {
    if tree.is_null() {
        return ptr::null_mut();
    }
    let def = search_ns(doc, tree, unsafe { (*ns).prefix });
    if !def.is_null() {
        return def;
    }
    let def = search_ns_by_href(doc, tree, unsafe { (*ns).href });
    if !def.is_null() {
        return def;
    }
    new_ns(tree, unsafe { (*ns).href }, unsafe { (*ns).prefix })
}

/// Port of upstream tree.c `xmlStaticCopyNode`: copy `node` into document
/// `doc` with parent `parent`; `extended` is 0 (shallow), 1 (deep) or 2
/// (shallow plus properties/namespaces). Returns the copy or NULL on
/// allocation failure.
///
/// # SAFETY
///
/// - `node` must be a valid node pointer or NULL.
/// - `doc` / `parent` may be NULL.
unsafe fn static_copy_node(
    node: *const _xmlNode,
    doc: *mut _xmlDoc,
    parent: *mut _xmlNode,
    extended: c_int,
) -> *mut _xmlNode {
    if node.is_null() {
        return ptr::null_mut();
    }
    let n = unsafe { &*node };
    match n.type_ {
        t if t == XML_TEXT_NODE as c_int
            || t == XML_CDATA_SECTION_NODE as c_int
            || t == XML_ELEMENT_NODE as c_int
            || t == XML_DOCUMENT_FRAG_NODE as c_int
            || t == XML_ENTITY_REF_NODE as c_int
            || t == XML_PI_NODE as c_int
            || t == XML_COMMENT_NODE as c_int
            || t == XML_XINCLUDE_START as c_int
            || t == XML_XINCLUDE_END as c_int => {}
        t if t == XML_ATTRIBUTE_NODE as c_int => {
            return copy_prop_internal(doc, parent, node as *mut _xmlAttr) as *mut _xmlNode;
        }
        t if t == XML_NAMESPACE_DECL as c_int => {
            return copy_namespace_list(node as *mut _xmlNs) as *mut _xmlNode;
        }
        t if t == XML_DTD_NODE as c_int => {
            return copy_dtd_internal(node as *mut _xmlDtd) as *mut _xmlNode;
        }
        t if t == XML_DOCUMENT_NODE as c_int || t == XML_HTML_DOCUMENT_NODE as c_int => {
            return crate::xml::tree::copy_doc(node as *const _xmlDoc, extended) as *mut _xmlNode;
        }
        _ => {
            return ptr::null_mut();
        }
    }

    let ret = xmlMallocZero(size_of::<_xmlNode>()) as *mut _xmlNode;
    if ret.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        (*ret).type_ = n.type_;
        (*ret).doc = doc;
        (*ret).parent = parent;
        if !n.name.is_null() {
            (*ret).name = xml_strdup(n.name);
            if (*ret).name.is_null() {
                free_node(ret);
                return ptr::null_mut();
            }
        }
        if n.type_ != XML_ELEMENT_NODE as c_int
            && !n.content.is_null()
            && n.type_ != XML_ENTITY_REF_NODE as c_int
            && n.type_ != XML_XINCLUDE_END as c_int
            && n.type_ != XML_XINCLUDE_START as c_int
        {
            (*ret).content = xml_strdup(n.content);
            if (*ret).content.is_null() {
                free_node(ret);
                return ptr::null_mut();
            }
        } else if n.type_ == XML_ELEMENT_NODE as c_int {
            (*ret).line = n.line;
        }
    }

    if extended == 0 {
        return ret;
    }

    // Namespace declarations, namespace reconciliation and properties.
    if (n.type_ == XML_ELEMENT_NODE as c_int || n.type_ == XML_XINCLUDE_START as c_int)
        && !n.nsDef.is_null()
    {
        let nsdef = copy_namespace_list(n.nsDef);
        if nsdef.is_null() {
            free_node(ret);
            return ptr::null_mut();
        }
        unsafe { (*ret).nsDef = nsdef };
    }

    if n.type_ == XML_ELEMENT_NODE as c_int && !n.ns.is_null() {
        let mut ns = search_ns(doc, ret, unsafe { (*n.ns).prefix });
        if ns.is_null() {
            /* Search it in the original tree and add it at the top. */
            ns = search_ns(unsafe { (*n.ns).context }, node as *mut _xmlNode, unsafe {
                (*n.ns).prefix
            });
            if !ns.is_null() {
                let mut root = ret;
                while unsafe { !(*root).parent.is_null() } {
                    root = unsafe { (*root).parent };
                }
                let newns = new_ns(root, unsafe { (*ns).href }, unsafe { (*ns).prefix });
                if newns.is_null() {
                    free_node(ret);
                    return ptr::null_mut();
                }
                unsafe { (*ret).ns = newns };
            } else {
                let newns = new_reconciled_ns(doc, ret, unsafe { n.ns });
                if newns.is_null() {
                    free_node(ret);
                    return ptr::null_mut();
                }
                unsafe { (*ret).ns = newns };
            }
        } else {
            /* reference the existing namespace definition in our own tree */
            unsafe { (*ret).ns = ns };
        }
    }

    if n.type_ == XML_ELEMENT_NODE as c_int && !n.properties.is_null() {
        let props = copy_prop_list(ret, n.properties);
        if props.is_null() {
            free_node(ret);
            return ptr::null_mut();
        }
        unsafe { (*ret).properties = props };
    }

    if n.type_ == XML_ENTITY_REF_NODE as c_int {
        unsafe {
            let children = if doc.is_null() || (*node).doc != doc {
                get_doc_entity(doc, (*ret).name) as *mut _xmlNode
            } else {
                (*node).children
            };
            (*ret).children = children;
            (*ret).last = children;
        }
    } else if !n.children.is_null() && extended != 2 {
        let mut cur = n.children;
        let mut insert = ret;
        while !cur.is_null() {
            let copy = static_copy_node(cur, doc, insert, 2);
            if copy.is_null() {
                free_node(ret);
                return ptr::null_mut();
            }
            unsafe {
                /* Check for coalesced text nodes */
                if (*insert).last != copy {
                    if (*insert).last.is_null() {
                        (*insert).children = copy;
                    } else {
                        (*copy).prev = (*insert).last;
                        (*(*insert).last).next = copy;
                    }
                    (*insert).last = copy;
                }

                if (*cur).type_ != XML_ENTITY_REF_NODE as c_int && !(*cur).children.is_null() {
                    cur = (*cur).children;
                    insert = copy;
                    continue;
                }
            }
            loop {
                unsafe {
                    if !(*cur).next.is_null() {
                        cur = (*cur).next;
                        break;
                    }
                    cur = (*cur).parent;
                    insert = (*insert).parent;
                    if std::ptr::eq(cur, node) {
                        cur = ptr::null_mut();
                        break;
                    }
                }
            }
        }
    }

    ret
}

/// Port of upstream tree.c `xmlStaticCopyNodeList`.
///
/// # SAFETY
///
/// - `node` must be a valid node pointer or NULL.
/// - `doc` / `parent` may be NULL.
unsafe fn static_copy_node_list(
    node: *const _xmlNode,
    doc: *mut _xmlDoc,
    parent: *mut _xmlNode,
) -> *mut _xmlNode {
    let mut ret: *mut _xmlNode = ptr::null_mut();
    let mut p: *mut _xmlNode = ptr::null_mut();
    let mut new_subset: *mut _xmlDtd = ptr::null_mut();
    let mut linked_subset: c_int = 0;

    let mut cur = node;
    while !cur.is_null() {
        unsafe {
            let next = (*cur).next;
            let mut q: *mut _xmlNode = ptr::null_mut();
            if (*cur).type_ == XML_DTD_NODE as c_int {
                if doc.is_null() {
                    cur = next;
                    continue;
                }
                if (*doc).intSubset.is_null() && new_subset.is_null() {
                    q = copy_dtd_internal(cur as *mut _xmlDtd) as *mut _xmlNode;
                    if q.is_null() {
                        free_node_list(ret);
                        if !new_subset.is_null() {
                            free_dtd_internal(new_subset);
                        }
                        return ptr::null_mut();
                    }
                    set_tree_doc(q, doc);
                    (*q).parent = parent;
                    new_subset = q as *mut _xmlDtd;
                } else {
                    linked_subset = 1;
                    q = (*doc).intSubset as *mut _xmlNode;
                    /* Unlink */
                    if (*q).prev.is_null() {
                        if !(*q).parent.is_null() {
                            (*(*q).parent).children = (*q).next;
                        }
                    } else {
                        (*(*q).prev).next = (*q).next;
                    }
                    if (*q).next.is_null() {
                        if !(*q).parent.is_null() {
                            (*(*q).parent).last = (*q).prev;
                        }
                    } else {
                        (*(*q).next).prev = (*q).prev;
                    }
                    (*q).parent = parent;
                    (*q).next = ptr::null_mut();
                    (*q).prev = ptr::null_mut();
                }
            } else {
                q = static_copy_node(cur, doc, parent, 1);
            }
            if q.is_null() {
                free_node_list(ret);
                if !new_subset.is_null() {
                    free_dtd_internal(new_subset);
                }
                if linked_subset != 0 && !doc.is_null() {
                    (*(*doc).intSubset).next = ptr::null_mut();
                    (*(*doc).intSubset).prev = ptr::null_mut();
                }
                return ptr::null_mut();
            }
            if ret.is_null() {
                (*q).prev = ptr::null_mut();
                ret = q;
                p = q;
            } else if p != q {
                (*p).next = q;
                (*q).prev = p;
                p = q;
            }
            cur = next;
        }
    }
    if !doc.is_null() && !new_subset.is_null() {
        unsafe { (*doc).intSubset = new_subset };
    }
    ret
}

/// Port of upstream tree.c `xmlCopyPropInternal`.
///
/// # SAFETY
///
/// - `doc` / `target` / `cur` must be valid pointers or NULL.
unsafe fn copy_prop_internal(
    doc: *mut _xmlDoc,
    target: *mut _xmlNode,
    cur: *mut _xmlAttr,
) -> *mut _xmlAttr {
    if cur.is_null() {
        return ptr::null_mut();
    }
    if !target.is_null() && unsafe { (*target).type_ } != XML_ELEMENT_NODE as c_int {
        return ptr::null_mut();
    }

    // Choose the owning document exactly like upstream xmlNewDocProp.
    let ret_doc: *mut _xmlDoc = if !target.is_null() {
        unsafe { (*target).doc }
    } else if !doc.is_null() {
        doc
    } else if !unsafe { (*cur).parent }.is_null() {
        unsafe { (*(*cur).parent).doc }
    } else if !unsafe { (*cur).children }.is_null() {
        unsafe { (*(*cur).children).doc }
    } else {
        ptr::null_mut()
    };

    let ret = xmlMallocZero(size_of::<_xmlAttr>()) as *mut _xmlAttr;
    if ret.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        (*ret).type_ = XML_ATTRIBUTE_NODE as c_int;
        (*ret).name = xml_strdup((*cur).name);
        (*ret).doc = ret_doc;
        (*ret).parent = target;

        if !(*cur).ns.is_null() && !target.is_null() {
            let mut ns = search_ns((*target).doc, target, (*(*cur).ns).prefix);
            if ns.is_null() {
                /* not in the new tree scope: search the original tree */
                let orig_parent = (*cur).parent;
                if !orig_parent.is_null() {
                    ns = search_ns((*orig_parent).doc, orig_parent, (*(*cur).ns).prefix);
                }
                if !ns.is_null() {
                    let mut root = target;
                    let mut pred: *mut _xmlNode = ptr::null_mut();
                    while !(*root).parent.is_null() {
                        pred = root;
                        root = (*root).parent;
                    }
                    if root == (*target).doc as *mut _xmlNode {
                        root = pred;
                    }
                    (*ret).ns = new_ns(root, (*ns).href, (*ns).prefix);
                    if (*ret).ns.is_null() {
                        free_prop_internal(ret);
                        return ptr::null_mut();
                    }
                }
            } else if !(*ns).href.is_null()
                && !(*(*cur).ns).href.is_null()
                && crate::abi::exports_xml2::xmlStrEqual((*ns).href, (*(*cur).ns).href) != 0
            {
                /* the nice case */
                (*ret).ns = ns;
            } else {
                /* we need a new reconciled namespace */
                let newns = new_reconciled_ns((*target).doc, target, (*cur).ns);
                if newns.is_null() {
                    free_prop_internal(ret);
                    return ptr::null_mut();
                }
                (*ret).ns = newns;
            }
        }

        if !(*cur).children.is_null() {
            let children = static_copy_node_list((*cur).children, ret_doc, ret as *mut _xmlNode);
            if children.is_null() {
                free_prop_internal(ret);
                return ptr::null_mut();
            }
            (*ret).children = children;
            let mut tmp = children;
            while !tmp.is_null() {
                if (*tmp).next.is_null() {
                    (*ret).last = tmp;
                }
                tmp = (*tmp).next;
            }
        }
        /* NOTE: upstream registers ID attributes (xmlAddIDSafe) here; this
         * crate keeps no ID table, so the copy is not ID-registered. */
    }

    ret
}

/// Free a property (upstream xmlFreeProp, tree.c).
///
/// # SAFETY
///
/// - `prop` must be a valid attribute pointer or NULL.
unsafe fn free_prop_internal(prop: *mut _xmlAttr) {
    if prop.is_null() {
        return;
    }
    unsafe {
        if !(*prop).name.is_null() {
            xmlFreeImpl((*prop).name as *mut c_void);
        }
        if !(*prop).children.is_null() {
            free_node_list((*prop).children);
        }
        xmlFreeImpl(prop as *mut c_void);
    }
}

/// Free a node (upstream xmlFreeNode, tree.c). DTD nodes route to the DTD
/// deallocator.
///
/// # SAFETY
///
/// - `node` must be a valid node pointer or NULL.
unsafe fn free_node(node: *mut _xmlNode) {
    if node.is_null() {
        return;
    }
    unsafe {
        let t = (*node).type_;
        if t == XML_DTD_NODE as c_int {
            free_dtd_internal(node as *mut _xmlDtd);
            return;
        }
        if !(*node).name.is_null() {
            xmlFreeImpl((*node).name as *mut c_void);
        }
        if !(*node).content.is_null()
            && t != XML_ATTRIBUTE_NODE as c_int
            && t != XML_ENTITY_REF_NODE as c_int
        {
            xmlFreeImpl((*node).content as *mut c_void);
        }
        if !(*node).properties.is_null() {
            let mut cur = (*node).properties;
            while !cur.is_null() {
                let next = (*cur).next;
                free_prop_internal(cur);
                cur = next;
            }
        }
        if !(*node).children.is_null() {
            free_node_list((*node).children);
        }
        xmlFreeImpl(node as *mut c_void);
    }
}

/// Copy a namespace (upstream tree.c `xmlCopyNamespace`).
///
/// # SAFETY
///
/// - `cur` must be a valid namespace pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlCopyNamespace(cur: *mut _xmlNs) -> *mut _xmlNs {
    if cur.is_null() {
        return ptr::null_mut();
    }
    if unsafe { (*cur).type_ } != XML_NAMESPACE_DECL as c_int {
        return ptr::null_mut();
    }
    unsafe { new_ns(ptr::null_mut(), (*cur).href, (*cur).prefix) }
}

/// Copy a linked list of namespaces (upstream tree.c `xmlCopyNamespaceList`).
///
/// # SAFETY
///
/// - `cur` must be a valid namespace pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlCopyNamespaceList(cur: *mut _xmlNs) -> *mut _xmlNs {
    copy_namespace_list(cur)
}

/// Internal copy of a namespace list; on allocation failure the partial
/// list is freed and NULL returned (upstream xmlCopyNamespaceList).
unsafe fn copy_namespace_list(cur: *mut _xmlNs) -> *mut _xmlNs {
    let mut ret: *mut _xmlNs = ptr::null_mut();
    let mut p: *mut _xmlNs = ptr::null_mut();
    let mut c = cur;
    while !c.is_null() {
        let q = xmlCopyNamespace(c);
        if q.is_null() {
            free_ns_list(ret);
            return ptr::null_mut();
        }
        if p.is_null() {
            ret = q;
            p = q;
        } else {
            unsafe { (*p).next = q };
            p = q;
        }
        c = unsafe { (*c).next };
    }
    ret
}

/// Free a namespace list (upstream xmlFreeNsList).
///
/// # SAFETY
///
/// - `ns` must be a valid namespace pointer or NULL.
unsafe fn free_ns_list(ns: *mut _xmlNs) {
    let mut cur = ns;
    while !cur.is_null() {
        let next = unsafe { (*cur).next };
        unsafe {
            if !(*cur).href.is_null() {
                xmlFreeImpl((*cur).href as *mut c_void);
            }
            if !(*cur).prefix.is_null() {
                xmlFreeImpl((*cur).prefix as *mut c_void);
            }
            xmlFreeImpl(cur as *mut c_void);
        }
        cur = next;
    }
}

/// Create a copy of the attribute `cur`; the copy's parent pointer is set
/// to `target` but the attribute is not linked on the target element
/// (upstream tree.c `xmlCopyProp`).
///
/// # SAFETY
///
/// - `target` / `cur` must be valid pointers or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlCopyProp(target: *mut _xmlNode, cur: *mut _xmlAttr) -> *mut _xmlAttr {
    copy_prop_internal(ptr::null_mut(), target, cur)
}

/// Create a copy of an attribute list; parent pointers are set to `target`
/// but the attributes are not linked on the target element (upstream tree.c
/// `xmlCopyPropList`).
///
/// # SAFETY
///
/// - `target` / `cur` must be valid pointers or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlCopyPropList(
    target: *mut _xmlNode,
    cur: *mut _xmlAttr,
) -> *mut _xmlAttr {
    copy_prop_list(target, cur)
}

/// Internal copy of an attribute list (upstream xmlCopyPropList).
unsafe fn copy_prop_list(target: *mut _xmlNode, cur: *mut _xmlAttr) -> *mut _xmlAttr {
    if !target.is_null() && unsafe { (*target).type_ } != XML_ELEMENT_NODE as c_int {
        return ptr::null_mut();
    }
    let mut ret: *mut _xmlAttr = ptr::null_mut();
    let mut p: *mut _xmlAttr = ptr::null_mut();
    let mut c = cur;
    while !c.is_null() {
        let q = xmlCopyProp(target, c);
        if q.is_null() {
            free_prop_list(ret);
            return ptr::null_mut();
        }
        if p.is_null() {
            ret = q;
            p = q;
        } else {
            unsafe {
                (*p).next = q;
                (*q).prev = p;
            }
            p = q;
        }
        c = unsafe { (*c).next };
    }
    ret
}

/// Free a property list (upstream xmlFreePropList).
///
/// # SAFETY
///
/// - `prop` must be a valid attribute pointer or NULL.
unsafe fn free_prop_list(prop: *mut _xmlAttr) {
    let mut cur = prop;
    while !cur.is_null() {
        let next = unsafe { (*cur).next };
        free_prop_internal(cur);
        cur = next;
    }
}

/// Copy a node list and all children (upstream tree.c `xmlCopyNodeList`).
/// The copied nodes are not attached to any document.
///
/// # SAFETY
///
/// - `node` must be a valid node pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlCopyNodeList(node: *mut _xmlNode) -> *mut _xmlNode {
    static_copy_node_list(node, ptr::null_mut(), ptr::null_mut())
}

/// Copy a node into another document (upstream tree.c `xmlDocCopyNode`).
/// `recursive` 0 = shallow, 1 = deep, 2 = shallow with properties and
/// namespaces.
///
/// # SAFETY
///
/// - `node` / `doc` must be valid pointers or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlDocCopyNode(
    node: *mut _xmlNode,
    doc: *mut _xmlDoc,
    recursive: c_int,
) -> *mut _xmlNode {
    static_copy_node(node, doc, ptr::null_mut(), recursive)
}

/// Copy a node list and all children into a new document (upstream tree.c
/// `xmlDocCopyNodeList`).
///
/// # SAFETY
///
/// - `doc` / `node` must be valid pointers or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlDocCopyNodeList(
    doc: *mut _xmlDoc,
    node: *mut _xmlNode,
) -> *mut _xmlNode {
    static_copy_node_list(node, doc, ptr::null_mut())
}

/// Copy an enumeration attribute node list (upstream valid.c
/// `xmlCopyEnumeration`).
///
/// # SAFETY
///
/// - `cur` must be a valid enumeration pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlCopyEnumeration(cur: *mut _xmlEnumeration) -> *mut _xmlEnumeration {
    let mut ret: *mut _xmlEnumeration = ptr::null_mut();
    let mut last: *mut _xmlEnumeration = ptr::null_mut();
    let mut c = cur;
    while !c.is_null() {
        let copy = xmlMallocZero(size_of::<_xmlEnumeration>()) as *mut _xmlEnumeration;
        if copy.is_null() {
            free_enumeration(ret);
            return ptr::null_mut();
        }
        unsafe {
            if !(*c).name.is_null() {
                (*copy).name = xml_strdup((*c).name);
                if (*copy).name.is_null() {
                    xmlFreeImpl(copy as *mut c_void);
                    free_enumeration(ret);
                    return ptr::null_mut();
                }
            }
        }
        if ret.is_null() {
            ret = copy;
            last = copy;
        } else {
            unsafe { (*last).next = copy };
            last = copy;
        }
        c = unsafe { (*c).next };
    }
    ret
}

/// Free an enumeration list (upstream valid.c `xmlFreeEnumeration`).
///
/// # SAFETY
///
/// - `cur` must be a valid enumeration pointer or NULL.
unsafe fn free_enumeration(cur: *mut _xmlEnumeration) {
    let mut c = cur;
    while !c.is_null() {
        let next = unsafe { (*c).next };
        unsafe {
            if !(*c).name.is_null() {
                xmlFreeImpl((*c).name as *mut c_void);
            }
            xmlFreeImpl(c as *mut c_void);
        }
        c = next;
    }
}

/// Build a copy of an element content description (upstream valid.c
/// `xmlCopyDocElementContent`). `doc` is only used for dictionary lookups
/// upstream; this crate keeps heap copies, so it is unused.
///
/// # SAFETY
///
/// - `doc` must be a valid `xmlDoc*` or NULL.
/// - `cur` must be a valid content-model pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlCopyDocElementContent(
    _doc: *mut _xmlDoc,
    cur: *mut _xmlElementContent,
) -> *mut _xmlElementContent {
    if cur.is_null() {
        return ptr::null_mut();
    }

    let ret = xmlMallocZero(size_of::<_xmlElementContent>()) as *mut _xmlElementContent;
    if ret.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        (*ret).type_ = (*cur).type_;
        (*ret).ocur = (*cur).ocur;
        if !(*cur).name.is_null() {
            (*ret).name = xml_strdup((*cur).name);
            if (*ret).name.is_null() {
                free_element_content_internal(ret);
                return ptr::null_mut();
            }
        }
        if !(*cur).prefix.is_null() {
            (*ret).prefix = xml_strdup((*cur).prefix);
            if (*ret).prefix.is_null() {
                free_element_content_internal(ret);
                return ptr::null_mut();
            }
        }
        if !(*cur).c1.is_null() {
            (*ret).c1 = xmlCopyDocElementContent(_doc, (*cur).c1);
            if (*ret).c1.is_null() {
                free_element_content_internal(ret);
                return ptr::null_mut();
            }
            (*(*ret).c1).parent = ret;
        }

        let mut prev = ret;
        let mut cc = (*cur).c2;
        while !cc.is_null() {
            let tmp = xmlMallocZero(size_of::<_xmlElementContent>()) as *mut _xmlElementContent;
            if tmp.is_null() {
                free_element_content_internal(ret);
                return ptr::null_mut();
            }
            (*tmp).type_ = (*cc).type_;
            (*tmp).ocur = (*cc).ocur;
            (*prev).c2 = tmp;
            (*tmp).parent = prev;
            if !(*cc).name.is_null() {
                (*tmp).name = xml_strdup((*cc).name);
                if (*tmp).name.is_null() {
                    free_element_content_internal(ret);
                    return ptr::null_mut();
                }
            }
            if !(*cc).prefix.is_null() {
                (*tmp).prefix = xml_strdup((*cc).prefix);
                if (*tmp).prefix.is_null() {
                    free_element_content_internal(ret);
                    return ptr::null_mut();
                }
            }
            if !(*cc).c1.is_null() {
                (*tmp).c1 = xmlCopyDocElementContent(_doc, (*cc).c1);
                if (*tmp).c1.is_null() {
                    free_element_content_internal(ret);
                    return ptr::null_mut();
                }
                (*(*tmp).c1).parent = tmp;
            }
            prev = tmp;
            cc = (*cc).c2;
        }
    }
    ret
}

/// Free an element content tree (upstream valid.c `xmlFreeElementContent`).
///
/// # SAFETY
///
/// - `cur` must be a valid content-model pointer or NULL.
unsafe fn free_element_content_internal(cur: *mut _xmlElementContent) {
    if cur.is_null() {
        return;
    }
    unsafe {
        if !(*cur).c1.is_null() {
            free_element_content_internal((*cur).c1);
        }
        if !(*cur).c2.is_null() {
            free_element_content_internal((*cur).c2);
        }
        if !(*cur).name.is_null() {
            xmlFreeImpl((*cur).name as *mut c_void);
        }
        if !(*cur).prefix.is_null() {
            xmlFreeImpl((*cur).prefix as *mut c_void);
        }
        xmlFreeImpl(cur as *mut c_void);
    }
}

/// Copy a DTD (upstream tree.c `xmlCopyDtd`): fresh DTD node plus copies of
/// all declaration tables.
///
/// # SAFETY
///
/// - `dtd` must be a valid DTD pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlCopyDtd(dtd: *mut _xmlDtd) -> *mut _xmlDtd {
    copy_dtd_internal(dtd)
}

/// Internal `xmlCopyDtd` implementation.
unsafe fn copy_dtd_internal(dtd: *mut _xmlDtd) -> *mut _xmlDtd {
    if dtd.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        let d = &*dtd;
        let ret = crate::xml::tree::new_dtd(ptr::null_mut(), d.name, d.ExternalID, d.SystemID);
        if ret.is_null() {
            return ptr::null_mut();
        }
        if !d.entities.is_null() {
            let t = xmlCopyEntitiesTable(d.entities);
            if t.is_null() {
                free_dtd_internal(ret);
                return ptr::null_mut();
            }
            (*ret).entities = t;
        }
        if !d.notations.is_null() {
            let t = xmlCopyNotationTable(d.notations);
            if t.is_null() {
                free_dtd_internal(ret);
                return ptr::null_mut();
            }
            (*ret).notations = t;
        }
        if !d.elements.is_null() {
            let t = xmlCopyElementTable(d.elements);
            if t.is_null() {
                free_dtd_internal(ret);
                return ptr::null_mut();
            }
            (*ret).elements = t;
        }
        if !d.attributes.is_null() {
            let t = xmlCopyAttributeTable(d.attributes);
            if t.is_null() {
                free_dtd_internal(ret);
                return ptr::null_mut();
            }
            (*ret).attributes = t;
        }
        if !d.pentities.is_null() {
            let t = xmlCopyEntitiesTable(d.pentities);
            if t.is_null() {
                free_dtd_internal(ret);
                return ptr::null_mut();
            }
            (*ret).pentities = t;
        }

        /* Link the copy's children to the declarations in the copied
         * tables (upstream xmlCopyDtd). */
        let mut cur = d.children;
        let mut p: *mut _xmlNode = ptr::null_mut();
        while !cur.is_null() {
            let mut q: *mut _xmlNode = ptr::null_mut();
            let ct = (*cur).type_;
            if ct == XML_ENTITY_DECL as c_int {
                let tmp = cur as *mut _xmlEntity;
                match (*tmp).etype {
                    t if t == XML_INTERNAL_GENERAL_ENTITY as c_int
                        || t == XML_EXTERNAL_GENERAL_PARSED_ENTITY as c_int
                        || t == XML_EXTERNAL_GENERAL_UNPARSED_ENTITY as c_int =>
                    {
                        q = crate::xml::entities::get_entity_from_dtd(ret, (*tmp).name)
                            as *mut _xmlNode;
                    }
                    t if t == XML_INTERNAL_PARAMETER_ENTITY as c_int
                        || t == XML_EXTERNAL_PARAMETER_ENTITY as c_int =>
                    {
                        q = crate::xml::hash::hash_lookup(
                            (*ret).pentities as *mut crate::xml::hash::HashTable,
                            (*tmp).name,
                        ) as *mut _xmlNode;
                    }
                    _ => {}
                }
            } else if ct == XML_ELEMENT_DECL as c_int {
                let tmp = cur as *mut _xmlElement;
                q = crate::xml::dtd::get_element_decl(ret, (*tmp).name) as *mut _xmlNode;
            } else if ct == XML_ATTRIBUTE_DECL as c_int {
                let tmp = cur as *mut _xmlAttribute;
                let elem = if (*tmp).elem.is_null() {
                    ptr::null_mut()
                } else {
                    crate::xml::dtd::get_element_decl(ret, (*tmp).elem)
                };
                q = crate::xml::dtd::get_attribute_decl(ret, elem, (*tmp).name, 0) as *mut _xmlNode;
            } else if ct == XML_COMMENT_NODE as c_int {
                q = copy_node(cur, 0);
                if q.is_null() {
                    free_dtd_internal(ret);
                    return ptr::null_mut();
                }
            }

            if !q.is_null() {
                if p.is_null() {
                    (*ret).children = q;
                } else {
                    (*p).next = q;
                    (*q).prev = p;
                }
                p = q;
            }
            cur = (*cur).next;
        }
        ret
    }
}

/// Free a DTD (upstream xmlFreeDtd, tree.c).
///
/// # SAFETY
///
/// - `dtd` must be a valid DTD pointer or NULL.
unsafe fn free_dtd_internal(dtd: *mut _xmlDtd) {
    if dtd.is_null() {
        return;
    }
    unsafe {
        // UPSTREAM-PARITY (tree.c xmlFreeDtd): declaration nodes in the child
        // list are owned by the hash tables and freed by the deallocators
        // below; only non-declaration children (comments, PIs) are freed here.
        // This must run BEFORE the hash tables are freed so the decl nodes are
        // still alive when their type is inspected.
        if !(*dtd).children.is_null() {
            let mut c = (*dtd).children;
            while !c.is_null() {
                let next = (*c).next;
                let t = (*c).type_;
                if t != crate::abi::types::xmlElementType::XML_ELEMENT_DECL as c_int
                    && t != crate::abi::types::xmlElementType::XML_ATTRIBUTE_DECL as c_int
                    && t != crate::abi::types::xmlElementType::XML_ENTITY_DECL as c_int
                {
                    free_node(c);
                }
                c = next;
            }
        }
        if !(*dtd).name.is_null() {
            xmlFreeImpl((*dtd).name as *mut c_void);
        }
        if !(*dtd).ExternalID.is_null() {
            xmlFreeImpl((*dtd).ExternalID as *mut c_void);
        }
        if !(*dtd).SystemID.is_null() {
            xmlFreeImpl((*dtd).SystemID as *mut c_void);
        }

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

        if !(*dtd).notations.is_null() {
            crate::xml::hash::hash_free(
                (*dtd).notations as *mut crate::xml::hash::HashTable,
                Some(free_notation_wrapper),
            );
        }
        if !(*dtd).elements.is_null() {
            crate::xml::hash::hash_free(
                (*dtd).elements as *mut crate::xml::hash::HashTable,
                Some(free_element_wrapper),
            );
        }
        if !(*dtd).attributes.is_null() {
            crate::xml::hash::hash_free(
                (*dtd).attributes as *mut crate::xml::hash::HashTable,
                Some(free_attribute_wrapper),
            );
        }
        if !(*dtd).entities.is_null() {
            crate::xml::hash::hash_free(
                (*dtd).entities as *mut crate::xml::hash::HashTable,
                Some(free_entity_wrapper),
            );
        }
        if !(*dtd).pentities.is_null() {
            crate::xml::hash::hash_free(
                (*dtd).pentities as *mut crate::xml::hash::HashTable,
                Some(free_entity_wrapper),
            );
        }
        xmlFreeImpl(dtd as *mut c_void);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Table copies (valid.h / entities.h)
// ═══════════════════════════════════════════════════════════════════════════════

/// Hash copier callbacks wrapping the crate's declaration copiers (upstream
/// valid.c `xmlCopyElement`/`xmlCopyAttribute`/`xmlCopyNotation`,
/// entities.c `xmlCopyEntity`).
unsafe extern "C" fn copy_element_cb(payload: *mut c_void, _name: *const xmlChar) -> *mut c_void {
    crate::xml::dtd::copy_element(payload as *mut _xmlElement) as *mut c_void
}

unsafe extern "C" fn copy_attribute_cb(payload: *mut c_void, _name: *const xmlChar) -> *mut c_void {
    crate::xml::dtd::copy_attribute_decl(payload as *mut _xmlAttribute) as *mut c_void
}

unsafe extern "C" fn copy_notation_cb(payload: *mut c_void, _name: *const xmlChar) -> *mut c_void {
    crate::xml::dtd::copy_notation(payload as *mut _xmlNotation) as *mut c_void
}

unsafe extern "C" fn copy_entity_cb(payload: *mut c_void, _name: *const xmlChar) -> *mut c_void {
    crate::xml::entities::copy_entity(payload as *mut _xmlEntity) as *mut c_void
}

/// Build a copy of an element table (upstream valid.c
/// `xmlCopyElementTable`).
///
/// # SAFETY
///
/// - `table` must be a valid hash-table pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlCopyElementTable(table: *mut c_void) -> *mut c_void {
    crate::xml::hash::hash_copy(
        table as *mut crate::xml::hash::HashTable,
        Some(copy_element_cb),
    ) as *mut c_void
}

/// Build a copy of an attribute table (upstream valid.c
/// `xmlCopyAttributeTable`).
///
/// # SAFETY
///
/// - `table` must be a valid hash-table pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlCopyAttributeTable(table: *mut c_void) -> *mut c_void {
    crate::xml::hash::hash_copy(
        table as *mut crate::xml::hash::HashTable,
        Some(copy_attribute_cb),
    ) as *mut c_void
}

/// Build a copy of a notation table (upstream valid.c
/// `xmlCopyNotationTable`).
///
/// # SAFETY
///
/// - `table` must be a valid hash-table pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlCopyNotationTable(table: *mut c_void) -> *mut c_void {
    crate::xml::hash::hash_copy(
        table as *mut crate::xml::hash::HashTable,
        Some(copy_notation_cb),
    ) as *mut c_void
}

/// Build a copy of an entities table (upstream entities.c
/// `xmlCopyEntitiesTable`).
///
/// # SAFETY
///
/// - `table` must be a valid hash-table pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlCopyEntitiesTable(table: *mut c_void) -> *mut c_void {
    crate::xml::hash::hash_copy(
        table as *mut crate::xml::hash::HashTable,
        Some(copy_entity_cb),
    ) as *mut c_void
}

// ═══════════════════════════════════════════════════════════════════════════════
// DTD declaration dumps (valid.h / entities.h)
// ═══════════════════════════════════════════════════════════════════════════════

/// Write a quoted string, escaping `"` as `&quot;` (upstream
/// xmlOutputBufferWriteQuotedString, xmlIO.c).
///
/// # SAFETY
///
/// - `buf` must be a valid xmlBuffer pointer.
/// - `str` must be a valid NUL-terminated string.
unsafe fn write_quoted_string(buf: *mut _xmlBuffer, str: *const xmlChar) {
    io::buf_ccat(buf, b'"');
    if !str.is_null() {
        let mut i = 0usize;
        while unsafe { *str.add(i) } != 0 {
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

/// Dump a notation declaration (upstream xmlBufDumpNotationDecl,
/// xmlsave.c): `<!NOTATION name PUBLIC "pub" "sys" >\n`.
///
/// # SAFETY
///
/// - `buf` must be a valid xmlBuffer pointer.
/// - `nota` must be a valid notation pointer.
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

/// Dump the content of an element declaration as an XML DTD definition
/// (upstream valid.c `xmlDumpElementDecl`, routed through the crate's
/// serializer, which matches `xmlBufDumpElementDecl`).
///
/// # SAFETY
///
/// - `buf` must be a valid xmlBuffer pointer.
/// - `elem` must be a valid element declaration pointer.
#[no_mangle]
pub unsafe extern "C" fn xmlDumpElementDecl(buf: *mut _xmlBuffer, elem: *mut _xmlElement) {
    if buf.is_null() || elem.is_null() {
        return;
    }
    serialize_node_opts(elem as *mut _xmlNode, buf, 0, 0, ptr::null(), 0);
}

/// Dump the content of an attribute declaration as an XML DTD definition
/// (upstream valid.c `xmlDumpAttributeDecl`, routed through the crate's
/// serializer, which matches `xmlSaveWriteAttributeDecl`).
///
/// # SAFETY
///
/// - `buf` must be a valid xmlBuffer pointer.
/// - `attr` must be a valid attribute declaration pointer.
#[no_mangle]
pub unsafe extern "C" fn xmlDumpAttributeDecl(buf: *mut _xmlBuffer, attr: *mut _xmlAttribute) {
    if buf.is_null() || attr.is_null() {
        return;
    }
    serialize_node_opts(attr as *mut _xmlNode, buf, 0, 0, ptr::null(), 0);
}

/// Dump the content of an entity declaration as an XML DTD definition
/// (upstream entities.c `xmlDumpEntityDecl`, routed through the crate's
/// serializer, which matches `xmlBufDumpEntityDecl`).
///
/// # SAFETY
///
/// - `buf` must be a valid xmlBuffer pointer.
/// - `ent` must be a valid entity declaration pointer.
#[no_mangle]
pub unsafe extern "C" fn xmlDumpEntityDecl(buf: *mut _xmlBuffer, ent: *mut _xmlEntity) {
    if buf.is_null() || ent.is_null() {
        return;
    }
    serialize_node_opts(ent as *mut _xmlNode, buf, 0, 0, ptr::null(), 0);
}

/// Dump the content of a notation declaration as an XML DTD definition
/// (upstream valid.c `xmlDumpNotationDecl`).
///
/// # SAFETY
///
/// - `buf` must be a valid xmlBuffer pointer.
/// - `nota` must be a valid notation pointer.
#[no_mangle]
pub unsafe extern "C" fn xmlDumpNotationDecl(buf: *mut _xmlBuffer, nota: *mut _xmlNotation) {
    if buf.is_null() || nota.is_null() {
        return;
    }
    dump_notation_decl(buf, nota);
}

/// Hash-scan callbacks for the `xmlDump*Table` family.
unsafe extern "C" fn dump_element_decl_cb(
    payload: *mut c_void,
    data: *mut c_void,
    _name: *const xmlChar,
) {
    if !payload.is_null() && !data.is_null() {
        serialize_node_opts(
            payload as *mut _xmlNode,
            data as *mut _xmlBuffer,
            0,
            0,
            ptr::null(),
            0,
        );
    }
}

unsafe extern "C" fn dump_attribute_decl_cb(
    payload: *mut c_void,
    data: *mut c_void,
    _name: *const xmlChar,
) {
    if !payload.is_null() && !data.is_null() {
        serialize_node_opts(
            payload as *mut _xmlNode,
            data as *mut _xmlBuffer,
            0,
            0,
            ptr::null(),
            0,
        );
    }
}

unsafe extern "C" fn dump_entity_decl_cb(
    payload: *mut c_void,
    data: *mut c_void,
    _name: *const xmlChar,
) {
    if !payload.is_null() && !data.is_null() {
        serialize_node_opts(
            payload as *mut _xmlNode,
            data as *mut _xmlBuffer,
            0,
            0,
            ptr::null(),
            0,
        );
    }
}

unsafe extern "C" fn dump_notation_decl_cb(
    payload: *mut c_void,
    data: *mut c_void,
    _name: *const xmlChar,
) {
    if !payload.is_null() && !data.is_null() {
        dump_notation_decl(data as *mut _xmlBuffer, payload as *mut _xmlNotation);
    }
}

/// Dump the content of an element table as XML DTD definitions (upstream
/// valid.c `xmlDumpElementTable`). Iteration order is hash-bucket order.
///
/// # SAFETY
///
/// - `buf` must be a valid xmlBuffer pointer.
/// - `table` must be a valid hash-table pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlDumpElementTable(buf: *mut _xmlBuffer, table: *mut c_void) {
    if buf.is_null() || table.is_null() {
        return;
    }
    crate::xml::hash::hash_scan(
        table as *mut crate::xml::hash::HashTable,
        Some(dump_element_decl_cb),
        buf as *mut c_void,
    );
}

/// Dump the content of an attribute table as XML DTD definitions (upstream
/// valid.c `xmlDumpAttributeTable`).
///
/// # SAFETY
///
/// - `buf` must be a valid xmlBuffer pointer.
/// - `table` must be a valid hash-table pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlDumpAttributeTable(buf: *mut _xmlBuffer, table: *mut c_void) {
    if buf.is_null() || table.is_null() {
        return;
    }
    crate::xml::hash::hash_scan(
        table as *mut crate::xml::hash::HashTable,
        Some(dump_attribute_decl_cb),
        buf as *mut c_void,
    );
}

/// Dump the content of a notation table as XML DTD definitions (upstream
/// valid.c `xmlDumpNotationTable`).
///
/// # SAFETY
///
/// - `buf` must be a valid xmlBuffer pointer.
/// - `table` must be a valid hash-table pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlDumpNotationTable(buf: *mut _xmlBuffer, table: *mut c_void) {
    if buf.is_null() || table.is_null() {
        return;
    }
    crate::xml::hash::hash_scan(
        table as *mut crate::xml::hash::HashTable,
        Some(dump_notation_decl_cb),
        buf as *mut c_void,
    );
}

/// Dump the content of an entities table as XML DTD definitions (upstream
/// entities.c `xmlDumpEntitiesTable`).
///
/// # SAFETY
///
/// - `buf` must be a valid xmlBuffer pointer.
/// - `table` must be a valid hash-table pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlDumpEntitiesTable(buf: *mut _xmlBuffer, table: *mut c_void) {
    if buf.is_null() || table.is_null() {
        return;
    }
    crate::xml::hash::hash_scan(
        table as *mut crate::xml::hash::HashTable,
        Some(dump_entity_decl_cb),
        buf as *mut c_void,
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlAttrSerializeTxtContent (tree.h / xmlsave.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// Serialize attribute text to an xmlBuffer, escaping with the attribute
/// rules (upstream xmlsave.c `xmlAttrSerializeTxtContent` →
/// `xmlBufAttrSerializeTxtContent`): tab/LF/CR → `&#9;`/`&#10;`/`&#13;`,
/// `"` → `&quot;`, `<` → `&lt;`, `>` → `&gt;`, `&` → `&amp;`.
///
/// # SAFETY
///
/// - `buf` must be a valid xmlBuffer pointer.
/// - `doc` / `attr` must be valid pointers or NULL (unused upstream).
/// - `string` must be a valid NUL-terminated string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlAttrSerializeTxtContent(
    buf: *mut _xmlBuffer,
    _doc: *mut _xmlDoc,
    _attr: *mut _xmlAttr,
    string: *const xmlChar,
) {
    if buf.is_null() || string.is_null() {
        return;
    }
    serialize_attr_value(buf, string);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Serialization front-ends (xmlsave.c / tree.h)
// ═══════════════════════════════════════════════════════════════════════════════

/// Serialize an XML node to an output buffer (upstream xmlsave.c
/// `xmlNodeDumpOutput`). `level` is clamped to [0, 100]. The `encoding`
/// argument selects the output encoding upstream; this crate serializes
/// UTF-8 only, so it is accepted but has no effect.
///
/// # SAFETY
///
/// - `buf` must be a valid xmlOutputBuffer pointer.
/// - `doc` / `cur` must be valid pointers or NULL; `cur` must be non-NULL.
/// - `encoding` must be a valid C string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlNodeDumpOutput(
    buf: *mut _xmlOutputBuffer,
    doc: *mut _xmlDoc,
    cur: *mut _xmlNode,
    level: c_int,
    format: c_int,
    _encoding: *const c_char,
) {
    if buf.is_null() || cur.is_null() {
        return;
    }
    let level = level.clamp(0, 100);
    let tmp = io::buf_create(-1);
    if tmp.is_null() {
        return;
    }
    // UPSTREAM-PARITY (xmlsave.c xmlNodeDumpOutput): when the document's
    // DTD is an XHTML DTD, serialize via xhtmlNodeDumpOutput — a bare
    // <html> element gets the XHTML namespace and non-empty elements are
    // open/close serialized.
    let xhtml = xml_is_xhtml(doc);
    unsafe {
        serialize_node_opts_xhtml(cur, tmp, format, level, ptr::null(), 0, ptr::null(), xhtml);
    }
    let content = io::buf_content(tmp);
    let len = io::buf_length(tmp);
    if !content.is_null() && len > 0 {
        io::output_buffer_write(buf, len, content as *const c_char);
    }
    io::buf_free(tmp);
}

/// Serialize an XML node to a `FILE` (upstream xmlsave.c `xmlElemDump`),
/// formatted. HTML documents are routed through the HTML serializer.
///
/// # SAFETY
///
/// - `f` must be a valid `FILE *` or NULL.
/// - `doc` / `cur` must be valid pointers or NULL; `cur` must be non-NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlElemDump(f: *mut c_void, doc: *mut _xmlDoc, cur: *mut _xmlNode) {
    if f.is_null() || cur.is_null() {
        return;
    }
    let out = io::output_buffer_create_file(f as *mut libc::FILE, ptr::null_mut());
    if out.is_null() {
        return;
    }
    let tmp = io::buf_create(-1);
    if tmp.is_null() {
        io::output_buffer_close(out);
        return;
    }
    if !doc.is_null() && (*doc).type_ == XML_HTML_DOCUMENT_NODE as c_int {
        crate::xml::html::serialize_node(cur, tmp, 1, 0);
    } else {
        serialize_node_opts(cur, tmp, 1, 0, ptr::null(), 0);
    }
    let content = io::buf_content(tmp);
    let len = io::buf_length(tmp);
    if !content.is_null() && len > 0 {
        io::output_buffer_write(out, len, content as *const c_char);
    }
    io::buf_free(tmp);
    io::output_buffer_close(out);
}

/// Serialize an XML document to a `FILE` (upstream xmlsave.c
/// `xmlDocFormatDump`). Returns the number of bytes written, or -1 on
/// failure.
///
/// # SAFETY
///
/// - `f` must be a valid `FILE *` or NULL.
/// - `cur` must be a valid `xmlDoc*` or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlDocFormatDump(
    f: *mut c_void,
    cur: *mut _xmlDoc,
    format: c_int,
) -> c_int {
    if cur.is_null() {
        return -1;
    }
    let out = io::output_buffer_create_file(f as *mut libc::FILE, ptr::null_mut());
    if out.is_null() {
        return -1;
    }
    let tmp = io::buf_create(-1);
    if tmp.is_null() {
        io::output_buffer_close(out);
        return -1;
    }
    serialize_node_opts(cur as *mut _xmlNode, tmp, format, 0, ptr::null(), 0);
    let content = io::buf_content(tmp);
    let len = io::buf_length(tmp);
    if !content.is_null() && len > 0 {
        io::output_buffer_write(out, len, content as *const c_char);
    }
    io::buf_free(tmp);
    let ret = io::output_buffer_close(out);
    if ret < 0 {
        -1
    } else {
        ret
    }
}

/// Serialize an XML document to memory with the given encoding and format
/// flag (upstream xmlsave.c `xmlDocDumpFormatMemoryEnc`). When
/// `txt_encoding` is non-NULL it is written into the XML declaration;
/// output bytes are UTF-8. The returned buffer must be freed with
/// `xmlFree`.
///
/// # SAFETY
///
/// - `out_doc` must be a valid `xmlDoc*` or NULL.
/// - `doc_txt_ptr` / `doc_txt_len` must be valid pointers.
/// - `txt_encoding` must be a valid C string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlDocDumpFormatMemoryEnc(
    out_doc: *mut _xmlDoc,
    doc_txt_ptr: *mut *mut xmlChar,
    doc_txt_len: *mut c_int,
    txt_encoding: *const c_char,
    format: c_int,
) {
    if !doc_txt_len.is_null() {
        unsafe { *doc_txt_len = 0 };
    }
    if doc_txt_ptr.is_null() {
        return;
    }
    unsafe { *doc_txt_ptr = ptr::null_mut() };
    if out_doc.is_null() {
        return;
    }

    let buf = io::buf_create(-1);
    if buf.is_null() {
        return;
    }

    if !txt_encoding.is_null() {
        // Upstream xmlSaveDocInternal writes the XML declaration with the
        // passed encoding (ctxt->encoding); the crate serializer uses
        // doc->encoding, so emit the declaration here and suppress the
        // serializer's.
        write_xml_declaration(buf, out_doc, txt_encoding as *const xmlChar);
        serialize_node_opts(out_doc as *mut _xmlNode, buf, format, 0, ptr::null(), 1);
    } else {
        serialize_node_opts(out_doc as *mut _xmlNode, buf, format, 0, ptr::null(), 0);
    }

    let content = io::buf_content(buf);
    let len = io::buf_length(buf);

    if !content.is_null() && len > 0 {
        let result = xmlMallocImpl((len + 1) as usize) as *mut xmlChar;
        if !result.is_null() {
            ptr::copy_nonoverlapping(content, result, len as usize);
            *result.add(len as usize) = 0;
            unsafe {
                *doc_txt_ptr = result;
                if !doc_txt_len.is_null() {
                    *doc_txt_len = len;
                }
            }
        }
    }

    io::buf_free(buf);
}

/// Write the XML declaration with a given encoding (upstream
/// xmlSaveDocInternal): `<?xml version="1.0" encoding="...";?>\n` plus the
/// standalone attribute.
///
/// # SAFETY
///
/// - `buf` must be a valid xmlBuffer pointer.
/// - `doc` must be a valid `xmlDoc*`.
/// - `encoding` must be a valid NUL-terminated string.
unsafe fn write_xml_declaration(buf: *mut _xmlBuffer, doc: *mut _xmlDoc, encoding: *const xmlChar) {
    let d = unsafe { &*doc };
    io::buf_add(buf, b"<?xml version=\"" as *const u8, 15);
    if !d.version.is_null() {
        io::buf_cat(buf, d.version);
    } else {
        io::buf_add(buf, b"1.0" as *const u8, 3);
    }
    io::buf_ccat(buf, b'"');
    if !encoding.is_null() {
        io::buf_add(buf, b" encoding=\"" as *const u8, 11);
        io::buf_cat(buf, encoding);
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

/// Serialize an XML document to memory (upstream xmlsave.c
/// `xmlDocDumpMemoryEnc`); equivalent to `xmlDocDumpFormatMemoryEnc` with
/// `format` set to 0.
///
/// # SAFETY
///
/// - `out_doc` must be a valid `xmlDoc*` or NULL.
/// - `doc_txt_ptr` / `doc_txt_len` must be valid pointers.
/// - `txt_encoding` must be a valid C string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlDocDumpMemoryEnc(
    out_doc: *mut _xmlDoc,
    doc_txt_ptr: *mut *mut xmlChar,
    doc_txt_len: *mut c_int,
    txt_encoding: *const c_char,
) {
    xmlDocDumpFormatMemoryEnc(out_doc, doc_txt_ptr, doc_txt_len, txt_encoding, 0)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Debug dumps (debugXML.h)
// ═══════════════════════════════════════════════════════════════════════════════

/// Debug error helper (upstream xmlDebugErr): `ERROR <code>: <msg>`.
unsafe fn debug_err(f: *mut _IO_FILE, errors: &mut c_int, code: c_int, msg: *const c_char) {
    *errors += 1;
    libc::fprintf(f, c"ERROR %d: %s".as_ptr() as *const c_char, code, msg);
}

/// Debug error helper with a `%s` substitution (upstream xmlDebugErr3).
unsafe fn debug_err_str(
    f: *mut _IO_FILE,
    errors: &mut c_int,
    code: c_int,
    msg: *const c_char,
    extra: *const c_char,
) {
    *errors += 1;
    libc::fprintf(f, c"ERROR %d: ".as_ptr() as *const c_char, code);
    libc::fprintf(f, msg, extra);
}

/// Debug error helper with a `%d` substitution (upstream xmlDebugErr2).
unsafe fn debug_err_int(
    f: *mut _IO_FILE,
    errors: &mut c_int,
    code: c_int,
    msg: *const c_char,
    extra: c_int,
) {
    *errors += 1;
    libc::fprintf(f, c"ERROR %d: ".as_ptr() as *const c_char, code);
    libc::fprintf(f, msg, extra);
}

/// Port of upstream `xmlNsCheckScope` (debugXML.c): 1 if in scope, -1 on
/// argument error, -2 if not in scope, -3 if not on an ancestor node.
///
/// # SAFETY
///
/// - `node` / `ns` must be valid pointers or NULL.
unsafe fn ns_check_scope(node: *mut _xmlNode, ns: *mut _xmlNs) -> c_int {
    if node.is_null() || ns.is_null() {
        return -1;
    }
    unsafe {
        let t = (*node).type_;
        if t != XML_ELEMENT_NODE as c_int
            && t != XML_ATTRIBUTE_NODE as c_int
            && t != XML_DOCUMENT_NODE as c_int
            && t != XML_TEXT_NODE as c_int
            && t != XML_HTML_DOCUMENT_NODE as c_int
            && t != XML_XINCLUDE_START as c_int
        {
            return -2;
        }

        let mut cur_node = node;
        while !cur_node.is_null()
            && ((*cur_node).type_ == XML_ELEMENT_NODE as c_int
                || (*cur_node).type_ == XML_ATTRIBUTE_NODE as c_int
                || (*cur_node).type_ == XML_TEXT_NODE as c_int
                || (*cur_node).type_ == XML_XINCLUDE_START as c_int)
        {
            if (*cur_node).type_ == XML_ELEMENT_NODE as c_int
                || (*cur_node).type_ == XML_XINCLUDE_START as c_int
            {
                let mut cur = (*cur_node).nsDef;
                while !cur.is_null() {
                    if cur == ns {
                        return 1;
                    }
                    if crate::abi::exports_xml2::xmlStrEqual((*cur).prefix, (*ns).prefix) != 0 {
                        return -2;
                    }
                    cur = (*cur).next;
                }
            }
            cur_node = (*cur_node).parent;
        }
        /* the xml namespace may be declared on the document node */
        if !cur_node.is_null()
            && ((*cur_node).type_ == XML_DOCUMENT_NODE as c_int
                || (*cur_node).type_ == XML_HTML_DOCUMENT_NODE as c_int)
        {
            let old_ns = (*(cur_node as *mut _xmlDoc)).oldNs;
            if old_ns == ns {
                return 1;
            }
        }
    }
    -3
}

/// Port of upstream `xmlCtxtNsCheckScope` (debugXML.c): report a namespace
/// that is not in scope.
unsafe fn ns_check_scope_report(
    f: *mut _IO_FILE,
    errors: &mut c_int,
    node: *mut _xmlNode,
    ns: *mut _xmlNs,
) {
    let ret = ns_check_scope(node, ns);
    if ret == -2 {
        if unsafe { (*ns).prefix.is_null() } {
            debug_err(
                f,
                errors,
                XML_CHECK_NS_SCOPE,
                c"Reference to default namespace not in scope\n".as_ptr() as *const c_char,
            );
        } else {
            debug_err_str(
                f,
                errors,
                XML_CHECK_NS_SCOPE,
                c"Reference to namespace '%s' not in scope\n".as_ptr() as *const c_char,
                unsafe { (*ns).prefix } as *const c_char,
            );
        }
    }
    if ret == -3 {
        if unsafe { (*ns).prefix.is_null() } {
            debug_err(
                f,
                errors,
                XML_CHECK_NS_ANCESTOR,
                c"Reference to default namespace not on ancestor\n".as_ptr() as *const c_char,
            );
        } else {
            debug_err_str(
                f,
                errors,
                XML_CHECK_NS_ANCESTOR,
                c"Reference to namespace '%s' not on ancestor\n".as_ptr() as *const c_char,
                unsafe { (*ns).prefix } as *const c_char,
            );
        }
    }
}

/// Port of upstream `xmlCtxtCheckString` (debugXML.c): UTF-8 check.
unsafe fn check_string(f: *mut _IO_FILE, errors: &mut c_int, str: *const xmlChar) {
    if str.is_null() {
        return;
    }
    if check_utf8(str) == 0 {
        debug_err_str(
            f,
            errors,
            XML_CHECK_NOT_UTF8,
            c"String is not UTF-8 %s".as_ptr() as *const c_char,
            str as *const c_char,
        );
    }
}

/// Port of upstream `xmlCtxtCheckName` (debugXML.c): name presence, NCName
/// conformance and dictionary status. The dictionary check is inert here —
/// this crate keeps no document dictionaries (doc->dict is NULL), which
/// matches the upstream guard `(ctxt->dict != NULL)`.
unsafe fn check_name(f: *mut _IO_FILE, errors: &mut c_int, name: *const xmlChar) {
    if name.is_null() {
        debug_err(
            f,
            errors,
            XML_CHECK_NO_NAME,
            c"Name is NULL".as_ptr() as *const c_char,
        );
        return;
    }
    if crate::xml::validation::validate_name_space(name, 0) != 0 {
        debug_err_str(
            f,
            errors,
            XML_CHECK_NOT_NCNAME,
            c"Name is not an NCName '%s'".as_ptr() as *const c_char,
            name as *const c_char,
        );
    }
}

/// Port of upstream `xmlCtxtGenericNodeCheck` (debugXML.c): the core
/// tree-integrity checks (parent/doc/prev/next links, namespace scope,
/// content UTF-8, node names).
unsafe fn generic_node_check(f: *mut _IO_FILE, errors: &mut c_int, node: *mut _xmlNode) {
    unsafe {
        let doc = (*node).doc;

        if (*node).parent.is_null() {
            debug_err(
                f,
                errors,
                XML_CHECK_NO_PARENT,
                c"Node has no parent\n".as_ptr() as *const c_char,
            );
        }
        if (*node).doc.is_null() {
            debug_err(
                f,
                errors,
                XML_CHECK_NO_DOC,
                c"Node has no doc\n".as_ptr() as *const c_char,
            );
        }
        if !(*node).parent.is_null()
            && (*node).doc != (*(*node).parent).doc
            && !c_str_eq_bytes((*node).name, b"pseudoroot")
        {
            debug_err(
                f,
                errors,
                XML_CHECK_WRONG_DOC,
                c"Node doc differs from parent's one\n".as_ptr() as *const c_char,
            );
        }
        if (*node).prev.is_null() {
            if (*node).type_ == XML_ATTRIBUTE_NODE as c_int {
                if !(*node).parent.is_null()
                    && node != (*(*node).parent).properties as *mut _xmlNode
                {
                    debug_err(
                        f,
                        errors,
                        XML_CHECK_NO_PREV,
                        c"Attr has no prev and not first of attr list\n".as_ptr() as *const c_char,
                    );
                }
            } else if !(*node).parent.is_null() && (*(*node).parent).children != node {
                debug_err(
                    f,
                    errors,
                    XML_CHECK_NO_PREV,
                    c"Node has no prev and not first of parent list\n".as_ptr() as *const c_char,
                );
            }
        } else if (*(*node).prev).next != node {
            debug_err(
                f,
                errors,
                XML_CHECK_WRONG_PREV,
                c"Node prev->next : back link wrong\n".as_ptr() as *const c_char,
            );
        }
        if (*node).next.is_null() {
            if !(*node).parent.is_null()
                && (*node).type_ != XML_ATTRIBUTE_NODE as c_int
                && (*(*node).parent).last != node
                && (*(*node).parent).type_ == XML_ELEMENT_NODE as c_int
            {
                debug_err(
                    f,
                    errors,
                    XML_CHECK_NO_NEXT,
                    c"Node has no next and not last of parent list\n".as_ptr() as *const c_char,
                );
            }
        } else {
            if (*(*node).next).prev != node {
                debug_err(
                    f,
                    errors,
                    XML_CHECK_WRONG_NEXT,
                    c"Node next->prev : forward link wrong\n".as_ptr() as *const c_char,
                );
            }
            if (*(*node).next).parent != (*node).parent {
                /* upstream 2.15 prints the same message for 5029 */
                debug_err(
                    f,
                    errors,
                    XML_CHECK_WRONG_PARENT,
                    c"Node next->prev : forward link wrong\n".as_ptr() as *const c_char,
                );
            }
        }
        if (*node).type_ == XML_ELEMENT_NODE as c_int {
            let mut ns = (*node).nsDef;
            while !ns.is_null() {
                ns_check_scope_report(f, errors, node, ns);
                ns = (*ns).next;
            }
            if !(*node).ns.is_null() {
                ns_check_scope_report(f, errors, node, (*node).ns);
            }
        } else if (*node).type_ == XML_ATTRIBUTE_NODE as c_int && !(*node).ns.is_null() {
            ns_check_scope_report(f, errors, node, (*node).ns);
        }

        if (*node).type_ != XML_ELEMENT_NODE as c_int
            && (*node).type_ != XML_ATTRIBUTE_NODE as c_int
            && (*node).type_ != XML_ELEMENT_DECL as c_int
            && (*node).type_ != XML_ATTRIBUTE_DECL as c_int
            && (*node).type_ != XML_DTD_NODE as c_int
            && (*node).type_ != XML_HTML_DOCUMENT_NODE as c_int
            && (*node).type_ != XML_DOCUMENT_NODE as c_int
            && !(*node).content.is_null()
        {
            check_string(f, errors, (*node).content);
        }
        let _ = doc;
        if (*node).type_ == XML_ELEMENT_NODE as c_int
            || (*node).type_ == XML_ATTRIBUTE_NODE as c_int
        {
            check_name(f, errors, (*node).name);
        } else if (*node).type_ == XML_TEXT_NODE as c_int {
            if !c_str_eq_bytes((*node).name, b"text") && !c_str_eq_bytes((*node).name, b"textnoenc")
            {
                debug_err_str(
                    f,
                    errors,
                    XML_CHECK_WRONG_NAME,
                    c"Text node has wrong name '%s'".as_ptr() as *const c_char,
                    (*node).name as *const c_char,
                );
            }
        } else if (*node).type_ == XML_COMMENT_NODE as c_int {
            if !c_str_eq_bytes((*node).name, b"comment") {
                debug_err_str(
                    f,
                    errors,
                    XML_CHECK_WRONG_NAME,
                    c"Comment node has wrong name '%s'".as_ptr() as *const c_char,
                    (*node).name as *const c_char,
                );
            }
        } else if (*node).type_ == XML_PI_NODE as c_int {
            check_name(f, errors, (*node).name);
        } else if (*node).type_ == XML_CDATA_SECTION_NODE as c_int && !(*node).name.is_null() {
            debug_err_str(
                f,
                errors,
                XML_CHECK_NAME_NOT_NULL,
                c"CData section has non NULL name '%s'".as_ptr() as *const c_char,
                (*node).name as *const c_char,
            );
        }
    }
}

/// Port of upstream `xmlCtxtDumpDocHead` (debugXML.c): prints the
/// DOCUMENT/HTML DOCUMENT line (only when `check` is 0), or
/// misplaced-node errors for other types (always).
unsafe fn dump_doc_head(f: *mut _IO_FILE, errors: &mut c_int, doc: *mut _xmlDoc, check: c_int) {
    unsafe {
        match (*doc).type_ {
            t if t == XML_ELEMENT_NODE as c_int => {
                debug_err(
                    f,
                    errors,
                    XML_CHECK_FOUND_ELEMENT,
                    c"Misplaced ELEMENT node\n".as_ptr() as *const c_char,
                );
            }
            t if t == XML_ATTRIBUTE_NODE as c_int => {
                debug_err(
                    f,
                    errors,
                    XML_CHECK_FOUND_ATTRIBUTE,
                    c"Misplaced ATTRIBUTE node\n".as_ptr() as *const c_char,
                );
            }
            t if t == XML_TEXT_NODE as c_int => {
                debug_err(
                    f,
                    errors,
                    XML_CHECK_FOUND_TEXT,
                    c"Misplaced TEXT node\n".as_ptr() as *const c_char,
                );
            }
            t if t == XML_CDATA_SECTION_NODE as c_int => {
                debug_err(
                    f,
                    errors,
                    XML_CHECK_FOUND_CDATA,
                    c"Misplaced CDATA node\n".as_ptr() as *const c_char,
                );
            }
            t if t == XML_ENTITY_REF_NODE as c_int => {
                debug_err(
                    f,
                    errors,
                    XML_CHECK_FOUND_ENTITYREF,
                    c"Misplaced ENTITYREF node\n".as_ptr() as *const c_char,
                );
            }
            t if t == XML_ENTITY_NODE as c_int => {
                debug_err(
                    f,
                    errors,
                    XML_CHECK_FOUND_ENTITY,
                    c"Misplaced ENTITY node\n".as_ptr() as *const c_char,
                );
            }
            t if t == XML_PI_NODE as c_int => {
                debug_err(
                    f,
                    errors,
                    XML_CHECK_FOUND_PI,
                    c"Misplaced PI node\n".as_ptr() as *const c_char,
                );
            }
            t if t == XML_COMMENT_NODE as c_int => {
                debug_err(
                    f,
                    errors,
                    XML_CHECK_FOUND_COMMENT,
                    c"Misplaced COMMENT node\n".as_ptr() as *const c_char,
                );
            }
            t if t == XML_DOCUMENT_NODE as c_int => {
                if check == 0 {
                    libc::fprintf(f, c"DOCUMENT\n".as_ptr() as *const c_char);
                }
            }
            t if t == XML_HTML_DOCUMENT_NODE as c_int => {
                if check == 0 {
                    libc::fprintf(f, c"HTML DOCUMENT\n".as_ptr() as *const c_char);
                }
            }
            t if t == XML_DOCUMENT_TYPE_NODE as c_int => {
                debug_err(
                    f,
                    errors,
                    XML_CHECK_FOUND_DOCTYPE,
                    c"Misplaced DOCTYPE node\n".as_ptr() as *const c_char,
                );
            }
            t if t == XML_DOCUMENT_FRAG_NODE as c_int => {
                debug_err(
                    f,
                    errors,
                    XML_CHECK_FOUND_FRAGMENT,
                    c"Misplaced FRAGMENT node\n".as_ptr() as *const c_char,
                );
            }
            t if t == XML_NOTATION_NODE as c_int => {
                debug_err(
                    f,
                    errors,
                    XML_CHECK_FOUND_NOTATION,
                    c"Misplaced NOTATION node\n".as_ptr() as *const c_char,
                );
            }
            _ => {
                debug_err_int(
                    f,
                    errors,
                    XML_CHECK_UNKNOWN_NODE,
                    c"Unknown node type %d\n".as_ptr() as *const c_char,
                    (*doc).type_ as c_int,
                );
            }
        }
    }
}

/// Port of upstream `xmlCtxtDumpEntityCallback` (debugXML.c): one entity
/// line in the `xmlDebugDumpEntities` listing.
unsafe extern "C" fn dump_entity_callback(
    payload: *mut c_void,
    data: *mut c_void,
    _name: *const xmlChar,
) {
    if payload.is_null() {
        return;
    }
    let cur = payload as *mut _xmlEntity;
    let f = data as *mut _IO_FILE;
    let mut errors: c_int = 0;
    unsafe {
        libc::fprintf(f, c"%s : ".as_ptr() as *const c_char, (*cur).name);
        match (*cur).etype {
            t if t == XML_INTERNAL_GENERAL_ENTITY as c_int => {
                libc::fprintf(f, c"INTERNAL GENERAL, ".as_ptr() as *const c_char);
            }
            t if t == XML_EXTERNAL_GENERAL_PARSED_ENTITY as c_int => {
                libc::fprintf(f, c"EXTERNAL PARSED, ".as_ptr() as *const c_char);
            }
            t if t == XML_EXTERNAL_GENERAL_UNPARSED_ENTITY as c_int => {
                libc::fprintf(f, c"EXTERNAL UNPARSED, ".as_ptr() as *const c_char);
            }
            t if t == XML_INTERNAL_PARAMETER_ENTITY as c_int => {
                libc::fprintf(f, c"INTERNAL PARAMETER, ".as_ptr() as *const c_char);
            }
            t if t == XML_EXTERNAL_PARAMETER_ENTITY as c_int => {
                libc::fprintf(f, c"EXTERNAL PARAMETER, ".as_ptr() as *const c_char);
            }
            _ => {
                debug_err_int(
                    f,
                    &mut errors,
                    XML_CHECK_ENTITY_TYPE,
                    c"Unknown entity type %d\n".as_ptr() as *const c_char,
                    (*cur).etype as c_int,
                );
            }
        }
        if !(*cur).ExternalID.is_null() {
            libc::fprintf(f, c"ID \"%s\"".as_ptr() as *const c_char, (*cur).ExternalID);
        }
        if !(*cur).SystemID.is_null() {
            libc::fprintf(
                f,
                c"SYSTEM \"%s\"".as_ptr() as *const c_char,
                (*cur).SystemID,
            );
        }
        if !(*cur).orig.is_null() {
            libc::fprintf(f, c"\n orig \"%s\"".as_ptr() as *const c_char, (*cur).orig);
        }
        if (*cur).type_ != XML_ELEMENT_NODE as c_int && !(*cur).content.is_null() {
            libc::fprintf(
                f,
                c"\n content \"%s\"".as_ptr() as *const c_char,
                (*cur).content,
            );
        }
        libc::fprintf(f, c"\n".as_ptr() as *const c_char);
    }
}

/// Dump the entity declarations in use by a document (upstream debugXML.c
/// `xmlDebugDumpEntities`).
///
/// # SAFETY
///
/// - `output` must be a valid `FILE *` or NULL.
/// - `doc` must be a valid `xmlDoc*` or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlDebugDumpEntities(output: *mut _IO_FILE, doc: *mut _xmlDoc) {
    if output.is_null() || doc.is_null() {
        return;
    }
    let mut errors: c_int = 0;
    dump_doc_head(output, &mut errors, doc, 0);
    unsafe {
        let int_subset = (*doc).intSubset;
        let ext_subset = (*doc).extSubset;
        if !int_subset.is_null()
            && !(*int_subset).entities.is_null()
            && crate::xml::hash::hash_size(
                (*int_subset).entities as *mut crate::xml::hash::HashTable,
            ) > 0
        {
            libc::fprintf(
                output,
                c"Entities in internal subset\n".as_ptr() as *const c_char,
            );
            crate::xml::hash::hash_scan(
                (*int_subset).entities as *mut crate::xml::hash::HashTable,
                Some(dump_entity_callback),
                output as *mut c_void,
            );
        } else {
            libc::fprintf(
                output,
                c"No entities in internal subset\n".as_ptr() as *const c_char,
            );
        }
        if !ext_subset.is_null()
            && !(*ext_subset).entities.is_null()
            && crate::xml::hash::hash_size(
                (*ext_subset).entities as *mut crate::xml::hash::HashTable,
            ) > 0
        {
            libc::fprintf(
                output,
                c"Entities in external subset\n".as_ptr() as *const c_char,
            );
            crate::xml::hash::hash_scan(
                (*ext_subset).entities as *mut crate::xml::hash::HashTable,
                Some(dump_entity_callback),
                output as *mut c_void,
            );
        } else {
            libc::fprintf(
                output,
                c"No entities in external subset\n".as_ptr() as *const c_char,
            );
        }
    }
}

/// Dump debug information for a DTD (upstream debugXML.c
/// `xmlDebugDumpDTD`): the `DTD(name)` line, then either the declaration
/// children or `    DTD is empty`.
///
/// # SAFETY
///
/// - `output` must be a valid `FILE *` or NULL.
/// - `dtd` must be a valid DTD pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlDebugDumpDTD(output: *mut _IO_FILE, dtd: *mut _xmlDtd) {
    let output = if output.is_null() {
        unsafe { stdout as *mut _IO_FILE }
    } else {
        output
    };
    if dtd.is_null() {
        libc::fprintf(output, c"DTD is NULL\n".as_ptr() as *const c_char);
        return;
    }
    unsafe {
        // xmlCtxtDumpDtdNode: depth is 0 here, so no indentation.
        if !(*dtd).name.is_null() {
            libc::fprintf(output, c"DTD(%s)".as_ptr() as *const c_char, (*dtd).name);
        } else {
            libc::fprintf(output, c"DTD".as_ptr() as *const c_char);
        }
        if !(*dtd).ExternalID.is_null() {
            libc::fprintf(
                output,
                c", PUBLIC %s".as_ptr() as *const c_char,
                (*dtd).ExternalID,
            );
        }
        if !(*dtd).SystemID.is_null() {
            libc::fprintf(
                output,
                c", SYSTEM %s".as_ptr() as *const c_char,
                (*dtd).SystemID,
            );
        }
        libc::fprintf(output, c"\n".as_ptr() as *const c_char);

        if (*dtd).children.is_null() {
            libc::fprintf(output, c"    DTD is empty\n".as_ptr() as *const c_char);
        } else {
            crate::xml::debug::xmlDebugDumpNodeList(output, (*dtd).children, 1);
        }
    }
}

/// Port of upstream `xmlCtxtDumpOneNode` (debugXML.c) in check mode: prints
/// misplaced-node lines and the generic node check.
unsafe fn dump_one_node_check(f: *mut _IO_FILE, errors: &mut c_int, node: *mut _xmlNode) {
    if node.is_null() {
        return;
    }
    unsafe {
        match (*node).type_ {
            t if t == XML_ATTRIBUTE_NODE as c_int => {
                libc::fprintf(
                    f,
                    c"Error, ATTRIBUTE found here\n".as_ptr() as *const c_char,
                );
                generic_node_check(f, errors, node);
                return;
            }
            t if t == XML_DOCUMENT_NODE as c_int || t == XML_HTML_DOCUMENT_NODE as c_int => {
                libc::fprintf(f, c"Error, DOCUMENT found here\n".as_ptr() as *const c_char);
                generic_node_check(f, errors, node);
                return;
            }
            t if t == XML_DTD_NODE as c_int => {
                if (*node).type_ != XML_DTD_NODE as c_int {
                    debug_err(
                        f,
                        errors,
                        XML_CHECK_NOT_DTD,
                        c"Node is not a DTD".as_ptr() as *const c_char,
                    );
                }
                generic_node_check(f, errors, node);
                return;
            }
            t if t == XML_ELEMENT_DECL as c_int => {
                let elem = node as *mut _xmlElement;
                if (*elem).type_ != XML_ELEMENT_DECL as c_int {
                    debug_err(
                        f,
                        errors,
                        XML_CHECK_NOT_ELEM_DECL,
                        c"Node is not an element declaration".as_ptr() as *const c_char,
                    );
                }
                if (*elem).name.is_null() {
                    debug_err(
                        f,
                        errors,
                        XML_CHECK_NO_NAME,
                        c"Element declaration has no name".as_ptr() as *const c_char,
                    );
                }
                generic_node_check(f, errors, node);
                return;
            }
            t if t == XML_ATTRIBUTE_DECL as c_int => {
                let attr = node as *mut _xmlAttribute;
                if (*attr).type_ != XML_ATTRIBUTE_DECL as c_int {
                    debug_err(
                        f,
                        errors,
                        XML_CHECK_NOT_ATTR_DECL,
                        c"Node is not an attribute declaration".as_ptr() as *const c_char,
                    );
                }
                if (*attr).name.is_null() {
                    debug_err(
                        f,
                        errors,
                        XML_CHECK_NO_NAME,
                        c"Node attribute declaration has no name".as_ptr() as *const c_char,
                    );
                }
                if (*attr).elem.is_null() {
                    debug_err(
                        f,
                        errors,
                        XML_CHECK_NO_ELEM,
                        c"Node attribute declaration has no element name".as_ptr() as *const c_char,
                    );
                }
                generic_node_check(f, errors, node);
                return;
            }
            t if t == XML_ENTITY_DECL as c_int => {
                let ent = node as *mut _xmlEntity;
                if (*ent).type_ != XML_ENTITY_DECL as c_int {
                    debug_err(
                        f,
                        errors,
                        XML_CHECK_NOT_ENTITY_DECL,
                        c"Node is not an entity declaration".as_ptr() as *const c_char,
                    );
                }
                if (*ent).name.is_null() {
                    debug_err(
                        f,
                        errors,
                        XML_CHECK_NO_NAME,
                        c"Entity declaration has no name".as_ptr() as *const c_char,
                    );
                }
                generic_node_check(f, errors, node);
                return;
            }
            t if t == XML_NAMESPACE_DECL as c_int => {
                let ns = node as *mut _xmlNs;
                if (*ns).type_ != XML_NAMESPACE_DECL as c_int {
                    debug_err(
                        f,
                        errors,
                        XML_CHECK_NOT_NS_DECL,
                        c"Node is not a namespace declaration".as_ptr() as *const c_char,
                    );
                }
                if (*ns).href.is_null() {
                    if !(*ns).prefix.is_null() {
                        debug_err_str(
                            f,
                            errors,
                            XML_CHECK_NO_HREF,
                            c"Incomplete namespace %s href=NULL\n".as_ptr() as *const c_char,
                            (*ns).prefix as *const c_char,
                        );
                    } else {
                        debug_err(
                            f,
                            errors,
                            XML_CHECK_NO_HREF,
                            c"Incomplete default namespace href=NULL\n".as_ptr() as *const c_char,
                        );
                    }
                }
                return;
            }
            t if t == XML_XINCLUDE_START as c_int || t == XML_XINCLUDE_END as c_int => {
                return;
            }
            _ => {}
        }

        if (*node).doc.is_null() {
            libc::fprintf(f, c"PBM: doc == NULL !!!\n".as_ptr() as *const c_char);
        }

        /* entity-ref expansion is print-only upstream and inert in check
         * mode; the generic check always runs. */
        generic_node_check(f, errors, node);
    }
}

/// Port of upstream `xmlCtxtDumpNodeList` / `xmlCtxtDumpNode` (debugXML.c)
/// in check mode: walk the node list, one-node check and children
/// recursion.
unsafe fn dump_node_list_check(
    f: *mut _IO_FILE,
    errors: &mut c_int,
    node: *mut _xmlNode,
    depth: c_int,
) {
    let mut cur = node;
    while !cur.is_null() {
        dump_node_check(f, errors, cur, depth);
        unsafe {
            cur = (*cur).next;
        }
    }
}

unsafe fn dump_node_check(f: *mut _IO_FILE, errors: &mut c_int, node: *mut _xmlNode, depth: c_int) {
    let _ = depth;
    if node.is_null() {
        return;
    }
    dump_one_node_check(f, errors, node);
    unsafe {
        let t = (*node).type_;
        if t != XML_NAMESPACE_DECL as c_int
            && !(*node).children.is_null()
            && t != XML_ENTITY_REF_NODE as c_int
        {
            dump_node_list_check(f, errors, (*node).children, depth + 1);
        }
    }
}

/// Check a document for potential content problems and output the errors to
/// `output` (upstream debugXML.c `xmlDebugCheckDocument`). Returns the
/// number of errors found.
///
/// # SAFETY
///
/// - `output` must be a valid `FILE *` or NULL.
/// - `doc` must be a valid `xmlDoc*` or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlDebugCheckDocument(output: *mut _IO_FILE, doc: *mut _xmlDoc) -> c_int {
    let output = if output.is_null() {
        unsafe { stdout as *mut _IO_FILE }
    } else {
        output
    };
    let mut errors: c_int = 0;
    if doc.is_null() {
        return errors;
    }
    // xmlCtxtDumpDocumentHead in check mode only runs the doc-head
    // misplaced-node checks; the DOCUMENT line is suppressed.
    dump_doc_head(output, &mut errors, doc, 1);
    unsafe {
        let t = (*doc).type_;
        if (t == XML_DOCUMENT_NODE as c_int || t == XML_HTML_DOCUMENT_NODE as c_int)
            && !(*doc).children.is_null()
        {
            dump_node_list_check(output, &mut errors, (*doc).children, 1);
        }
    }
    errors
}
