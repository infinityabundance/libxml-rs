//! exports_xslt_avt — attribute value templates / namespace fixup / utility
//! family closure.
//!
//! C ABI exports for the libxslt "avt" family: `xsltAttrListTemplateProcess`,
//! `xsltAttrTemplateProcess`, `xsltAttrTemplateValueProcess[Node]`,
//! `xsltEvalAVT`, `xsltEvalAttrValueTemplate`, `xsltEvalStaticAttrValueTemplate`,
//! `xsltEvalTemplateString`, `xsltFreeAVTList`, `xsltGetCNsProp`,
//! `xsltGetNsProp`, `xsltGetNamespace`, `xsltGetPlainNamespace`,
//! `xsltGetSpecialNamespace`, `xsltCopyNamespace`, `xsltCopyNamespaceList`,
//! `xsltCopyTextString`, `xsltNamespaceAlias`, `xsltFreeNamespaceAliasHashes`,
//! `xsltIsBlank`, `xsltSplitQName`, `xsltGetQNameURI`, `xsltGetQNameURI2`,
//! `xsltGetUTF8Char`.
//!
//! # UPSTREAM-PARITY
//!
//! All semantics follow upstream libxslt 1.1.45 (`archaeology/libxslt-git`):
//! `templates.c`, `attrvt.c`, `namespaces.c`, `transform.c`, `xsltutils.c`
//! and `xslt.c`. The engine-level wiring notes (where the native-Rust engine
//! replaces an upstream data structure) are documented per function.
//!
//! # Upstream contract
//!
//! Parity target is upstream libxslt 1.1.45 (`templates.c`, `attrvt.c`,
//! `namespaces.c`, `transform.c`, `xsltutils.c`, `xslt.c`) with the upstream
//! headers; R-000160 (11.1-I) dispositioned `xsltFreeAVTList` (empty body in
//! the candidate because AVTs are stored as raw strings).
//!
//! # Conceptual behavior
//!
//! This module implements the attribute-value-template ABI: the
//! `xsltAttr*TemplateProcess*` family, `xsltEvalAVT` and the
//! `xsltEval*ValueTemplate` entry points, namespace fixup (`xsltGetNamespace`,
//! `xsltGetSpecialNamespace`, `xsltCopyNamespace*`), `xsltSplitQName`/
//! `xsltGetQNameURI` and `xsltGetUTF8Char`.
//!
//! # Ownership & safety invariants
//!
//! `xsltEvalAVT`/`xsltAttrTemplateValueProcess` return fresh xml-allocator
//! strings the caller frees with `xmlFree` (OWNERSHIP_ATLAS section 3);
//! `xsltSplitQName` returns dict-interned borrowed strings; `xsltGetSpecialNamespace`
//! returns a namespace owned by the nodes ns list (borrowed — never freed
//! separately); `xsltCopyNamespace` returns a fresh copy the caller frees with
//! `xmlFreeNs`.
//!
//! # Historical quirks & epochs
//!
//! The AVT machinery dates to the libxslt 1.1 era (2004+, HISTORY.md 2.5) and
//! is covered by the frozen E-008 transform epoch; R-000160 records that
//! `xsltFreeAVTList` is a literal empty body in upstreams own
//! caller-visible behavior for this engine (the candidate stores AVTs as
//! plain strings owned by the stylesheet doc).
//!
//! # Deliberate oddities
//!
//! `xsltFreeAVTList` is a no-op that accepts any pointer without dereferencing
//! it (documented above) — a deliberate safe no-op because this engine never
//! allocates `xsltAttrVT` structures.
//!
//! # Proving courts
//!
//! The CLI-XSLTPROC court cases and the XSLT court family cover the AVT
//! evaluation paths; DSO-LOADER and HEADER-COMPILE resolve
//! the exports; the avt/namespace unit tests run under cargo test.
//!
//! # Tempting simplifications that would break parity
//!
//! A tempting simplification is to have `xsltFreeAVTList` free its argument
//! when non-NULL to be defensive — the argument cannot belong to this engine,
//! so freeing it would corrupt foreign memory; the no-op must stay. Another
//! shortcut, returning owned Rust strings from `xsltSplitQName`, would break
//! the dict-borrowed contract downstream code relies on.

#![allow(non_snake_case)]
#![allow(unused_variables)]
#![allow(clippy::missing_safety_doc)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]
#![allow(clippy::comparison_chain)]
// Ported goto-heavy C (namespaces.c) writes state variables before
// re-reading them; the dead first assignments mirror upstream exactly.
#![allow(unused_assignments)]

// SAFETY-SCOPE: EXPORT-XSLT_AVT-MECHANICAL-001
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

use crate::abi::allocator::*;
use crate::abi::exports_hash::xmlDictOwns;
use crate::abi::exports_tree::{xmlNewDocProp, xmlNewNsProp, xmlNewTextLen};
use crate::abi::exports_treedump::xmlNodeListGetString;
use crate::abi::exports_xml2::*;
use crate::abi::structs::*;
use crate::abi::types::xmlElementType::*;
use crate::abi::types::*;

/// The XSLT namespace URI (NUL-terminated; upstream `XSLT_NAMESPACE`).
const XSLT_NAMESPACE: &[u8] = b"http://www.w3.org/1999/XSL/Transform\0";

/// `xsltOutputType::XSLT_OUTPUT_XML` (upstream xsltInternals.h: the enum
/// starts at 0).
const XSLT_OUTPUT_XML: c_int = 0;

/// `UNDEFINED_DEFAULT_NS` — upstream namespaces.h defines the sentinel as
/// `(const xmlChar *) -1L`, stored in the nsAliases table when
/// `result-prefix="#default"` is used without a default namespace in scope.
const UNDEFINED_DEFAULT_NS: *const xmlChar = -1isize as *const xmlChar;

/// Heap-storable representation of `UNDEFINED_DEFAULT_NS`.
///
/// The engine keeps `style->nsAliases` as a linked list of `_xsltNsAlias`
/// whose `resultNs`/`styleNs` are heap strings freed by `xsltFreeNsAlias`;
/// the raw sentinel pointer is not freeable. `0xFF` bytes cannot occur in a
/// namespace URI, so this marker is unambiguous. `ns_uri_is_undefined()`
/// accepts both representations.
const UNDEFINED_DEFAULT_NS_MARKER: &[u8] = &[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];

/// Duplicate a byte slice into a heap NUL-terminated string.
///
/// # SAFETY
///
/// - The caller owns the returned allocation (free with `xmlFree`).
unsafe fn alloc_str(bytes: &[u8]) -> *mut xmlChar {
    let p = xmlMallocImpl(bytes.len() + 1) as *mut xmlChar;
    if p.is_null() {
        return ptr::null_mut();
    }
    ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
    *p.add(bytes.len()) = 0;
    p
}

/// Is this the "undefined default namespace" marker (either the upstream
/// raw sentinel or the heap marker used by this engine)?
///
/// # SAFETY
///
/// - `uri` must be NULL, the upstream sentinel, or a valid NUL-terminated
///   string.
unsafe fn ns_uri_is_undefined(uri: *const xmlChar) -> bool {
    if uri == UNDEFINED_DEFAULT_NS {
        return true;
    }
    if uri.is_null() {
        return false;
    }
    let len = libc::strlen(uri as *const c_char);
    len == UNDEFINED_DEFAULT_NS_MARKER.len() as libc::size_t
        && core::slice::from_raw_parts(uri, UNDEFINED_DEFAULT_NS_MARKER.len())
            == UNDEFINED_DEFAULT_NS_MARKER
}

/// Upstream `xmlHashLookup(style->nsAliases, href)`.
///
/// The engine stores the alias table as a linked list of `_xsltNsAlias`
/// (see `crate::xslt::namespace_alias`), so the hash lookup is walked
/// linearly. Returns the result-side URI (possibly the undefined-default
/// marker) or NULL when no alias matches.
///
/// # SAFETY
///
/// - `style` must be a valid `_xsltStylesheet` or NULL.
/// - `href` must be a valid NUL-terminated string or NULL.
unsafe fn lookup_ns_alias(style: *mut _xsltStylesheet, href: *const xmlChar) -> *const xmlChar {
    if style.is_null() || href.is_null() {
        return ptr::null();
    }
    let mut cur = (*style).nsAliases as *mut _xsltNsAlias;
    while !cur.is_null() {
        if !(*cur).styleNs.is_null()
            && libc::strcmp((*cur).styleNs as *const c_char, href as *const c_char) == 0
        {
            return (*cur).resultNs;
        }
        cur = (*cur).next;
    }
    ptr::null()
}

/// Upstream `xsltNextImport()` (imports.c): the next stylesheet in import
/// precedence order.
///
/// # SAFETY
///
/// - `cur` must be a valid `_xsltStylesheet` or NULL.
unsafe fn xslt_next_import(cur: *mut _xsltStylesheet) -> *mut _xsltStylesheet {
    if cur.is_null() {
        return ptr::null_mut();
    }
    if !(*cur).imports.is_null() {
        return (*cur).imports;
    }
    if !(*cur).next.is_null() {
        return (*cur).next;
    }
    let mut c = cur;
    loop {
        c = (*c).parent;
        if c.is_null() {
            break;
        }
        if !(*c).next.is_null() {
            return (*c).next;
        }
    }
    c
}

/// `xmlDictLookup(dict, "", 0)` — the dictionary-interned empty string.
///
/// The engine's `dict_lookup` returns NULL for `len == 0` (engine
/// deviation); the fallback literal is then borrowed and the
/// `xmlDictOwns` fast-path simply misses, producing an allocated copy of
/// `""` — the same observable value upstream produces.
///
/// # SAFETY
///
/// - `dict` must be a valid dictionary pointer or NULL.
unsafe fn dict_empty(dict: *mut c_void) -> *const xmlChar {
    let e = xmlDictLookup(dict, c"".as_ptr() as *const xmlChar, 0);
    if e.is_null() {
        c"".as_ptr() as *const xmlChar
    } else {
        e
    }
}

/// Create a text node for an attribute value and set its content,
/// mirroring the `xmlNewText(NULL)` + content-assignment pattern of the
/// upstream attribute-template functions. The engine's `new_text(NULL)`
/// allocates an empty string which is freed before the real content is
/// installed (no leak).
///
/// Returns the text node or NULL.
///
/// # SAFETY
///
/// - All pointers must be valid or NULL as noted.
unsafe fn new_attr_text_node(attr: *mut _xmlAttr, content: *mut xmlChar) -> *mut _xmlNode {
    let text = crate::xml::tree::new_text(ptr::null());
    if text.is_null() {
        return ptr::null_mut();
    }
    if !(*text).content.is_null() {
        xmlFreeImpl((*text).content as *mut c_void);
    }
    (*text).content = content;
    (*attr).last = text;
    (*attr).children = text;
    (*text).parent = attr as *mut _xmlNode;
    (*text).doc = (*attr).doc;
    text
}

