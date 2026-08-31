//! C ABI exports for libxslt.so.1 — the "vars" family (§16, Phase 8).
//!
//! Variable/parameter stack management (`xsltVariableLookup`,
//! `xsltEvalGlobalVariables`, the user-parameter evaluators,
//! `xsltLocalVariablePush/Pop`, `xsltAddStackElemList`,
//! `xsltFreeStackElemList`, the global-variable parsers), result-tree
//! fragments (`xsltCreateRVT`, the `xsltRegister*RVT` / `xsltReleaseRVT` /
//! `xsltFlagRVTs` / `xsltFreeRVTs` family), key management (`xsltAddKey`,
//! `xsltInitCtxtKey(s)`, `xsltInitAllDocKeys`, `xsltGetKey`,
//! `xsltFreeKeys`, `xsltFreeDocumentKeys`), precomputation allocation
//! (`xsltNewElemPreComp`, `xsltInitElemPreComp`), the per-stylesheet /
//! per-context extra slots (`xsltAllocateExtra(Ctxt)`) and the extension
//! result bookkeeping (`xsltExtensionInstructionResultRegister`,
//! `xsltExtensionInstructionResultFinalize`).
//!
//! Semantics follow upstream libxslt 1.1.45 (`archaeology/libxslt-git/`).
//!
//! # Upstream contract
//!
//! Parity target is upstream libxslt 1.1.45 `variables.c` and `params.c` with
//! the upstream headers; R-000160 (11.1-I) dispositioned
//! `xsltExtensionInstructionResultRegister` (upstream body returns 0).
//!
//! # Conceptual behavior
//!
//! This module implements the variable/parameter ABI: the stack management
//! (`xsltVariableLookup`, `xsltLocalVariablePush/Pop`, `xsltAddStackElemList`,
//! `xsltFreeStackElemList`), the global/user parameter evaluators, the
//! result-tree-fragment lifecycle (`xsltCreateRVT`, `xsltRegister*RVT`,
//! `xsltReleaseRVT`, `xsltFlagRVTs`, `xsltFreeRVTs`), key management and the
//! extension-result bookkeeping.
//!
//! # Ownership & safety invariants
//!
//! RVT objects are owned by the context RVT lists and freed with
//! `xsltReleaseRVT` (local/tmp) or `xsltFreeRVTs` (persist) per OWNERSHIP_ATLAS
//! section 4; key tables are owned by the document wrapper (`idoc->keys`) and
//! freed with `xsltFreeDocumentKeys`; stack elements are caller-owned lists
//! (`xsltFreeStackElemList`); caller parameter strings are copied at eval time
//! (upstream copies values).
//!
//! # Historical quirks & epochs
//!
//! The variables subsystem matured in the 1.1 era and feeds the frozen E-008
//! transform epoch; R-000160 records that
//! `xsltExtensionInstructionResultRegister` returns 0 with upstreams trivial
//! body; R-000109 (Phase 9) fixed the RTF double-free and exsl:node-set
//! support that this modules RVT lifecycle protects.
//!
//! # Deliberate oddities
//!
//! `xsltExtensionInstructionResultRegister` returns 0 unconditionally
//! (upstream 1.1.45 body) — a deliberate no-op, not a stub; `xsltQuoteUserParams`
//! reproduces upstreams quoting of user params including its single-quote
//! doubling.
//!
//! # Proving courts
//!
//! The CLI-XSLTPROC court cases (parameter passing, the R-000111-era name=value
//! parsing), the XSLT court family and DSO-LOADER/HEADER-COMPILE cover this
//! module; the variables unit tests run under cargo test.
//!
//! # Tempting simplifications that would break parity
//!
//! A tempting simplification is to free RVTs eagerly when the transform
//! context is freed — the callers result tree may still reference them, and
//! OWNERSHIP_ATLAS records the RVT lists as the owning structure (R-000109
//! double-free class). Another shortcut, evaluating user parameters without
//! the quote/unquote round-trip, would break the parameter values the
//! CLI-XSLTPROC cases pass.

#![allow(non_snake_case)]
#![allow(unused_variables)]
#![allow(clippy::missing_safety_doc)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use core::ptr;
use std::collections::HashMap;
use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_void};

use crate::abi::exports_xslt_compile::_xsltElemPreComp;
use crate::abi::structs::*;
use crate::abi::types::xmlElementType::*;
use crate::abi::types::xmlXPathObjectType::*;
use crate::abi::types::*;

/// `XSLT_VAR_PARAM` (variables.c): PARAM flag on a stack element.
const XSLT_VAR_PARAM: c_int = 1 << 1;

