//! XPath / XPointer C export bridge (§25, 11.1-I XPath family closure).
//!
//! Implements the remaining `xmlXPath*` / `xmlXPtr*` C ABI surface over the
//! internal Rust XPath engine. The bridge converts between the C ABI
//! `_xmlXPathObject` / `_xmlNodeSet` representation and the internal
//! `XPathValue` / `NodeSet` model, and provides the parser-context stack
//! (upstream `xmlXPathParserContext`) that the exported core-function
//! implementations operate on.
//!
//! UPSTREAM-PARITY notes are recorded per function; behaviors verified by the
//! XPATH-001 differential court.
//!
//! # Upstream contract
//!
//! Mirrors upstream `xpath.c` / `xpathInternals.h`
//! (`SRC-LIBXML2-2.15.0-XPATH-C`, parity target libxml2 2.15.3 oracle):
//! the exported `xmlXPath*` / `xmlXPtr*` surface — object constructors and
//! wrappers (xmlXPathNewString, xmlXPathWrapNodeSet, ...), the value-stack
//! push/pop functions and the compiled-expression entry points
//! (xmlXPathCtxtCompile, xmlXPathCompiledEval).
//!
//! # Conceptual behavior
//!
//! Bridges the C ABI `_xmlXPathObject` / `_xmlNodeSet` representation to
//! the internal `XPathValue` / `NodeSet` model and provides the
//! parser-context value stack that exported core-function implementations
//! operate on. Object storage is xmlMalloc'd C-layout; the bridge converts
//! both directions on every crossing.
//!
//! # Ownership & safety invariants
//!
//! Objects returned to C are owned by the caller (freed with
//! `xmlXPathFreeObject`); wrapped strings/node-sets transfer ownership of
//! the backing storage, not the tree nodes. `dup_rust_string` must NOT use
//! xml_strdup on a Rust String as_ptr (not NUL-terminated) — the R-000169
//! heap-buffer-overflow fix is visible here and in pop_string.
//!
//! # Historical quirks & epochs
//!
//! This bridge was built during the 11.1-I XPath family closure
//! (R-000126 header surface, R-000128 struct widths) and hardened in
//! 11.1-X (R-000169 xml_strndup ownership fix); the target is the 2.15.3
//! oracle epoch.
//!
//! # Deliberate oddities
//!
//! Wrapping functions take ownership of caller storage exactly like
//! upstream (xmlXPathWrapString adopts the xmlChar*), including on the
//! error path where upstream frees the input.
//!
//! # Proving courts
//!
//! XPATH-001 (courts/suites/data-abi/xpath-family-probe.c) exercises the
//! object/wrapper surface byte-identical against the oracle DSO; cargo
//! test covers the bridge unit suites.
//!
//! # Tempting simplifications that would break parity
//!
//! Do not copy strings with xml_strdup on Rust String pointers: that is
//! the R-000169 heap-buffer-overflow. Do not return internal Rust values
//! directly as _xmlXPathObject: C consumers read the C-layout fields.

#![allow(
    missing_docs,
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals
)]

// SAFETY-SCOPE: EXPORTS-MECHANICAL-001
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
use std::os::raw::{c_char, c_double, c_int, c_long};

use crate::abi::allocator::{xmlFreeImpl, xmlMallocImpl, xmlMallocZero};
use crate::abi::structs::{
    _xmlDoc, _xmlNode, _xmlNodeSet, _xmlNs, _xmlXPathContext, _xmlXPathObject,
};
use crate::abi::types::{xmlChar, xmlXPathObjectType};
use crate::xml::string::xml_strdup;
use crate::xml::xpath::types::{node_string_value, NodeSet, XPathValue};

/// Number formatting for XPath (upstream xmlXPathCastNumberToString).
fn number_to_xmlstring(val: c_double) -> *mut xmlChar {
    let s = crate::xml::xpath::types::number_to_string(val);
    dup_rust_string(&s)
}

/// Copy a Rust string into a NUL-terminated xmlChar buffer (xmlMalloc'd).
/// NOTE: `s.as_bytes().as_ptr()` is *not* NUL-terminated, so xml_strdup
/// cannot be used on it directly.
fn dup_rust_string(s: &str) -> *mut xmlChar {
    let bytes = s.as_bytes();
    let buf = unsafe { xmlMallocImpl(bytes.len() + 1) } as *mut xmlChar;
    if buf.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        ptr::copy_nonoverlapping(bytes.as_ptr(), buf, bytes.len());
        *buf.add(bytes.len()) = 0;
    }
    buf
}

// ═══════════════════════════════════════════════════════════════════════════════
// Object construction / wrapping
// ═══════════════════════════════════════════════════════════════════════════════

/// `xmlXPathObjectPtr xmlXPathNewString(const xmlChar *val)`.
///
/// # SAFETY
///
/// - `val` must be a valid NUL-terminated string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathNewString(val: *const xmlChar) -> *mut _xmlXPathObject {
    let obj = xmlMallocZero(size_of::<_xmlXPathObject>()) as *mut _xmlXPathObject;
    if obj.is_null() {
        return ptr::null_mut();
    }
    (*obj).type_ = xmlXPathObjectType::XPATH_STRING as c_int;
    (*obj).stringval = if val.is_null() {
        xml_strdup(c"".as_ptr() as *const xmlChar)
    } else {
        xml_strdup(val)
    };
    obj
}

/// `xmlXPathObjectPtr xmlXPathNewValueTree(xmlNodePtr val)` — a node-set object
/// containing a single node whose subtree is owned by the object.
///
/// # SAFETY
///
/// - `val` must be a valid node pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathNewValueTree(val: *mut _xmlNode) -> *mut _xmlXPathObject {
    let obj = xmlMallocZero(size_of::<_xmlXPathObject>()) as *mut _xmlXPathObject;
    if obj.is_null() {
        return ptr::null_mut();
    }
    (*obj).type_ = xmlXPathObjectType::XPATH_XSLT_TREE as c_int;
    let ns = xmlMallocZero(size_of::<_xmlNodeSet>()) as *mut _xmlNodeSet;
    if ns.is_null() {
        xmlFreeImpl(obj as *mut c_void);
        return ptr::null_mut();
    }
    (*ns).nodeNr = 0;
    (*ns).nodeMax = 1;
    let tab = xmlMallocImpl(size_of::<*mut _xmlNode>()) as *mut *mut _xmlNode;
    if tab.is_null() {
        xmlFreeImpl(ns as *mut c_void);
        xmlFreeImpl(obj as *mut c_void);
        return ptr::null_mut();
    }
    if val.is_null() {
        (*ns).nodeNr = 0;
        (*ns).nodeMax = 0;
        xmlFreeImpl(tab as *mut c_void);
        (*ns).nodeTab = ptr::null_mut();
    } else {
        ptr::write(tab, val);
        (*ns).nodeTab = tab;
        (*ns).nodeNr = 1;
    }
    (*obj).nodesetval = ns as *mut c_void;
    obj
}

/// `xmlXPathObjectPtr xmlXPathNewNodeSetList(xmlNodeSetPtr val)` — a node-set
/// object that COPIES the given node set.
///
/// # SAFETY
///
/// - `val` must be a valid node-set pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathNewNodeSetList(val: *mut _xmlNodeSet) -> *mut _xmlXPathObject {
    let obj = xmlMallocZero(size_of::<_xmlXPathObject>()) as *mut _xmlXPathObject;
    if obj.is_null() {
        return ptr::null_mut();
    }
    (*obj).type_ = xmlXPathObjectType::XPATH_NODESET as c_int;
    if val.is_null() {
        (*obj).nodesetval = ptr::null_mut();
        return obj;
    }
    let src = &*val;
    let nr = src.nodeNr;
    let ns = xmlMallocZero(size_of::<_xmlNodeSet>()) as *mut _xmlNodeSet;
    if ns.is_null() {
        xmlFreeImpl(obj as *mut c_void);
        return ptr::null_mut();
    }
    (*ns).nodeNr = nr;
    (*ns).nodeMax = nr;
    if nr > 0 && !src.nodeTab.is_null() {
        let tab = xmlMallocImpl((nr as usize) * size_of::<*mut _xmlNode>()) as *mut *mut _xmlNode;
        if tab.is_null() {
            xmlFreeImpl(ns as *mut c_void);
            xmlFreeImpl(obj as *mut c_void);
            return ptr::null_mut();
        }
        ptr::copy_nonoverlapping(src.nodeTab, tab, nr as usize);
        (*ns).nodeTab = tab;
    } else {
        (*ns).nodeTab = ptr::null_mut();
    }
    (*obj).nodesetval = ns as *mut c_void;
    obj
}

/// `xmlXPathObjectPtr xmlXPathWrapString(xmlChar *val)` — wraps a string,
/// TAKING OWNERSHIP of `val`.
///
/// # SAFETY
///
/// - `val` must be a heap-allocated NUL-terminated string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathWrapString(val: *mut xmlChar) -> *mut _xmlXPathObject {
    let obj = xmlMallocZero(size_of::<_xmlXPathObject>()) as *mut _xmlXPathObject;
    if obj.is_null() {
        if !val.is_null() {
            xmlFreeImpl(val as *mut c_void);
        }
        return ptr::null_mut();
    }
    (*obj).type_ = xmlXPathObjectType::XPATH_STRING as c_int;
    (*obj).stringval = val;
    obj
}

/// `xmlXPathObjectPtr xmlXPathWrapCString(char *val)`.
///
/// # SAFETY
///
/// - `val` must be a heap-allocated NUL-terminated string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathWrapCString(val: *mut c_char) -> *mut _xmlXPathObject {
    unsafe { xmlXPathWrapString(val as *mut xmlChar) }
}

/// `xmlXPathObjectPtr xmlXPathWrapNodeSet(xmlNodeSetPtr val)` — wraps a node
/// set, TAKING OWNERSHIP.
///
/// # SAFETY
///
/// - `val` must be a heap-allocated node set or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathWrapNodeSet(val: *mut _xmlNodeSet) -> *mut _xmlXPathObject {
    let obj = xmlMallocZero(size_of::<_xmlXPathObject>()) as *mut _xmlXPathObject;
    if obj.is_null() {
        if !val.is_null() {
            xmlFreeImpl(val as *mut c_void);
        }
        return ptr::null_mut();
    }
    (*obj).type_ = xmlXPathObjectType::XPATH_NODESET as c_int;
    (*obj).nodesetval = val as *mut c_void;
    obj
}

/// `xmlXPathObjectPtr xmlXPathWrapExternal(void *val)`.
///
/// # SAFETY
///
/// - `val` must be a valid pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathWrapExternal(val: *mut c_void) -> *mut _xmlXPathObject {
    let obj = xmlMallocZero(size_of::<_xmlXPathObject>()) as *mut _xmlXPathObject;
    if obj.is_null() {
        return ptr::null_mut();
    }
    (*obj).type_ = xmlXPathObjectType::XPATH_USERS as c_int;
    (*obj).user = val;
    obj
}

/// `void xmlXPathFreeNodeSetList(xmlXPathObjectPtr obj)` — frees a node-set
/// typed object and its node set.
///
/// # SAFETY
///
/// - `obj` must be a valid object pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathFreeNodeSetList(obj: *mut _xmlXPathObject) {
    if obj.is_null() {
        return;
    }
    let typ = (*obj).type_;
    if typ == xmlXPathObjectType::XPATH_NODESET as c_int
        || typ == xmlXPathObjectType::XPATH_XSLT_TREE as c_int
    {
        let ns = (*obj).nodesetval as *mut _xmlNodeSet;
        if !ns.is_null() {
            if !(*ns).nodeTab.is_null() {
                xmlFreeImpl((*ns).nodeTab as *mut c_void);
            }
            xmlFreeImpl(ns as *mut c_void);
        }
    }
    xmlFreeImpl(obj as *mut c_void);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Conversion (in-place, upstream semantics: the old object is freed unless the
// type already matches)
// ═══════════════════════════════════════════════════════════════════════════════

/// `xmlXPathObjectPtr xmlXPathConvertBoolean(xmlXPathObjectPtr val)`.
///
/// # SAFETY
///
/// - `val` must be a valid object pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathConvertBoolean(val: *mut _xmlXPathObject) -> *mut _xmlXPathObject {
    if val.is_null() {
        return ptr::null_mut();
    }
    let typ = (*val).type_;
    if typ == xmlXPathObjectType::XPATH_BOOLEAN as c_int {
        return val;
    }
    let b = crate::abi::exports_xml2::object_to_xpathvalue_pub(val).as_boolean();
    crate::abi::exports_xml2::xmlXPathFreeObject(val);
    let obj = xmlMallocZero(size_of::<_xmlXPathObject>()) as *mut _xmlXPathObject;
    if obj.is_null() {
        return ptr::null_mut();
    }
    (*obj).type_ = xmlXPathObjectType::XPATH_BOOLEAN as c_int;
    (*obj).boolval = if b { 1 } else { 0 };
    obj
}

/// `xmlXPathObjectPtr xmlXPathConvertNumber(xmlXPathObjectPtr val)`.
///
/// # SAFETY
///
/// - `val` must be a valid object pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathConvertNumber(val: *mut _xmlXPathObject) -> *mut _xmlXPathObject {
    if val.is_null() {
        return ptr::null_mut();
    }
    let typ = (*val).type_;
    if typ == xmlXPathObjectType::XPATH_NUMBER as c_int {
        return val;
    }
    let n = crate::abi::exports_xml2::object_to_xpathvalue_pub(val).as_number();
    crate::abi::exports_xml2::xmlXPathFreeObject(val);
    let obj = xmlMallocZero(size_of::<_xmlXPathObject>()) as *mut _xmlXPathObject;
    if obj.is_null() {
        return ptr::null_mut();
    }
    (*obj).type_ = xmlXPathObjectType::XPATH_NUMBER as c_int;
    (*obj).floatval = n;
    obj
}

/// `xmlXPathObjectPtr xmlXPathConvertString(xmlXPathObjectPtr val)`.
///
/// # SAFETY
///
/// - `val` must be a valid object pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathConvertString(val: *mut _xmlXPathObject) -> *mut _xmlXPathObject {
    if val.is_null() {
        return unsafe { xmlXPathNewString(ptr::null()) };
    }
    let typ = (*val).type_;
    if typ == xmlXPathObjectType::XPATH_STRING as c_int {
        return val;
    }
    let s = crate::abi::exports_xml2::object_to_xpathvalue_pub(val).as_string();
    crate::abi::exports_xml2::xmlXPathFreeObject(val);
    let buf = dup_rust_string(&s);
    unsafe { xmlXPathWrapString(buf) }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Casts (value-level, no object allocation)
// ═══════════════════════════════════════════════════════════════════════════════

/// `int xmlXPathCastToBoolean(xmlXPathObjectPtr val)`.
///
/// # SAFETY
///
/// - `val` must be a valid object pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathCastToBoolean(val: *mut _xmlXPathObject) -> c_int {
    if val.is_null() {
        return 0;
    }
    crate::abi::exports_xml2::object_to_xpathvalue_pub(val).as_boolean() as c_int
}

/// `double xmlXPathCastToNumber(xmlXPathObjectPtr val)`.
///
/// # SAFETY
///
/// - `val` must be a valid object pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathCastToNumber(val: *mut _xmlXPathObject) -> c_double {
    if val.is_null() {
        return f64::NAN;
    }
    crate::abi::exports_xml2::object_to_xpathvalue_pub(val).as_number()
}

/// `double xmlXPathCastBooleanToNumber(int val)`.
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
pub const unsafe extern "C" fn xmlXPathCastBooleanToNumber(val: c_int) -> c_double {
    if val != 0 {
        1.0
    } else {
        0.0
    }
}

/// `xmlChar *xmlXPathCastBooleanToString(int val)`.
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
pub unsafe extern "C" fn xmlXPathCastBooleanToString(val: c_int) -> *mut xmlChar {
    if val != 0 {
        xml_strdup(c"true".as_ptr() as *const xmlChar)
    } else {
        xml_strdup(c"false".as_ptr() as *const xmlChar)
    }
}

/// `int xmlXPathCastNodeSetToBoolean(xmlNodeSetPtr ns)`.
///
/// # SAFETY
///
/// - `ns` must be a valid node-set pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathCastNodeSetToBoolean(ns: *mut _xmlNodeSet) -> c_int {
    if ns.is_null() {
        return 0;
    }
    (unsafe { (*ns).nodeNr > 0 }) as c_int
}

/// `double xmlXPathCastNodeSetToNumber(xmlNodeSetPtr ns)`.
///
/// # SAFETY
///
/// - `ns` must be a valid node-set pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathCastNodeSetToNumber(ns: *mut _xmlNodeSet) -> c_double {
    unsafe {
        let val = crate::abi::exports_xml2::object_to_xpathvalue_pub(xmlXPathWrapNodeSet(ns));
        let n = val.as_number();
        // The wrapper owns the node set — release without freeing.
        n
    }
}

/// `xmlChar *xmlXPathCastNodeSetToString(xmlNodeSetPtr ns)`.
///
/// # SAFETY
///
/// - `ns` must be a valid node-set pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathCastNodeSetToString(ns: *mut _xmlNodeSet) -> *mut xmlChar {
    let val = crate::xml::xpath::types::XPathValue::NodeSet(unsafe { node_set_to_internal(ns) });
    let s = val.as_string();
    dup_rust_string(&s)
}

/// Convert a C node set into an internal NodeSet (copying the node pointers).
unsafe fn node_set_to_internal(ns: *mut _xmlNodeSet) -> NodeSet {
    let mut out = NodeSet::new();
    if ns.is_null() {
        return out;
    }
    let nr = unsafe { (*ns).nodeNr };
    let tab = unsafe { (*ns).nodeTab };
    if !tab.is_null() {
        for i in 0..nr as isize {
            out.push(unsafe { *tab.add(i as usize) });
        }
    }
    out
}

