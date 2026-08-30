//! exports_misc — family closure (11.1-I).
//!
//! C ABI exports for the "miscellaneous" families:
//!
//! 1. getset     — xmlGet*/xmlSet* legacy accessors (features, threads,
//!                 globals, tree navigation, entities, buffers)
//! 2. module     — xmlModule* dynamic-loading API (dlopen/dlsym/dlclose)
//! 3. ucs        — legacy `xmlUCSIsBlock`/`xmlUCSIsCat` name-table lookups
//!                 plus the `xmlUCSIsCatCc` control-character test
//! 4. valid      — validation helpers (attribute-value normalization,
//!                 potential-children / valid-elements enumeration)
//! 5. misc2      — `__xml*` aliases, parser-context error helpers,
//!                 xmlFormatError, tree node constructors, content-model
//!                 serialization, deprecated stubs
//!
//! Every function here mirrors an exported symbol of the oracle DSO
//! (`nm -D /usr/lib/libxml2.so.2`); signatures follow the installed
//! headers (`/usr/include/libxml2/libxml/*.h`) and the archaeology tree
//! (`archaeology/libxml2-git/*.c`).

#![allow(missing_docs)]
#![allow(non_snake_case)]
#![allow(non_camel_case_types)]
#![allow(non_upper_case_globals)]
#![allow(unused_variables)]
#![allow(clippy::missing_safety_doc)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use core::ffi::{c_char, c_void};
use core::mem::{size_of, zeroed};
use core::ptr;
use core::sync::atomic::{AtomicI32, AtomicPtr, Ordering};
use std::os::raw::c_int;

use crate::abi::allocator::*;
use crate::abi::callbacks::*;
use crate::abi::structs::*;
use crate::abi::types::xmlAttributeType::*;
use crate::abi::types::xmlBufferAllocationScheme::*;
use crate::abi::types::xmlElementContentOccur::*;
use crate::abi::types::xmlElementContentType::*;
use crate::abi::types::xmlElementType::*;
use crate::abi::types::xmlEntityType::*;
use crate::abi::types::xmlErrorLevel::*;
use crate::abi::types::*;

// ═══════════════════════════════════════════════════════════════════════════════
// Small C-string / xmlChar helpers (local, so this module is self-contained)
// ═══════════════════════════════════════════════════════════════════════════════

/// strlen for a NUL-terminated byte string.
unsafe fn cstr_len(s: *const c_char) -> usize {
    if s.is_null() {
        return 0;
    }
    let mut n = 0usize;
    while unsafe { *((s as *const u8).add(n)) } != 0 {
        n += 1;
    }
    n
}

/// Compare a C string against a byte literal (with implicit NUL).
unsafe fn cstr_eq(s: *const c_char, lit: &[u8]) -> bool {
    if s.is_null() {
        return false;
    }
    let b = s as *const u8;
    let mut i = 0usize;
    while i < lit.len() {
        if unsafe { *b.add(i) } != lit[i] {
            return false;
        }
        i += 1;
    }
    unsafe { *b.add(i) == 0 }
}

/// strcmp ordering between a C string and a byte literal.
unsafe fn cstr_cmp(s: *const c_char, lit: &[u8]) -> core::cmp::Ordering {
    let b = s as *const u8;
    let mut i = 0usize;
    loop {
        let c = if i < lit.len() { lit[i] } else { 0 };
        let sc = unsafe { *b.add(i) };
        if sc < c {
            return core::cmp::Ordering::Less;
        }
        if sc > c {
            return core::cmp::Ordering::Greater;
        }
        if sc == 0 {
            return core::cmp::Ordering::Equal;
        }
        i += 1;
    }
}

/// Append a NUL-terminated C string's bytes to `v`.
unsafe fn append_cstr(v: &mut Vec<u8>, s: *const c_char) {
    if s.is_null() {
        return;
    }
    let b = s as *const u8;
    let mut i = 0usize;
    loop {
        let c = unsafe { *b.add(i) };
        if c == 0 {
            break;
        }
        v.push(c);
        i += 1;
    }
}

/// Append an xmlChar (byte) string to `v`.
unsafe fn append_xmlstr(v: &mut Vec<u8>, s: *const xmlChar) {
    if s.is_null() {
        return;
    }
    let b = s as *const u8;
    let mut i = 0usize;
    loop {
        let c = unsafe { *b.add(i) };
        if c == 0 {
            break;
        }
        v.push(c);
        i += 1;
    }
}

/// strcat a byte literal onto a C string buffer.
unsafe fn cstr_cat_lit(buf: *mut c_char, lit: &[u8]) {
    if buf.is_null() {
        return;
    }
    let mut i = cstr_len(buf);
    let b = buf as *mut u8;
    for &c in lit {
        unsafe {
            *b.add(i) = c;
        }
        i += 1;
    }
    unsafe {
        *b.add(i) = 0;
    }
}

/// strcat an xmlChar string onto a C string buffer.
unsafe fn cstr_cat_xmlstr(buf: *mut c_char, s: *const xmlChar) {
    if s.is_null() {
        return;
    }
    let mut i = cstr_len(buf);
    let mut j = 0usize;
    let b = buf as *mut u8;
    let sb = s as *const u8;
    loop {
        let c = unsafe { *sb.add(j) };
        if c == 0 {
            break;
        }
        unsafe {
            *b.add(i) = c;
        }
        i += 1;
        j += 1;
    }
    unsafe {
        *b.add(i) = 0;
    }
}

/// Membership test over a merged short+long range list (upstream
/// `xmlCharInRange` semantics; short and long ranges are disjoint so a
/// linear scan over the merged list is equivalent).
fn ucs_in_ranges(code: c_int, ranges: &[(u32, u32)]) -> c_int {
    let v = code as u32;
    for &(lo, hi) in ranges {
        if v >= lo && v <= hi {
            return 1;
        }
    }
    0
}

/// Append an integer in decimal to `v`.
fn append_int(v: &mut Vec<u8>, n: i32) {
    let mut x = n as i64;
    if x < 0 {
        v.push(b'-');
        x = -x;
    }
    let mut buf = [0u8; 20];
    let mut i = 0usize;
    loop {
        buf[i] = b'0' + (x % 10) as u8;
        i += 1;
        x /= 10;
        if x == 0 {
            break;
        }
    }
    while i > 0 {
        i -= 1;
        v.push(buf[i]);
    }
}

/// Upper-case hex digit for a nibble.
fn hex_digit(v: u8) -> u8 {
    if v < 10 {
        b'0' + v
    } else {
        b'A' + v - 10
    }
}

/// Emit `text` (without NUL) through the generic error channel.
unsafe fn chan_emit(channel: xmlGenericErrorFunc, data: *mut c_void, text: &[u8]) {
    let mut b = text.to_vec();
    b.push(0);
    channel(data, b.as_ptr() as *const c_char);
}

// ═══════════════════════════════════════════════════════════════════════════════
// 1. getset family
// ═══════════════════════════════════════════════════════════════════════════════

/// The deprecated global buffer allocation scheme.
///
/// Upstream 2.13+ removed per-buffer allocation schemes; the getter always
/// returns `XML_BUFFER_ALLOC_EXACT` and the setter is a no-op. The candidate
/// keeps a process-global slot (defaulting to `XML_BUFFER_ALLOC_EXACT` = 1)
/// so the get/set pair round-trips while matching the upstream default.
static XML_BUFFER_ALLOC_SCHEME: AtomicI32 = AtomicI32::new(XML_BUFFER_ALLOC_EXACT as c_int);

/// # UPSTREAM-PARITY
///
/// ```c
/// xmlBufferAllocationScheme xmlGetBufferAllocationScheme(void);
/// ```
#[no_mangle]
pub extern "C" fn xmlGetBufferAllocationScheme() -> xmlBufferAllocationScheme {
    match XML_BUFFER_ALLOC_SCHEME.load(Ordering::Relaxed) {
        v if v == XML_BUFFER_ALLOC_DOUBLEIT as c_int => {
            xmlBufferAllocationScheme::XML_BUFFER_ALLOC_DOUBLEIT
        }
        v if v == XML_BUFFER_ALLOC_EXACT as c_int => {
            xmlBufferAllocationScheme::XML_BUFFER_ALLOC_EXACT
        }
        v if v == XML_BUFFER_ALLOC_IMMUTABLE as c_int => {
            xmlBufferAllocationScheme::XML_BUFFER_ALLOC_IMMUTABLE
        }
        v if v == XML_BUFFER_ALLOC_IO as c_int => xmlBufferAllocationScheme::XML_BUFFER_ALLOC_IO,
        v if v == XML_BUFFER_ALLOC_HYBRID as c_int => {
            xmlBufferAllocationScheme::XML_BUFFER_ALLOC_HYBRID
        }
        _ => xmlBufferAllocationScheme::XML_BUFFER_ALLOC_BOUNDED,
    }
}

/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlSetBufferAllocationScheme(xmlBufferAllocationScheme scheme);
/// ```
#[no_mangle]
pub extern "C" fn xmlSetBufferAllocationScheme(scheme: xmlBufferAllocationScheme) {
    XML_BUFFER_ALLOC_SCHEME.store(scheme as c_int, Ordering::Relaxed);
}

/// The legacy feature-name list (upstream features.c `xmlFeaturesList[]`,
/// 42 entries, as exported by the oracle DSO).
static XML_FEATURES: [&[u8]; 42] = [
    b"validate\0",
    b"load subset\0",
    b"keep blanks\0",
    b"disable SAX\0",
    b"fetch external entities\0",
    b"substitute entities\0",
    b"gather line info\0",
    b"user data\0",
    b"is html\0",
    b"is standalone\0",
    b"stop parser\0",
    b"document\0",
    b"is well formed\0",
    b"is valid\0",
    b"SAX block\0",
    b"SAX function internalSubset\0",
    b"SAX function isStandalone\0",
    b"SAX function hasInternalSubset\0",
    b"SAX function hasExternalSubset\0",
    b"SAX function resolveEntity\0",
    b"SAX function getEntity\0",
    b"SAX function entityDecl\0",
    b"SAX function notationDecl\0",
    b"SAX function attributeDecl\0",
    b"SAX function elementDecl\0",
    b"SAX function unparsedEntityDecl\0",
    b"SAX function setDocumentLocator\0",
    b"SAX function startDocument\0",
    b"SAX function endDocument\0",
    b"SAX function startElement\0",
    b"SAX function endElement\0",
    b"SAX function reference\0",
    b"SAX function characters\0",
    b"SAX function ignorableWhitespace\0",
    b"SAX function processingInstruction\0",
    b"SAX function comment\0",
    b"SAX function warning\0",
    b"SAX function error\0",
    b"SAX function fatalError\0",
    b"SAX function getParameterEntity\0",
    b"SAX function cdataBlock\0",
    b"SAX function externalSubset\0",
];

/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlGetFeaturesList(int *len, const char **result);
/// ```
///
/// Legacy API (features.c, removed from source but still exported). Copies
/// up to `*len` feature names into `result` and returns the total number of
/// features (42). Returns -1 when `*len` is larger than 999.
#[no_mangle]
pub unsafe extern "C" fn xmlGetFeaturesList(len: *mut c_int, result: *mut *const c_char) -> c_int {
    let n = XML_FEATURES.len() as c_int;
    if len.is_null() || result.is_null() {
        return n;
    }
    let cur = unsafe { *len };
    if cur > 999 {
        return -1;
    }
    let mut cnt = cur;
    if cnt > n {
        cnt = n;
        unsafe {
            *len = n;
        }
    }
    if cnt > 0 {
        for i in 0..cnt as usize {
            unsafe {
                *result.add(i) = XML_FEATURES[i].as_ptr() as *const c_char;
            }
        }
    }
    n
}

/// Byte offset of each SAX function slot inside `_xmlSAXHandler`
/// (upstream features.c reads `ctxt->sax-><slot>`; the candidate struct
/// is `#[repr(C)]` with the same field order, so the offsets match).
const SAX_FN_OFFSETS: [(&[u8], usize); 27] = [
    (b"SAX function internalSubset", 0x00),
    (b"SAX function isStandalone", 0x08),
    (b"SAX function hasInternalSubset", 0x10),
    (b"SAX function hasExternalSubset", 0x18),
    (b"SAX function resolveEntity", 0x20),
    (b"SAX function getEntity", 0x28),
    (b"SAX function entityDecl", 0x30),
    (b"SAX function notationDecl", 0x38),
    (b"SAX function attributeDecl", 0x40),
    (b"SAX function elementDecl", 0x48),
    (b"SAX function unparsedEntityDecl", 0x50),
    (b"SAX function setDocumentLocator", 0x58),
    (b"SAX function startDocument", 0x60),
    (b"SAX function endDocument", 0x68),
    (b"SAX function startElement", 0x70),
    (b"SAX function endElement", 0x78),
    (b"SAX function reference", 0x80),
    (b"SAX function characters", 0x88),
    (b"SAX function ignorableWhitespace", 0x90),
    (b"SAX function processingInstruction", 0x98),
    (b"SAX function comment", 0xa0),
    (b"SAX function warning", 0xa8),
    (b"SAX function error", 0xb0),
    (b"SAX function fatalError", 0xb8),
    (b"SAX function getParameterEntity", 0xc0),
    (b"SAX function cdataBlock", 0xc8),
    (b"SAX function externalSubset", 0xd0),
];

/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlGetFeature(xmlParserCtxtPtr ctxt, const char *name, void *result);
/// ```
///
/// Legacy feature getter (features.c). Returns 0 and stores the feature
/// value at `result`, or -1 on error / unknown feature.
#[no_mangle]
pub unsafe extern "C" fn xmlGetFeature(
    ctxt: *mut _xmlParserCtxt,
    name: *const c_char,
    result: *mut c_void,
) -> c_int {
    if name.is_null() || result.is_null() || ctxt.is_null() {
        return -1;
    }
    let c = unsafe { &*ctxt };
    if unsafe { cstr_eq(name, b"validate") } {
        unsafe { *(result as *mut c_int) = c.validate };
        return 0;
    }
    if unsafe { cstr_eq(name, b"keep blanks") } {
        unsafe { *(result as *mut c_int) = c.keepBlanks };
        return 0;
    }
    if unsafe { cstr_eq(name, b"disable SAX") } {
        unsafe { *(result as *mut c_int) = c.disableSAX };
        return 0;
    }
    if unsafe { cstr_eq(name, b"fetch external entities") } {
        unsafe { *(result as *mut c_int) = c.loadsubset };
        return 0;
    }
    if unsafe { cstr_eq(name, b"substitute entities") } {
        unsafe { *(result as *mut c_int) = c.replaceEntities };
        return 0;
    }
    if unsafe { cstr_eq(name, b"gather line info") } {
        unsafe { *(result as *mut c_int) = c.record_info };
        return 0;
    }
    if unsafe { cstr_eq(name, b"user data") } {
        unsafe { *(result as *mut *mut c_void) = c.userData };
        return 0;
    }
    if unsafe { cstr_eq(name, b"is html") } {
        unsafe { *(result as *mut c_int) = c.html };
        return 0;
    }
    if unsafe { cstr_eq(name, b"is standalone") } {
        unsafe { *(result as *mut c_int) = c.standalone };
        return 0;
    }
    if unsafe { cstr_eq(name, b"document") } {
        unsafe { *(result as *mut *mut _xmlDoc) = c.myDoc };
        return 0;
    }
    if unsafe { cstr_eq(name, b"is well formed") } {
        unsafe { *(result as *mut c_int) = c.wellFormed };
        return 0;
    }
    if unsafe { cstr_eq(name, b"is valid") } {
        unsafe { *(result as *mut c_int) = c.valid };
        return 0;
    }
    if unsafe { cstr_eq(name, b"SAX block") } {
        unsafe { *(result as *mut *mut _xmlSAXHandler) = c.sax };
        return 0;
    }
    for &(feat, off) in &SAX_FN_OFFSETS {
        if unsafe { cstr_eq(name, feat) } {
            if c.sax.is_null() {
                return -1;
            }
            let slot = (c.sax as *mut u8).add(off) as *mut *mut c_void;
            unsafe { *(result as *mut *mut c_void) = *slot };
            return 0;
        }
    }
    -1
}

