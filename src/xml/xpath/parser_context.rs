//! XPath parser context and value stack (§25, 11.1-I XPath family closure).
//!
//! Upstream libxml2 exposes `xmlXPathParserContextPtr` (a fully public struct
//! in the headers) together with a value stack (`xmlXPathPush*` /
//! `xmlXPathPop*`) and stack-based value operators (`xmlXPathAddValues` etc.).
//!
//! # UPSTREAM-PARITY (field layout)
//!
//! The candidate struct layout mirrors the upstream `_xmlXPathParserContext`
//! field-for-field (see include/libxml/xpath.h and upstream xpath.h):
//!
//! ```c
//! struct _xmlXPathParserContext {
//!     const xmlChar *cur;      /* the current char being parsed */
//!     const xmlChar *base;     /* the full expression */
//!     int error;               /* error code */
//!     xmlXPathContext *context;/* the evaluation context */
//!     xmlXPathObject *value;   /* the current value */
//!     int valueNr;             /* number of values stacked */
//!     int valueMax;            /* max number of values stacked */
//!     xmlXPathObject **valueTab;/* stack of values */
//!     xmlXPathCompExpr *comp;  /* the precompiled expression */
//!     int xptr;                /* it this an XPointer expression */
//!     xmlNode *ancestor;       /* used for walking preceding axis */
//!     int valueFrame;          /* always zero for compatibility */
//! };
//! ```
//!
//! The XPATH-001 differential court verifies the stack operators and the
//! parser-context APIs against the oracle.

#![allow(missing_docs)]

use core::ffi::c_void;
use core::ptr;
use std::os::raw::{c_char, c_int};

use crate::abi::allocator::{xmlFreeImpl, xmlMallocImpl, xmlMallocZero};
use crate::abi::structs::{_xmlDoc, _xmlNode, _xmlNodeSet, _xmlXPathContext, _xmlXPathObject};
use crate::abi::types::{xmlChar, xmlXPathObjectType};
use crate::xml::xpath::types::{node_string_value, NodeSet, XPathValue};

/// The parser-context stack depth (upstream `XML_XPATH_STACK_BYTES` / 10 slots).
const VALUE_TAB_SIZE: usize = 10;

/// The public `xmlXPathParserContext` (layout mirrors upstream xpath.h).
#[repr(C)]
#[derive(Debug)]
pub struct XmlXPathParserContext {
    pub cur: *const xmlChar,
    pub base: *const xmlChar,
    pub error: c_int,
    pub context: *mut _xmlXPathContext,
    pub value: *mut _xmlXPathObject,
    pub value_nr: c_int,
    pub value_max: c_int,
    pub value_tab: *mut *mut _xmlXPathObject,
    pub comp: *mut c_void,
    pub xptr: c_int,
    pub ancestor: *mut _xmlNode,
    pub value_frame: c_int,
}

/// Set the parser-context error code (upstream `xmlXPathSetError` /
/// `XP_ERROR`).
///
/// # SAFETY
///
/// - `pc` must be a valid parser context.
pub unsafe fn pc_set_error(pc: *mut XmlXPathParserContext, code: c_int) {
    if pc.is_null() {
        return;
    }
    unsafe { (*pc).error = code };
}

/// Create a new parser context over `str` with the given evaluation context.
///
/// # SAFETY
///
/// - `str` must be a valid NUL-terminated string or NULL.
/// - `ctxt` must be a valid `_xmlXPathContext` or NULL.
pub unsafe fn new_parser_context(
    str_: *const xmlChar,
    ctxt: *mut _xmlXPathContext,
) -> *mut XmlXPathParserContext {
    let pc = xmlMallocZero(size_of::<XmlXPathParserContext>()) as *mut XmlXPathParserContext;
    if pc.is_null() {
        return ptr::null_mut();
    }
    let tab =
        xmlMallocImpl(VALUE_TAB_SIZE * size_of::<*mut _xmlXPathObject>()) as *mut *mut _xmlXPathObject;
    if tab.is_null() {
        xmlFreeImpl(pc as *mut c_void);
        return ptr::null_mut();
    }
    unsafe {
        (*pc).cur = str_;
        (*pc).base = str_;
        (*pc).context = ctxt;
        (*pc).value_nr = 0;
        (*pc).value_max = VALUE_TAB_SIZE as c_int;
        (*pc).value_tab = tab;
        (*pc).value = ptr::null_mut();
        (*pc).ancestor = ptr::null_mut();
        (*pc).value_frame = 0;
    }
    pc
}