/// `double xmlXPathCastNodeToNumber(xmlNodePtr node)`.
///
/// # SAFETY
///
/// - `node` must be a valid node pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathCastNodeToNumber(node: *mut _xmlNode) -> c_double {
    let s = node_string_value(node);
    crate::xml::xpath::types::string_to_number(&s)
}

/// `xmlChar *xmlXPathCastNodeToString(xmlNodePtr node)`.
///
/// # SAFETY
///
/// - `node` must be a valid node pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathCastNodeToString(node: *mut _xmlNode) -> *mut xmlChar {
    let s = node_string_value(node);
    dup_rust_string(&s)
}

/// `int xmlXPathCastNumberToBoolean(double val)`.
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
pub unsafe extern "C" fn xmlXPathCastNumberToBoolean(val: c_double) -> c_int {
    (val != 0.0 && !val.is_nan()) as c_int
}

/// `xmlChar *xmlXPathCastNumberToString(double val)`.
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
pub unsafe extern "C" fn xmlXPathCastNumberToString(val: c_double) -> *mut xmlChar {
    number_to_xmlstring(val)
}

/// `int xmlXPathCastStringToBoolean(const xmlChar *val)`.
///
/// # SAFETY
///
/// - `val` must be a valid NUL-terminated string or NULL.
#[no_mangle]
pub const unsafe extern "C" fn xmlXPathCastStringToBoolean(val: *const xmlChar) -> c_int {
    if val.is_null() || unsafe { *val } == 0 {
        0
    } else {
        1
    }
}

/// `int xmlXPathIsNaN(double val)`.
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
pub const unsafe extern "C" fn xmlXPathIsNaN(val: c_double) -> c_int {
    val.is_nan() as c_int
}

/// `int xmlXPathIsInf(double val)` — 1 for +inf, -1 for -inf, 0 otherwise.
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
pub unsafe extern "C" fn xmlXPathIsInf(val: c_double) -> c_int {
    if val.is_infinite() {
        if val > 0.0 {
            1
        } else {
            -1
        }
    } else {
        0
    }
}

/// `double xmlXPathStringEvalNumber(const xmlChar *str)`.
///
/// # SAFETY
///
/// - `str` must be a valid NUL-terminated string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathStringEvalNumber(str_: *const xmlChar) -> c_double {
    if str_.is_null() {
        return f64::NAN;
    }
    let s = unsafe { crate::xml::string::xmlstr_to_string(str_) };
    crate::xml::xpath::types::string_to_number(&s)
}

/// `int xmlXPathIsNodeType(const xmlChar *name)` — whether `name` is one of
/// the XPath node-type names.
///
/// # SAFETY
///
/// - `name` must be a valid NUL-terminated string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathIsNodeType(name: *const xmlChar) -> c_int {
    if name.is_null() {
        return 0;
    }
    let s = unsafe { crate::xml::string::xmlstr_to_string(name) };
    match s.as_str() {
        "comment" | "text" | "processing-instruction" | "node" => 1,
        _ => 0,
    }
}

/// `void xmlXPathInit(void)` — no-op (the candidate needs no initialization).
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
pub const unsafe extern "C" fn xmlXPathInit() {}

/// `void xmlXPathErr(xmlXPathParserContextPtr ctxt, int error)` — stub entry
/// (the parser-context error channel is set via the context bridge).
///
/// # SAFETY
///
/// - `ctxt` must be a valid parser context or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathErr(
    ctxt: *mut crate::xml::xpath::parser_context::XmlXPathParserContext,
    error: c_int,
) {
    if !ctxt.is_null() {
        unsafe { (*ctxt).error = error };
    }
}

/// `void xmlXPatherror(xmlXPathParserContextPtr ctxt, const char *file, int line, int no)`.
///
/// # SAFETY
///
/// - `ctxt` must be a valid parser context or NULL.
/// - `file` must be a valid string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPatherror(
    ctxt: *mut crate::xml::xpath::parser_context::XmlXPathParserContext,
    _file: *const c_char,
    _line: c_int,
    _no: c_int,
) {
    if !ctxt.is_null() {
        unsafe { (*ctxt).error = _no };
    }
}

#[allow(unused)]
const fn _unused_doc(_d: *mut _xmlDoc) {}

// ═══════════════════════════════════════════════════════════════════════════════
// Node-set operations
// ═══════════════════════════════════════════════════════════════════════════════

/// Internal: ensure a node set has room for one more node.
unsafe fn node_set_grow(ns: *mut _xmlNodeSet) {
    if ns.is_null() {
        return;
    }
    unsafe {
        let nr = (*ns).nodeNr;
        let max = (*ns).nodeMax;
        if nr < max {
            return;
        }
        let new_max = if max <= 0 { 8 } else { max * 2 };
        let new_tab = crate::abi::allocator::xmlReallocImpl(
            (*ns).nodeTab as *mut c_void,
            (new_max as usize) * size_of::<*mut _xmlNode>(),
        ) as *mut *mut _xmlNode;
        if !new_tab.is_null() {
            (*ns).nodeTab = new_tab;
            (*ns).nodeMax = new_max;
        }
    }
}

/// `int xmlXPathNodeSetContains(xmlNodeSetPtr cur, xmlNodePtr val)`.
///
/// # SAFETY
///
/// - `cur` must be a valid node set or NULL; `val` a valid node or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathNodeSetContains(
    cur: *mut _xmlNodeSet,
    val: *mut _xmlNode,
) -> c_int {
    if cur.is_null() || val.is_null() {
        return 0;
    }
    unsafe {
        let nr = (*cur).nodeNr;
        let tab = (*cur).nodeTab;
        if !tab.is_null() {
            for i in 0..nr as isize {
                if *tab.add(i as usize) == val {
                    return 1;
                }
            }
        }
    }
    0
}

/// `int xmlXPathNodeSetAdd(xmlNodeSetPtr cur, xmlNodePtr val)` — adds `val` if
/// not already present; returns 0 on success, -1 on error.
///
/// # SAFETY
///
/// - `cur` must be a valid node set or NULL; `val` a valid node or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathNodeSetAdd(cur: *mut _xmlNodeSet, val: *mut _xmlNode) -> c_int {
    if cur.is_null() || val.is_null() {
        return -1;
    }
    if xmlXPathNodeSetContains(cur, val) != 0 {
        return 0;
    }
    unsafe {
        node_set_grow(cur);
        if (*cur).nodeNr >= (*cur).nodeMax {
            return -1;
        }
        let idx = (*cur).nodeNr as usize;
        ptr::write((*cur).nodeTab.add(idx), val);
        (*cur).nodeNr += 1;
    }
    0
}

/// `int xmlXPathNodeSetAddUnique(xmlNodeSetPtr cur, xmlNodePtr val)` — adds
/// without a duplicate check.
///
/// # SAFETY
///
/// - `cur` must be a valid node set or NULL; `val` a valid node or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathNodeSetAddUnique(
    cur: *mut _xmlNodeSet,
    val: *mut _xmlNode,
) -> c_int {
    if cur.is_null() || val.is_null() {
        return -1;
    }
    unsafe {
        node_set_grow(cur);
        if (*cur).nodeNr >= (*cur).nodeMax {
            return -1;
        }
        let idx = (*cur).nodeNr as usize;
        ptr::write((*cur).nodeTab.add(idx), val);
        (*cur).nodeNr += 1;
    }
    0
}

/// `int xmlXPathNodeSetAddNs(xmlNodeSetPtr cur, xmlNodePtr node, xmlNsPtr ns)`
/// — adds the namespace declaration as a namespace node.
///
/// # SAFETY
///
/// - `cur` must be a valid node set or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathNodeSetAddNs(
    cur: *mut _xmlNodeSet,
    _node: *mut _xmlNode,
    ns: *mut _xmlNs,
) -> c_int {
    if cur.is_null() || ns.is_null() {
        return -1;
    }
    // UPSTREAM-PARITY: namespace nodes are represented as the _xmlNs pointer
    // cast to a node pointer; the reader exposes them via the same encoding.
    let ns_node = ns as *mut _xmlNode;
    if xmlXPathNodeSetContains(cur, ns_node) != 0 {
        return 0;
    }
    xmlXPathNodeSetAddUnique(cur, ns_node)
}

/// `void xmlXPathNodeSetDel(xmlNodeSetPtr cur, xmlNodePtr val)` (upstream
/// xpath.h — R-000176, the candidate previously returned an int).
///
/// # SAFETY
///
/// - `cur` must be a valid node set or NULL; `val` a valid node or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathNodeSetDel(cur: *mut _xmlNodeSet, val: *mut _xmlNode) {
    if cur.is_null() || val.is_null() {
        return;
    }
    unsafe {
        let nr = (*cur).nodeNr;
        let tab = (*cur).nodeTab;
        let mut found = -1;
        if !tab.is_null() {
            for i in 0..nr as isize {
                if *tab.add(i as usize) == val {
                    found = i as c_int;
                    break;
                }
            }
        }
        if found >= 0 {
            let fi = found as usize;
            for i in fi..(nr as usize - 1) {
                ptr::write(tab.add(i), *tab.add(i + 1));
            }
            (*cur).nodeNr -= 1;
        }
    }
}

/// `void xmlXPathNodeSetRemove(xmlNodeSetPtr cur, int val)` — remove by
/// index (upstream xpath.h — R-000176).
///
/// # SAFETY
///
/// - `cur` must be a valid node set or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathNodeSetRemove(cur: *mut _xmlNodeSet, val: c_int) {
    if cur.is_null() || val < 0 {
        return;
    }
    unsafe {
        let nr = (*cur).nodeNr;
        if val >= nr {
            return;
        }
        let tab = (*cur).nodeTab;
        let vi = val as usize;
        for i in vi..(nr as usize - 1) {
            ptr::write(tab.add(i), *tab.add(i + 1));
        }
        (*cur).nodeNr -= 1;
    }
}

/// `void xmlXPathNodeSetSort(xmlNodeSetPtr set)` — sort in document order
/// (duplicates removed, matching upstream xmlXPathNodeSetSort).
///
/// # SAFETY
///
/// - `set` must be a valid node set or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathNodeSetSort(set: *mut _xmlNodeSet) {
    if set.is_null() {
        return;
    }
    unsafe {
        let nr = (*set).nodeNr;
        let tab = (*set).nodeTab;
        if nr <= 1 || tab.is_null() {
            return;
        }
        // Insertion sort in document order (upstream uses a bubble-ish sort
        // with the same ordering predicate).
        for i in 1..nr as usize {
            let key = *tab.add(i);
            let mut j = i;
            while j > 0 {
                let prev = *tab.add(j - 1);
                if crate::xml::xpath::types::compare_document_order(prev, key)
                    == core::cmp::Ordering::Greater
                {
                    ptr::write(tab.add(j), prev);
                    j -= 1;
                } else {
                    break;
                }
            }
            ptr::write(tab.add(j), key);
        }
        // Deduplicate (upstream xmlXPathNodeSetSort removes duplicates).
        let mut w = 0usize;
        for r in 0..nr as usize {
            if w == 0 || *tab.add(w - 1) != *tab.add(r) {
                ptr::write(tab.add(w), *tab.add(r));
                w += 1;
            }
        }
        (*set).nodeNr = w as c_int;
    }
}

/// `xmlNodeSetPtr xmlXPathNodeSetMerge(xmlNodeSetPtr val1, xmlNodeSetPtr val2)`
/// — merges val2 into val1 (nodes not already present), returns val1.
///
/// # SAFETY
///
/// - `val1`/`val2` must be valid node sets or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathNodeSetMerge(
    val1: *mut _xmlNodeSet,
    val2: *mut _xmlNodeSet,
) -> *mut _xmlNodeSet {
    if val1.is_null() && val2.is_null() {
        return ptr::null_mut();
    }
    if val1.is_null() {
        // UPSTREAM-PARITY: merging into NULL returns a copy of val2.
        let obj = xmlXPathNewNodeSetList(val2);
        let ns = unsafe { (*obj).nodesetval as *mut _xmlNodeSet };
        if obj.is_null() {
            return ptr::null_mut();
        }
        return ns;
    }
    if val2.is_null() {
        return val1;
    }
    unsafe {
        let nr2 = (*val2).nodeNr;
        let tab2 = (*val2).nodeTab;
        if !tab2.is_null() {
            for i in 0..nr2 as isize {
                let n = *tab2.add(i as usize);
                if xmlXPathNodeSetContains(val1, n) == 0 {
                    xmlXPathNodeSetAddUnique(val1, n);
                }
            }
        }
    }
    val1
}

/// `xmlNodeSetPtr xmlXPathDifference(xmlNodeSetPtr nodes1, xmlNodeSetPtr nodes2)`
/// — nodes in nodes1 not in nodes2 (document order).
///
/// # SAFETY
///
/// - `nodes1`/`nodes2` must be valid node sets or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathDifference(
    nodes1: *mut _xmlNodeSet,
    nodes2: *mut _xmlNodeSet,
) -> *mut _xmlNodeSet {
    if nodes1.is_null() {
        return ptr::null_mut();
    }
    let mut a = unsafe { node_set_to_internal(nodes1) };
    a.sort();
    let b = unsafe { node_set_to_internal(nodes2) };
    let mut out = NodeSet::new();
    for n in a.iter() {
        if !b.contains(n) {
            out.push(n);
        }
    }
    out.sort();
    out.to_raw()
}

/// `xmlNodeSetPtr xmlXPathIntersection(xmlNodeSetPtr nodes1, xmlNodeSetPtr nodes2)`.
///
/// # SAFETY
///
/// - `nodes1`/`nodes2` must be valid node sets or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathIntersection(
    nodes1: *mut _xmlNodeSet,
    nodes2: *mut _xmlNodeSet,
) -> *mut _xmlNodeSet {
    let a = unsafe { node_set_to_internal(nodes1) };
    let b = unsafe { node_set_to_internal(nodes2) };
    let mut out = NodeSet::new();
    for n in a.iter() {
        if b.contains(n) {
            out.push(n);
        }
    }
    out.sort();
    out.to_raw()
}

/// `xmlNodeSetPtr xmlXPathDistinct(xmlNodeSetPtr nodes)`.
///
/// # SAFETY
///
/// - `nodes` must be a valid node set or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathDistinct(nodes: *mut _xmlNodeSet) -> *mut _xmlNodeSet {
    if nodes.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        xmlXPathNodeSetSort(nodes);
        nodes
    }
}

/// `xmlNodeSetPtr xmlXPathDistinctSorted(xmlNodeSetPtr nodes)`.
///
/// # SAFETY
///
/// - `nodes` must be a valid node set or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathDistinctSorted(nodes: *mut _xmlNodeSet) -> *mut _xmlNodeSet {
    if nodes.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        let nr = (*nodes).nodeNr;
        let tab = (*nodes).nodeTab;
        let mut w = 0usize;
        if !tab.is_null() {
            for r in 0..nr as usize {
                if w == 0 || *tab.add(w - 1) != *tab.add(r) {
                    ptr::write(tab.add(w), *tab.add(r));
                    w += 1;
                }
            }
        }
        (*nodes).nodeNr = w as c_int;
        nodes
    }
}

/// `int xmlXPathHasSameNodes(xmlNodeSetPtr nodes1, xmlNodeSetPtr nodes2)`.
///
/// # SAFETY
///
/// - `nodes1`/`nodes2` must be valid node sets or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathHasSameNodes(
    nodes1: *mut _xmlNodeSet,
    nodes2: *mut _xmlNodeSet,
) -> c_int {
    if nodes1.is_null() || nodes2.is_null() {
        return 0;
    }
    unsafe {
        let nr1 = (*nodes1).nodeNr;
        let nr2 = (*nodes2).nodeNr;
        if nr1 != nr2 {
            return 0;
        }
        let tab1 = (*nodes1).nodeTab;
        let tab2 = (*nodes2).nodeTab;
        for i in 0..nr1 as isize {
            let mut found = false;
            for j in 0..nr2 as isize {
                if *tab1.add(i as usize) == *tab2.add(j as usize) {
                    found = true;
                    break;
                }
            }
            if !found {
                return 0;
            }
        }
    }
    1
}

/// Internal: leading/trailing helpers.
unsafe fn leading_nodes(nodes: &NodeSet, node: *mut _xmlNode) -> NodeSet {
    let mut out = NodeSet::new();
    for n in nodes.iter() {
        if n == node {
            break;
        }
        out.push(n);
    }
    out
}

unsafe fn trailing_nodes(nodes: &NodeSet, node: *mut _xmlNode) -> NodeSet {
    let mut out = NodeSet::new();
    let mut seen = false;
    for n in nodes.iter() {
        if n == node {
            seen = true;
            continue;
        }
        if seen {
            out.push(n);
        }
    }
    out
}

/// `xmlNodeSetPtr xmlXPathLeading(xmlNodeSetPtr nodes1, xmlNodeSetPtr nodes2)`.
///
/// # SAFETY
///
/// - `nodes1`/`nodes2` must be valid node sets or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathLeading(
    nodes1: *mut _xmlNodeSet,
    nodes2: *mut _xmlNodeSet,
) -> *mut _xmlNodeSet {
    if nodes1.is_null() {
        return ptr::null_mut();
    }
    let mut a = unsafe { node_set_to_internal(nodes1) };
    a.sort();
    let b = unsafe { node_set_to_internal(nodes2) };
    if b.is_empty() {
        let raw = a.to_raw();
        return raw;
    }
    let first = b.first().unwrap();
    let out = unsafe { leading_nodes(&a, first) };
    out.to_raw()
}

/// `xmlNodeSetPtr xmlXPathLeadingSorted(xmlNodeSetPtr nodes1, xmlNodeSetPtr nodes2)`.
///
/// # SAFETY
///
/// - `nodes1`/`nodes2` must be valid node sets or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathLeadingSorted(
    nodes1: *mut _xmlNodeSet,
    nodes2: *mut _xmlNodeSet,
) -> *mut _xmlNodeSet {
    unsafe { xmlXPathLeading(nodes1, nodes2) }
}