/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlSetFeature(xmlParserCtxtPtr ctxt, const char *name, void *value);
/// ```
///
/// Legacy feature setter (features.c). Returns 0 on success, -1 on error /
/// unknown feature. Enabling "validate" also wires up the default validity
/// error/warning handlers on the context's validation context.
#[no_mangle]
pub unsafe extern "C" fn xmlSetFeature(
    ctxt: *mut _xmlParserCtxt,
    name: *const c_char,
    value: *mut c_void,
) -> c_int {
    if name.is_null() || value.is_null() || ctxt.is_null() {
        return -1;
    }
    let c = unsafe { &mut *ctxt };
    if unsafe { cstr_eq(name, b"validate") } {
        let val = unsafe { *(value as *const c_int) };
        if c.validate == 0 && val != 0 {
            if c.vctxt.warning.is_none() {
                c.vctxt.warning = Some(crate::xml::errors::xmlParserValidityWarning);
            }
            if c.vctxt.error.is_none() {
                c.vctxt.error = Some(crate::xml::errors::xmlParserValidityError);
            }
            c.vctxt.valid = 0;
        }
        c.validate = val;
        return 0;
    }
    if unsafe { cstr_eq(name, b"keep blanks") } {
        c.keepBlanks = unsafe { *(value as *const c_int) };
        return 0;
    }
    if unsafe { cstr_eq(name, b"disable SAX") } {
        c.disableSAX = unsafe { *(value as *const c_int) };
        return 0;
    }
    if unsafe { cstr_eq(name, b"fetch external entities") } {
        c.loadsubset = unsafe { *(value as *const c_int) };
        return 0;
    }
    if unsafe { cstr_eq(name, b"substitute entities") } {
        c.replaceEntities = unsafe { *(value as *const c_int) };
        return 0;
    }
    if unsafe { cstr_eq(name, b"gather line info") } {
        c.record_info = unsafe { *(value as *const c_int) };
        return 0;
    }
    if unsafe { cstr_eq(name, b"user data") } {
        c.userData = unsafe { *(value as *const *mut c_void) };
        return 0;
    }
    if unsafe { cstr_eq(name, b"is html") } {
        c.html = unsafe { *(value as *const c_int) };
        return 0;
    }
    if unsafe { cstr_eq(name, b"is standalone") } {
        c.standalone = unsafe { *(value as *const c_int) };
        return 0;
    }
    if unsafe { cstr_eq(name, b"document") } {
        c.myDoc = unsafe { *(value as *const *mut _xmlDoc) };
        return 0;
    }
    if unsafe { cstr_eq(name, b"is well formed") } {
        c.wellFormed = unsafe { *(value as *const c_int) };
        return 0;
    }
    if unsafe { cstr_eq(name, b"is valid") } {
        c.valid = unsafe { *(value as *const c_int) };
        return 0;
    }
    if unsafe { cstr_eq(name, b"SAX block") } {
        c.sax = unsafe { *(value as *const *mut _xmlSAXHandler) };
        return 0;
    }
    for &(feat, off) in &SAX_FN_OFFSETS {
        if unsafe { cstr_eq(name, feat) } {
            if c.sax.is_null() {
                return -1;
            }
            let slot = (c.sax as *mut u8).add(off) as *mut *mut c_void;
            unsafe { *slot = *(value as *const *mut c_void) };
            return 0;
        }
    }
    -1
}

/// # UPSTREAM-PARITY
///
/// ```c
/// xmlGlobalStatePtr xmlGetGlobalState(void);
/// ```
///
/// Deprecated (globals.c): returns the global state, which the candidate
/// does not keep — the oracle DSO itself returns NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlGetGlobalState() -> *mut c_void {
    ptr::null_mut()
}

/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNodePtr xmlGetLastChild(const xmlNode *parent);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlGetLastChild(parent: *const _xmlNode) -> *mut _xmlNode {
    if parent.is_null() || unsafe { (*parent).type_ == XML_NAMESPACE_DECL as c_int } {
        return ptr::null_mut();
    }
    unsafe { (*parent).last }
}

/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlGetNoNsProp(const xmlNode *node, const xmlChar *name);
/// ```
///
/// Value of the no-namespace attribute `name` (with the DTD default/fixed
/// declaration fallback), or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlGetNoNsProp(
    node: *const _xmlNode,
    name: *const xmlChar,
) -> *mut xmlChar {
    if node.is_null() || unsafe { (*node).type_ != XML_ELEMENT_NODE as c_int } || name.is_null() {
        return ptr::null_mut();
    }
    let mut prop = unsafe { (*node).properties };
    while !prop.is_null() {
        if unsafe { (*prop).ns.is_null() }
            && unsafe { crate::abi::exports_xml2::xmlStrEqual((*prop).name, name) != 0 }
        {
            return get_prop_value(prop);
        }
        prop = unsafe { (*prop).next };
    }
    dtd_default_attr(node, name, ptr::null())
}

/// The DTD default/fixed attribute declaration fallback of upstream
/// `xmlGetPropNodeInternal` (useDTD == 1).
unsafe fn dtd_default_attr(
    node: *const _xmlNode,
    name: *const xmlChar,
    ns_uri: *const xmlChar,
) -> *mut xmlChar {
    let doc = unsafe { (*node).doc };
    if doc.is_null() || unsafe { (*doc).intSubset.is_null() } {
        return ptr::null_mut();
    }
    // Build the element QName for the DTD lookup.
    let mut tmp: *mut xmlChar = ptr::null_mut();
    let elem_qname: *const xmlChar;
    let ns = unsafe { (*node).ns };
    if !ns.is_null() && !unsafe { (*ns).prefix.is_null() } {
        tmp = unsafe { crate::abi::exports_xml2::xmlStrdup((*ns).prefix) };
        if !tmp.is_null() {
            let colon = b":\0";
            tmp = unsafe {
                crate::abi::exports_xml2::xmlStrcat(tmp, colon.as_ptr() as *const xmlChar)
            };
        }
        if !tmp.is_null() {
            tmp = unsafe { crate::abi::exports_xml2::xmlStrcat(tmp, (*node).name) };
        }
        if tmp.is_null() {
            return ptr::null_mut();
        }
        elem_qname = tmp;
    } else {
        elem_qname = unsafe { (*node).name };
    }

    let mut attr_decl: *mut _xmlAttribute = ptr::null_mut();
    let doc = unsafe { &*doc };
    let xml_ns = b"http://www.w3.org/XML/1998/namespace\0";
    if ns_uri.is_null() {
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
        crate::abi::exports_xml2::xmlStrEqual(ns_uri, xml_ns.as_ptr() as *const xmlChar) != 0
    } {
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
        // corresponding to ns_uri.
        let ns_list = crate::xml::tree::get_ns_list((*node).doc, node as *mut _xmlNode);
        if ns_list.is_null() {
            if !tmp.is_null() {
                unsafe { xmlFree(tmp as *mut c_void) };
            }
            return ptr::null_mut();
        }
        let mut cur = ns_list;
        while !unsafe { *cur }.is_null() {
            let n = unsafe { *cur };
            if !unsafe { (*n).href }.is_null()
                && unsafe { crate::abi::exports_xml2::xmlStrEqual((*n).href, ns_uri) != 0 }
            {
                attr_decl = crate::xml::validation::get_dtd_qattr_desc(
                    doc.intSubset,
                    elem_qname,
                    name,
                    (*n).prefix,
                );
                if attr_decl.is_null() && !doc.extSubset.is_null() {
                    attr_decl = crate::xml::validation::get_dtd_qattr_desc(
                        doc.extSubset,
                        elem_qname,
                        name,
                        (*n).prefix,
                    );
                }
                if !attr_decl.is_null() {
                    break;
                }
            }
            cur = cur.add(1);
        }
        unsafe { xmlFree(ns_list as *mut c_void) };
    }
    if !tmp.is_null() {
        unsafe { xmlFree(tmp as *mut c_void) };
    }

    if !attr_decl.is_null() && !unsafe { (*attr_decl).defaultValue.is_null() } {
        return unsafe { crate::abi::exports_xml2::xmlStrdup((*attr_decl).defaultValue) };
    }
    ptr::null_mut()
}

/// Value of an attribute node (upstream `xmlGetPropNodeValueInternal`):
/// content of the attribute's children for attribute nodes, the default
/// value for attribute declarations.
unsafe fn get_prop_value(prop: *mut _xmlAttr) -> *mut xmlChar {
    if prop.is_null() {
        return ptr::null_mut();
    }
    if unsafe { (*prop).type_ == XML_ATTRIBUTE_NODE as c_int } {
        unsafe { crate::xml::tree::node_get_content(prop as *mut _xmlNode) }
    } else if unsafe { (*prop).type_ == XML_ATTRIBUTE_DECL as c_int } {
        let a = prop as *mut _xmlAttribute;
        unsafe { crate::abi::exports_xml2::xmlStrdup((*a).defaultValue) }
    } else {
        ptr::null_mut()
    }
}

/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlGetNodePath(const xmlNode *node);
/// ```
///
/// Build an XPath-like path for `node` (tree.c `xmlGetNodePath`), or NULL
/// on error. The result is allocated with xmlMalloc.
#[no_mangle]
pub unsafe extern "C" fn xmlGetNodePath(node: *const _xmlNode) -> *mut xmlChar {
    if node.is_null() || unsafe { (*node).type_ == XML_NAMESPACE_DECL as c_int } {
        return ptr::null_mut();
    }
    unsafe {
        // Collect the ancestor chain (node, parent, ..., root).
        let mut num_nodes = 0usize;
        let mut cur: *const _xmlNode = node;
        while !cur.is_null() {
            num_nodes += 1;
            cur = (*cur).parent;
        }
        let mut nodes: Vec<*const _xmlNode> = Vec::with_capacity(num_nodes);
        cur = node;
        while !cur.is_null() && nodes.len() < num_nodes {
            nodes.push(cur);
            cur = (*cur).parent;
        }

        let mut out: Vec<u8> = Vec::new();
        let mut i = nodes.len();
        while i > 0 {
            let mut occur: i32 = 0;
            i -= 1;
            let cur = nodes[i];
            let t = (*cur).type_;

            if t == XML_DOCUMENT_NODE as c_int || t == XML_HTML_DOCUMENT_NODE as c_int {
                if i == 0 {
                    out.push(b'/');
                }
            } else if t == XML_ELEMENT_NODE as c_int {
                let mut generic = 0;
                out.push(b'/');
                let ns = (*cur).ns;
                if !ns.is_null() {
                    if !(*ns).prefix.is_null() {
                        append_xmlstr(&mut out, (*ns).prefix);
                        out.push(b':');
                        append_xmlstr(&mut out, (*cur).name);
                    } else {
                        // Cannot express named elements in the default
                        // namespace, so use "*".
                        generic = 1;
                        out.push(b'*');
                    }
                } else {
                    append_xmlstr(&mut out, (*cur).name);
                }
                // Thumbler index computation.
                let mut tmp = (*cur).prev;
                while !tmp.is_null() {
                    if (*tmp).type_ == XML_ELEMENT_NODE as c_int
                        && (generic != 0
                            || (crate::abi::exports_xml2::xmlStrEqual((*cur).name, (*tmp).name)
                                != 0
                                && ((*tmp).ns == ns
                                    || (!(*tmp).ns.is_null()
                                        && !ns.is_null()
                                        && crate::abi::exports_xml2::xmlStrEqual(
                                            (*ns).prefix,
                                            (*(*tmp).ns).prefix,
                                        ) != 0))))
                    {
                        occur += 1;
                    }
                    tmp = (*tmp).prev;
                }
                if occur == 0 {
                    tmp = (*cur).next;
                    while !tmp.is_null() && occur == 0 {
                        if (*tmp).type_ == XML_ELEMENT_NODE as c_int
                            && (generic != 0
                                || (crate::abi::exports_xml2::xmlStrEqual(
                                    (*cur).name,
                                    (*tmp).name,
                                ) != 0
                                    && ((*tmp).ns == ns
                                        || (!(*tmp).ns.is_null()
                                            && !ns.is_null()
                                            && crate::abi::exports_xml2::xmlStrEqual(
                                                (*ns).prefix,
                                                (*(*tmp).ns).prefix,
                                            ) != 0))))
                        {
                            occur += 1;
                        }
                        tmp = (*tmp).next;
                    }
                    if occur != 0 {
                        occur = 1;
                    }
                } else {
                    occur += 1;
                }
            } else if t == XML_COMMENT_NODE as c_int {
                out.extend_from_slice(b"/comment()");
                let mut tmp = (*cur).prev;
                while !tmp.is_null() {
                    if (*tmp).type_ == XML_COMMENT_NODE as c_int {
                        occur += 1;
                    }
                    tmp = (*tmp).prev;
                }
                if occur == 0 {
                    tmp = (*cur).next;
                    while !tmp.is_null() && occur == 0 {
                        if (*tmp).type_ == XML_COMMENT_NODE as c_int {
                            occur += 1;
                        }
                        tmp = (*tmp).next;
                    }
                    if occur != 0 {
                        occur = 1;
                    }
                } else {
                    occur += 1;
                }
            } else if t == XML_TEXT_NODE as c_int || t == XML_CDATA_SECTION_NODE as c_int {
                out.extend_from_slice(b"/text()");
                let mut tmp = (*cur).prev;
                while !tmp.is_null() {
                    if (*tmp).type_ == XML_TEXT_NODE as c_int
                        || (*tmp).type_ == XML_CDATA_SECTION_NODE as c_int
                    {
                        occur += 1;
                    }
                    tmp = (*tmp).prev;
                }
                if occur == 0 {
                    tmp = (*cur).next;
                    while !tmp.is_null() {
                        if (*tmp).type_ == XML_TEXT_NODE as c_int
                            || (*tmp).type_ == XML_CDATA_SECTION_NODE as c_int
                        {
                            occur = 1;
                            break;
                        }
                        tmp = (*tmp).next;
                    }
                } else {
                    occur += 1;
                }
            } else if t == XML_PI_NODE as c_int {
                out.extend_from_slice(b"/processing-instruction('");
                append_xmlstr(&mut out, (*cur).name);
                out.extend_from_slice(b"')");
                let mut tmp = (*cur).prev;
                while !tmp.is_null() {
                    if (*tmp).type_ == XML_PI_NODE as c_int
                        && crate::abi::exports_xml2::xmlStrEqual((*cur).name, (*tmp).name) != 0
                    {
                        occur += 1;
                    }
                    tmp = (*tmp).prev;
                }
                if occur == 0 {
                    tmp = (*cur).next;
                    while !tmp.is_null() && occur == 0 {
                        if (*tmp).type_ == XML_PI_NODE as c_int
                            && crate::abi::exports_xml2::xmlStrEqual((*cur).name, (*tmp).name) != 0
                        {
                            occur += 1;
                        }
                        tmp = (*tmp).next;
                    }
                    if occur != 0 {
                        occur = 1;
                    }
                } else {
                    occur += 1;
                }
            } else if t == XML_ATTRIBUTE_NODE as c_int {
                out.extend_from_slice(b"/@");
                let ns = (*cur).ns;
                if !ns.is_null() && !(*ns).prefix.is_null() {
                    append_xmlstr(&mut out, (*ns).prefix);
                    out.push(b':');
                }
                append_xmlstr(&mut out, (*cur).name);
            } else {
                return ptr::null_mut();
            }

            if occur > 0 {
                out.push(b'[');
                append_int(&mut out, occur);
                out.push(b']');
            }
        }

        out.push(0);
        let ret = xmlMalloc(out.len());
        if ret.is_null() {
            return ptr::null_mut();
        }
        ptr::copy_nonoverlapping(out.as_ptr(), ret as *mut u8, out.len());
        ret as *mut xmlChar
    }
}

/// The five XML predefined entities, mirroring upstream entities.c static
/// `xmlEntityLt`/`xmlEntityGt`/`xmlEntityAmp`/`xmlEntityQuot`/`xmlEntityApos`.
struct SyncEntity(*const _xmlEntity);
unsafe impl Sync for SyncEntity {}

static PREDEFINED_LT_DATA: _xmlEntity = _xmlEntity {
    _private: ptr::null_mut(),
    type_: XML_ENTITY_DECL as c_int,
    name: b"lt\0" as *const u8 as *const xmlChar,
    children: ptr::null_mut(),
    last: ptr::null_mut(),
    parent: ptr::null_mut(),
    next: ptr::null_mut(),
    prev: ptr::null_mut(),
    doc: ptr::null_mut(),
    orig: ptr::null_mut(),
    content: b"<\0" as *const u8 as *mut xmlChar,
    length: 1,
    etype: XML_INTERNAL_PREDEFINED_ENTITY as c_int,
    ExternalID: ptr::null(),
    SystemID: ptr::null(),
    nexte: ptr::null_mut(),
    URI: ptr::null(),
    owner: 0,
    flags: 0,
    expandedSize: 0,
};
static PREDEFINED_GT_DATA: _xmlEntity = _xmlEntity {
    _private: ptr::null_mut(),
    type_: XML_ENTITY_DECL as c_int,
    name: b"gt\0" as *const u8 as *const xmlChar,
    children: ptr::null_mut(),
    last: ptr::null_mut(),
    parent: ptr::null_mut(),
    next: ptr::null_mut(),
    prev: ptr::null_mut(),
    doc: ptr::null_mut(),
    orig: ptr::null_mut(),
    content: b">\0" as *const u8 as *mut xmlChar,
    length: 1,
    etype: XML_INTERNAL_PREDEFINED_ENTITY as c_int,
    ExternalID: ptr::null(),
    SystemID: ptr::null(),
    nexte: ptr::null_mut(),
    URI: ptr::null(),
    owner: 0,
    flags: 0,
    expandedSize: 0,
};
static PREDEFINED_AMP_DATA: _xmlEntity = _xmlEntity {
    _private: ptr::null_mut(),
    type_: XML_ENTITY_DECL as c_int,
    name: b"amp\0" as *const u8 as *const xmlChar,
    children: ptr::null_mut(),
    last: ptr::null_mut(),
    parent: ptr::null_mut(),
    next: ptr::null_mut(),
    prev: ptr::null_mut(),
    doc: ptr::null_mut(),
    orig: ptr::null_mut(),
    content: b"&\0" as *const u8 as *mut xmlChar,
    length: 1,
    etype: XML_INTERNAL_PREDEFINED_ENTITY as c_int,
    ExternalID: ptr::null(),
    SystemID: ptr::null(),
    nexte: ptr::null_mut(),
    URI: ptr::null(),
    owner: 0,
    flags: 0,
    expandedSize: 0,
};
static PREDEFINED_QUOT_DATA: _xmlEntity = _xmlEntity {
    _private: ptr::null_mut(),
    type_: XML_ENTITY_DECL as c_int,
    name: b"quot\0" as *const u8 as *const xmlChar,
    children: ptr::null_mut(),
    last: ptr::null_mut(),
    parent: ptr::null_mut(),
    next: ptr::null_mut(),
    prev: ptr::null_mut(),
    doc: ptr::null_mut(),
    orig: ptr::null_mut(),
    content: b"\"\0" as *const u8 as *mut xmlChar,
    length: 1,
    etype: XML_INTERNAL_PREDEFINED_ENTITY as c_int,
    ExternalID: ptr::null(),
    SystemID: ptr::null(),
    nexte: ptr::null_mut(),
    URI: ptr::null(),
    owner: 0,
    flags: 0,
    expandedSize: 0,
};
static PREDEFINED_APOS_DATA: _xmlEntity = _xmlEntity {
    _private: ptr::null_mut(),
    type_: XML_ENTITY_DECL as c_int,
    name: b"apos\0" as *const u8 as *const xmlChar,
    children: ptr::null_mut(),
    last: ptr::null_mut(),
    parent: ptr::null_mut(),
    next: ptr::null_mut(),
    prev: ptr::null_mut(),
    doc: ptr::null_mut(),
    orig: ptr::null_mut(),
    content: b"'\0" as *const u8 as *mut xmlChar,
    length: 1,
    etype: XML_INTERNAL_PREDEFINED_ENTITY as c_int,
    ExternalID: ptr::null(),
    SystemID: ptr::null(),
    nexte: ptr::null_mut(),
    URI: ptr::null(),
    owner: 0,
    flags: 0,
    expandedSize: 0,
};