/// Free a parser context.
///
/// # SAFETY
///
/// - `pc` must be a valid parser context or NULL.
pub unsafe fn free_parser_context(pc: *mut XmlXPathParserContext) {
    if pc.is_null() {
        return;
    }
    unsafe {
        // Free any remaining stack values. `value` always aliases the top of
        // the stack (or NULL when empty), so it must not be freed separately.
        let tab = (*pc).value_tab;
        if !tab.is_null() {
            for i in 0..(*pc).value_nr as usize {
                let obj = *tab.add(i);
                if !obj.is_null() {
                    crate::abi::exports_xml2::xmlXPathFreeObject(obj);
                }
            }
            xmlFreeImpl(tab as *mut c_void);
        }
        xmlFreeImpl(pc as *mut c_void);
    }
}

/// Push a value object onto the stack (upstream `valuePush`).
///
/// # SAFETY
///
/// - `pc` must be a valid parser context.
pub unsafe fn value_push(
    pc: *mut XmlXPathParserContext,
    val: *mut _xmlXPathObject,
) -> *mut _xmlXPathObject {
    if pc.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        let nr = (*pc).value_nr as usize;
        if nr >= VALUE_TAB_SIZE {
            // UPSTREAM-PARITY: stack overflow leaves the value untouched and
            // reports an XPATH_STACK_ERROR.
            (*pc).error = crate::abi::types::XPATH_STACK_ERROR as c_int;
            return val;
        }
        ptr::write((*pc).value_tab.add(nr), val);
        (*pc).value_nr = nr as c_int + 1;
        (*pc).value = val;
        val
    }
}

/// Pop a value object from the stack (upstream `valuePop`).
///
/// # SAFETY
///
/// - `pc` must be a valid parser context.
pub unsafe fn value_pop(pc: *mut XmlXPathParserContext) -> *mut _xmlXPathObject {
    if pc.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        let nr = (*pc).value_nr;
        if nr <= 0 {
            return ptr::null_mut();
        }
        let idx = (nr - 1) as usize;
        let val = *(*pc).value_tab.add(idx);
        ptr::write((*pc).value_tab.add(idx), ptr::null_mut());
        (*pc).value_nr = nr - 1;
        (*pc).value = if (*pc).value_nr > 0 {
            *(*pc).value_tab.add((*pc).value_nr as usize - 1)
        } else {
            ptr::null_mut()
        };
        val
    }
}

/// Build a boolean object.
///
/// # SAFETY
///
/// - The returned object is heap-allocated; the caller owns it.
pub unsafe fn new_bool(b: bool) -> *mut _xmlXPathObject {
    let obj = xmlMallocZero(size_of::<_xmlXPathObject>()) as *mut _xmlXPathObject;
    if !obj.is_null() {
        unsafe {
            (*obj).type_ = xmlXPathObjectType::XPATH_BOOLEAN as c_int;
            (*obj).boolval = if b { 1 } else { 0 };
        }
    }
    obj
}

/// Build a number object.
pub unsafe fn new_number(n: f64) -> *mut _xmlXPathObject {
    let obj = xmlMallocZero(size_of::<_xmlXPathObject>()) as *mut _xmlXPathObject;
    if !obj.is_null() {
        unsafe {
            (*obj).type_ = xmlXPathObjectType::XPATH_NUMBER as c_int;
            (*obj).floatval = n;
        }
    }
    obj
}

/// Build a string object (copies the value).
///
/// # SAFETY
///
/// - `s` must be a valid NUL-terminated string or NULL.
pub unsafe fn new_string(s: *const xmlChar) -> *mut _xmlXPathObject {
    crate::xml::xpath::exports::xmlXPathNewString(s)
}