/// `xmlNodeSetPtr xmlXPathTrailing(xmlNodeSetPtr nodes1, xmlNodeSetPtr nodes2)`.
///
/// # SAFETY
///
/// - `nodes1`/`nodes2` must be valid node sets or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathTrailing(
    nodes1: *mut _xmlNodeSet,
    nodes2: *mut _xmlNodeSet,
) -> *mut _xmlNodeSet {
    if nodes1.is_null() {
        return ptr::null_mut();
    }
    let mut a = unsafe { node_set_to_internal(nodes1) };
    a.sort();
    let b = unsafe { node_set_to_internal(nodes2) };
    if b.is_empty() {
        return a.to_raw();
    }
    let last = b.last().unwrap();
    let out = unsafe { trailing_nodes(&a, last) };
    out.to_raw()
}

/// `xmlNodeSetPtr xmlXPathTrailingSorted(xmlNodeSetPtr nodes1, xmlNodeSetPtr nodes2)`.
///
/// # SAFETY
///
/// - `nodes1`/`nodes2` must be valid node sets or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathTrailingSorted(
    nodes1: *mut _xmlNodeSet,
    nodes2: *mut _xmlNodeSet,
) -> *mut _xmlNodeSet {
    unsafe { xmlXPathTrailing(nodes1, nodes2) }
}

/// `xmlNodeSetPtr xmlXPathNodeLeading(xmlNodeSetPtr nodes, xmlNodePtr node)`.
///
/// # SAFETY
///
/// - `nodes` must be a valid node set or NULL; `node` a valid node or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathNodeLeading(
    nodes: *mut _xmlNodeSet,
    node: *mut _xmlNode,
) -> *mut _xmlNodeSet {
    if nodes.is_null() {
        return ptr::null_mut();
    }
    let mut a = unsafe { node_set_to_internal(nodes) };
    a.sort();
    let out = unsafe { leading_nodes(&a, node) };
    out.to_raw()
}

/// `xmlNodeSetPtr xmlXPathNodeLeadingSorted(xmlNodeSetPtr nodes, xmlNodePtr node)`.
///
/// # SAFETY
///
/// - `nodes` must be a valid node set or NULL; `node` a valid node or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathNodeLeadingSorted(
    nodes: *mut _xmlNodeSet,
    node: *mut _xmlNode,
) -> *mut _xmlNodeSet {
    unsafe { xmlXPathNodeLeading(nodes, node) }
}

/// `xmlNodeSetPtr xmlXPathNodeTrailing(xmlNodeSetPtr nodes, xmlNodePtr node)`.
///
/// # SAFETY
///
/// - `nodes` must be a valid node set or NULL; `node` a valid node or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathNodeTrailing(
    nodes: *mut _xmlNodeSet,
    node: *mut _xmlNode,
) -> *mut _xmlNodeSet {
    if nodes.is_null() {
        return ptr::null_mut();
    }
    let mut a = unsafe { node_set_to_internal(nodes) };
    a.sort();
    let out = unsafe { trailing_nodes(&a, node) };
    out.to_raw()
}

/// `xmlNodeSetPtr xmlXPathNodeTrailingSorted(xmlNodeSetPtr nodes, xmlNodePtr node)`.
///
/// # SAFETY
///
/// - `nodes` must be a valid node set or NULL; `node` a valid node or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathNodeTrailingSorted(
    nodes: *mut _xmlNodeSet,
    node: *mut _xmlNode,
) -> *mut _xmlNodeSet {
    unsafe { xmlXPathNodeTrailing(nodes, node) }
}

/// `void xmlXPathNodeSetFreeNs(xmlNsPtr ns)` — releases a synthesized
/// namespace node (upstream xpath.c `xmlXPathNodeSetFreeNs`).
///
/// UPSTREAM-PARITY: an XPath node-set that contains namespace nodes holds
/// *synthesized* copies whose `next` field points at the owner element (not
/// at another namespace declaration). Such nodes are freed here along with
/// their href/prefix; real namespace declarations (owned by the tree) are
/// left untouched.
///
/// # SAFETY
///
/// - `ns` must be a valid namespace pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathNodeSetFreeNs(ns: *mut _xmlNs) {
    unsafe {
        if ns.is_null() {
            return;
        }
        if (*ns).type_ != crate::abi::types::xmlElementType::XML_NAMESPACE_DECL as c_int {
            return;
        }
        // A synthesized namespace node's `next` is the owner element.
        if !(*ns).next.is_null()
            && (*(*ns).next).type_ != crate::abi::types::xmlElementType::XML_NAMESPACE_DECL as c_int
        {
            if !(*ns).href.is_null() {
                libc::free((*ns).href as *mut libc::c_void);
            }
            if !(*ns).prefix.is_null() {
                libc::free((*ns).prefix as *mut libc::c_void);
            }
            libc::free(ns as *mut libc::c_void);
        }
    }
}

/// `XML_INTPTR_T xmlXPathOrderDocElems(xmlDocPtr doc)` (2.15 signature) —
/// indexes the document's elements in document order: each element's
/// `content` field is set to `-(n)` where n is its 1-based document-order
/// position, and the total element count is returned (-1 for NULL).
///
/// # UPSTREAM-PARITY
///
/// Upstream 2.13+ changed the return type from `xmlNodeSetPtr` to
/// `XML_INTPTR_T` (long). The element `content` slots are repurposed as the
/// document-order index (XML_INT_TO_PTR(-count)).
///
/// # SAFETY
///
/// - `doc` must be a valid document or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathOrderDocElems(doc: *mut _xmlDoc) -> c_long {
    if doc.is_null() {
        return -1;
    }
    let mut count: c_long = 0;
    unsafe {
        let mut cur = (*doc).children;
        while !cur.is_null() {
            if (*cur).type_ == crate::abi::types::xmlElementType::XML_ELEMENT_NODE as c_int {
                count += 1;
                // Upstream stores the negative 1-based index in `content`
                // (XML_INT_TO_PTR(-count)); element nodes keep content NULL
                // otherwise, so this is non-destructive for our tree.
                (*cur).content = (-count) as *mut xmlChar;
                if !(*cur).children.is_null() {
                    cur = (*cur).children;
                    continue;
                }
            }
            if !(*cur).next.is_null() {
                cur = (*cur).next;
                continue;
            }
            loop {
                cur = (*cur).parent;
                if cur.is_null() {
                    break;
                }
                if cur == doc as *mut _xmlNode {
                    cur = ptr::null_mut();
                    break;
                }
                if !(*cur).next.is_null() {
                    cur = (*cur).next;
                    break;
                }
            }
        }
    }
    count
}
use std::collections::HashMap;
use std::ffi::{CStr, CString};

use crate::abi::structs::_xmlAttr;
use crate::xml::validation::{get_id, is_xml_name_char, is_xml_name_start};
use crate::xml::xpath::context::XPathContext;
use crate::xml::xpath::parser_context::{
    cast_top_to_number, compare_values_impl, equal_values_impl, free_parser_context, new_bool,
    new_number, new_parser_context, pc_set_error, pop_boolean, pop_external, pop_node_set,
    pop_number, pop_string, value_pop, value_push, XmlXPathParserContext,
};

// ── Shared helpers ──────────────────────────────────────────────────────

/// Opaque `xmlXPathParserContextPtr` → typed pointer.
const unsafe fn pc_from(p: *mut c_void) -> *mut XmlXPathParserContext {
    p as *mut XmlXPathParserContext
}

/// Byte-wise C string equality (upstream `xmlStrEqual`).
unsafe fn cstr_eq(a: *const xmlChar, b: *const xmlChar) -> bool {
    if a.is_null() || b.is_null() {
        return a == b;
    }
    let mut i = 0usize;
    loop {
        let ca = unsafe { *a.add(i) };
        let cb = unsafe { *b.add(i) };
        if ca != cb {
            return false;
        }
        if ca == 0 {
            return true;
        }
        i += 1;
    }
}

/// Upstream IS_BLANK_CH: space, tab, LF, CR.
const unsafe fn is_blank_ch(c: xmlChar) -> bool {
    c == b' ' || c == b'\t' || c == b'\n' || c == b'\r'
}

/// CAST_TO_STRING equivalent on the top-of-stack object (in place).
///
/// # SAFETY
///
/// - `pc` must be a valid parser context with a non-NULL `value`.
unsafe fn cast_top_to_string(pc: *mut XmlXPathParserContext) {
    unsafe {
        let val = (*pc).value;
        if val.is_null() {
            pc_set_error(pc, crate::abi::types::XPATH_INVALID_OPERAND as c_int);
            return;
        }
        if (*val).type_ != xmlXPathObjectType::XPATH_STRING as c_int {
            let v = crate::abi::exports_xml2::object_to_xpathvalue_pub(val);
            let s = v.as_string();
            if !(*val).stringval.is_null() {
                xmlFreeImpl((*val).stringval as *mut c_void);
            }
            (*val).stringval = dup_rust_string(&s);
            (*val).type_ = xmlXPathObjectType::XPATH_STRING as c_int;
        }
    }
}

/// CAST_TO_BOOLEAN equivalent on the top-of-stack object (in place).
///
/// # SAFETY
///
/// - `pc` must be a valid parser context with a non-NULL `value`.
unsafe fn cast_top_to_boolean(pc: *mut XmlXPathParserContext) {
    unsafe {
        let val = (*pc).value;
        if val.is_null() {
            pc_set_error(pc, crate::abi::types::XPATH_INVALID_OPERAND as c_int);
            return;
        }
        if (*val).type_ != xmlXPathObjectType::XPATH_BOOLEAN as c_int {
            let v = crate::abi::exports_xml2::object_to_xpathvalue_pub(val);
            (*val).boolval = v.as_boolean() as c_int;
            (*val).type_ = xmlXPathObjectType::XPATH_BOOLEAN as c_int;
        }
    }
}

/// CHECK_ARITY equivalent: fails with XPATH_INVALID_ARITY when fewer than `n`
/// values are stacked.
unsafe fn check_arity(pc: *mut XmlXPathParserContext, n: c_int) -> bool {
    if pc.is_null() || (*pc).value_nr < n {
        pc_set_error(pc, crate::abi::types::XPATH_INVALID_ARITY as c_int);
        return false;
    }
    true
}

/// Consume an XML Name / NCName from `cur` (NUL-terminated), returning the
/// number of bytes consumed. Mirrors upstream `xmlScanName(ptr, SIZE_MAX,
/// flags)` with XML 1.0 Fifth-Edition character classes.
const unsafe fn scan_c_name(cur: *const xmlChar, nc: bool) -> usize {
    if cur.is_null() {
        return 0;
    }
    let mut i = 0usize;
    let mut first = true;
    loop {
        let b = unsafe { *cur.add(i) };
        if b == 0 {
            break;
        }
        if nc && b == b':' {
            break;
        }
        let (ch, adv): (char, usize) = if b < 0x80 {
            (b as char, 1)
        } else if b >= 0xC0 && b <= 0xDF {
            (
                unsafe {
                    char::from_u32_unchecked(
                        ((b as u32 & 0x1F) << 6) | (*cur.add(i + 1) as u32 & 0x3F),
                    )
                },
                2,
            )
        } else if b >= 0xE0 && b <= 0xEF {
            (
                unsafe {
                    char::from_u32_unchecked(
                        ((b as u32 & 0x0F) << 12)
                            | ((*cur.add(i + 1) as u32 & 0x3F) << 6)
                            | (*cur.add(i + 2) as u32 & 0x3F),
                    )
                },
                3,
            )
        } else if b >= 0xF0 && b <= 0xF7 {
            (
                unsafe {
                    char::from_u32_unchecked(
                        ((b as u32 & 0x07) << 18)
                            | ((*cur.add(i + 1) as u32 & 0x3F) << 12)
                            | ((*cur.add(i + 2) as u32 & 0x3F) << 6)
                            | (*cur.add(i + 3) as u32 & 0x3F),
                    )
                },
                4,
            )
        } else {
            break;
        };
        let ok = if first {
            is_xml_name_start(ch)
        } else {
            is_xml_name_char(ch)
        };
        if !ok {
            break;
        }
        first = false;
        i += adv;
    }
    i
}

/// Byte-wise substring search (upstream `xmlStrstr`).
unsafe fn cstr_find(hay: *const xmlChar, needle: *const xmlChar) -> *const xmlChar {
    if hay.is_null() || needle.is_null() {
        return ptr::null();
    }
    if unsafe { *needle } == 0 {
        return hay;
    }
    let hlen = unsafe { crate::xml::string::xml_strlen(hay) };
    let nlen = unsafe { crate::xml::string::xml_strlen(needle) };
    if nlen > hlen {
        return ptr::null();
    }
    let hay_b = unsafe { core::slice::from_raw_parts(hay, hlen) };
    let needle_b = unsafe { core::slice::from_raw_parts(needle, nlen) };
    for off in 0..=hlen - nlen {
        if &hay_b[off..off + nlen] == needle_b {
            return unsafe { hay.add(off) };
        }
    }
    ptr::null()
}

/// The `xml:` namespace URI (upstream `XML_XML_NAMESPACE`).
const XML_XML_NAMESPACE_BYTES: &[u8] = b"http://www.w3.org/XML/1998/namespace\0";

/// Static fake `xml` namespace node (upstream `xmlXPathXMLNamespace`).
/// Wrapped so the raw-pointer struct can live in a `static` (the pointer
/// fields are never written after construction).
struct XmlXPathXmlNs(_xmlNs);
unsafe impl Sync for XmlXPathXmlNs {}
static XML_XPATH_XML_NS: XmlXPathXmlNs = XmlXPathXmlNs(crate::abi::structs::_xmlNs {
    next: ptr::null_mut(),
    type_: crate::abi::types::xmlElementType::XML_NAMESPACE_DECL as c_int,
    href: XML_XML_NAMESPACE_BYTES.as_ptr() as *const xmlChar,
    prefix: c"xml".as_ptr() as *const xmlChar,
    _private: ptr::null_mut(),
    context: ptr::null_mut(),
});

// Upstream `xmlXPathStringHash` (FNV-ish over the string bytes) is not
// observable through the public API; the node-set equality helpers below
// perform the full string comparison the hash only gates.
// ═══════════════════════════════════════════════════════════════════════════════
// Value stack operators (upstream xmlXPathValuePush/Pop + typed Pop*)
// ═══════════════════════════════════════════════════════════════════════════════

/// `int xmlXPathValuePush(xmlXPathParserContextPtr ctxt, xmlXPathObjectPtr
/// value)` — returns the stack slot index of the pushed value (upstream
/// xpath.c `return (ctxt->valueNr++)`), -1 on NULL arguments. R-000176: the
/// candidate previously returned the pushed value pointer.
///
/// # SAFETY
///
/// - `ctxt` must be a valid parser context or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathValuePush(
    ctxt: *mut c_void,
    value: *mut _xmlXPathObject,
) -> c_int {
    if ctxt.is_null() || value.is_null() {
        return -1;
    }
    let pc = pc_from(ctxt);
    // SAFETY: pc is non-NULL here; the slot index is the pre-increment
    // value_nr (upstream `return (ctxt->valueNr++)`).
    let idx = unsafe { (*pc).value_nr };
    value_push(pc, value);
    idx
}

/// `xmlXPathObjectPtr xmlXPathValuePop(xmlXPathParserContextPtr ctxt)`.
///
/// # SAFETY
///
/// - `ctxt` must be a valid parser context or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathValuePop(ctxt: *mut c_void) -> *mut _xmlXPathObject {
    value_pop(pc_from(ctxt))
}

/// `int xmlXPathPopBoolean(xmlXPathParserContextPtr ctxt)`.
///
/// # SAFETY
///
/// - `ctxt` must be a valid parser context or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathPopBoolean(ctxt: *mut c_void) -> c_int {
    pop_boolean(pc_from(ctxt))
}

/// `void *xmlXPathPopExternal(xmlXPathParserContextPtr ctxt)`.
///
/// # SAFETY
///
/// - `ctxt` must be a valid parser context or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathPopExternal(ctxt: *mut c_void) -> *mut c_void {
    pop_external(pc_from(ctxt))
}

/// `xmlNodeSetPtr xmlXPathPopNodeSet(xmlXPathParserContextPtr ctxt)`.
///
/// # SAFETY
///
/// - `ctxt` must be a valid parser context or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathPopNodeSet(ctxt: *mut c_void) -> *mut _xmlNodeSet {
    pop_node_set(pc_from(ctxt))
}

/// `double xmlXPathPopNumber(xmlXPathParserContextPtr ctxt)`.
///
/// # SAFETY
///
/// - `ctxt` must be a valid parser context or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathPopNumber(ctxt: *mut c_void) -> c_double {
    pop_number(pc_from(ctxt))
}

/// `xmlChar *xmlXPathPopString(xmlXPathParserContextPtr ctxt)`.
///
/// # SAFETY
///
/// - `ctxt` must be a valid parser context or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathPopString(ctxt: *mut c_void) -> *mut xmlChar {
    pop_string(pc_from(ctxt))
}

/// Shared body of the in-place arithmetic operators: pops the right operand,
/// converts it to a number, converts the (remaining) top of stack to a number
/// and applies `op` to it in place. UPSTREAM-PARITY: `xmlXPathAddValues` etc.
/// operate on `ctxt->value` in place instead of pushing a fresh object.
unsafe fn binary_inplace(ctxt: *mut c_void, op: impl Fn(&mut f64, f64)) {
    let pc = pc_from(ctxt);
    if pc.is_null() {
        return;
    }
    let arg = value_pop(pc);
    if arg.is_null() {
        pc_set_error(pc, crate::abi::types::XPATH_INVALID_OPERAND as c_int);
        return;
    }
    let val = crate::abi::exports_xml2::object_to_xpathvalue_pub(arg).as_number();
    crate::abi::exports_xml2::xmlXPathFreeObject(arg);
    if unsafe { (*pc).value.is_null() } {
        pc_set_error(pc, crate::abi::types::XPATH_INVALID_OPERAND as c_int);
        return;
    }
    cast_top_to_number(pc);
    if (*pc).error != 0 {
        return;
    }
    // Bind the field as a place before passing it by mutable reference
    // (a bare `&mut unsafe { ... }` would take the address of a temporary
    // copy of the float and the arithmetic would be lost).
    unsafe {
        let float_ref: &mut f64 = &mut (*(*pc).value).floatval;
        op(float_ref, val);
    }
}