static PREDEFINED_LT: SyncEntity = SyncEntity(&PREDEFINED_LT_DATA as *const _xmlEntity);
static PREDEFINED_GT: SyncEntity = SyncEntity(&PREDEFINED_GT_DATA as *const _xmlEntity);
static PREDEFINED_AMP: SyncEntity = SyncEntity(&PREDEFINED_AMP_DATA as *const _xmlEntity);
static PREDEFINED_QUOT: SyncEntity = SyncEntity(&PREDEFINED_QUOT_DATA as *const _xmlEntity);
static PREDEFINED_APOS: SyncEntity = SyncEntity(&PREDEFINED_APOS_DATA as *const _xmlEntity);

/// # UPSTREAM-PARITY
///
/// ```c
/// xmlEntityPtr xmlGetPredefinedEntity(const xmlChar *name);
/// ```
///
/// Returns a pointer to the static predefined entity, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlGetPredefinedEntity(name: *const xmlChar) -> *mut _xmlEntity {
    if name.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        if crate::abi::exports_xml2::xmlStrEqual(name, b"lt\0" as *const u8 as *const xmlChar) != 0
        {
            return PREDEFINED_LT.0 as *mut _xmlEntity;
        }
        if crate::abi::exports_xml2::xmlStrEqual(name, b"gt\0" as *const u8 as *const xmlChar) != 0
        {
            return PREDEFINED_GT.0 as *mut _xmlEntity;
        }
        if crate::abi::exports_xml2::xmlStrEqual(name, b"amp\0" as *const u8 as *const xmlChar) != 0
        {
            return PREDEFINED_AMP.0 as *mut _xmlEntity;
        }
        if crate::abi::exports_xml2::xmlStrEqual(name, b"quot\0" as *const u8 as *const xmlChar)
            != 0
        {
            return PREDEFINED_QUOT.0 as *mut _xmlEntity;
        }
        if crate::abi::exports_xml2::xmlStrEqual(name, b"apos\0" as *const u8 as *const xmlChar)
            != 0
        {
            return PREDEFINED_APOS.0 as *mut _xmlEntity;
        }
    }
    ptr::null_mut()
}

/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlGetThreadId(void);
/// ```
///
/// Upstream returns `pthread_self()` (or 0 when single-threaded); the
/// candidate is single-threaded, so 0 matches the oracle's common path.
#[no_mangle]
pub extern "C" fn xmlGetThreadId() -> c_int {
    0
}

/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlGetUTF8Char(const unsigned char *utf, int *len);
/// ```
///
/// Decode the UTF-8 character at `utf`, set `*len` to its byte length and
/// return the code point; -1 (and `*len = 0`) on error.
#[no_mangle]
pub unsafe extern "C" fn xmlGetUTF8Char(utf: *const u8, len: *mut c_int) -> c_int {
    unsafe {
        if utf.is_null() || len.is_null() {
            if !len.is_null() {
                *len = 0;
            }
            return -1;
        }
        let mut c: u32 = *utf as u32;
        if c < 0x80 {
            if *len < 1 {
                *len = 0;
                return -1;
            }
            *len = 1;
        } else {
            if *len < 2 || (*utf.add(1) & 0xc0) != 0x80 {
                *len = 0;
                return -1;
            }
            if c < 0xe0 {
                if c < 0xc2 {
                    *len = 0;
                    return -1;
                }
                *len = 2;
                c = (c & 0x1f) << 6;
                c |= (*utf.add(1) & 0x3f) as u32;
            } else {
                if *len < 3 || (*utf.add(2) & 0xc0) != 0x80 {
                    *len = 0;
                    return -1;
                }
                if c < 0xf0 {
                    *len = 3;
                    c = (c & 0x0f) << 12;
                    c |= ((*utf.add(1) & 0x3f) as u32) << 6;
                    c |= (*utf.add(2) & 0x3f) as u32;
                    if c < 0x800 || (c >= 0xd800 && c < 0xe000) {
                        *len = 0;
                        return -1;
                    }
                } else {
                    if *len < 4 || (*utf.add(3) & 0xc0) != 0x80 {
                        *len = 0;
                        return -1;
                    }
                    *len = 4;
                    c = (c & 0x07) << 18;
                    c |= ((*utf.add(1) & 0x3f) as u32) << 12;
                    c |= ((*utf.add(2) & 0x3f) as u32) << 6;
                    c |= (*utf.add(3) & 0x3f) as u32;
                    if c < 0x10000 || c >= 0x110000 {
                        *len = 0;
                        return -1;
                    }
                }
            }
        }
        c as c_int
    }
}

/// Type of the entity-reference callback (upstream entities.h
/// `xmlEntityReferenceFunc`).
pub type xmlEntityReferenceFunc = unsafe extern "C" fn(
    entity: *mut _xmlEntity,
    firstChild: *mut _xmlNode,
    lastChild: *mut _xmlNode,
);

/// The global entity-reference callback (legacy entities.c global; the
/// candidate's parser does not invoke it, mirroring modern upstream where
/// `xmlSetEntityReferenceFunc` is a no-op).
static ENTITY_REFERENCE_FUNC: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlSetEntityReferenceFunc(xmlEntityReferenceFunc func);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlSetEntityReferenceFunc(func: Option<xmlEntityReferenceFunc>) {
    ENTITY_REFERENCE_FUNC.store(
        func.map_or(ptr::null_mut(), |f| f as *const c_void as *mut c_void),
        Ordering::Relaxed,
    );
}

/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlSetListDoc(xmlNode *list, xmlDoc *doc);
/// ```
///
/// Set `doc` on every node of the sibling list (tree.c). Returns 0, or -1
/// if a subtree assignment failed.
#[no_mangle]
pub unsafe extern "C" fn xmlSetListDoc(list: *mut _xmlNode, doc: *mut _xmlDoc) -> c_int {
    if list.is_null() || unsafe { (*list).type_ == XML_NAMESPACE_DECL as c_int } {
        return 0;
    }
    let mut ret = 0;
    let mut cur = list;
    while !cur.is_null() {
        if unsafe { (*cur).doc != doc } {
            if unsafe { crate::abi::exports_tree::xmlSetTreeDoc(cur, doc) } < 0 {
                ret = -1;
            }
        }
        cur = unsafe { (*cur).next };
    }
    ret
}

// ═══════════════════════════════════════════════════════════════════════════════
// 2. module family — xmlModule* (xmlmodule.c, dlopen/dlsym/dlclose)
// ═══════════════════════════════════════════════════════════════════════════════

/// Module handle (upstream `struct _xmlModule`, xmlmodule.c).
#[repr(C)]
pub struct _xmlModule {
    pub name: *mut xmlChar,
    pub handle: *mut c_void,
}

extern "C" {
    fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlclose(handle: *mut c_void) -> c_int;
    fn dlerror() -> *mut c_char;
}

// RTLD_NOW | RTLD_GLOBAL (glibc; matches upstream xmlModulePlatformOpen).
const RTLD_NOW: c_int = 2;
const RTLD_GLOBAL: c_int = 0x100;

/// # UPSTREAM-PARITY
///
/// ```c
/// xmlModulePtr xmlModuleOpen(const char *filename, int options);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlModuleOpen(
    filename: *const c_char,
    _options: c_int,
) -> *mut _xmlModule {
    let module = unsafe { xmlMallocZero(size_of::<_xmlModule>()) } as *mut _xmlModule;
    if module.is_null() {
        return ptr::null_mut();
    }
    let handle = unsafe { dlopen(filename, RTLD_GLOBAL | RTLD_NOW) };
    if handle.is_null() {
        unsafe { xmlFree(module as *mut c_void) };
        return ptr::null_mut();
    }
    unsafe {
        (*module).handle = handle;
        if !filename.is_null() {
            (*module).name = crate::abi::exports_xml2::xmlStrdup(filename as *const xmlChar);
        }
    }
    module
}

/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlModuleSymbol(xmlModule *module, const char *name, void **symbol);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlModuleSymbol(
    module: *mut _xmlModule,
    name: *const c_char,
    symbol: *mut *mut c_void,
) -> c_int {
    if module.is_null() || symbol.is_null() || name.is_null() {
        return -1;
    }
    unsafe {
        *symbol = dlsym((*module).handle, name);
        if !dlerror().is_null() {
            return -1;
        }
    }
    0
}

/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlModuleClose(xmlModule *module);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlModuleClose(module: *mut _xmlModule) -> c_int {
    if module.is_null() {
        return -1;
    }
    let rc = unsafe { dlclose((*module).handle) };
    if rc != 0 {
        return -2;
    }
    unsafe { xmlModuleFree(module) }
}

/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlModuleFree(xmlModule *module);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlModuleFree(module: *mut _xmlModule) -> c_int {
    if module.is_null() {
        return -1;
    }
    unsafe {
        if !(*module).name.is_null() {
            xmlFree((*module).name as *mut c_void);
        }
        xmlFree(module as *mut c_void);
    }
    0
}

// ═══════════════════════════════════════════════════════════════════════════════
// 3. ucs family — xmlUCSIsBlock / xmlUCSIsCat / xmlUCSIsCatCc
// ═══════════════════════════════════════════════════════════════════════════════

// Generated: UCS block/category name tables (archaeology libxml2-git
// codegen/unicode.inc). Each entry: (name, &[(lo, hi)]) sorted by name
// (upstream xmlUnicodeBlocks / xmlUnicodeCats).

#[rustfmt::skip]
static XML_UCS_BLOCKS: &[(&str, &[(u32, u32)])] = &[
    ("AegeanNumbers", &[(65792,65855)]),
    ("AlphabeticPresentationForms", &[(64256,64335)]),
    ("Arabic", &[(1536,1791)]),
    ("ArabicPresentationForms-A", &[(64336,65023)]),
    ("ArabicPresentationForms-B", &[(65136,65279)]),
    ("Armenian", &[(1328,1423)]),
    ("Arrows", &[(8592,8703)]),
    ("BasicLatin", &[(0,127)]),
    ("Bengali", &[(2432,2559)]),
    ("BlockElements", &[(9600,9631)]),
    ("Bopomofo", &[(12544,12591)]),
    ("BopomofoExtended", &[(12704,12735)]),
    ("BoxDrawing", &[(9472,9599)]),
    ("BraillePatterns", &[(10240,10495)]),
    ("Buhid", &[(5952,5983)]),
    ("ByzantineMusicalSymbols", &[(118784,119039)]),
    ("CJKCompatibility", &[(13056,13311)]),
    ("CJKCompatibilityForms", &[(65072,65103)]),
    ("CJKCompatibilityIdeographs", &[(63744,64255)]),
    ("CJKCompatibilityIdeographsSupplement", &[(194560,195103)]),
    ("CJKRadicalsSupplement", &[(11904,12031)]),
    ("CJKSymbolsandPunctuation", &[(12288,12351)]),
    ("CJKUnifiedIdeographs", &[(19968,40959)]),
    ("CJKUnifiedIdeographsExtensionA", &[(13312,19903)]),
    ("CJKUnifiedIdeographsExtensionB", &[(131072,173791)]),
    ("Cherokee", &[(5024,5119)]),
    ("CombiningDiacriticalMarks", &[(768,879)]),
    ("CombiningDiacriticalMarksforSymbols", &[(8400,8447)]),
    ("CombiningHalfMarks", &[(65056,65071)]),
    ("CombiningMarksforSymbols", &[(8400,8447)]),
    ("ControlPictures", &[(9216,9279)]),
    ("CurrencySymbols", &[(8352,8399)]),
    ("CypriotSyllabary", &[(67584,67647)]),
    ("Cyrillic", &[(1024,1279)]),
    ("CyrillicSupplement", &[(1280,1327)]),
    ("Deseret", &[(66560,66639)]),
    ("Devanagari", &[(2304,2431)]),
    ("Dingbats", &[(9984,10175)]),
    ("EnclosedAlphanumerics", &[(9312,9471)]),
    ("EnclosedCJKLettersandMonths", &[(12800,13055)]),
    ("Ethiopic", &[(4608,4991)]),
    ("GeneralPunctuation", &[(8192,8303)]),
    ("GeometricShapes", &[(9632,9727)]),
    ("Georgian", &[(4256,4351)]),
    ("Gothic", &[(66352,66383)]),
    ("Greek", &[(880,1023)]),
    ("GreekExtended", &[(7936,8191)]),
    ("GreekandCoptic", &[(880,1023)]),
    ("Gujarati", &[(2688,2815)]),
    ("Gurmukhi", &[(2560,2687)]),
    ("HalfwidthandFullwidthForms", &[(65280,65519)]),
    ("HangulCompatibilityJamo", &[(12592,12687)]),
    ("HangulJamo", &[(4352,4607)]),
    ("HangulSyllables", &[(44032,55215)]),
    ("Hanunoo", &[(5920,5951)]),
    ("Hebrew", &[(1424,1535)]),
    ("HighPrivateUseSurrogates", &[(56192,56319)]),
    ("HighSurrogates", &[(55296,56191)]),
    ("Hiragana", &[(12352,12447)]),
    ("IPAExtensions", &[(592,687)]),
    ("IdeographicDescriptionCharacters", &[(12272,12287)]),
    ("Kanbun", &[(12688,12703)]),
    ("KangxiRadicals", &[(12032,12255)]),
    ("Kannada", &[(3200,3327)]),
    ("Katakana", &[(12448,12543)]),
    ("KatakanaPhoneticExtensions", &[(12784,12799)]),
    ("Khmer", &[(6016,6143)]),
    ("KhmerSymbols", &[(6624,6655)]),
    ("Lao", &[(3712,3839)]),
    ("Latin-1Supplement", &[(128,255)]),
    ("LatinExtended-A", &[(256,383)]),
    ("LatinExtended-B", &[(384,591)]),
    ("LatinExtendedAdditional", &[(7680,7935)]),
    ("LetterlikeSymbols", &[(8448,8527)]),
    ("Limbu", &[(6400,6479)]),
    ("LinearBIdeograms", &[(65664,65791)]),
    ("LinearBSyllabary", &[(65536,65663)]),
    ("LowSurrogates", &[(56320,57343)]),
    ("Malayalam", &[(3328,3455)]),
    ("MathematicalAlphanumericSymbols", &[(119808,120831)]),
    ("MathematicalOperators", &[(8704,8959)]),
    ("MiscellaneousMathematicalSymbols-A", &[(10176,10223)]),
    ("MiscellaneousMathematicalSymbols-B", &[(10624,10751)]),
    ("MiscellaneousSymbols", &[(9728,9983)]),
    ("MiscellaneousSymbolsandArrows", &[(11008,11263)]),
    ("MiscellaneousTechnical", &[(8960,9215)]),
    ("Mongolian", &[(6144,6319)]),
    ("MusicalSymbols", &[(119040,119295)]),
    ("Myanmar", &[(4096,4255)]),
    ("NumberForms", &[(8528,8591)]),
    ("Ogham", &[(5760,5791)]),
    ("OldItalic", &[(66304,66351)]),
    ("OpticalCharacterRecognition", &[(9280,9311)]),
    ("Oriya", &[(2816,2943)]),
    ("Osmanya", &[(66688,66735)]),
    ("PhoneticExtensions", &[(7424,7551)]),
    ("PrivateUse", &[(57344,63743),(983040,1048575),(1048576,1114111)]),
    ("PrivateUseArea", &[(57344,63743)]),
    ("Runic", &[(5792,5887)]),
    ("Shavian", &[(66640,66687)]),
    ("Sinhala", &[(3456,3583)]),
    ("SmallFormVariants", &[(65104,65135)]),
    ("SpacingModifierLetters", &[(688,767)]),
    ("Specials", &[(65520,65535)]),
    ("SuperscriptsandSubscripts", &[(8304,8351)]),
    ("SupplementalArrows-A", &[(10224,10239)]),
    ("SupplementalArrows-B", &[(10496,10623)]),
    ("SupplementalMathematicalOperators", &[(10752,11007)]),
    ("SupplementaryPrivateUseArea-A", &[(983040,1048575)]),
    ("SupplementaryPrivateUseArea-B", &[(1048576,1114111)]),
    ("Syriac", &[(1792,1871)]),
    ("Tagalog", &[(5888,5919)]),
    ("Tagbanwa", &[(5984,6015)]),
    ("Tags", &[(917504,917631)]),
    ("TaiLe", &[(6480,6527)]),
    ("TaiXuanJingSymbols", &[(119552,119647)]),
    ("Tamil", &[(2944,3071)]),
    ("Telugu", &[(3072,3199)]),
    ("Thaana", &[(1920,1983)]),
    ("Thai", &[(3584,3711)]),
    ("Tibetan", &[(3840,4095)]),
    ("Ugaritic", &[(66432,66463)]),
    ("UnifiedCanadianAboriginalSyllabics", &[(5120,5759)]),
    ("VariationSelectors", &[(65024,65039)]),
    ("VariationSelectorsSupplement", &[(917760,917999)]),
    ("YiRadicals", &[(42128,42191)]),
    ("YiSyllables", &[(40960,42127)]),
    ("YijingHexagramSymbols", &[(19904,19967)]),
];