/// Pop a number (upstream `xmlXPathPopNumber`): converts the top of the stack
/// to a number, frees it, returns the value.
///
/// # SAFETY
///
/// - `pc` must be a valid parser context or NULL.
pub unsafe fn pop_number(pc: *mut XmlXPathParserContext) -> f64 {
    if pc.is_null() {
        return f64::NAN;
    }
    let arg = unsafe { value_pop(pc) };
    if arg.is_null() {
        return f64::NAN;
    }
    let n = crate::abi::exports_xml2::object_to_xpathvalue_pub(arg).as_number();
    crate::abi::exports_xml2::xmlXPathFreeObject(arg);
    n
}

/// Pop a boolean.
///
/// # SAFETY
///
/// - `pc` must be a valid parser context or NULL.
pub unsafe fn pop_boolean(pc: *mut XmlXPathParserContext) -> c_int {
    if pc.is_null() {
        return 0;
    }
    let arg = unsafe { value_pop(pc) };
    if arg.is_null() {
        return 0;
    }
    let b = crate::abi::exports_xml2::object_to_xpathvalue_pub(arg).as_boolean();
    crate::abi::exports_xml2::xmlXPathFreeObject(arg);
    b as c_int
}

/// Pop a string (freshly allocated; caller frees with xmlFree).
///
/// # SAFETY
///
/// - `pc` must be a valid parser context or NULL.
pub unsafe fn pop_string(pc: *mut XmlXPathParserContext) -> *mut xmlChar {
    if pc.is_null() {
        return ptr::null_mut();
    }
    let arg = unsafe { value_pop(pc) };
    if arg.is_null() {
        return ptr::null_mut();
    }
    let s = crate::abi::exports_xml2::object_to_xpathvalue_pub(arg).as_string();
    crate::abi::exports_xml2::xmlXPathFreeObject(arg);
    crate::xml::string::xml_strdup(s.as_bytes().as_ptr() as *const xmlChar)
}

/// Pop a node set (freshly allocated; caller frees with xmlXPathFreeNodeSet).
///
/// # SAFETY
///
/// - `pc` must be a valid parser context or NULL.
pub unsafe fn pop_node_set(pc: *mut XmlXPathParserContext) -> *mut _xmlNodeSet {
    if pc.is_null() {
        return ptr::null_mut();
    }
    let arg = unsafe { value_pop(pc) };
    if arg.is_null() {
        return ptr::null_mut();
    }
    let ns_ptr = unsafe { (*arg).nodesetval as *mut _xmlNodeSet };
    // Detach the node set from the object before freeing the object.
    unsafe { (*arg).nodesetval = ptr::null_mut() };
    crate::abi::exports_xml2::xmlXPathFreeObject(arg);
    ns_ptr
}

/// Pop an external (user) pointer.
///
/// # SAFETY
///
/// - `pc` must be a valid parser context or NULL.
pub unsafe fn pop_external(pc: *mut XmlXPathParserContext) -> *mut c_void {
    if pc.is_null() {
        return ptr::null_mut();
    }
    let arg = unsafe { value_pop(pc) };
    if arg.is_null() {
        return ptr::null_mut();
    }
    let user = unsafe { (*arg).user };
    crate::abi::exports_xml2::xmlXPathFreeObject(arg);
    user
}

/// Pop two number operands from the stack and apply `op`.
///
/// UPSTREAM-PARITY: the first popped value is the RIGHT operand
/// (xmlXPathAddValues pops arg (rhs) then arg (lhs)).
///
/// # SAFETY
///
/// - `pc` must be a valid parser context.
pub unsafe fn binary_number_op(pc: *mut XmlXPathParserContext, op: impl Fn(f64, f64) -> f64) {
    let rhs = unsafe { pop_number(pc) };
    let lhs = unsafe { pop_number(pc) };
    let r = op(lhs, rhs);
    unsafe { value_push(pc, new_number(r)) };
}