/// `void xmlXPathAddValues(xmlXPathParserContextPtr ctxt)`.
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlXPathAddValues(ctxt: *mut c_void) {
    binary_inplace(ctxt, |x, v| *x += v);
}

/// `void xmlXPathSubValues(xmlXPathParserContextPtr ctxt)`.
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlXPathSubValues(ctxt: *mut c_void) {
    binary_inplace(ctxt, |x, v| *x -= v);
}

/// `void xmlXPathMultValues(xmlXPathParserContextPtr ctxt)`.
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlXPathMultValues(ctxt: *mut c_void) {
    binary_inplace(ctxt, |x, v| *x *= v);
}

/// `void xmlXPathDivValues(xmlXPathParserContextPtr ctxt)`.
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlXPathDivValues(ctxt: *mut c_void) {
    binary_inplace(ctxt, |x, v| *x /= v);
}

/// `void xmlXPathModValues(xmlXPathParserContextPtr ctxt)`.
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlXPathModValues(ctxt: *mut c_void) {
    binary_inplace(ctxt, |x, v| *x %= v);
}

/// `void xmlXPathValueFlipSign(xmlXPathParserContextPtr ctxt)` — unary minus.
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlXPathValueFlipSign(ctxt: *mut c_void) {
    let pc = pc_from(ctxt);
    if pc.is_null() {
        return;
    }
    cast_top_to_number(pc);
    if (*pc).error != 0 {
        return;
    }
    unsafe { (*(*pc).value).floatval = -(*(*pc).value).floatval };
}

/// `int xmlXPathEqualValues(xmlXPathParserContextPtr ctxt)` — pops two values,
/// pushes the boolean result and returns it.
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlXPathEqualValues(ctxt: *mut c_void) -> c_int {
    equal_values_impl(pc_from(ctxt), false)
}

/// `int xmlXPathNotEqualValues(xmlXPathParserContextPtr ctxt)`.
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlXPathNotEqualValues(ctxt: *mut c_void) -> c_int {
    equal_values_impl(pc_from(ctxt), true)
}

/// `int xmlXPathCompareValues(xmlXPathParserContextPtr ctxt, int inf, int strict)`.
///
/// `inf`/`strict` encode the operator: `<`=(1,1), `<=`=(1,0), `>`=(0,1),
/// `>=`=(0,0). Returns the comparison result without pushing (upstream callers
/// push the boolean themselves).
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlXPathCompareValues(
    ctxt: *mut c_void,
    inf: c_int,
    strict: c_int,
) -> c_int {
    compare_values_impl(pc_from(ctxt), inf != 0, strict != 0)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Parser context
// ═══════════════════════════════════════════════════════════════════════════════

/// `xmlXPathParserContextPtr xmlXPathNewParserContext(const xmlChar *str, xmlXPathContextPtr ctxt)`.
///
/// # SAFETY
///
/// - `str` must be a valid NUL-terminated string or NULL.
/// - `ctxt` must be a valid context or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathNewParserContext(
    str_: *const xmlChar,
    ctxt: *mut _xmlXPathContext,
) -> *mut c_void {
    new_parser_context(str_, ctxt) as *mut c_void
}

/// `void xmlXPathFreeParserContext(xmlXPathParserContextPtr ctxt)`.
///
/// # SAFETY
///
/// - `ctxt` must be a valid parser context or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathFreeParserContext(ctxt: *mut c_void) {
    free_parser_context(pc_from(ctxt));
}

/// `xmlChar *xmlXPathParseNCName(xmlXPathParserContextPtr ctxt)` — parses an
/// NCName from `ctxt->cur`, advancing it past the name.
///
/// # SAFETY
///
/// - `ctxt` must be a valid parser context.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathParseNCName(ctxt: *mut c_void) -> *mut xmlChar {
    let pc = pc_from(ctxt);
    if pc.is_null() {
        return ptr::null_mut();
    }
    let cur = unsafe { (*pc).cur };
    if cur.is_null() {
        return ptr::null_mut();
    }
    let len = scan_c_name(cur, true);
    if len == 0 {
        return ptr::null_mut();
    }
    let ret = crate::xml::string::xml_strndup(cur, len);
    unsafe { (*pc).cur = cur.add(len) };
    ret
}

/// `xmlChar *xmlXPathParseName(xmlXPathParserContextPtr ctxt)` — parses an XML
/// Name from `ctxt->cur`, advancing it past the name.
///
/// # SAFETY
///
/// - `ctxt` must be a valid parser context.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathParseName(ctxt: *mut c_void) -> *mut xmlChar {
    let pc = pc_from(ctxt);
    if pc.is_null() {
        return ptr::null_mut();
    }
    let cur = unsafe { (*pc).cur };
    if cur.is_null() {
        return ptr::null_mut();
    }
    let len = scan_c_name(cur, false);
    if len == 0 {
        return ptr::null_mut();
    }
    let ret = crate::xml::string::xml_strndup(cur, len);
    unsafe { (*pc).cur = cur.add(len) };
    ret
}

/// `void xmlXPathRoot(xmlXPathParserContextPtr ctxt)` — pushes a node-set
/// containing the document node.
///
/// # SAFETY
///
/// - `ctxt` must be a valid parser context.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathRoot(ctxt: *mut c_void) {
    let pc = pc_from(ctxt);
    if pc.is_null() {
        return;
    }
    let ctx = unsafe { (*pc).context };
    if ctx.is_null() {
        return;
    }
    let ns = NodeSet::singleton(unsafe { (*ctx).doc } as *mut _xmlNode);
    let obj = crate::abi::exports_xml2::xpath_to_object_pub(XPathValue::NodeSet(ns));
    value_push(pc, obj);
}

/// `void xmlXPathEvalExpr(xmlXPathParserContextPtr ctxt)` — compiles and
/// evaluates the expression in `ctxt->base` against `ctxt->context` and pushes
/// the result object (upstream `xmlXPathCompileExpr` + `xmlXPathRunEval`).
///
/// # SAFETY
///
/// - `ctxt` must be a valid parser context.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathEvalExpr(ctxt: *mut c_void) {
    let pc = pc_from(ctxt);
    if pc.is_null() {
        return;
    }
    let ctx = unsafe { (*pc).context };
    if ctx.is_null() {
        return;
    }
    let base = unsafe { (*pc).base };
    if base.is_null() {
        return;
    }
    let expr_str = match CStr::from_ptr(base as *const c_char).to_str() {
        Ok(s) => s,
        Err(_) => {
            pc_set_error(pc, crate::abi::types::XPATH_EXPR_ERROR as c_int);
            return;
        }
    };
    let internal = unsafe { (*ctx).extra } as *mut XPathContext;
    if internal.is_null() {
        return;
    }
    let internal = unsafe { &mut *internal };
    match crate::xml::xpath::evaluate_str(expr_str, internal) {
        Some(val) => {
            let obj = crate::abi::exports_xml2::xpath_to_object_pub(val);
            value_push(pc, obj);
        }
        None => {
            pc_set_error(pc, crate::abi::types::XPATH_EXPR_ERROR as c_int);
        }
    }
}

/// Shared predicate-result evaluation (upstream `xmlXPathEvalPredicate`).
unsafe fn eval_predicate_result(ctxt: *mut _xmlXPathContext, res: *mut _xmlXPathObject) -> c_int {
    if ctxt.is_null() || res.is_null() {
        return 0;
    }
    unsafe {
        let t = (*res).type_;
        if t == xmlXPathObjectType::XPATH_BOOLEAN as c_int {
            (*res).boolval
        } else if t == xmlXPathObjectType::XPATH_NUMBER as c_int {
            ((*res).floatval == (*ctxt).proximityPosition as f64) as c_int
        } else if t == xmlXPathObjectType::XPATH_NODESET as c_int
            || t == xmlXPathObjectType::XPATH_XSLT_TREE as c_int
        {
            let nsp = (*res).nodesetval as *mut _xmlNodeSet;
            if nsp.is_null() || (*nsp).nodeNr == 0 {
                0
            } else {
                1
            }
        } else if t == xmlXPathObjectType::XPATH_STRING as c_int {
            if (*res).stringval.is_null() || *(*res).stringval == 0 {
                0
            } else {
                1
            }
        } else {
            0
        }
    }
}

/// `int xmlXPathEvalPredicate(xmlXPathContext *ctxt, xmlXPathObject *res)`
/// (2.15 signature).
///
/// # SAFETY
///
/// - `ctxt` must be a valid context or NULL; `res` a valid object or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathEvalPredicate(
    ctxt: *mut _xmlXPathContext,
    res: *mut _xmlXPathObject,
) -> c_int {
    eval_predicate_result(ctxt, res)
}

/// `int xmlXPathEvaluatePredicateResult(xmlXPathParserContextPtr ctxt, xmlXPathObject *res)`.
///
/// # SAFETY
///
/// - `ctxt` must be a valid parser context; `res` a valid object or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathEvaluatePredicateResult(
    ctxt: *mut c_void,
    res: *mut _xmlXPathObject,
) -> c_int {
    let pc = pc_from(ctxt);
    if pc.is_null() {
        return 0;
    }
    let ctx = unsafe { (*pc).context };
    eval_predicate_result(ctx, res)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Axis traversal (xmlXPathNext*)
// ═══════════════════════════════════════════════════════════════════════════════

/// `xmlNodePtr xmlXPathNextSelf(xmlXPathParserContextPtr ctxt, xmlNodePtr cur)`.
///
/// # SAFETY
///
/// - `ctxt`, `cur` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlXPathNextSelf(ctxt: *mut c_void, cur: *mut _xmlNode) -> *mut _xmlNode {
    let pc = pc_from(ctxt);
    if pc.is_null() {
        return ptr::null_mut();
    }
    let ctx = unsafe { (*pc).context };
    if ctx.is_null() {
        return ptr::null_mut();
    }
    if cur.is_null() {
        return unsafe { (*ctx).node };
    }
    ptr::null_mut()
}

/// `xmlNodePtr xmlXPathNextChild(xmlXPathParserContextPtr ctxt, xmlNodePtr cur)`.
///
/// # SAFETY
///
/// - `ctxt`, `cur` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlXPathNextChild(ctxt: *mut c_void, cur: *mut _xmlNode) -> *mut _xmlNode {
    let pc = pc_from(ctxt);
    if pc.is_null() {
        return ptr::null_mut();
    }
    let ctx = unsafe { (*pc).context };
    if ctx.is_null() {
        return ptr::null_mut();
    }
    use crate::abi::types::xmlElementType as ET;
    if cur.is_null() {
        let node = unsafe { (*ctx).node };
        if node.is_null() {
            return ptr::null_mut();
        }
        return match unsafe { (*node).type_ } {
            t if t == ET::XML_ELEMENT_NODE as c_int
                || t == ET::XML_TEXT_NODE as c_int
                || t == ET::XML_CDATA_SECTION_NODE as c_int
                || t == ET::XML_ENTITY_REF_NODE as c_int
                || t == ET::XML_ENTITY_NODE as c_int
                || t == ET::XML_PI_NODE as c_int
                || t == ET::XML_COMMENT_NODE as c_int
                || t == ET::XML_NOTATION_NODE as c_int
                || t == ET::XML_DTD_NODE as c_int =>
            unsafe { (*node).children },
            t if t == ET::XML_DOCUMENT_NODE as c_int
                || t == ET::XML_DOCUMENT_TYPE_NODE as c_int
                || t == ET::XML_DOCUMENT_FRAG_NODE as c_int
                || t == ET::XML_HTML_DOCUMENT_NODE as c_int =>
            unsafe { (*(node as *mut _xmlDoc)).children },
            _ => ptr::null_mut(),
        };
    }
    let t = unsafe { (*cur).type_ };
    if t == ET::XML_DOCUMENT_NODE as c_int || t == ET::XML_HTML_DOCUMENT_NODE as c_int {
        return ptr::null_mut();
    }
    unsafe { (*cur).next }
}

/// `xmlNodePtr xmlXPathNextDescendant(xmlXPathParserContextPtr ctxt, xmlNodePtr cur)`.
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlXPathNextDescendant(
    ctxt: *mut c_void,
    mut cur: *mut _xmlNode,
) -> *mut _xmlNode {
    let pc = pc_from(ctxt);
    if pc.is_null() {
        return ptr::null_mut();
    }
    let ctx = unsafe { (*pc).context };
    if ctx.is_null() {
        return ptr::null_mut();
    }
    use crate::abi::types::xmlElementType as ET;
    if cur.is_null() {
        let node = unsafe { (*ctx).node };
        if node.is_null() {
            return ptr::null_mut();
        }
        let t = unsafe { (*node).type_ };
        if t == ET::XML_ATTRIBUTE_NODE as c_int || t == ET::XML_NAMESPACE_DECL as c_int {
            return ptr::null_mut();
        }
        if node == unsafe { (*ctx).doc } as *mut _xmlNode {
            return unsafe { (*(*ctx).doc).children };
        }
        return unsafe { (*node).children };
    }
    unsafe {
        if (*cur).type_ == ET::XML_NAMESPACE_DECL as c_int {
            return ptr::null_mut();
        }
        if !(*cur).children.is_null() && (*(*cur).children).type_ != ET::XML_ENTITY_DECL as c_int {
            cur = (*cur).children;
            if (*cur).type_ != ET::XML_DTD_NODE as c_int {
                return cur;
            }
        }
        if cur == (*ctx).node {
            return ptr::null_mut();
        }
        while !(*cur).next.is_null() {
            cur = (*cur).next;
            if (*cur).type_ != ET::XML_ENTITY_DECL as c_int
                && (*cur).type_ != ET::XML_DTD_NODE as c_int
            {
                return cur;
            }
        }
        loop {
            cur = (*cur).parent;
            if cur.is_null() {
                break;
            }
            if cur == (*ctx).node {
                return ptr::null_mut();
            }
            if !(*cur).next.is_null() {
                cur = (*cur).next;
                return cur;
            }
        }
        cur
    }
}

/// `xmlNodePtr xmlXPathNextDescendantOrSelf(xmlXPathParserContextPtr ctxt, xmlNodePtr cur)`.
///
/// # SAFETY
///
/// - `ctxt`, `cur` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlXPathNextDescendantOrSelf(
    ctxt: *mut c_void,
    cur: *mut _xmlNode,
) -> *mut _xmlNode {
    let pc = pc_from(ctxt);
    if pc.is_null() {
        return ptr::null_mut();
    }
    let ctx = unsafe { (*pc).context };
    if ctx.is_null() {
        return ptr::null_mut();
    }
    if cur.is_null() {
        return unsafe { (*ctx).node };
    }
    let node = unsafe { (*ctx).node };
    if node.is_null() {
        return ptr::null_mut();
    }
    use crate::abi::types::xmlElementType as ET;
    let t = unsafe { (*node).type_ };
    if t == ET::XML_ATTRIBUTE_NODE as c_int || t == ET::XML_NAMESPACE_DECL as c_int {
        return ptr::null_mut();
    }
    xmlXPathNextDescendant(ctxt, cur)
}

/// `xmlNodePtr xmlXPathNextParent(xmlXPathParserContextPtr ctxt, xmlNodePtr cur)`.
///
/// # SAFETY
///
/// - `ctxt`, `cur` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlXPathNextParent(
    ctxt: *mut c_void,
    cur: *mut _xmlNode,
) -> *mut _xmlNode {
    let pc = pc_from(ctxt);
    if pc.is_null() {
        return ptr::null_mut();
    }
    let ctx = unsafe { (*pc).context };
    if ctx.is_null() {
        return ptr::null_mut();
    }
    if !cur.is_null() {
        return ptr::null_mut();
    }
    next_parent_impl(ctx)
}

/// Shared parent resolution (upstream `xmlXPathNextParent` / `xmlXPathNextAncestor`).
unsafe fn next_parent_impl(ctx: *mut _xmlXPathContext) -> *mut _xmlNode {
    use crate::abi::types::xmlElementType as ET;
    let node = unsafe { (*ctx).node };
    if node.is_null() {
        return ptr::null_mut();
    }
    match unsafe { (*node).type_ } {
        t if t == ET::XML_ELEMENT_NODE as c_int
            || t == ET::XML_TEXT_NODE as c_int
            || t == ET::XML_CDATA_SECTION_NODE as c_int
            || t == ET::XML_ENTITY_REF_NODE as c_int
            || t == ET::XML_ENTITY_NODE as c_int
            || t == ET::XML_PI_NODE as c_int
            || t == ET::XML_COMMENT_NODE as c_int
            || t == ET::XML_NOTATION_NODE as c_int
            || t == ET::XML_DTD_NODE as c_int
            || t == ET::XML_ELEMENT_DECL as c_int
            || t == ET::XML_ATTRIBUTE_DECL as c_int
            || t == ET::XML_ENTITY_DECL as c_int
            || t == ET::XML_XINCLUDE_START as c_int
            || t == ET::XML_XINCLUDE_END as c_int =>
        unsafe {
            let parent = (*node).parent;
            if parent.is_null() {
                return (*ctx).doc as *mut _xmlNode;
            }
            if (*parent).type_ == ET::XML_ELEMENT_NODE as c_int
                && ((*parent).name.is_null() || *(*parent).name == b' ')
            {
                return ptr::null_mut();
            }
            parent
        },
        t if t == ET::XML_ATTRIBUTE_NODE as c_int => unsafe { (*(node as *mut _xmlAttr)).parent },
        t if t == ET::XML_DOCUMENT_NODE as c_int
            || t == ET::XML_DOCUMENT_TYPE_NODE as c_int
            || t == ET::XML_DOCUMENT_FRAG_NODE as c_int
            || t == ET::XML_HTML_DOCUMENT_NODE as c_int =>
        {
            ptr::null_mut()
        }
        t if t == ET::XML_NAMESPACE_DECL as c_int => unsafe {
            let ns = node as *mut crate::abi::structs::_xmlNs;
            if !(*ns).next.is_null() && (*(*ns).next).type_ != ET::XML_NAMESPACE_DECL as c_int {
                (*ns).next as *mut _xmlNode
            } else {
                ptr::null_mut()
            }
        },
        _ => ptr::null_mut(),
    }
}