#[rustfmt::skip]

static XML_UCS_CATS: &[(&str, &[(u32, u32)])] = &[
    ("C", &[(0,31),(127,159),(173,173),(1536,1539),(1757,1757),(1807,1807),(6068,6069),(8203,8207),(8234,8238),(8288,8291),(8298,8303),(55296,55296),(56191,56192),(56319,56320),(57343,57344),(63743,63743),(65279,65279),(65529,65531),(119155,119162),(917505,917505),(917536,917631),(983040,983040),(1048573,1048573),(1048576,1048576),(1114109,1114109)]),
    ("Cc", &[(0,31),(127,159)]),
    ("Cf", &[(173,173),(1536,1539),(1757,1757),(1807,1807),(6068,6069),(8203,8207),(8234,8238),(8288,8291),(8298,8303),(65279,65279),(65529,65531),(119155,119162),(917505,917505),(917536,917631)]),
    ("Co", &[(57344,57344),(63743,63743),(983040,983040),(1048573,1048573),(1048576,1048576),(1114109,1114109)]),
    ("Cs", &[(55296,57343)]),
    ("L", &[(65,90),(97,122),(170,170),(181,181),(186,186),(192,214),(216,246),(248,566),(592,705),(710,721),(736,740),(750,750),(890,890),(902,902),(904,906),(908,908),(910,929),(931,974),(976,1013),(1015,1019),(1024,1153),(1162,1230),(1232,1269),(1272,1273),(1280,1295),(1329,1366),(1369,1369),(1377,1415),(1488,1514),(1520,1522),(1569,1594),(1600,1610),(1646,1647),(1649,1747),(1749,1749),(1765,1766),(1774,1775),(1786,1788),(1791,1791),(1808,1808),(1810,1839),(1869,1871),(1920,1957),(1969,1969),(2308,2361),(2365,2365),(2384,2384),(2392,2401),(2437,2444),(2447,2448),(2451,2472),(2474,2480),(2482,2482),(2486,2489),(2493,2493),(2524,2525),(2527,2529),(2544,2545),(2565,2570),(2575,2576),(2579,2600),(2602,2608),(2610,2611),(2613,2614),(2616,2617),(2649,2652),(2654,2654),(2674,2676),(2693,2701),(2703,2705),(2707,2728),(2730,2736),(2738,2739),(2741,2745),(2749,2749),(2768,2768),(2784,2785),(2821,2828),(2831,2832),(2835,2856),(2858,2864),(2866,2867),(2869,2873),(2877,2877),(2908,2909),(2911,2913),(2929,2929),(2947,2947),(2949,2954),(2958,2960),(2962,2965),(2969,2970),(2972,2972),(2974,2975),(2979,2980),(2984,2986),(2990,2997),(2999,3001),(3077,3084),(3086,3088),(3090,3112),(3114,3123),(3125,3129),(3168,3169),(3205,3212),(3214,3216),(3218,3240),(3242,3251),(3253,3257),(3261,3261),(3294,3294),(3296,3297),(3333,3340),(3342,3344),(3346,3368),(3370,3385),(3424,3425),(3461,3478),(3482,3505),(3507,3515),(3517,3517),(3520,3526),(3585,3632),(3634,3635),(3648,3654),(3713,3714),(3716,3716),(3719,3720),(3722,3722),(3725,3725),(3732,3735),(3737,3743),(3745,3747),(3749,3749),(3751,3751),(3754,3755),(3757,3760),(3762,3763),(3773,3773),(3776,3780),(3782,3782),(3804,3805),(3840,3840),(3904,3911),(3913,3946),(3976,3979),(4096,4129),(4131,4135),(4137,4138),(4176,4181),(4256,4293),(4304,4344),(4352,4441),(4447,4514),(4520,4601),(4608,4614),(4616,4678),(4680,4680),(4682,4685),(4688,4694),(4696,4696),(4698,4701),(4704,4742),(4744,4744),(4746,4749),(4752,4782),(4784,4784),(4786,4789),(4792,4798),(4800,4800),(4802,4805),(4808,4814),(4816,4822),(4824,4846),(4848,4878),(4880,4880),(4882,4885),(4888,4894),(4896,4934),(4936,4954),(5024,5108),(5121,5740),(5743,5750),(5761,5786),(5792,5866),(5888,5900),(5902,5905),(5920,5937),(5952,5969),(5984,5996),(5998,6000),(6016,6067),(6103,6103),(6108,6108),(6176,6263),(6272,6312),(6400,6428),(6480,6509),(6512,6516),(7424,7531),(7680,7835),(7840,7929),(7936,7957),(7960,7965),(7968,8005),(8008,8013),(8016,8023),(8025,8025),(8027,8027),(8029,8029),(8031,8061),(8064,8116),(8118,8124),(8126,8126),(8130,8132),(8134,8140),(8144,8147),(8150,8155),(8160,8172),(8178,8180),(8182,8188),(8305,8305),(8319,8319),(8450,8450),(8455,8455),(8458,8467),(8469,8469),(8473,8477),(8484,8484),(8486,8486),(8488,8488),(8490,8493),(8495,8497),(8499,8505),(8509,8511),(8517,8521),(12293,12294),(12337,12341),(12347,12348),(12353,12438),(12445,12447),(12449,12538),(12540,12543),(12549,12588),(12593,12686),(12704,12727),(12784,12799),(13312,13312),(19893,19893),(19968,19968),(40869,40869),(40960,42124),(44032,44032),(55203,55203),(63744,64045),(64048,64106),(64256,64262),(64275,64279),(64285,64285),(64287,64296),(64298,64310),(64312,64316),(64318,64318),(64320,64321),(64323,64324),(64326,64433),(64467,64829),(64848,64911),(64914,64967),(65008,65019),(65136,65140),(65142,65276),(65313,65338),(65345,65370),(65382,65470),(65474,65479),(65482,65487),(65490,65495),(65498,65500),(65536,65547),(65549,65574),(65576,65594),(65596,65597),(65599,65613),(65616,65629),(65664,65786),(66304,66334),(66352,66377),(66432,66461),(66560,66717),(67584,67589),(67592,67592),(67594,67637),(67639,67640),(67644,67644),(67647,67647),(119808,119892),(119894,119964),(119966,119967),(119970,119970),(119973,119974),(119977,119980),(119982,119993),(119995,119995),(119997,120003),(120005,120069),(120071,120074),(120077,120084),(120086,120092),(120094,120121),(120123,120126),(120128,120132),(120134,120134),(120138,120144),(120146,120483),(120488,120512),(120514,120538),(120540,120570),(120572,120596),(120598,120628),(120630,120654),(120656,120686),(120688,120712),(120714,120744),(120746,120770),(120772,120777),(131072,131072),(173782,173782),(194560,195101)]),
    ("Ll", &[(97,122),(170,170),(181,181),(186,186),(223,246),(248,255),(257,257),(259,259),(261,261),(263,263),(265,265),(267,267),(269,269),(271,271),(273,273),(275,275),(277,277),(279,279),(281,281),(283,283),(285,285),(287,287),(289,289),(291,291),(293,293),(295,295),(297,297),(299,299),(301,301),(303,303),(305,305),(307,307),(309,309),(311,312),(314,314),(316,316),(318,318),(320,320),(322,322),(324,324),(326,326),(328,329),(331,331),(333,333),(335,335),(337,337),(339,339),(341,341),(343,343),(345,345),(347,347),(349,349),(351,351),(353,353),(355,355),(357,357),(359,359),(361,361),(363,363),(365,365),(367,367),(369,369),(371,371),(373,373),(375,375),(378,378),(380,380),(382,384),(387,387),(389,389),(392,392),(396,397),(402,402),(405,405),(409,411),(414,414),(417,417),(419,419),(421,421),(424,424),(426,427),(429,429),(432,432),(436,436),(438,438),(441,442),(445,447),(454,454),(457,457),(460,460),(462,462),(464,464),(466,466),(468,468),(470,470),(472,472),(474,474),(476,477),(479,479),(481,481),(483,483),(485,485),(487,487),(489,489),(491,491),(493,493),(495,496),(499,499),(501,501),(505,505),(507,507),(509,509),(511,511),(513,513),(515,515),(517,517),(519,519),(521,521),(523,523),(525,525),(527,527),(529,529),(531,531),(533,533),(535,535),(537,537),(539,539),(541,541),(543,543),(545,545),(547,547),(549,549),(551,551),(553,553),(555,555),(557,557),(559,559),(561,561),(563,566),(592,687),(912,912),(940,974),(976,977),(981,983),(985,985),(987,987),(989,989),(991,991),(993,993),(995,995),(997,997),(999,999),(1001,1001),(1003,1003),(1005,1005),(1007,1011),(1013,1013),(1016,1016),(1019,1019),(1072,1119),(1121,1121),(1123,1123),(1125,1125),(1127,1127),(1129,1129),(1131,1131),(1133,1133),(1135,1135),(1137,1137),(1139,1139),(1141,1141),(1143,1143),(1145,1145),(1147,1147),(1149,1149),(1151,1151),(1153,1153),(1163,1163),(1165,1165),(1167,1167),(1169,1169),(1171,1171),(1173,1173),(1175,1175),(1177,1177),(1179,1179),(1181,1181),(1183,1183),(1185,1185),(1187,1187),(1189,1189),(1191,1191),(1193,1193),(1195,1195),(1197,1197),(1199,1199),(1201,1201),(1203,1203),(1205,1205),(1207,1207),(1209,1209),(1211,1211),(1213,1213),(1215,1215),(1218,1218),(1220,1220),(1222,1222),(1224,1224),(1226,1226),(1228,1228),(1230,1230),(1233,1233),(1235,1235),(1237,1237),(1239,1239),(1241,1241),(1243,1243),(1245,1245),(1247,1247),(1249,1249),(1251,1251),(1253,1253),(1255,1255),(1257,1257),(1259,1259),(1261,1261),(1263,1263),(1265,1265),(1267,1267),(1269,1269),(1273,1273),(1281,1281),(1283,1283),(1285,1285),(1287,1287),(1289,1289),(1291,1291),(1293,1293),(1295,1295),(1377,1415),(7424,7467),(7522,7531),(7681,7681),(7683,7683),(7685,7685),(7687,7687),(7689,7689),(7691,7691),(7693,7693),(7695,7695),(7697,7697),(7699,7699),(7701,7701),(7703,7703),(7705,7705),(7707,7707),(7709,7709),(7711,7711),(7713,7713),(7715,7715),(7717,7717),(7719,7719),(7721,7721),(7723,7723),(7725,7725),(7727,7727),(7729,7729),(7731,7731),(7733,7733),(7735,7735),(7737,7737),(7739,7739),(7741,7741),(7743,7743),(7745,7745),(7747,7747),(7749,7749),(7751,7751),(7753,7753),(7755,7755),(7757,7757),(7759,7759),(7761,7761),(7763,7763),(7765,7765),(7767,7767),(7769,7769),(7771,7771),(7773,7773),(7775,7775),(7777,7777),(7779,7779),(7781,7781),(7783,7783),(7785,7785),(7787,7787),(7789,7789),(7791,7791),(7793,7793),(7795,7795),(7797,7797),(7799,7799),(7801,7801),(7803,7803),(7805,7805),(7807,7807),(7809,7809),(7811,7811),(7813,7813),(7815,7815),(7817,7817),(7819,7819),(7821,7821),(7823,7823),(7825,7825),(7827,7827),(7829,7835),(7841,7841),(7843,7843),(7845,7845),(7847,7847),(7849,7849),(7851,7851),(7853,7853),(7855,7855),(7857,7857),(7859,7859),(7861,7861),(7863,7863),(7865,7865),(7867,7867),(7869,7869),(7871,7871),(7873,7873),(7875,7875),(7877,7877),(7879,7879),(7881,7881),(7883,7883),(7885,7885),(7887,7887),(7889,7889),(7891,7891),(7893,7893),(7895,7895),(7897,7897),(7899,7899),(7901,7901),(7903,7903),(7905,7905),(7907,7907),(7909,7909),(7911,7911),(7913,7913),(7915,7915),(7917,7917),(7919,7919),(7921,7921),(7923,7923),(7925,7925),(7927,7927),(7929,7929),(7936,7943),(7952,7957),(7968,7975),(7984,7991),(8000,8005),(8016,8023),(8032,8039),(8048,8061),(8064,8071),(8080,8087),(8096,8103),(8112,8116),(8118,8119),(8126,8126),(8130,8132),(8134,8135),(8144,8147),(8150,8151),(8160,8167),(8178,8180),(8182,8183),(8305,8305),(8319,8319),(8458,8458),(8462,8463),(8467,8467),(8495,8495),(8500,8500),(8505,8505),(8509,8509),(8518,8521),(64256,64262),(64275,64279),(65345,65370),(66600,66639),(119834,119859),(119886,119892),(119894,119911),(119938,119963),(119990,119993),(119995,119995),(119997,120003),(120005,120015),(120042,120067),(120094,120119),(120146,120171),(120198,120223),(120250,120275),(120302,120327),(120354,120379),(120406,120431),(120458,120483),(120514,120538),(120540,120545),(120572,120596),(120598,120603),(120630,120654),(120656,120661),(120688,120712),(120714,120719),(120746,120770),(120772,120777)]),
    ("Lm", &[(688,705),(710,721),(736,740),(750,750),(890,890),(1369,1369),(1600,1600),(1765,1766),(3654,3654),(3782,3782),(6103,6103),(6211,6211),(7468,7521),(12293,12293),(12337,12341),(12347,12347),(12445,12446),(12540,12542),(65392,65392),(65438,65439)]),
    ("Lo", &[(443,443),(448,451),(1488,1514),(1520,1522),(1569,1594),(1601,1610),(1646,1647),(1649,1747),(1749,1749),(1774,1775),(1786,1788),(1791,1791),(1808,1808),(1810,1839),(1869,1871),(1920,1957),(1969,1969),(2308,2361),(2365,2365),(2384,2384),(2392,2401),(2437,2444),(2447,2448),(2451,2472),(2474,2480),(2482,2482),(2486,2489),(2493,2493),(2524,2525),(2527,2529),(2544,2545),(2565,2570),(2575,2576),(2579,2600),(2602,2608),(2610,2611),(2613,2614),(2616,2617),(2649,2652),(2654,2654),(2674,2676),(2693,2701),(2703,2705),(2707,2728),(2730,2736),(2738,2739),(2741,2745),(2749,2749),(2768,2768),(2784,2785),(2821,2828),(2831,2832),(2835,2856),(2858,2864),(2866,2867),(2869,2873),(2877,2877),(2908,2909),(2911,2913),(2929,2929),(2947,2947),(2949,2954),(2958,2960),(2962,2965),(2969,2970),(2972,2972),(2974,2975),(2979,2980),(2984,2986),(2990,2997),(2999,3001),(3077,3084),(3086,3088),(3090,3112),(3114,3123),(3125,3129),(3168,3169),(3205,3212),(3214,3216),(3218,3240),(3242,3251),(3253,3257),(3261,3261),(3294,3294),(3296,3297),(3333,3340),(3342,3344),(3346,3368),(3370,3385),(3424,3425),(3461,3478),(3482,3505),(3507,3515),(3517,3517),(3520,3526),(3585,3632),(3634,3635),(3648,3653),(3713,3714),(3716,3716),(3719,3720),(3722,3722),(3725,3725),(3732,3735),(3737,3743),(3745,3747),(3749,3749),(3751,3751),(3754,3755),(3757,3760),(3762,3763),(3773,3773),(3776,3780),(3804,3805),(3840,3840),(3904,3911),(3913,3946),(3976,3979),(4096,4129),(4131,4135),(4137,4138),(4176,4181),(4304,4344),(4352,4441),(4447,4514),(4520,4601),(4608,4614),(4616,4678),(4680,4680),(4682,4685),(4688,4694),(4696,4696),(4698,4701),(4704,4742),(4744,4744),(4746,4749),(4752,4782),(4784,4784),(4786,4789),(4792,4798),(4800,4800),(4802,4805),(4808,4814),(4816,4822),(4824,4846),(4848,4878),(4880,4880),(4882,4885),(4888,4894),(4896,4934),(4936,4954),(5024,5108),(5121,5740),(5743,5750),(5761,5786),(5792,5866),(5888,5900),(5902,5905),(5920,5937),(5952,5969),(5984,5996),(5998,6000),(6016,6067),(6108,6108),(6176,6210),(6212,6263),(6272,6312),(6400,6428),(6480,6509),(6512,6516),(8501,8504),(12294,12294),(12348,12348),(12353,12438),(12447,12447),(12449,12538),(12543,12543),(12549,12588),(12593,12686),(12704,12727),(12784,12799),(13312,13312),(19893,19893),(19968,19968),(40869,40869),(40960,42124),(44032,44032),(55203,55203),(63744,64045),(64048,64106),(64285,64285),(64287,64296),(64298,64310),(64312,64316),(64318,64318),(64320,64321),(64323,64324),(64326,64433),(64467,64829),(64848,64911),(64914,64967),(65008,65019),(65136,65140),(65142,65276),(65382,65391),(65393,65437),(65440,65470),(65474,65479),(65482,65487),(65490,65495),(65498,65500),(65536,65547),(65549,65574),(65576,65594),(65596,65597),(65599,65613),(65616,65629),(65664,65786),(66304,66334),(66352,66377),(66432,66461),(66640,66717),(67584,67589),(67592,67592),(67594,67637),(67639,67640),(67644,67644),(67647,67647),(131072,131072),(173782,173782),(194560,195101)]),
    ("Lt", &[(453,453),(456,456),(459,459),(498,498),(8072,8079),(8088,8095),(8104,8111),(8124,8124),(8140,8140),(8188,8188)]),
    ("Lu", &[(65,90),(192,214),(216,222),(256,256),(258,258),(260,260),(262,262),(264,264),(266,266),(268,268),(270,270),(272,272),(274,274),(276,276),(278,278),(280,280),(282,282),(284,284),(286,286),(288,288),(290,290),(292,292),(294,294),(296,296),(298,298),(300,300),(302,302),(304,304),(306,306),(308,308),(310,310),(313,313),(315,315),(317,317),(319,319),(321,321),(323,323),(325,325),(327,327),(330,330),(332,332),(334,334),(336,336),(338,338),(340,340),(342,342),(344,344),(346,346),(348,348),(350,350),(352,352),(354,354),(356,356),(358,358),(360,360),(362,362),(364,364),(366,366),(368,368),(370,370),(372,372),(374,374),(376,377),(379,379),(381,381),(385,386),(388,388),(390,391),(393,395),(398,401),(403,404),(406,408),(412,413),(415,416),(418,418),(420,420),(422,423),(425,425),(428,428),(430,431),(433,435),(437,437),(439,440),(444,444),(452,452),(455,455),(458,458),(461,461),(463,463),(465,465),(467,467),(469,469),(471,471),(473,473),(475,475),(478,478),(480,480),(482,482),(484,484),(486,486),(488,488),(490,490),(492,492),(494,494),(497,497),(500,500),(502,504),(506,506),(508,508),(510,510),(512,512),(514,514),(516,516),(518,518),(520,520),(522,522),(524,524),(526,526),(528,528),(530,530),(532,532),(534,534),(536,536),(538,538),(540,540),(542,542),(544,544),(546,546),(548,548),(550,550),(552,552),(554,554),(556,556),(558,558),(560,560),(562,562),(902,902),(904,906),(908,908),(910,911),(913,929),(931,939),(978,980),(984,984),(986,986),(988,988),(990,990),(992,992),(994,994),(996,996),(998,998),(1000,1000),(1002,1002),(1004,1004),(1006,1006),(1012,1012),(1015,1015),(1017,1018),(1024,1071),(1120,1120),(1122,1122),(1124,1124),(1126,1126),(1128,1128),(1130,1130),(1132,1132),(1134,1134),(1136,1136),(1138,1138),(1140,1140),(1142,1142),(1144,1144),(1146,1146),(1148,1148),(1150,1150),(1152,1152),(1162,1162),(1164,1164),(1166,1166),(1168,1168),(1170,1170),(1172,1172),(1174,1174),(1176,1176),(1178,1178),(1180,1180),(1182,1182),(1184,1184),(1186,1186),(1188,1188),(1190,1190),(1192,1192),(1194,1194),(1196,1196),(1198,1198),(1200,1200),(1202,1202),(1204,1204),(1206,1206),(1208,1208),(1210,1210),(1212,1212),(1214,1214),(1216,1217),(1219,1219),(1221,1221),(1223,1223),(1225,1225),(1227,1227),(1229,1229),(1232,1232),(1234,1234),(1236,1236),(1238,1238),(1240,1240),(1242,1242),(1244,1244),(1246,1246),(1248,1248),(1250,1250),(1252,1252),(1254,1254),(1256,1256),(1258,1258),(1260,1260),(1262,1262),(1264,1264),(1266,1266),(1268,1268),(1272,1272),(1280,1280),(1282,1282),(1284,1284),(1286,1286),(1288,1288),(1290,1290),(1292,1292),(1294,1294),(1329,1366),(4256,4293),(7680,7680),(7682,7682),(7684,7684),(7686,7686),(7688,7688),(7690,7690),(7692,7692),(7694,7694),(7696,7696),(7698,7698),(7700,7700),(7702,7702),(7704,7704),(7706,7706),(7708,7708),(7710,7710),(7712,7712),(7714,7714),(7716,7716),(7718,7718),(7720,7720),(7722,7722),(7724,7724),(7726,7726),(7728,7728),(7730,7730),(7732,7732),(7734,7734),(7736,7736),(7738,7738),(7740,7740),(7742,7742),(7744,7744),(7746,7746),(7748,7748),(7750,7750),(7752,7752),(7754,7754),(7756,7756),(7758,7758),(7760,7760),(7762,7762),(7764,7764),(7766,7766),(7768,7768),(7770,7770),(7772,7772),(7774,7774),(7776,7776),(7778,7778),(7780,7780),(7782,7782),(7784,7784),(7786,7786),(7788,7788),(7790,7790),(7792,7792),(7794,7794),(7796,7796),(7798,7798),(7800,7800),(7802,7802),(7804,7804),(7806,7806),(7808,7808),(7810,7810),(7812,7812),(7814,7814),(7816,7816),(7818,7818),(7820,7820),(7822,7822),(7824,7824),(7826,7826),(7828,7828),(7840,7840),(7842,7842),(7844,7844),(7846,7846),(7848,7848),(7850,7850),(7852,7852),(7854,7854),(7856,7856),(7858,7858),(7860,7860),(7862,7862),(7864,7864),(7866,7866),(7868,7868),(7870,7870),(7872,7872),(7874,7874),(7876,7876),(7878,7878),(7880,7880),(7882,7882),(7884,7884),(7886,7886),(7888,7888),(7890,7890),(7892,7892),(7894,7894),(7896,7896),(7898,7898),(7900,7900),(7902,7902),(7904,7904),(7906,7906),(7908,7908),(7910,7910),(7912,7912),(7914,7914),(7916,7916),(7918,7918),(7920,7920),(7922,7922),(7924,7924),(7926,7926),(7928,7928),(7944,7951),(7960,7965),(7976,7983),(7992,7999),(8008,8013),(8025,8025),(8027,8027),(8029,8029),(8031,8031),(8040,8047),(8120,8123),(8136,8139),(8152,8155),(8168,8172),(8184,8187),(8450,8450),(8455,8455),(8459,8461),(8464,8466),(8469,8469),(8473,8477),(8484,8484),(8486,8486),(8488,8488),(8490,8493),(8496,8497),(8499,8499),(8510,8511),(8517,8517),(65313,65338),(66560,66599),(119808,119833),(119860,119885),(119912,119937),(119964,119964),(119966,119967),(119970,119970),(119973,119974),(119977,119980),(119982,119989),(120016,120041),(120068,120069),(120071,120074),(120077,120084),(120086,120092),(120120,120121),(120123,120126),(120128,120132),(120134,120134),(120138,120144),(120172,120197),(120224,120249),(120276,120301),(120328,120353),(120380,120405),(120432,120457),(120488,120512),(120546,120570),(120604,120628),(120662,120686),(120720,120744)]),
    ("M", &[(768,855),(861,879),(1155,1158),(1160,1161),(1425,1441),(1443,1465),(1467,1469),(1471,1471),(1473,1474),(1476,1476),(1552,1557),(1611,1624),(1648,1648),(1750,1756),(1758,1764),(1767,1768),(1770,1773),(1809,1809),(1840,1866),(1958,1968),(2305,2307),(2364,2364),(2366,2381),(2385,2388),(2402,2403),(2433,2435),(2492,2492),(2494,2500),(2503,2504),(2507,2509),(2519,2519),(2530,2531),(2561,2563),(2620,2620),(2622,2626),(2631,2632),(2635,2637),(2672,2673),(2689,2691),(2748,2748),(2750,2757),(2759,2761),(2763,2765),(2786,2787),(2817,2819),(2876,2876),(2878,2883),(2887,2888),(2891,2893),(2902,2903),(2946,2946),(3006,3010),(3014,3016),(3018,3021),(3031,3031),(3073,3075),(3134,3140),(3142,3144),(3146,3149),(3157,3158),(3202,3203),(3260,3260),(3262,3268),(3270,3272),(3274,3277),(3285,3286),(3330,3331),(3390,3395),(3398,3400),(3402,3405),(3415,3415),(3458,3459),(3530,3530),(3535,3540),(3542,3542),(3544,3551),(3570,3571),(3633,3633),(3636,3642),(3655,3662),(3761,3761),(3764,3769),(3771,3772),(3784,3789),(3864,3865),(3893,3893),(3895,3895),(3897,3897),(3902,3903),(3953,3972),(3974,3975),(3984,3991),(3993,4028),(4038,4038),(4140,4146),(4150,4153),(4182,4185),(5906,5908),(5938,5940),(5970,5971),(6002,6003),(6070,6099),(6109,6109),(6155,6157),(6313,6313),(6432,6443),(6448,6459),(8400,8426),(12330,12335),(12441,12442),(64286,64286),(65024,65039),(65056,65059),(119141,119145),(119149,119154),(119163,119170),(119173,119179),(119210,119213),(917760,917999)]),
    ("Mc", &[(2307,2307),(2366,2368),(2377,2380),(2434,2435),(2494,2496),(2503,2504),(2507,2508),(2519,2519),(2563,2563),(2622,2624),(2691,2691),(2750,2752),(2761,2761),(2763,2764),(2818,2819),(2878,2878),(2880,2880),(2887,2888),(2891,2892),(2903,2903),(3006,3007),(3009,3010),(3014,3016),(3018,3020),(3031,3031),(3073,3075),(3137,3140),(3202,3203),(3262,3262),(3264,3268),(3271,3272),(3274,3275),(3285,3286),(3330,3331),(3390,3392),(3398,3400),(3402,3404),(3415,3415),(3458,3459),(3535,3537),(3544,3551),(3570,3571),(3902,3903),(3967,3967),(4140,4140),(4145,4145),(4152,4152),(4182,4183),(6070,6070),(6078,6085),(6087,6088),(6435,6438),(6441,6443),(6448,6449),(6451,6456),(119141,119142),(119149,119154)]),
    ("Me", &[(1160,1161),(1758,1758),(8413,8416),(8418,8420)]),
    ("Mn", &[(768,855),(861,879),(1155,1158),(1425,1441),(1443,1465),(1467,1469),(1471,1471),(1473,1474),(1476,1476),(1552,1557),(1611,1624),(1648,1648),(1750,1756),(1759,1764),(1767,1768),(1770,1773),(1809,1809),(1840,1866),(1958,1968),(2305,2306),(2364,2364),(2369,2376),(2381,2381),(2385,2388),(2402,2403),(2433,2433),(2492,2492),(2497,2500),(2509,2509),(2530,2531),(2561,2562),(2620,2620),(2625,2626),(2631,2632),(2635,2637),(2672,2673),(2689,2690),(2748,2748),(2753,2757),(2759,2760),(2765,2765),(2786,2787),(2817,2817),(2876,2876),(2879,2879),(2881,2883),(2893,2893),(2902,2902),(2946,2946),(3008,3008),(3021,3021),(3134,3136),(3142,3144),(3146,3149),(3157,3158),(3260,3260),(3263,3263),(3270,3270),(3276,3277),(3393,3395),(3405,3405),(3530,3530),(3538,3540),(3542,3542),(3633,3633),(3636,3642),(3655,3662),(3761,3761),(3764,3769),(3771,3772),(3784,3789),(3864,3865),(3893,3893),(3895,3895),(3897,3897),(3953,3966),(3968,3972),(3974,3975),(3984,3991),(3993,4028),(4038,4038),(4141,4144),(4146,4146),(4150,4151),(4153,4153),(4184,4185),(5906,5908),(5938,5940),(5970,5971),(6002,6003),(6071,6077),(6086,6086),(6089,6099),(6109,6109),(6155,6157),(6313,6313),(6432,6434),(6439,6440),(6450,6450),(6457,6459),(8400,8412),(8417,8417),(8421,8426),(12330,12335),(12441,12442),(64286,64286),(65024,65039),(65056,65059),(119143,119145),(119163,119170),(119173,119179),(119210,119213),(917760,917999)]),
    ("N", &[(48,57),(178,179),(185,185),(188,190),(1632,1641),(1776,1785),(2406,2415),(2534,2543),(2548,2553),(2662,2671),(2790,2799),(2918,2927),(3047,3058),(3174,3183),(3302,3311),(3430,3439),(3664,3673),(3792,3801),(3872,3891),(4160,4169),(4969,4988),(5870,5872),(6112,6121),(6128,6137),(6160,6169),(6470,6479),(8304,8304),(8308,8313),(8320,8329),(8531,8579),(9312,9371),(9450,9471),(10102,10131),(12295,12295),(12321,12329),(12344,12346),(12690,12693),(12832,12841),(12881,12895),(12928,12937),(12977,12991),(65296,65305),(65799,65843),(66336,66339),(66378,66378),(66720,66729),(120782,120831)]),
    ("Nd", &[(48,57),(1632,1641),(1776,1785),(2406,2415),(2534,2543),(2662,2671),(2790,2799),(2918,2927),(3047,3055),(3174,3183),(3302,3311),(3430,3439),(3664,3673),(3792,3801),(3872,3881),(4160,4169),(4969,4977),(6112,6121),(6160,6169),(6470,6479),(65296,65305),(66720,66729),(120782,120831)]),
    ("Nl", &[(5870,5872),(8544,8579),(12295,12295),(12321,12329),(12344,12346),(66378,66378)]),
    ("No", &[(178,179),(185,185),(188,190),(2548,2553),(3056,3058),(3882,3891),(4978,4988),(6128,6137),(8304,8304),(8308,8313),(8320,8329),(8531,8543),(9312,9371),(9450,9471),(10102,10131),(12690,12693),(12832,12841),(12881,12895),(12928,12937),(12977,12991),(65799,65843),(66336,66339)]),
    ("P", &[(33,35),(37,42),(44,47),(58,59),(63,64),(91,93),(95,95),(123,123),(125,125),(161,161),(171,171),(183,183),(187,187),(191,191),(894,894),(903,903),(1370,1375),(1417,1418),(1470,1470),(1472,1472),(1475,1475),(1523,1524),(1548,1549),(1563,1563),(1567,1567),(1642,1645),(1748,1748),(1792,1805),(2404,2405),(2416,2416),(3572,3572),(3663,3663),(3674,3675),(3844,3858),(3898,3901),(3973,3973),(4170,4175),(4347,4347),(4961,4968),(5741,5742),(5787,5788),(5867,5869),(5941,5942),(6100,6102),(6104,6106),(6144,6154),(6468,6469),(8208,8231),(8240,8259),(8261,8273),(8275,8276),(8279,8279),(8317,8318),(8333,8334),(9001,9002),(9140,9142),(10088,10101),(10214,10219),(10627,10648),(10712,10715),(10748,10749),(12289,12291),(12296,12305),(12308,12319),(12336,12336),(12349,12349),(12448,12448),(12539,12539),(64830,64831),(65072,65106),(65108,65121),(65123,65123),(65128,65128),(65130,65131),(65281,65283),(65285,65290),(65292,65295),(65306,65307),(65311,65312),(65339,65341),(65343,65343),(65371,65371),(65373,65373),(65375,65381),(65792,65793),(66463,66463)]),
    ("Pc", &[(95,95),(8255,8256),(8276,8276),(12539,12539),(65075,65076),(65101,65103),(65343,65343),(65381,65381)]),
    ("Pd", &[(45,45),(1418,1418),(6150,6150),(8208,8213),(12316,12316),(12336,12336),(12448,12448),(65073,65074),(65112,65112),(65123,65123),(65293,65293)]),
    ("Pe", &[(41,41),(93,93),(125,125),(3899,3899),(3901,3901),(5788,5788),(8262,8262),(8318,8318),(8334,8334),(9002,9002),(9141,9141),(10089,10089),(10091,10091),(10093,10093),(10095,10095),(10097,10097),(10099,10099),(10101,10101),(10215,10215),(10217,10217),(10219,10219),(10628,10628),(10630,10630),(10632,10632),(10634,10634),(10636,10636),(10638,10638),(10640,10640),(10642,10642),(10644,10644),(10646,10646),(10648,10648),(10713,10713),(10715,10715),(10749,10749),(12297,12297),(12299,12299),(12301,12301),(12303,12303),(12305,12305),(12309,12309),(12311,12311),(12313,12313),(12315,12315),(12318,12319),(64831,64831),(65078,65078),(65080,65080),(65082,65082),(65084,65084),(65086,65086),(65088,65088),(65090,65090),(65092,65092),(65096,65096),(65114,65114),(65116,65116),(65118,65118),(65289,65289),(65341,65341),(65373,65373),(65376,65376),(65379,65379)]),
    ("Pf", &[(187,187),(8217,8217),(8221,8221),(8250,8250)]),
    ("Pi", &[(171,171),(8216,8216),(8219,8220),(8223,8223),(8249,8249)]),
    ("Po", &[(33,35),(37,39),(42,42),(44,44),(46,47),(58,59),(63,64),(92,92),(161,161),(183,183),(191,191),(894,894),(903,903),(1370,1375),(1417,1417),(1470,1470),(1472,1472),(1475,1475),(1523,1524),(1548,1549),(1563,1563),(1567,1567),(1642,1645),(1748,1748),(1792,1805),(2404,2405),(2416,2416),(3572,3572),(3663,3663),(3674,3675),(3844,3858),(3973,3973),(4170,4175),(4347,4347),(4961,4968),(5741,5742),(5867,5869),(5941,5942),(6100,6102),(6104,6106),(6144,6149),(6151,6154),(6468,6469),(8214,8215),(8224,8231),(8240,8248),(8251,8254),(8257,8259),(8263,8273),(8275,8275),(8279,8279),(9142,9142),(12289,12291),(12349,12349),(65072,65072),(65093,65094),(65097,65100),(65104,65106),(65108,65111),(65119,65121),(65128,65128),(65130,65131),(65281,65283),(65285,65287),(65290,65290),(65292,65292),(65294,65295),(65306,65307),(65311,65312),(65340,65340),(65377,65377),(65380,65380),(65792,65793),(66463,66463)]),
    ("Ps", &[(40,40),(91,91),(123,123),(3898,3898),(3900,3900),(5787,5787),(8218,8218),(8222,8222),(8261,8261),(8317,8317),(8333,8333),(9001,9001),(9140,9140),(10088,10088),(10090,10090),(10092,10092),(10094,10094),(10096,10096),(10098,10098),(10100,10100),(10214,10214),(10216,10216),(10218,10218),(10627,10627),(10629,10629),(10631,10631),(10633,10633),(10635,10635),(10637,10637),(10639,10639),(10641,10641),(10643,10643),(10645,10645),(10647,10647),(10712,10712),(10714,10714),(10748,10748),(12296,12296),(12298,12298),(12300,12300),(12302,12302),(12304,12304),(12308,12308),(12310,12310),(12312,12312),(12314,12314),(12317,12317),(64830,64830),(65077,65077),(65079,65079),(65081,65081),(65083,65083),(65085,65085),(65087,65087),(65089,65089),(65091,65091),(65095,65095),(65113,65113),(65115,65115),(65117,65117),(65288,65288),(65339,65339),(65371,65371),(65375,65375),(65378,65378)]),
    ("S", &[(36,36),(43,43),(60,62),(94,94),(96,96),(124,124),(126,126),(162,169),(172,172),(174,177),(180,180),(182,182),(184,184),(215,215),(247,247),(706,709),(722,735),(741,749),(751,767),(884,885),(900,901),(1014,1014),(1154,1154),(1550,1551),(1769,1769),(1789,1790),(2546,2547),(2554,2554),(2801,2801),(2928,2928),(3059,3066),(3647,3647),(3841,3843),(3859,3863),(3866,3871),(3892,3892),(3894,3894),(3896,3896),(4030,4037),(4039,4044),(4047,4047),(6107,6107),(6464,6464),(6624,6655),(8125,8125),(8127,8129),(8141,8143),(8157,8159),(8173,8175),(8189,8190),(8260,8260),(8274,8274),(8314,8316),(8330,8332),(8352,8369),(8448,8449),(8451,8454),(8456,8457),(8468,8468),(8470,8472),(8478,8483),(8485,8485),(8487,8487),(8489,8489),(8494,8494),(8498,8498),(8506,8507),(8512,8516),(8522,8523),(8592,9000),(9003,9139),(9143,9168),(9216,9254),(9280,9290),(9372,9449),(9472,9751),(9753,9853),(9856,9873),(9888,9889),(9985,9988),(9990,9993),(9996,10023),(10025,10059),(10061,10061),(10063,10066),(10070,10070),(10072,10078),(10081,10087),(10132,10132),(10136,10159),(10161,10174),(10192,10213),(10224,10626),(10649,10711),(10716,10747),(10750,11021),(11904,11929),(11931,12019),(12032,12245),(12272,12283),(12292,12292),(12306,12307),(12320,12320),(12342,12343),(12350,12351),(12443,12444),(12688,12689),(12694,12703),(12800,12830),(12842,12867),(12880,12880),(12896,12925),(12927,12927),(12938,12976),(12992,13054),(13056,13311),(19904,19967),(42128,42182),(64297,64297),(65020,65021),(65122,65122),(65124,65126),(65129,65129),(65284,65284),(65291,65291),(65308,65310),(65342,65342),(65344,65344),(65372,65372),(65374,65374),(65504,65510),(65512,65518),(65532,65533),(65794,65794),(65847,65855),(118784,119029),(119040,119078),(119082,119140),(119146,119148),(119171,119172),(119180,119209),(119214,119261),(119552,119638),(120513,120513),(120539,120539),(120571,120571),(120597,120597),(120629,120629),(120655,120655),(120687,120687),(120713,120713),(120745,120745),(120771,120771)]),
    ("Sc", &[(36,36),(162,165),(2546,2547),(2801,2801),(3065,3065),(3647,3647),(6107,6107),(8352,8369),(65020,65020),(65129,65129),(65284,65284),(65504,65505),(65509,65510)]),
    ("Sk", &[(94,94),(96,96),(168,168),(175,175),(180,180),(184,184),(706,709),(722,735),(741,749),(751,767),(884,885),(900,901),(8125,8125),(8127,8129),(8141,8143),(8157,8159),(8173,8175),(8189,8190),(12443,12444),(65342,65342),(65344,65344),(65507,65507)]),
    ("Sm", &[(43,43),(60,62),(124,124),(126,126),(172,172),(177,177),(215,215),(247,247),(1014,1014),(8260,8260),(8274,8274),(8314,8316),(8330,8332),(8512,8516),(8523,8523),(8592,8596),(8602,8603),(8608,8608),(8611,8611),(8614,8614),(8622,8622),(8654,8655),(8658,8658),(8660,8660),(8692,8959),(8968,8971),(8992,8993),(9084,9084),(9115,9139),(9655,9655),(9665,9665),(9720,9727),(9839,9839),(10192,10213),(10224,10239),(10496,10626),(10649,10711),(10716,10747),(10750,11007),(64297,64297),(65122,65122),(65124,65126),(65291,65291),(65308,65310),(65372,65372),(65374,65374),(65506,65506),(65513,65516),(120513,120513),(120539,120539),(120571,120571),(120597,120597),(120629,120629),(120655,120655),(120687,120687),(120713,120713),(120745,120745),(120771,120771)]),
    ("So", &[(166,167),(169,169),(174,174),(176,176),(182,182),(1154,1154),(1550,1551),(1769,1769),(1789,1790),(2554,2554),(2928,2928),(3059,3064),(3066,3066),(3841,3843),(3859,3863),(3866,3871),(3892,3892),(3894,3894),(3896,3896),(4030,4037),(4039,4044),(4047,4047),(6464,6464),(6624,6655),(8448,8449),(8451,8454),(8456,8457),(8468,8468),(8470,8472),(8478,8483),(8485,8485),(8487,8487),(8489,8489),(8494,8494),(8498,8498),(8506,8507),(8522,8522),(8597,8601),(8604,8607),(8609,8610),(8612,8613),(8615,8621),(8623,8653),(8656,8657),(8659,8659),(8661,8691),(8960,8967),(8972,8991),(8994,9000),(9003,9083),(9085,9114),(9143,9168),(9216,9254),(9280,9290),(9372,9449),(9472,9654),(9656,9664),(9666,9719),(9728,9751),(9753,9838),(9840,9853),(9856,9873),(9888,9889),(9985,9988),(9990,9993),(9996,10023),(10025,10059),(10061,10061),(10063,10066),(10070,10070),(10072,10078),(10081,10087),(10132,10132),(10136,10159),(10161,10174),(10240,10495),(11008,11021),(11904,11929),(11931,12019),(12032,12245),(12272,12283),(12292,12292),(12306,12307),(12320,12320),(12342,12343),(12350,12351),(12688,12689),(12694,12703),(12800,12830),(12842,12867),(12880,12880),(12896,12925),(12927,12927),(12938,12976),(12992,13054),(13056,13311),(19904,19967),(42128,42182),(65021,65021),(65508,65508),(65512,65512),(65517,65518),(65532,65533),(65794,65794),(65847,65855),(118784,119029),(119040,119078),(119082,119140),(119146,119148),(119171,119172),(119180,119209),(119214,119261),(119552,119638)]),
    ("Z", &[(32,32),(160,160),(5760,5760),(6158,6158),(8192,8202),(8232,8233),(8239,8239),(8287,8287),(12288,12288)]),
    ("Zl", &[(8232,8232)]),
    ("Zp", &[(8233,8233)]),
    ("Zs", &[(32,32),(160,160),(5760,5760),(6158,6158),(8192,8202),(8239,8239),(8287,8287),(12288,12288)]),
];

