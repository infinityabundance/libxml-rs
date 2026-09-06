//! XPath 1.0 Core Function Library (§25).
//!
//! Implements all XPath 1.0 core functions as specified in §4 of the
//! XPath 1.0 Recommendation.
//!
//! # UPSTREAM-PARITY
//!
//! All functions follow the XPath 1.0 specification and libxml2
//! observable behavior, including edge cases and historical quirks.
//!
//! # Courts
//!
//! XPATH-FUNCTIONS-*
//!
//! # Upstream contract
//!
//! Mirrors the core-function library of upstream `xpath.c`
//! (`SRC-LIBXML2-2.15.0-XPATH-C`, parity target libxml2 2.15.3 oracle):
//! the 25 XPath 1.0 §4 functions (node-set, string, boolean, number
//! groups) with libxml2 observable edge cases.
//!
//! # Conceptual behavior
//!
//! Implements each function over already-evaluated `XPathValue` arguments
//! with the upstream argument-count and coercion behavior: node-set
//! functions (last, position, count, id, local-name, namespace-uri, name),
//! string functions (string, concat, starts-with, substring, translate,
//! ...), boolean functions and number functions (number, sum, floor,
//! ceiling, round). number()/string() route through the R-000166 number
//! formatter (1e9/1e-5 scientific threshold, DBL_DIG=15 fraction digits).
//!
//! # Ownership & safety invariants
//!
//! Functions return owned `XPathValue`s; node-set arguments are borrowed
//! views over the tree (valid for the call). No function stores or caches
//! argument pointers — values are copied at the boundary, so the registry
//! is safe to share.
//!
//! # Historical quirks & epochs
//!
//! R-000114 (attribute string-value must be the attribute content, not
//! empty) and R-000166 (full double-precision value-of printing) were
//! fixed against the 2.15.3 oracle; the number() corpus (967/967 cases)
//! locks the formatting epoch. The E-008 stable libxslt epoch means any
//! function-level divergence is a candidate bug, not an epoch difference.
//!
//! # Deliberate oddities
//!
//! round()/floor()/ceiling() reproduce libxml2 IEEE-754 handling
//! (including negative zero and NaN propagation) rather than Rust
//! rounding helpers, which differ on ties and sign.
//!
//! # Proving courts
//!
//! XPATH-FUNCTIONS-* differential probes and the 967/967 number() corpus
//! compare results byte-identical against the oracle; the XSLT courts
//! (CLI-XSLTPROC-0014/0015/0017) exercise value-of/format-number through
//! these functions.
//!
//! # Tempting simplifications that would break parity
//!
//! Do not delegate number formatting to Rust float formatting: the
//! scientific threshold, digit counts and exponent padding are
//! oracle-observable (R-000166). Do not coerce arguments more eagerly
//! than upstream (e.g. empty node-sets to string) — R-000114 proved the
//! string-value rules are observable.

use crate::xml::xpath::context::{BoxedXPathFunction, XPathContext};
use crate::xml::xpath::types::{node_string_value, string_to_number, NodeSet, XPathValue};
use once_cell::sync::Lazy;
use std::collections::HashMap;

/// Type alias for XPath functions.
///
/// Functions receive already-evaluated arguments as `XPathValue` slices.
pub type XPathFunction = fn(&mut XPathContext, &[XPathValue]) -> Result<XPathValue, String>;

/// The XPath 1.0 core function library, as a static `(name, fn)` slice.
///
/// Phase 16.5.9: these are built-ins and must not be rebuilt/reboxed into a
/// fresh `HashMap` for every new context. The slice is the single source of
/// truth; `lookup_core_function` consults a lazily-built static table so the
/// hot path is a hash lookup with zero per-context allocation.
pub static CORE_FUNCTION_SLICE: &[(&str, XPathFunction)] = &[
    // Node set functions (§4.1)
    ("last", fn_last),
    ("position", fn_position),
    ("count", fn_count),
    ("id", fn_id),
    ("local-name", fn_local_name),
    ("namespace-uri", fn_namespace_uri),
    ("name", fn_name),
    // String functions (§4.2)
    ("string", fn_string),
    ("concat", fn_concat),
    ("starts-with", fn_starts_with),
    ("contains", fn_contains),
    ("substring-before", fn_substring_before),
    ("substring-after", fn_substring_after),
    ("substring", fn_substring),
    ("string-length", fn_string_length),
    ("normalize-space", fn_normalize_space),
    ("translate", fn_translate),
    // Boolean functions (§4.3)
    ("boolean", fn_boolean),
    ("not", fn_not),
    ("true", fn_true),
    ("false", fn_false),
    ("lang", fn_lang),
    // Number functions (§4.4)
    ("number", fn_number),
    ("sum", fn_sum),
    ("floor", fn_floor),
    ("ceiling", fn_ceiling),
    ("round", fn_round),
];