/// `xmlNodePtr xmlXPathNextAncestor(xmlXPathParserContextPtr ctxt, xmlNodePtr cur)`.
///
/// # SAFETY
///
/// - `ctxt`, `cur` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlXPathNextAncestor(
    ctxt: *mut c_void,
    cur: *mut _xmlNode,
) -> *mut _xmlNode {
    let pc = pc_from(ctxt);
    if pc.is_null() {
        return ptr::null_mut();
    }
    let ctx = unsafe { (*pc).context };
    if ctx.is_null() {
        return ptr::null_mut();
    }
    use crate::abi::types::xmlElementType as ET;
    if cur.is_null() {
        let node = unsafe { (*ctx).node };
        if node.is_null() {
            return ptr::null_mut();
        }
        let t = unsafe { (*node).type_ };
        if t == ET::XML_ATTRIBUTE_NODE as c_int {
            return unsafe { (*(node as *mut _xmlAttr)).parent };
        }
        if t == ET::XML_NAMESPACE_DECL as c_int {
            let ns = node as *mut crate::abi::structs::_xmlNs;
            return unsafe {
                if !(*ns).next.is_null() && (*(*ns).next).type_ != ET::XML_NAMESPACE_DECL as c_int {
                    (*ns).next as *mut _xmlNode
                } else {
                    ptr::null_mut()
                }
            };
        }
        if t == ET::XML_DOCUMENT_NODE as c_int
            || t == ET::XML_DOCUMENT_TYPE_NODE as c_int
            || t == ET::XML_DOCUMENT_FRAG_NODE as c_int
            || t == ET::XML_HTML_DOCUMENT_NODE as c_int
        {
            return ptr::null_mut();
        }
        // element/text/cdata/entity-ref/entity/pi/comment/dtd/decls: parent or doc
        return next_parent_impl(ctx);
    }
    if cur == unsafe { (*ctx).doc } as *mut _xmlNode {
        return ptr::null_mut();
    }
    if cur == unsafe { (*(*ctx).doc).children } {
        return unsafe { (*ctx).doc } as *mut _xmlNode;
    }
    let t = unsafe { (*cur).type_ };
    if t == ET::XML_ATTRIBUTE_NODE as c_int {
        return unsafe { (*(cur as *mut _xmlAttr)).parent };
    }
    if t == ET::XML_NAMESPACE_DECL as c_int {
        let ns = cur as *mut crate::abi::structs::_xmlNs;
        return unsafe {
            if !(*ns).next.is_null() && (*(*ns).next).type_ != ET::XML_NAMESPACE_DECL as c_int {
                (*ns).next as *mut _xmlNode
            } else {
                ptr::null_mut()
            }
        };
    }
    if t == ET::XML_DOCUMENT_NODE as c_int
        || t == ET::XML_DOCUMENT_TYPE_NODE as c_int
        || t == ET::XML_DOCUMENT_FRAG_NODE as c_int
        || t == ET::XML_HTML_DOCUMENT_NODE as c_int
    {
        return ptr::null_mut();
    }
    unsafe {
        let parent = (*cur).parent;
        if parent.is_null() {
            return ptr::null_mut();
        }
        if (*parent).type_ == ET::XML_ELEMENT_NODE as c_int
            && ((*parent).name.is_null() || *(*parent).name == b' ')
        {
            return ptr::null_mut();
        }
        parent
    }
}

/// `xmlNodePtr xmlXPathNextAncestorOrSelf(xmlXPathParserContextPtr ctxt, xmlNodePtr cur)`.
///
/// # SAFETY
///
/// - `ctxt`, `cur` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlXPathNextAncestorOrSelf(
    ctxt: *mut c_void,
    cur: *mut _xmlNode,
) -> *mut _xmlNode {
    let pc = pc_from(ctxt);
    if pc.is_null() {
        return ptr::null_mut();
    }
    let ctx = unsafe { (*pc).context };
    if ctx.is_null() {
        return ptr::null_mut();
    }
    if cur.is_null() {
        return unsafe { (*ctx).node };
    }
    xmlXPathNextAncestor(ctxt, cur)
}

/// `xmlNodePtr xmlXPathNextFollowingSibling(xmlXPathParserContextPtr ctxt, xmlNodePtr cur)`.
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlXPathNextFollowingSibling(
    ctxt: *mut c_void,
    mut cur: *mut _xmlNode,
) -> *mut _xmlNode {
    let pc = pc_from(ctxt);
    if pc.is_null() {
        return ptr::null_mut();
    }
    let ctx = unsafe { (*pc).context };
    if ctx.is_null() {
        return ptr::null_mut();
    }
    use crate::abi::types::xmlElementType as ET;
    unsafe {
        let cnode = (*ctx).node;
        if !cnode.is_null() {
            let t = (*cnode).type_;
            if t == ET::XML_ATTRIBUTE_NODE as c_int || t == ET::XML_NAMESPACE_DECL as c_int {
                return ptr::null_mut();
            }
        }
        if cur == (*ctx).doc as *mut _xmlNode {
            return ptr::null_mut();
        }
        if cur.is_null() {
            cur = cnode;
        }
        if cur.is_null() {
            return ptr::null_mut();
        }
        if (*cur).type_ == ET::XML_DOCUMENT_NODE as c_int {
            return ptr::null_mut();
        }
        (*cur).next
    }
}

/// `xmlNodePtr xmlXPathNextPrecedingSibling(xmlXPathParserContextPtr ctxt, xmlNodePtr cur)`.
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlXPathNextPrecedingSibling(
    ctxt: *mut c_void,
    mut cur: *mut _xmlNode,
) -> *mut _xmlNode {
    let pc = pc_from(ctxt);
    if pc.is_null() {
        return ptr::null_mut();
    }
    let ctx = unsafe { (*pc).context };
    if ctx.is_null() {
        return ptr::null_mut();
    }
    use crate::abi::types::xmlElementType as ET;
    unsafe {
        let cnode = (*ctx).node;
        if !cnode.is_null() {
            let t = (*cnode).type_;
            if t == ET::XML_ATTRIBUTE_NODE as c_int || t == ET::XML_NAMESPACE_DECL as c_int {
                return ptr::null_mut();
            }
        }
        if cur == (*ctx).doc as *mut _xmlNode {
            return ptr::null_mut();
        }
        if cur.is_null() {
            cur = cnode;
        } else if !(*cur).prev.is_null() && (*(*cur).prev).type_ == ET::XML_DTD_NODE as c_int {
            cur = (*cur).prev;
            if cur.is_null() {
                return ptr::null_mut();
            }
        }
        if cur.is_null() {
            return ptr::null_mut();
        }
        if (*cur).type_ == ET::XML_DOCUMENT_NODE as c_int {
            return ptr::null_mut();
        }
        (*cur).prev
    }
}

/// `xmlNodePtr xmlXPathNextFollowing(xmlXPathParserContextPtr ctxt, xmlNodePtr cur)`.
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlXPathNextFollowing(
    ctxt: *mut c_void,
    mut cur: *mut _xmlNode,
) -> *mut _xmlNode {
    let pc = pc_from(ctxt);
    if pc.is_null() {
        return ptr::null_mut();
    }
    let ctx = unsafe { (*pc).context };
    if ctx.is_null() {
        return ptr::null_mut();
    }
    use crate::abi::types::xmlElementType as ET;
    unsafe {
        if !cur.is_null()
            && (*cur).type_ != ET::XML_ATTRIBUTE_NODE as c_int
            && (*cur).type_ != ET::XML_NAMESPACE_DECL as c_int
            && !(*cur).children.is_null()
        {
            return (*cur).children;
        }
        if cur.is_null() {
            cur = (*ctx).node;
            if cur.is_null() {
                return ptr::null_mut();
            }
            if (*cur).type_ == ET::XML_ATTRIBUTE_NODE as c_int {
                cur = (*cur).parent;
            } else if (*cur).type_ == ET::XML_NAMESPACE_DECL as c_int {
                let ns = cur as *mut crate::abi::structs::_xmlNs;
                if (*ns).next.is_null() || (*(*ns).next).type_ == ET::XML_NAMESPACE_DECL as c_int {
                    return ptr::null_mut();
                }
                cur = (*ns).next as *mut _xmlNode;
            }
        }
        if cur.is_null() {
            return ptr::null_mut();
        }
        if (*cur).type_ == ET::XML_DOCUMENT_NODE as c_int {
            return ptr::null_mut();
        }
        if !(*cur).next.is_null() {
            return (*cur).next;
        }
        loop {
            cur = (*cur).parent;
            if cur.is_null() {
                break;
            }
            if cur == (*ctx).doc as *mut _xmlNode {
                return ptr::null_mut();
            }
            if !(*cur).next.is_null() && (*cur).type_ != ET::XML_DOCUMENT_NODE as c_int {
                return (*cur).next;
            }
        }
        cur
    }
}

/// `xmlNodePtr xmlXPathNextPreceding(xmlXPathParserContextPtr ctxt, xmlNodePtr cur)`.
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlXPathNextPreceding(
    ctxt: *mut c_void,
    mut cur: *mut _xmlNode,
) -> *mut _xmlNode {
    let pc = pc_from(ctxt);
    if pc.is_null() {
        return ptr::null_mut();
    }
    let ctx = unsafe { (*pc).context };
    if ctx.is_null() {
        return ptr::null_mut();
    }
    use crate::abi::types::xmlElementType as ET;
    unsafe {
        let is_ancestor = |ancestor: *mut _xmlNode, node: *mut _xmlNode| -> bool {
            if ancestor.is_null() || node.is_null() {
                return false;
            }
            if (*node).type_ == ET::XML_NAMESPACE_DECL as c_int
                || (*ancestor).type_ == ET::XML_NAMESPACE_DECL as c_int
            {
                return false;
            }
            if (*ancestor).doc != (*node).doc {
                return false;
            }
            if ancestor == (*node).doc as *mut _xmlNode {
                return true;
            }
            if node == (*ancestor).doc as *mut _xmlNode {
                return false;
            }
            let mut n = node;
            while !(*n).parent.is_null() {
                if (*n).parent == ancestor {
                    return true;
                }
                n = (*n).parent;
            }
            false
        };
        if cur.is_null() {
            cur = (*ctx).node;
            if cur.is_null() {
                return ptr::null_mut();
            }
            if (*cur).type_ == ET::XML_ATTRIBUTE_NODE as c_int {
                cur = (*cur).parent;
            } else if (*cur).type_ == ET::XML_NAMESPACE_DECL as c_int {
                let ns = cur as *mut crate::abi::structs::_xmlNs;
                if (*ns).next.is_null() || (*(*ns).next).type_ == ET::XML_NAMESPACE_DECL as c_int {
                    return ptr::null_mut();
                }
                cur = (*ns).next as *mut _xmlNode;
            }
        }
        if cur.is_null() || (*cur).type_ == ET::XML_NAMESPACE_DECL as c_int {
            return ptr::null_mut();
        }
        if !(*cur).prev.is_null() && (*(*cur).prev).type_ == ET::XML_DTD_NODE as c_int {
            cur = (*cur).prev;
        }
        loop {
            if !(*cur).prev.is_null() {
                let mut n = (*cur).prev;
                while !(*n).last.is_null() {
                    n = (*n).last;
                }
                return n;
            }
            cur = (*cur).parent;
            if cur.is_null() {
                return ptr::null_mut();
            }
            if cur == (*(*ctx).doc).children {
                return ptr::null_mut();
            }
            if !is_ancestor(cur, (*ctx).node) {
                return cur;
            }
        }
    }
}

/// `xmlNodePtr xmlXPathNextNamespace(xmlXPathParserContextPtr ctxt, xmlNodePtr cur)`.
///
/// # SAFETY
///
/// - `ctxt`, `cur` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlXPathNextNamespace(
    ctxt: *mut c_void,
    cur: *mut _xmlNode,
) -> *mut _xmlNode {
    let pc = pc_from(ctxt);
    if pc.is_null() {
        return ptr::null_mut();
    }
    let ctx = unsafe { (*pc).context };
    if ctx.is_null() {
        return ptr::null_mut();
    }
    use crate::abi::types::xmlElementType as ET;
    unsafe {
        let cnode = (*ctx).node;
        if cnode.is_null() || (*cnode).type_ != ET::XML_ELEMENT_NODE as c_int {
            return ptr::null_mut();
        }
        if cur.is_null() {
            if !(*ctx).tmpNsList.is_null() {
                xmlFreeImpl((*ctx).tmpNsList as *mut c_void);
            }
            (*ctx).tmpNsNr = 0;
            (*ctx).tmpNsList = crate::xml::tree::get_ns_list((*ctx).doc, cnode);
            if !(*ctx).tmpNsList.is_null() {
                while !(*(*ctx).tmpNsList.add((*ctx).tmpNsNr as usize)).is_null() {
                    (*ctx).tmpNsNr += 1;
                }
            }
            return (&XML_XPATH_XML_NS.0) as *const crate::abi::structs::_xmlNs as *mut _xmlNode;
        }
        if (*ctx).tmpNsNr > 0 {
            (*ctx).tmpNsNr -= 1;
            return (*(*ctx).tmpNsList.add((*ctx).tmpNsNr as usize)) as *mut _xmlNode;
        }
        if !(*ctx).tmpNsList.is_null() {
            xmlFreeImpl((*ctx).tmpNsList as *mut c_void);
        }
        (*ctx).tmpNsList = ptr::null_mut();
        ptr::null_mut()
    }
}