// ═══════════════════════════════════════════════════════════════════════════════
// Attribute value templates (templates.c / attrvt.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// Process all attributes of a Literal Result Element.
///
/// Copies all non-XSLT attributes over to the `target` element, evaluating
/// attribute value templates, and applies `xsl:use-attribute-sets` first.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlAttrPtr xsltAttrListTemplateProcess(xsltTransformContextPtr ctxt,
///                                        xmlNodePtr target, xmlAttrPtr attrs);
/// ```
///
/// Returns a new list of attribute nodes, or NULL in case of error.
///
/// # SAFETY
///
/// - `ctxt`, `target`, `attrs` must be valid pointers (or NULL where
///   permitted); `target` must be an element node.
#[no_mangle]
pub unsafe extern "C" fn xsltAttrListTemplateProcess(
    ctxt: *mut _xsltTransformContext,
    target: *mut _xmlNode,
    attrs: *mut _xmlAttr,
) -> *mut _xmlAttr {
    if ctxt.is_null() || target.is_null() || attrs.is_null() {
        return ptr::null_mut();
    }
    if (*target).type_ != XML_ELEMENT_NODE as c_int {
        return ptr::null_mut();
    }

    let old_insert = (*ctxt).insert;
    (*ctxt).insert = target;

    // Apply attribute sets (upstream: xsl:use-attribute-sets on the LRE).
    // Wired to the engine's attribute-set machinery.
    let mut attr = attrs;
    loop {
        if !(*attr).ns.is_null()
            && xmlStrEqual(
                (*attr).name,
                c"use-attribute-sets".as_ptr() as *const xmlChar,
            ) != 0
            && xmlStrEqual(
                (*(*attr).ns).href,
                XSLT_NAMESPACE.as_ptr() as *const xmlChar,
            ) != 0
        {
            let value = crate::xml::tree::node_get_content((*attr).children);
            if !value.is_null() {
                crate::xslt::attributes::xsltApplyAttrSets(ctxt, target, value);
                xmlFreeImpl(value as *mut c_void);
            }
        }
        attr = (*attr).next;
        if attr.is_null() {
            break;
        }
    }

    let mut has_attr = 0;
    if !(*target).properties.is_null() {
        has_attr = 1;
    }

    // Instantiate the LRE attributes.
    let mut attr = attrs;
    let mut last: *mut _xmlAttr = ptr::null_mut();
    let mut orig_ns: *mut _xmlNs = ptr::null_mut();
    let mut copy_ns: *mut _xmlNs = ptr::null_mut();
    loop {
        // Skip XSLT attributes.
        if !(*attr).ns.is_null()
            && xmlStrEqual(
                (*(*attr).ns).href,
                XSLT_NAMESPACE.as_ptr() as *const xmlChar,
            ) != 0
        {
            attr = (*attr).next;
            if attr.is_null() {
                break;
            }
            continue;
        }
        // Get the value.
        let value: *const xmlChar;
        if !(*attr).children.is_null() {
            if (*(*attr).children).type_ != XML_TEXT_NODE as c_int
                || !(*(*attr).children).next.is_null()
            {
                crate::xslt::errors::xsltTransformError(
                    ctxt,
                    ptr::null_mut(),
                    (*attr).parent,
                    c"Internal error: The children of an attribute node of a literal result element are not in the expected form.\n"
                        .as_ptr() as *const c_char,
                );
                (*ctxt).insert = old_insert;
                return ptr::null_mut();
            }
            let c = (*(*attr).children).content;
            if c.is_null() {
                value = dict_empty((*ctxt).dict);
            } else {
                value = c;
            }
        } else {
            value = dict_empty((*ctxt).dict);
        }

        // Get the namespace. Avoid lookups of the same namespace.
        if (*attr).ns != orig_ns {
            orig_ns = (*attr).ns;
            if !(*attr).ns.is_null() {
                copy_ns = xsltGetNamespace(ctxt, (*attr).parent, (*attr).ns, target);
                if copy_ns.is_null() {
                    (*ctxt).insert = old_insert;
                    return ptr::null_mut();
                }
            } else {
                copy_ns = ptr::null_mut();
            }
        }

        // Create a new attribute.
        let copy: *mut _xmlAttr;
        if has_attr != 0 {
            copy = xmlSetNsProp(target, copy_ns, (*attr).name, ptr::null());
        } else {
            copy = xmlNewDocProp((*target).doc, (*attr).name, ptr::null());
            if !copy.is_null() {
                (*copy).ns = copy_ns;
                (*copy).parent = target;
                if last.is_null() {
                    (*target).properties = copy;
                    last = copy;
                } else {
                    (*last).next = copy;
                    (*copy).prev = last;
                    last = copy;
                }
            }
        }
        if copy.is_null() {
            crate::xslt::errors::xsltTransformError(
                ctxt,
                ptr::null_mut(),
                (*attr).parent,
                c"Internal error: Failed to create attribute.\n".as_ptr() as *const c_char,
            );
            (*ctxt).insert = old_insert;
            return ptr::null_mut();
        }

        // Set the value.

        let text: *mut _xmlNode = if (*attr).psvi.is_null() {
            // No precompiled AVT: copy the value verbatim (upstream keeps
            // the dictionary-owned string when internalized).

            let content: *mut xmlChar = if (*ctxt).internalized != 0
                && !(*target).doc.is_null()
                && (*(*target).doc).dict == (*ctxt).dict
                && !(*ctxt).dict.is_null()
                && xmlDictOwns((*ctxt).dict, value) != 0
            {
                value as *mut xmlChar
            } else {
                xmlStrdup(value)
            };
            new_attr_text_node(copy, content)
        } else {
            // Evaluate the precompiled AVT. In this engine the compiled
            // tree stores AVT strings verbatim (see xsltEvalAVT), so the
            // compiled form is a raw string.
            let value_avt = xsltEvalAVT(ctxt, (*attr).psvi, (*attr).parent);
            let content: *mut xmlChar = if value_avt.is_null() {
                xmlStrdup(c"".as_ptr() as *const xmlChar)
            } else {
                value_avt
            };
            new_attr_text_node(copy, content)
        };
        if !text.is_null() && xmlIsID((*copy).doc, (*copy).parent, copy) != 0 {
            xmlAddID(ptr::null_mut(), (*copy).doc, (*text).content, copy);
        }

        attr = (*attr).next;
        if attr.is_null() {
            break;
        }
    }

    (*ctxt).insert = old_insert;
    (*target).properties
}

/// Process one attribute of a Literal Result Element.
///
/// Evaluates attribute value templates and copies the attribute over to
/// the result element. Does *not* process attribute sets.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlAttrPtr xsltAttrTemplateProcess(xsltTransformContextPtr ctxt,
///                                    xmlNodePtr target, xmlAttrPtr attr);
/// ```
///
/// Returns the generated attribute node.
///
/// # SAFETY
///
/// - All pointers must be valid or NULL where permitted; `target` must be
///   an element node.
#[no_mangle]
pub unsafe extern "C" fn xsltAttrTemplateProcess(
    ctxt: *mut _xsltTransformContext,
    target: *mut _xmlNode,
    attr: *mut _xmlAttr,
) -> *mut _xmlAttr {
    if ctxt.is_null() || attr.is_null() || target.is_null() {
        return ptr::null_mut();
    }
    if (*target).type_ != XML_ELEMENT_NODE as c_int {
        return ptr::null_mut();
    }
    if (*attr).type_ != XML_ATTRIBUTE_NODE as c_int {
        return ptr::null_mut();
    }
    // Skip all XSLT attributes.
    if !(*attr).ns.is_null()
        && xmlStrEqual(
            (*(*attr).ns).href,
            XSLT_NAMESPACE.as_ptr() as *const xmlChar,
        ) != 0
    {
        return ptr::null_mut();
    }
    // Get the value.
    let value: *const xmlChar;
    if !(*attr).children.is_null() {
        if (*(*attr).children).type_ != XML_TEXT_NODE as c_int
            || !(*(*attr).children).next.is_null()
        {
            crate::xslt::errors::xsltTransformError(
                ctxt,
                ptr::null_mut(),
                (*attr).parent,
                c"Internal error: The children of an attribute node of a literal result element are not in the expected form.\n"
                    .as_ptr() as *const c_char,
            );
            return ptr::null_mut();
        }
        let c = (*(*attr).children).content;
        if c.is_null() {
            value = dict_empty((*ctxt).dict);
        } else {
            value = c;
        }
    } else {
        value = dict_empty((*ctxt).dict);
    }

    // Overwrite duplicates.
    let mut ret = (*target).properties;
    while !ret.is_null() {
        let same_ns = ((*attr).ns.is_null() && (*ret).ns.is_null())
            || (!(*attr).ns.is_null() && !(*ret).ns.is_null());
        if same_ns
            && xmlStrEqual((*ret).name, (*attr).name) != 0
            && ((*attr).ns.is_null() || xmlStrEqual((*(*attr).ns).href, (*(*ret).ns).href) != 0)
        {
            break;
        }
        ret = (*ret).next;
    }
    if !ret.is_null() {
        // Free the existing value.
        if !(*ret).children.is_null() {
            crate::xml::tree::free_node_list((*ret).children);
        }
        (*ret).children = ptr::null_mut();
        (*ret).last = ptr::null_mut();
        // Adjust the ns-prefix if needed.
        if !(*ret).ns.is_null() && xmlStrEqual((*(*ret).ns).prefix, (*(*attr).ns).prefix) == 0 {
            (*ret).ns = xsltGetNamespace(ctxt, (*attr).parent, (*attr).ns, target);
        }
    } else {
        // Create a new attribute.
        if !(*attr).ns.is_null() {
            ret = xmlNewNsProp(
                target,
                xsltGetNamespace(ctxt, (*attr).parent, (*attr).ns, target),
                (*attr).name,
                ptr::null(),
            );
        } else {
            ret = xmlNewNsProp(target, ptr::null_mut(), (*attr).name, ptr::null());
        }
    }
    // Set the value.
    if !ret.is_null() {
        if (*attr).psvi.is_null() {
            let content: *mut xmlChar = if (*ctxt).internalized != 0
                && !target.is_null()
                && !(*target).doc.is_null()
                && (*(*target).doc).dict == (*ctxt).dict
                && !(*ctxt).dict.is_null()
                && xmlDictOwns((*ctxt).dict, value) != 0
            {
                value as *mut xmlChar
            } else {
                xmlStrdup(value)
            };
            let text = new_attr_text_node(ret, content);
            let _ = text;
        } else {
            let val = xsltEvalAVT(ctxt, (*attr).psvi, (*attr).parent);
            let content: *mut xmlChar = if val.is_null() {
                xmlStrdup(c"".as_ptr() as *const xmlChar)
            } else {
                val
            };
            let text = new_attr_text_node(ret, content);
            let _ = text;
        }
    }
    ret
}