/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlUCSIsBlock(int code, const char *block);
/// ```
///
/// Legacy (xmlregexp.c): 1 if `code` is in the named Unicode block, 0 if
/// not, -1 for an unknown block name.
#[no_mangle]
pub unsafe extern "C" fn xmlUCSIsBlock(code: c_int, block: *const c_char) -> c_int {
    if block.is_null() || XML_UCS_BLOCKS.is_empty() {
        return -1;
    }
    let mut low = 0usize;
    let mut high = XML_UCS_BLOCKS.len() - 1;
    while low <= high {
        let mid = (low + high) / 2;
        let (name, ranges) = XML_UCS_BLOCKS[mid];
        match unsafe { cstr_cmp(block, name.as_bytes()) } {
            core::cmp::Ordering::Equal => return ucs_in_ranges(code, ranges),
            core::cmp::Ordering::Less => {
                if mid == 0 {
                    break;
                }
                high = mid - 1;
            }
            core::cmp::Ordering::Greater => low = mid + 1,
        }
    }
    -1
}

/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlUCSIsCat(int code, const char *cat);
/// ```
///
/// Legacy: 1 if `code` belongs to the named Unicode general category, 0 if
/// not, -1 for an unknown category name.
#[no_mangle]
pub unsafe extern "C" fn xmlUCSIsCat(code: c_int, cat: *const c_char) -> c_int {
    if cat.is_null() || XML_UCS_CATS.is_empty() {
        return -1;
    }
    let mut low = 0usize;
    let mut high = XML_UCS_CATS.len() - 1;
    while low <= high {
        let mid = (low + high) / 2;
        let (name, ranges) = XML_UCS_CATS[mid];
        match unsafe { cstr_cmp(cat, name.as_bytes()) } {
            core::cmp::Ordering::Equal => return ucs_in_ranges(code, ranges),
            core::cmp::Ordering::Less => {
                if mid == 0 {
                    break;
                }
                high = mid - 1;
            }
            core::cmp::Ordering::Greater => low = mid + 1,
        }
    }
    -1
}