/// `xmlNodePtr xmlXPathNextAttribute(xmlXPathParserContextPtr ctxt, xmlNodePtr cur)`.
///
/// # SAFETY
///
/// - `ctxt`, `cur` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlXPathNextAttribute(
    ctxt: *mut c_void,
    cur: *mut _xmlNode,
) -> *mut _xmlNode {
    let pc = pc_from(ctxt);
    if pc.is_null() {
        return ptr::null_mut();
    }
    let ctx = unsafe { (*pc).context };
    if ctx.is_null() {
        return ptr::null_mut();
    }
    use crate::abi::types::xmlElementType as ET;
    unsafe {
        let cnode = (*ctx).node;
        if cnode.is_null() || (*cnode).type_ != ET::XML_ELEMENT_NODE as c_int {
            return ptr::null_mut();
        }
        if cur.is_null() {
            if cnode == (*ctx).doc as *mut _xmlNode {
                return ptr::null_mut();
            }
            return (*cnode).properties as *mut _xmlNode;
        }
        (*cur).next
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// The explicit core function library (xmlXPath*Function)
// ═══════════════════════════════════════════════════════════════════════════════

/// `void xmlXPathBooleanFunction(xmlXPathParserContextPtr ctxt, int nargs)`.
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlXPathBooleanFunction(ctxt: *mut c_void, _nargs: c_int) {
    let pc = pc_from(ctxt);
    if pc.is_null() {
        return;
    }
    if !check_arity(pc, 1) {
        return;
    }
    let cur = value_pop(pc);
    if cur.is_null() {
        pc_set_error(pc, crate::abi::types::XPATH_INVALID_OPERAND as c_int);
        return;
    }
    let b = crate::abi::exports_xml2::object_to_xpathvalue_pub(cur).as_boolean();
    crate::abi::exports_xml2::xmlXPathFreeObject(cur);
    value_push(pc, new_bool(b));
}

/// `void xmlXPathNotFunction(xmlXPathParserContextPtr ctxt, int nargs)`.
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlXPathNotFunction(ctxt: *mut c_void, _nargs: c_int) {
    let pc = pc_from(ctxt);
    if pc.is_null() {
        return;
    }
    if !check_arity(pc, 1) {
        return;
    }
    cast_top_to_boolean(pc);
    if (*pc).error != 0 {
        return;
    }
    unsafe {
        (*(*pc).value).boolval = if (*(*pc).value).boolval == 0 { 1 } else { 0 };
    }
}

/// `void xmlXPathTrueFunction(xmlXPathParserContextPtr ctxt, int nargs)`.
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlXPathTrueFunction(ctxt: *mut c_void, _nargs: c_int) {
    let pc = pc_from(ctxt);
    if pc.is_null() {
        return;
    }

    value_push(pc, new_bool(true));
}

/// `void xmlXPathFalseFunction(xmlXPathParserContextPtr ctxt, int nargs)`.
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlXPathFalseFunction(ctxt: *mut c_void, _nargs: c_int) {
    let pc = pc_from(ctxt);
    if pc.is_null() {
        return;
    }

    value_push(pc, new_bool(false));
}

/// Upstream `lang()` semantics: `lang` matches the nearest ancestor/self
/// `xml:lang` attribute value, case-insensitively, with `-` sublanguage.
const unsafe fn lang_matches(lang: *const xmlChar, the_lang: *const xmlChar) -> bool {
    if lang.is_null() || the_lang.is_null() {
        return false;
    }
    let mut i = 0usize;
    loop {
        let lc = unsafe { *lang.add(i) };
        if lc == 0 {
            break;
        }
        let tc = unsafe { *the_lang.add(i) };
        if tc == 0 {
            return false;
        }
        if !lc.eq_ignore_ascii_case(&tc) {
            return false;
        }
        i += 1;
    }
    let c = unsafe { *the_lang.add(i) };
    c == 0 || c == b'-'
}

/// `void xmlXPathLangFunction(xmlXPathParserContextPtr ctxt, int nargs)`.
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlXPathLangFunction(ctxt: *mut c_void, _nargs: c_int) {
    let pc = pc_from(ctxt);
    if pc.is_null() {
        return;
    }
    let ctx = unsafe { (*pc).context };
    if ctx.is_null() {
        return;
    }
    if !check_arity(pc, 1) {
        return;
    }
    cast_top_to_string(pc);
    if (*pc).error != 0 {
        return;
    }
    let val = value_pop(pc);
    if val.is_null() {
        pc_set_error(pc, crate::abi::types::XPATH_INVALID_OPERAND as c_int);
        return;
    }
    let lang = unsafe { (*val).stringval };
    let mut ret = 0;
    unsafe {
        let mut n = (*ctx).node;
        let mut found: *mut xmlChar = ptr::null_mut();
        while !n.is_null() {
            let got = crate::xml::tree::get_ns_prop(
                n,
                c"lang".as_ptr() as *const xmlChar,
                XML_XML_NAMESPACE_BYTES.as_ptr() as *const xmlChar,
            );
            if !got.is_null() {
                found = got;
                break;
            }
            n = (*n).parent;
        }
        if !found.is_null() && lang_matches(lang, found) {
            ret = 1;
        }
        if !found.is_null() {
            xmlFreeImpl(found as *mut c_void);
        }
    }
    crate::abi::exports_xml2::xmlXPathFreeObject(val);
    value_push(pc, new_bool(ret != 0));
}

/// `void xmlXPathNumberFunction(xmlXPathParserContextPtr ctxt, int nargs)`.
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlXPathNumberFunction(ctxt: *mut c_void, nargs: c_int) {
    let pc = pc_from(ctxt);
    if pc.is_null() {
        return;
    }
    let ctx = unsafe { (*pc).context };
    if ctx.is_null() {
        return;
    }
    if nargs == 0 {
        let node = unsafe { (*ctx).node };
        let res = if node.is_null() {
            0.0
        } else {
            let sv = node_string_value(node);
            crate::xml::xpath::types::string_to_number(&sv)
        };
        value_push(pc, new_number(res));
        return;
    }
    if !check_arity(pc, 1) {
        return;
    }
    cast_top_to_number(pc);
}

/// `void xmlXPathSumFunction(xmlXPathParserContextPtr ctxt, int nargs)`.
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlXPathSumFunction(ctxt: *mut c_void, _nargs: c_int) {
    let pc = pc_from(ctxt);
    if pc.is_null() {
        return;
    }
    if !check_arity(pc, 1) {
        return;
    }
    let cur = value_pop(pc);
    if cur.is_null() {
        pc_set_error(pc, crate::abi::types::XPATH_INVALID_OPERAND as c_int);
        return;
    }
    let typ = unsafe { (*cur).type_ };
    if typ != xmlXPathObjectType::XPATH_NODESET as c_int
        && typ != xmlXPathObjectType::XPATH_XSLT_TREE as c_int
    {
        crate::abi::exports_xml2::xmlXPathFreeObject(cur);
        pc_set_error(pc, crate::abi::types::XPATH_INVALID_TYPE as c_int);
        return;
    }
    let mut res = 0.0;
    unsafe {
        let ns = (*cur).nodesetval as *mut _xmlNodeSet;
        if !ns.is_null() {
            let nr = (*ns).nodeNr;
            let tab = (*ns).nodeTab;
            if !tab.is_null() {
                for i in 0..nr as isize {
                    let node = *tab.add(i as usize);
                    let sv = node_string_value(node);
                    res += crate::xml::xpath::types::string_to_number(&sv);
                }
            }
        }
    }
    crate::abi::exports_xml2::xmlXPathFreeObject(cur);
    value_push(pc, new_number(res));
}

/// `void xmlXPathFloorFunction(xmlXPathParserContextPtr ctxt, int nargs)`.
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlXPathFloorFunction(ctxt: *mut c_void, _nargs: c_int) {
    let pc = pc_from(ctxt);
    if pc.is_null() {
        return;
    }
    if !check_arity(pc, 1) {
        return;
    }
    cast_top_to_number(pc);
    if (*pc).error != 0 {
        return;
    }
    unsafe {
        (*(*pc).value).floatval = (*(*pc).value).floatval.floor();
    }
}

/// `void xmlXPathCeilingFunction(xmlXPathParserContextPtr ctxt, int nargs)`.
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlXPathCeilingFunction(ctxt: *mut c_void, _nargs: c_int) {
    let pc = pc_from(ctxt);
    if pc.is_null() {
        return;
    }
    if !check_arity(pc, 1) {
        return;
    }
    cast_top_to_number(pc);
    if (*pc).error != 0 {
        return;
    }
    unsafe {
        (*(*pc).value).floatval = (*(*pc).value).floatval.ceil();
    }
}

/// `void xmlXPathRoundFunction(xmlXPathParserContextPtr ctxt, int nargs)`.
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlXPathRoundFunction(ctxt: *mut c_void, _nargs: c_int) {
    let pc = pc_from(ctxt);
    if pc.is_null() {
        return;
    }
    if !check_arity(pc, 1) {
        return;
    }
    cast_top_to_number(pc);
    if (*pc).error != 0 {
        return;
    }
    unsafe {
        let f = (*(*pc).value).floatval;
        if (-0.5..0.5).contains(&f) {
            // Handles negative zero.
            (*(*pc).value).floatval *= 0.0;
        } else {
            let mut rounded = f.floor();
            if f - rounded >= 0.5 {
                rounded += 1.0;
            }
            (*(*pc).value).floatval = rounded;
        }
    }
}

/// `void xmlXPathLastFunction(xmlXPathParserContextPtr ctxt, int nargs)`.
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlXPathLastFunction(ctxt: *mut c_void, _nargs: c_int) {
    let pc = pc_from(ctxt);
    if pc.is_null() {
        return;
    }
    let ctx = unsafe { (*pc).context };
    if ctx.is_null() {
        return;
    }

    value_push(pc, new_number(unsafe { (*ctx).contextSize } as f64));
}

/// `void xmlXPathPositionFunction(xmlXPathParserContextPtr ctxt, int nargs)`.
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlXPathPositionFunction(ctxt: *mut c_void, _nargs: c_int) {
    let pc = pc_from(ctxt);
    if pc.is_null() {
        return;
    }
    let ctx = unsafe { (*pc).context };
    if ctx.is_null() {
        return;
    }

    value_push(pc, new_number(unsafe { (*ctx).proximityPosition } as f64));
}

/// `void xmlXPathCountFunction(xmlXPathParserContextPtr ctxt, int nargs)`.
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlXPathCountFunction(ctxt: *mut c_void, _nargs: c_int) {
    let pc = pc_from(ctxt);
    if pc.is_null() {
        return;
    }
    if !check_arity(pc, 1) {
        return;
    }
    let cur = value_pop(pc);
    if cur.is_null() {
        pc_set_error(pc, crate::abi::types::XPATH_INVALID_OPERAND as c_int);
        return;
    }
    let typ = unsafe { (*cur).type_ };
    if typ != xmlXPathObjectType::XPATH_NODESET as c_int
        && typ != xmlXPathObjectType::XPATH_XSLT_TREE as c_int
    {
        crate::abi::exports_xml2::xmlXPathFreeObject(cur);
        pc_set_error(pc, crate::abi::types::XPATH_INVALID_TYPE as c_int);
        return;
    }
    let count = unsafe {
        let ns = (*cur).nodesetval as *mut _xmlNodeSet;
        if ns.is_null() {
            0
        } else {
            (*ns).nodeNr
        }
    };
    crate::abi::exports_xml2::xmlXPathFreeObject(cur);
    value_push(pc, new_number(count as f64));
}

/// Elements selected by whitespace-separated ID tokens (upstream
/// `xmlXPathGetElementsByIds`).
unsafe fn get_elements_by_ids(doc: *mut _xmlDoc, ids: *const xmlChar) -> *mut _xmlNodeSet {
    use crate::abi::types::xmlElementType as ET;
    if ids.is_null() {
        return ptr::null_mut();
    }
    let mut out = NodeSet::new();
    unsafe {
        let mut p = ids;
        while *p != 0 {
            while is_blank_ch(*p) {
                p = p.add(1);
            }
            if *p == 0 {
                break;
            }
            let start = p;
            while *p != 0 && !is_blank_ch(*p) {
                p = p.add(1);
            }
            let id_c = crate::xml::string::xml_strndup(start, p.offset_from(start) as usize);
            if id_c.is_null() {
                break;
            }
            let attr = get_id(doc, id_c);
            xmlFreeImpl(id_c as *mut c_void);
            if !attr.is_null() {
                let t = (*attr).type_;
                let elem = if t == ET::XML_ATTRIBUTE_NODE as c_int {
                    (*attr).parent
                } else if t == ET::XML_ELEMENT_NODE as c_int {
                    attr as *mut _xmlNode
                } else {
                    ptr::null_mut()
                };
                if !elem.is_null() {
                    out.push(elem);
                }
            }
        }
    }
    out.to_raw()
}

/// `void xmlXPathIdFunction(xmlXPathParserContextPtr ctxt, int nargs)`.
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlXPathIdFunction(ctxt: *mut c_void, _nargs: c_int) {
    let pc = pc_from(ctxt);
    if pc.is_null() {
        return;
    }
    let ctx = unsafe { (*pc).context };
    if ctx.is_null() {
        return;
    }
    if !check_arity(pc, 1) {
        return;
    }
    let obj = value_pop(pc);
    if obj.is_null() {
        pc_set_error(pc, crate::abi::types::XPATH_INVALID_OPERAND as c_int);
        return;
    }
    let doc = unsafe { (*ctx).doc };
    let v = crate::abi::exports_xml2::object_to_xpathvalue_pub(obj);
    crate::abi::exports_xml2::xmlXPathFreeObject(obj);
    match &v {
        XPathValue::NodeSet(ns) => {
            let mut merged = NodeSet::new();
            for n in ns.iter() {
                let sv = node_string_value(n);
                let c = dup_rust_string(&sv);
                let sub = get_elements_by_ids(doc, c);
                xmlFreeImpl(c as *mut c_void);
                if !sub.is_null() {
                    let sub_internal = node_set_to_internal(sub);
                    for m in sub_internal.iter() {
                        if !merged.contains(m) {
                            merged.push(m);
                        }
                    }
                    // Release the raw node-set (nodes are borrowed).
                    crate::abi::exports_xml2::xmlXPathFreeNodeSet(sub);
                }
            }
            value_push(
                pc,
                crate::abi::exports_xml2::xpath_to_object_pub(XPathValue::NodeSet(merged)),
            );
        }
        _ => {
            let s = v.as_string();
            let c = dup_rust_string(&s);
            let ret = get_elements_by_ids(doc, c);
            xmlFreeImpl(c as *mut c_void);
            if ret.is_null() {
                value_push(
                    pc,
                    crate::abi::exports_xml2::xpath_to_object_pub(XPathValue::NodeSet(
                        NodeSet::new(),
                    )),
                );
            } else {
                let obj2 = crate::abi::exports_xml2::xpath_to_object_pub(XPathValue::NodeSet(
                    node_set_to_internal(ret),
                ));
                crate::abi::exports_xml2::xmlXPathFreeNodeSet(ret);
                value_push(pc, obj2);
            }
        }
    }
}

/// Local part of a node name (upstream `xmlXPathLocalNameFunction` first-node
/// logic). Returns an empty string when the node has no local name.
unsafe fn node_local_name(node: *mut _xmlNode) -> String {
    use crate::abi::types::xmlElementType as ET;
    if node.is_null() {
        return String::new();
    }
    unsafe {
        match (*node).type_ {
            t if t == ET::XML_ELEMENT_NODE as c_int
                || t == ET::XML_ATTRIBUTE_NODE as c_int
                || t == ET::XML_PI_NODE as c_int =>
            {
                let name = (*node).name;
                if name.is_null() || *name == b' ' {
                    String::new()
                } else {
                    let s = CStr::from_ptr(name as *const c_char)
                        .to_string_lossy()
                        .into_owned();
                    match s.split_once(':') {
                        Some((_, local)) => local.to_string(),
                        None => s,
                    }
                }
            }
            t if t == ET::XML_NAMESPACE_DECL as c_int => {
                let ns = node as *mut crate::abi::structs::_xmlNs;
                let p = (*ns).prefix;
                if p.is_null() {
                    String::new()
                } else {
                    CStr::from_ptr(p as *const c_char)
                        .to_string_lossy()
                        .into_owned()
                }
            }
            _ => String::new(),
        }
    }
}

/// `void xmlXPathLocalNameFunction(xmlXPathParserContextPtr ctxt, int nargs)`.
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlXPathLocalNameFunction(ctxt: *mut c_void, nargs: c_int) {
    let pc = pc_from(ctxt);
    if pc.is_null() {
        return;
    }
    let ctx = unsafe { (*pc).context };
    if ctx.is_null() {
        return;
    }
    if nargs == 0 {
        value_push(
            pc,
            crate::abi::exports_xml2::xpath_to_object_pub(XPathValue::NodeSet(NodeSet::singleton(
                unsafe { (*ctx).node },
            ))),
        );
        // fallthrough with nargs = 1
    }

    if !check_arity(pc, 1) {
        return;
    }
    let cur = value_pop(pc);
    if cur.is_null() {
        pc_set_error(pc, crate::abi::types::XPATH_INVALID_OPERAND as c_int);
        return;
    }
    let typ = unsafe { (*cur).type_ };
    if typ != xmlXPathObjectType::XPATH_NODESET as c_int
        && typ != xmlXPathObjectType::XPATH_XSLT_TREE as c_int
    {
        crate::abi::exports_xml2::xmlXPathFreeObject(cur);
        pc_set_error(pc, crate::abi::types::XPATH_INVALID_TYPE as c_int);
        return;
    }
    let name = unsafe {
        let ns = (*cur).nodesetval as *mut _xmlNodeSet;
        if ns.is_null() || (*ns).nodeNr == 0 {
            String::new()
        } else {
            node_local_name(*(*ns).nodeTab)
        }
    };
    crate::abi::exports_xml2::xmlXPathFreeObject(cur);
    let out = dup_rust_string(&name);
    value_push(pc, xmlXPathWrapString(out));
}

/// Namespace URI of a node (upstream `xmlXPathNamespaceURIFunction`).
unsafe fn node_namespace_uri(node: *mut _xmlNode) -> String {
    use crate::abi::types::xmlElementType as ET;
    if node.is_null() {
        return String::new();
    }
    unsafe {
        match (*node).type_ {
            t if t == ET::XML_ELEMENT_NODE as c_int || t == ET::XML_ATTRIBUTE_NODE as c_int => {
                let ns = (*node).ns;
                if ns.is_null() || (*ns).href.is_null() {
                    String::new()
                } else {
                    CStr::from_ptr((*ns).href as *const c_char)
                        .to_string_lossy()
                        .into_owned()
                }
            }
            t if t == ET::XML_NAMESPACE_DECL as c_int => {
                let ns = node as *mut crate::abi::structs::_xmlNs;
                if (*ns).href.is_null() {
                    String::new()
                } else {
                    CStr::from_ptr((*ns).href as *const c_char)
                        .to_string_lossy()
                        .into_owned()
                }
            }
            _ => String::new(),
        }
    }
}

/// `void xmlXPathNamespaceURIFunction(xmlXPathParserContextPtr ctxt, int nargs)`.
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlXPathNamespaceURIFunction(ctxt: *mut c_void, nargs: c_int) {
    let pc = pc_from(ctxt);
    if pc.is_null() {
        return;
    }
    let ctx = unsafe { (*pc).context };
    if ctx.is_null() {
        return;
    }
    if nargs == 0 {
        value_push(
            pc,
            crate::abi::exports_xml2::xpath_to_object_pub(XPathValue::NodeSet(NodeSet::singleton(
                unsafe { (*ctx).node },
            ))),
        );
    }

    if !check_arity(pc, 1) {
        return;
    }
    let cur = value_pop(pc);
    if cur.is_null() {
        pc_set_error(pc, crate::abi::types::XPATH_INVALID_OPERAND as c_int);
        return;
    }
    let typ = unsafe { (*cur).type_ };
    if typ != xmlXPathObjectType::XPATH_NODESET as c_int
        && typ != xmlXPathObjectType::XPATH_XSLT_TREE as c_int
    {
        crate::abi::exports_xml2::xmlXPathFreeObject(cur);
        pc_set_error(pc, crate::abi::types::XPATH_INVALID_TYPE as c_int);
        return;
    }
    let uri = unsafe {
        let ns = (*cur).nodesetval as *mut _xmlNodeSet;
        if ns.is_null() || (*ns).nodeNr == 0 {
            String::new()
        } else {
            node_namespace_uri(*(*ns).nodeTab)
        }
    };
    crate::abi::exports_xml2::xmlXPathFreeObject(cur);
    let out = dup_rust_string(&uri);
    value_push(pc, xmlXPathWrapString(out));
}

/// `void xmlXPathStringFunction(xmlXPathParserContextPtr ctxt, int nargs)`.
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlXPathStringFunction(ctxt: *mut c_void, nargs: c_int) {
    let pc = pc_from(ctxt);
    if pc.is_null() {
        return;
    }
    let ctx = unsafe { (*pc).context };
    if ctx.is_null() {
        return;
    }
    if nargs == 0 {
        let node = unsafe { (*ctx).node };
        let sv = if node.is_null() {
            String::new()
        } else {
            node_string_value(node)
        };
        let out = dup_rust_string(&sv);
        value_push(pc, xmlXPathWrapString(out));
        return;
    }
    if !check_arity(pc, 1) {
        return;
    }
    cast_top_to_string(pc);
}

/// `void xmlXPathStringLengthFunction(xmlXPathParserContextPtr ctxt, int nargs)`.
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlXPathStringLengthFunction(ctxt: *mut c_void, nargs: c_int) {
    let pc = pc_from(ctxt);
    if pc.is_null() {
        return;
    }
    let ctx = unsafe { (*pc).context };
    if ctx.is_null() {
        return;
    }
    if nargs == 0 {
        let node = unsafe { (*ctx).node };
        let len = if node.is_null() {
            0
        } else {
            let sv = node_string_value(node);
            sv.chars().count()
        };
        value_push(pc, new_number(len as f64));
        return;
    }
    if !check_arity(pc, 1) {
        return;
    }
    cast_top_to_string(pc);
    if (*pc).error != 0 {
        return;
    }
    let len = unsafe {
        let s = (*(*pc).value).stringval;
        if s.is_null() {
            0
        } else {
            let sv = CStr::from_ptr(s as *const c_char).to_string_lossy();
            sv.chars().count()
        }
    };
    let cur = value_pop(pc);
    if !cur.is_null() {
        crate::abi::exports_xml2::xmlXPathFreeObject(cur);
    }
    value_push(pc, new_number(len as f64));
}

/// `void xmlXPathConcatFunction(xmlXPathParserContextPtr ctxt, int nargs)`.
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlXPathConcatFunction(ctxt: *mut c_void, nargs: c_int) {
    let pc = pc_from(ctxt);
    if pc.is_null() {
        return;
    }
    if nargs < 2 && !check_arity(pc, 2) {
        return;
    }
    if !check_arity(pc, nargs) {
        return;
    }
    let mut parts: Vec<String> = Vec::with_capacity(nargs as usize);
    for _ in 0..nargs {
        cast_top_to_string(pc);
        if (*pc).error != 0 {
            return;
        }
        let v = crate::abi::exports_xml2::object_to_xpathvalue_pub(unsafe { (*pc).value });
        let s = v.as_string();
        let obj = value_pop(pc);
        if !obj.is_null() {
            crate::abi::exports_xml2::xmlXPathFreeObject(obj);
        }
        parts.push(s);
    }
    parts.reverse();
    let joined = parts.concat();
    let out = dup_rust_string(&joined);
    value_push(pc, xmlXPathWrapString(out));
}

/// `void xmlXPathContainsFunction(xmlXPathParserContextPtr ctxt, int nargs)`.
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlXPathContainsFunction(ctxt: *mut c_void, _nargs: c_int) {
    let pc = pc_from(ctxt);
    if pc.is_null() {
        return;
    }
    if !check_arity(pc, 2) {
        return;
    }
    cast_top_to_string(pc);
    if (*pc).error != 0 {
        return;
    }
    let needle = value_pop(pc);
    cast_top_to_string(pc);
    if (*pc).error != 0 {
        if !needle.is_null() {
            crate::abi::exports_xml2::xmlXPathFreeObject(needle);
        }
        return;
    }
    let hay = value_pop(pc);
    let found = if hay.is_null() || needle.is_null() {
        false
    } else {
        unsafe { !cstr_find((*hay).stringval, (*needle).stringval).is_null() }
    };
    if !hay.is_null() {
        crate::abi::exports_xml2::xmlXPathFreeObject(hay);
    }
    if !needle.is_null() {
        crate::abi::exports_xml2::xmlXPathFreeObject(needle);
    }
    value_push(pc, new_bool(found));
}

/// `void xmlXPathStartsWithFunction(xmlXPathParserContextPtr ctxt, int nargs)`.
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlXPathStartsWithFunction(ctxt: *mut c_void, _nargs: c_int) {
    let pc = pc_from(ctxt);
    if pc.is_null() {
        return;
    }
    if !check_arity(pc, 2) {
        return;
    }
    cast_top_to_string(pc);
    if (*pc).error != 0 {
        return;
    }
    let needle = value_pop(pc);
    cast_top_to_string(pc);
    if (*pc).error != 0 {
        if !needle.is_null() {
            crate::abi::exports_xml2::xmlXPathFreeObject(needle);
        }
        return;
    }
    let hay = value_pop(pc);
    let found = if hay.is_null() || needle.is_null() {
        false
    } else {
        unsafe { crate::xml::string::xml_str_starts_with((*hay).stringval, (*needle).stringval) }
    };
    if !hay.is_null() {
        crate::abi::exports_xml2::xmlXPathFreeObject(hay);
    }
    if !needle.is_null() {
        crate::abi::exports_xml2::xmlXPathFreeObject(needle);
    }
    value_push(pc, new_bool(found));
}

/// `void xmlXPathSubstringFunction(xmlXPathParserContextPtr ctxt, int nargs)`.
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlXPathSubstringFunction(ctxt: *mut c_void, nargs: c_int) {
    let pc = pc_from(ctxt);
    if pc.is_null() {
        return;
    }
    if nargs < 2 {
        if !check_arity(pc, 2) {
            return;
        }
    } else if nargs > 3 && !check_arity(pc, 3) {
        return;
    }
    let mut le = 0.0;
    if nargs == 3 {
        cast_top_to_number(pc);
        if (*pc).error != 0 {
            return;
        }
        let len_obj = value_pop(pc);
        if !len_obj.is_null() {
            le = unsafe { (*len_obj).floatval };
            crate::abi::exports_xml2::xmlXPathFreeObject(len_obj);
        }
    }
    cast_top_to_number(pc);
    if (*pc).error != 0 {
        return;
    }
    let start_obj = value_pop(pc);
    let in_ = if start_obj.is_null() {
        f64::NAN
    } else {
        let v = unsafe { (*start_obj).floatval };
        crate::abi::exports_xml2::xmlXPathFreeObject(start_obj);
        v
    };
    cast_top_to_string(pc);
    if (*pc).error != 0 {
        return;
    }
    let str_obj = value_pop(pc);
    let s = if str_obj.is_null() {
        String::new()
    } else {
        let v = unsafe { CStr::from_ptr((*str_obj).stringval as *const c_char) }
            .to_string_lossy()
            .into_owned();
        crate::abi::exports_xml2::xmlXPathFreeObject(str_obj);
        v
    };

    let int_max = i32::MAX as f64;
    let mut i: i64 = 1;
    let mut j: i64 = i32::MAX as i64;
    // UPSTREAM-PARITY: `!(in < int_max)` mirrors xpath.c xmlXPathSubstring
    // verbatim; rewriting it as `in >= int_max` would change NaN handling
    // (the oracle treats NaN as "not less" -> clamps to INT_MAX).
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    if !(in_ < int_max) {
        i = i32::MAX as i64;
    } else if in_ >= 1.0 {
        i = in_ as i64;
        if in_ - in_.floor() >= 0.5 {
            i += 1;
        }
    }
    if nargs == 3 {
        let mut rin = in_.floor();
        if in_ - rin >= 0.5 {
            rin += 1.0;
        }
        let mut rle = le.floor();
        if le - rle >= 0.5 {
            rle += 1.0;
        }
        let end = rin + rle;
        #[allow(clippy::neg_cmp_op_on_partial_ord)]
        if !(end >= 1.0) {
            j = 1;
        } else if end < int_max {
            j = end as i64;
        }
    }
    i -= 1;
    j -= 1;
    let chars: Vec<char> = s.chars().collect();
    let slen = chars.len() as i64;
    let out = if i < j && i < slen {
        let start_i = i.max(0) as usize;
        let end_i = (j.min(slen)).max(start_i as i64) as usize;
        chars[start_i..end_i].iter().collect()
    } else {
        String::new()
    };
    let c = dup_rust_string(&out);
    value_push(pc, xmlXPathWrapString(c));
}

/// `void xmlXPathSubstringBeforeFunction(xmlXPathParserContextPtr ctxt, int nargs)`.
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlXPathSubstringBeforeFunction(ctxt: *mut c_void, _nargs: c_int) {
    let pc = pc_from(ctxt);
    if pc.is_null() {
        return;
    }
    if !check_arity(pc, 2) {
        return;
    }
    cast_top_to_string(pc);
    if (*pc).error != 0 {
        return;
    }
    let find = value_pop(pc);
    cast_top_to_string(pc);
    if (*pc).error != 0 {
        if !find.is_null() {
            crate::abi::exports_xml2::xmlXPathFreeObject(find);
        }
        return;
    }
    let str_obj = value_pop(pc);
    let out: String = if str_obj.is_null() || find.is_null() {
        String::new()
    } else {
        unsafe {
            let hay = (*str_obj).stringval;
            let needle = (*find).stringval;
            let point = cstr_find(hay, needle);
            if point.is_null() {
                String::new()
            } else {
                let len = point.offset_from(hay) as usize;
                let bytes = core::slice::from_raw_parts(hay as *const u8, len);
                String::from_utf8_lossy(bytes).into_owned()
            }
        }
    };
    if !str_obj.is_null() {
        crate::abi::exports_xml2::xmlXPathFreeObject(str_obj);
    }
    if !find.is_null() {
        crate::abi::exports_xml2::xmlXPathFreeObject(find);
    }
    let c = dup_rust_string(&out);
    value_push(pc, xmlXPathWrapString(c));
}

/// `void xmlXPathSubstringAfterFunction(xmlXPathParserContextPtr ctxt, int nargs)`.
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlXPathSubstringAfterFunction(ctxt: *mut c_void, _nargs: c_int) {
    let pc = pc_from(ctxt);
    if pc.is_null() {
        return;
    }
    if !check_arity(pc, 2) {
        return;
    }
    cast_top_to_string(pc);
    if (*pc).error != 0 {
        return;
    }
    let find = value_pop(pc);
    cast_top_to_string(pc);
    if (*pc).error != 0 {
        if !find.is_null() {
            crate::abi::exports_xml2::xmlXPathFreeObject(find);
        }
        return;
    }
    let str_obj = value_pop(pc);
    let out: String = if str_obj.is_null() || find.is_null() {
        String::new()
    } else {
        unsafe {
            let hay = (*str_obj).stringval;
            let needle = (*find).stringval;
            let point = cstr_find(hay, needle);
            if point.is_null() {
                String::new()
            } else {
                let nlen = crate::xml::string::xml_strlen(needle);
                let rest = point.add(nlen);
                let len = crate::xml::string::xml_strlen(rest);
                let bytes = core::slice::from_raw_parts(rest, len);
                String::from_utf8_lossy(bytes).into_owned()
            }
        }
    };
    if !str_obj.is_null() {
        crate::abi::exports_xml2::xmlXPathFreeObject(str_obj);
    }
    if !find.is_null() {
        crate::abi::exports_xml2::xmlXPathFreeObject(find);
    }
    let c = dup_rust_string(&out);
    value_push(pc, xmlXPathWrapString(c));
}

/// `void xmlXPathNormalizeFunction(xmlXPathParserContextPtr ctxt, int nargs)`.
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlXPathNormalizeFunction(ctxt: *mut c_void, nargs: c_int) {
    let pc = pc_from(ctxt);
    if pc.is_null() {
        return;
    }
    let ctx = unsafe { (*pc).context };
    if ctx.is_null() {
        return;
    }
    if nargs == 0 {
        let node = unsafe { (*ctx).node };
        let sv = if node.is_null() {
            String::new()
        } else {
            node_string_value(node)
        };
        let c = dup_rust_string(&sv);
        value_push(pc, xmlXPathWrapString(c));
        // fallthrough with nargs = 1
    }

    if !check_arity(pc, 1) {
        return;
    }
    cast_top_to_string(pc);
    if (*pc).error != 0 {
        return;
    }
    let s = unsafe {
        let p = (*(*pc).value).stringval;
        if p.is_null() {
            String::new()
        } else {
            CStr::from_ptr(p as *const c_char)
                .to_string_lossy()
                .into_owned()
        }
    };
    // Strip leading/trailing blanks; collapse internal runs to a single space.
    let mut out = String::with_capacity(s.len());
    let mut blank = false;
    let mut started = false;
    for c in s.chars() {
        let is_b = c == ' ' || c == '\t' || c == '\n' || c == '\r';
        if is_b {
            if started {
                blank = true;
            }
        } else {
            if blank {
                out.push(' ');
                blank = false;
            }
            out.push(c);
            started = true;
        }
    }
    unsafe {
        let val = (*pc).value;
        if !(*val).stringval.is_null() {
            xmlFreeImpl((*val).stringval as *mut c_void);
        }
        (*val).stringval = dup_rust_string(&out);
    }
}

/// `void xmlXPathTranslateFunction(xmlXPathParserContextPtr ctxt, int nargs)`.
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlXPathTranslateFunction(ctxt: *mut c_void, _nargs: c_int) {
    let pc = pc_from(ctxt);
    if pc.is_null() {
        return;
    }
    if !check_arity(pc, 3) {
        return;
    }
    cast_top_to_string(pc);
    if (*pc).error != 0 {
        return;
    }
    let to = value_pop(pc);
    cast_top_to_string(pc);
    if (*pc).error != 0 {
        if !to.is_null() {
            crate::abi::exports_xml2::xmlXPathFreeObject(to);
        }
        return;
    }
    let from = value_pop(pc);
    cast_top_to_string(pc);
    if (*pc).error != 0 {
        if !to.is_null() {
            crate::abi::exports_xml2::xmlXPathFreeObject(to);
        }
        if !from.is_null() {
            crate::abi::exports_xml2::xmlXPathFreeObject(from);
        }
        return;
    }
    let str_obj = value_pop(pc);
    let (s, f, t) = unsafe {
        let s = if str_obj.is_null() || (*str_obj).stringval.is_null() {
            String::new()
        } else {
            CStr::from_ptr((*str_obj).stringval as *const c_char)
                .to_string_lossy()
                .into_owned()
        };
        let f = if from.is_null() || (*from).stringval.is_null() {
            String::new()
        } else {
            CStr::from_ptr((*from).stringval as *const c_char)
                .to_string_lossy()
                .into_owned()
        };
        let t = if to.is_null() || (*to).stringval.is_null() {
            String::new()
        } else {
            CStr::from_ptr((*to).stringval as *const c_char)
                .to_string_lossy()
                .into_owned()
        };
        (s, f, t)
    };
    if !str_obj.is_null() {
        crate::abi::exports_xml2::xmlXPathFreeObject(str_obj);
    }
    if !from.is_null() {
        crate::abi::exports_xml2::xmlXPathFreeObject(from);
    }
    if !to.is_null() {
        crate::abi::exports_xml2::xmlXPathFreeObject(to);
    }
    let from_chars: Vec<char> = f.chars().collect();
    let to_chars: Vec<char> = t.chars().collect();
    // UPSTREAM-PARITY: a character in `from` with no corresponding `to`
    // character (from longer than to) is removed from the output.
    let out: String = s
        .chars()
        .filter_map(|c| match from_chars.iter().position(|&x| x == c) {
            Some(i) if i < to_chars.len() => Some(to_chars[i]),
            Some(_) => None,
            _ => Some(c),
        })
        .collect();
    let c = dup_rust_string(&out);
    value_push(pc, xmlXPathWrapString(c));
}

/// `void xmlXPathRegisterAllFunctions(xmlXPathContextPtr ctxt)` — no-op since
/// 2.14.0 (the core library is compiled in; upstream keeps an empty body).
///
/// # SAFETY
///
/// - `_ctxt` must be valid pointers (or NULL
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
pub const unsafe extern "C" fn xmlXPathRegisterAllFunctions(_ctxt: *mut _xmlXPathContext) {}

/// Standard core function name → exported C shim pointer (upstream
/// `xmlXPathStandardFunctions` table).
unsafe fn standard_function_pointer(
    name: &str,
) -> Option<unsafe extern "C" fn(*mut c_void, c_int)> {
    let f: unsafe extern "C" fn(*mut c_void, c_int) = match name {
        "boolean" => xmlXPathBooleanFunction,
        "not" => xmlXPathNotFunction,
        "true" => xmlXPathTrueFunction,
        "false" => xmlXPathFalseFunction,
        "lang" => xmlXPathLangFunction,
        "number" => xmlXPathNumberFunction,
        "sum" => xmlXPathSumFunction,
        "floor" => xmlXPathFloorFunction,
        "ceiling" => xmlXPathCeilingFunction,
        "round" => xmlXPathRoundFunction,
        "last" => xmlXPathLastFunction,
        "position" => xmlXPathPositionFunction,
        "count" => xmlXPathCountFunction,
        "id" => xmlXPathIdFunction,
        "local-name" => xmlXPathLocalNameFunction,
        "namespace-uri" => xmlXPathNamespaceURIFunction,
        "string" => xmlXPathStringFunction,
        "string-length" => xmlXPathStringLengthFunction,
        "concat" => xmlXPathConcatFunction,
        "contains" => xmlXPathContainsFunction,
        "starts-with" => xmlXPathStartsWithFunction,
        "substring" => xmlXPathSubstringFunction,
        "substring-before" => xmlXPathSubstringBeforeFunction,
        "substring-after" => xmlXPathSubstringAfterFunction,
        "normalize-space" => xmlXPathNormalizeFunction,
        "translate" => xmlXPathTranslateFunction,
        _ => return None,
    };
    Some(f)
}

/// `xmlXPathFunction xmlXPathFunctionLookup(xmlXPathContextPtr ctxt, const xmlChar *name)`.
///
/// # SAFETY
///
/// - `ctxt` must be a valid context or NULL; `name` a valid string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathFunctionLookup(
    ctxt: *mut _xmlXPathContext,
    name: *const xmlChar,
) -> Option<unsafe extern "C" fn(*mut c_void, c_int)> {
    xmlXPathFunctionLookupNS(ctxt, name, ptr::null())
}

/// `xmlXPathFunction xmlXPathFunctionLookupNS(xmlXPathContextPtr ctxt, const xmlChar *name, const xmlChar *ns_uri)`.
///
/// # SAFETY
///
/// - `ctxt` must be a valid context or NULL; `name` a valid string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathFunctionLookupNS(
    ctxt: *mut _xmlXPathContext,
    name: *const xmlChar,
    ns_uri: *const xmlChar,
) -> Option<unsafe extern "C" fn(*mut c_void, c_int)> {
    if ctxt.is_null() || name.is_null() {
        return None;
    }
    let name_str = match CStr::from_ptr(name as *const c_char).to_str() {
        Ok(s) => s.to_string(),
        Err(_) => return None,
    };
    if ns_uri.is_null() {
        if let Some(f) = standard_function_pointer(&name_str) {
            return Some(f);
        }
    }
    // User function-lookup callback first, then the C-registered hash.
    if let Some(f) = (*ctxt).funcLookupFunc {
        let ret = f((*ctxt).funcLookupData, name, ns_uri);
        if !ret.is_null() {
            // The callback stores an xmlXPathFunction (fn pointer) as void*.
            let fp =
                std::mem::transmute::<*mut c_void, unsafe extern "C" fn(*mut c_void, c_int)>(ret);
            return Some(fp);
        }
    }
    let qualified = if ns_uri.is_null() {
        name_str
    } else {
        let ns = match CStr::from_ptr(ns_uri as *const c_char).to_str() {
            Ok(s) => s,
            Err(_) => return None,
        };
        format!("{{{}}}{}", ns, name_str)
    };
    crate::abi::exports_xml2::xpath_cfunc_lookup((*ctxt).extra, &qualified)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Context / compiled-expression handling
// ═══════════════════════════════════════════════════════════════════════════════

/// `xmlXPathCompExpr *xmlXPathCtxtCompile(xmlXPathContextPtr ctxt, const xmlChar *str)`.
///
/// The candidate compiles name tests without context-dependent prefix
/// resolution at compile time (prefixes resolve during evaluation), so the
/// result equals `xmlXPathCompile` for every expression.
///
/// # SAFETY
///
/// - `ctxt` may be NULL; `str` must be a valid string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathCtxtCompile(
    _ctxt: *mut _xmlXPathContext,
    str_: *const xmlChar,
) -> *mut c_void {
    crate::abi::exports_xml2::xmlXPathCompile(str_)
}