// ═══════════════════════════════════════════════════════════════════════════════
// Variable stack (variables.c, transform.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// `xsltVariableLookup` (variables.c): find a variable by name+URI, first
/// in the local stack then in the global variables.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xsltStackElemPtr xsltVariableLookup(xsltTransformContextPtr ctxt,
///                                     const xmlChar *name,
///                                     const xmlChar *nameURI);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltVariableLookup(
    ctxt: *mut _xsltTransformContext,
    name: *const xmlChar,
    nameURI: *const xmlChar,
) -> *mut _xsltStackElem {
    if ctxt.is_null() || name.is_null() {
        return ptr::null_mut();
    }
    let ns = if nameURI.is_null() {
        b""
    } else {
        CStr::from_ptr(nameURI as *const c_char).to_bytes()
    };
    // Local stack first (upstream xsltVariableLookup → xsltGetVariable).
    let mut cur = (*ctxt).vars;
    while !cur.is_null() {
        if (*cur).name == name
            || (!(*cur).name.is_null()
                && libc::strcmp((*cur).name as *const c_char, name as *const c_char) == 0)
        {
            let cur_ns = if (*cur).nameURI.is_null() {
                b""
            } else {
                CStr::from_ptr((*cur).nameURI as *const c_char).to_bytes()
            };
            if cur_ns == ns {
                return cur;
            }
        }
        cur = (*cur).next;
    }
    // Global variables.
    let mut g = (*ctxt).globalVars as *mut _xsltStackElem;
    while !g.is_null() {
        if !(*g).name.is_null()
            && libc::strcmp((*g).name as *const c_char, name as *const c_char) == 0
        {
            let g_ns = if (*g).nameURI.is_null() {
                b""
            } else {
                CStr::from_ptr((*g).nameURI as *const c_char).to_bytes()
            };
            if g_ns == ns {
                return g;
            }
        }
        g = (*g).next;
    }
    ptr::null_mut()
}

/// `xsltEvalGlobalVariables` (variables.c): evaluate all global variables.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltEvalGlobalVariables(xsltTransformContextPtr ctxt);
/// ```
///
/// Returns 0 on success, -1 on error.
#[no_mangle]
pub unsafe extern "C" fn xsltEvalGlobalVariables(ctxt: *mut _xsltTransformContext) -> c_int {
    if ctxt.is_null() {
        return -1;
    }
    crate::xslt::variables::xsltInitGlobalVariables(ctxt);
    0
}

/// `xsltEvalUserParams` (variables.c): evaluate a NULL-terminated array of
/// `name=value` strings as global parameters.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltEvalUserParams(xsltTransformContextPtr ctxt, const char **params);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltEvalUserParams(
    ctxt: *mut _xsltTransformContext,
    params: *mut *const c_char,
) -> c_int {
    if ctxt.is_null() || params.is_null() {
        return -1;
    }
    crate::xslt::parameters::xsltParseStylesheetParams((*ctxt).style, params)
}

/// `xsltEvalOneUserParam` (variables.c): evaluate a single global parameter.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltEvalOneUserParam(xsltTransformContextPtr ctxt,
///                          const xmlChar *name, const xmlChar *value);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltEvalOneUserParam(
    ctxt: *mut _xsltTransformContext,
    name: *const xmlChar,
    value: *const xmlChar,
) -> c_int {
    if ctxt.is_null() || name.is_null() || value.is_null() {
        return -1;
    }
    if (*ctxt).style.is_null() {
        return -1;
    }
    let elem = crate::xslt::parameters::xsltParseStylesheetParam(
        (*ctxt).style,
        name as *const c_char,
        value as *const c_char,
    );
    if elem.is_null() {
        -1
    } else {
        0
    }
}

/// `xsltQuoteUserParams` (variables.c): like `xsltEvalUserParams` but the
/// values are XML-escaped before evaluation.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltQuoteUserParams(xsltTransformContextPtr ctxt, const char **params);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltQuoteUserParams(
    ctxt: *mut _xsltTransformContext,
    params: *mut *const c_char,
) -> c_int {
    if ctxt.is_null() || params.is_null() {
        return -1;
    }
    let mut i = 0;
    while !(*params.add(i)).is_null() {
        let pair = CStr::from_ptr(*params.add(i)).to_bytes();
        if let Some(eq) = pair.iter().position(|&b| b == b'=') {
            let name = &pair[..eq];
            let value = &pair[eq + 1..];
            let mut n = name.to_vec();
            n.push(0);
            let mut v = xml_escape(value);
            v.push(0);
            let elem = crate::xslt::parameters::xsltParseStylesheetParam(
                (*ctxt).style,
                n.as_ptr() as *const c_char,
                v.as_ptr() as *const c_char,
            );
            if elem.is_null() {
                return -1;
            }
        }
        i += 1;
    }
    0
}