/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlUCSIsCatCc(int code);
/// ```
///
/// Control characters (C0 + DEL + C1).
#[no_mangle]
pub extern "C" fn xmlUCSIsCatCc(code: c_int) -> c_int {
    if (code >= 0x0 && code <= 0x1f) || (code >= 0x7f && code <= 0x9f) {
        1
    } else {
        0
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 4. valid family
// ═══════════════════════════════════════════════════════════════════════════════

/// Normalize an attribute value in place (upstream valid.c
/// `xmlValidNormalizeString`): trim leading/trailing spaces and collapse
/// runs of spaces to a single space.
unsafe fn normalize_string(str_: *mut xmlChar) {
    if str_.is_null() {
        return;
    }
    let mut src = str_;
    let mut dst = str_;
    while unsafe { *src == 0x20 } {
        src = src.add(1);
    }
    loop {
        let c = unsafe { *src };
        if c == 0 {
            break;
        }
        if c == 0x20 {
            while unsafe { *src == 0x20 } {
                src = src.add(1);
            }
            if unsafe { *src != 0 } {
                unsafe {
                    *dst = 0x20;
                }
                dst = dst.add(1);
            }
        } else {
            unsafe {
                *dst = *src;
            }
            dst = dst.add(1);
            src = src.add(1);
        }
    }
    unsafe {
        *dst = 0;
    }
}

/// Report a validation memory error (upstream `xmlVErrMemory`).
unsafe fn v_err_memory(ctxt: *mut _xmlValidCtxt) {
    if !ctxt.is_null() {
        unsafe {
            (*ctxt).valid = 0;
        }
    }
    unsafe {
        crate::xml::errors::raise_error(
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            XML_FROM_VALID,
            XML_ERR_NO_MEMORY,
            XML_ERR_FATAL as c_int,
            ptr::null(),
            0,
            ptr::null(),
            ptr::null(),
            ptr::null(),
            0,
            0,
            b"Memory allocation failed : \n\0".as_ptr() as *const c_char,
        );
    }
}

// XML_DTD_NOT_STANDALONE (xmlerror.h, value 530) — not re-exported by the
// candidate's constants module.
const XML_DTD_NOT_STANDALONE: c_int = 530;

/// Report a validation error with node context (upstream `xmlErrValidNode`),
/// and clear `ctxt->valid`.
unsafe fn v_err_valid_node(
    ctxt: *mut _xmlValidCtxt,
    _node: *mut _xmlNode,
    code: c_int,
    msg: *const c_char,
    str1: *const xmlChar,
    str2: *const xmlChar,
) {
    if !ctxt.is_null() {
        unsafe {
            (*ctxt).valid = 0;
        }
    }
    let mut buf = Vec::new();
    let mut arg_idx = 0;
    if !msg.is_null() {
        let mut i = 0usize;
        loop {
            let c = unsafe { *((msg as *const u8).add(i)) };
            if c == 0 {
                break;
            }
            if c == b'%' {
                let c2 = unsafe { *((msg as *const u8).add(i + 1)) };
                if c2 == b's' {
                    if arg_idx == 0 {
                        append_xmlstr(&mut buf, str1);
                    } else {
                        append_xmlstr(&mut buf, str2);
                    }
                    arg_idx += 1;
                    i += 2;
                    continue;
                }
            }
            buf.push(c);
            i += 1;
        }
    }
    buf.push(0);
    unsafe {
        crate::xml::errors::raise_error(
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            XML_FROM_VALID,
            code,
            XML_ERR_ERROR as c_int,
            ptr::null(),
            0,
            ptr::null(),
            ptr::null(),
            ptr::null(),
            0,
            0,
            buf.as_ptr() as *const c_char,
        );
    }
}

/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlValidNormalizeAttributeValue(xmlDoc *doc, xmlNode *elem,
///                                          const xmlChar *name,
///                                          const xmlChar *value);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlValidNormalizeAttributeValue(
    doc: *mut _xmlDoc,
    elem: *mut _xmlNode,
    name: *const xmlChar,
    value: *const xmlChar,
) -> *mut xmlChar {
    if doc.is_null() || elem.is_null() || name.is_null() || value.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        let mut attr_decl =
            crate::xml::validation::get_dtd_attr_desc((*doc).intSubset, (*elem).name, name);
        if attr_decl.is_null() && !(*doc).extSubset.is_null() {
            attr_decl =
                crate::xml::validation::get_dtd_attr_desc((*doc).extSubset, (*elem).name, name);
        }
        if attr_decl.is_null() || (*attr_decl).atype == XML_ATTRIBUTE_CDATA as c_int {
            return ptr::null_mut();
        }
        let ret = crate::abi::exports_xml2::xmlStrdup(value);
        if ret.is_null() {
            return ptr::null_mut();
        }
        normalize_string(ret);
        ret
    }
}

/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlValidCtxtNormalizeAttributeValue(xmlValidCtxt *ctxt,
///                                              xmlDoc *doc, xmlNode *elem,
///                                              const xmlChar *name,
///                                              const xmlChar *value);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlValidCtxtNormalizeAttributeValue(
    ctxt: *mut _xmlValidCtxt,
    doc: *mut _xmlDoc,
    elem: *mut _xmlNode,
    name: *const xmlChar,
    value: *const xmlChar,
) -> *mut xmlChar {
    if doc.is_null() || elem.is_null() || name.is_null() || value.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        let mut prefix: *mut xmlChar = ptr::null_mut();
        let local_name = crate::xml::string::split_qname2(name, &mut prefix);
        if local_name.is_null() {
            if !prefix.is_null() {
                xmlFree(prefix as *mut c_void);
            }
            v_err_memory(ctxt);
            return ptr::null_mut();
        }

        let mut attr_decl: *mut _xmlAttribute = ptr::null_mut();
        let mut extsubset = 0;

        let ns = (*elem).ns;
        if !ns.is_null() && !(*ns).prefix.is_null() {
            let mut buf = [0u8; 50];
            let elemname = crate::xml::string::build_qname(
                (*elem).name,
                (*ns).prefix,
                buf.as_mut_ptr() as *mut xmlChar,
                50,
            );
            if elemname.is_null() {
                if !prefix.is_null() {
                    xmlFree(prefix as *mut c_void);
                }
                v_err_memory(ctxt);
                return ptr::null_mut();
            }
            if !(*doc).intSubset.is_null() {
                attr_decl = crate::xml::hash::hash_lookup3(
                    (*(*doc).intSubset).attributes as *mut crate::xml::hash::HashTable,
                    local_name,
                    prefix,
                    elemname,
                ) as *mut _xmlAttribute;
            }
            if attr_decl.is_null() && !(*doc).extSubset.is_null() {
                attr_decl = crate::xml::hash::hash_lookup3(
                    (*(*doc).extSubset).attributes as *mut crate::xml::hash::HashTable,
                    local_name,
                    prefix,
                    elemname,
                ) as *mut _xmlAttribute;
                if !attr_decl.is_null() {
                    extsubset = 1;
                }
            }
            if elemname as *const xmlChar != (*elem).name {
                xmlFree(elemname as *mut c_void);
            }
        }
        if attr_decl.is_null() && !(*doc).intSubset.is_null() {
            attr_decl = crate::xml::hash::hash_lookup3(
                (*(*doc).intSubset).attributes as *mut crate::xml::hash::HashTable,
                local_name,
                prefix,
                (*elem).name,
            ) as *mut _xmlAttribute;
        }
        if attr_decl.is_null() && !(*doc).extSubset.is_null() {
            attr_decl = crate::xml::hash::hash_lookup3(
                (*(*doc).extSubset).attributes as *mut crate::xml::hash::HashTable,
                local_name,
                prefix,
                (*elem).name,
            ) as *mut _xmlAttribute;
            if !attr_decl.is_null() {
                extsubset = 1;
            }
        }

        if attr_decl.is_null() || (*attr_decl).atype == XML_ATTRIBUTE_CDATA as c_int {
            if !prefix.is_null() {
                xmlFree(prefix as *mut c_void);
            }
            return ptr::null_mut();
        }
        let ret = crate::abi::exports_xml2::xmlStrdup(value);
        if ret.is_null() {
            if !prefix.is_null() {
                xmlFree(prefix as *mut c_void);
            }
            v_err_memory(ctxt);
            return ptr::null_mut();
        }
        normalize_string(ret);
        if (*doc).standalone != 0
            && extsubset == 1
            && crate::abi::exports_xml2::xmlStrEqual(value, ret) == 0
        {
            v_err_valid_node(
                ctxt,
                elem,
                XML_DTD_NOT_STANDALONE,
                b"standalone: %s on %s value had to be normalized based on external subset declaration\n\0"
                    .as_ptr() as *const c_char,
                name,
                (*elem).name,
            );
        }
        if !prefix.is_null() {
            xmlFree(prefix as *mut c_void);
        }
        ret
    }
}