/// `xmlXPathObject *xmlXPathCompiledEval(xmlXPathCompExpr *comp, xmlXPathContext *ctx)`.
///
/// # SAFETY
///
/// - `comp` must be a compiled expression or NULL; `ctx` a valid context.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathCompiledEval(
    comp: *mut c_void,
    ctx: *mut _xmlXPathContext,
) -> *mut _xmlXPathObject {
    if comp.is_null() || ctx.is_null() {
        return ptr::null_mut();
    }
    let internal = (*ctx).extra as *mut XPathContext;
    if internal.is_null() {
        return ptr::null_mut();
    }
    let internal = &mut *internal;
    let registry = crate::abi::exports_xml2::xpath_compiled_registry();
    let map = registry.lock();
    match map.get(&(comp as u64)) {
        Some(compiled) => match crate::xml::xpath::evaluate(compiled, internal) {
            Some(val) => crate::abi::exports_xml2::xpath_to_object_pub(val),
            None => ptr::null_mut(),
        },
        None => ptr::null_mut(),
    }
}

/// `int xmlXPathCompiledEvalToBoolean(xmlXPathCompExpr *comp, xmlXPathContext *ctxt)`.
///
/// Returns 1 / 0 for the boolean result, -1 on error.
///
/// # SAFETY
///
/// - `comp` must be a compiled expression or NULL; `ctxt` a valid context.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathCompiledEvalToBoolean(
    comp: *mut c_void,
    ctxt: *mut _xmlXPathContext,
) -> c_int {
    let obj = xmlXPathCompiledEval(comp, ctxt);
    if obj.is_null() {
        return -1;
    }
    let b = crate::abi::exports_xml2::object_to_xpathvalue_pub(obj).as_boolean();
    crate::abi::exports_xml2::xmlXPathFreeObject(obj);
    b as c_int
}