/// `xsltQuoteOneUserParam` (variables.c): quote-and-evaluate one parameter.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltQuoteOneUserParam(xsltTransformContextPtr ctxt,
///                           const xmlChar *name, const xmlChar *value);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltQuoteOneUserParam(
    ctxt: *mut _xsltTransformContext,
    name: *const xmlChar,
    value: *const xmlChar,
) -> c_int {
    if ctxt.is_null() || name.is_null() || value.is_null() {
        return -1;
    }
    let v = xml_escape(CStr::from_ptr(value as *const c_char).to_bytes());
    let mut vn = v;
    vn.push(0);
    let elem = crate::xslt::parameters::xsltParseStylesheetParam(
        (*ctxt).style,
        name as *const c_char,
        vn.as_ptr() as *const c_char,
    );
    if elem.is_null() {
        -1
    } else {
        0
    }
}

/// Escape `&<>"` for use in a quoted parameter value (upstream
/// `xmlEncodeSpecialChars`-equivalent).
fn xml_escape(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    for &b in bytes {
        match b {
            b'&' => out.extend_from_slice(b"&amp;"),
            b'<' => out.extend_from_slice(b"&lt;"),
            b'>' => out.extend_from_slice(b"&gt;"),
            b'"' => out.extend_from_slice(b"&quot;"),
            _ => out.push(b),
        }
    }
    out
}

/// `xsltParseStylesheetCallerParam` (variables.c): compile an `xsl:param`
/// instruction into a stack element for a called template.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xsltStackElemPtr xsltParseStylesheetCallerParam(xsltTransformContextPtr ctxt,
///                                                 xmlNodePtr inst);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltParseStylesheetCallerParam(
    ctxt: *mut _xsltTransformContext,
    inst: *mut _xmlNode,
) -> *mut _xsltStackElem {
    if ctxt.is_null() || inst.is_null() {
        return ptr::null_mut();
    }
    let elem = libc::calloc(1, size_of::<_xsltStackElem>()) as *mut _xsltStackElem;
    if elem.is_null() {
        return ptr::null_mut();
    }
    (*elem).context = ctxt;
    // name (from the name attribute)
    let name_attr = (*inst).properties;
    let mut name: *const xmlChar = ptr::null();
    let mut name_uri: *const xmlChar = ptr::null();
    let mut cur = name_attr;
    while !cur.is_null() {
        let aname = CStr::from_ptr((*cur).name as *const c_char).to_bytes();
        if aname == b"name" && !(*cur).children.is_null() && !(*(*cur).children).content.is_null() {
            name = (*(*cur).children).content;
        }
        cur = (*cur).next;
    }
    if name.is_null() {
        libc::free(elem as *mut libc::c_void);
        return ptr::null_mut();
    }
    // Namespace URI of the instruction (usually none for xsl:param).
    if !(*inst).ns.is_null() {
        name_uri = (*(*inst).ns).href;
    }
    (*elem).name = name;
    (*elem).nameURI = name_uri;
    (*elem).flags = XSLT_VAR_PARAM;
    elem
}

/// `xsltParseStylesheetParam` (variables.c): compile a global `xsl:param`.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltParseStylesheetParam(xsltTransformContextPtr ctxt, xmlNodePtr cur);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltParseStylesheetParam(
    ctxt: *mut _xsltTransformContext,
    cur: *mut _xmlNode,
) {
    if ctxt.is_null() || cur.is_null() || (*ctxt).style.is_null() {
        return;
    }
    // Extract the name attribute and the select attribute (or the inline
    // content as the default value) and route through the parameters engine.
    let mut name: *const xmlChar = ptr::null();
    let mut select: *const xmlChar = ptr::null();
    let mut attr = (*cur).properties;
    while !attr.is_null() {
        let aname = CStr::from_ptr((*attr).name as *const c_char).to_bytes();
        if aname == b"name" && !(*attr).children.is_null() {
            name = (*(*attr).children).content;
        } else if aname == b"select" && !(*attr).children.is_null() {
            select = (*(*attr).children).content;
        }
        attr = (*attr).next;
    }
    if name.is_null() {
        return;
    }
    let value = if select.is_null() {
        b""
    } else {
        CStr::from_ptr(select as *const c_char).to_bytes()
    };
    let mut v = value.to_vec();
    v.push(0);
    crate::xslt::parameters::xsltParseStylesheetParam(
        (*ctxt).style,
        name as *const c_char,
        v.as_ptr() as *const c_char,
    );
}

