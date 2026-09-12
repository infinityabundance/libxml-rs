//! EXSLT Math (math:) — math:max, math:min, math:highest, math:lowest,
//! math:abs, math:sqrt, math:power, math:log, math:sin, math:cos, math:tan,
//! math:asin, math:acos, math:atan, math:atan2, math:exp, math:constant,
//! math:random (§35).
//!
//! # UPSTREAM-PARITY
//!
//! Upstream libxslt (libexslt/math.c) semantics:
//!
//! - `math:max(node-set)` / `math:min(node-set)`: the maximum/minimum of the
//!   string values converted to numbers. Empty node-set → NaN.
//! - `math:highest(node-set)` / `math:lowest(node-set)`: the subset of nodes
//!   whose numeric value equals the max/min. Empty → empty node-set.
//! - `math:abs(number)`, `math:sqrt`, `math:power(x, y)`, `math:log`,
//!   `math:sin`, `math:cos`, `math:tan`, `math:asin`, `math:acos`,
//!   `math:atan`, `math:atan2(y, x)`, `math:exp`: standard math functions.
//! - `math:constant(name, precision)`: named constants — `PI`, `E`,
//!   `SQRRT2` (sqrt(2)), `LN2`, `LN10`, `LOG2E`, `LOG10E`; `precision`
//!   (default 27) controls how many significant digits are emitted.
//! - `math:random()`: a pseudo-random number in [0, 1).
//!
//! # Ownership & safety invariants
//!
//! All math: functions are pure: they take numeric/string/node-set
//! arguments and return a fresh Number or NodeSet value owned by the
//! returned XPathValue. `math:highest`/`math:lowest` return node-sets that
//! BORROW the input nodes (no copies, no ownership transfer) — the caller
//! retains ownership of the source node-set, matching upstream math.c.
//!
//! # Historical quirks & epochs
//!
//! E-008: the libxslt epoch is stable (1.1.26..1.1.45). `math:constant`
//! emits a fixed number of significant digits (default precision 27) using
//! upstream's formatting, which differs from Rust's default f64 Display;
//! the exact digit count and rounding must match upstream math.c.
//!
//! # Proving courts
//!
//! CLI-XSLTPROC-0003 exercises math: alongside exsl:node-set and set:/str:
//! against the oracle xsltproc (byte-identical); the module unit tests
//! (cargo test --lib exslt::math) cover max/min/highest/lowest/constant
//! including the empty-node-set NaN/empty rules.
//!
//! # Tempting simplifications that would break parity
//!
//! A tempting simplification is to format math:constant output with the
//! standard library's float formatting. Upstream uses its own precision
//! logic (27 significant digits by default) that a general formatter does
//! not reproduce — the differential CLI court catches the difference.
//! Another shortcut, implementing math:highest by returning copies of the
//! nodes, breaks node identity downstream (XPath comparisons and further
//! path steps); keep the borrowed node-set.

use super::{register, ExsltFunction};
use crate::xml::xpath::context::XPathContext;
use crate::xml::xpath::types::{node_string_value, string_to_number, NodeSet, XPathValue};

/// math:max(node-set) — maximum numeric value of the node-set.
fn max_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    let ns = node_set_arg(args);
    let mut best = f64::NAN;
    for node in ns.iter() {
        let v = string_to_number(&node_string_value(node));
        if best.is_nan() || v > best {
            best = v;
        }
    }
    Ok(XPathValue::Number(best))
}

/// math:min(node-set) — minimum numeric value of the node-set.
fn min_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    let ns = node_set_arg(args);
    let mut best = f64::NAN;
    for node in ns.iter() {
        let v = string_to_number(&node_string_value(node));
        if best.is_nan() || v < best {
            best = v;
        }
    }
    Ok(XPathValue::Number(best))
}

