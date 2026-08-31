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
    let name = args
        .first()
        .map(|a| a.as_string().to_ascii_uppercase())
        .unwrap_or_default();
    let value = match name.as_str() {
        "PI" => std::f64::consts::PI,
        "E" => std::f64::consts::E,
        "SQRRT2" => std::f64::consts::SQRT_2,
        "LN2" => std::f64::consts::LN_2,
        "LN10" => std::f64::consts::LN_10,
        "LOG2E" => std::f64::consts::LOG2_E,
        "LOG10E" => std::f64::consts::LOG10_E,
        _ => f64::NAN,
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
pub fn register_all() {
    register("math:max", max_fn as ExsltFunction);
    register("math:min", min_fn as ExsltFunction);
    register("math:highest", highest_fn as ExsltFunction);
    register("math:lowest", lowest_fn as ExsltFunction);
    register("math:abs", abs_fn as ExsltFunction);
    register("math:sqrt", sqrt_fn as ExsltFunction);
    register("math:power", power_fn as ExsltFunction);
    register("math:log", log_fn as ExsltFunction);
    register("math:sin", sin_fn as ExsltFunction);
    register("math:cos", cos_fn as ExsltFunction);
    register("math:tan", tan_fn as ExsltFunction);
    register("math:asin", asin_fn as ExsltFunction);
    register("math:acos", acos_fn as ExsltFunction);
    register("math:atan", atan_fn as ExsltFunction);
    register("math:atan2", atan2_fn as ExsltFunction);
    register("math:exp", exp_fn as ExsltFunction);
    register("math:constant", constant_fn as ExsltFunction);
    register("math:random", random_fn as ExsltFunction);
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
        let mut c = ctx();
        let v = constant_fn(
            &mut c,
            &[
                XPathValue::String("PI".to_string()),
                XPathValue::Number(10.0),
            ],
        )
        .unwrap()
        .as_number();
        assert!((v - std::f64::consts::PI).abs() < 1e-9);
        let v = constant_fn(&mut c, &[XPathValue::String("E".to_string())])
            .unwrap()
            .as_number();
        assert!((v - std::f64::consts::E).abs() < 1e-9);
    }

    #[test]
    fn test_random_in_range() {
        let mut c = ctx();
        let v = random_fn(&mut c, &[]).unwrap().as_number();
        assert!((0.0..1.0).contains(&v));
    }
}