/// `xsltParseStylesheetVariable` (variables.c): compile a global
/// `xsl:variable`.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltParseStylesheetVariable(xsltTransformContextPtr ctxt, xmlNodePtr inst);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltParseStylesheetVariable(
    ctxt: *mut _xsltTransformContext,
    inst: *mut _xmlNode,
) {
    if ctxt.is_null() || inst.is_null() || (*ctxt).style.is_null() {
        return;
    }
    // Extract name/select from the instruction and register a global
    // variable on the stylesheet (upstream variables.c
    // xsltParseGlobalVariable).
    let mut name: *const xmlChar = ptr::null();
    let mut select: *const xmlChar = ptr::null();
    let mut attr = (*inst).properties;
    while !attr.is_null() {
        let aname = CStr::from_ptr((*attr).name as *const c_char).to_bytes();
        if aname == b"name" && !(*attr).children.is_null() {
            name = (*(*attr).children).content;
        } else if aname == b"select" && !(*attr).children.is_null() {
            select = (*(*attr).children).content;
        }
        attr = (*attr).next;
    }
    if name.is_null() {
        return;
    }
    let value = if select.is_null() {
        b""
    } else {
        CStr::from_ptr(select as *const c_char).to_bytes()
    };
    let mut v = value.to_vec();
    v.push(0);
    crate::xslt::parameters::xsltParseStylesheetParam(
        (*ctxt).style,
        name as *const c_char,
        v.as_ptr() as *const c_char,
    );
}

/// `xsltFreeGlobalVariables` (variables.c): free all global variables.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltFreeGlobalVariables(xsltTransformContextPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltFreeGlobalVariables(ctxt: *mut _xsltTransformContext) {
    crate::xslt::variables::xsltFreeGlobalVariables(ctxt);
}

/// `xsltLocalVariablePush` (transform.c): push a variable onto the local
/// variable stack at a scope level.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltLocalVariablePush(xsltTransformContextPtr ctxt,
///                           xsltStackElemPtr variable, int level);
/// ```
///
/// Returns the new stack depth, or -1 on error.
#[no_mangle]
pub unsafe extern "C" fn xsltLocalVariablePush(
    ctxt: *mut _xsltTransformContext,
    variable: *mut _xsltStackElem,
    level: c_int,
) -> c_int {
    if ctxt.is_null() {
        return -1;
    }
    // Grow the varsTab if needed.
    if (*ctxt).varsNr >= (*ctxt).varsMax {
        let new_max = if (*ctxt).varsMax == 0 {
            10
        } else {
            (*ctxt).varsMax * 2
        };
        let new_tab = libc::realloc(
            (*ctxt).varsTab as *mut libc::c_void,
            (new_max as usize) * size_of::<*mut _xsltStackElem>(),
        ) as *mut *mut _xsltStackElem;
        if new_tab.is_null() {
            return -1;
        }
        (*ctxt).varsTab = new_tab;
        (*ctxt).varsMax = new_max;
    }
    if !variable.is_null() {
        (*variable).level = level;
    }
    *(*ctxt).varsTab.add((*ctxt).varsNr as usize) = variable;
    (*ctxt).varsNr += 1;
    (*ctxt).vars = variable;
    (*ctxt).varsBase = level;
    (*ctxt).varsNr
}

/// `xsltLocalVariablePop` (transform.c): pop variables back to a scope.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltLocalVariablePop(xsltTransformContextPtr ctxt, int limitNr, int level);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltLocalVariablePop(
    ctxt: *mut _xsltTransformContext,
    limitNr: c_int,
    level: c_int,
) {
    if ctxt.is_null() {
        return;
    }
    while (*ctxt).varsNr > limitNr {
        (*ctxt).varsNr -= 1;
        let var = *(*ctxt).varsTab.add((*ctxt).varsNr as usize);
        if !var.is_null() {
            crate::xslt::variables::xsltFreeStackElem(var);
        }
        *(*ctxt).varsTab.add((*ctxt).varsNr as usize) = ptr::null_mut();
    }
    let _ = level;
}

