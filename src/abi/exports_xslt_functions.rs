//! C ABI exports for libxslt.so.1 — the "functions" (XPath extension
//! functions) family (§16, Phase 8).
//!
//! Implements the libxslt 1.1.45 XPath extension-function surface:
//!
//! - XSLT XPath functions: `xsltDocumentFunction`, `xsltKeyFunction`,
//!   `xsltUnparsedEntityURIFunction`, `xsltFormatNumberFunction`,
//!   `xsltGenerateIdFunction`, `xsltSystemPropertyFunction`,
//!   `xsltElementAvailableFunction`, `xsltFunctionAvailableFunction`
//! - EXSLT helpers: `xsltFunctionNodeSet` (node-set())
//! - XPath evaluation utilities: `xsltEvalXPathPredicate`,
//!   `xsltEvalXPathString`, `xsltEvalXPathStringNs`, `xsltXPathCompile`,
//!   `xsltXPathCompileFlags`
//! - XPath-context hooks: `xsltXPathFunctionLookup`, `xsltXPathVariableLookup`,
//!   `xsltXPathGetTransformContext`
//! - Registration: `xsltRegisterAllFunctions`
//!
//! All semantics follow upstream libxslt 1.1.45 (`archaeology/libxslt-git`):
//! `functions.c`, `extra.c`, `templates.c`, `xsltutils.c`, `variables.c`,
//! `xslt.c`, `numbers.c` and `extensions.c`. Where the native-Rust engine in
//! `src/xslt/*` already implements the upstream behaviour, the exports below
//! are wired to it; everything else is a faithful port.
//!
//! # Candidate wiring notes
//!
//! Upstream stores the transform context in the XPath context's `extra`
//! slot (`ctxt->context->extra`, set by `XSLT_REGISTER_VARIABLE_LOOKUP` in
//! transform.c). The candidate uses `extra` for the internal Rust
//! `XPathContext` and stashes the transform context in the internal
//! context's `func_lookup_data` (`xsltNewTransformContext` in
//! `src/xslt/transform/mod.rs`). `xsltXPathGetTransformContext` therefore
//! resolves the transform context through that slot.

#![allow(non_snake_case)]
#![allow(unused_variables)]
#![allow(clippy::missing_safety_doc)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]
#![allow(clippy::comparison_chain)]

use core::ffi::c_void;
use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_ulong, c_ushort};
use std::ptr;

use crate::abi::allocator::{xmlFreeImpl, xmlMallocImpl};
use crate::abi::structs::*;
use crate::abi::types::xmlElementType::*;
use crate::abi::types::*;
use crate::xml::xpath::context::XPathContext;
use crate::xml::xpath::exports::xmlXPathNewString;
use crate::xml::xpath::parser_context::{value_pop, value_push, XmlXPathParserContext};

// ═══════════════════════════════════════════════════════════════════════════════
// Constants (upstream xslt.h / xsltutils.h / transform.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// `XSLT_NAMESPACE` — the XSLT namespace URI.
const XSLT_NAMESPACE: &[u8] = b"http://www.w3.org/1999/XSL/Transform\0";

/// `XSLT_DEFAULT_VERSION` (xslt.h).
const XSLT_DEFAULT_VERSION: &[u8] = b"1.0\0";

/// `XSLT_DEFAULT_VENDOR` (xslt.h).
const XSLT_DEFAULT_VENDOR: &[u8] = b"libxslt\0";

/// `XSLT_DEFAULT_URL` (xslt.h).
const XSLT_DEFAULT_URL: &[u8] = b"http://xmlsoft.org/XSLT/\0";

/// `XSLT_STATE_STOPPED` (transform.c): the transform was stopped.
const XSLT_STATE_STOPPED: c_int = 2;

/// `XSLT_SOURCE_NODE_MASK` (xsltutils.h).
const XSLT_SOURCE_NODE_MASK: c_int = 15;

/// `XSLT_SOURCE_NODE_HAS_ID` (xsltutils.h).
const XSLT_SOURCE_NODE_HAS_ID: c_int = 2;

/// `SYMBOL_QUOTE` (numbers.c).
const SYMBOL_QUOTE: xmlChar = b'\'';

/// `DBL_MAX_10_EXP` (numbers.c): 308 on IEEE platforms.
const DBL_MAX_10_EXP: c_int = 308;

// ═══════════════════════════════════════════════════════════════════════════════
// Small helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// The internal Rust XPath context stored in `_xmlXPathContext.extra`.
unsafe fn internal_xpath_ctxt(xpath_ctxt: *mut _xmlXPathContext) -> *mut XPathContext {
    if xpath_ctxt.is_null() {
        return ptr::null_mut();
    }
    unsafe { (*xpath_ctxt).extra as *mut XPathContext }
}

/// Resolve the transform context from an XPath parser context.
///
/// UPSTREAM-PARITY: upstream returns `ctxt->context->extra` (extensions.c
/// `xsltXPathGetTransformContext`); the candidate keeps the internal Rust
/// `XPathContext` in `extra` and the transform context in
/// `internal->func_lookup_data` (see module docs).
unsafe fn transform_context_from_parser(ctxt: *mut c_void) -> *mut _xsltTransformContext {
    if ctxt.is_null() {
        return ptr::null_mut();
    }
    let pc = ctxt as *mut XmlXPathParserContext;
    let xpath_ctxt = unsafe { (*pc).context };
    if xpath_ctxt.is_null() {
        return ptr::null_mut();
    }
    let internal = unsafe { internal_xpath_ctxt(xpath_ctxt) };
    if internal.is_null() {
        return ptr::null_mut();
    }
    unsafe { (*internal).func_lookup_data as *mut _xsltTransformContext }
}

/// Compare two NUL-terminated xmlChar strings for equality.
unsafe fn xml_chars_equal(a: *const xmlChar, b: *const xmlChar) -> bool {
    if a.is_null() || b.is_null() {
        return false;
    }
    libc::strcmp(a as *const libc::c_char, b as *const libc::c_char) == 0
}

/// `IS_XSLT_REAL_NODE` (xsltutils.h): document, element, text, cdata,
/// attribute, comment or PI node.
unsafe fn is_real_node(node: *mut _xmlNode) -> bool {
    if node.is_null() {
        return false;
    }
    let t = unsafe { (*node).type_ };
    t == XML_ELEMENT_NODE as c_int
        || t == XML_TEXT_NODE as c_int
        || t == XML_CDATA_SECTION_NODE as c_int
        || t == XML_ATTRIBUTE_NODE as c_int
        || t == XML_DOCUMENT_NODE as c_int
        || t == XML_HTML_DOCUMENT_NODE as c_int
        || t == XML_COMMENT_NODE as c_int
        || t == XML_PI_NODE as c_int
}

/// Split a QName into (prefix, local) — pointers into `name` (upstream
/// `xsltSplitQName`, xsltutils.c; the dict-interning is not observable for
/// the read-only uses here).
///
/// Returns `(prefix, local)`: `prefix` is NULL when `name` has no prefix
/// (or starts with ':'), otherwise it points at the prefix inside `name`;
/// `local` points at the local part.
const unsafe fn split_qname_ref(name: *const xmlChar) -> (*const xmlChar, *const xmlChar) {
    if name.is_null() {
        return (ptr::null(), ptr::null());
    }
    unsafe {
        if *name == b':' {
            return (ptr::null(), name);
        }
        let mut len: usize = 0;
        while *name.add(len) != 0 && *name.add(len) != b':' {
            len += 1;
        }
        if *name.add(len) == 0 {
            return (ptr::null(), name);
        }
        (name, name.add(len + 1))
    }
}

/// `xsltGetPSVIPtr` (xsltutils.c): pointer to the psvi member of a node.
unsafe fn get_psvi_ptr(cur: *mut _xmlNode) -> *mut *mut c_void {
    if cur.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        match (*cur).type_ {
            t if t == XML_DOCUMENT_NODE as c_int || t == XML_HTML_DOCUMENT_NODE as c_int => {
                &mut (*(cur as *mut _xmlDoc)).psvi
            }
            t if t == XML_ATTRIBUTE_NODE as c_int => &mut (*(cur as *mut _xmlAttr)).psvi,
            t if t == XML_ELEMENT_NODE as c_int
                || t == XML_TEXT_NODE as c_int
                || t == XML_CDATA_SECTION_NODE as c_int
                || t == XML_PI_NODE as c_int
                || t == XML_COMMENT_NODE as c_int =>
            {
                &mut (*cur).psvi
            }
            _ => ptr::null_mut(),
        }
    }
}

/// `xsltGetSourceNodeFlags` (xsltutils.c).
unsafe fn get_source_node_flags(node: *mut _xmlNode) -> c_int {
    if node.is_null() {
        return 0;
    }
    unsafe {
        match (*node).type_ {
            t if t == XML_DOCUMENT_NODE as c_int || t == XML_HTML_DOCUMENT_NODE as c_int => {
                (*(node as *mut _xmlDoc)).properties >> 27
            }
            t if t == XML_ATTRIBUTE_NODE as c_int => (*(node as *mut _xmlAttr)).atype >> 27,
            t if t == XML_ELEMENT_NODE as c_int
                || t == XML_TEXT_NODE as c_int
                || t == XML_CDATA_SECTION_NODE as c_int
                || t == XML_PI_NODE as c_int
                || t == XML_COMMENT_NODE as c_int =>
            {
                ((*node).extra as c_int) >> 12
            }
            _ => 0,
        }
    }
}

/// `xsltSetSourceNodeFlags` (xsltutils.c).
unsafe fn set_source_node_flags(
    ctxt: *mut _xsltTransformContext,
    node: *mut _xmlNode,
    flags: c_int,
) -> c_int {
    unsafe {
        if !ctxt.is_null()
            && !(*ctxt).initialContextDoc.is_null()
            && !node.is_null()
            && (*node).doc == (*ctxt).initialContextDoc
        {
            (*ctxt).sourceDocDirty = 1;
        }
        match (*node).type_ {
            t if t == XML_DOCUMENT_NODE as c_int || t == XML_HTML_DOCUMENT_NODE as c_int => {
                (*(node as *mut _xmlDoc)).properties |= flags << 27;
                0
            }
            t if t == XML_ATTRIBUTE_NODE as c_int => {
                (*(node as *mut _xmlAttr)).atype |= flags << 27;
                0
            }
            t if t == XML_ELEMENT_NODE as c_int
                || t == XML_TEXT_NODE as c_int
                || t == XML_CDATA_SECTION_NODE as c_int
                || t == XML_PI_NODE as c_int
                || t == XML_COMMENT_NODE as c_int =>
            {
                (*node).extra = ((*node).extra as c_int | (flags << 12)) as c_ushort;
                0
            }
            _ => -1,
        }
    }
}

/// `xsltClearSourceNodeFlags` (xsltutils.c).
unsafe fn clear_source_node_flags(node: *mut _xmlNode, flags: c_int) -> c_int {
    unsafe {
        match (*node).type_ {
            t if t == XML_DOCUMENT_NODE as c_int || t == XML_HTML_DOCUMENT_NODE as c_int => {
                (*(node as *mut _xmlDoc)).properties &= !(flags << 27);
                0
            }
            t if t == XML_ATTRIBUTE_NODE as c_int => {
                (*(node as *mut _xmlAttr)).atype &= !(flags << 27);
                0
            }
            t if t == XML_ELEMENT_NODE as c_int
                || t == XML_TEXT_NODE as c_int
                || t == XML_CDATA_SECTION_NODE as c_int
                || t == XML_PI_NODE as c_int
                || t == XML_COMMENT_NODE as c_int =>
            {
                (*node).extra = ((*node).extra as c_int & !(flags << 12)) as c_ushort;
                0
            }
            _ => -1,
        }
    }
}

/// `xsltCleanupSourceDoc` (transform.c): remove psvi fields and source-node
/// flags from a document tree (used on stylesheet-doc copies).
unsafe fn cleanup_source_doc(doc: *mut _xmlDoc) {
    if doc.is_null() {
        return;
    }
    unsafe {
        let mut cur = doc as *mut _xmlNode;
        loop {
            clear_source_node_flags(cur, XSLT_SOURCE_NODE_MASK);
            let psvi_ptr = get_psvi_ptr(cur);
            if !psvi_ptr.is_null() {
                *psvi_ptr = ptr::null_mut();
            }
            if (*cur).type_ == XML_ELEMENT_NODE as c_int {
                let mut prop = (*cur).properties;
                while !prop.is_null() {
                    (*prop).atype &= !(XSLT_SOURCE_NODE_MASK << 27);
                    (*prop).psvi = ptr::null_mut();
                    prop = (*prop).next;
                }
            }
            if !(*cur).children.is_null() && (*cur).type_ != XML_ENTITY_REF_NODE as c_int {
                cur = (*cur).children;
            } else {
                if cur == doc as *mut _xmlNode {
                    return;
                }
                while (*cur).next.is_null() {
                    cur = (*cur).parent;
                    if cur == doc as *mut _xmlNode {
                        return;
                    }
                }
                cur = (*cur).next;
            }
        }
    }
}

/// `xsltNextImport` (imports.c).
#[allow(dead_code)]
unsafe fn next_import(cur: *mut _xsltStylesheet) -> *mut _xsltStylesheet {
    if cur.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        if !(*cur).imports.is_null() {
            return (*cur).imports;
        }
        if !(*cur).next.is_null() {
            return (*cur).next;
        }
        let mut c = (*cur).parent;
        while !c.is_null() {
            if !(*c).next.is_null() {
                return (*c).next;
            }
            c = (*c).parent;
        }
    }
    ptr::null_mut()
}