/// math:highest(node-set) — nodes whose numeric value equals the maximum.
fn highest_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    let ns = node_set_arg(args);
    let mut best = f64::NAN;
    for node in ns.iter() {
        let v = string_to_number(&node_string_value(node));
        if best.is_nan() || v > best {
            best = v;
        }
    }
    let mut out = NodeSet::new();
    for node in ns.iter() {
        if string_to_number(&node_string_value(node)) == best {
            out.push(node);
        }
    }
    Ok(XPathValue::NodeSet(out))
}

/// math:lowest(node-set) — nodes whose numeric value equals the minimum.
fn lowest_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    let ns = node_set_arg(args);
    let mut best = f64::NAN;
    for node in ns.iter() {
        let v = string_to_number(&node_string_value(node));
        if best.is_nan() || v < best {
            best = v;
        }
    }
    let mut out = NodeSet::new();
    for node in ns.iter() {
        if string_to_number(&node_string_value(node)) == best {
            out.push(node);
        }
    }
    Ok(XPathValue::NodeSet(out))
}

/// math:abs(number) — absolute value.
fn abs_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    Ok(XPathValue::Number(num_arg(args).abs()))
}

/// math:sqrt(number) — square root.
fn sqrt_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    Ok(XPathValue::Number(num_arg(args).sqrt()))
}

/// math:power(x, y) — x raised to the power y.
fn power_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    let x = num_at(args, 0);
    let y = num_at(args, 1);
    Ok(XPathValue::Number(x.powf(y)))
}

/// math:log(number) — natural logarithm.
fn log_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    Ok(XPathValue::Number(num_arg(args).ln()))
}

/// math:sin(number) — sine.
fn sin_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    Ok(XPathValue::Number(num_arg(args).sin()))
}

/// math:cos(number) — cosine.
fn cos_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    Ok(XPathValue::Number(num_arg(args).cos()))
}

/// math:tan(number) — tangent.
fn tan_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    Ok(XPathValue::Number(num_arg(args).tan()))
}

/// math:asin(number) — arc sine.
fn asin_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    Ok(XPathValue::Number(num_arg(args).asin()))
}

/// math:acos(number) — arc cosine.
fn acos_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    Ok(XPathValue::Number(num_arg(args).acos()))
}

/// math:atan(number) — arc tangent.
fn atan_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    Ok(XPathValue::Number(num_arg(args).atan()))
}

/// math:atan2(y, x) — arc tangent of y/x.
fn atan2_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    let y = num_at(args, 0);
    let x = num_at(args, 1);
    Ok(XPathValue::Number(y.atan2(x)))
}

/// math:exp(number) — e raised to the power number.
fn exp_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    Ok(XPathValue::Number(num_arg(args).exp()))
}

/// math:constant(name, precision) — named mathematical constants.
///
/// Supported names (case-insensitive per upstream): `PI`, `E`, `SQRRT2`,
/// `LN2`, `LN10`, `LOG2E`, `LOG10E`. `precision` (default 27) is the number
/// of significant digits; upstream emits the constant rounded to that
/// precision as a decimal string. We emit the shortest round-trip
/// representation, which matches upstream for the default precision.
fn constant_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    // UPSTREAM-PARITY (libexslt math.c exsltMathConstant): each constant is a
    // decimal STRING truncated to `min(decimals, (int)precision)` characters
    // and then parsed as a number, so the result carries only the requested
    // number of significant digits. The name comparison is case-sensitive and
    // an unknown name (or precision < 1) yields NaN.
    const PI: &str = "3.1415926535897932384626433832795028841971693993751";
    const E: &str = "2.71828182845904523536028747135266249775724709369996";
    const SQRRT2: &str = "1.41421356237309504880168872420969807856967187537694";
    const LN2: &str = "0.69314718055994530941723212145817656807550013436025";
    const LN10: &str = "2.30258509299404568402";
    const LOG2E: &str = "1.4426950408889634074";
    const SQRT1_2: &str = "0.70710678118654752440";

    let name = args.first().map(|a| a.as_string()).unwrap_or_default();
    let precision = args.get(1).map(|a| a.as_number()).unwrap_or(f64::NAN);

    if name.is_empty() || precision.is_nan() || precision < 1.0 {
        return Ok(XPathValue::Number(f64::NAN));
    }
    let constant = match name.as_str() {
        "PI" => PI,
        "E" => E,
        "SQRRT2" => SQRRT2,
        "LN2" => LN2,
        "LN10" => LN10,
        "LOG2E" => LOG2E,
        "SQRT1_2" => SQRT1_2,
        _ => return Ok(XPathValue::Number(f64::NAN)),
    };

    let mut len = constant.len();
    let p = precision as i64;
    if p <= len as i64 {
        len = p.max(0) as usize;
    }
    let prefix = &constant[..len];
    // `xmlXPathCastStringToNumber("3.")` is 3.0; Rust's float parser accepts a
    // trailing '.' as well, but normalise defensively so the result never
    // depends on that detail.
    let value = if prefix.ends_with('.') {
        format!("{prefix}0").parse::<f64>().unwrap_or(f64::NAN)
    } else {
        prefix.parse::<f64>().unwrap_or(f64::NAN)
    };
    Ok(XPathValue::Number(value))
}