/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidGetPotentialChildren(xmlElementContent *ctree,
///                                  const xmlChar **names,
///                                  int *len, int max);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlValidGetPotentialChildren(
    ctree: *mut _xmlElementContent,
    names: *mut *const xmlChar,
    len: *mut c_int,
    max: c_int,
) -> c_int {
    if ctree.is_null() || names.is_null() || len.is_null() {
        return -1;
    }
    unsafe {
        if *len >= max {
            return *len;
        }
        let pcdata = b"#PCDATA\0";
        match (*ctree).type_ as u32 {
            t if t == XML_ELEMENT_CONTENT_PCDATA as u32 => {
                for i in 0..(*len as usize) {
                    if crate::abi::exports_xml2::xmlStrEqual(
                        pcdata.as_ptr() as *const xmlChar,
                        *names.add(i),
                    ) != 0
                    {
                        return *len;
                    }
                }
                *names.add(*len as usize) = pcdata.as_ptr() as *const xmlChar;
                *len += 1;
            }
            t if t == XML_ELEMENT_CONTENT_ELEMENT as u32 => {
                for i in 0..(*len as usize) {
                    if crate::abi::exports_xml2::xmlStrEqual((*ctree).name, *names.add(i)) != 0 {
                        return *len;
                    }
                }
                *names.add(*len as usize) = (*ctree).name;
                *len += 1;
            }
            t if t == XML_ELEMENT_CONTENT_SEQ as u32 || t == XML_ELEMENT_CONTENT_OR as u32 => {
                xmlValidGetPotentialChildren((*ctree).c1, names, len, max);
                xmlValidGetPotentialChildren((*ctree).c2, names, len, max);
            }
            _ => {}
        }
        *len
    }
}

/// Dummy validity error handler that suppresses messages (upstream
/// `xmlNoValidityErr`).
unsafe extern "C" fn xml_no_validity_err(_ctx: *mut c_void, _msg: *const c_char) {}