/// Process the given string, allowing to pass a namespace mapping context,
/// and return the new string value.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar * xsltAttrTemplateValueProcessNode(xsltTransformContextPtr ctxt,
///                                            const xmlChar *str, xmlNodePtr inst);
/// ```
///
/// Returns the computed string value or NULL, must be deallocated by the
/// caller.
///
/// # SAFETY
///
/// - `ctxt` must be a valid transform context (or NULL).
/// - `str` must be NULL or a valid NUL-terminated string.
/// - `inst` must be a valid node or NULL.
#[no_mangle]
pub unsafe extern "C" fn xsltAttrTemplateValueProcessNode(
    ctxt: *mut _xsltTransformContext,
    str: *const xmlChar,
    inst: *mut _xmlNode,
) -> *mut xmlChar {
    if ctxt.is_null() {
        return ptr::null_mut();
    }
    if str.is_null() {
        return ptr::null_mut();
    }
    if *str == 0 {
        return crate::xml::string::bytes_to_xmlstr(&[]);
    }
    if inst.is_null() {
        // Upstream xsltAttrTemplateValueProcess() passes a NULL @inst: the
        // expressions are evaluated against the current context node with
        // no extra namespace mapping.
        return crate::xslt::transform::eval_avt(ctxt, str);
    }
    /*
     * The engine's `eval_avt` resolves XPath namespace prefixes through
     * `ctxt->node` (upstream passes the in-scope namespace list of @inst
     * separately). Swapping in @inst for the duration of the evaluation
     * reproduces the upstream namespace context. Note: as a consequence
     * the XPath context node is temporarily @inst; upstream keeps the
     * source context node and only extends the namespace list.
     */
    let saved_node = (*ctxt).node;
    let saved_inst = (*ctxt).inst;
    (*ctxt).node = inst;
    (*ctxt).inst = inst;
    let ret = crate::xslt::transform::eval_avt(ctxt, str);
    (*ctxt).node = saved_node;
    (*ctxt).inst = saved_inst;
    ret
}

/// Process the given node and return the new string value.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar * xsltAttrTemplateValueProcess(xsltTransformContextPtr ctxt,
///                                        const xmlChar *str);
/// ```
///
/// Returns the computed string value or NULL, must be deallocated by the
/// caller.
///
/// # SAFETY
///
/// - `ctxt` must be a valid transform context (or NULL).
/// - `str` must be NULL or a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn xsltAttrTemplateValueProcess(
    ctxt: *mut _xsltTransformContext,
    str: *const xmlChar,
) -> *mut xmlChar {
    xsltAttrTemplateValueProcessNode(ctxt, str, ptr::null_mut())
}

/// Evaluate a precompiled attribute value template.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar * xsltEvalAVT(xsltTransformContextPtr ctxt, void *avt,
///                       xmlNodePtr node);
/// ```
///
/// Returns the computed string value or NULL, must be deallocated by the
/// caller.
///
/// # Engine mapping (documented simplification)
///
/// Upstream `avt` is an `xsltAttrVTPtr` — a linked list of alternating
/// literal strings and compiled XPath expressions (`attrvt.c`
/// `struct _xsltAttrVT`), built by `xsltCompileAttr` at stylesheet-compile
/// time. This engine does **not** precompile AVTs: the compiled stylesheet
/// tree keeps attribute values as raw strings (no `attr->psvi` is set),
/// and `crate::xslt::transform::eval_avt` performs the AVT scan +
/// XPath evaluation lazily. A non-NULL `avt` is therefore interpreted as a
/// NUL-terminated AVT string and evaluated directly. Callers that stored
/// an upstream-layout `xsltAttrVT` cannot be evaluated here (compiled
/// XPath objects are engine-specific).
///
/// # SAFETY
///
/// - `ctxt` must be a valid transform context (or NULL).
/// - `avt` must be NULL or a valid NUL-terminated string.
/// - `node` must be valid (checked for NULL like upstream).
#[no_mangle]
pub unsafe extern "C" fn xsltEvalAVT(
    ctxt: *mut _xsltTransformContext,
    avt: *mut c_void,
    node: *mut _xmlNode,
) -> *mut xmlChar {
    if ctxt.is_null() || avt.is_null() || node.is_null() {
        return ptr::null_mut();
    }
    crate::xslt::transform::eval_avt(ctxt, avt as *const xmlChar)
}

/// Evaluate an attribute value template of an attribute on a stylesheet
/// instruction node.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar * xsltEvalAttrValueTemplate(xsltTransformContextPtr ctxt,
///                                     xmlNodePtr inst,
///                                     const xmlChar *name, const xmlChar *ns);
/// ```
///
/// Returns the computed string value or NULL, must be deallocated by the
/// caller.
///
/// # SAFETY
///
/// - All pointers must be valid or NULL where permitted; `inst` must be an
///   element node.
#[no_mangle]
pub unsafe extern "C" fn xsltEvalAttrValueTemplate(
    ctxt: *mut _xsltTransformContext,
    inst: *mut _xmlNode,
    name: *const xmlChar,
    ns: *const xmlChar,
) -> *mut xmlChar {
    if ctxt.is_null() || inst.is_null() || name.is_null() {
        return ptr::null_mut();
    }
    if (*inst).type_ != XML_ELEMENT_NODE as c_int {
        return ptr::null_mut();
    }
    let expr = xsltGetNsProp(inst, name, ns);
    if expr.is_null() {
        return ptr::null_mut();
    }
    let ret = xsltAttrTemplateValueProcessNode(ctxt, expr, inst);
    xmlFreeImpl(expr as *mut c_void);
    ret
}

/// Check if an attribute value template has a static value, i.e. the
/// attribute value does not contain expressions contained in curly braces
/// (`{}`).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// const xmlChar * xsltEvalStaticAttrValueTemplate(xsltStylesheetPtr style,
///                                                 xmlNodePtr inst,
///                                                 const xmlChar *name,
///                                                 const xmlChar *ns,
///                                                 int *found);
/// ```
///
/// Returns the static string value or NULL, must be deallocated by the
/// caller (the returned string is owned by the stylesheet dictionary).
///
/// # SAFETY
///
/// - All pointers must be valid or NULL where permitted; `inst` must be an
///   element node.
#[no_mangle]
pub unsafe extern "C" fn xsltEvalStaticAttrValueTemplate(
    style: *mut _xsltStylesheet,
    inst: *mut _xmlNode,
    name: *const xmlChar,
    ns: *const xmlChar,
    found: *mut c_int,
) -> *const xmlChar {
    if style.is_null() || inst.is_null() || name.is_null() {
        return ptr::null();
    }
    if (*inst).type_ != XML_ELEMENT_NODE as c_int {
        return ptr::null();
    }
    let expr = xsltGetNsProp(inst, name, ns);
    if expr.is_null() {
        if !found.is_null() {
            *found = 0;
        }
        return ptr::null();
    }
    if !found.is_null() {
        *found = 1;
    }
    if !xmlStrchr(expr, b'{' as xmlChar).is_null() {
        xmlFreeImpl(expr as *mut c_void);
        return ptr::null();
    }
    let mut ret = xmlDictLookup((*style).dict, expr, -1);
    if ret.is_null() {
        // Engine deviation: dict_lookup returns NULL for empty strings;
        // fall back to the borrowed literal (caller must not free either
        // way).
        ret = c"".as_ptr() as *const xmlChar;
    }
    xmlFreeImpl(expr as *mut c_void);
    ret
}

/// Processes the sequence constructor of the given instruction on
/// `contextNode` and converts the resulting tree to a string. This is
/// needed by e.g. `xsl:comment` and `xsl:processing-instruction`.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar * xsltEvalTemplateString(xsltTransformContextPtr ctxt,
///                                  xmlNodePtr contextNode, xmlNodePtr inst);
/// ```
///
/// Returns the computed string value or NULL; it's up to the caller to
/// free the result.
///
/// # SAFETY
///
/// - All pointers must be valid or NULL where permitted; `inst` must be an
///   element node.
#[no_mangle]
pub unsafe extern "C" fn xsltEvalTemplateString(
    ctxt: *mut _xsltTransformContext,
    contextNode: *mut _xmlNode,
    inst: *mut _xmlNode,
) -> *mut xmlChar {
    if ctxt.is_null() || contextNode.is_null() || inst.is_null() {
        return ptr::null_mut();
    }
    if (*inst).type_ != XML_ELEMENT_NODE as c_int {
        return ptr::null_mut();
    }
    if (*inst).children.is_null() {
        return ptr::null_mut();
    }
    // Create a temporary element node to collect the resulting content
    // (upstream xmlNewDocNode(ctxt->output, NULL, "fake", NULL)).
    let insert = crate::xml::tree::new_node(ptr::null_mut(), c"fake".as_ptr() as *const xmlChar);
    if insert.is_null() {
        crate::xslt::errors::xsltTransformError(
            ctxt,
            ptr::null_mut(),
            inst,
            c"Failed to create temporary node\n".as_ptr() as *const c_char,
        );
        return ptr::null_mut();
    }
    (*insert).doc = (*ctxt).output;

    let old_insert = (*ctxt).insert;
    let old_node = (*ctxt).node;
    let old_inst = (*ctxt).inst;
    let old_lasttext = (*ctxt).lasttext;
    let old_lasttsize = (*ctxt).lasttsize;
    let old_lasttuse = (*ctxt).lasttuse;
    (*ctxt).insert = insert;
    (*ctxt).node = contextNode;
    (*ctxt).inst = inst;
    crate::xslt::transform::execute_content(ctxt, (*inst).children);
    (*ctxt).insert = old_insert;
    (*ctxt).node = old_node;
    (*ctxt).inst = old_inst;
    (*ctxt).lasttext = old_lasttext;
    (*ctxt).lasttsize = old_lasttsize;
    (*ctxt).lasttuse = old_lasttuse;

    let ret = crate::xml::tree::node_get_content(insert);
    crate::xml::tree::free_node_list(insert);
    ret
}

/// Free a list of precompiled attribute value templates.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltFreeAVTList(void *avt);
/// ```
///
/// # Engine mapping (documented simplification)
///
/// Upstream frees the linked list of `xsltAttrVT` structures built by
/// `xsltCompileAttr`. This engine never allocates such structures — the
/// compiled tree stores attribute values as plain strings owned by the
/// stylesheet document (see `xsltEvalAVT`) — so there is nothing owned by
/// the caller to release. A NULL pointer is accepted (no-op) and a
/// non-NULL pointer cannot belong to this engine; it is left untouched.
///
/// # SAFETY
///
/// - `avt` may be any pointer; it is never dereferenced.
#[no_mangle]
pub const unsafe extern "C" fn xsltFreeAVTList(_avt: *mut c_void) {}
// R-000160: upstream 1.1.45 xsltFreeAVTList frees xsltAttrVT lists this engine
// never allocates (AVTs are plain strings owned by the stylesheet doc); the
// empty body is the oracle observable behavior, not a stub.