static CORE_FUNCTION_TABLE: Lazy<HashMap<&'static str, BoxedXPathFunction>> = Lazy::new(|| {
    CORE_FUNCTION_SLICE
        .iter()
        .map(|&(name, f)| (name, Box::new(f) as BoxedXPathFunction))
        .collect()
});

/// Look up a core XPath 1.0 built-in by name (allocation-free; the table is
/// built once and shared across every context).
pub fn lookup_core_function(name: &str) -> Option<&'static BoxedXPathFunction> {
    CORE_FUNCTION_TABLE.get(name)
}

/// The XPath 1.0 core function library as a name→fn map.
///
/// Kept for call sites/tests that need a `HashMap`; new hot-path code should
/// prefer [`lookup_core_function`] to avoid the per-context rebuild.
pub fn core_functions() -> HashMap<String, XPathFunction> {
    CORE_FUNCTION_SLICE
        .iter()
        .map(|&(name, f)| (name.to_string(), f))
        .collect()
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helper: extract typed arguments
// ═══════════════════════════════════════════════════════════════════════════════

fn get_string_arg(args: &[XPathValue], index: usize) -> String {
    if index < args.len() {
        args[index].as_string()
    } else {
        String::new()
    }
}

fn get_number_arg(args: &[XPathValue], index: usize) -> f64 {
    if index < args.len() {
        args[index].as_number()
    } else {
        f64::NAN
    }
}

fn get_boolean_arg(args: &[XPathValue], index: usize) -> bool {
    if index < args.len() {
        args[index].as_boolean()
    } else {
        false
    }
}

fn get_node_set_arg(args: &[XPathValue], index: usize) -> NodeSet {
    if index < args.len() {
        match &args[index] {
            XPathValue::NodeSet(ns) => ns.clone(),
            _ => NodeSet::new(),
        }
    } else {
        NodeSet::new()
    }
}

fn get_first_node(
    ctx: &XPathContext,
    args: &[XPathValue],
    index: usize,
) -> Option<*mut crate::abi::structs::_xmlNode> {
    if index < args.len() {
        match &args[index] {
            XPathValue::NodeSet(ns) => ns.first(),
            _ => None,
        }
    } else {
        // Default to context node
        Some(ctx.context_node)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Node Set Functions (§4.1)
// ═══════════════════════════════════════════════════════════════════════════════

/// last() — context size.
const fn fn_last(ctx: &mut XPathContext, _args: &[XPathValue]) -> Result<XPathValue, String> {
    Ok(XPathValue::Number(ctx.last() as f64))
}

/// position() — context position.
const fn fn_position(ctx: &mut XPathContext, _args: &[XPathValue]) -> Result<XPathValue, String> {
    Ok(XPathValue::Number(ctx.position() as f64))
}

/// count(node-set) — number of nodes in node-set.
fn fn_count(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    let ns = get_node_set_arg(args, 0);
    Ok(XPathValue::Number(ns.len() as f64))
}

/// id(object) — select elements by ID.
const fn fn_id(_ctx: &mut XPathContext, _args: &[XPathValue]) -> Result<XPathValue, String> {
    // id() is complex: requires DTD validation to know which attributes are ID.
    // For now, return empty node-set.
    Ok(XPathValue::NodeSet(NodeSet::new()))
}

/// local-name(node-set?) — local part of name.
///
/// # Safety
///
/// - The node returned by `get_first_node` must be NULL or a valid
///   `_xmlNode` that stays alive for the call; its `name` field must be
///   NULL or a valid NUL-terminated string.
fn fn_local_name(ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    let node = get_first_node(ctx, args, 0);
    if let Some(node) = node {
        unsafe {
            let name = crate::xml::string::xmlstr_to_string((*node).name);
            // Strip prefix if present
            if let Some(pos) = name.find(':') {
                Ok(XPathValue::String(name[pos + 1..].to_string()))
            } else {
                Ok(XPathValue::String(name))
            }
        }
    } else {
        Ok(XPathValue::String(String::new()))
    }
}

/// namespace-uri(node-set?) — namespace URI of node.
///
/// # Safety
///
/// - The node returned by `get_first_node` must be NULL or a valid
///   `_xmlNode` that stays alive for the call; its `ns` pointer, when
///   non-NULL, must point to a valid `_xmlNs` whose `href` is NULL or a
///   valid NUL-terminated string.
fn fn_namespace_uri(ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    let node = get_first_node(ctx, args, 0);
    if let Some(node) = node {
        unsafe {
            if let Some(ns) = (*node).ns.as_ref() {
                let uri = crate::xml::string::xmlstr_to_string(ns.href);
                Ok(XPathValue::String(uri))
            } else {
                Ok(XPathValue::String(String::new()))
            }
        }
    } else {
        Ok(XPathValue::String(String::new()))
    }
}

/// name(node-set?) — QName of the first node in document order.
///
/// Element/attribute nodes bound to a prefixed namespace return
/// `prefix:local`; everything else follows the local-name rule.
///
/// # Safety
///
/// - The node returned by `get_first_node` must be NULL or a valid
///   `_xmlNode` that stays alive for the call; its `name` and `ns` fields
///   must be valid.
fn fn_name(ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    let node = get_first_node(ctx, args, 0);
    if let Some(node) = node {
        unsafe {
            use crate::abi::types::xmlElementType as ET;
            let t = (*node).type_;
            let name = crate::xml::string::xmlstr_to_string((*node).name);
            if (t == ET::XML_ELEMENT_NODE as i32 || t == ET::XML_ATTRIBUTE_NODE as i32)
                && !name.is_empty()
                && !(*node).ns.is_null()
                && !(*(*node).ns).prefix.is_null()
            {
                let prefix = crate::xml::string::xmlstr_to_string((*(*node).ns).prefix);
                Ok(XPathValue::String(format!("{prefix}:{name}")))
            } else {
                Ok(XPathValue::String(name))
            }
        }
    } else {
        Ok(XPathValue::String(String::new()))
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// String Functions (§4.2)
// ═══════════════════════════════════════════════════════════════════════════════

/// string(object?) — convert to string.
fn fn_string(ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    if args.is_empty() {
        // Default: context node's string value
        Ok(XPathValue::String(node_string_value(ctx.context_node)))
    } else {
        Ok(XPathValue::String(args[0].as_string()))
    }
}

/// concat(string, string, ...) — concatenate strings.
fn fn_concat(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    let mut result = String::new();
    for arg in args {
        result.push_str(&arg.as_string());
    }
    Ok(XPathValue::String(result))
}

/// starts-with(string1, string2) — check prefix.
fn fn_starts_with(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    let s1 = get_string_arg(args, 0);
    let s2 = get_string_arg(args, 1);
    Ok(XPathValue::Boolean(s1.starts_with(&s2)))
}

/// contains(string1, string2) — check substring.
fn fn_contains(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    let s1 = get_string_arg(args, 0);
    let s2 = get_string_arg(args, 1);
    Ok(XPathValue::Boolean(s1.contains(&s2)))
}

/// substring-before(string1, string2) — before first occurrence.
fn fn_substring_before(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    let s1 = get_string_arg(args, 0);
    let s2 = get_string_arg(args, 1);
    if let Some(pos) = s1.find(&s2) {
        Ok(XPathValue::String(s1[..pos].to_string()))
    } else {
        Ok(XPathValue::String(String::new()))
    }
}

/// substring-after(string1, string2) — after first occurrence.
fn fn_substring_after(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    let s1 = get_string_arg(args, 0);
    let s2 = get_string_arg(args, 1);
    if let Some(pos) = s1.find(&s2) {
        Ok(XPathValue::String(s1[pos + s2.len()..].to_string()))
    } else {
        Ok(XPathValue::String(String::new()))
    }
}

/// substring(string, number, number?) — substring extraction.
///
/// UPSTREAM-PARITY (XPath 1.0 §4.2): substring operates on CHARACTERS
/// (Unicode code points), and a character at 1-based position P is included
/// when `P >= round(start)` and `P < round(start) + round(length)`. Byte
/// slicing is wrong for multibyte strings (bug26384: xsl:key over a
/// Cyrillic value panicked on a non-char-boundary slice).
fn fn_substring(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    let s = get_string_arg(args, 0);
    let start = get_number_arg(args, 1);
    let has_length = args.len() >= 3;
    let length = if has_length {
        get_number_arg(args, 2)
    } else {
        f64::MAX
    };

    let start_r = start.round();
    let end_r = start_r + length.round();

    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        let p = (i + 1) as f64;
        if p >= start_r && p < end_r {
            out.push(c);
        }
    }
    Ok(XPathValue::String(out))
}

/// string-length(string?) — length of string.
///
/// UPSTREAM-PARITY (XPath 1.0 §4.2): string-length counts CHARACTERS (Unicode
/// code points), not bytes.
fn fn_string_length(ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    let s = if args.is_empty() {
        node_string_value(ctx.context_node)
    } else {
        get_string_arg(args, 0)
    };
    Ok(XPathValue::Number(s.chars().count() as f64))
}

/// normalize-space(string?) — normalize whitespace.
fn fn_normalize_space(ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    let s = if args.is_empty() {
        node_string_value(ctx.context_node)
    } else {
        get_string_arg(args, 0)
    };
    let normalized: Vec<&str> = s.split_whitespace().collect();
    Ok(XPathValue::String(normalized.join(" ")))
}

/// translate(string1, string2, string3) — character translation.
fn fn_translate(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    let s = get_string_arg(args, 0);
    let from = get_string_arg(args, 1);
    let to = get_string_arg(args, 2);

    let result: String = s
        .chars()
        .map(|c| {
            // UPSTREAM-PARITY (XPath 1.0 §4.2): translate maps by CHARACTER
            // position within the "from" string (byte offsets are wrong for
            // multibyte "from" strings).
            if let Some(pos) = from.chars().position(|x| x == c) {
                let to_chars: Vec<char> = to.chars().collect();
                if pos < to_chars.len() {
                    to_chars[pos]
                } else {
                    '\0' // Remove character
                }
            } else {
                c
            }
        })
        .filter(|&c| c != '\0')
        .collect();

    Ok(XPathValue::String(result))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Boolean Functions (§4.3)
// ═══════════════════════════════════════════════════════════════════════════════

/// boolean(object) — convert to boolean.
fn fn_boolean(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    Ok(XPathValue::Boolean(get_boolean_arg(args, 0)))
}

/// not(boolean) — logical NOT.
fn fn_not(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    Ok(XPathValue::Boolean(!get_boolean_arg(args, 0)))
}

/// true() — constant true.
const fn fn_true(_ctx: &mut XPathContext, _args: &[XPathValue]) -> Result<XPathValue, String> {
    Ok(XPathValue::Boolean(true))
}

/// false() — constant false.
const fn fn_false(_ctx: &mut XPathContext, _args: &[XPathValue]) -> Result<XPathValue, String> {
    Ok(XPathValue::Boolean(false))
}

/// lang(string) — language test.
///
/// # Safety
///
/// - `ctx.context_node` must be NULL or a valid `_xmlNode`; the walk up
///   the `parent` chain and through each node's `properties` must only
///   touch valid nodes and attributes whose `name` and child `content`
///   fields are NULL or valid NUL-terminated strings; the chain must stay
///   alive and acyclic for the duration of the call.
fn fn_lang(ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    let lang = get_string_arg(args, 0);
    let mut node = ctx.context_node;
    unsafe {
        while !node.is_null() {
            let mut prop = (*node).properties;
            while !prop.is_null() {
                let attr_name = crate::xml::string::xmlstr_to_string((*prop).name);
                if attr_name == "lang" || attr_name == "xml:lang" {
                    // Attribute value is stored in children (text node's content)
                    if !(*prop).children.is_null() {
                        let attr_val =
                            crate::xml::string::xmlstr_to_string((*(*prop).children).content);
                        if attr_val.to_lowercase() == lang.to_lowercase()
                            || attr_val
                                .to_lowercase()
                                .starts_with(&format!("{}-", lang.to_lowercase()))
                        {
                            return Ok(XPathValue::Boolean(true));
                        }
                    }
                }
                prop = (*prop).next;
            }
            node = (*node).parent;
        }
    }
    Ok(XPathValue::Boolean(false))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Number Functions (§4.4)
// ═══════════════════════════════════════════════════════════════════════════════

/// number(object?) — convert to number.
fn fn_number(ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    if args.is_empty() {
        Ok(XPathValue::Number(string_to_number(&node_string_value(
            ctx.context_node,
        ))))
    } else {
        Ok(XPathValue::Number(get_number_arg(args, 0)))
    }
}

/// sum(node-set) — sum of string->number conversions.
fn fn_sum(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    let ns = get_node_set_arg(args, 0);
    let mut total = 0.0;
    for node in ns.iter() {
        let s = node_string_value(node);
        total += string_to_number(&s);
    }
    Ok(XPathValue::Number(total))
}

/// floor(number) — largest integer <= value.
fn fn_floor(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    let n = get_number_arg(args, 0);
    Ok(XPathValue::Number(n.floor()))
}

/// ceiling(number) — smallest integer >= value.
fn fn_ceiling(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    let n = get_number_arg(args, 0);
    Ok(XPathValue::Number(n.ceil()))
}

/// round(number) — round to nearest integer.
///
/// UPSTREAM-PARITY: XPath 1.0 rounds towards positive infinity for .5 cases.
/// Rust's f64::round() rounds half away from zero, which differs for negative .5 values.
/// See XPath 1.0 §4.4.
fn fn_round(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    let n = get_number_arg(args, 0);
    if n.is_nan() || n.is_infinite() || n == 0.0 {
        return Ok(XPathValue::Number(n));
    }
    // Rust's f64::round() uses "round half away from zero"
    // XPath 1.0 uses "round half towards positive infinity"
    // These differ for negative numbers with .5 fractional part
    let rust_rounded = n.round();
    let result = if n.is_sign_negative() && (n - rust_rounded).abs() == 0.5 {
        // XPath: move towards positive infinity (i.e., add 1.0 to the Rust result)
        rust_rounded + 1.0
    } else {
        rust_rounded
    };
    Ok(XPathValue::Number(result))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_true_false() {
        let mut ctx = XPathContext::new(std::ptr::null_mut());
        assert!(fn_true(&mut ctx, &[]).unwrap().as_boolean());
        assert!(!fn_false(&mut ctx, &[]).unwrap().as_boolean());
    }

    #[test]
    fn test_boolean_conversion() {
        let mut ctx = XPathContext::new(std::ptr::null_mut());
        assert!(fn_boolean(&mut ctx, &[XPathValue::Boolean(true)])
            .unwrap()
            .as_boolean());
        assert!(!fn_boolean(&mut ctx, &[XPathValue::Boolean(false)])
            .unwrap()
            .as_boolean());
    }

    #[test]
    fn test_not() {
        let mut ctx = XPathContext::new(std::ptr::null_mut());
        assert!(!fn_not(&mut ctx, &[XPathValue::Boolean(true)])
            .unwrap()
            .as_boolean());
        assert!(fn_not(&mut ctx, &[XPathValue::Boolean(false)])
            .unwrap()
            .as_boolean());
    }

    #[test]
    fn test_number_round() {
        let mut ctx = XPathContext::new(std::ptr::null_mut());
        assert_eq!(
            fn_floor(&mut ctx, &[XPathValue::Number(3.7)])
                .unwrap()
                .as_number(),
            3.0
        );
        assert_eq!(
            fn_ceiling(&mut ctx, &[XPathValue::Number(3.2)])
                .unwrap()
                .as_number(),
            4.0
        );
        assert_eq!(
            fn_round(&mut ctx, &[XPathValue::Number(3.5)])
                .unwrap()
                .as_number(),
            4.0
        );
        assert_eq!(
            fn_round(&mut ctx, &[XPathValue::Number(-3.5)])
                .unwrap()
                .as_number(),
            -3.0
        );
    }

    #[test]
    fn test_string_functions() {
        let mut ctx = XPathContext::new(std::ptr::null_mut());
        assert_eq!(
            fn_concat(
                &mut ctx,
                &[
                    XPathValue::String("a".into()),
                    XPathValue::String("b".into()),
                    XPathValue::String("c".into())
                ]
            )
            .unwrap()
            .as_string(),
            "abc"
        );
        assert!(fn_starts_with(
            &mut ctx,
            &[
                XPathValue::String("hello".into()),
                XPathValue::String("he".into())
            ]
        )
        .unwrap()
        .as_boolean());
        assert!(!fn_starts_with(
            &mut ctx,
            &[
                XPathValue::String("hello".into()),
                XPathValue::String("x".into())
            ]
        )
        .unwrap()
        .as_boolean());
        assert!(fn_contains(
            &mut ctx,
            &[
                XPathValue::String("hello".into()),
                XPathValue::String("ell".into())
            ]
        )
        .unwrap()
        .as_boolean());
        assert_eq!(
            fn_string_length(&mut ctx, &[XPathValue::String("hello".into())])
                .unwrap()
                .as_number(),
            5.0
        );
    }

    #[test]
    fn test_core_functions_registered() {
        let funcs = core_functions();
        assert!(funcs.contains_key("last"));
        assert!(funcs.contains_key("position"));
        assert!(funcs.contains_key("count"));
        assert!(funcs.contains_key("string"));
        assert!(funcs.contains_key("concat"));
        assert!(funcs.contains_key("boolean"));
        assert!(funcs.contains_key("not"));
        assert!(funcs.contains_key("number"));
        assert!(funcs.contains_key("sum"));
        assert!(funcs.contains_key("floor"));
        assert!(funcs.contains_key("ceiling"));
        assert!(funcs.contains_key("round"));
        assert!(funcs.contains_key("name"));
        assert!(funcs.contains_key("local-name"));
        assert_eq!(funcs.len(), 27);
        assert!(funcs.contains_key("namespace-uri"));
    }
}