/// Number → number conversion of the top-of-stack object, in place
/// (upstream `CAST_TO_NUMBER`): the object's type becomes XPATH_NUMBER and
/// `floatval` holds the converted value. A NULL top (or a USERS object)
/// reports XPATH_INVALID_OPERAND.
///
/// # SAFETY
///
/// - `pc` must be a valid parser context with a non-NULL `value`.
pub unsafe fn cast_top_to_number(pc: *mut XmlXPathParserContext) {
    unsafe {
        let val = (*pc).value;
        if val.is_null() {
            pc_set_error(pc, crate::abi::types::XPATH_INVALID_OPERAND as c_int);
            return;
        }
        if (*val).type_ != xmlXPathObjectType::XPATH_NUMBER as c_int {
            let v = crate::abi::exports_xml2::object_to_xpathvalue_pub(val);
            (*val).floatval = v.as_number();
            (*val).type_ = xmlXPathObjectType::XPATH_NUMBER as c_int;
        }
    }
}

/// String-value equality of two node-set members (upstream
/// `xmlXPathEqualNodeSets` core loop; the hash fast-path is not observable).
fn node_set_pair_equal(a: &NodeSet, b: &NodeSet, neq: bool) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }
    // For "=", an identical node pointer in both sets short-circuits to true
    // (upstream `xmlXPathEqualNodeSets`).
    if !neq {
        for na in a.iter() {
            if b.contains(na) {
                return true;
            }
        }
    }
    for na in a.iter() {
        let sa = node_string_value(na);
        for nb in b.iter() {
            let sb = node_string_value(nb);
            if (sa == sb) ^ neq {
                return true;
            }
        }
    }
    false
}

/// Node-set vs string comparison (upstream `xmlXPathEqualNodeSetString`).
fn node_set_string_equal(a: &NodeSet, s: &str, neq: bool) -> bool {
    if a.is_empty() {
        return false;
    }
    for n in a.iter() {
        let sv = node_string_value(n);
        if sv == s {
            if neq {
                continue;
            }
            return true;
        } else if neq {
            return true;
        }
    }
    false
}

/// Node-set vs number comparison (upstream `xmlXPathEqualNodeSetFloat`).
fn node_set_number_equal(a: &NodeSet, f: f64, neq: bool) -> bool {
    for n in a.iter() {
        let sv = node_string_value(n);
        let v = crate::xml::xpath::types::string_to_number(&sv);
        if v.is_nan() {
            if neq {
                return true;
            }
        } else if (!neq && v == f) || (neq && v != f) {
            return true;
        }
    }
    false
}

/// Full XPath 1.0 equality matrix (§3.4) matching upstream 2.15
/// `xmlXPathEqualValues` / `xmlXPathNotEqualValues` (including the node-set
/// pair/string/number/boolean special cases).
pub fn equal_values_inner(v1: &XPathValue, v2: &XPathValue, neq: bool) -> bool {
    // Normalise so that, when exactly one side is a node-set, `ns` is the
    // node-set side (upstream swaps so arg1 is the node-set).
    match (v1, v2) {
        (XPathValue::NodeSet(a), XPathValue::NodeSet(b)) => node_set_pair_equal(a, b, neq),
        (XPathValue::NodeSet(a), other) => node_set_vs_value(a, other, neq),
        (other, XPathValue::NodeSet(b)) => node_set_vs_value(b, other, neq),
        _ => common_equal(v1, v2) ^ neq,
    }
}

fn node_set_vs_value(ns: &NodeSet, other: &XPathValue, neq: bool) -> bool {
    match other {
        XPathValue::Boolean(b) => (!ns.is_empty()) == *b,
        XPathValue::Number(f) => node_set_number_equal(ns, *f, neq),
        XPathValue::String(s) => node_set_string_equal(ns, s, neq),
        // Both sides node-sets are handled by the caller before this point.
        XPathValue::NodeSet(_) => unreachable!(),
    }
}

/// The non-node-set equality matrix (upstream `xmlXPathEqualValuesCommon`).
fn common_equal(v1: &XPathValue, v2: &XPathValue) -> bool {
    // Boolean involved → boolean conversion of both sides.
    if matches!(v1, XPathValue::Boolean(_)) || matches!(v2, XPathValue::Boolean(_)) {
        return v1.as_boolean() == v2.as_boolean();
    }
    // Number involved (and no boolean) → number conversion of both sides.
    // IEEE 754 equality gives exactly the upstream NaN/±Infinity rules.
    if matches!(v1, XPathValue::Number(_)) || matches!(v2, XPathValue::Number(_)) {
        return v1.as_number() == v2.as_number();
    }
    // Both strings (or undefined → converted to strings).
    v1.as_string() == v2.as_string()
}