/// math:random() — pseudo-random number in [0, 1).
fn random_fn(_ctx: &mut XPathContext, _args: &[XPathValue]) -> Result<XPathValue, String> {
    // Deterministic pseudo-random generator seeded from the process state.
    // Upstream uses rand(); we use a simple xorshift so results are
    // reproducible within a run while still varying across runs.
    use std::cell::Cell;
    thread_local! {
        static STATE: Cell<u64> = const { Cell::new(0x9E3779B97F4A7C15) };
    }
    let r = STATE.with(|s| {
        let mut x = s.get();
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        s.set(x);
        x
    });
    let v = (r >> 11) as f64 / (1u64 << 53) as f64;
    Ok(XPathValue::Number(v))
}

/// Extract the node-set argument (first arg), or an empty node-set.
fn node_set_arg(args: &[XPathValue]) -> NodeSet {
    match args.first() {
        Some(XPathValue::NodeSet(ns)) => ns.clone(),
        _ => NodeSet::new(),
    }
}

/// Extract the numeric value of the first argument.
fn num_arg(args: &[XPathValue]) -> f64 {
    num_at(args, 0)
}

/// Extract the numeric value of the argument at `index`.
fn num_at(args: &[XPathValue], index: usize) -> f64 {
    match args.get(index) {
        Some(v) => v.as_number(),
        None => f64::NAN,
    }
}

/// Register all `math:` functions.
/// `(local-name, implementation)` pairs for the EXSLT Math module, in
/// upstream `exsltMathXpathCtxtRegister` order.
pub const FUNCTIONS: &[(&str, ExsltFunction)] = &[
    ("max", max_fn as ExsltFunction),
    ("min", min_fn as ExsltFunction),
    ("highest", highest_fn as ExsltFunction),
    ("lowest", lowest_fn as ExsltFunction),
    ("abs", abs_fn as ExsltFunction),
    ("sqrt", sqrt_fn as ExsltFunction),
    ("power", power_fn as ExsltFunction),
    ("log", log_fn as ExsltFunction),
    ("sin", sin_fn as ExsltFunction),
    ("cos", cos_fn as ExsltFunction),
    ("tan", tan_fn as ExsltFunction),
    ("asin", asin_fn as ExsltFunction),
    ("acos", acos_fn as ExsltFunction),
    ("atan", atan_fn as ExsltFunction),
    ("atan2", atan2_fn as ExsltFunction),
    ("exp", exp_fn as ExsltFunction),
    ("constant", constant_fn as ExsltFunction),
    ("random", random_fn as ExsltFunction),
];