// ═══════════════════════════════════════════════════════════════════════════════
// Namespace helpers (namespaces.c / xsltutils.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// Search and get the value of an attribute anchored in the namespace
/// specified, or with no namespace when the element is in that namespace.
///
/// The string is allocated in the stylesheet dictionary.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// const xmlChar * xsltGetCNsProp(xsltStylesheetPtr style, xmlNodePtr node,
///                                const xmlChar *name,
///                                const xmlChar *nameSpace);
/// ```
///
/// Returns the attribute value or NULL if not found.
///
/// # SAFETY
///
/// - All pointers must be valid or NULL where permitted.
#[no_mangle]
pub unsafe extern "C" fn xsltGetCNsProp(
    style: *mut _xsltStylesheet,
    node: *mut _xmlNode,
    name: *const xmlChar,
    nameSpace: *const xmlChar,
) -> *const xmlChar {
    if node.is_null() || style.is_null() || (*style).dict.is_null() {
        return ptr::null();
    }
    if nameSpace.is_null() {
        return xmlGetProp(node, name);
    }
    if (*node).type_ == XML_NAMESPACE_DECL as c_int {
        return ptr::null();
    }
    let mut prop = if (*node).type_ == XML_ELEMENT_NODE as c_int {
        (*node).properties
    } else {
        ptr::null_mut()
    };
    while !prop.is_null() {
        let prop_ns = (*prop).ns;
        let matches = (prop_ns.is_null()
            && !(*node).ns.is_null()
            && xmlStrEqual((*(*node).ns).href, nameSpace) != 0)
            || (!prop_ns.is_null() && xmlStrEqual((*prop_ns).href, nameSpace) != 0);
        if xmlStrEqual((*prop).name, name) != 0 && matches {
            let tmp = xmlNodeListGetString((*node).doc, (*prop).children, 1);
            let ret;
            if tmp.is_null() {
                ret = dict_empty((*style).dict);
            } else {
                ret = xmlDictLookup((*style).dict, tmp, -1);
                xmlFreeImpl(tmp as *mut c_void);
            }
            return ret;
        }
        prop = (*prop).next;
    }
    // Check for a default declaration in the internal or external subsets.
    let doc = (*node).doc;
    if !doc.is_null() && !(*doc).intSubset.is_null() {
        let mut attr_decl = xmlGetDtdAttrDesc((*doc).intSubset, (*node).name, name);
        if attr_decl.is_null() && !(*doc).extSubset.is_null() {
            attr_decl = xmlGetDtdAttrDesc((*doc).extSubset, (*node).name, name);
        }
        if !attr_decl.is_null() && !(*attr_decl).prefix.is_null() {
            let ns = xmlSearchNs(doc, node, (*attr_decl).prefix);
            if !ns.is_null() && xmlStrEqual((*ns).href, nameSpace) != 0 {
                return xmlDictLookup((*style).dict, (*attr_decl).defaultValue, -1);
            }
        }
    }
    ptr::null()
}

/// Search and get the value of an attribute anchored in the namespace
/// specified, or with no namespace when the element is in that namespace.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar * xsltGetNsProp(xmlNodePtr node, const xmlChar *name,
///                         const xmlChar *nameSpace);
/// ```
///
/// Returns the attribute value or NULL if not found; it's up to the caller
/// to free the memory.
///
/// # SAFETY
///
/// - All pointers must be valid or NULL where permitted.
#[no_mangle]
pub unsafe extern "C" fn xsltGetNsProp(
    node: *mut _xmlNode,
    name: *const xmlChar,
    nameSpace: *const xmlChar,
) -> *mut xmlChar {
    if node.is_null() {
        return ptr::null_mut();
    }
    if nameSpace.is_null() {
        return xmlGetProp(node, name);
    }
    if (*node).type_ == XML_NAMESPACE_DECL as c_int {
        return ptr::null_mut();
    }
    let mut prop = if (*node).type_ == XML_ELEMENT_NODE as c_int {
        (*node).properties
    } else {
        ptr::null_mut()
    };
    while !prop.is_null() {
        let prop_ns = (*prop).ns;
        let matches = (prop_ns.is_null()
            && !(*node).ns.is_null()
            && xmlStrEqual((*(*node).ns).href, nameSpace) != 0)
            || (!prop_ns.is_null() && xmlStrEqual((*prop_ns).href, nameSpace) != 0);
        if xmlStrEqual((*prop).name, name) != 0 && matches {
            let ret = xmlNodeListGetString((*node).doc, (*prop).children, 1);
            if ret.is_null() {
                return xmlStrdup(c"".as_ptr() as *const xmlChar);
            }
            return ret;
        }
        prop = (*prop).next;
    }
    // Check for a default declaration in the internal or external subsets.
    let doc = (*node).doc;
    if !doc.is_null() && !(*doc).intSubset.is_null() {
        let mut attr_decl = xmlGetDtdAttrDesc((*doc).intSubset, (*node).name, name);
        if attr_decl.is_null() && !(*doc).extSubset.is_null() {
            attr_decl = xmlGetDtdAttrDesc((*doc).extSubset, (*node).name, name);
        }
        if !attr_decl.is_null() && !(*attr_decl).prefix.is_null() {
            let ns = xmlSearchNs(doc, node, (*attr_decl).prefix);
            if !ns.is_null() && xmlStrEqual((*ns).href, nameSpace) != 0 {
                return xmlStrdup((*attr_decl).defaultValue);
            }
        }
    }
    ptr::null_mut()
}

/// Find a matching (prefix and ns-name) ns-declaration for the requested
/// `ns->prefix` and `ns->href` in the result tree, applying namespace
/// aliasing.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNsPtr xsltGetNamespace(xsltTransformContextPtr ctxt, xmlNodePtr cur,
///                           xmlNsPtr ns, xmlNodePtr out);
/// ```
///
/// Returns a namespace declaration or NULL in case of namespace fixup
/// failures or API/internal errors.
///
/// # SAFETY
///
/// - All pointers must be valid or NULL where permitted.
#[no_mangle]
pub unsafe extern "C" fn xsltGetNamespace(
    ctxt: *mut _xsltTransformContext,
    cur: *mut _xmlNode,
    ns: *mut _xmlNs,
    out: *mut _xmlNode,
) -> *mut _xmlNs {
    if ns.is_null() {
        return ptr::null_mut();
    }
    if ctxt.is_null() || cur.is_null() || out.is_null() {
        return ptr::null_mut();
    }
    // Upstream walks the import chain looking for a stylesheet whose
    // nsAliases table maps ns->href (alias resolution); the engine keeps
    // the table as a linked list.
    let mut style = (*ctxt).style;
    let mut uri: *const xmlChar = ptr::null();
    while !style.is_null() {
        if !(*ns).href.is_null() && !(*style).nsAliases.is_null() {
            uri = lookup_ns_alias(style, (*ns).href);
        }
        if !uri.is_null() {
            break;
        }
        style = xslt_next_import(style);
    }
    if ns_uri_is_undefined(uri) {
        return xsltGetSpecialNamespace(ctxt, cur, ptr::null(), ptr::null(), out);
    }
    if uri.is_null() {
        uri = (*ns).href;
    }
    xsltGetSpecialNamespace(ctxt, cur, uri, (*ns).prefix, out)
}

/// Exactly the same as `xsltGetNamespace()`; obsolete upstream, kept for
/// ABI compatibility.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNsPtr xsltGetPlainNamespace(xsltTransformContextPtr ctxt,
///                                xmlNodePtr cur, xmlNsPtr ns,
///                                xmlNodePtr out);
/// ```
///
/// Returns a namespace declaration or NULL in case of namespace fixup
/// failures or API/internal errors.
///
/// # SAFETY
///
/// - All pointers must be valid or NULL where permitted.
#[no_mangle]
pub unsafe extern "C" fn xsltGetPlainNamespace(
    ctxt: *mut _xsltTransformContext,
    cur: *mut _xmlNode,
    ns: *mut _xmlNs,
    out: *mut _xmlNode,
) -> *mut _xmlNs {
    xsltGetNamespace(ctxt, cur, ns, out)
}