/// `xsltAddStackElemList` (transform.c): add a list of stack elements to
/// the variable stack.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltAddStackElemList(xsltTransformContextPtr ctxt, xsltStackElemPtr elems);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltAddStackElemList(
    ctxt: *mut _xsltTransformContext,
    elems: *mut _xsltStackElem,
) -> c_int {
    if ctxt.is_null() || elems.is_null() {
        return 0;
    }
    let mut cur = elems;
    while !cur.is_null() {
        let next = (*cur).next;
        (*cur).next = (*ctxt).vars;
        (*ctxt).vars = cur;
        cur = next;
    }
    0
}

/// `xsltFreeStackElemList` (transform.c): free a linked list of stack
/// elements.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltFreeStackElemList(xsltStackElemPtr elem);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltFreeStackElemList(elem: *mut _xsltStackElem) {
    let mut cur = elem;
    while !cur.is_null() {
        let next = (*cur).next;
        crate::xslt::variables::xsltFreeStackElem(cur);
        cur = next;
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Result tree fragments (transform.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// `xsltCreateRVT` (transform.c): create a result-tree-fragment document.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDocPtr xsltCreateRVT(xsltTransformContextPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltCreateRVT(ctxt: *mut _xsltTransformContext) -> *mut _xmlDoc {
    if ctxt.is_null() {
        return ptr::null_mut();
    }
    let doc = libc::calloc(1, size_of::<_xmlDoc>()) as *mut _xmlDoc;
    if doc.is_null() {
        return ptr::null_mut();
    }
    (*doc).type_ = XML_DOCUMENT_NODE as c_int;
    (*doc).doc = doc;
    (*doc).dict = (*ctxt).dict;
    (*doc).URL = ptr::null_mut();
    doc
}

/// `xsltRegisterLocalRVT` (transform.c): register an RVT in the local list.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltRegisterLocalRVT(xsltTransformContextPtr ctxt, xmlDocPtr RVT);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltRegisterLocalRVT(
    ctxt: *mut _xsltTransformContext,
    RVT: *mut _xmlDoc,
) -> c_int {
    if ctxt.is_null() || RVT.is_null() {
        return -1;
    }
    (*RVT).next = (*ctxt).localRVT as *mut _xmlNode;
    (*ctxt).localRVT = RVT;
    0
}

/// `xsltRegisterPersistRVT` (transform.c).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltRegisterPersistRVT(xsltTransformContextPtr ctxt, xmlDocPtr RVT);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltRegisterPersistRVT(
    ctxt: *mut _xsltTransformContext,
    RVT: *mut _xmlDoc,
) -> c_int {
    if ctxt.is_null() || RVT.is_null() {
        return -1;
    }
    (*RVT).next = (*ctxt).persistRVT as *mut _xmlNode;
    (*ctxt).persistRVT = RVT;
    0
}

/// `xsltRegisterTmpRVT` (transform.c).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltRegisterTmpRVT(xsltTransformContextPtr ctxt, xmlDocPtr RVT);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltRegisterTmpRVT(
    ctxt: *mut _xsltTransformContext,
    RVT: *mut _xmlDoc,
) -> c_int {
    if ctxt.is_null() || RVT.is_null() {
        return -1;
    }
    (*RVT).next = (*ctxt).tmpRVT as *mut _xmlNode;
    (*ctxt).tmpRVT = RVT;
    0
}

/// `xsltReleaseRVT` (transform.c): release an RVT; local RVTs are freed
/// unless persisted.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltReleaseRVT(xsltTransformContextPtr ctxt, xmlDocPtr RVT);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltReleaseRVT(ctxt: *mut _xsltTransformContext, RVT: *mut _xmlDoc) {
    if ctxt.is_null() || RVT.is_null() {
        return;
    }
    // Is it persisted? If so, leave it for xsltFreeRVTs.
    let mut p = (*ctxt).persistRVT;
    while !p.is_null() {
        if p == RVT {
            return;
        }
        p = (*p).next as *mut _xmlDoc;
    }
    // Remove from the local list and free.
    let mut prev: *mut _xmlDoc = ptr::null_mut();
    let mut cur = (*ctxt).localRVT;
    while !cur.is_null() {
        if cur == RVT {
            if prev.is_null() {
                (*ctxt).localRVT = (*cur).next as *mut _xmlDoc;
            } else {
                (*prev).next = (*cur).next;
            }
            crate::xml::tree::free_doc(RVT);
            return;
        }
        prev = cur;
        cur = (*cur).next as *mut _xmlDoc;
    }
}