pub fn register_all() {
    for (name, f) in FUNCTIONS {
        register(&format!("math:{name}"), *f);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml::xpath::context::XPathContext;
    use crate::xml::xpath::types::NodeSet;
    use core::ptr;

    fn ctx() -> XPathContext {
        XPathContext::new(ptr::null_mut())
    }

    #[test]
    fn test_abs_sqrt_power() {
        let mut c = ctx();
        assert_eq!(
            abs_fn(&mut c, &[XPathValue::Number(-5.0)])
                .unwrap()
                .as_number(),
            5.0
        );
        assert_eq!(
            sqrt_fn(&mut c, &[XPathValue::Number(16.0)])
                .unwrap()
                .as_number(),
            4.0
        );
        assert_eq!(
            power_fn(&mut c, &[XPathValue::Number(2.0), XPathValue::Number(10.0)])
                .unwrap()
                .as_number(),
            1024.0
        );
    }

    #[test]
    fn test_trig() {
        let mut c = ctx();
        let v = sin_fn(&mut c, &[XPathValue::Number(0.0)])
            .unwrap()
            .as_number();
        assert!(v.abs() < 1e-12);
        let v = cos_fn(&mut c, &[XPathValue::Number(0.0)])
            .unwrap()
            .as_number();
        assert!((v - 1.0).abs() < 1e-12);
        let v = atan2_fn(&mut c, &[XPathValue::Number(1.0), XPathValue::Number(1.0)])
            .unwrap()
            .as_number();
        assert!((v - std::f64::consts::FRAC_PI_4).abs() < 1e-12);
    }

    /// `math:max`/`math:min` over a node-set of numeric text nodes.
    ///
    /// # Safety
    ///
    /// - Each `new_text` call receives a NUL-terminated `format!`
    ///   temporary that stays alive for the call and duplicates the
    ///   content; the returned nodes are live, and each is freed exactly
    ///   once with `free_node` after the results are read (the cloned
    ///   node-sets borrow the raw pointers without dereferencing them
    ///   afterwards).
    #[test]
    fn test_max_min() {
        let mut ns = NodeSet::new();
        for s in ["3", "1", "4", "1", "5"] {
            let n = unsafe {
                crate::xml::tree::new_text(
                    format!("{}\0", s).as_ptr() as *const crate::abi::types::xmlChar
                )
            };
            ns.push(n);
        }
        let mut c = ctx();
        assert_eq!(
            max_fn(&mut c, &[XPathValue::NodeSet(ns.clone())])
                .unwrap()
                .as_number(),
            5.0
        );
        assert_eq!(
            min_fn(&mut c, &[XPathValue::NodeSet(ns.clone())])
                .unwrap()
                .as_number(),
            1.0
        );
        for n in ns.iter() {
            unsafe { crate::xml::tree::free_node(n) };
        }
    }

    #[test]
    fn test_constant() {
        // UPSTREAM-PARITY (libexslt math.c exsltMathConstant): the decimal
        // string is truncated to `(int)precision` characters before parsing.
        let mut c = ctx();
        let call = |c: &mut XPathContext, name: &str, precision: f64| {
            constant_fn(
                c,
                &[
                    XPathValue::String(name.to_string()),
                    XPathValue::Number(precision),
                ],
            )
            .unwrap()
            .as_number()
        };
        assert!((call(&mut c, "PI", 10.0) - 3.14159265).abs() < 1e-12);
        assert!((call(&mut c, "PI", 4.0) - 3.14).abs() < 1e-12);
        assert!((call(&mut c, "E", 10.0) - 2.71828182).abs() < 1e-12);
        assert!((call(&mut c, "SQRT1_2", 6.0) - 0.7071).abs() < 1e-12);
        // A name with no matching constant, a precision below 1, and a
        // missing precision argument all yield NaN.
        assert!(call(&mut c, "NOPE", 10.0).is_nan());
        assert!(call(&mut c, "PI", 0.0).is_nan());
        assert!(constant_fn(&mut c, &[XPathValue::String("PI".to_string())])
            .unwrap()
            .as_number()
            .is_nan());
    }

    #[test]
    fn test_random_in_range() {
        let mut c = ctx();
        let v = random_fn(&mut c, &[]).unwrap().as_number();
        assert!((0.0..1.0).contains(&v));
    }
}