/// Pop two objects and compare for equality (upstream xmlXPathEqualValues /
/// xmlXPathNotEqualValues semantics): pushes the boolean result object and
/// returns the comparison value.
///
/// # SAFETY
///
/// - `pc` must be a valid parser context.
pub unsafe fn equal_values_impl(pc: *mut XmlXPathParserContext, neq: bool) -> c_int {
    let arg2 = unsafe { value_pop(pc) };
    if arg2.is_null() {
        unsafe { (*pc).error = crate::abi::types::XPATH_INVALID_OPERAND as c_int };
        return 0;
    }
    let arg1 = unsafe { value_pop(pc) };
    if arg1.is_null() {
        unsafe {
            value_push(pc, arg2);
        }
        unsafe { (*pc).error = crate::abi::types::XPATH_INVALID_OPERAND as c_int };
        return 0;
    }
    // UPSTREAM-PARITY: comparing an object with itself yields 1 for
    // equality / 0 for inequality without pushing a result.
    if arg1 == arg2 {
        crate::abi::exports_xml2::xmlXPathFreeObject(arg1);
        return if neq { 0 } else { 1 };
    }
    let v1 = crate::abi::exports_xml2::object_to_xpathvalue_pub(arg1);
    let v2 = crate::abi::exports_xml2::object_to_xpathvalue_pub(arg2);
    crate::abi::exports_xml2::xmlXPathFreeObject(arg1);
    crate::abi::exports_xml2::xmlXPathFreeObject(arg2);

    let eq = equal_values_inner(&v1, &v2, neq);
    unsafe { value_push(pc, new_bool(eq)) };
    eq as c_int
}

/// Pop two objects and compare with `<`, `<=`, `>`, `>=` (upstream
/// `xmlXPathCompareValues` with the `inf` / `strict` encoding).
///
/// # SAFETY
///
/// - `pc` must be a valid parser context.
pub unsafe fn compare_values_impl(
    pc: *mut XmlXPathParserContext,
    inf: bool,
    strict: bool,
) -> c_int {
    let arg2 = unsafe { value_pop(pc) };
    if arg2.is_null() {
        unsafe { (*pc).error = crate::abi::types::XPATH_INVALID_OPERAND as c_int };
        return 0;
    }
    let arg1 = unsafe { value_pop(pc) };
    if arg1.is_null() {
        unsafe {
            value_push(pc, arg2);
        }
        unsafe { (*pc).error = crate::abi::types::XPATH_INVALID_OPERAND as c_int };
        return 0;
    }

    let v1 = crate::abi::exports_xml2::object_to_xpathvalue_pub(arg1);
    let v2 = crate::abi::exports_xml2::object_to_xpathvalue_pub(arg2);
    crate::abi::exports_xml2::xmlXPathFreeObject(arg1);
    crate::abi::exports_xml2::xmlXPathFreeObject(arg2);

    let (ns_side, other): (Option<&NodeSet>, Option<(&XPathValue, &XPathValue)>) = match (&v1, &v2)
    {
        (XPathValue::NodeSet(a), XPathValue::NodeSet(b)) => {
            return compare_node_sets(a, b, inf, strict);
        }
        (XPathValue::NodeSet(a), _) => (Some(a), None),
        (_, XPathValue::NodeSet(b)) => (Some(b), None),
        _ => (None, Some((&v1, &v2))),
    };

    if let Some(ns) = ns_side {
        // One side is a node-set, the other a scalar.
        let other = if matches!(&v1, XPathValue::NodeSet(_)) {
            &v2
        } else {
            &v1
        };
        // The node-set is on the left of the operator (upstream swaps
        // direction when the value is on the left).
        let (ns_is_left, scalar) = if matches!(&v1, XPathValue::NodeSet(_)) {
            (true, other)
        } else {
            (false, other)
        };
        return compare_node_set_value(ns, scalar, ns_is_left, inf, strict);
    }

    let (l, r) = other.unwrap();
    // Neither side is a node-set: convert both to numbers and compare.
    // NaN comparisons are always false (upstream).
    let a = l.as_number();
    let b = r.as_number();
    if a.is_nan() || b.is_nan() {
        return 0;
    }
    let ret = if inf && strict {
        a < b
    } else if inf && !strict {
        a <= b
    } else if !inf && strict {
        a > b
    } else {
        a >= b
    };
    ret as c_int
}