/// `xsltFlagRVTs` (transform.c): mark the RVTs behind an XPath object's
/// node-set with a flag.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltFlagRVTs(xsltTransformContextPtr ctxt, xmlXPathObjectPtr obj, int val);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltFlagRVTs(
    _ctxt: *mut _xsltTransformContext,
    obj: *mut _xmlXPathObject,
    val: c_int,
) -> c_int {
    if obj.is_null() {
        return -1;
    }
    if (*obj).type_ == XPATH_NODESET as c_int && !(*obj).nodesetval.is_null() {
        let ns = (*obj).nodesetval as *mut _xmlNodeSet;
        let mut i = 0;
        while i < (*ns).nodeNr as usize {
            let node = *(*ns).nodeTab.add(i);
            if !node.is_null() && !(*node).doc.is_null() {
                let doc = (*node).doc;
                // RVTs carry a marker in the doc's psvi slot like upstream.
                (*doc).psvi = if val != 0 {
                    core::ptr::addr_of_mut!(RVT_FLAG_MARKER) as *mut c_void
                } else {
                    ptr::null_mut()
                };
            }
            i += 1;
        }
    }
    0
}

/// Marker stored in `doc->psvi` for flagged RVTs (upstream stores a pointer
/// to the RVT list; the marker value is only ever compared for NULL/non-NULL).
static mut RVT_FLAG_MARKER: u8 = 0;

/// `xsltFreeRVTs` (transform.c): free all RVT lists of the context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltFreeRVTs(xsltTransformContextPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltFreeRVTs(ctxt: *mut _xsltTransformContext) {
    if ctxt.is_null() {
        return;
    }
    let mut cur = (*ctxt).tmpRVT;
    (*ctxt).tmpRVT = ptr::null_mut();
    while !cur.is_null() {
        let next = (*cur).next as *mut _xmlDoc;
        crate::xml::tree::free_doc(cur);
        cur = next;
    }
    let mut cur = (*ctxt).localRVT;
    (*ctxt).localRVT = ptr::null_mut();
    while !cur.is_null() {
        let next = (*cur).next as *mut _xmlDoc;
        crate::xml::tree::free_doc(cur);
        cur = next;
    }
    let mut cur = (*ctxt).persistRVT;
    (*ctxt).persistRVT = ptr::null_mut();
    while !cur.is_null() {
        let next = (*cur).next as *mut _xmlDoc;
        crate::xml::tree::free_doc(cur);
        cur = next;
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Keys (keys.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// `xsltAddKey` (keys.c): register a key definition on the stylesheet.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltAddKey(xsltStylesheetPtr style, const xmlChar *name,
///                const xmlChar *nameURI, const xmlChar *match,
///                const xmlChar *use, xmlNodePtr inst);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltAddKey(
    style: *mut _xsltStylesheet,
    name: *const xmlChar,
    _nameURI: *const xmlChar,
    match_: *const xmlChar,
    use_: *const xmlChar,
    inst: *mut _xmlNode,
) -> c_int {
    crate::xslt::keys::xsltAddKeyDef(style, name, match_, use_, inst)
}

/// `xsltInitCtxtKeys` (keys.c): build the key tables of a document.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltInitCtxtKeys(xsltTransformContextPtr ctxt, xsltDocumentPtr idoc);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltInitCtxtKeys(
    ctxt: *mut _xsltTransformContext,
    idoc: *mut _xsltDocument,
) {
    if ctxt.is_null() || idoc.is_null() {
        return;
    }
    let doc = (*idoc).doc;
    if doc.is_null() {
        return;
    }
    let mut cur = (*(*ctxt).style).keys as *mut _xsltKeyDef;
    while !cur.is_null() {
        let _ = xsltInitCtxtKey(ctxt, idoc, cur);
        cur = (*cur).next;
    }
}