/// `int xmlXPathSetContextNode(xmlNodePtr node, xmlXPathContextPtr ctx)` —
/// sets the context node; fails when the node belongs to a different document.
///
/// # SAFETY
///
/// - `node` / `ctx` must be valid or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathSetContextNode(
    node: *mut _xmlNode,
    ctx: *mut _xmlXPathContext,
) -> c_int {
    if node.is_null() || ctx.is_null() {
        return -1;
    }
    if (*node).doc != (*ctx).doc {
        return -1;
    }
    (*ctx).node = node;
    let internal = (*ctx).extra as *mut XPathContext;
    if !internal.is_null() {
        (*internal).context_node = node;
    }
    0
}

/// `xmlXPathObject *xmlXPathNodeEval(xmlNodePtr node, const xmlChar *str, xmlXPathContextPtr ctx)`.
///
/// # SAFETY
///
/// - `node` / `ctx` must be valid or NULL; `str` a valid string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathNodeEval(
    node: *mut _xmlNode,
    str_: *const xmlChar,
    ctx: *mut _xmlXPathContext,
) -> *mut _xmlXPathObject {
    if str_.is_null() {
        return ptr::null_mut();
    }
    if xmlXPathSetContextNode(node, ctx) < 0 {
        return ptr::null_mut();
    }
    crate::abi::exports_xml2::xmlXPathEvalExpression(str_, ctx)
}

/// `int xmlXPathContextSetCache(xmlXPathContextPtr ctxt, int active, int value, int options)`.
///
/// The candidate has no object cache; the call is accepted and recorded
/// (active ⇒ a marker in `ctxt->cache`), returning 0 on success.
///
/// # SAFETY
///
/// - `ctxt` must be a valid context or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathContextSetCache(
    ctxt: *mut _xmlXPathContext,
    active: c_int,
    _value: c_int,
    _options: c_int,
) -> c_int {
    if ctxt.is_null() {
        return -1;
    }
    (*ctxt).cache = if active != 0 {
        (&XML_XPATH_XML_NS.0) as *const crate::abi::structs::_xmlNs as *mut c_void
    } else {
        ptr::null_mut()
    };
    0
}

/// `void xmlXPathRegisterFuncLookup(xmlXPathContextPtr ctxt, xmlXPathFuncLookupFunc f, void *funcCtxt)`.
///
/// # SAFETY
///
/// - `ctxt` must be a valid context or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathRegisterFuncLookup(
    ctxt: *mut _xmlXPathContext,
    f: Option<crate::abi::callbacks::xmlXPathFuncLookupFunc>,
    data: *mut c_void,
) {
    if ctxt.is_null() {
        return;
    }
    (*ctxt).funcLookupFunc = f;
    (*ctxt).funcLookupData = data;
    let internal = (*ctxt).extra as *mut XPathContext;
    if !internal.is_null() {
        (*internal).func_lookup_func = f;
        (*internal).func_lookup_data = data;
    }
}

/// `void xmlXPathRegisterVariableLookup(xmlXPathContextPtr ctxt, xmlXPathVariableLookupFunc f, void *data)`.
///
/// # SAFETY
///
/// - `ctxt` must be a valid context or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathRegisterVariableLookup(
    ctxt: *mut _xmlXPathContext,
    f: Option<crate::abi::callbacks::xmlXPathVariableLookupFunc>,
    data: *mut c_void,
) {
    if ctxt.is_null() {
        return;
    }
    (*ctxt).varLookupFunc = f;
    (*ctxt).varLookupData = data;
    let internal = (*ctxt).extra as *mut XPathContext;
    if !internal.is_null() {
        (*internal).var_lookup_func = f;
        (*internal).var_lookup_data = data;
    }
}

/// `int xmlXPathRegisterVariableNS(xmlXPathContextPtr ctxt, const xmlChar *name, const xmlChar *ns_uri, xmlXPathObjectPtr value)`.
///
/// # SAFETY
///
/// - `ctxt` must be a valid context; `name`/`value` valid; `ns_uri` may be NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathRegisterVariableNS(
    ctxt: *mut _xmlXPathContext,
    name: *const xmlChar,
    ns_uri: *const xmlChar,
    value: *mut _xmlXPathObject,
) -> c_int {
    if ctxt.is_null() || name.is_null() || value.is_null() {
        return -1;
    }
    let internal = (*ctxt).extra as *mut XPathContext;
    if internal.is_null() {
        return -1;
    }
    let internal = &mut *internal;
    let name_str = match CStr::from_ptr(name as *const c_char).to_str() {
        Ok(s) => s.to_string(),
        Err(_) => return -1,
    };
    let qualified = if ns_uri.is_null() {
        name_str
    } else {
        match CStr::from_ptr(ns_uri as *const c_char).to_str() {
            Ok(s) => format!("{{{}}}{}", s, name_str),
            Err(_) => return -1,
        }
    };
    let xpath_val = crate::abi::exports_xml2::object_to_xpathvalue_pub(value);
    internal.register_variable(&qualified, xpath_val);
    0
}

/// `xmlXPathObjectPtr xmlXPathVariableLookup(xmlXPathContextPtr ctxt, const xmlChar *name)`.
///
/// # SAFETY
///
/// - `ctxt` must be a valid context; `name` a valid string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathVariableLookup(
    ctxt: *mut _xmlXPathContext,
    name: *const xmlChar,
) -> *mut _xmlXPathObject {
    if ctxt.is_null() {
        return ptr::null_mut();
    }
    if let Some(f) = (*ctxt).varLookupFunc {
        let ret = f((*ctxt).varLookupData, name, ptr::null());
        return ret;
    }
    xmlXPathVariableLookupNS(ctxt, name, ptr::null())
}

/// `xmlXPathObjectPtr xmlXPathVariableLookupNS(xmlXPathContextPtr ctxt, const xmlChar *name, const xmlChar *ns_uri)`.
///
/// # SAFETY
///
/// - `ctxt` must be a valid context; `name` a valid string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathVariableLookupNS(
    ctxt: *mut _xmlXPathContext,
    name: *const xmlChar,
    ns_uri: *const xmlChar,
) -> *mut _xmlXPathObject {
    if ctxt.is_null() || name.is_null() {
        return ptr::null_mut();
    }
    if let Some(f) = (*ctxt).varLookupFunc {
        let ret = f((*ctxt).varLookupData, name, ns_uri);
        if !ret.is_null() {
            return ret;
        }
    }
    let internal = (*ctxt).extra as *mut XPathContext;
    if internal.is_null() {
        return ptr::null_mut();
    }
    let internal = &*internal;
    let name_str = match CStr::from_ptr(name as *const c_char).to_str() {
        Ok(s) => s.to_string(),
        Err(_) => return ptr::null_mut(),
    };
    let qualified = if ns_uri.is_null() {
        name_str
    } else {
        match CStr::from_ptr(ns_uri as *const c_char).to_str() {
            Ok(s) => format!("{{{}}}{}", s, name_str),
            Err(_) => return ptr::null_mut(),
        }
    };
    match internal.variables.get(&qualified) {
        Some(v) => crate::abi::exports_xml2::xpath_to_object_pub(v.clone()),
        None => ptr::null_mut(),
    }
}

/// `const xmlChar *xmlXPathNsLookup(xmlXPathContextPtr ctxt, const xmlChar *prefix)`.
///
/// # SAFETY
///
/// - `ctxt` must be a valid context; `prefix` a valid string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathNsLookup(
    ctxt: *mut _xmlXPathContext,
    prefix: *const xmlChar,
) -> *const xmlChar {
    if ctxt.is_null() || prefix.is_null() {
        return ptr::null();
    }
    // The xml prefix always maps to the XML namespace (upstream).
    if cstr_eq(prefix, c"xml".as_ptr() as *const xmlChar) {
        return XML_XML_NAMESPACE_BYTES.as_ptr() as *const xmlChar;
    }
    // In-scope namespace declarations on the context.
    let namespaces = (*ctxt).namespaces;
    if !namespaces.is_null() {
        for i in 0..(*ctxt).nsNr as isize {
            let ns = *namespaces.add(i as usize);
            if !ns.is_null() && !(*ns).prefix.is_null() && cstr_eq((*ns).prefix, prefix) {
                return (*ns).href;
            }
        }
    }
    // Registered namespace hash (owned C strings, upstream xmlXPathRegisterNs
    // stores strdup'd URIs in ctxt->nsHash; the candidate mirrors that).
    if !(*ctxt).nsHash.is_null() {
        let map = &*((*ctxt).nsHash as *const HashMap<String, CString>);
        let p = CStr::from_ptr(prefix as *const c_char)
            .to_string_lossy()
            .into_owned();
        if let Some(c) = map.get(&p) {
            return c.as_ptr() as *const xmlChar;
        }
    }
    ptr::null()
}

/// `void xmlXPathRegisteredFuncsCleanup(xmlXPathContextPtr ctxt)`.
///
/// # SAFETY
///
/// - `ctxt` must be a valid context or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathRegisteredFuncsCleanup(ctxt: *mut _xmlXPathContext) {
    if ctxt.is_null() {
        return;
    }
    let internal = (*ctxt).extra as *mut XPathContext;
    if !internal.is_null() {
        (*internal).functions.clear();
    }
    crate::abi::exports_xml2::xpath_cfunc_cleanup((*ctxt).extra);
}

/// `void xmlXPathRegisteredVariablesCleanup(xmlXPathContextPtr ctxt)`.
///
/// # SAFETY
///
/// - `ctxt` must be a valid context or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathRegisteredVariablesCleanup(ctxt: *mut _xmlXPathContext) {
    if ctxt.is_null() {
        return;
    }
    let internal = (*ctxt).extra as *mut XPathContext;
    if !internal.is_null() {
        (*internal).variables.clear();
    }
}

/// `void xmlXPathRegisteredNsCleanup(xmlXPathContextPtr ctxt)`.
///
/// # SAFETY
///
/// - `ctxt` must be a valid context or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathRegisteredNsCleanup(ctxt: *mut _xmlXPathContext) {
    if ctxt.is_null() {
        return;
    }
    let internal = (*ctxt).extra as *mut XPathContext;
    if !internal.is_null() {
        (*internal).namespaces.clear();
    }
    if !(*ctxt).nsHash.is_null() {
        drop(Box::from_raw(
            (*ctxt).nsHash as *mut HashMap<String, CString>,
        ));
        (*ctxt).nsHash = ptr::null_mut();
    }
}

/// `void xmlXPathSetErrorHandler(xmlXPathContextPtr ctxt, xmlStructuredErrorFunc handler, void *context)`.
///
/// # SAFETY
///
/// - `ctxt` must be a valid context or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathSetErrorHandler(
    ctxt: *mut _xmlXPathContext,
    handler: Option<crate::abi::callbacks::xmlStructuredErrorFunc>,
    data: *mut c_void,
) {
    if ctxt.is_null() {
        return;
    }
    (*ctxt).error = handler;
    (*ctxt).userData = data;
}

extern "C" {
    fn fwrite(ptr: *const c_void, size: usize, nmemb: usize, stream: *mut c_void) -> usize;
}

unsafe fn dump_write(output: *mut c_void, s: &str) {
    unsafe {
        fwrite(s.as_ptr() as *const c_void, 1, s.len(), output);
    }
}

/// `void xmlXPathDebugDumpObject(FILE *output, xmlXPathObject *cur, int depth)`.
///
/// # SAFETY
///
/// - `output` must be a valid FILE* or NULL; `cur` a valid object or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathDebugDumpObject(
    output: *mut c_void,
    cur: *mut _xmlXPathObject,
    depth: c_int,
) {
    if output.is_null() {
        return;
    }
    let mut s = String::new();
    for _ in 0..depth.clamp(0, 25) {
        s.push_str("  ");
    }
    if cur.is_null() {
        s.push_str("Object is empty (NULL)\n");
        dump_write(output, &s);
        return;
    }
    unsafe {
        match (*cur).type_ {
            t if t == xmlXPathObjectType::XPATH_BOOLEAN as c_int => {
                s.push_str("Object is a Boolean : ");
                s.push_str(if (*cur).boolval != 0 {
                    "true\n"
                } else {
                    "false\n"
                });
            }
            t if t == xmlXPathObjectType::XPATH_NUMBER as c_int => {
                let f = (*cur).floatval;
                if f.is_nan() {
                    s.push_str("Object is a number : NaN\n");
                } else if f == f64::INFINITY {
                    s.push_str("Object is a number : Infinity\n");
                } else if f == f64::NEG_INFINITY {
                    s.push_str("Object is a number : -Infinity\n");
                } else if f == 0.0 {
                    s.push_str("Object is a number : 0\n");
                } else {
                    s.push_str("Object is a number : ");
                    s.push_str(&f.to_string());
                    s.push('\n');
                }
            }
            t if t == xmlXPathObjectType::XPATH_STRING as c_int => {
                s.push_str("Object is a string : ");
                if (*cur).stringval.is_null() {
                    s.push_str("(null)");
                } else {
                    let sv = CStr::from_ptr((*cur).stringval as *const c_char).to_string_lossy();
                    s.push_str(&sv);
                }
                s.push('\n');
            }
            t if t == xmlXPathObjectType::XPATH_NODESET as c_int => {
                s.push_str("Object is a Node Set :\n");
                let ns = (*cur).nodesetval as *mut _xmlNodeSet;
                if !ns.is_null() {
                    for _ in 0..=depth.min(24) {
                        s.push_str("  ");
                    }
                    s.push_str(&format!("Object contains {} nodes\n", (*ns).nodeNr));
                }
            }
            t if t == xmlXPathObjectType::XPATH_XSLT_TREE as c_int => {
                s.push_str("Object is an XSLT value tree :\n");
            }
            t if t == xmlXPathObjectType::XPATH_USERS as c_int => {
                s.push_str("Object is user defined\n");
            }
            _ => {
                s.push_str("Object is uninitialized\n");
            }
        }
    }
    dump_write(output, &s);
}

/// `void xmlXPathDebugDumpCompExpr(FILE *output, xmlXPathCompExpr *comp, int depth)`.
///
/// The candidate's compiled expressions are opaque registry handles; the dump
/// prints the original expression text. NULL handles print nothing (matching
/// upstream's early return).
///
/// # SAFETY
///
/// - `output` must be a valid FILE* or NULL; `comp` a compiled expression or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlXPathDebugDumpCompExpr(
    output: *mut c_void,
    comp: *mut c_void,
    depth: c_int,
) {
    if output.is_null() || comp.is_null() {
        return;
    }
    let registry = crate::abi::exports_xml2::xpath_compiled_registry();
    let map = registry.lock();
    if let Some(compiled) = map.get(&(comp as u64)) {
        let mut s = String::new();
        for _ in 0..depth.clamp(0, 25) {
            s.push_str("  ");
        }
        s.push_str("Compiled Expression : ");
        s.push_str(&compiled.original);
        s.push('\n');
        dump_write(output, &s);
    }
}

#[allow(unused)]
const fn _unused_xpath_batch(_: *mut _xmlAttr) {}