fn compare_node_sets(a: &NodeSet, b: &NodeSet, inf: bool, strict: bool) -> c_int {
    if a.is_empty() || b.is_empty() {
        return 0;
    }
    let b_nums: Vec<f64> = b.iter().map(|n| unsafe { node_to_number(n) }).collect();
    for na in a.iter() {
        let va = unsafe { node_to_number(na) };
        if va.is_nan() {
            continue;
        }
        for &vb in &b_nums {
            if vb.is_nan() {
                continue;
            }
            let ret = if inf && strict {
                va < vb
            } else if inf && !strict {
                va <= vb
            } else if !inf && strict {
                va > vb
            } else {
                va >= vb
            };
            if ret {
                return 1;
            }
        }
    }
    0
}

fn compare_node_set_value(
    ns: &NodeSet,
    scalar: &XPathValue,
    ns_is_left: bool,
    inf: bool,
    strict: bool,
) -> c_int {
    match scalar {
        XPathValue::Number(f) => compare_node_set_number(ns, *f, ns_is_left, inf, strict),
        XPathValue::String(s) => compare_node_set_string(ns, s, ns_is_left, inf, strict),
        XPathValue::Boolean(_) => {
            // Convert the node-set to a boolean and compare.
            let ns_bool = !ns.is_empty();
            let b = scalar.as_boolean();
            let (a, b) = if ns_is_left {
                (ns_bool, b)
            } else {
                (b, ns_bool)
            };
            let ret = if inf && strict {
                a < b
            } else if inf && !strict {
                a <= b
            } else if !inf && strict {
                a > b
            } else {
                a >= b
            };
            ret as c_int
        }
        _ => 0,
    }
}

fn compare_node_set_number(
    ns: &NodeSet,
    f: f64,
    ns_is_left: bool,
    inf: bool,
    strict: bool,
) -> c_int {
    for n in ns.iter() {
        let v = unsafe { node_to_number(n) };
        if v.is_nan() {
            continue;
        }
        let (a, b) = if ns_is_left { (v, f) } else { (f, v) };
        let ret = if inf && strict {
            a < b
        } else if inf && !strict {
            a <= b
        } else if !inf && strict {
            a > b
        } else {
            a >= b
        };
        if ret {
            return 1;
        }
    }
    0
}

fn compare_node_set_string(
    ns: &NodeSet,
    s: &str,
    ns_is_left: bool,
    inf: bool,
    strict: bool,
) -> c_int {
    for n in ns.iter() {
        let sv = node_string_value(n);
        let v = crate::xml::xpath::types::string_to_number(&sv);
        let w = crate::xml::xpath::types::string_to_number(s);
        if v.is_nan() || w.is_nan() {
            continue;
        }
        let (a, b) = if ns_is_left { (v, w) } else { (w, v) };
        let ret = if inf && strict {
            a < b
        } else if inf && !strict {
            a <= b
        } else if !inf && strict {
            a > b
        } else {
            a >= b
        };
        if ret {
            return 1;
        }
    }
    0
}

/// String-value of a node converted to a number (upstream
/// `xmlXPathNodeToNumber`).
unsafe fn node_to_number(node: *mut _xmlNode) -> f64 {
    if node.is_null() {
        return f64::NAN;
    }
    let sv = node_string_value(node);
    crate::xml::xpath::types::string_to_number(&sv)
}

/// Convert an internal value to a C object.
///
/// # SAFETY
///
/// - The returned object is heap-allocated.
pub unsafe fn value_to_object(v: XPathValue) -> *mut _xmlXPathObject {
    crate::abi::exports_xml2::xpath_to_object_pub(v)
}

#[allow(unused)]
fn _unused(_: *mut _xmlDoc) {}

#[allow(unused)]
fn _unused_char(_: *const c_char) {}