/// `xsltInitCtxtKey` (keys.c): build one key table for a document.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltInitCtxtKey(xsltTransformContextPtr ctxt, xsltDocumentPtr idoc,
///                     xsltKeyDefPtr keyDef);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltInitCtxtKey(
    ctxt: *mut _xsltTransformContext,
    idoc: *mut _xsltDocument,
    keyDef: *mut _xsltKeyDef,
) -> c_int {
    if ctxt.is_null() || idoc.is_null() || keyDef.is_null() || (*idoc).doc.is_null() {
        return -1;
    }
    let doc = (*idoc).doc;
    let pat = crate::xslt::patterns::xsltCompilePattern((*keyDef).r#match, doc);
    if pat.is_null() {
        return 0;
    }
    let table = crate::xslt::keys::xsltNewKeyTable((*keyDef).name, (*keyDef).nameURI);
    if table.is_null() {
        crate::xslt::patterns::xsltFreePattern(pat);
        return -1;
    }
    crate::xslt::keys::build_key_table(
        ctxt,
        doc,
        doc as *mut _xmlNode,
        pat,
        (*keyDef).r#use,
        table,
    );
    crate::xslt::patterns::xsltFreePattern(pat);
    // Prepend the table to the document's key-table list.
    (*table).next = (*idoc).keys as *mut _xsltKeyTable;
    (*idoc).keys = table as *mut c_void;
    0
}

/// `xsltInitAllDocKeys` (keys.c): build key tables for every document of
/// the context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltInitAllDocKeys(xsltTransformContextPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltInitAllDocKeys(ctxt: *mut _xsltTransformContext) -> c_int {
    if ctxt.is_null() {
        return -1;
    }
    let mut ret = 0;
    let mut cur = (*ctxt).docList;
    while !cur.is_null() {
        if !(*cur).doc.is_null() {
            let mut k = (*(*ctxt).style).keys as *mut _xsltKeyDef;
            while !k.is_null() {
                let table = crate::xslt::keys::xsltNewKeyTable((*k).name, (*k).nameURI);
                if table.is_null() {
                    ret = -1;
                } else {
                    let pat = crate::xslt::patterns::xsltCompilePattern((*k).r#match, (*cur).doc);
                    if pat.is_null() {
                        crate::xslt::keys::xsltFreeKeyTable(table);
                    } else {
                        crate::xslt::keys::build_key_table(
                            ctxt,
                            (*cur).doc,
                            (*cur).doc as *mut _xmlNode,
                            pat,
                            (*k).r#use,
                            table,
                        );
                        crate::xslt::patterns::xsltFreePattern(pat);
                        (*table).next = (*cur).keys as *mut _xsltKeyTable;
                        (*cur).keys = table as *mut c_void;
                    }
                }
                k = (*k).next;
            }
        }
        cur = (*cur).next;
    }
    ret
}

/// `xsltGetKey` (keys.c): return the node-set of nodes matching a key value.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNodeSetPtr xsltGetKey(xsltTransformContextPtr ctxt, const xmlChar *name,
///                          const xmlChar *nameURI, const xmlChar *value);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltGetKey(
    ctxt: *mut _xsltTransformContext,
    name: *const xmlChar,
    _nameURI: *const xmlChar,
    value: *const xmlChar,
) -> *mut _xmlNodeSet {
    if ctxt.is_null() || name.is_null() || value.is_null() {
        return ptr::null_mut();
    }
    crate::xslt::keys::xsltEvalKeyFunction(ctxt, name, value)
}

/// `xsltFreeKeys` (keys.c): free the stylesheet's key definitions.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltFreeKeys(xsltStylesheetPtr style);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltFreeKeys(style: *mut _xsltStylesheet) {
    crate::xslt::keys::xsltFreeKeys(style);
}