/// Find a matching (prefix and ns-name) ns-declaration for the requested
/// `nsName` and `nsPrefix` in the result tree. If none is found a new
/// ns-declaration is added to `target`; if the given prefix is already in
/// use, a ns-declaration with a modified ns-prefix is created.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNsPtr xsltGetSpecialNamespace(xsltTransformContextPtr ctxt,
///                                  xmlNodePtr invocNode,
///                                  const xmlChar *nsName,
///                                  const xmlChar *nsPrefix,
///                                  xmlNodePtr target);
/// ```
///
/// Returns a namespace declaration or NULL.
///
/// # SAFETY
///
/// - All pointers must be valid or NULL where permitted; `target` must be
///   an element node.
#[no_mangle]
pub unsafe extern "C" fn xsltGetSpecialNamespace(
    ctxt: *mut _xsltTransformContext,
    invocNode: *mut _xmlNode,
    nsName: *const xmlChar,
    nsPrefix: *const xmlChar,
    target: *mut _xmlNode,
) -> *mut _xmlNs {
    if ctxt.is_null() || target.is_null() {
        return ptr::null_mut();
    }
    if (*target).type_ != XML_ELEMENT_NODE as c_int {
        return ptr::null_mut();
    }

    // "Undeclaration" of the default namespace (bug #302020).
    if nsPrefix.is_null() && (nsName.is_null() || *nsName == 0) {
        if !(*target).nsDef.is_null() {
            let mut ns = (*target).nsDef;
            loop {
                if (*ns).prefix.is_null() {
                    if !(*ns).href.is_null() && *(*ns).href != 0 {
                        crate::xslt::errors::xsltTransformError(
                            ctxt,
                            ptr::null_mut(),
                            invocNode,
                            c"Namespace normalization error: Cannot undeclare the default namespace, since the default namespace is already declared on the result element.\n"
                                .as_ptr() as *const c_char,
                        );
                        return ptr::null_mut();
                    } else {
                        // The default namespace was undeclared on the
                        // result element.
                        return ptr::null_mut();
                    }
                }
                ns = (*ns).next;
                if ns.is_null() {
                    break;
                }
            }
        }
        if !(*target).parent.is_null() && (*(*target).parent).type_ == XML_ELEMENT_NODE as c_int {
            // The parent element is in no namespace, so assume there is no
            // default namespace in scope.
            if (*(*target).parent).ns.is_null() {
                return ptr::null_mut();
            }
            let ns = xmlSearchNs((*target).doc, (*target).parent, ptr::null());
            if ns.is_null() || (*ns).href.is_null() || *(*ns).href == 0 {
                return ptr::null_mut();
            }
            // Undeclare the default namespace.
            xmlNewNs(target, c"".as_ptr() as *const xmlChar, ptr::null());
            return ptr::null_mut();
        }
        return ptr::null_mut();
    }

    // Handle the XML namespace.
    if !nsPrefix.is_null() {
        let plen = libc::strlen(nsPrefix as *const c_char);
        if plen == 3 && core::slice::from_raw_parts(nsPrefix, 3) == b"xml" {
            return xmlSearchNs((*target).doc, target, nsPrefix);
        }
    }

    // First: search on the result element itself.
    let mut prefix_occupied = 0;
    let mut ns: *mut _xmlNs = ptr::null_mut();
    if !(*target).nsDef.is_null() {
        ns = (*target).nsDef;
        loop {
            let ns_prefix = (*ns).prefix;
            if (ns_prefix.is_null()) == (nsPrefix.is_null())
                && (ns_prefix == nsPrefix || xmlStrEqual(ns_prefix, nsPrefix) != 0)
            {
                if xmlStrEqual((*ns).href, nsName) != 0 {
                    return ns;
                }
                prefix_occupied = 1;
                break;
            }
            ns = (*ns).next;
            if ns.is_null() {
                break;
            }
        }
    }
    if prefix_occupied != 0 {
        // The desired prefix is shadowed: search for any in-scope
        // ns-decl with a matching ns-name before changing the prefix.
        ns = xmlSearchNsByHref((*target).doc, target, nsName);
        if !ns.is_null() {
            return ns;
        }
    } else if !(*target).parent.is_null() && (*(*target).parent).type_ == XML_ELEMENT_NODE as c_int
    {
        // Check the common case: the parent of the current result element
        // is in the same namespace (with an equal ns-prefix).
        let parent_ns = (*(*target).parent).ns;
        if !parent_ns.is_null() && ((*parent_ns).prefix.is_null()) == (nsPrefix.is_null()) {
            if nsPrefix.is_null() {
                if xmlStrEqual((*parent_ns).href, nsName) != 0 {
                    return parent_ns;
                }
            } else if xmlStrEqual((*parent_ns).prefix, nsPrefix) != 0
                && xmlStrEqual((*parent_ns).href, nsName) != 0
            {
                return parent_ns;
            }
        }
        // Lookup the remaining in-scope namespaces.
        ns = xmlSearchNs((*target).doc, (*target).parent, nsPrefix);
        if !ns.is_null() {
            if xmlStrEqual((*ns).href, nsName) != 0 {
                return ns;
            }
            // Ensure the new ns-decl won't shadow a prefix in-use by an
            // existing attribute.
            if !(*target).properties.is_null() {
                let mut attr = (*target).properties;
                loop {
                    if !(*attr).ns.is_null() && xmlStrEqual((*(*attr).ns).prefix, nsPrefix) != 0 {
                        ns = xmlSearchNsByHref((*target).doc, target, nsName);
                        if !ns.is_null() {
                            return ns;
                        }
                        return xslt_declare_new_prefix(ctxt, invocNode, nsName, nsPrefix, target);
                    }
                    attr = (*attr).next;
                    if attr.is_null() {
                        break;
                    }
                }
            }
        }
        // Create the ns-decl on the current result element.
        ns = xmlNewNs(target, nsName, nsPrefix);
        return ns;
    } else {
        // This is either the root of the tree or something weird is going
        // on.
        ns = xmlNewNs(target, nsName, nsPrefix);
        return ns;
    }
    // Fall-through of the prefix-occupied case: no in-scope declaration
    // with a matching ns-name was found, so generate a new prefix
    // (upstream `declare_new_prefix:` label).
    xslt_declare_new_prefix(ctxt, invocNode, nsName, nsPrefix, target)
}

/// Fallback of `xsltGetSpecialNamespace`: generate a new prefix and
/// declare the namespace on the result element.
///
/// # SAFETY
///
/// - All pointers must be valid or NULL as permitted.
unsafe fn xslt_declare_new_prefix(
    ctxt: *mut _xsltTransformContext,
    invocNode: *mut _xmlNode,
    nsName: *const xmlChar,
    nsPrefix: *const xmlChar,
    target: *mut _xmlNode,
) -> *mut _xmlNs {
    let base: *const xmlChar = if nsPrefix.is_null() {
        c"ns".as_ptr() as *const xmlChar
    } else {
        nsPrefix
    };
    let base_str = String::from_utf8_lossy(core::slice::from_raw_parts(
        base,
        libc::strlen(base as *const c_char) as usize,
    ))
    .into_owned();
    let mut counter: c_int = 1;
    loop {
        let pref = format!("{}_{}", base_str, counter);
        counter += 1;
        let pref_c = crate::xml::string::bytes_to_xmlstr(pref.as_bytes());
        if pref_c.is_null() {
            return ptr::null_mut();
        }
        let ns = xmlSearchNs((*target).doc, target, pref_c);
        xmlFreeImpl(pref_c as *mut c_void);
        if counter > 1000 {
            crate::xslt::errors::xsltTransformError(
                ctxt,
                ptr::null_mut(),
                invocNode,
                c"Internal error in xsltAcquireResultInScopeNs(): Failed to compute a unique ns-prefix for the generated element"
                    .as_ptr() as *const c_char,
            );
            return ptr::null_mut();
        }
        if ns.is_null() {
            let pref_c = crate::xml::string::bytes_to_xmlstr(pref.as_bytes());
            return xmlNewNs(target, nsName, pref_c);
        }
    }
}

/// Copy a namespace node (declaration). If `elem` is not NULL, the new
/// namespace will be declared on `elem`.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNsPtr xsltCopyNamespace(xsltTransformContextPtr ctxt,
///                            xmlNodePtr elem, xmlNsPtr ns);
/// ```
///
/// Returns a new `xmlNsPtr`, or NULL in case of an error.
///
/// # SAFETY
///
/// - All pointers must be valid or NULL where permitted.
#[no_mangle]
pub unsafe extern "C" fn xsltCopyNamespace(
    _ctxt: *mut _xsltTransformContext,
    elem: *mut _xmlNode,
    ns: *mut _xmlNs,
) -> *mut _xmlNs {
    if ns.is_null() || (*ns).type_ != XML_NAMESPACE_DECL as c_int {
        return ptr::null_mut();
    }
    // One can add namespaces only on element nodes.
    if !elem.is_null() && (*elem).type_ != XML_ELEMENT_NODE as c_int {
        return xmlNewNs(ptr::null_mut(), (*ns).href, (*ns).prefix);
    }
    xmlNewNs(elem, (*ns).href, (*ns).prefix)
}

/// Copy a namespace list. If `node` is non-NULL the new namespaces are
/// added automatically. This handles namespace aliases.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNsPtr xsltCopyNamespaceList(xsltTransformContextPtr ctxt,
///                                xmlNodePtr node, xmlNsPtr cur);
/// ```
///
/// Returns a new `xmlNsPtr`, or NULL in case of error.
///
/// # SAFETY
///
/// - All pointers must be valid or NULL where permitted.
#[no_mangle]
pub unsafe extern "C" fn xsltCopyNamespaceList(
    ctxt: *mut _xsltTransformContext,
    node: *mut _xmlNode,
    cur: *mut _xmlNs,
) -> *mut _xmlNs {
    if cur.is_null() {
        return ptr::null_mut();
    }
    if (*cur).type_ != XML_NAMESPACE_DECL as c_int {
        return ptr::null_mut();
    }
    // One can add namespaces only on element nodes.
    let mut node = node;
    if !node.is_null() && (*node).type_ != XML_ELEMENT_NODE as c_int {
        node = ptr::null_mut();
    }

    let mut ret: *mut _xmlNs = ptr::null_mut();
    let mut p: *mut _xmlNs = ptr::null_mut();
    let mut cur = cur;
    while !cur.is_null() {
        if (*cur).type_ != XML_NAMESPACE_DECL as c_int {
            break;
        }
        // Avoid duplicating namespace declarations in the tree if a
        // matching declaration is in scope.
        if !node.is_null() {
            if !(*node).ns.is_null()
                && xmlStrEqual((*(*node).ns).prefix, (*cur).prefix) != 0
                && xmlStrEqual((*(*node).ns).href, (*cur).href) != 0
            {
                cur = (*cur).next;
                continue;
            }
            let tmp = xmlSearchNs((*node).doc, node, (*cur).prefix);
            if !tmp.is_null() && xmlStrEqual((*tmp).href, (*cur).href) != 0 {
                cur = (*cur).next;
                continue;
            }
        }
        if xmlStrEqual((*cur).href, XSLT_NAMESPACE.as_ptr() as *const xmlChar) == 0 {
            // Apply namespace aliasing (upstream hash lookup on the current
            // stylesheet, without walking the import chain).
            let style = if ctxt.is_null() {
                ptr::null_mut()
            } else {
                (*ctxt).style
            };
            let uri = if !style.is_null() {
                lookup_ns_alias(style, (*cur).href)
            } else {
                ptr::null()
            };
            if ns_uri_is_undefined(uri) {
                cur = (*cur).next;
                continue;
            }
            let q: *mut _xmlNs = if !uri.is_null() {
                xmlNewNs(node, uri, (*cur).prefix)
            } else {
                xmlNewNs(node, (*cur).href, (*cur).prefix)
            };
            if p.is_null() {
                ret = q;
                p = q;
            } else {
                (*p).next = q;
                p = q;
            }
        }
        cur = (*cur).next;
    }
    ret
}