/// Report an XSLT error with a static format string (variadic arguments are
/// not expanded; the raw format string is recorded, matching the candidate's
/// `xsltTransformError` safe subset).
unsafe fn xslt_error(ctxt: *mut _xsltTransformContext, msg: &'static [u8]) {
    crate::xslt::errors::xsltTransformError(
        ctxt,
        ptr::null_mut(),
        ptr::null_mut(),
        msg.as_ptr() as *const c_char,
    );
}

/// Emit a transform error through the XSLT error machinery with a
/// pre-formatted message (the upstream messages are variadic; the candidate
/// formats before calling).
///
/// # SAFETY
///
/// - `ctxt` may be NULL; `msg` must not contain interior NUL bytes.
unsafe fn emit_fn_error(ctxt: *mut _xsltTransformContext, msg: &[u8]) {
    let mut buf = msg.to_vec();
    buf.push(0);
    crate::xslt::errors::xsltTransformError(
        ctxt,
        ptr::null_mut(),
        ptr::null_mut(),
        buf.as_ptr() as *const c_char,
    );
}

/// Render a NUL-terminated string as an owned Rust String.
///
/// # SAFETY
///
/// - `p` must be NULL or a valid NUL-terminated string.
unsafe fn cstr_to_string(p: *const xmlChar) -> String {
    if p.is_null() {
        return String::new();
    }
    unsafe {
        std::ffi::CStr::from_ptr(p as *const c_char)
            .to_string_lossy()
            .into_owned()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 1. XSLT XPath functions (functions.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// Upstream `xsltDocumentFunctionLoadDocument` (functions.c): load a
/// document by URI and push its root node-set; honour a `#fragment`
/// XPointer selection.
///
/// # SAFETY
///
/// - `ctxt` must be a valid parser context; `URI`/`fragment` valid strings
///   or NULL.
unsafe fn xslt_document_function_load_document(
    ctxt: *mut c_void,
    uri: *const xmlChar,
    fragment: *const xmlChar,
) {
    let pc = ctxt as *mut XmlXPathParserContext;
    let tctxt = transform_context_from_parser(ctxt);
    if tctxt.is_null() {
        xslt_error(
            ptr::null_mut(),
            b"document() : internal error tctxt == NULL\n\0",
        );
        value_push(
            pc,
            crate::abi::exports_xml2::xmlXPathNewNodeSet(ptr::null_mut()),
        );
        return;
    }

    let mut doc = crate::xslt::documents::xsltLoadDocument(tctxt, uri);
    if doc.is_null() {
        // This selects the stylesheet's doc itself.
        let style = (*tctxt).style;
        let style_doc = if style.is_null() {
            ptr::null_mut()
        } else {
            (*style).doc
        };
        let select_style = uri.is_null()
            || *uri == b'#'
            || (!style_doc.is_null() && {
                let url = (*style_doc).URL;
                !url.is_null()
                    && crate::abi::exports_xml2::xmlStrEqual(url as *const xmlChar, uri) != 0
            });
        if select_style && !style_doc.is_null() {
            let copy = crate::abi::exports_xml2::xmlCopyDoc(style_doc as *const _xmlDoc, 1);
            if copy.is_null() {
                xslt_error(tctxt, b"document() : failed to copy style doc\n\0");
                value_push(
                    pc,
                    crate::abi::exports_xml2::xmlXPathNewNodeSet(ptr::null_mut()),
                );
                return;
            }
            cleanup_source_doc(copy);
            doc = copy;
        } else {
            value_push(
                pc,
                crate::abi::exports_xml2::xmlXPathNewNodeSet(ptr::null_mut()),
            );
            return;
        }
    }

    if fragment.is_null() {
        value_push(
            pc,
            crate::abi::exports_xml2::xmlXPathNewNodeSet(doc as *mut _xmlNode),
        );
        return;
    }

    // Use XPointer / HTML location for the fragment ID.
    // UPSTREAM-PARITY: upstream evaluates with xmlXPtrEval() on a fresh
    // context of `doc`; the candidate's XPointer bridge
    // (xmlXPtrEvalNodeSet) evaluates the fragment against the document and
    // returns a node-set.
    let res_ns = crate::xml::xpointer::xmlXPtrEvalNodeSet(fragment as *const c_char, doc);
    if res_ns.is_null() {
        value_push(
            pc,
            crate::abi::exports_xml2::xmlXPathNewNodeSet(ptr::null_mut()),
        );
    } else {
        value_push(pc, crate::xml::xpath::exports::xmlXPathWrapNodeSet(res_ns));
    }
}

/// Implement the XSLT `document()` function:
/// `node-set document(object, node-set?)`.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltDocumentFunction(xmlXPathParserContextPtr ctxt, int nargs);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be a valid parser context.
#[no_mangle]
pub unsafe extern "C" fn xsltDocumentFunction(ctxt: *mut c_void, nargs: c_int) {
    let pc = ctxt as *mut XmlXPathParserContext;
    if pc.is_null() {
        return;
    }
    let mut obj: *mut _xmlXPathObject = ptr::null_mut();
    let mut obj2: *mut _xmlXPathObject = ptr::null_mut();
    let mut new_uri: *mut xmlChar = ptr::null_mut();
    let mut fragment: *mut xmlChar = ptr::null_mut();

    if !(1..=2).contains(&nargs) {
        xslt_error(
            transform_context_from_parser(ctxt),
            b"document() : invalid number of args %d\n\0",
        );
        (*pc).error = XPATH_INVALID_ARITY as c_int;
        return;
    }
    if (*pc).value.is_null() {
        xslt_error(
            transform_context_from_parser(ctxt),
            b"document() : invalid arg value\n\0",
        );
        (*pc).error = XPATH_INVALID_TYPE as c_int;
        return;
    }

    if nargs == 2 {
        if (*(*pc).value).type_ != xmlXPathObjectType::XPATH_NODESET as c_int {
            xslt_error(
                transform_context_from_parser(ctxt),
                b"document() : invalid arg expecting a nodeset\n\0",
            );
            (*pc).error = XPATH_INVALID_TYPE as c_int;
            return;
        }
        obj2 = value_pop(pc);
    }

    if !(*pc).value.is_null() && (*(*pc).value).type_ == xmlXPathObjectType::XPATH_NODESET as c_int
    {
        // First argument is a node-set: apply document() to each node and
        // merge the results.
        let mut i: c_int = 0;
        let mut newobj: *mut _xmlXPathObject = ptr::null_mut();
        obj = value_pop(pc);
        let ret = crate::abi::exports_xml2::xmlXPathNewNodeSet(ptr::null_mut());
        if !obj.is_null() && !(*obj).nodesetval.is_null() && !ret.is_null() {
            let ns = (*obj).nodesetval as *mut _xmlNodeSet;
            while i < (*ns).nodeNr {
                let node = *(*ns).nodeTab.offset(i as isize);
                value_push(pc, crate::abi::exports_xml2::xmlXPathNewNodeSet(node));
                crate::xml::xpath::exports::xmlXPathStringFunction(ctxt, 1);
                if nargs == 2 {
                    value_push(pc, crate::abi::exports_xml2::xmlXPathObjectCopy(obj2));
                } else {
                    value_push(pc, crate::abi::exports_xml2::xmlXPathNewNodeSet(node));
                }
                if (*pc).error != 0 {
                    break;
                }
                xsltDocumentFunction(ctxt, 2);
                newobj = value_pop(pc);
                if !newobj.is_null() {
                    let merged = crate::xml::xpath::exports::xmlXPathNodeSetMerge(
                        (*ret).nodesetval as *mut _xmlNodeSet,
                        (*newobj).nodesetval as *mut _xmlNodeSet,
                    );
                    (*ret).nodesetval = merged as *mut c_void;
                    crate::abi::exports_xml2::xmlXPathFreeObject(newobj);
                }
                i += 1;
            }
        }
        if !obj.is_null() {
            crate::abi::exports_xml2::xmlXPathFreeObject(obj);
        }
        if !obj2.is_null() {
            crate::abi::exports_xml2::xmlXPathFreeObject(obj2);
        }
        value_push(pc, ret);
        return;
    }

    // Make sure it's converted to a string.
    crate::xml::xpath::exports::xmlXPathStringFunction(ctxt, 1);
    if (*pc).value.is_null() || (*(*pc).value).type_ != xmlXPathObjectType::XPATH_STRING as c_int {
        xslt_error(
            transform_context_from_parser(ctxt),
            b"document() : invalid arg expecting a string\n\0",
        );
        (*pc).error = XPATH_INVALID_TYPE as c_int;
        if !obj2.is_null() {
            crate::abi::exports_xml2::xmlXPathFreeObject(obj2);
        }
        return;
    }
    obj = value_pop(pc);

    if (*obj).stringval.is_null() {
        value_push(
            pc,
            crate::abi::exports_xml2::xmlXPathNewNodeSet(ptr::null_mut()),
        );
    } else {
        let tctxt = transform_context_from_parser(ctxt);
        let mut url = (*obj).stringval;

        let uri = crate::abi::exports_xml2::xmlParseURI(url as *const c_char);
        if uri.is_null() {
            xslt_error(tctxt, b"document() : failed to parse URI '%s'\n\0");
            value_push(
                pc,
                crate::abi::exports_xml2::xmlXPathNewNodeSet(ptr::null_mut()),
            );
            // goto error
        } else {
            // Check for and remove the fragment identifier. The URI object
            // mirrors upstream `_xmlURI`; the fragment string is detached so
            // xmlFreeURI does not free it (the caller frees it below).
            #[repr(C)]
            struct XmlUriC {
                scheme: *mut c_char,
                opaque: *mut c_char,
                authority: *mut c_char,
                server: *mut c_char,
                user: *mut c_char,
                port: c_int,
                path: *mut c_char,
                query: *mut c_char,
                fragment: *mut c_char,
                cleanup: c_int,
                query_raw: *mut c_char,
            }
            let uri_c = uri as *mut XmlUriC;
            fragment = (*uri_c).fragment as *mut xmlChar;
            if !fragment.is_null() {
                (*uri_c).fragment = ptr::null_mut();
                new_uri = crate::abi::exports_xml2::xmlSaveUri(uri);
                url = new_uri;
            }
            crate::abi::exports_xml2::xmlFreeURI(uri);

            // Compute the base URI from the optional second argument or the
            // current instruction.
            let mut base: *mut xmlChar = ptr::null_mut();
            if !obj2.is_null()
                && !(*obj2).nodesetval.is_null()
                && {
                    let ns2 = (*obj2).nodesetval as *mut _xmlNodeSet;
                    (*ns2).nodeNr > 0
                }
                && is_real_node(*(*((*obj2).nodesetval as *mut _xmlNodeSet)).nodeTab)
            {
                let mut target = *(*((*obj2).nodesetval as *mut _xmlNodeSet)).nodeTab;
                if (*target).type_ == XML_ATTRIBUTE_NODE as c_int
                    || (*target).type_ == XML_PI_NODE as c_int
                {
                    target = (*target).parent;
                }
                base = crate::abi::exports_tree::xmlNodeGetBase((*target).doc, target);
            } else if !tctxt.is_null() && !(*tctxt).inst.is_null() {
                base =
                    crate::abi::exports_tree::xmlNodeGetBase((*(*tctxt).inst).doc, (*tctxt).inst);
            } else if !tctxt.is_null()
                && !(*tctxt).style.is_null()
                && !(*tctxt).style.is_null()
                && !(*(*tctxt).style).doc.is_null()
            {
                base = crate::abi::exports_tree::xmlNodeGetBase(
                    (*(*tctxt).style).doc,
                    (*(*tctxt).style).doc as *mut _xmlNode,
                );
            }

            let resolved =
                crate::abi::exports_uri::xmlBuildURI(url as *const c_char, base as *const c_char);
            if !base.is_null() {
                xmlFreeImpl(base as *mut c_void);
            }
            if resolved.is_null() {
                if !tctxt.is_null()
                    && !(*tctxt).style.is_null()
                    && !(*(*tctxt).style).doc.is_null()
                    && !(*(*(*tctxt).style).doc).URL.is_null()
                    && crate::abi::exports_xml2::xmlStrEqual(
                        resolved as *const xmlChar,
                        (*(*(*tctxt).style).doc).URL as *const xmlChar,
                    ) != 0
                {
                    // This selects the stylesheet's doc itself.
                    value_push(
                        pc,
                        crate::abi::exports_xml2::xmlXPathNewNodeSet(
                            (*(*tctxt).style).doc as *mut _xmlNode,
                        ),
                    );
                } else {
                    value_push(
                        pc,
                        crate::abi::exports_xml2::xmlXPathNewNodeSet(ptr::null_mut()),
                    );
                }
            } else {
                xslt_document_function_load_document(ctxt, resolved, fragment);
                xmlFreeImpl(resolved as *mut c_void);
            }
        }
    }

    // error: label cleanup
    if !new_uri.is_null() {
        xmlFreeImpl(new_uri as *mut c_void);
    }
    if !fragment.is_null() {
        xmlFreeImpl(fragment as *mut c_void);
    }
    if !obj.is_null() {
        crate::abi::exports_xml2::xmlXPathFreeObject(obj);
    }
    if !obj2.is_null() {
        crate::abi::exports_xml2::xmlXPathFreeObject(obj2);
    }
}

/// Implement the XSLT `key()` function: `node-set key(string, object)`.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltKeyFunction(xmlXPathParserContextPtr ctxt, int nargs);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be a valid parser context.
#[no_mangle]
pub unsafe extern "C" fn xsltKeyFunction(ctxt: *mut c_void, nargs: c_int) {
    let pc = ctxt as *mut XmlXPathParserContext;
    if pc.is_null() {
        return;
    }
    let mut obj1: *mut _xmlXPathObject = ptr::null_mut();
    let mut obj2: *mut _xmlXPathObject = ptr::null_mut();

    if nargs != 2 {
        xslt_error(
            transform_context_from_parser(ctxt),
            b"key() : expects two arguments\n\0",
        );
        (*pc).error = XPATH_INVALID_ARITY as c_int;
        return;
    }

    // Get the key's value.
    obj2 = value_pop(pc);
    crate::xml::xpath::exports::xmlXPathStringFunction(ctxt, 1);
    if obj2.is_null()
        || (*pc).value.is_null()
        || (*(*pc).value).type_ != xmlXPathObjectType::XPATH_STRING as c_int
    {
        xslt_error(
            transform_context_from_parser(ctxt),
            b"key() : invalid arg expecting a string\n\0",
        );
        (*pc).error = XPATH_INVALID_TYPE as c_int;
        crate::abi::exports_xml2::xmlXPathFreeObject(obj2);
        return;
    }
    // Get the key's name.
    obj1 = value_pop(pc);

    if (*obj2).type_ == xmlXPathObjectType::XPATH_NODESET as c_int
        || (*obj2).type_ == xmlXPathObjectType::XPATH_XSLT_TREE as c_int
    {
        let mut i: c_int = 0;
        let mut newobj: *mut _xmlXPathObject = ptr::null_mut();
        let ret = crate::abi::exports_xml2::xmlXPathNewNodeSet(ptr::null_mut());
        if ret.is_null() {
            (*pc).error = XPATH_MEMORY_ERROR as c_int;
            crate::abi::exports_xml2::xmlXPathFreeObject(obj1);
            crate::abi::exports_xml2::xmlXPathFreeObject(obj2);
            return;
        }
        if !(*obj2).nodesetval.is_null() {
            let ns = (*obj2).nodesetval as *mut _xmlNodeSet;
            while i < (*ns).nodeNr {
                value_push(pc, crate::abi::exports_xml2::xmlXPathObjectCopy(obj1));
                value_push(
                    pc,
                    crate::abi::exports_xml2::xmlXPathNewNodeSet(*(*ns).nodeTab.offset(i as isize)),
                );
                crate::xml::xpath::exports::xmlXPathStringFunction(ctxt, 1);
                xsltKeyFunction(ctxt, 2);
                newobj = value_pop(pc);
                if !newobj.is_null() {
                    let merged = crate::xml::xpath::exports::xmlXPathNodeSetMerge(
                        (*ret).nodesetval as *mut _xmlNodeSet,
                        (*newobj).nodesetval as *mut _xmlNodeSet,
                    );
                    (*ret).nodesetval = merged as *mut c_void;
                }
                crate::abi::exports_xml2::xmlXPathFreeObject(newobj);
                i += 1;
            }
        }
        value_push(pc, ret);
    } else {
        let mut nodelist: *mut _xmlNodeSet = ptr::null_mut();
        let mut key: *mut xmlChar = ptr::null_mut();
        let mut key_uri: *const xmlChar = ptr::null();
        let tctxt = transform_context_from_parser(ctxt);
        let xpctxt = (*pc).context;
        let mut tmp_node: *mut _xmlNode = ptr::null_mut();

        if tctxt.is_null() || xpctxt.is_null() {
            (*pc).error = XPATH_INVALID_TYPE as c_int;
            // mirror upstream error: push an empty node-set.
            value_push(
                pc,
                crate::abi::exports_xml2::xmlXPathNewNodeSet(ptr::null_mut()),
            );
        } else {
            let old_doc_info = (*tctxt).document;

            if (*xpctxt).node.is_null() {
                xslt_error(
                    tctxt,
                    b"Internal error in xsltKeyFunction(): The context node is not set on the XPath context.\n\0",
                );
                (*tctxt).state = XSLT_STATE_STOPPED;
                value_push(
                    pc,
                    crate::abi::exports_xml2::xmlXPathNewNodeSet(ptr::null_mut()),
                );
            } else {
                // Get the associated namespace URI if qualified name.
                let qname = (*obj1).stringval;
                let mut prefix: *mut xmlChar = ptr::null_mut();
                key = crate::xml::string::split_qname2(qname, &mut prefix);
                if key.is_null() {
                    key = crate::abi::exports_xml2::xmlStrdup(qname);
                    key_uri = ptr::null();
                    if !prefix.is_null() {
                        xmlFreeImpl(prefix as *mut c_void);
                    }
                } else if !prefix.is_null() {
                    key_uri = crate::xml::xpath::exports::xmlXPathNsLookup(xpctxt, prefix);
                    if key_uri.is_null() {
                        xslt_error(tctxt, b"key() : prefix %s is not bound\n\0");
                    }
                    xmlFreeImpl(prefix as *mut c_void);
                } else {
                    key_uri = ptr::null();
                }

                // Force conversion of the value to string.
                value_push(pc, obj2);
                crate::xml::xpath::exports::xmlXPathStringFunction(ctxt, 1);
                obj2 = value_pop(pc);
                if obj2.is_null() || (*obj2).type_ != xmlXPathObjectType::XPATH_STRING as c_int {
                    xslt_error(tctxt, b"key() : invalid arg expecting a string\n\0");
                    (*pc).error = XPATH_INVALID_TYPE as c_int;
                } else {
                    let value = (*obj2).stringval;

                    // Determine the context node's owner doc.
                    if (*xpctxt).node.is_null() {
                        tmp_node = ptr::null_mut();
                    } else if (*xpctxt).node.is_null()
                        || (*(*xpctxt).node).type_ != XML_NAMESPACE_DECL as c_int
                    {
                        tmp_node = (*xpctxt).node;
                    } else {
                        // Namespace node: the owner element is stored in
                        // ns->next (libxml2 XPath hack).
                        let ns = (*xpctxt).node as *mut _xmlNs;
                        if !(*ns).next.is_null() && (*(*ns).next).type_ == XML_ELEMENT_NODE as c_int
                        {
                            tmp_node = (*ns).next as *mut _xmlNode;
                        }
                    }

                    if tmp_node.is_null() || (*tmp_node).doc.is_null() {
                        xslt_error(
                            tctxt,
                            b"Internal error in xsltKeyFunction(): Couldn't get the doc of the XPath context node.\n\0",
                        );
                    } else if (*tctxt).document.is_null()
                        || (*(*tctxt).document).doc != (*tmp_node).doc
                    {
                        // UPSTREAM-PARITY (simplified): upstream switches
                        // tctxt->document to the context node's document
                        // wrapper (xsltNewDocument / xsltFindDocument).
                        // The candidate indexes key tables on the main
                        // document wrapper only (see xsltInitKeys /
                        // xsltEvalKeyFunction in src/xslt/keys), so no
                        // switch is performed here; the lookup below uses
                        // ctxt->document as-is.
                        if (*tctxt).document.is_null() {
                            (*tctxt).state = XSLT_STATE_STOPPED;
                        }
                    }

                    if (*tctxt).document.is_null() {
                        xslt_error(
                            tctxt,
                            b"Internal error in xsltKeyFunction(): Could not get the document info of a context doc.\n\0",
                        );
                        (*tctxt).state = XSLT_STATE_STOPPED;
                    } else {
                        // Get/compute the key value. UPSTREAM-PARITY: the
                        // candidate's key-table lookup (xsltEvalKeyFunction)
                        // matches tables by local name only; the namespace
                        // URI resolved above is not part of the lookup
                        // (same behaviour as the internal engine's key()).
                        nodelist = crate::xslt::keys::xsltEvalKeyFunction(tctxt, key, value);
                    }
                }

                (*tctxt).document = old_doc_info;
            }

            // error: push the (possibly empty) merged node-set.
            if nodelist.is_null() {
                value_push(
                    pc,
                    crate::abi::exports_xml2::xmlXPathNewNodeSet(ptr::null_mut()),
                );
            } else {
                let merged =
                    crate::xml::xpath::exports::xmlXPathNodeSetMerge(ptr::null_mut(), nodelist);
                crate::abi::exports_xml2::xmlXPathFreeNodeSet(nodelist);
                value_push(pc, crate::xml::xpath::exports::xmlXPathWrapNodeSet(merged));
            }
            if !key.is_null() {
                xmlFreeImpl(key as *mut c_void);
            }
        }
    }

    if !obj1.is_null() {
        crate::abi::exports_xml2::xmlXPathFreeObject(obj1);
    }
    if !obj2.is_null() {
        crate::abi::exports_xml2::xmlXPathFreeObject(obj2);
    }
}

/// Implement the XSLT `unparsed-entity-uri()` function:
/// `string unparsed-entity-uri(string)`.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltUnparsedEntityURIFunction(xmlXPathParserContextPtr ctxt, int nargs);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be a valid parser context.
#[no_mangle]
pub unsafe extern "C" fn xsltUnparsedEntityURIFunction(ctxt: *mut c_void, nargs: c_int) {
    let pc = ctxt as *mut XmlXPathParserContext;
    if pc.is_null() {
        return;
    }
    let mut obj: *mut _xmlXPathObject = ptr::null_mut();

    if (nargs != 1) || (*pc).value.is_null() {
        crate::xslt::errors::xsltTransformError(
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            c"unparsed-entity-uri() : expects one string arg\n".as_ptr() as *const c_char,
        );
        (*pc).error = XPATH_INVALID_ARITY as c_int;
        return;
    }
    obj = value_pop(pc);
    if (*obj).type_ != xmlXPathObjectType::XPATH_STRING as c_int {
        obj = crate::xml::xpath::exports::xmlXPathConvertString(obj);
        if obj.is_null() {
            (*pc).error = XPATH_MEMORY_ERROR as c_int;
            return;
        }
    }

    let str = (*obj).stringval;
    let xpctxt = (*pc).context;
    if str.is_null() || xpctxt.is_null() || (*xpctxt).doc.is_null() {
        value_push(pc, xmlXPathNewString(ptr::null()));
    } else {
        let entity = crate::abi::exports_xml2::xmlGetDocEntity((*xpctxt).doc, str);
        if entity.is_null() || (*entity).URI.is_null() {
            value_push(pc, xmlXPathNewString(ptr::null()));
        } else {
            value_push(pc, xmlXPathNewString((*entity).URI as *const xmlChar));
        }
    }
    crate::abi::exports_xml2::xmlXPathFreeObject(obj);
}

// ── format-number(): picture-format algorithm (numbers.c) ──────────────────────

/// `xsltUTF8Charcmp` (numbers.c): compare the first UTF-8 char of `utf1`
/// with the string at `utf2` (byte comparison of the char's encoded form).
unsafe fn utf8_charcmp(utf1: *const xmlChar, utf2: *const xmlChar) -> c_int {
    if utf1.is_null() {
        return -1;
    }
    let len = crate::abi::exports_string::xmlUTF8Strsize(utf1, 1);
    if len < 1 {
        return -1;
    }
    crate::abi::exports_xml2::xmlStrncmp(utf1, utf2, len)
}

/// `IS_SPECIAL` (numbers.c).
unsafe fn is_special(self_: *mut _xsltDecimalFormat, letter: *const xmlChar) -> bool {
    unsafe {
        utf8_charcmp(letter, (*self_).zeroDigit) == 0
            || utf8_charcmp(letter, (*self_).digit) == 0
            || utf8_charcmp(letter, (*self_).decimalPoint) == 0
            || utf8_charcmp(letter, (*self_).grouping) == 0
            || utf8_charcmp(letter, (*self_).patternSeparator) == 0
    }
}

/// `xsltCopyCharMultiByte` (numbers.c): encode a UCS-4 value as UTF-8 into
/// `out`; returns the number of bytes written.
unsafe fn copy_char_multibyte(mut out: *mut xmlChar, val: c_int) -> c_int {
    if out.is_null() || val < 0 {
        return 0;
    }
    unsafe {
        if val < 0x80 {
            *out = val as xmlChar;
            return 1;
        }
        let savedout = out;
        let mut bits: c_int;
        if val < 0x800 {
            *out = ((val >> 6) as xmlChar) | 0xC0;
            bits = 0;
            out = out.add(1);
        } else if val < 0x10000 {
            *out = ((val >> 12) as xmlChar) | 0xE0;
            bits = 6;
            out = out.add(1);
        } else if val < 0x110000 {
            *out = ((val >> 18) as xmlChar) | 0xF0;
            bits = 12;
            out = out.add(1);
        } else {
            return 0;
        }
        while bits >= 0 {
            *out = (((val >> bits) as xmlChar) & 0x3F) | 0x80;
            out = out.add(1);
            bits -= 6;
        }
        out.offset_from(savedout) as c_int
    }
}

/// `xsltGetUTF8Char` (xsltutils.c): decode the first UTF-8 char of `utf`;
/// `*len` is the available byte count and is updated to the char length.
unsafe fn xslt_get_utf8_char(utf: *const u8, len: *mut c_int) -> c_int {
    unsafe {
        if utf.is_null() || len.is_null() || *len < 1 {
            if !len.is_null() {
                *len = 0;
            }
            return -1;
        }
        let mut c: c_int = *utf as c_int;
        if (c & 0x80) != 0 {
            if *len < 2 || (*utf.add(1) & 0xc0) != 0x80 {
                *len = 0;
                return -1;
            }
            if (c & 0xe0) == 0xe0 {
                if *len < 3 || (*utf.add(2) & 0xc0) != 0x80 {
                    *len = 0;
                    return -1;
                }
                if (c & 0xf0) == 0xf0 {
                    if *len < 4 || (c & 0xf8) != 0xf0 || (*utf.add(3) & 0xc0) != 0x80 {
                        *len = 0;
                        return -1;
                    }
                    *len = 4;
                    c = (*utf as c_int & 0x7) << 18;
                    c |= (*utf.add(1) as c_int & 0x3f) << 12;
                    c |= (*utf.add(2) as c_int & 0x3f) << 6;
                    c |= *utf.add(3) as c_int & 0x3f;
                } else {
                    *len = 3;
                    c = (*utf as c_int & 0xf) << 12;
                    c |= (*utf.add(1) as c_int & 0x3f) << 6;
                    c |= *utf.add(2) as c_int & 0x3f;
                }
            } else {
                *len = 2;
                c = (*utf as c_int & 0x1f) << 6;
                c |= *utf.add(1) as c_int & 0x3f;
            }
        } else {
            *len = 1;
        }
        c
    }
}

/// `xsltNumberFormatDecimal` (numbers.c): write the decimal digits of
/// `number` into `buffer` with the given width, grouping and zero digit.
unsafe fn number_format_decimal(
    buffer: *mut _xmlBuffer,
    number: f64,
    digit_zero: c_int,
    width: c_int,
    digits_per_group: c_int,
    grouping_character: c_int,
    grouping_character_len: c_int,
) {
    unsafe {
        let mut temp_string = [0u8; 500];
        let mut temp_char = [0u8; 6];
        let mut i: c_int = 0;
        let mut number = number;

        // Build the buffer from the back.
        let mut pointer = (temp_string.as_mut_ptr() as usize + temp_string.len() - 1) as *mut u8;
        *pointer = 0;
        while (pointer as usize) > (temp_string.as_ptr() as usize) {
            if (i >= width) && number.abs() < 1.0 {
                break;
            }
            if (i > 0)
                && (grouping_character != 0)
                && (digits_per_group > 0)
                && ((i % digits_per_group) == 0)
            {
                if (pointer as usize)
                    .checked_sub(grouping_character_len as usize)
                    .is_none_or(|p| p < temp_string.as_ptr() as usize)
                {
                    i = -1; // flag error
                    break;
                }
                pointer = pointer.sub(grouping_character_len as usize);
                copy_char_multibyte(pointer as *mut xmlChar, grouping_character);
            }

            let val = digit_zero + (number % 10.0) as c_int;
            if val < 0x80 {
                // Shortcut if ASCII.
                if (pointer as usize) <= (temp_string.as_ptr() as usize) {
                    i = -1;
                    break;
                }
                pointer = pointer.sub(1);
                *pointer = val as u8;
            } else {
                // Multibyte character: encode into temp_char, then copy.
                let len = copy_char_multibyte(temp_char.as_mut_ptr() as *mut xmlChar, val);
                if (pointer as usize)
                    .checked_sub(len as usize)
                    .is_none_or(|p| p < temp_string.as_ptr() as usize)
                {
                    i = -1;
                    break;
                }
                pointer = pointer.sub(len as usize);
                ptr::copy_nonoverlapping(temp_char.as_ptr(), pointer, len as usize);
            }
            number /= 10.0;
            i += 1;
        }
        if i < 0 {
            // xsltGenericError(xsltGenericErrorContext, "xsltNumberFormatDecimal: ...")
            let msg = b"xsltNumberFormatDecimal: Internal buffer size exceeded\n";
            libc::write(2, msg.as_ptr() as *const libc::c_void, msg.len());
        }
        crate::abi::exports_xml2::xmlBufferCat(buffer, pointer as *mut xmlChar);
    }
}

/// `xsltFormatNumberPreSuffix` (numbers.c): consume a prefix/suffix of the
/// format string, returning its byte length (or -1 on error).
unsafe fn format_number_presuffix(
    self_: *mut _xsltDecimalFormat,
    format: *mut *mut xmlChar,
    info: *mut FormatNumberInfo,
) -> c_int {
    let mut count: c_int = 0;
    loop {
        let cur = *format;
        if cur.is_null() || *cur == 0 {
            return count;
        }
        if *cur == SYMBOL_QUOTE {
            let next = cur.add(1);
            *format = next;
            if *next == 0 {
                return -1;
            }
        } else if is_special(self_, cur) {
            return count;
        } else if utf8_charcmp(cur, (*self_).percent) == 0 {
            if (*info).is_multiplier_set != 0 {
                return -1;
            }
            (*info).multiplier = 100;
            (*info).is_multiplier_set = 1;
        } else if utf8_charcmp(cur, (*self_).permille) == 0 {
            if (*info).is_multiplier_set != 0 {
                return -1;
            }
            (*info).multiplier = 1000;
            (*info).is_multiplier_set = 1;
        }
        let len = crate::abi::exports_string::xmlUTF8Strsize(*format, 1);
        if len < 1 {
            return -1;
        }
        count += len;
        *format = (*format).add(len as usize);
    }
}

/// `xsltFormatNumberInfo` (numbers.c) — parsed picture-format state.
#[repr(C)]
#[derive(Clone, Copy)]
struct FormatNumberInfo {
    integer_hash: c_int,
    integer_digits: c_int,
    frac_digits: c_int,
    frac_hash: c_int,
    group: c_int,
    multiplier: c_int,
    add_decimal: c_int,
    is_multiplier_set: c_int,
    is_negative_pattern: c_int,
}

impl Default for FormatNumberInfo {
    fn default() -> Self {
        FormatNumberInfo {
            integer_hash: 0,
            integer_digits: 0,
            frac_digits: 0,
            frac_hash: 0,
            group: -1,
            multiplier: 1,
            add_decimal: 0,
            is_multiplier_set: 0,
            is_negative_pattern: 0,
        }
    }
}

/// `xsltFormatNumberConversion` (numbers.c): the JDK 1.1 DecimalFormat-style
/// picture-format algorithm behind format-number().
///
/// Returns an XPath error code; `*result` receives an xmlMalloc'd string
/// (NULL on failure).
unsafe fn format_number_conversion(
    self_: *mut _xsltDecimalFormat,
    format: *mut xmlChar,
    number: f64,
    result: *mut *mut xmlChar,
) -> c_int {
    unsafe {
        let status = XPATH_EXPRESSION_OK as c_int;
        let mut the_format: *mut xmlChar = ptr::null_mut();
        let mut prefix: *mut xmlChar = ptr::null_mut();
        let mut suffix: *mut xmlChar = ptr::null_mut();
        let mut nprefix: *mut xmlChar = ptr::null_mut();
        let mut nsuffix: *mut xmlChar = ptr::null_mut();
        let mut prefix_length: c_int = 0;
        let mut suffix_length: c_int = 0;
        let mut nprefix_length: c_int = 0;
        let mut nsuffix_length: c_int = 0;
        let mut len: c_int = 0;
        let j: c_int;
        let mut info = FormatNumberInfo::default();
        let mut delayed_multiplier: c_int = 0;
        let mut default_sign: c_int = 0;
        let mut found_error: c_int = 0;
        let mut number = number;

        if crate::abi::exports_xml2::xmlStrlen(format) <= 0 {
            xslt_error(
                ptr::null_mut(),
                b"xsltFormatNumberConversion : Invalid format (0-length)\n\0",
            );
        }
        *result = ptr::null_mut();
        if crate::xml::xpath::exports::xmlXPathIsNaN(number) != 0 {
            *result = if self_.is_null() || (*self_).noNumber.is_null() {
                crate::abi::exports_xml2::xmlStrdup(c"NaN".as_ptr() as *const xmlChar)
            } else {
                crate::abi::exports_xml2::xmlStrdup((*self_).noNumber)
            };
            return status;
        }

        the_format = format;

        // First process the +ve pattern to get percent/permille as well as
        // the main format.
        prefix = the_format;
        prefix_length = format_number_presuffix(self_, &mut the_format, &mut info);
        if prefix_length < 0 {
            found_error = 1;
        }

        // Process the "number" part of the format. A trailing percent or
        // per-mille may be part of the suffix, so it is delayed.
        let self_grouping_len = crate::abi::exports_xml2::xmlStrlen((*self_).grouping);
        while found_error == 0
            && !the_format.is_null()
            && *the_format != 0
            && utf8_charcmp(the_format, (*self_).decimalPoint) != 0
            && utf8_charcmp(the_format, (*self_).patternSeparator) != 0
        {
            if delayed_multiplier != 0 {
                info.multiplier = delayed_multiplier;
                info.is_multiplier_set = 1;
                delayed_multiplier = 0;
            }
            if utf8_charcmp(the_format, (*self_).digit) == 0 {
                if info.integer_digits > 0 {
                    found_error = 1;
                    break;
                }
                info.integer_hash += 1;
                if info.group >= 0 {
                    info.group += 1;
                }
            } else if utf8_charcmp(the_format, (*self_).zeroDigit) == 0 {
                info.integer_digits += 1;
                if info.group >= 0 {
                    info.group += 1;
                }
            } else if (self_grouping_len > 0)
                && !(*self_).grouping.is_null()
                && crate::abi::exports_xml2::xmlStrncmp(
                    the_format,
                    (*self_).grouping,
                    self_grouping_len,
                ) == 0
            {
                // Reset group count.
                info.group = 0;
                the_format = the_format.add(self_grouping_len as usize);
                continue;
            } else if utf8_charcmp(the_format, (*self_).percent) == 0 {
                if info.is_multiplier_set != 0 {
                    found_error = 1;
                    break;
                }
                delayed_multiplier = 100;
            } else if utf8_charcmp(the_format, (*self_).permille) == 0 {
                if info.is_multiplier_set != 0 {
                    found_error = 1;
                    break;
                }
                delayed_multiplier = 1000;
            } else {
                break; // while
            }

            len = crate::abi::exports_string::xmlUTF8Strsize(the_format, 1);
            if len < 1 {
                found_error = 1;
                break;
            }
            the_format = the_format.add(len as usize);
        }

        // Finished the integer part; now work on the fraction.
        if found_error == 0
            && !the_format.is_null()
            && *the_format != 0
            && utf8_charcmp(the_format, (*self_).decimalPoint) == 0
        {
            info.add_decimal = 1;
            len = crate::abi::exports_string::xmlUTF8Strsize(the_format, 1);
            if len < 1 {
                found_error = 1;
            } else {
                the_format = the_format.add(len as usize); // skip over the decimal
            }
        }

        while found_error == 0 && !the_format.is_null() && *the_format != 0 {
            if utf8_charcmp(the_format, (*self_).zeroDigit) == 0 {
                if info.frac_hash != 0 {
                    found_error = 1;
                    break;
                }
                info.frac_digits += 1;
            } else if utf8_charcmp(the_format, (*self_).digit) == 0 {
                info.frac_hash += 1;
            } else if utf8_charcmp(the_format, (*self_).percent) == 0 {
                if info.is_multiplier_set != 0 {
                    found_error = 1;
                    break;
                }
                delayed_multiplier = 100;
                len = crate::abi::exports_string::xmlUTF8Strsize(the_format, 1);
                if len < 1 {
                    found_error = 1;
                    break;
                }
                the_format = the_format.add(len as usize);
                continue;
            } else if utf8_charcmp(the_format, (*self_).permille) == 0 {
                if info.is_multiplier_set != 0 {
                    found_error = 1;
                    break;
                }
                delayed_multiplier = 1000;
                len = crate::abi::exports_string::xmlUTF8Strsize(the_format, 1);
                if len < 1 {
                    found_error = 1;
                    break;
                }
                the_format = the_format.add(len as usize);
                continue;
            } else if utf8_charcmp(the_format, (*self_).grouping) != 0 {
                break; // while
            }
            len = crate::abi::exports_string::xmlUTF8Strsize(the_format, 1);
            if len < 1 {
                found_error = 1;
                break;
            }
            the_format = the_format.add(len as usize);
            if delayed_multiplier != 0 {
                info.multiplier = delayed_multiplier;
                delayed_multiplier = 0;
                info.is_multiplier_set = 1;
            }
        }

        // If delayed_multiplier is set after processing the "number" part,
        // it should be in the suffix.
        if delayed_multiplier != 0 && found_error == 0 {
            the_format = the_format.sub(len as usize);
            delayed_multiplier = 0;
        }

        suffix = the_format;
        suffix_length = format_number_presuffix(self_, &mut the_format, &mut info);
        if (suffix_length < 0)
            || (!the_format.is_null()
                && *the_format != 0
                && utf8_charcmp(the_format, (*self_).patternSeparator) != 0)
        {
            found_error = 1;
        }

        // If the number is -ve, substitute the -ve prefix/suffix.
        if found_error == 0 && number < 0.0 {
            // j is the number of UTF-8 chars before the separator.
            j = crate::abi::exports_string::xmlUTF8Strloc(format, (*self_).patternSeparator);
            if j < 0 {
                // No -ve pattern present, so use default signing.
                default_sign = 1;
            } else {
                // Skip over the pattern separator (accounting for UTF-8).
                the_format =
                    crate::abi::exports_string::xmlUTF8Strpos(format, j + 1) as *mut xmlChar;
                // Flag changes interpretation of percent/permille in the
                // -ve pattern.
                info.is_negative_pattern = 1;
                info.is_multiplier_set = 0;

                // First do the -ve prefix.
                nprefix = the_format;
                nprefix_length = format_number_presuffix(self_, &mut the_format, &mut info);
                if nprefix_length < 0 {
                    found_error = 1;
                }
                while found_error == 0 && !the_format.is_null() && *the_format != 0 {
                    if (utf8_charcmp(the_format, (*self_).percent) == 0)
                        || (utf8_charcmp(the_format, (*self_).permille) == 0)
                    {
                        if info.is_multiplier_set != 0 {
                            found_error = 1;
                            break;
                        }
                        info.is_multiplier_set = 1;
                        delayed_multiplier = 1;
                    } else if is_special(self_, the_format) {
                        delayed_multiplier = 0;
                    } else {
                        break; // while
                    }
                    len = crate::abi::exports_string::xmlUTF8Strsize(the_format, 1);
                    if len < 1 {
                        found_error = 1;
                        break;
                    }
                    the_format = the_format.add(len as usize);
                }
                if delayed_multiplier != 0 && found_error == 0 {
                    info.is_multiplier_set = 0;
                    the_format = the_format.sub(len as usize);
                }

                // Finally do the -ve suffix.
                if found_error == 0 && !the_format.is_null() && *the_format != 0 {
                    nsuffix = the_format;
                    nsuffix_length = format_number_presuffix(self_, &mut the_format, &mut info);
                    if nsuffix_length < 0 {
                        found_error = 1;
                    }
                } else if found_error == 0 {
                    nsuffix_length = 0;
                }
                if found_error == 0 && !the_format.is_null() && *the_format != 0 {
                    found_error = 1;
                }
                if found_error == 0
                    && ((nprefix_length != prefix_length)
                        || (nsuffix_length != suffix_length)
                        || ((nprefix_length > 0)
                            && crate::abi::exports_xml2::xmlStrncmp(
                                nprefix,
                                prefix,
                                prefix_length,
                            ) != 0)
                        || ((nsuffix_length > 0)
                            && crate::abi::exports_xml2::xmlStrncmp(
                                nsuffix,
                                suffix,
                                suffix_length,
                            ) != 0))
                {
                    prefix = nprefix;
                    prefix_length = nprefix_length;
                    suffix = nsuffix;
                    suffix_length = nsuffix_length;
                }
            }
        }

        // OUTPUT_NUMBER: on error use the default format.
        if found_error != 0 {
            xslt_error(
                ptr::null_mut(),
                b"xsltFormatNumberConversion : error in format string '%s', using default\n\0",
            );
            default_sign = if number < 0.0 { 1 } else { 0 };
            prefix_length = 0;
            suffix_length = 0;
            info.integer_hash = 0;
            info.integer_digits = 1;
            info.frac_digits = 1;
            info.frac_hash = 4;
            info.group = -1;
            info.multiplier = 1;
            info.add_decimal = 1;
        }

        // Apply the multiplier.
        number *= info.multiplier as f64;
        match crate::xml::xpath::exports::xmlXPathIsInf(number) {
            -1 => {
                // Intentional fall-through: minus sign then infinity.
                let minus = if (*self_).minusSign.is_null() {
                    c"-".as_ptr() as *const xmlChar
                } else {
                    (*self_).minusSign
                };
                let mut res = crate::abi::exports_xml2::xmlStrdup(minus);
                let inf = if self_.is_null() || (*self_).infinity.is_null() {
                    c"Infinity".as_ptr() as *const xmlChar
                } else {
                    (*self_).infinity
                };
                res = crate::abi::exports_xml2::xmlStrcat(res, inf);
                *result = res;
                return status;
            }
            1 => {
                let inf = if self_.is_null() || (*self_).infinity.is_null() {
                    c"Infinity".as_ptr() as *const xmlChar
                } else {
                    (*self_).infinity
                };
                *result = crate::abi::exports_xml2::xmlStrcat(ptr::null_mut(), inf);
                return status;
            }
            _ => {}
        }

        let buffer = crate::abi::exports_xml2::xmlBufferCreate();
        if buffer.is_null() {
            return XPATH_MEMORY_ERROR as c_int;
        }

        // Default sign first.
        if default_sign != 0 {
            crate::abi::exports_xml2::xmlBufferAdd(
                buffer,
                (*self_).minusSign,
                crate::abi::exports_string::xmlUTF8Strsize((*self_).minusSign, 1),
            );
        }

        // Prefix.
        let mut p = prefix;
        let mut j: c_int = 0;
        while j < prefix_length {
            if *p == SYMBOL_QUOTE {
                p = p.add(1);
            }
            len = crate::abi::exports_string::xmlUTF8Strsize(p, 1);
            crate::abi::exports_xml2::xmlBufferAdd(buffer, p, len);
            p = p.add(len as usize);
            j += len;
        }

        // Round to n digits.
        number = number.abs();
        let mut exp10: c_int = info.frac_digits + info.frac_hash;
        if exp10 > DBL_MAX_10_EXP {
            if info.frac_digits > DBL_MAX_10_EXP {
                info.frac_digits = DBL_MAX_10_EXP;
                info.frac_hash = 0;
            } else {
                info.frac_hash = DBL_MAX_10_EXP - info.frac_digits;
            }
            exp10 = DBL_MAX_10_EXP;
        }
        let scale = 10.0f64.powi(exp10);
        number += 0.5 / scale;
        number -= number % (1.0 / scale);

        // Integer part.
        if !(*self_).grouping.is_null() && *(*self_).grouping != 0 {
            len = crate::abi::exports_xml2::xmlStrlen((*self_).grouping);
            let gchar = xslt_get_utf8_char((*self_).grouping as *const u8, &mut len);
            number_format_decimal(
                buffer,
                number.floor(),
                *((*self_).zeroDigit) as c_int,
                info.integer_digits,
                info.group,
                gchar,
                len,
            );
        } else {
            number_format_decimal(
                buffer,
                number.floor(),
                *((*self_).zeroDigit) as c_int,
                info.integer_digits,
                info.group,
                b',' as c_int,
                1,
            );
        }

        // Special case: java treats '.#' like '.0', '.##' like '.0#', etc.
        if (info.integer_digits + info.integer_hash + info.frac_digits == 0) && (info.frac_hash > 0)
        {
            info.frac_digits += 1;
            info.frac_hash -= 1;
        }

        // Add a leading zero if required.
        if (number.floor() == 0.0) && (info.integer_digits + info.frac_digits == 0) {
            crate::abi::exports_xml2::xmlBufferAdd(
                buffer,
                (*self_).zeroDigit,
                crate::abi::exports_string::xmlUTF8Strsize((*self_).zeroDigit, 1),
            );
        }

        // Fractional part, if required.
        if info.frac_digits + info.frac_hash == 0 {
            if info.add_decimal != 0 {
                crate::abi::exports_xml2::xmlBufferAdd(
                    buffer,
                    (*self_).decimalPoint,
                    crate::abi::exports_string::xmlUTF8Strsize((*self_).decimalPoint, 1),
                );
            }
        } else {
            number -= number.floor();
            if (number != 0.0) || (info.frac_digits != 0) {
                crate::abi::exports_xml2::xmlBufferAdd(
                    buffer,
                    (*self_).decimalPoint,
                    crate::abi::exports_string::xmlUTF8Strsize((*self_).decimalPoint, 1),
                );
                number = (scale * number + 0.5).floor();
                let mut k = info.frac_hash;
                while k > 0 {
                    if number % 10.0 >= 1.0 {
                        break;
                    }
                    number /= 10.0;
                    k -= 1;
                }
                number_format_decimal(
                    buffer,
                    number.floor(),
                    *((*self_).zeroDigit) as c_int,
                    info.frac_digits + k,
                    0,
                    0,
                    0,
                );
            }
        }

        // Suffix.
        let mut s = suffix;
        let mut j: c_int = 0;
        while j < suffix_length {
            if *s == SYMBOL_QUOTE {
                s = s.add(1);
            }
            len = crate::abi::exports_string::xmlUTF8Strsize(s, 1);
            crate::abi::exports_xml2::xmlBufferAdd(buffer, s, len);
            s = s.add(len as usize);
            j += len;
        }

        *result =
            crate::abi::exports_xml2::xmlStrdup(crate::abi::exports_xml2::xmlBufferContent(buffer));
        crate::abi::exports_xml2::xmlBufferFree(buffer);
        status
    }
}

/// `xsltDecimalFormatGetByQName` (xslt.c): find a decimal format by QName,
/// searching the stylesheet and its imports.
unsafe fn decimal_format_get_by_qname(
    style: *mut _xsltStylesheet,
    ns_uri: *const xmlChar,
    name: *const xmlChar,
) -> *mut _xsltDecimalFormat {
    crate::xslt::numbering::decimal_format_by_qname(style, ns_uri, name)
}

/// Implement the XSLT `format-number()` function:
/// `string format-number(number, string, string?)`.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltFormatNumberFunction(xmlXPathParserContextPtr ctxt, int nargs);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be a valid parser context.
#[no_mangle]
pub unsafe extern "C" fn xsltFormatNumberFunction(ctxt: *mut c_void, nargs: c_int) {
    let pc = ctxt as *mut XmlXPathParserContext;
    if pc.is_null() {
        return;
    }
    let mut number_obj: *mut _xmlXPathObject = ptr::null_mut();
    let mut format_obj: *mut _xmlXPathObject = ptr::null_mut();
    let mut decimal_obj: *mut _xmlXPathObject = ptr::null_mut();
    let mut format_values: *mut _xsltDecimalFormat = ptr::null_mut();

    let tctxt = transform_context_from_parser(ctxt);
    if tctxt.is_null() || (*tctxt).inst.is_null() {
        return;
    }
    let sheet = (*tctxt).style;
    if sheet.is_null() {
        return;
    }
    // UPSTREAM-PARITY (functions.c xsltFormatNumberFunction): start from
    // the default format (the chain head).
    format_values = (*sheet).decimalFormat;

    match nargs {
        3 => {
            if !(*pc).value.is_null()
                && (*(*pc).value).type_ != xmlXPathObjectType::XPATH_STRING as c_int
            {
                crate::xml::xpath::exports::xmlXPathStringFunction(ctxt, 1);
            }
            decimal_obj = value_pop(pc);
            let (prefix, ncname) = split_qname_ref((*decimal_obj).stringval);
            let mut ns_uri: *const xmlChar = ptr::null();
            let mut ncname = ncname;
            if !prefix.is_null() {
                let ns = crate::abi::exports_xml2::xmlSearchNs(
                    (*(*tctxt).inst).doc,
                    (*tctxt).inst,
                    prefix,
                );
                if ns.is_null() {
                    let msg = format!(
                        "format-number : No namespace found for QName '{}:{}'\n",
                        cstr_to_string(prefix),
                        cstr_to_string(ncname)
                    );
                    emit_fn_error(tctxt, msg.as_bytes());
                    (*sheet).errors += 1;
                    ncname = ptr::null();
                } else {
                    ns_uri = (*ns).href;
                }
            }
            if !ncname.is_null() {
                format_values = decimal_format_get_by_qname(sheet, ns_uri, ncname);
            }
            if format_values.is_null() {
                let msg = format!(
                    "format-number() : undeclared decimal format '{}'\n",
                    cstr_to_string((*decimal_obj).stringval)
                );
                emit_fn_error(tctxt, msg.as_bytes());
            }
            // Intentional fall-through.
            if !(*pc).value.is_null()
                && (*(*pc).value).type_ != xmlXPathObjectType::XPATH_STRING as c_int
            {
                crate::xml::xpath::exports::xmlXPathStringFunction(ctxt, 1);
            }
            format_obj = value_pop(pc);
            if !(*pc).value.is_null()
                && (*(*pc).value).type_ != xmlXPathObjectType::XPATH_NUMBER as c_int
            {
                crate::xml::xpath::exports::xmlXPathNumberFunction(ctxt, 1);
            }
            number_obj = value_pop(pc);
        }
        2 => {
            if !(*pc).value.is_null()
                && (*(*pc).value).type_ != xmlXPathObjectType::XPATH_STRING as c_int
            {
                crate::xml::xpath::exports::xmlXPathStringFunction(ctxt, 1);
            }
            format_obj = value_pop(pc);
            if !(*pc).value.is_null()
                && (*(*pc).value).type_ != xmlXPathObjectType::XPATH_NUMBER as c_int
            {
                crate::xml::xpath::exports::xmlXPathNumberFunction(ctxt, 1);
            }
            number_obj = value_pop(pc);
        }
        _ => {
            (*pc).error = XPATH_INVALID_ARITY as c_int;
            return;
        }
    }

    if (*pc).error == 0
        && !format_values.is_null()
        && !format_obj.is_null()
        && !number_obj.is_null()
    {
        let mut result: *mut xmlChar = ptr::null_mut();
        if format_number_conversion(
            format_values,
            (*format_obj).stringval,
            (*number_obj).floatval,
            &mut result,
        ) == XPATH_EXPRESSION_OK as c_int
        {
            value_push(pc, xmlXPathNewString(result));
            if !result.is_null() {
                xmlFreeImpl(result as *mut c_void);
            }
        }
    }

    crate::abi::exports_xml2::xmlXPathFreeObject(number_obj);
    crate::abi::exports_xml2::xmlXPathFreeObject(format_obj);
    crate::abi::exports_xml2::xmlXPathFreeObject(decimal_obj);
}

/// Implement the XSLT `generate-id()` function:
/// `string generate-id(node-set?)`.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltGenerateIdFunction(xmlXPathParserContextPtr ctxt, int nargs);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be a valid parser context.
#[no_mangle]
pub unsafe extern "C" fn xsltGenerateIdFunction(ctxt: *mut c_void, nargs: c_int) {
    let pc = ctxt as *mut XmlXPathParserContext;
    if pc.is_null() {
        return;
    }
    let mut cur: *mut _xmlNode = ptr::null_mut();
    let mut obj: *mut _xmlXPathObject = ptr::null_mut();
    let mut ns_prefix: *const xmlChar = ptr::null();
    let id: c_ulong;
    let mut ns_prefix_size: usize = 0;

    let tctxt = transform_context_from_parser(ctxt);

    if nargs == 0 {
        let xpctxt = (*pc).context;
        cur = if xpctxt.is_null() {
            ptr::null_mut()
        } else {
            (*xpctxt).node
        };
    } else if nargs == 1 {
        if (*pc).value.is_null()
            || (*(*pc).value).type_ != xmlXPathObjectType::XPATH_NODESET as c_int
        {
            (*pc).error = XPATH_INVALID_TYPE as c_int;
            xslt_error(
                tctxt,
                b"generate-id() : invalid arg expecting a node-set\n\0",
            );
            // goto out
            crate::abi::exports_xml2::xmlXPathFreeObject(obj);
            return;
        }
        obj = value_pop(pc);
        let nodelist = (*obj).nodesetval as *mut _xmlNodeSet;
        if nodelist.is_null() || (*nodelist).nodeNr <= 0 {
            value_push(
                pc,
                crate::abi::exports_xml2::xmlXPathNewCString(ptr::null()),
            );
            // goto out
            crate::abi::exports_xml2::xmlXPathFreeObject(obj);
            return;
        }
        cur = *(*nodelist).nodeTab;
        let mut i: c_int = 1;
        while i < (*nodelist).nodeNr {
            let ret = crate::abi::exports_xml2::xmlXPathCmpNodes(
                cur,
                *(*nodelist).nodeTab.offset(i as isize),
            );
            if ret == -1 {
                cur = *(*nodelist).nodeTab.offset(i as isize);
            }
            i += 1;
        }
    } else {
        xslt_error(tctxt, b"generate-id() : invalid number of args %d\n\0");
        (*pc).error = XPATH_INVALID_ARITY as c_int;
        // goto out
        crate::abi::exports_xml2::xmlXPathFreeObject(obj);
        return;
    }

    let mut size: usize = 30; // for "id%lu"

    if cur.is_null() {
        (*pc).error = XPATH_INVALID_TYPE as c_int;
        crate::abi::exports_xml2::xmlXPathFreeObject(obj);
        return;
    }

    if (*cur).type_ == XML_NAMESPACE_DECL as c_int {
        let ns = cur as *mut _xmlNs;
        ns_prefix = (*ns).prefix;
        if ns_prefix.is_null() {
            ns_prefix = c"".as_ptr() as *const xmlChar;
        }
        ns_prefix_size = crate::abi::exports_xml2::xmlStrlen(ns_prefix) as usize;
        // For "ns" and the hex-encoded string.
        size += ns_prefix_size * 2 + 2;
        // Parent is stored in 'next'.
        cur = (*ns).next as *mut _xmlNode;
    }

    let psvi_ptr = get_psvi_ptr(cur);
    if psvi_ptr.is_null() {
        xslt_error(tctxt, b"generate-id(): invalid node type %d\n\0");
        (*pc).error = XPATH_INVALID_TYPE as c_int;
        // goto out
        crate::abi::exports_xml2::xmlXPathFreeObject(obj);
        return;
    }

    if (get_source_node_flags(cur) & XSLT_SOURCE_NODE_HAS_ID) != 0 {
        id = *psvi_ptr as c_ulong;
    } else {
        if (*cur).type_ == XML_TEXT_NODE as c_int && (*cur).line == u16::MAX {
            // Text nodes store big line numbers in psvi.
            (*cur).line = 0;
        } else if !(*psvi_ptr).is_null() {
            xslt_error(tctxt, b"generate-id(): psvi already set\n\0");
            (*pc).error = XPATH_MEMORY_ERROR as c_int;
            // goto out
            crate::abi::exports_xml2::xmlXPathFreeObject(obj);
            return;
        }

        if !tctxt.is_null() && (*tctxt).currentId == c_ulong::MAX {
            xslt_error(tctxt, b"generate-id(): id overflow\n\0");
            (*pc).error = XPATH_MEMORY_ERROR as c_int;
            // goto out
            crate::abi::exports_xml2::xmlXPathFreeObject(obj);
            return;
        }

        if tctxt.is_null() {
            (*pc).error = XPATH_MEMORY_ERROR as c_int;
            crate::abi::exports_xml2::xmlXPathFreeObject(obj);
            return;
        }
        id = (*tctxt).currentId + 1;
        (*tctxt).currentId = id;
        *psvi_ptr = id as *mut c_void;
        set_source_node_flags(tctxt, cur, XSLT_SOURCE_NODE_HAS_ID);
    }

    let buf = xmlMallocImpl(size) as *mut u8;
    if buf.is_null() {
        xslt_error(tctxt, b"generate-id(): out of memory\n\0");
        (*pc).error = XPATH_MEMORY_ERROR as c_int;
        // goto out
        crate::abi::exports_xml2::xmlXPathFreeObject(obj);
        return;
    }
    let mut content: Vec<u8> = Vec::with_capacity(size);
    if ns_prefix.is_null() {
        content.extend_from_slice(format!("id{}", id).as_bytes());
    } else {
        content.extend_from_slice(format!("id{}ns", id).as_bytes());
        // Only ASCII alphanumerics are allowed, so hex-encode the prefix.
        for i in 0..ns_prefix_size {
            let v = *ns_prefix.add(i) >> 4;
            content.push(if v < 10 { b'0' + v } else { b'A' + (v - 10) });
            let v = *ns_prefix.add(i) & 15;
            content.push(if v < 10 { b'0' + v } else { b'A' + (v - 10) });
        }
    }
    debug_assert!(content.len() < size);
    ptr::copy_nonoverlapping(content.as_ptr(), buf, content.len());
    *buf.add(content.len()) = 0;
    value_push(
        pc,
        crate::xml::xpath::exports::xmlXPathWrapString(buf as *mut xmlChar),
    );

    // out:
    crate::abi::exports_xml2::xmlXPathFreeObject(obj);
}

/// Implement the XSLT `system-property()` function:
/// `object system-property(string)`.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltSystemPropertyFunction(xmlXPathParserContextPtr ctxt, int nargs);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be a valid parser context.
#[no_mangle]
pub unsafe extern "C" fn xsltSystemPropertyFunction(ctxt: *mut c_void, nargs: c_int) {
    let pc = ctxt as *mut XmlXPathParserContext;
    if pc.is_null() {
        return;
    }
    let mut obj: *mut _xmlXPathObject = ptr::null_mut();

    if nargs != 1 {
        xslt_error(
            transform_context_from_parser(ctxt),
            b"system-property() : expects one string arg\n\0",
        );
        (*pc).error = XPATH_INVALID_ARITY as c_int;
        return;
    }
    if (*pc).value.is_null() || (*(*pc).value).type_ != xmlXPathObjectType::XPATH_STRING as c_int {
        xslt_error(
            transform_context_from_parser(ctxt),
            b"system-property() : invalid arg expecting a string\n\0",
        );
        (*pc).error = XPATH_INVALID_TYPE as c_int;
        return;
    }
    obj = value_pop(pc);

    if (*obj).stringval.is_null() {
        value_push(pc, xmlXPathNewString(ptr::null()));
    } else {
        let mut name: *mut xmlChar = ptr::null_mut();
        let mut ns_uri: *const xmlChar = ptr::null();
        let mut prefix: *mut xmlChar = ptr::null_mut();
        name = crate::xml::string::split_qname2((*obj).stringval, &mut prefix);
        if name.is_null() {
            name = crate::abi::exports_xml2::xmlStrdup((*obj).stringval);
        } else {
            let xpctxt = (*pc).context;
            ns_uri = if xpctxt.is_null() {
                ptr::null()
            } else {
                crate::xml::xpath::exports::xmlXPathNsLookup(xpctxt, prefix)
            };
            if ns_uri.is_null() {
                xslt_error(
                    transform_context_from_parser(ctxt),
                    b"system-property() : prefix %s is not bound\n\0",
                );
            }
        }

        if crate::abi::exports_xml2::xmlStrEqual(ns_uri, XSLT_NAMESPACE.as_ptr() as *const xmlChar)
            != 0
        {
            if xml_chars_equal(name, c"vendor".as_ptr() as *const xmlChar) {
                // DOCBOOK_XSL_HACK (functions.c): DocBook XSL uses the vendor
                // string to detect chunking support.
                let tctxt = transform_context_from_parser(ctxt);
                let sheet = if !tctxt.is_null()
                    && !(*tctxt).inst.is_null()
                    && xml_chars_equal(
                        (*(*tctxt).inst).name,
                        c"variable".as_ptr() as *const xmlChar,
                    )
                    && !(*(*tctxt).inst).parent.is_null()
                    && xml_chars_equal(
                        (*(*(*tctxt).inst).parent).name,
                        c"template".as_ptr() as *const xmlChar,
                    ) {
                    (*tctxt).style
                } else {
                    ptr::null_mut()
                };
                if !sheet.is_null()
                    && !(*sheet).doc.is_null()
                    && !(*(*sheet).doc).URL.is_null()
                    && !crate::abi::exports_string::xmlStrstr(
                        (*(*sheet).doc).URL as *const xmlChar,
                        c"chunk".as_ptr() as *const xmlChar,
                    )
                    .is_null()
                {
                    value_push(
                        pc,
                        xmlXPathNewString(
                            c"libxslt (SAXON 6.2 compatible)".as_ptr() as *const xmlChar
                        ),
                    );
                } else {
                    value_push(
                        pc,
                        xmlXPathNewString(XSLT_DEFAULT_VENDOR.as_ptr() as *const xmlChar),
                    );
                }
            } else if xml_chars_equal(name, c"version".as_ptr() as *const xmlChar) {
                value_push(
                    pc,
                    xmlXPathNewString(XSLT_DEFAULT_VERSION.as_ptr() as *const xmlChar),
                );
            } else if xml_chars_equal(name, c"vendor-url".as_ptr() as *const xmlChar) {
                value_push(
                    pc,
                    xmlXPathNewString(XSLT_DEFAULT_URL.as_ptr() as *const xmlChar),
                );
            } else {
                value_push(pc, xmlXPathNewString(ptr::null()));
            }
        } else {
            value_push(pc, xmlXPathNewString(ptr::null()));
        }
        if !name.is_null() {
            xmlFreeImpl(name as *mut c_void);
        }
        if !prefix.is_null() {
            xmlFreeImpl(prefix as *mut c_void);
        }
    }
    crate::abi::exports_xml2::xmlXPathFreeObject(obj);
}

/// The standard XSLT elements registered by upstream `xsltRegisterAllElement`
/// (transform.c) — used by element-available().
static XSLT_ELEMENTS: &[&[u8]] = &[
    b"apply-templates\0",
    b"apply-imports\0",
    b"call-template\0",
    b"element\0",
    b"attribute\0",
    b"text\0",
    b"processing-instruction\0",
    b"comment\0",
    b"copy\0",
    b"value-of\0",
    b"number\0",
    b"for-each\0",
    b"if\0",
    b"choose\0",
    b"sort\0",
    b"copy-of\0",
    b"message\0",
    b"variable\0",
    b"param\0",
    b"with-param\0",
    b"decimal-format\0",
    b"when\0",
    b"otherwise\0",
    b"fallback\0",
];

/// `xsltExtElementLookup` (extensions.c): context-level extension elements
/// first, then the module registry.
///
/// UPSTREAM-PARITY (simplified): the candidate has no module-level element
/// registry (`xsltRegisterExtModuleElement`); the standard XSLT instruction
/// elements that upstream registers globally are reported available here.
unsafe fn xslt_ext_element_lookup(
    ctxt: *mut _xsltTransformContext,
    name: *const xmlChar,
    ns_uri: *const xmlChar,
) -> *mut c_void {
    if name.is_null() || ns_uri.is_null() {
        return ptr::null_mut();
    }
    if !ctxt.is_null() && !(*ctxt).extElements.is_null() {
        let f = crate::xslt::extensions::xsltFindExtElement(ctxt, name, ns_uri);
        if !f.is_null() {
            return f;
        }
    }
    if xml_chars_equal(ns_uri, XSLT_NAMESPACE.as_ptr() as *const xmlChar) {
        for e in XSLT_ELEMENTS {
            if xml_chars_equal(name, e.as_ptr() as *const xmlChar) {
                // Only a non-NULL presence marker is tested by the caller.
                return std::ptr::dangling_mut::<c_void>();
            }
        }
    }
    ptr::null_mut()
}

/// Implement the XSLT `element-available()` function:
/// `boolean element-available(string)`.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltElementAvailableFunction(xmlXPathParserContextPtr ctxt, int nargs);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be a valid parser context.
#[no_mangle]
pub unsafe extern "C" fn xsltElementAvailableFunction(ctxt: *mut c_void, nargs: c_int) {
    let pc = ctxt as *mut XmlXPathParserContext;
    if pc.is_null() {
        return;
    }
    let mut obj: *mut _xmlXPathObject = ptr::null_mut();

    if nargs != 1 {
        xslt_error(
            transform_context_from_parser(ctxt),
            b"element-available() : expects one string arg\n\0",
        );
        (*pc).error = XPATH_INVALID_ARITY as c_int;
        return;
    }
    crate::xml::xpath::exports::xmlXPathStringFunction(ctxt, 1);
    if (*pc).value.is_null() || (*(*pc).value).type_ != xmlXPathObjectType::XPATH_STRING as c_int {
        xslt_error(
            transform_context_from_parser(ctxt),
            b"element-available() : invalid arg expecting a string\n\0",
        );
        (*pc).error = XPATH_INVALID_TYPE as c_int;
        return;
    }
    obj = value_pop(pc);
    let tctxt = transform_context_from_parser(ctxt);
    if tctxt.is_null() || (*tctxt).inst.is_null() {
        xslt_error(
            transform_context_from_parser(ctxt),
            b"element-available() : internal error tctxt == NULL\n\0",
        );
        crate::abi::exports_xml2::xmlXPathFreeObject(obj);
        value_push(pc, crate::abi::exports_xml2::xmlXPathNewBoolean(0));
        return;
    }

    let mut name: *mut xmlChar = ptr::null_mut();
    let mut ns_uri: *const xmlChar = ptr::null();
    let mut prefix: *mut xmlChar = ptr::null_mut();
    name = crate::xml::string::split_qname2((*obj).stringval, &mut prefix);
    if name.is_null() {
        name = crate::abi::exports_xml2::xmlStrdup((*obj).stringval);
        let ns =
            crate::abi::exports_xml2::xmlSearchNs((*(*tctxt).inst).doc, (*tctxt).inst, ptr::null());
        if !ns.is_null() {
            ns_uri = (*ns).href;
        }
    } else {
        let xpctxt = (*pc).context;
        ns_uri = if xpctxt.is_null() {
            ptr::null()
        } else {
            crate::xml::xpath::exports::xmlXPathNsLookup(xpctxt, prefix)
        };
        if ns_uri.is_null() {
            xslt_error(tctxt, b"element-available() : prefix %s is not bound\n\0");
        }
    }

    if !xslt_ext_element_lookup(tctxt, name, ns_uri).is_null() {
        value_push(pc, crate::abi::exports_xml2::xmlXPathNewBoolean(1));
    } else {
        value_push(pc, crate::abi::exports_xml2::xmlXPathNewBoolean(0));
    }

    crate::abi::exports_xml2::xmlXPathFreeObject(obj);
    if !name.is_null() {
        xmlFreeImpl(name as *mut c_void);
    }
    if !prefix.is_null() {
        xmlFreeImpl(prefix as *mut c_void);
    }
}

/// Implement the XSLT `function-available()` function:
/// `boolean function-available(string)`.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltFunctionAvailableFunction(xmlXPathParserContextPtr ctxt, int nargs);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be a valid parser context.
#[no_mangle]
pub unsafe extern "C" fn xsltFunctionAvailableFunction(ctxt: *mut c_void, nargs: c_int) {
    let pc = ctxt as *mut XmlXPathParserContext;
    if pc.is_null() {
        return;
    }
    let mut obj: *mut _xmlXPathObject = ptr::null_mut();

    if nargs != 1 {
        xslt_error(
            transform_context_from_parser(ctxt),
            b"function-available() : expects one string arg\n\0",
        );
        (*pc).error = XPATH_INVALID_ARITY as c_int;
        return;
    }
    crate::xml::xpath::exports::xmlXPathStringFunction(ctxt, 1);
    if (*pc).value.is_null() || (*(*pc).value).type_ != xmlXPathObjectType::XPATH_STRING as c_int {
        xslt_error(
            transform_context_from_parser(ctxt),
            b"function-available() : invalid arg expecting a string\n\0",
        );
        (*pc).error = XPATH_INVALID_TYPE as c_int;
        return;
    }
    obj = value_pop(pc);

    let mut name: *mut xmlChar = ptr::null_mut();
    let mut ns_uri: *const xmlChar = ptr::null();
    let mut prefix: *mut xmlChar = ptr::null_mut();
    name = crate::xml::string::split_qname2((*obj).stringval, &mut prefix);
    if name.is_null() {
        name = crate::abi::exports_xml2::xmlStrdup((*obj).stringval);
    } else {
        let xpctxt = (*pc).context;
        ns_uri = if xpctxt.is_null() {
            ptr::null()
        } else {
            crate::xml::xpath::exports::xmlXPathNsLookup(xpctxt, prefix)
        };
        if ns_uri.is_null() {
            xslt_error(
                transform_context_from_parser(ctxt),
                b"function-available() : prefix %s is not bound\n\0",
            );
        }
    }

    let xpctxt = (*pc).context;
    if !xpctxt.is_null()
        && !crate::xml::xpath::exports::xmlXPathFunctionLookupNS(xpctxt, name, ns_uri).is_none()
    {
        value_push(pc, crate::abi::exports_xml2::xmlXPathNewBoolean(1));
    } else {
        value_push(pc, crate::abi::exports_xml2::xmlXPathNewBoolean(0));
    }

    crate::abi::exports_xml2::xmlXPathFreeObject(obj);
    if !name.is_null() {
        xmlFreeImpl(name as *mut c_void);
    }
    if !prefix.is_null() {
        xmlFreeImpl(prefix as *mut c_void);
    }
}

/// Upstream `xsltCurrentFunction` (functions.c, static): the XSLT `current()`
/// function, registered by `xsltRegisterAllFunctions`.
unsafe extern "C" fn xslt_current_function(ctxt: *mut c_void, nargs: c_int) {
    let pc = ctxt as *mut XmlXPathParserContext;
    if pc.is_null() {
        return;
    }
    if nargs != 0 {
        xslt_error(
            transform_context_from_parser(ctxt),
            b"current() : function uses no argument\n\0",
        );
        (*pc).error = XPATH_INVALID_ARITY as c_int;
        return;
    }
    let tctxt = transform_context_from_parser(ctxt);
    if tctxt.is_null() {
        xslt_error(
            transform_context_from_parser(ctxt),
            b"current() : internal error tctxt == NULL\n\0",
        );
        value_push(
            pc,
            crate::abi::exports_xml2::xmlXPathNewNodeSet(ptr::null_mut()),
        );
    } else {
        value_push(
            pc,
            crate::abi::exports_xml2::xmlXPathNewNodeSet((*tctxt).node),
        ); // current
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 2. node-set() extension (extra.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// Implement the `node-set()` XSLT extension function (libxslt, saxon and xt
/// namespaces): convert a result tree fragment to a node-set.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltFunctionNodeSet(xmlXPathParserContextPtr ctxt, int nargs);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be a valid parser context.
#[no_mangle]
pub unsafe extern "C" fn xsltFunctionNodeSet(ctxt: *mut c_void, nargs: c_int) {
    let pc = ctxt as *mut XmlXPathParserContext;
    if pc.is_null() {
        return;
    }
    if nargs != 1 {
        xslt_error(
            transform_context_from_parser(ctxt),
            b"node-set() : expects one result-tree arg\n\0",
        );
        (*pc).error = XPATH_INVALID_ARITY as c_int;
        return;
    }
    if (*pc).value.is_null()
        || ((*(*pc).value).type_ != xmlXPathObjectType::XPATH_XSLT_TREE as c_int
            && (*(*pc).value).type_ != xmlXPathObjectType::XPATH_NODESET as c_int)
    {
        xslt_error(
            transform_context_from_parser(ctxt),
            b"node-set() invalid arg expecting a result tree\n\0",
        );
        (*pc).error = XPATH_INVALID_TYPE as c_int;
        return;
    }
    if (*(*pc).value).type_ == xmlXPathObjectType::XPATH_XSLT_TREE as c_int {
        (*(*pc).value).type_ = xmlXPathObjectType::XPATH_NODESET as c_int;
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 3. XPath evaluation utilities (templates.c / xsltutils.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// Mirror the C-level XPath context state into the internal Rust evaluator
/// context (which is what the evaluator reads via `extra`), and apply the
/// temporary namespace list.
///
/// Returns the previous internal namespace map (to restore later).
unsafe fn mirror_context_for_eval(
    ctxt: *mut _xsltTransformContext,
    ns_list: *mut *mut _xmlNs,
    ns_nr: c_int,
) -> std::collections::HashMap<String, String> {
    let xpath_ctxt = (*ctxt).xpathCtxt;
    let internal = internal_xpath_ctxt(xpath_ctxt);
    let mut saved: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    if internal.is_null() {
        return saved;
    }
    unsafe {
        let node = (*ctxt).node;
        (*internal).context_node = node;
        if !node.is_null() && !(*node).doc.is_null() {
            (*internal).document = (*node).doc;
        }
        (*internal).context_size = (*xpath_ctxt).contextSize;
        (*internal).context_position = (*xpath_ctxt).proximityPosition;
        (*internal).proximity_position = (*xpath_ctxt).proximityPosition;
        if !(*internal).context_list.is_empty() {
            // keep the existing context list; position/size are mirrored above
        }
        // Temporarily replace the namespace map with the nsList contents.
        saved = std::mem::take(&mut (*internal).namespaces);
        if !ns_list.is_null() {
            let mut i: c_int = 0;
            while i < ns_nr {
                let ns = *ns_list.offset(i as isize);
                if !ns.is_null() {
                    let p = (*ns).prefix;
                    let h = (*ns).href;
                    if !p.is_null() && !h.is_null() {
                        let prefix_str = CStr::from_ptr(p as *const c_char)
                            .to_string_lossy()
                            .into_owned();
                        let href_str = CStr::from_ptr(h as *const c_char)
                            .to_string_lossy()
                            .into_owned();
                        (*internal).namespaces.insert(prefix_str, href_str);
                    }
                }
                i += 1;
            }
        }
    }
    saved
}

/// Evaluate a compiled XPath expression as a predicate on `ctxt->node`.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltEvalXPathPredicate(xsltTransformContextPtr ctxt,
///                            xmlXPathCompExprPtr comp,
///                            xmlNsPtr *nsList, int nsNr);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be a valid transform context; `comp` a compiled expression
///   or NULL.
#[no_mangle]
pub unsafe extern "C" fn xsltEvalXPathPredicate(
    ctxt: *mut _xsltTransformContext,
    comp: *mut c_void,
    nsList: *mut *mut _xmlNs,
    nsNr: c_int,
) -> c_int {
    if ctxt.is_null() || (*ctxt).inst.is_null() {
        xslt_error(
            ctxt,
            b"xsltEvalXPathPredicate: No context or instruction\n\0",
        );
        return 0;
    }
    let xpath_ctxt = (*ctxt).xpathCtxt;
    if xpath_ctxt.is_null() {
        return 0;
    }

    let old_node = (*xpath_ctxt).node;
    let old_context_size = (*xpath_ctxt).contextSize;
    let old_proximity_position = (*xpath_ctxt).proximityPosition;
    let old_ns_nr = (*xpath_ctxt).nsNr;
    let old_namespaces = (*xpath_ctxt).namespaces;
    let old_inst = (*ctxt).inst;

    (*xpath_ctxt).node = (*ctxt).node;
    (*xpath_ctxt).namespaces = nsList;
    (*xpath_ctxt).nsNr = nsNr;
    let saved_ns = mirror_context_for_eval(ctxt, nsList, nsNr);

    let res = crate::xml::xpath::exports::xmlXPathCompiledEval(comp, xpath_ctxt);

    let ret: c_int;
    if !res.is_null() {
        ret = crate::xml::xpath::exports::xmlXPathEvalPredicate(xpath_ctxt, res);
        crate::abi::exports_xml2::xmlXPathFreeObject(res);
    } else {
        (*ctxt).state = XSLT_STATE_STOPPED;
        ret = 0;
    }

    (*xpath_ctxt).node = old_node;
    (*xpath_ctxt).nsNr = old_ns_nr;
    (*xpath_ctxt).namespaces = old_namespaces;
    (*ctxt).inst = old_inst;
    (*xpath_ctxt).contextSize = old_context_size;
    (*xpath_ctxt).proximityPosition = old_proximity_position;
    let internal = internal_xpath_ctxt(xpath_ctxt);
    if !internal.is_null() {
        (*internal).namespaces = saved_ns;
        (*internal).context_node = old_node;
        (*internal).context_size = old_context_size;
        (*internal).context_position = old_proximity_position;
        (*internal).proximity_position = old_proximity_position;
    }

    ret
}

/// Evaluate a compiled XPath expression with an explicit namespace mapping
/// and return its string value.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar * xsltEvalXPathStringNs(xsltTransformContextPtr ctxt,
///                                 xmlXPathCompExprPtr comp,
///                                 int nsNr, xmlNsPtr *nsList);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be a valid transform context; `comp` a compiled expression
///   or NULL. The returned string is xmlMalloc'd and must be freed by the
///   caller with `xmlFree`.
#[no_mangle]
pub unsafe extern "C" fn xsltEvalXPathStringNs(
    ctxt: *mut _xsltTransformContext,
    comp: *mut c_void,
    nsNr: c_int,
    nsList: *mut *mut _xmlNs,
) -> *mut xmlChar {
    if ctxt.is_null() || (*ctxt).inst.is_null() {
        xslt_error(
            ctxt,
            b"xsltEvalXPathStringNs: No context or instruction\n\0",
        );
        return ptr::null_mut();
    }
    let xpath_ctxt = (*ctxt).xpathCtxt;
    if xpath_ctxt.is_null() {
        return ptr::null_mut();
    }

    let old_inst = (*ctxt).inst;
    let old_node = (*xpath_ctxt).node;
    let old_pos = (*xpath_ctxt).proximityPosition;
    let old_size = (*xpath_ctxt).contextSize;
    let old_ns_nr = (*xpath_ctxt).nsNr;
    let old_namespaces = (*xpath_ctxt).namespaces;

    (*xpath_ctxt).node = (*ctxt).node;
    (*xpath_ctxt).namespaces = nsList;
    (*xpath_ctxt).nsNr = nsNr;
    let saved_ns = mirror_context_for_eval(ctxt, nsList, nsNr);

    let mut ret: *mut xmlChar = ptr::null_mut();
    let mut res = crate::xml::xpath::exports::xmlXPathCompiledEval(comp, xpath_ctxt);
    if !res.is_null() {
        if (*res).type_ != xmlXPathObjectType::XPATH_STRING as c_int {
            res = crate::xml::xpath::exports::xmlXPathConvertString(res);
        }
        if !res.is_null() && (*res).type_ == xmlXPathObjectType::XPATH_STRING as c_int {
            ret = (*res).stringval;
            (*res).stringval = ptr::null_mut();
        } else {
            xslt_error(
                ctxt,
                b"xpath : string() function didn't return a String\n\0",
            );
        }
        crate::abi::exports_xml2::xmlXPathFreeObject(res);
    } else {
        (*ctxt).state = XSLT_STATE_STOPPED;
    }

    (*ctxt).inst = old_inst;
    (*xpath_ctxt).node = old_node;
    (*xpath_ctxt).contextSize = old_size;
    (*xpath_ctxt).proximityPosition = old_pos;
    (*xpath_ctxt).nsNr = old_ns_nr;
    (*xpath_ctxt).namespaces = old_namespaces;
    let internal = internal_xpath_ctxt(xpath_ctxt);
    if !internal.is_null() {
        (*internal).namespaces = saved_ns;
        (*internal).context_node = old_node;
        (*internal).context_size = old_size;
        (*internal).context_position = old_pos;
        (*internal).proximity_position = old_pos;
    }

    ret
}

/// Evaluate a compiled XPath expression and return its string value.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar * xsltEvalXPathString(xsltTransformContextPtr ctxt,
///                               xmlXPathCompExprPtr comp);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be a valid transform context; `comp` a compiled expression
///   or NULL. The returned string is xmlMalloc'd and must be freed by the
///   caller with `xmlFree`.
#[no_mangle]
pub unsafe extern "C" fn xsltEvalXPathString(
    ctxt: *mut _xsltTransformContext,
    comp: *mut c_void,
) -> *mut xmlChar {
    unsafe { xsltEvalXPathStringNs(ctxt, comp, 0, ptr::null_mut()) }
}

/// Compile an XPath expression with extra compilation flags, using the
/// stylesheet's XPath context (or a fresh one when `style` is NULL).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlXPathCompExprPtr xsltXPathCompileFlags(xsltStylesheetPtr style,
///                                           const xmlChar *str, int flags);
/// ```
///
/// # SAFETY
///
/// - `style` must be a valid stylesheet or NULL; `str` a valid string or
///   NULL. The caller frees the result with `xmlXPathFreeCompExpr`.
#[no_mangle]
pub unsafe extern "C" fn xsltXPathCompileFlags(
    style: *mut _xsltStylesheet,
    str_: *const xmlChar,
    flags: c_int,
) -> *mut c_void {
    if str_.is_null() {
        return ptr::null_mut();
    }
    let xpath_ctxt: *mut _xmlXPathContext;
    let mut free_ctxt: bool = false;
    if !style.is_null() {
        // UPSTREAM-PARITY: upstream uses `style->principal->xpathCtxt`
        // (principal is the top-level stylesheet for imports).
        let principal = if (*style).principal.is_null() {
            style
        } else {
            (*style).principal
        };
        xpath_ctxt = (*principal).xpathCtxt;
        if xpath_ctxt.is_null() {
            return ptr::null_mut();
        }
        (*xpath_ctxt).dict = (*style).dict;
    } else {
        xpath_ctxt = crate::abi::exports_xml2::xmlXPathNewContext(ptr::null_mut());
        if xpath_ctxt.is_null() {
            return ptr::null_mut();
        }
        free_ctxt = true;
    }
    (*xpath_ctxt).flags = flags;

    let ret = crate::xml::xpath::exports::xmlXPathCtxtCompile(xpath_ctxt, str_);

    if free_ctxt {
        crate::abi::exports_xml2::xmlXPathFreeContext(xpath_ctxt);
    }
    ret
}

/// Compile an XPath expression.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlXPathCompExprPtr xsltXPathCompile(xsltStylesheetPtr style,
///                                      const xmlChar *str);
/// ```
///
/// # SAFETY
///
/// - `style` must be a valid stylesheet or NULL; `str` a valid string or
///   NULL. The caller frees the result with `xmlXPathFreeCompExpr`.
#[no_mangle]
pub unsafe extern "C" fn xsltXPathCompile(
    style: *mut _xsltStylesheet,
    str_: *const xmlChar,
) -> *mut c_void {
    unsafe { xsltXPathCompileFlags(style, str_, 0) }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 4. XPath-context hooks (functions.c / variables.c / extensions.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// Look up an XPath extension function for the XPath interpreter
/// (registered as the context's `funcLookupFunc`; `vctxt` is the XPath
/// context).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlXPathFunction xsltXPathFunctionLookup(void *vctxt,
///                                          const xmlChar *name,
///                                          const xmlChar *ns_uri);
/// ```
///
/// # SAFETY
///
/// - `vctxt` must be a valid XPath context; `name`/`ns_uri` valid strings.
#[no_mangle]
pub unsafe extern "C" fn xsltXPathFunctionLookup(
    vctxt: *mut c_void,
    name: *const xmlChar,
    ns_uri: *const xmlChar,
) -> Option<unsafe extern "C" fn(*mut c_void, c_int)> {
    if vctxt.is_null() || name.is_null() || ns_uri.is_null() {
        return None;
    }
    let xpath_ctxt = vctxt as *mut _xmlXPathContext;
    // Give priority to context-level functions (upstream
    // xmlHashLookup2(ctxt->funcHash, name, ns_uri)).
    let name_str = match CStr::from_ptr(name as *const c_char).to_str() {
        Ok(s) => s,
        Err(_) => return None,
    };
    let qualified = if ns_uri.is_null() {
        name_str.to_string()
    } else {
        let ns = match CStr::from_ptr(ns_uri as *const c_char).to_str() {
            Ok(s) => s,
            Err(_) => return None,
        };
        format!("{{{}}}{}", ns, name_str)
    };
    if let Some(f) = crate::abi::exports_xml2::xpath_cfunc_lookup((*xpath_ctxt).extra, &qualified) {
        return Some(f);
    }
    // Module registry fallback (upstream xsltExtModuleFunctionLookup).
    // UPSTREAM-PARITY (simplified): the candidate's module-level registry
    // (crate::exslt) holds Rust closures with the internal XPathFunction
    // signature, which cannot be exposed through the C `xmlXPathFunction`
    // pointer type; nothing else registers module functions.
    None
}

/// Look up a global XSLT variable in the internal XPath context's variable
/// hash (the candidate keeps globals there — see xsltInitGlobalVariables).
unsafe fn internal_global_variable_lookup(
    tctxt: *mut _xsltTransformContext,
    name: *const xmlChar,
) -> *mut _xmlXPathObject {
    if tctxt.is_null() || name.is_null() || (*tctxt).xpathCtxt.is_null() {
        return ptr::null_mut();
    }
    let internal = internal_xpath_ctxt((*tctxt).xpathCtxt);
    if internal.is_null() {
        return ptr::null_mut();
    }
    let name_str = match CStr::from_ptr(name as *const c_char).to_str() {
        Ok(s) => s,
        Err(_) => return ptr::null_mut(),
    };
    match (*internal).variables.get(name_str) {
        Some(v) => crate::abi::exports_xml2::xpath_to_object_pub(v.clone()),
        None => ptr::null_mut(),
    }
}

/// Look up a variable for the XPath interpreter (registered as the
/// context's `varLookupFunc`; `ctxt` is the transform context).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlXPathObjectPtr xsltXPathVariableLookup(void *ctxt,
///                                           const xmlChar *name,
///                                           const xmlChar *ns_uri);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be a valid transform context; `name`/`ns_uri` valid
///   strings. The returned object is owned by the caller.
#[no_mangle]
pub unsafe extern "C" fn xsltXPathVariableLookup(
    ctxt: *mut c_void,
    name: *const xmlChar,
    ns_uri: *const xmlChar,
) -> *mut _xmlXPathObject {
    if ctxt.is_null() || name.is_null() {
        return ptr::null_mut();
    }
    let tctxt = ctxt as *mut _xsltTransformContext;
    let mut value_obj: *mut _xmlXPathObject = ptr::null_mut();

    // Local variables/params: lookup from the top of the stack.
    if (*tctxt).varsNr != 0 {
        let variable = crate::xslt::variables::xsltLookupVariable(tctxt, name, ns_uri);
        if !variable.is_null() {
            if (*variable).computed == 0 {
                // UPSTREAM-PARITY: upstream computes lazily and stores the
                // value on the stack element; the candidate computes
                // variables eagerly at push time, so xsltEvalVariable
                // returns a copy of the stored value.
                let v = crate::xslt::variables::xsltEvalVariable(tctxt, variable);
                (*variable).computed = 1;
                if !v.is_null() {
                    value_obj = v;
                }
            } else if !(*variable).value.is_null() {
                value_obj = crate::abi::exports_xml2::xmlXPathObjectCopy((*variable).value);
            }
            if !value_obj.is_null() {
                return value_obj;
            }
        }
    }

    // Global variables/params.
    if value_obj.is_null() {
        value_obj = internal_global_variable_lookup(tctxt, name);
    }

    if value_obj.is_null() {
        // UPSTREAM-PARITY: upstream reports the undeclared variable via
        // xsltTransformError (the format arguments are not expanded in the
        // candidate's safe error subset).
        if !ns_uri.is_null() {
            xslt_error(tctxt, b"Variable '{%s}%s' has not been declared.\n\0");
        } else {
            xslt_error(tctxt, b"Variable '%s' has not been declared.\n\0");
        }
    }
    value_obj
}

/// Provide the XSLT transformation context from the XPath parser context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xsltTransformContextPtr xsltXPathGetTransformContext(xmlXPathParserContextPtr ctxt);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be a valid parser context or NULL.
#[no_mangle]
pub unsafe extern "C" fn xsltXPathGetTransformContext(
    ctxt: *mut c_void,
) -> *mut _xsltTransformContext {
    unsafe { transform_context_from_parser(ctxt) }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 5. Registration (functions.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// Register all default XSLT functions in an XPath context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltRegisterAllFunctions(xmlXPathContextPtr ctxt);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be a valid XPath context.
#[no_mangle]
pub unsafe extern "C" fn xsltRegisterAllFunctions(ctxt: *mut _xmlXPathContext) {
    if ctxt.is_null() {
        return;
    }
    // Order matters: register the C-level functions first (they install a
    // stub in the internal evaluator context), then re-register the
    // internal Rust implementations so the native evaluator keeps its full
    // implementations (the stubs cannot be invoked from the Rust engine).
    crate::abi::exports_xml2::xmlXPathRegisterFunc(
        ctxt,
        c"current".as_ptr() as *const xmlChar,
        Some(xslt_current_function),
    );
    crate::abi::exports_xml2::xmlXPathRegisterFunc(
        ctxt,
        c"document".as_ptr() as *const xmlChar,
        Some(xsltDocumentFunction),
    );
    crate::abi::exports_xml2::xmlXPathRegisterFunc(
        ctxt,
        c"key".as_ptr() as *const xmlChar,
        Some(xsltKeyFunction),
    );
    crate::abi::exports_xml2::xmlXPathRegisterFunc(
        ctxt,
        c"unparsed-entity-uri".as_ptr() as *const xmlChar,
        Some(xsltUnparsedEntityURIFunction),
    );
    crate::abi::exports_xml2::xmlXPathRegisterFunc(
        ctxt,
        c"format-number".as_ptr() as *const xmlChar,
        Some(xsltFormatNumberFunction),
    );
    crate::abi::exports_xml2::xmlXPathRegisterFunc(
        ctxt,
        c"generate-id".as_ptr() as *const xmlChar,
        Some(xsltGenerateIdFunction),
    );
    crate::abi::exports_xml2::xmlXPathRegisterFunc(
        ctxt,
        c"system-property".as_ptr() as *const xmlChar,
        Some(xsltSystemPropertyFunction),
    );
    crate::abi::exports_xml2::xmlXPathRegisterFunc(
        ctxt,
        c"element-available".as_ptr() as *const xmlChar,
        Some(xsltElementAvailableFunction),
    );
    crate::abi::exports_xml2::xmlXPathRegisterFunc(
        ctxt,
        c"function-available".as_ptr() as *const xmlChar,
        Some(xsltFunctionAvailableFunction),
    );

    // Re-register the internal Rust implementations on the transform's
    // XPath context (candidate wiring — see register_xslt_functions in
    // src/xslt/transform/mod.rs).
    let internal = internal_xpath_ctxt(ctxt);
    if !internal.is_null() {
        let tctxt = (*internal).func_lookup_data as *mut _xsltTransformContext;
        if !tctxt.is_null() {
            crate::xslt::transform::register_xslt_functions(tctxt);
        }
    }
}