/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidGetValidElements(xmlNode *prev, xmlNode *next,
///                              const xmlChar **names, int max);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlValidGetValidElements(
    prev: *mut _xmlNode,
    next: *mut _xmlNode,
    names: *mut *const xmlChar,
    max: c_int,
) -> c_int {
    if prev.is_null() && next.is_null() {
        return -1;
    }
    if names.is_null() || max <= 0 {
        return -1;
    }
    unsafe {
        // Local validation context with errors suppressed, exactly like the
        // upstream xmlValidCtxt stack instance in valid.c.
        let mut vctxt: _xmlValidCtxt = zeroed();
        vctxt.error = Some(xml_no_validity_err);

        let mut nb_valid_elements = 0;
        let ref_node = if prev.is_null() { next } else { prev };
        let parent = (*ref_node).parent;

        /*
         * Retrieves the parent element declaration.
         */
        let mut element_desc = if (*parent).doc.is_null() {
            ptr::null_mut()
        } else {
            crate::xml::validation::get_dtd_element_desc((*(*parent).doc).intSubset, (*parent).name)
        };
        if element_desc.is_null()
            && !(*parent).doc.is_null()
            && !(*(*parent).doc).extSubset.is_null()
        {
            element_desc = crate::xml::validation::get_dtd_element_desc(
                (*(*parent).doc).extSubset,
                (*parent).name,
            );
        }
        if element_desc.is_null() {
            return -1;
        }

        /*
         * Do a backup of the current tree structure.
         */
        let prev_next = if prev.is_null() {
            ptr::null_mut()
        } else {
            (*prev).next
        };
        let next_prev = if next.is_null() {
            ptr::null_mut()
        } else {
            (*next).prev
        };
        let parent_childs = (*parent).children;
        let parent_last = (*parent).last;

        /*
         * Create a dummy node and insert it into the tree.
         */
        let dummy_name = b"<!dummy?>\0";
        let test_node = crate::abi::exports_tree::xmlNewDocNode(
            (*ref_node).doc,
            ptr::null_mut(),
            dummy_name.as_ptr() as *const xmlChar,
            ptr::null(),
        );
        if test_node.is_null() {
            return -1;
        }

        (*test_node).parent = parent;
        (*test_node).prev = prev;
        (*test_node).next = next;
        let name = (*test_node).name;

        if prev.is_null() {
            (*parent).children = test_node;
        } else {
            (*prev).next = test_node;
        }
        if next.is_null() {
            (*parent).last = test_node;
        } else {
            (*next).prev = test_node;
        }

        /*
         * Insert each potential child node and check if the parent is
         * still valid.
         */
        let mut elements: [*const xmlChar; 256] = [ptr::null(); 256];
        let mut nb_elements = 0;
        nb_elements = xmlValidGetPotentialChildren(
            (*element_desc).content,
            elements.as_mut_ptr(),
            &mut nb_elements,
            256,
        );

        let mut i = 0;
        while i < nb_elements {
            (*test_node).name = elements[i as usize];
            if crate::xml::validation::validate_one_element(&mut vctxt, (*parent).doc, parent) != 0
            {
                let mut j = 0;
                while j < nb_valid_elements {
                    if crate::abi::exports_xml2::xmlStrEqual(
                        elements[i as usize],
                        *names.add(j as usize),
                    ) != 0
                    {
                        break;
                    }
                    j += 1;
                }
                if j >= nb_valid_elements {
                    *names.add(nb_valid_elements as usize) = elements[i as usize];
                    nb_valid_elements += 1;
                    if nb_valid_elements >= max {
                        break;
                    }
                }
            }
            i += 1;
        }

        /*
         * Restore the tree structure.
         */
        if prev.is_null() {
            (*parent).children = parent_childs;
        } else {
            (*prev).next = prev_next;
        }
        if next.is_null() {
            (*parent).last = parent_last;
        } else {
            (*next).prev = next_prev;
        }

        /*
         * Free the dummy node.
         */
        (*test_node).name = name;
        crate::abi::exports_xml2::xmlFreeNode(test_node);

        nb_valid_elements
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 5. misc2 family
// ═══════════════════════════════════════════════════════════════════════════════

// ── __xml* aliases (globals.c / xmlIO.c) ───────────────────────────────────────

/// Upstream `__xmlDefaultSAXHandler(void)` — pointer to `xmlDefaultSAXHandler`.
#[no_mangle]
pub unsafe extern "C" fn __xmlDefaultSAXHandler() -> *mut _xmlSAXHandlerV1 {
    // SAFETY: returning a pointer to an exported static; the caller may
    // read/write it exactly as with upstream's deprecated accessor.
    core::ptr::addr_of!(crate::abi::data_globals::xmlDefaultSAXHandler) as *mut _xmlSAXHandlerV1
}

/// Upstream `__xmlDefaultSAXLocator(void)` — pointer to `xmlDefaultSAXLocator`.
#[no_mangle]
pub unsafe extern "C" fn __xmlDefaultSAXLocator() -> *mut _xmlSAXLocator {
    // SAFETY: returning a pointer to an exported static; the caller may
    // read/write it exactly as with upstream's deprecated accessor.
    core::ptr::addr_of!(crate::abi::data_globals::xmlDefaultSAXLocator) as *mut _xmlSAXLocator
}

/// Upstream `__xmlLastError(void)` — pointer to the exported `xmlLastError`
/// mirror (kept in sync with the thread-local error state on every raise).
#[no_mangle]
pub unsafe extern "C" fn __xmlLastError() -> *mut _xmlError {
    // SAFETY: returning a pointer to an exported static; the caller may
    // read/write it exactly as with upstream's deprecated accessor.
    core::ptr::addr_of!(crate::abi::data_globals::xmlLastError) as *mut _xmlError
}

/// Upstream `__xmlParserInputBufferCreateFilename(const char *URI,
/// xmlCharEncoding enc)` — the non-reentrant internal variant; forwards to
/// the regular `xmlParserInputBufferCreateFilename`.
#[no_mangle]
pub unsafe extern "C" fn __xmlParserInputBufferCreateFilename(
    URI: *const c_char,
    enc: c_int,
) -> *mut _xmlParserInputBuffer {
    crate::abi::exports_xml2::xmlParserInputBufferCreateFilename(URI, enc)
}

// ── xmlFormatError (error.c) ───────────────────────────────────────────────────

/// Compute the input window around `input->cur` (upstream parserInternals.c
/// `xmlParserInputGetWindow`).
unsafe fn parser_input_get_window(
    input: *mut _xmlParserInput,
    start_out: *mut *const xmlChar,
    size_in_out: *mut c_int,
    offset_out: *mut c_int,
) {
    unsafe {
        let mut cur = (*input).cur;
        let base = (*input).base;
        let size = *size_in_out;
        // Skip backwards over any end-of-lines.
        while cur > base && (*cur == b'\n' || *cur == b'\r') {
            cur = cur.sub(1);
        }
        let mut n: usize = 0;
        // Search backwards for beginning-of-line (to max buff size).
        while n < size as usize && cur > base && *cur != b'\n' && *cur != b'\r' {
            cur = cur.sub(1);
            n += 1;
        }
        if n > 0 && (*cur == b'\n' || *cur == b'\r') {
            cur = cur.add(1);
        } else {
            // Skip over continuation bytes.
            while cur < (*input).cur && (*cur & 0xC0) == 0x80 {
                cur = cur.add(1);
            }
        }
        // Calculate the error position in terms of the current position.
        let mut col = (*input).cur as usize - cur as usize;
        // Search forward for end-of-line (to max buff size).
        let mut nfwd: usize = 0;
        let start = cur;
        while *cur != 0 && *cur != b'\n' && *cur != b'\r' {
            let avail = (*input).end as usize - cur as usize;
            let mut clen: c_int = avail as c_int;
            let c = xmlGetUTF8Char(cur, &mut clen);
            if c < 0 || nfwd + clen as usize > size as usize {
                break;
            }
            cur = cur.add(clen as usize);
            nfwd += clen as usize;
        }
        if col >= nfwd {
            col = if nfwd < size as usize {
                nfwd
            } else {
                size as usize - 1
            };
        }
        *start_out = start;
        *size_in_out = nfwd as c_int;
        *offset_out = col as c_int;
    }
}

/// Print the source context around an input position (upstream error.c
/// `xmlParserPrintFileContextInternal`).
unsafe fn print_file_context(
    input: *mut _xmlParserInput,
    channel: xmlGenericErrorFunc,
    data: *mut c_void,
) {
    if input.is_null() || unsafe { (*input).cur.is_null() } {
        return;
    }
    let mut n: c_int = 80;
    let mut start: *const xmlChar = ptr::null();
    let mut col: c_int = 0;
    unsafe { parser_input_get_window(input, &mut start, &mut n, &mut col) };
    let mut content = [0u8; 81];
    if n > 0 && !start.is_null() {
        unsafe {
            ptr::copy_nonoverlapping(start, content.as_mut_ptr(), n as usize);
        }
    }
    content[n as usize] = 0;
    unsafe { chan_emit(channel, data, &content[..n as usize]) };
    // Create blank line with problem pointer.
    let mut i = 0usize;
    while i < col as usize {
        if content[i] != b'\t' {
            content[i] = b' ';
        }
        i += 1;
    }
    content[i] = b'^';
    i += 1;
    content[i] = 0;
    unsafe { chan_emit(channel, data, &content[..i]) };
}

/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlFormatError(const xmlError *err, xmlGenericErrorFunc channel,
///                     void *data);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlFormatError(
    err: *const _xmlError,
    channel: Option<xmlGenericErrorFunc>,
    data: *mut c_void,
) {
    if err.is_null() || channel.is_none() {
        return;
    }
    let channel = channel.unwrap();
    unsafe {
        let e = &*err;
        let message = e.message;
        let file = e.file;
        let line = e.line;
        let code = e.code;
        let domain = e.domain;
        let level = e.level;
        let node = e.node as *mut _xmlNode;

        if code == XML_ERR_OK {
            return;
        }

        let mut ctxt: *mut _xmlParserCtxt = ptr::null_mut();
        if domain == XML_FROM_PARSER
            || domain == XML_FROM_HTML
            || domain == XML_FROM_DTD
            || domain == XML_FROM_NAMESPACE
            || domain == XML_FROM_IO
            || domain == XML_FROM_VALID
        {
            ctxt = e.ctxt as *mut _xmlParserCtxt;
        }

        let mut name: *const xmlChar = ptr::null();
        if !node.is_null()
            && (*node).type_ == XML_ELEMENT_NODE as c_int
            && domain != XML_FROM_SCHEMASV
        {
            name = (*node).name;
        }

        let mut input: *mut _xmlParserInput = ptr::null_mut();
        let mut cur: *mut _xmlParserInput = ptr::null_mut();
        if !ctxt.is_null() && !(*ctxt).input.is_null() {
            input = (*ctxt).input;
            if (*input).filename.is_null() && (*ctxt).inputNr > 1 {
                cur = input;
                input = *(*ctxt).inputTab.add((*ctxt).inputNr as usize - 2);
            }
            if !(*input).filename.is_null() {
                let mut buf = Vec::new();
                append_cstr(&mut buf, (*input).filename);
                buf.push(b':');
                append_int(&mut buf, (*input).line);
                buf.extend_from_slice(b": ");
                chan_emit(channel, data, &buf);
            } else if line != 0 && domain == XML_FROM_PARSER {
                let mut buf = b"Entity: line ".to_vec();
                append_int(&mut buf, (*input).line);
                buf.extend_from_slice(b": ");
                chan_emit(channel, data, &buf);
            }
        } else {
            if !file.is_null() {
                let mut buf = Vec::new();
                append_cstr(&mut buf, file);
                buf.push(b':');
                append_int(&mut buf, line);
                buf.extend_from_slice(b": ");
                chan_emit(channel, data, &buf);
            } else if line != 0
                && (domain == XML_FROM_PARSER
                    || domain == XML_FROM_SCHEMASV
                    || domain == XML_FROM_SCHEMASP
                    || domain == XML_FROM_DTD
                    || domain == XML_FROM_RELAXNGP
                    || domain == XML_FROM_RELAXNGV)
            {
                let mut buf = b"Entity: line ".to_vec();
                append_int(&mut buf, line);
                buf.extend_from_slice(b": ");
                chan_emit(channel, data, &buf);
            }
        }
        if !name.is_null() {
            let mut buf = b"element ".to_vec();
            append_xmlstr(&mut buf, name);
            buf.extend_from_slice(b": ");
            chan_emit(channel, data, &buf);
        }
        let domain_prefix: &[u8] = match domain {
            XML_FROM_PARSER => b"parser ",
            XML_FROM_NAMESPACE => b"namespace ",
            XML_FROM_DTD | XML_FROM_VALID => b"validity ",
            XML_FROM_HTML => b"HTML parser ",
            XML_FROM_MEMORY => b"memory ",
            XML_FROM_OUTPUT => b"output ",
            XML_FROM_IO => b"I/O ",
            XML_FROM_XINCLUDE => b"XInclude ",
            XML_FROM_XPATH => b"XPath ",
            XML_FROM_XPOINTER => b"parser ",
            XML_FROM_REGEXP => b"regexp ",
            XML_FROM_MODULE => b"module ",
            XML_FROM_SCHEMASV => b"Schemas validity ",
            XML_FROM_SCHEMASP => b"Schemas parser ",
            XML_FROM_RELAXNGP => b"Relax-NG parser ",
            XML_FROM_RELAXNGV => b"Relax-NG validity ",
            XML_FROM_CATALOG => b"Catalog ",
            XML_FROM_C14N => b"C14N ",
            XML_FROM_XSLT => b"XSLT ",
            XML_FROM_I18N => b"encoding ",
            XML_FROM_SCHEMATRONV => b"schematron ",
            XML_FROM_BUFFER => b"internal buffer ",
            XML_FROM_URI => b"URI ",
            _ => b"",
        };
        chan_emit(channel, data, domain_prefix);
        let level_prefix: &[u8] = if level == XML_ERR_NONE as c_int {
            b": "
        } else if level == XML_ERR_WARNING as c_int {
            b"warning : "
        } else if level == XML_ERR_ERROR as c_int || level == XML_ERR_FATAL as c_int {
            b"error : "
        } else {
            b": "
        };
        chan_emit(channel, data, level_prefix);
        if !message.is_null() {
            let len = cstr_len(message as *const c_char);
            let msg_bytes = core::slice::from_raw_parts(message as *const u8, len);
            let mut buf = msg_bytes.to_vec();
            if len > 0 && *message.add(len - 1) as u8 != b'\n' {
                buf.push(b'\n');
            }
            chan_emit(channel, data, &buf);
        } else {
            chan_emit(channel, data, b"No error message provided\n");
        }

        if !ctxt.is_null() {
            if !input.is_null()
                && ((*input).buf.is_null() || (*(*input).buf).encoder.is_null())
                && code == XML_ERR_INVALID_ENCODING
                && (*input).cur < (*input).end
            {
                let mut buf = b"Bytes:".to_vec();
                for i in 0..4 {
                    if (*input).cur.add(i) >= (*input).end {
                        break;
                    }
                    let b = *(*input).cur.add(i);
                    buf.extend_from_slice(b" 0x");
                    buf.push(hex_digit(b >> 4));
                    buf.push(hex_digit(b & 0xf));
                }
                buf.push(b'\n');
                chan_emit(channel, data, &buf);
            }
            print_file_context(input, channel, data);
            if !cur.is_null() {
                if !(*cur).filename.is_null() {
                    let mut buf = Vec::new();
                    append_cstr(&mut buf, (*cur).filename);
                    buf.extend_from_slice(b": ");
                    append_int(&mut buf, (*cur).line);
                    buf.extend_from_slice(b": \n");
                    chan_emit(channel, data, &buf);
                } else if line != 0
                    && (domain == XML_FROM_PARSER
                        || domain == XML_FROM_SCHEMASV
                        || domain == XML_FROM_SCHEMASP
                        || domain == XML_FROM_DTD
                        || domain == XML_FROM_RELAXNGP
                        || domain == XML_FROM_RELAXNGV)
                {
                    let mut buf = b"Entity: line ".to_vec();
                    append_int(&mut buf, (*cur).line);
                    buf.extend_from_slice(b": \n");
                    chan_emit(channel, data, &buf);
                }
                print_file_context(cur, channel, data);
            }
        }
        if domain == XML_FROM_XPATH
            && !e.str1.is_null()
            && e.int1 < 100
            && e.int1 < crate::abi::exports_xml2::xmlStrlen(e.str1 as *const xmlChar)
        {
            let mut buf = Vec::new();
            append_cstr(&mut buf, e.str1);
            buf.push(b'\n');
            chan_emit(channel, data, &buf);
            let mut marker = vec![b' '; e.int1 as usize];
            marker.push(b'^');
            marker.push(b'\n');
            chan_emit(channel, data, &marker);
        }
    }
}

// ── tree node constructors (tree.c) ───────────────────────────────────────────

/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNode *xmlNewCharRef(xmlDoc *doc, const xmlChar *name);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlNewCharRef(doc: *mut _xmlDoc, name: *const xmlChar) -> *mut _xmlNode {
    if name.is_null() {
        return ptr::null_mut();
    }
    crate::abi::exports_tree::xmlNewReference(doc, name)
}

/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNode *xmlNewDocText(const xmlDoc *doc, const xmlChar *content);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlNewDocText(
    doc: *const _xmlDoc,
    content: *const xmlChar,
) -> *mut _xmlNode {
    let cur = unsafe { crate::abi::exports_xml2::xmlNewText(content) };
    if !cur.is_null() {
        unsafe { (*cur).doc = doc as *mut _xmlDoc };
    }
    cur
}

/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNode *xmlNewDocTextLen(xmlDoc *doc, const xmlChar *content, int len);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlNewDocTextLen(
    doc: *mut _xmlDoc,
    content: *const xmlChar,
    len: c_int,
) -> *mut _xmlNode {
    let cur = unsafe { crate::abi::exports_tree::xmlNewTextLen(content, len) };
    if !cur.is_null() {
        unsafe { (*cur).doc = doc };
    }
    cur
}

/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNode *xmlNewDocComment(xmlDoc *doc, const xmlChar *content);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlNewDocComment(
    doc: *mut _xmlDoc,
    content: *const xmlChar,
) -> *mut _xmlNode {
    let cur = unsafe { crate::abi::exports_xml2::xmlNewComment(content) };
    if !cur.is_null() {
        unsafe { (*cur).doc = doc };
    }
    cur
}

/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNode *xmlNewDocPI(xmlDoc *doc, const xmlChar *name,
///                      const xmlChar *content);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlNewDocPI(
    doc: *mut _xmlDoc,
    name: *const xmlChar,
    content: *const xmlChar,
) -> *mut _xmlNode {
    let cur = unsafe { crate::abi::exports_xml2::xmlNewPI(name, content) };
    if !cur.is_null() {
        unsafe { (*cur).doc = doc };
    }
    cur
}

/// Split a QName, returning a pointer to the local part and storing the
/// prefix length (upstream tree.c `xmlSplitQName3`).
unsafe fn qname_split3(name: *const xmlChar, len: *mut c_int) -> *const xmlChar {
    if name.is_null() || len.is_null() {
        return ptr::null();
    }
    unsafe {
        if *name == b':' as xmlChar {
            return ptr::null();
        }
        let mut l = 0usize;
        while *name.add(l) != 0 && *name.add(l) != b':' as xmlChar {
            l += 1;
        }
        if *name.add(l) == 0 {
            return ptr::null();
        }
        *len = l as c_int;
        name.add(l + 1)
    }
}

/// # UPSTREAM-PARITY
///
/// ```c
/// xmlElementContent *xmlNewDocElementContent(xmlDoc *doc,
///                                            const xmlChar *name,
///                                            xmlElementContentType type);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlNewDocElementContent(
    doc: *mut _xmlDoc,
    name: *const xmlChar,
    type_: xmlElementContentType,
) -> *mut _xmlElementContent {
    let _ = doc;
    unsafe {
        let ret = xmlMallocZero(size_of::<_xmlElementContent>()) as *mut _xmlElementContent;
        if ret.is_null() {
            return ptr::null_mut();
        }
        (*ret).type_ = type_ as c_int;
        (*ret).ocur = XML_ELEMENT_CONTENT_ONCE as c_int;
        if !name.is_null() {
            let mut l: c_int = 0;
            let tmp = qname_split3(name, &mut l);
            if tmp.is_null() {
                (*ret).name = crate::abi::exports_xml2::xmlStrdup(name);
            } else {
                (*ret).prefix = crate::abi::exports_xml2::xmlStrndup(name, l);
                (*ret).name = crate::abi::exports_xml2::xmlStrdup(tmp);
                if (*ret).prefix.is_null() {
                    crate::xml::dtd::free_content_model(ret);
                    return ptr::null_mut();
                }
            }
            if (*ret).name.is_null() {
                crate::xml::dtd::free_content_model(ret);
                return ptr::null_mut();
            }
        }
        ret
    }
}

// ── node content / attribute value (tree.c) ───────────────────────────────────

/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlNodeBufGetContent(xmlBuffer *buffer, const xmlNode *cur);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlNodeBufGetContent(
    buffer: *mut _xmlBuffer,
    cur: *const _xmlNode,
) -> c_int {
    if cur.is_null() || buffer.is_null() {
        return -1;
    }
    let content = unsafe { crate::xml::tree::node_get_content(cur as *mut _xmlNode) };
    if content.is_null() {
        return 0;
    }
    let len = unsafe { crate::abi::exports_xml2::xmlStrlen(content) };
    let ret = crate::xml::io::buf_add(buffer, content, len);
    unsafe { xmlFree(content as *mut c_void) };
    if ret < 0 {
        -1
    } else {
        0
    }
}

/// Find an attribute node by name and namespace (upstream tree.c
/// `xmlGetPropNodeInternal` with useDTD == 0).
unsafe fn find_prop_ns(
    node: *const _xmlNode,
    name: *const xmlChar,
    ns_uri: *const xmlChar,
) -> *mut _xmlAttr {
    if node.is_null() || unsafe { (*node).type_ != XML_ELEMENT_NODE as c_int } || name.is_null() {
        return ptr::null_mut();
    }
    let mut prop = unsafe { (*node).properties };
    while !prop.is_null() {
        let ns = unsafe { (*prop).ns };
        let ns_match = if ns_uri.is_null() {
            ns.is_null()
        } else {
            !ns.is_null()
                && !unsafe { (*ns).href }.is_null()
                && unsafe { crate::abi::exports_xml2::xmlStrEqual((*ns).href, ns_uri) != 0 }
        };
        if ns_match && unsafe { crate::abi::exports_xml2::xmlStrEqual((*prop).name, name) != 0 } {
            return prop;
        }
        prop = unsafe { (*prop).next };
    }
    ptr::null_mut()
}

/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlNodeGetAttrValue(const xmlNode *node, const xmlChar *name,
///                         const xmlChar *nsUri, xmlChar **out);
/// ```
///
/// Returns 0 with `*out` set to the attribute value, 1 when the attribute
/// is missing (or arguments are invalid), -1 on allocation failure.
#[no_mangle]
pub unsafe extern "C" fn xmlNodeGetAttrValue(
    node: *const _xmlNode,
    name: *const xmlChar,
    ns_uri: *const xmlChar,
    out: *mut *mut xmlChar,
) -> c_int {
    if out.is_null() {
        return 1;
    }
    unsafe { *out = ptr::null_mut() };
    let prop = unsafe { find_prop_ns(node, name, ns_uri) };
    if prop.is_null() {
        return 1;
    }
    let value = unsafe { crate::xml::tree::node_get_content(prop as *mut _xmlNode) };
    if value.is_null() {
        return -1;
    }
    unsafe { *out = value };
    0
}

// ── element content serialization (valid.c) ───────────────────────────────────

/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlSprintfElementContent(char *buf, xmlElementContent *content,
///                               int englob);
/// ```
///
/// Deprecated; upstream (2.13+) ships this as an empty stub.
#[no_mangle]
pub unsafe extern "C" fn xmlSprintfElementContent(
    _buf: *mut c_char,
    _content: *mut _xmlElementContent,
    _englob: c_int,
) {
}

/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlSnprintfElementContent(char *buf, int size,
///                                xmlElementContent *content, int englob);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlSnprintfElementContent(
    buf: *mut c_char,
    size: c_int,
    content: *mut _xmlElementContent,
    englob: c_int,
) {
    if content.is_null() || buf.is_null() {
        return;
    }
    unsafe {
        let mut len = cstr_len(buf) as i32;
        if size - len < 50 {
            if len > 0 && size - len > 4 && *buf.add(len as usize - 1) != b'.' as c_char {
                cstr_cat_lit(buf, b" ...");
            }
            return;
        }
        if englob != 0 {
            cstr_cat_lit(buf, b"(");
        }
        match (*content).type_ as u32 {
            t if t == XML_ELEMENT_CONTENT_PCDATA as u32 => {
                cstr_cat_lit(buf, b"#PCDATA");
            }
            t if t == XML_ELEMENT_CONTENT_ELEMENT as u32 => {
                let mut qname_len = crate::abi::exports_xml2::xmlStrlen((*content).name);
                if !(*content).prefix.is_null() {
                    qname_len += crate::abi::exports_xml2::xmlStrlen((*content).prefix) + 1;
                }
                if size - len < qname_len + 10 {
                    cstr_cat_lit(buf, b" ...");
                    return;
                }
                if !(*content).prefix.is_null() {
                    cstr_cat_xmlstr(buf, (*content).prefix);
                    cstr_cat_lit(buf, b":");
                }
                if !(*content).name.is_null() {
                    cstr_cat_xmlstr(buf, (*content).name);
                }
            }
            t if t == XML_ELEMENT_CONTENT_SEQ as u32 => {
                if (*(*content).c1).type_ == XML_ELEMENT_CONTENT_OR as c_int
                    || (*(*content).c1).type_ == XML_ELEMENT_CONTENT_SEQ as c_int
                {
                    xmlSnprintfElementContent(buf, size, (*content).c1, 1);
                } else {
                    xmlSnprintfElementContent(buf, size, (*content).c1, 0);
                }
                len = cstr_len(buf) as i32;
                if size - len < 50 {
                    if len > 0 && size - len > 4 && *buf.add(len as usize - 1) != b'.' as c_char {
                        cstr_cat_lit(buf, b" ...");
                    }
                    return;
                }
                cstr_cat_lit(buf, b" , ");
                if ((*(*content).c2).type_ == XML_ELEMENT_CONTENT_OR as c_int
                    || (*(*content).c2).ocur != XML_ELEMENT_CONTENT_ONCE as c_int)
                    && (*(*content).c2).type_ != XML_ELEMENT_CONTENT_ELEMENT as c_int
                {
                    xmlSnprintfElementContent(buf, size, (*content).c2, 1);
                } else {
                    xmlSnprintfElementContent(buf, size, (*content).c2, 0);
                }
            }
            t if t == XML_ELEMENT_CONTENT_OR as u32 => {
                if (*(*content).c1).type_ == XML_ELEMENT_CONTENT_OR as c_int
                    || (*(*content).c1).type_ == XML_ELEMENT_CONTENT_SEQ as c_int
                {
                    xmlSnprintfElementContent(buf, size, (*content).c1, 1);
                } else {
                    xmlSnprintfElementContent(buf, size, (*content).c1, 0);
                }
                len = cstr_len(buf) as i32;
                if size - len < 50 {
                    if len > 0 && size - len > 4 && *buf.add(len as usize - 1) != b'.' as c_char {
                        cstr_cat_lit(buf, b" ...");
                    }
                    return;
                }
                cstr_cat_lit(buf, b" | ");
                if ((*(*content).c2).type_ == XML_ELEMENT_CONTENT_SEQ as c_int
                    || (*(*content).c2).ocur != XML_ELEMENT_CONTENT_ONCE as c_int)
                    && (*(*content).c2).type_ != XML_ELEMENT_CONTENT_ELEMENT as c_int
                {
                    xmlSnprintfElementContent(buf, size, (*content).c2, 1);
                } else {
                    xmlSnprintfElementContent(buf, size, (*content).c2, 0);
                }
            }
            _ => {}
        }
        if size - cstr_len(buf) as i32 <= 2 {
            return;
        }
        if englob != 0 {
            cstr_cat_lit(buf, b")");
        }
        match (*content).ocur as u32 {
            t if t == XML_ELEMENT_CONTENT_OPT as u32 => cstr_cat_lit(buf, b"?"),
            t if t == XML_ELEMENT_CONTENT_MULT as u32 => cstr_cat_lit(buf, b"*"),
            t if t == XML_ELEMENT_CONTENT_PLUS as u32 => cstr_cat_lit(buf, b"+"),
            _ => {}
        }
    }
}

// ── deprecated stubs ───────────────────────────────────────────────────────────

/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlUpgradeOldNs(xmlDocPtr doc);
/// ```
///
/// Deprecated (legacy.c): modern documents no longer carry old-style
/// namespace declarations, so this is a no-op returning 0.
#[no_mangle]
pub unsafe extern "C" fn xmlUpgradeOldNs(_doc: *mut _xmlDoc) -> c_int {
    0
}