/// Read the `stylesheet-prefix` and `result-prefix` attributes of an
/// `xsl:namespace-alias` node and register them as a namespace alias.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltNamespaceAlias(xsltStylesheetPtr style, xmlNodePtr node);
/// ```
///
/// # SAFETY
///
/// - `style` and `node` must be valid pointers or NULL.
#[no_mangle]
pub unsafe extern "C" fn xsltNamespaceAlias(style: *mut _xsltStylesheet, node: *mut _xmlNode) {
    if style.is_null() || node.is_null() {
        return;
    }
    let style_prefix = xmlGetProp(node, c"stylesheet-prefix".as_ptr() as *const xmlChar);
    if style_prefix.is_null() {
        crate::xslt::errors::xsltTransformError(
            ptr::null_mut(),
            style,
            node,
            c"namespace-alias: stylesheet-prefix attribute missing\n".as_ptr() as *const c_char,
        );
        return;
    }
    let result_prefix = xmlGetProp(node, c"result-prefix".as_ptr() as *const xmlChar);
    if result_prefix.is_null() {
        crate::xslt::errors::xsltTransformError(
            ptr::null_mut(),
            style,
            node,
            c"namespace-alias: result-prefix attribute missing\n".as_ptr() as *const c_char,
        );
        xmlFreeImpl(style_prefix as *mut c_void);
        return;
    }

    // Resolve the stylesheet-prefix to its namespace.
    let literal_ns: *mut _xmlNs;
    let literal_ns_name: *const xmlChar;
    if xmlStrEqual(style_prefix, c"#default".as_ptr() as *const xmlChar) != 0 {
        literal_ns = xmlSearchNs((*node).doc, node, ptr::null());
        if literal_ns.is_null() {
            literal_ns_name = ptr::null();
        } else {
            literal_ns_name = (*literal_ns).href;
        }
    } else {
        literal_ns = xmlSearchNs((*node).doc, node, style_prefix);
        if literal_ns.is_null() || (*literal_ns).href.is_null() {
            crate::xslt::errors::xsltTransformError(
                ptr::null_mut(),
                style,
                node,
                c"namespace-alias: prefix not bound to any namespace\n".as_ptr() as *const c_char,
            );
            xmlFreeImpl(style_prefix as *mut c_void);
            xmlFreeImpl(result_prefix as *mut c_void);
            return;
        }
        literal_ns_name = (*literal_ns).href;
    }

    // Resolve the result-prefix to its namespace. When "#default" is used
    // without a default namespace in scope, the special value
    // UNDEFINED_DEFAULT_NS is stored in the nsAliases table.
    let target_ns: *mut _xmlNs;
    let target_ns_name: *const xmlChar;
    if xmlStrEqual(result_prefix, c"#default".as_ptr() as *const xmlChar) != 0 {
        target_ns = xmlSearchNs((*node).doc, node, ptr::null());
        if target_ns.is_null() {
            target_ns_name = UNDEFINED_DEFAULT_NS;
        } else {
            target_ns_name = (*target_ns).href;
        }
    } else {
        target_ns = xmlSearchNs((*node).doc, node, result_prefix);
        if target_ns.is_null() || (*target_ns).href.is_null() {
            crate::xslt::errors::xsltTransformError(
                ptr::null_mut(),
                style,
                node,
                c"namespace-alias: prefix not bound to any namespace\n".as_ptr() as *const c_char,
            );
            xmlFreeImpl(style_prefix as *mut c_void);
            xmlFreeImpl(result_prefix as *mut c_void);
            return;
        }
        target_ns_name = (*target_ns).href;
    }

    if literal_ns_name.is_null() {
        // #default used for the stylesheet-prefix with no default
        // namespace in scope: use style->defaultAlias for this.
        if !target_ns.is_null() {
            (*style).defaultAlias = (*target_ns).href;
        }
    } else {
        // Register the alias. Upstream stores the pair in an
        // xmlHashAddEntry() table (the first declaration of a duplicate
        // key wins); the engine keeps a linked list, so the most recent
        // declaration wins. The "undefined default namespace" target is
        // stored as an unambiguous heap marker (see ns_uri_is_undefined).
        let result_stored: *const xmlChar = if ns_uri_is_undefined(target_ns_name) {
            alloc_str(UNDEFINED_DEFAULT_NS_MARKER)
        } else {
            alloc_str(core::slice::from_raw_parts(
                target_ns_name,
                libc::strlen(target_ns_name as *const c_char) as usize,
            ))
        };
        let style_stored = alloc_str(core::slice::from_raw_parts(
            literal_ns_name,
            libc::strlen(literal_ns_name as *const c_char) as usize,
        ));
        if result_stored.is_null() || style_stored.is_null() {
            if !result_stored.is_null() {
                xmlFreeImpl(result_stored as *mut c_void);
            }
            if !style_stored.is_null() {
                xmlFreeImpl(style_stored as *mut c_void);
            }
        } else {
            let alias = libc::calloc(1, core::mem::size_of::<_xsltNsAlias>()) as *mut _xsltNsAlias;
            if alias.is_null() {
                xmlFreeImpl(result_stored as *mut c_void);
                xmlFreeImpl(style_stored as *mut c_void);
            } else {
                (*alias).styleNs = style_stored;
                (*alias).resultNs = result_stored;
                (*alias).next = (*style).nsAliases as *mut _xsltNsAlias;
                (*style).nsAliases = alias as *mut c_void;
            }
        }
    }

    xmlFreeImpl(style_prefix as *mut c_void);
    xmlFreeImpl(result_prefix as *mut c_void);
}

/// Free the memory used by namespace aliases.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltFreeNamespaceAliasHashes(xsltStylesheetPtr style);
/// ```
///
/// # SAFETY
///
/// - `style` must be a valid `_xsltStylesheet` or NULL.
#[no_mangle]
pub unsafe extern "C" fn xsltFreeNamespaceAliasHashes(style: *mut _xsltStylesheet) {
    if style.is_null() {
        return;
    }
    crate::xslt::namespace_alias::xsltFreeNsAliases(style);
}

/// Copy a text string: add `string` to a newly created or an existent text
/// node child of `target`.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNodePtr xsltCopyTextString(xsltTransformContextPtr ctxt,
///                               xmlNodePtr target, const xmlChar *string,
///                               int noescape);
/// ```
///
/// Returns the text node, or NULL in case of API or internal errors.
///
/// # SAFETY
///
/// - All pointers must be valid or NULL where permitted.
#[no_mangle]
pub unsafe extern "C" fn xsltCopyTextString(
    ctxt: *mut _xsltTransformContext,
    target: *mut _xmlNode,
    string: *const xmlChar,
    noescape: c_int,
) -> *mut _xmlNode {
    if string.is_null() {
        return ptr::null_mut();
    }
    // Defensive: upstream dereferences @ctxt unconditionally (crash on
    // NULL); returning NULL keeps the Rust build free of UB.
    if ctxt.is_null() {
        return ptr::null_mut();
    }
    // Play safe and reset the merging mechanism for every new target node.
    if target.is_null() || (*target).children.is_null() {
        (*ctxt).lasttext = ptr::null();
    }

    // Handle coalescing of text nodes here.
    let len = xmlStrlen(string);
    let cdata_section = (*ctxt).type_ == XSLT_OUTPUT_XML
        && !(*ctxt).style.is_null()
        && !(*(*ctxt).style).cdataSection.is_null()
        && !target.is_null()
        && (*target).type_ == XML_ELEMENT_NODE as c_int
        && (((*target).ns.is_null()
            && !xmlHashLookup2((*(*ctxt).style).cdataSection, (*target).name, ptr::null())
                .is_null())
            || (!(*target).ns.is_null()
                && !xmlHashLookup2(
                    (*(*ctxt).style).cdataSection,
                    (*target).name,
                    (*(*target).ns).href,
                )
                .is_null()));
    let copy: *mut _xmlNode;
    if cdata_section {
        // Process "cdata-section-elements".
        if !target.is_null()
            && !(*target).last.is_null()
            && (*(*target).last).type_ == XML_CDATA_SECTION_NODE as c_int
        {
            return xslt_add_text_string(ctxt, (*target).last, string, len);
        }
        copy = xmlNewCDataBlock((*ctxt).output, string, len);
    } else if noescape != 0 {
        // Process "disable-output-escaping".
        if !target.is_null()
            && !(*target).last.is_null()
            && (*(*target).last).type_ == XML_TEXT_NODE as c_int
            && node_name_eq((*(*target).last).name, b"textnoenc")
        {
            return xslt_add_text_string(ctxt, (*target).last, string, len);
        }
        copy = xmlNewTextLen(string, len);
        if !copy.is_null() {
            // Upstream renames the text node to the xmlStringTextNoenc
            // static marker; this engine duplicates the marker string
            // (ownership: the node frees its name). NB: we use a local
            // NUL-terminated copy rather than the engine's
            // xmlStringTextNoenc static, which lacks its NUL terminator.
            let name = alloc_str(b"textnoenc");
            if !name.is_null() {
                xmlFreeImpl((*copy).name as *mut c_void);
                (*copy).name = name;
            }
        }
    } else {
        // Default processing.
        if !target.is_null()
            && !(*target).last.is_null()
            && (*(*target).last).type_ == XML_TEXT_NODE as c_int
            && node_name_eq((*(*target).last).name, b"text")
        {
            return xslt_add_text_string(ctxt, (*target).last, string, len);
        }
        copy = xmlNewTextLen(string, len);
    }
    let copy = if !copy.is_null() && !target.is_null() {
        xmlAddChild(target, copy)
    } else {
        copy
    };
    if !copy.is_null() {
        (*ctxt).lasttext = (*copy).content;
        (*ctxt).lasttsize = len;
        (*ctxt).lasttuse = len;
    } else {
        crate::xslt::errors::xsltTransformError(
            ctxt,
            ptr::null_mut(),
            target,
            c"xsltCopyTextString: text copy failed\n".as_ptr() as *const c_char,
        );
        (*ctxt).lasttext = ptr::null();
    }
    copy
}

/// Compare a NUL-terminated xmlChar string with a byte slice.
///
/// # SAFETY
///
/// - `s` must be NULL or a valid NUL-terminated string.
unsafe fn node_name_eq(s: *const xmlChar, b: &[u8]) -> bool {
    if s.is_null() {
        return false;
    }
    let len = libc::strlen(s as *const c_char) as usize;
    len == b.len() && core::slice::from_raw_parts(s, len) == b
}

/// Upstream static `xsltAddTextString()` (transform.c): extend the current
/// text node with a new string, handling coalescing. Returns the text node.
///
/// # SAFETY
///
/// - All pointers must be valid or NULL where permitted.
unsafe fn xslt_add_text_string(
    ctxt: *mut _xsltTransformContext,
    target: *mut _xmlNode,
    string: *const xmlChar,
    len: c_int,
) -> *mut _xmlNode {
    if len <= 0 || string.is_null() || target.is_null() {
        return target;
    }
    // Defensive: upstream dereferences @ctxt unconditionally.
    if ctxt.is_null() {
        crate::xml::tree::text_concat(target, string, len);
        return target;
    }
    if (*ctxt).lasttext == (*target).content {
        // Check for integer overflow accounting for the NUL terminator.
        if len >= c_int::MAX - (*ctxt).lasttuse {
            crate::xslt::errors::xsltTransformError(
                ctxt,
                ptr::null_mut(),
                target,
                c"xsltCopyText: text allocation failed\n".as_ptr() as *const c_char,
            );
            return ptr::null_mut();
        }
        let min_size = (*ctxt).lasttuse + len + 1;
        if (*ctxt).lasttsize < min_size {
            let extra = if min_size < 100 { 100 } else { min_size };
            let size = if extra > c_int::MAX - (*ctxt).lasttsize {
                c_int::MAX
            } else {
                (*ctxt).lasttsize + extra
            };
            let newbuf =
                xmlReallocImpl((*target).content as *mut c_void, size as usize) as *mut xmlChar;
            if newbuf.is_null() {
                crate::xslt::errors::xsltTransformError(
                    ctxt,
                    ptr::null_mut(),
                    target,
                    c"xsltCopyText: text allocation failed\n".as_ptr() as *const c_char,
                );
                return ptr::null_mut();
            }
            (*ctxt).lasttsize = size;
            (*ctxt).lasttext = newbuf;
            (*target).content = newbuf;
        }
        libc::memcpy(
            (*target).content.add((*ctxt).lasttuse as usize) as *mut libc::c_void,
            string as *const libc::c_void,
            len as usize,
        );
        (*ctxt).lasttuse += len;
        *(*target).content.add((*ctxt).lasttuse as usize) = 0;
    } else {
        crate::xml::tree::text_concat(target, string, len);
        (*ctxt).lasttext = (*target).content;
        let l = xmlStrlen((*target).content);
        (*ctxt).lasttsize = l;
        (*ctxt).lasttuse = l;
    }
    target
}