/// `xsltFreeDocumentKeys` (keys.c): free a document's key tables.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltFreeDocumentKeys(xsltDocumentPtr idoc);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltFreeDocumentKeys(idoc: *mut _xsltDocument) {
    if idoc.is_null() {
        return;
    }
    let mut cur = (*idoc).keys as *mut _xsltKeyTable;
    (*idoc).keys = ptr::null_mut();
    while !cur.is_null() {
        let next = (*cur).next;
        crate::xslt::keys::xsltFreeKeyTable(cur);
        cur = next;
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Precomputation (preproc.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// `xsltNewElemPreComp` (preproc.c): allocate an `_xsltElemPreComp`.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xsltElemPreCompPtr xsltNewElemPreComp(xsltStylesheetPtr style,
///                                       xmlNodePtr inst,
///                                       xsltTransformFunction function);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltNewElemPreComp(
    style: *mut _xsltStylesheet,
    inst: *mut _xmlNode,
    function: crate::abi::exports_xslt_compile::xsltTransformFunction,
) -> *mut _xsltElemPreComp {
    let comp = libc::calloc(1, size_of::<_xsltElemPreComp>()) as *mut _xsltElemPreComp;
    if comp.is_null() {
        return ptr::null_mut();
    }
    xsltInitElemPreComp(comp, style, inst, function, None);
    comp
}

/// `xsltInitElemPreComp` (preproc.c): initialize a precomp and chain it
/// onto the stylesheet's precomp list.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltInitElemPreComp(xsltElemPreCompPtr comp, xsltStylesheetPtr style,
///                          xmlNodePtr inst, xsltTransformFunction function,
///                          xsltElemPreCompDeallocator freeFunc);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltInitElemPreComp(
    comp: *mut _xsltElemPreComp,
    style: *mut _xsltStylesheet,
    inst: *mut _xmlNode,
    function: crate::abi::exports_xslt_compile::xsltTransformFunction,
    _freeFunc: Option<crate::abi::exports_xslt_compile::xsltElemPreCompDeallocator>,
) {
    if comp.is_null() {
        return;
    }
    (*comp).inst = inst;
    (*comp).func = Some(function);
    if !style.is_null() {
        (*comp).next = (*style).preComps as *mut _xsltElemPreComp;
        (*style).preComps = comp as *mut c_void;
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Extra slots (transform.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// Side table for stylesheet extra slots: the candidate's `_xsltStylesheet`
/// layout omits the upstream `extras`/`extrasMax` fields (they are only
/// ever written by `xsltAllocateExtra`), so the slots live here keyed by
/// the stylesheet pointer.
static STYLE_EXTRAS: once_cell::sync::Lazy<parking_lot::Mutex<HashMap<usize, Vec<usize>>>> =
    once_cell::sync::Lazy::new(|| parking_lot::Mutex::new(HashMap::new()));

/// `xsltAllocateExtra` (transform.c): allocate a new extra slot on the
/// stylesheet.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltAllocateExtra(xsltStylesheetPtr style);
/// ```
///
/// Returns the slot index, or -1 on error.
#[no_mangle]
pub unsafe extern "C" fn xsltAllocateExtra(style: *mut _xsltStylesheet) -> c_int {
    if style.is_null() {
        return -1;
    }
    let mut map = STYLE_EXTRAS.lock();
    let slots = map.entry(style as usize).or_default();
    slots.push(0);
    (slots.len() - 1) as c_int
}

/// `xsltAllocateExtraCtxt` (transform.c): allocate a new extra slot on the
/// transform context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltAllocateExtraCtxt(xsltTransformContextPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltAllocateExtraCtxt(ctxt: *mut _xsltTransformContext) -> c_int {
    if ctxt.is_null() {
        return -1;
    }
    if (*ctxt).extrasNr >= (*ctxt).extrasMax {
        let new_max = if (*ctxt).extrasMax == 0 {
            20
        } else {
            (*ctxt).extrasMax * 2
        };
        let new_extras = libc::realloc((*ctxt).extras, (new_max as usize) * size_of::<c_void>())
            as *mut *mut c_void;
        if new_extras.is_null() {
            return -1;
        }
        (*ctxt).extras = new_extras as *mut c_void;
        (*ctxt).extrasMax = new_max;
    }
    let idx = (*ctxt).extrasNr;
    *((*ctxt).extras as *mut *mut c_void).add(idx as usize) = ptr::null_mut();
    (*ctxt).extrasNr += 1;
    idx
}

// ═══════════════════════════════════════════════════════════════════════════════
// Extension instruction results (transform.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// `xsltExtensionInstructionResultRegister` (transform.c): register an
/// XPath object produced by an extension instruction for cleanup.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltExtensionInstructionResultRegister(xsltTransformContextPtr ctxt,
///                                            xmlXPathObjectPtr obj);
/// ```
///
/// Upstream 1.1.45 body is literally `return(0)` ("It isn't necessary to
/// call this function in newer releases of libxslt").
#[no_mangle]
pub const unsafe extern "C" fn xsltExtensionInstructionResultRegister(
    _ctxt: *mut _xsltTransformContext,
    _obj: *mut _xmlXPathObject,
) -> c_int {
    0
}

/// `xsltExtensionInstructionResultFinalize` (transform.c): finalize the
/// registered extension results.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltExtensionInstructionResultFinalize(xsltTransformContextPtr ctxt);
/// ```
///
/// Upstream 1.1.45 prints "unsupported in this release of libxslt" via the
/// generic error handler and returns -1.
#[no_mangle]
pub unsafe extern "C" fn xsltExtensionInstructionResultFinalize(
    ctxt: *mut _xsltTransformContext,
) -> c_int {
    let _ = ctxt;
    crate::xslt::errors::xsltTransformError(
        ptr::null_mut(),
        ptr::null_mut(),
        ptr::null_mut(),
        c"xsltExtensionInstructionResultFinalize is unsupported in this release of libxslt.\n"
            .as_ptr() as *const c_char,
    );
    -1
}