// ═══════════════════════════════════════════════════════════════════════════════
// String / QName utilities (xsltutils.c, xslt.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// Check if a string is ignorable: NULL or made of blank characters.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltIsBlank(xmlChar *str);
/// ```
///
/// Returns 1 if the string is NULL or made of blanks chars, 0 otherwise.
///
/// # SAFETY
///
/// - `str` must be NULL or a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn xsltIsBlank(str: *mut xmlChar) -> c_int {
    if str.is_null() {
        return 1;
    }
    let mut cur = str;
    while *cur != 0 {
        // IS_BLANK: 0x20 (space), 0x09 (tab), 0x0A (LF), 0x0D (CR).
        if *cur != b' ' && *cur != b'\t' && *cur != b'\n' && *cur != b'\r' {
            return 0;
        }
        cur = cur.add(1);
    }
    1
}

/// Split QNames into prefix and local names, both allocated from a
/// dictionary.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// const xmlChar * xsltSplitQName(xmlDictPtr dict, const xmlChar *name,
///                                const xmlChar **prefix);
/// ```
///
/// Returns the localname or NULL in case of error.
///
/// # SAFETY
///
/// - `dict` must be a valid dictionary pointer or NULL.
/// - `name` must be a valid NUL-terminated string or NULL.
/// - `prefix` must be a valid `const xmlChar **` or NULL.
#[no_mangle]
pub unsafe extern "C" fn xsltSplitQName(
    dict: *mut c_void,
    name: *const xmlChar,
    prefix: *mut *const xmlChar,
) -> *const xmlChar {
    if !prefix.is_null() {
        *prefix = ptr::null();
    }
    if name.is_null() || dict.is_null() {
        return ptr::null();
    }
    if *name == b':' as xmlChar {
        return xmlDictLookup(dict, name, -1);
    }
    let mut len: usize = 0;
    while *name.add(len) != 0 && *name.add(len) != b':' as xmlChar {
        len += 1;
    }
    if *name.add(len) == 0 {
        return xmlDictLookup(dict, name, -1);
    }
    if !prefix.is_null() {
        *prefix = xmlDictLookup(dict, name, len as c_int);
    }
    xmlDictLookup(dict, name.add(len + 1), -1)
}

/// Analyze `*name`; if the name contains a prefix, search the associated
/// namespace in scope for it. `*name` is replaced with the NCName (the old
/// value being freed). Errors in the prefix lookup are signalled by setting
/// `*name` to NULL.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// const xmlChar * xsltGetQNameURI(xmlNodePtr node, xmlChar ** name);
/// ```
///
/// Returns the namespace URI if there is a prefix, or NULL if `*name` is
/// not prefixed.
///
/// # SAFETY
///
/// - `node` must be a valid node or NULL.
/// - `name` must be a valid `xmlChar **` or NULL.
#[no_mangle]
pub unsafe extern "C" fn xsltGetQNameURI(
    node: *mut _xmlNode,
    name: *mut *mut xmlChar,
) -> *const xmlChar {
    if name.is_null() {
        return ptr::null();
    }
    let qname = *name;
    if qname.is_null() || *qname == 0 {
        return ptr::null();
    }
    if node.is_null() {
        xmlFreeImpl(qname as *mut c_void);
        *name = ptr::null_mut();
        return ptr::null();
    }
    // Nasty but valid.
    if *qname == b':' as xmlChar {
        return ptr::null();
    }
    // We are not trying to validate but just to cut, and yes it will work
    // even if this is a set of UTF-8 encoded chars.
    let qlen = libc::strlen(qname as *const c_char) as usize;
    let mut len: usize = 0;
    while len < qlen && *qname.add(len) != b':' as xmlChar {
        len += 1;
    }
    if len == qlen {
        return ptr::null();
    }
    // Handle xml: separately, this one is magical.
    if len >= 4 && core::slice::from_raw_parts(qname, 4) == b"xml:" {
        if qlen == 4 {
            return ptr::null();
        }
        *name = xmlStrdup(qname.add(4));
        xmlFreeImpl(qname as *mut c_void);
        return crate::abi::constants::XML_XML_NAMESPACE.as_ptr() as *const xmlChar;
    }
    *qname.add(len) = 0;
    let ns = xmlSearchNs((*node).doc, node, qname);
    if ns.is_null() {
        *name = ptr::null_mut();
        xmlFreeImpl(qname as *mut c_void);
        return ptr::null();
    }
    *name = xmlStrdup(qname.add(len + 1));
    xmlFreeImpl(qname as *mut c_void);
    (*ns).href
}

/// Similar to `xsltGetQNameURI`, but used when `*name` is a dictionary
/// entry.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// const xmlChar * xsltGetQNameURI2(xsltStylesheetPtr style,
///                                  xmlNodePtr node,
///                                  const xmlChar **name);
/// ```
///
/// Returns the namespace URI if there is a prefix, or NULL if `*name` is
/// not prefixed.
///
/// # SAFETY
///
/// - All pointers must be valid or NULL where permitted.
#[no_mangle]
pub unsafe extern "C" fn xsltGetQNameURI2(
    style: *mut _xsltStylesheet,
    node: *mut _xmlNode,
    name: *mut *const xmlChar,
) -> *const xmlChar {
    if name.is_null() {
        return ptr::null();
    }
    let qname = *name;
    if qname.is_null() || *qname == 0 {
        return ptr::null();
    }
    if node.is_null() {
        *name = ptr::null();
        return ptr::null();
    }
    let qlen = libc::strlen(qname as *const c_char) as usize;
    let mut len: usize = 0;
    while len < qlen && *qname.add(len) != b':' as xmlChar {
        len += 1;
    }
    if len == qlen {
        return ptr::null();
    }
    // Handle xml: separately, this one is magical.
    if len >= 4 && core::slice::from_raw_parts(qname, 4) == b"xml:" {
        if qlen == 4 {
            return ptr::null();
        }
        if !style.is_null() {
            *name = xmlDictLookup((*style).dict, qname.add(4), -1);
        } else {
            *name = ptr::null();
        }
        return crate::abi::constants::XML_XML_NAMESPACE.as_ptr() as *const xmlChar;
    }
    let qname_prefix = xmlStrndup(qname, len as c_int);
    let ns = xmlSearchNs((*node).doc, node, qname_prefix);
    if ns.is_null() {
        if !style.is_null() {
            crate::xslt::errors::xsltTransformError(
                ptr::null_mut(),
                style,
                node,
                c"No namespace bound to prefix.\n".as_ptr() as *const c_char,
            );
            (*style).errors += 1;
        }
        *name = ptr::null();
        if !qname_prefix.is_null() {
            xmlFreeImpl(qname_prefix as *mut c_void);
        }
        return ptr::null();
    }
    if !style.is_null() {
        *name = xmlDictLookup((*style).dict, qname.add(len + 1), -1);
    } else {
        *name = ptr::null();
    }
    if !qname_prefix.is_null() {
        xmlFreeImpl(qname_prefix as *mut c_void);
    }
    (*ns).href
}

/// Read one UTF-8 char from `utf` (copied from libxml2
/// `xmlGetUTF8Char()`).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltGetUTF8Char(const unsigned char *utf, int *len);
/// ```
///
/// Returns the char value or -1 in case of error and updates `*len` with
/// the number of bytes used.
///
/// # SAFETY
///
/// - `utf` must be NULL or point to a valid NUL-terminated byte sequence.
/// - `len` must be a valid `int *` or NULL.
#[no_mangle]
pub unsafe extern "C" fn xsltGetUTF8Char(utf: *const u8, len: *mut c_int) -> c_int {
    let mut c: u32;
    if utf.is_null() {
        return error_utf8(len);
    }
    if len.is_null() {
        return error_utf8(len);
    }
    if *len < 1 {
        return error_utf8(len);
    }

    c = *utf as u32;
    if c & 0x80 != 0 {
        if *len < 2 {
            return error_utf8(len);
        }
        if *utf.add(1) & 0xc0 != 0x80 {
            return error_utf8(len);
        }
        if c & 0xe0 == 0xe0 {
            if *len < 3 {
                return error_utf8(len);
            }
            if *utf.add(2) & 0xc0 != 0x80 {
                return error_utf8(len);
            }
            if c & 0xf0 == 0xf0 {
                if *len < 4 {
                    return error_utf8(len);
                }
                if c & 0xf8 != 0xf0 || *utf.add(3) & 0xc0 != 0x80 {
                    return error_utf8(len);
                }
                *len = 4;
                /* 4-byte code */
                c = ((*utf & 0x7) as u32) << 18;
                c |= ((*utf.add(1) & 0x3f) as u32) << 12;
                c |= ((*utf.add(2) & 0x3f) as u32) << 6;
                c |= (*utf.add(3) & 0x3f) as u32;
            } else {
                /* 3-byte code */
                *len = 3;
                c = ((*utf & 0xf) as u32) << 12;
                c |= ((*utf.add(1) & 0x3f) as u32) << 6;
                c |= (*utf.add(2) & 0x3f) as u32;
            }
        } else {
            /* 2-byte code */
            *len = 2;
            c = ((*utf & 0x1f) as u32) << 6;
            c |= (*utf.add(1) & 0x3f) as u32;
        }
    } else {
        /* 1-byte code */
        *len = 1;
    }
    c as c_int
}

/// The `error:` label of upstream `xsltGetUTF8Char`.
unsafe fn error_utf8(len: *mut c_int) -> c_int {
    if !len.is_null() {
        *len = 0;
    }
    -1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml::string::xml_strdup;
    use crate::xml::tree::*;

    fn cstr(s: &[u8]) -> *const xmlChar {
        let mut v = s.to_vec();
        v.push(0);
        // Leaked for the duration of the test process; the call sites pass
        // fixed literals only.
        let leaked: &'static [u8] = Box::leak(v.into_boxed_slice());
        leaked.as_ptr() as *const xmlChar
    }

    fn cstr_mut(s: &[u8]) -> *mut xmlChar {
        cstr(s) as *mut xmlChar
    }

    fn free_str(s: *mut xmlChar) {
        if !s.is_null() {
            unsafe { xmlFreeImpl(s as *mut c_void) };
        }
    }

    /// A minimal transform context wired to an XPath context, for
    /// exercising the AVT evaluators.
    unsafe fn make_ctxt(doc: *mut _xmlDoc, node: *mut _xmlNode) -> *mut _xsltTransformContext {
        let ctxt = libc::calloc(1, core::mem::size_of::<_xsltTransformContext>())
            as *mut _xsltTransformContext;
        assert!(!ctxt.is_null());
        (*ctxt).xpathCtxt = xmlXPathNewContext(doc);
        assert!(!(*ctxt).xpathCtxt.is_null());
        (*ctxt).node = node;
        if !doc.is_null() {
            let docu = libc::calloc(1, core::mem::size_of::<_xsltDocument>()) as *mut _xsltDocument;
            (*docu).doc = doc;
            (*docu).main = 1;
            (*ctxt).document = docu;
        }
        ctxt
    }

    unsafe fn free_ctxt(ctxt: *mut _xsltTransformContext) {
        if !(*ctxt).document.is_null() {
            (*(*ctxt).document).doc = ptr::null_mut();
            libc::free((*ctxt).document as *mut libc::c_void);
        }
        if !(*ctxt).xpathCtxt.is_null() {
            xmlXPathFreeContext((*ctxt).xpathCtxt);
        }
        libc::free(ctxt as *mut libc::c_void);
    }

    #[test]
    fn test_is_blank() {
        unsafe {
            assert_eq!(xsltIsBlank(ptr::null_mut()), 1);
            assert_eq!(xsltIsBlank(cstr_mut(b"")), 1);
            assert_eq!(xsltIsBlank(cstr_mut(b" \t\r\n ")), 1);
            assert_eq!(xsltIsBlank(cstr_mut(b" a")), 0);
        }
    }

    #[test]
    fn test_get_utf8_char() {
        unsafe {
            let mut len: c_int = 1;
            // ASCII.
            assert_eq!(xsltGetUTF8Char(b"A".as_ptr(), &mut len), 0x41);
            assert_eq!(len, 1);
            // 2-byte: U+00E9 é.
            let two = [0xC3u8, 0xA9];
            len = 2;
            assert_eq!(xsltGetUTF8Char(two.as_ptr(), &mut len), 0xE9);
            assert_eq!(len, 2);
            // 3-byte: U+20AC €.
            let three = [0xE2u8, 0x82, 0xAC];
            len = 3;
            assert_eq!(xsltGetUTF8Char(three.as_ptr(), &mut len), 0x20AC);
            assert_eq!(len, 3);
            // 4-byte: U+1F600.
            let four = [0xF0u8, 0x9F, 0x98, 0x80];
            len = 4;
            assert_eq!(xsltGetUTF8Char(four.as_ptr(), &mut len), 0x1F600);
            assert_eq!(len, 4);
            // Invalid: truncated.
            let bad = [0xC3u8];
            len = 1;
            assert_eq!(xsltGetUTF8Char(bad.as_ptr(), &mut len), -1);
            assert_eq!(len, 0);
            // Invalid continuation byte.
            let bad2 = [0xC3u8, 0x41];
            len = 2;
            assert_eq!(xsltGetUTF8Char(bad2.as_ptr(), &mut len), -1);
            assert_eq!(len, 0);
            // NULL input.
            assert_eq!(xsltGetUTF8Char(ptr::null(), ptr::null_mut()), -1);
        }
    }

    #[test]
    fn test_split_qname() {
        unsafe {
            let dict = xmlDictCreate();
            assert!(!dict.is_null());
            let mut prefix: *const xmlChar = ptr::null();
            let local = xsltSplitQName(dict, cstr(b"p:local"), &mut prefix);
            assert!(!local.is_null());
            assert!(!prefix.is_null());
            assert_eq!(
                libc::strcmp(local as *const c_char, c"local".as_ptr() as *const c_char),
                0
            );
            assert_eq!(
                libc::strcmp(prefix as *const c_char, c"p".as_ptr() as *const c_char),
                0
            );
            // No colon: local only, prefix NULL.
            prefix = cstr(b"x");
            let local2 = xsltSplitQName(dict, cstr(b"plain"), &mut prefix);
            assert!(!local2.is_null());
            assert!(prefix.is_null());
            // NULL checks.
            assert!(xsltSplitQName(dict, ptr::null(), &mut prefix).is_null());
            assert!(xsltSplitQName(ptr::null_mut(), cstr(b"a:b"), &mut prefix).is_null());
            xmlDictFree(dict);
        }
    }

    #[test]
    fn test_attr_template_value_process() {
        unsafe {
            let doc = new_doc(ptr::null());
            let root = new_node(ptr::null_mut(), cstr(b"root"));
            add_child(doc as *mut _xmlNode, root);
            let item = new_node(ptr::null_mut(), cstr(b"item"));
            add_child(root, item);
            set_prop(item, cstr(b"n"), cstr(b"7"));
            let ctxt = make_ctxt(doc, item);

            // Plain string passthrough.
            let r1 = xsltAttrTemplateValueProcess(ctxt, cstr(b"hello"));
            assert!(!r1.is_null());
            assert_eq!(
                libc::strcmp(r1 as *const c_char, c"hello".as_ptr() as *const c_char),
                0
            );
            free_str(r1);
            // Embedded expression against the context node.
            let r2 = xsltAttrTemplateValueProcess(ctxt, cstr(b"item-{@n}"));
            assert!(!r2.is_null());
            assert_eq!(
                libc::strcmp(r2 as *const c_char, c"item-7".as_ptr() as *const c_char),
                0
            );
            free_str(r2);
            // Escaped braces.
            let r3 = xsltAttrTemplateValueProcess(ctxt, cstr(b"{{x}}y}}z"));
            assert!(!r3.is_null());
            assert_eq!(
                libc::strcmp(r3 as *const c_char, c"{x}y}z".as_ptr() as *const c_char),
                0
            );
            free_str(r3);
            // Empty string yields an allocated empty string.
            let r4 = xsltAttrTemplateValueProcess(ctxt, cstr(b""));
            assert!(!r4.is_null());
            assert_eq!(*r4, 0);
            free_str(r4);
            // NULL string yields NULL.
            assert!(xsltAttrTemplateValueProcess(ctxt, ptr::null()).is_null());

            free_ctxt(ctxt);
            free_doc(doc);
        }
    }

    #[test]
    fn test_get_ns_prop() {
        unsafe {
            let doc = new_doc(ptr::null());
            let root = new_node(ptr::null_mut(), cstr(b"root"));
            add_child(doc as *mut _xmlNode, root);
            let ns = new_ns(root, cstr(b"urn:test"), cstr(b"t"));
            (*root).ns = ns;
            set_ns_prop(root, ns, cstr(b"attr"), cstr(b"v"));
            // Namespace-anchored attribute.
            let v = xsltGetNsProp(root, cstr(b"attr"), cstr(b"urn:test"));
            assert!(!v.is_null());
            assert_eq!(
                libc::strcmp(v as *const c_char, c"v".as_ptr() as *const c_char),
                0
            );
            free_str(v);
            // Wrong namespace: not found.
            assert!(xsltGetNsProp(root, cstr(b"attr"), cstr(b"urn:other")).is_null());
            // No namespace requested: plain name lookup.
            let v2 = xsltGetNsProp(root, cstr(b"attr"), ptr::null());
            assert!(!v2.is_null());
            free_str(v2);
            free_doc(doc);
        }
    }

    #[test]
    fn test_eval_static_attr_value_template() {
        unsafe {
            let style =
                libc::calloc(1, core::mem::size_of::<_xsltStylesheet>()) as *mut _xsltStylesheet;
            (*style).dict = xmlDictCreate();
            let inst = new_node(ptr::null_mut(), cstr(b"elem"));
            set_prop(inst, cstr(b"a"), cstr(b"static"));
            set_prop(inst, cstr(b"b"), cstr(b"dyn-{x}"));
            let mut found: c_int = -1;
            let r1 =
                xsltEvalStaticAttrValueTemplate(style, inst, cstr(b"a"), ptr::null(), &mut found);
            assert_eq!(found, 1);
            assert!(!r1.is_null());
            assert_eq!(
                libc::strcmp(r1 as *const c_char, c"static".as_ptr() as *const c_char),
                0
            );
            // AVT: static check fails (returns NULL, *found == 1).
            let r2 =
                xsltEvalStaticAttrValueTemplate(style, inst, cstr(b"b"), ptr::null(), &mut found);
            assert_eq!(found, 1);
            assert!(r2.is_null());
            // Missing attribute: *found == 0.
            let r3 =
                xsltEvalStaticAttrValueTemplate(style, inst, cstr(b"zzz"), ptr::null(), &mut found);
            assert_eq!(found, 0);
            assert!(r3.is_null());
            xmlDictFree((*style).dict);
            crate::xml::tree::free_node_list(inst);
            libc::free(style as *mut libc::c_void);
        }
    }

    #[test]
    fn test_get_qname_uri() {
        unsafe {
            let doc = new_doc(ptr::null());
            let root = new_node(ptr::null_mut(), cstr(b"root"));
            add_child(doc as *mut _xmlNode, root);
            let ns = new_ns(root, cstr(b"urn:q"), cstr(b"p"));
            (*root).ns = ns;
            let name = xml_strdup(cstr(b"p:local"));
            let mut name_ptr = name;
            let uri = xsltGetQNameURI(root, &mut name_ptr);
            assert!(!uri.is_null());
            assert_eq!(
                libc::strcmp(uri as *const c_char, c"urn:q".as_ptr() as *const c_char),
                0
            );
            free_doc(doc);
        }
    }

    #[test]
    fn test_copy_namespace_and_text() {
        unsafe {
            let elem = new_node(ptr::null_mut(), cstr(b"out"));
            let ns = new_ns(ptr::null_mut(), cstr(b"urn:copy"), cstr(b"c"));
            let copy = xsltCopyNamespace(ptr::null_mut(), elem, ns);
            assert!(!copy.is_null());
            assert_eq!((*copy).context as *mut _xmlNode, elem);
            assert_eq!(
                libc::strcmp(
                    (*copy).href as *const c_char,
                    c"urn:copy".as_ptr() as *const c_char
                ),
                0
            );
            assert!(xsltCopyNamespace(ptr::null_mut(), elem, ptr::null_mut()).is_null());

            let ctxt = libc::calloc(1, core::mem::size_of::<_xsltTransformContext>())
                as *mut _xsltTransformContext;
            let text = xsltCopyTextString(ctxt, elem, cstr(b"text!"), 0);
            assert!(!text.is_null());
            assert_eq!((*text).type_, XML_TEXT_NODE as c_int);
            assert_eq!(
                libc::strcmp(
                    (*text).content as *const c_char,
                    c"text!".as_ptr() as *const c_char
                ),
                0
            );
            assert_eq!((*elem).last, text);
            let noenc = xsltCopyTextString(ctxt, elem, cstr(b"<raw>"), 1);
            assert!(!noenc.is_null());
            assert!(node_name_eq((*noenc).name, b"textnoenc"));
            assert_eq!(
                libc::strcmp(
                    (*noenc).content as *const c_char,
                    c"<raw>".as_ptr() as *const c_char
                ),
                0
            );
            // NULL string returns NULL.
            assert!(xsltCopyTextString(ctxt, elem, ptr::null(), 0).is_null());
            libc::free(ctxt as *mut libc::c_void);
            crate::xml::tree::free_node_list(elem);
            // The original ns is standalone (new_ns(NULL, ...)); freeing it
            // with free_node would misinterpret the _xmlNs layout (ASan
            // heap-use-after-free). Use the namespace free.
            crate::abi::exports_tree::xmlFreeNs(ns);
        }
    }
}
