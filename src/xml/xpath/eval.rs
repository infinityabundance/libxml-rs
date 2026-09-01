//! XPath 1.0 Evaluation Engine (§25).
//!
//! Evaluates compiled XPath expressions against XML trees using the
//! internal Rust representation.
//!
//! # UPSTREAM-PARITY
//!
//! Full XPath 1.0 evaluation semantics: location paths, axes, node tests,
//! predicates, functions, operators, type conversions, comparison semantics.
//!
//! # Courts
//!
//! XPATH-EVAL-*
//!
//! # Upstream contract
//!
//! Mirrors the evaluation core of upstream `xpath.c`
//! (`SRC-LIBXML2-2.15.0-XPATH-C`, parity target libxml2 2.15.3 oracle):
//! xmlXPathEvalExpr semantics for location paths, predicates, operators,
//! type conversions and comparison rules.
//!
//! # Conceptual behavior
//!
//! Walks the AST with an explicit recursion guard (`push_recursion` /
//! `pop_recursion`). Absolute paths start at the document node (the
//! `_xmlDoc` cast to `_xmlNode`, type 9), NOT at the root element — the
//! R-000102 fix that aligns `/` with XPath 1.0. Predicates are evaluated
//! per-step with live context position/size; node-sets are deduplicated
//! in document order.
//!
//! # Ownership & safety invariants
//!
//! Evaluation borrows the tree and the context: node-sets hold borrowed
//! `_xmlNode` pointers that stay valid while the document lives; the
//! engine never frees tree nodes. Values returned to the C ABI are
//! converted in exports.rs and owned by the `_xmlXPathObject`. Context
//! mutation is confined to the evaluation call (no global state).
//!
//! # Historical quirks & epochs
//!
//! R-000102: absolute paths originally evaluated from the root element
//! (candidate bug) and were fixed to the document node. R-000159:
//! `//book[position() <= 2]` matched one extra node until the predicate
//! context-size semantics were aligned with the oracle. Both were sealed
//! during the 11.1 XPath closure against the 2.15.3 oracle.
//!
//! # Deliberate oddities
//!
//! The engine evaluates predicates eagerly per step (upstream
//! xmlXPathCompOpEval style) rather than collecting then filtering —
//! position()/last() depend on that order, so it is deliberate.
//!
//! # Proving courts
//!
//! XPATH-EVAL-* and the XPATH differential probes compare evaluation
//! results byte-identical against the oracle; the XSLT courts
//! (CLI-XSLTPROC-*) run compiled templates through this engine.
//!
//! # Tempting simplifications that would break parity
//!
//! Do not defer predicate evaluation to a post-filter pass: position() /
//! last() are order-sensitive (R-000159). Do not drop the recursion
//! guard, and do not evaluate absolute paths from the root element
//! (R-000102) — both are observable through xmlXPathEval results.

use crate::abi::structs::_xmlNode;
use crate::xml::xpath::ast::{Axis, BinaryOp, Expr, NameTest, NodeTest, Step};
use crate::xml::xpath::axes;
use crate::xml::xpath::context::XPathContext;
use crate::xml::xpath::parser_context::{
    free_parser_context, new_parser_context, value_pop, value_push,
};
use crate::xml::xpath::types::{node_string_value, string_to_number, NodeSet, XPathValue};
use core::ffi::c_void;
use std::os::raw::c_int;

// ═══════════════════════════════════════════════════════════════════════════════
// Evaluation
// ═══════════════════════════════════════════════════════════════════════════════

/// Evaluate an XPath expression in the given context.
pub fn eval(ctx: &mut XPathContext, expr: &Expr) -> Result<XPathValue, String> {
    ctx.push_recursion()?;

    let result = match expr {
        Expr::Step(step) => eval_step(ctx, step),
        Expr::AbsolutePath(expr) => eval_absolute_path(ctx, expr),
        Expr::RelativePath(left, right) => eval_relative_path(ctx, left, right),
        Expr::Filter(expr, predicates) => eval_filter(ctx, expr, predicates),
        Expr::Variable(name) => eval_variable(ctx, name),
        Expr::StringLiteral(s) => Ok(XPathValue::String(s.clone())),
        Expr::NumberLiteral(n) => Ok(XPathValue::Number(*n)),
        Expr::BooleanLiteral(b) => Ok(XPathValue::Boolean(*b)),
        Expr::FunctionCall { name, args } => eval_function_call(ctx, name, args),
        Expr::BinaryOp { op, left, right } => eval_binary_op(ctx, op, left, right),
        Expr::UnaryMinus(expr) => {
            let val = eval(ctx, expr)?;
            Ok(XPathValue::Number(-val.as_number()))
        }
        Expr::Union(left, right) => eval_union(ctx, left, right),
    };

    ctx.pop_recursion();
    result
}

// ═══════════════════════════════════════════════════════════════════════════════
// Location Path Evaluation
// ═══════════════════════════════════════════════════════════════════════════════

/// Evaluate an absolute location path: `/foo/bar`.
fn eval_absolute_path(ctx: &mut XPathContext, expr: &Expr) -> Result<XPathValue, String> {
    // Start from the document root
    let doc = ctx.document;
    if doc.is_null() {
        return Ok(XPathValue::NodeSet(NodeSet::new()));
    }

    {
        // The document node is the _xmlDoc cast to _xmlNode (type 9).
        // Absolute paths like `/root/item` select relative to the
        // document node itself, NOT the root element. This matches
        // XPath 1.0: `/` selects the document root node.
        let doc_node = doc as *mut _xmlNode;

        // Set context to document node and evaluate the path
        let saved_node = ctx.context_node;
        let saved_list = ctx.context_list.clone();
        ctx.context_node = doc_node;
        ctx.set_context_list(vec![doc_node]);

        let result = eval(ctx, expr);

        ctx.context_node = saved_node;
        ctx.context_list = saved_list;

        result
    }
}

/// Evaluate a relative location path: `foo/bar`.
fn eval_relative_path(
    ctx: &mut XPathContext,
    left: &Expr,
    right: &Expr,
) -> Result<XPathValue, String> {
    // Evaluate left side to get a node-set
    let left_val = eval(ctx, left)?;
    let left_ns = left_val.as_node_set().clone();

    let mut result = NodeSet::new();

    for node in left_ns.iter() {
        // For each node in the left result, evaluate the right step
        let saved_node = ctx.context_node;
        let saved_list = ctx.context_list.clone();
        ctx.context_node = node;
        ctx.set_context_list(left_ns.iter().collect());

        match eval(ctx, right) {
            Ok(val) => {
                if let XPathValue::NodeSet(ns) = val {
                    for n in ns.iter() {
                        result.push(n);
                    }
                }
            }
            Err(e) => {
                ctx.context_node = saved_node;
                ctx.context_list = saved_list;
                return Err(e);
            }
        }

        ctx.context_node = saved_node;
        ctx.context_list = saved_list;
    }

    Ok(XPathValue::NodeSet(result))
}

/// Evaluate a single step.
/// Evaluate one axis step, applying predicates to the traversed node-set.
///
/// # Safety
///
/// - `ctx.context_node` must be NULL or a valid `_xmlNode` that stays
///   alive for the call; `traverse_axis` reads the node's subtree, so the
///   whole reachable tree must be valid and stable while it runs; the
///   predicate evaluation temporarily mutates the context fields, which
///   must not be observed by other threads during the call.
fn eval_step(ctx: &mut XPathContext, step: &Step) -> Result<XPathValue, String> {
    let context_node = ctx.context_node;
    if context_node.is_null() {
        return Ok(XPathValue::NodeSet(NodeSet::new()));
    }

    // UPSTREAM-PARITY (xpath.c xmlXPathCompQName): a QName / prefix:* node
    // test resolves its prefix through the context's REGISTERED namespaces
    // (ctxt->nsHash) — never the document's in-scope declarations. An
    // unregistered prefix raises XPATH_UNDEF_PREFIX_ERROR
    // ("Undefined namespace prefix: prefix").
    let node_test = resolve_step_prefix(&step.node_test, &ctx.namespaces)?;

    // Traverse the axis. Attribute nodes are included for every axis step;
    // namespace nodes only when the step is explicitly the namespace axis
    // (upstream collects namespace nodes only on AXIS_NAMESPACE).
    let include_namespaces = step.axis == Axis::Namespace;
    let mut result = unsafe {
        axes::traverse_axis(
            context_node,
            step.axis,
            &node_test,
            true, // include attributes
            include_namespaces,
        )
    };

    // Apply predicates
    for predicate in &step.predicates {
        let mut filtered = NodeSet::new();

        for (i, node) in result.iter().enumerate() {
            // Set context position and size for this node
            let saved_node = ctx.context_node;
            let saved_pos = ctx.context_position;
            let saved_prox = ctx.proximity_position;
            let saved_size = ctx.context_size;
            let saved_list = ctx.context_list.clone();

            ctx.context_node = node;
            ctx.context_position = (i + 1) as i32;
            // UPSTREAM-PARITY: position() reads proximityPosition — both
            // must track the predicate position (R-000159).
            ctx.proximity_position = (i + 1) as i32;
            ctx.context_size = result.len() as i32;

            // Evaluate predicate
            let pred_val = eval(ctx, predicate)?;

            // Predicate is true if:
            // - It's a number and equals the context position
            // - It's a boolean and is true
            // - It converts to true
            let matches = match pred_val {
                XPathValue::Number(n) => {
                    // Number predicate: match if n == context_position
                    (n - (i as f64 + 1.0)).abs() < f64::EPSILON
                        || (n.round() as i32) == (i + 1) as i32
                }
                _ => pred_val.as_boolean(),
            };

            if matches {
                filtered.push(node);
            }

            ctx.context_node = saved_node;
            ctx.context_position = saved_pos;
            ctx.proximity_position = saved_prox;
            ctx.context_size = saved_size;
            ctx.context_list = saved_list;
        }

        result = filtered;
    }

    Ok(XPathValue::NodeSet(result))
}

/// Resolve a step's node-test prefix through the registered namespaces,
/// replacing prefix-based tests with URI-based ones (upstream
/// `xmlXPathCompQName`/`xmlXPathCompNodeTest` do this at compile time; the
/// candidate's AST is built without the context, so the resolution happens
/// here at the start of each step evaluation).
///
/// An unregistered prefix is an error (XPATH_UNDEF_PREFIX_ERROR).
fn resolve_step_prefix(
    node_test: &NodeTest,
    namespaces: &std::collections::HashMap<String, String>,
) -> Result<NodeTest, String> {
    match node_test {
        NodeTest::NameTest(NameTest::QName { prefix, local }) if !prefix.is_empty() => {
            let uri = namespaces
                .get(prefix)
                .ok_or_else(|| format!("Undefined namespace prefix: {}", prefix))?;
            Ok(NodeTest::NameTest(NameTest::QNameUri {
                uri: uri.clone(),
                local: local.clone(),
            }))
        }
        NodeTest::NsWildcard(prefix) if !prefix.is_empty() => {
            let uri = namespaces
                .get(prefix)
                .ok_or_else(|| format!("Undefined namespace prefix: {}", prefix))?;
            Ok(NodeTest::NsWildcardUri(uri.clone()))
        }
        _ => Ok(node_test.clone()),
    }
}

/// Evaluate a filter expression: `primary[pred1][pred2]`.
fn eval_filter(
    ctx: &mut XPathContext,
    expr: &Expr,
    predicates: &[Expr],
) -> Result<XPathValue, String> {
    // Evaluate the primary expression
    let mut result = eval(ctx, expr)?;

    // Apply predicates
    let ns = match &mut result {
        XPathValue::NodeSet(ns) => ns,
        _ => return Ok(result), // Non-node-set can't have predicates
    };

    for predicate in predicates {
        let mut filtered = NodeSet::new();
        let nodes: Vec<_> = ns.iter().collect();

        for (i, node) in nodes.iter().enumerate() {
            let saved_node = ctx.context_node;
            let saved_pos = ctx.context_position;
            let saved_prox = ctx.proximity_position;
            let saved_size = ctx.context_size;
            let saved_list = ctx.context_list.clone();

            ctx.context_node = *node;
            ctx.context_position = (i + 1) as i32;
            // UPSTREAM-PARITY: position() reads proximityPosition (R-000159).
            ctx.proximity_position = (i + 1) as i32;
            ctx.context_size = nodes.len() as i32;

            let pred_val = eval(ctx, predicate)?;

            let matches = match pred_val {
                XPathValue::Number(n) => {
                    (n - (i as f64 + 1.0)).abs() < f64::EPSILON
                        || (n.round() as i32) == (i + 1) as i32
                }
                _ => pred_val.as_boolean(),
            };

            if matches {
                filtered.push(*node);
            }

            ctx.context_node = saved_node;
            ctx.context_position = saved_pos;
            ctx.proximity_position = saved_prox;
            ctx.context_size = saved_size;
            ctx.context_list = saved_list;
        }

        *ns = filtered;
    }

    Ok(result)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Variable / Function Call Evaluation
// ═══════════════════════════════════════════════════════════════════════════════

/// Evaluate a variable reference.
fn eval_variable(ctx: &mut XPathContext, name: &str) -> Result<XPathValue, String> {
    ctx.resolve_variable(name)
        .ok_or_else(|| format!("Undefined variable: ${}", name))
}

/// Resolve a function-call name to the qualified `{URI}local` key used by
/// `xmlXPathRegisterFuncNS`. Unprefixed names stay as-is; a `prefix:local`
/// name is expanded using the context's registered namespace map. Returns
/// `None` when the prefix is undeclared or the name is unprefixed.
fn resolve_function_qualified_name(ctx: &XPathContext, name: &str) -> Option<String> {
    let (prefix, local) = name.split_once(':')?;
    let uri = ctx.namespaces.get(prefix)?;
    Some(format!("{{{}}}{}", uri, local))
}

/// Evaluate a function call.
fn eval_function_call(
    ctx: &mut XPathContext,
    name: &str,
    args: &[Expr],
) -> Result<XPathValue, String> {
    // Evaluate arguments first
    let mut evaluated_args = Vec::new();
    for arg in args {
        evaluated_args.push(eval(ctx, arg)?);
    }

    // Look up the function. Namespaced function calls (nokogiri's
    // `nokogiri-builtin:css-class` registered via xmlXPathRegisterFuncNS with
    // URI `https://www.nokogiri.org/default_ns/ruby/builtins`) are stored under
    // the `{URI}local` qualified key. The raw name (e.g. `exsl:node-set`) is
    // tried first so the XSLT function_lookup closure (which splits on ':') and
    // prefix-string-registered functions resolve; the qualified key falls back.
    let mut func_ptr: Option<*const crate::xml::xpath::context::BoxedXPathFunction> = None;
    if let Some(f) = ctx.lookup_function(name) {
        func_ptr = Some(f as *const _);
    } else if let Some(qname) = resolve_function_qualified_name(ctx, name) {
        if let Some(f) = ctx.lookup_function(&qname) {
            func_ptr = Some(f as *const _);
        }
    }
    match func_ptr {
        Some(p) => {
            // SAFETY: `p` points into `ctx.functions`, which is alive for the
            // duration of this call and is not mutated during evaluation.
            let f: &crate::xml::xpath::context::BoxedXPathFunction = unsafe { &*p };
            f(ctx, &evaluated_args)
        }
        None => {
            // C-registered extension function (xmlXPathRegisterFuncLookup):
            // consumers like nokogiri register a lookup callback that returns
            // an xmlXPathFunction; the candidate must invoke it with the
            // upstream parser-context protocol (name on ctxt->function, args
            // on the value stack, result popped off the stack).
            if ctx.func_lookup_func.is_some() {
                if let Some(result) = invoke_c_extension_function(ctx, name, &evaluated_args) {
                    return Ok(result);
                }
            }
            // UPSTREAM-PARITY (xpath.c xmlXPathCompFunction): an unknown
            // function reports "Unregistered function: name"; when the name
            // carries a prefix whose namespace was never declared, the error
            // is "Undefined namespace prefix: prefix" instead (both are
            // XPATH_UNKNOWN_FUNC / XPATH_UNDEF_PREFIX_ERROR, delivered as
            // "XPath error : ...").
            let msg = match name.split_once(':') {
                Some((prefix, _)) if !ctx.namespaces.contains_key(prefix) => {
                    format!("Undefined namespace prefix: {}", prefix)
                }
                _ => format!("Unregistered function: {}", name),
            };
            Err(msg)
        }
    }
}

/// Invoke a C-registered XPath extension function through the upstream
/// parser-context protocol.
///
/// UPSTREAM-PARITY (xpath.c `xmlXPathFunctionCall` / `xmlXPathCompFunction`):
/// the function-lookup callback (`xmlXPathFuncLookupFunc`) is called with a
/// NUL-terminated name; when it returns a function pointer, the candidate
/// builds a fresh `xmlXPathParserContext`, sets `ctxt->function` to the
/// function name (the invoker reads it), pushes the evaluated arguments in
/// REVERSE order (so the first argument is on top — the function pops them in
/// order), calls `func(pc, argc)`, and pops the result. Returns `None` when
/// the callback is absent, returns NULL, or the C context is unavailable.
///
/// # Safety
///
/// - `ctx.func_lookup_func` / `ctx.func_lookup_data` come from
///   `xmlXPathRegisterFuncLookup`; `ctx.c_context` must be the owning
///   `_xmlXPathContext`.
fn invoke_c_extension_function(
    ctx: &mut XPathContext,
    name: &str,
    args: &[XPathValue],
) -> Option<XPathValue> {
    let lookup = ctx.func_lookup_func?;
    let c_ctxt = ctx.c_context;
    if c_ctxt.is_null() {
        return None;
    }

    let mut name_nul: Vec<crate::abi::types::xmlChar> = name.as_bytes().to_vec();
    name_nul.push(0);
    // SAFETY: the callback contract returns an xmlXPathFunction (fn pointer)
    // stored as void*, or NULL when the handler does not provide the name.
    let fp = unsafe {
        lookup(
            ctx.func_lookup_data,
            name_nul.as_ptr() as *const crate::abi::types::xmlChar,
            std::ptr::null(),
        )
    };
    if fp.is_null() {
        return None;
    }
    let func: unsafe extern "C" fn(*mut c_void, c_int) = unsafe { std::mem::transmute(fp) };

    unsafe {
        let saved_function = (*c_ctxt).function;
        (*c_ctxt).function = name_nul.as_ptr() as *const crate::abi::types::xmlChar;
        let pc = new_parser_context(std::ptr::null(), c_ctxt);
        if pc.is_null() {
            (*c_ctxt).function = saved_function;
            return None;
        }

        // Push the evaluated arguments in reverse order (upstream
        // xmlXPathFunctionCall evaluates and pushes args from the last to the
        // first, so the first argument sits on top of the stack).
        let mut ok = true;
        for val in args.iter().rev() {
            let obj = crate::abi::exports_xml2::xpath_to_object_pub(val.clone());
            if obj.is_null() {
                ok = false;
                break;
            }
            value_push(pc, obj);
        }

        let result = if ok {
            func(pc as *mut c_void, args.len() as c_int);
            let res = value_pop(pc);
            if res.is_null() {
                None
            } else {
                let val = crate::abi::exports_xml2::object_to_xpathvalue_pub(res);
                crate::abi::exports_xml2::xmlXPathFreeObject(res);
                Some(val)
            }
        } else {
            None
        };

        // free_parser_context frees any remaining stack values and the
        // context itself; the popped result was already freed above.
        free_parser_context(pc);
        (*c_ctxt).function = saved_function;
        result
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Operators
// ═══════════════════════════════════════════════════════════════════════════════

/// Evaluate a binary operation.
fn eval_binary_op(
    ctx: &mut XPathContext,
    op: &BinaryOp,
    left: &Expr,
    right: &Expr,
) -> Result<XPathValue, String> {
    match op {
        BinaryOp::Or => {
            // Short-circuit: evaluate left, if true return true
            let left_val = eval(ctx, left)?;
            if left_val.as_boolean() {
                return Ok(XPathValue::Boolean(true));
            }
            let right_val = eval(ctx, right)?;
            Ok(XPathValue::Boolean(right_val.as_boolean()))
        }
        BinaryOp::And => {
            // Short-circuit: evaluate left, if false return false
            let left_val = eval(ctx, left)?;
            if !left_val.as_boolean() {
                return Ok(XPathValue::Boolean(false));
            }
            let right_val = eval(ctx, right)?;
            Ok(XPathValue::Boolean(right_val.as_boolean()))
        }
        BinaryOp::Eq | BinaryOp::Ne => {
            let left_val = eval(ctx, left)?;
            let right_val = eval(ctx, right)?;
            let eq = compare_equal(ctx, &left_val, &right_val);
            Ok(match op {
                BinaryOp::Eq => XPathValue::Boolean(eq),
                BinaryOp::Ne => XPathValue::Boolean(!eq),
                _ => unreachable!(),
            })
        }
        BinaryOp::Lt | BinaryOp::Gt | BinaryOp::Le | BinaryOp::Ge => {
            let left_val = eval(ctx, left)?;
            let right_val = eval(ctx, right)?;
            let cmp = compare_ordered(ctx, &left_val, &right_val);
            let result = match op {
                BinaryOp::Lt => cmp == std::cmp::Ordering::Less,
                BinaryOp::Gt => cmp == std::cmp::Ordering::Greater,
                BinaryOp::Le => cmp != std::cmp::Ordering::Greater,
                BinaryOp::Ge => cmp != std::cmp::Ordering::Less,
                _ => unreachable!(),
            };
            Ok(XPathValue::Boolean(result))
        }
        BinaryOp::Add => {
            let left_val = eval(ctx, left)?;
            let right_val = eval(ctx, right)?;
            Ok(XPathValue::Number(
                left_val.as_number() + right_val.as_number(),
            ))
        }
        BinaryOp::Sub => {
            let left_val = eval(ctx, left)?;
            let right_val = eval(ctx, right)?;
            Ok(XPathValue::Number(
                left_val.as_number() - right_val.as_number(),
            ))
        }
        BinaryOp::Mul => {
            let left_val = eval(ctx, left)?;
            let right_val = eval(ctx, right)?;
            Ok(XPathValue::Number(
                left_val.as_number() * right_val.as_number(),
            ))
        }
        BinaryOp::Div => {
            let left_val = eval(ctx, left)?;
            let right_val = eval(ctx, right)?;
            Ok(XPathValue::Number(
                left_val.as_number() / right_val.as_number(),
            ))
        }
        BinaryOp::Mod => {
            let left_val = eval(ctx, left)?;
            let right_val = eval(ctx, right)?;
            Ok(XPathValue::Number(
                left_val.as_number() % right_val.as_number(),
            ))
        }
        BinaryOp::Union => {
            // Union is handled at the Expr level, not BinaryOp level
            unreachable!("Union operator should be handled by Expr::Union")
        }
    }
}

/// Evaluate a union expression: `left | right`.
fn eval_union(ctx: &mut XPathContext, left: &Expr, right: &Expr) -> Result<XPathValue, String> {
    let left_val = eval(ctx, left)?;
    let right_val = eval(ctx, right)?;

    let mut result = left_val.as_node_set().clone();
    result.extend(right_val.as_node_set());
    result.sort();

    Ok(XPathValue::NodeSet(result))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Comparison Semantics
// ═══════════════════════════════════════════════════════════════════════════════

/// Compare two XPath values for equality (XPath 1.0 §3.4).
fn compare_equal(_ctx: &mut XPathContext, a: &XPathValue, b: &XPathValue) -> bool {
    match (a, b) {
        // If both are node-sets, compare by set intersection
        (XPathValue::NodeSet(ns_a), XPathValue::NodeSet(ns_b)) => {
            for node_a in ns_a.iter() {
                let val_a = node_string_value(node_a);
                for node_b in ns_b.iter() {
                    let val_b = node_string_value(node_b);
                    if val_a == val_b {
                        return true;
                    }
                }
            }
            false
        }
        // If one is a node-set and the other is not
        (XPathValue::NodeSet(ns), other) | (other, XPathValue::NodeSet(ns)) => {
            for node in ns.iter() {
                let node_str = node_string_value(node);
                match other {
                    XPathValue::Boolean(_) => {
                        // Compare boolean(node-set) == other
                        return (!ns.is_empty()) == other.as_boolean();
                    }
                    XPathValue::Number(_) => {
                        let node_num = string_to_number(&node_str);
                        if (node_num - other.as_number()).abs() < f64::EPSILON {
                            return true;
                        }
                        continue;
                    }
                    XPathValue::String(_) => {
                        if node_str == other.as_string() {
                            return true;
                        }
                        continue;
                    }
                    _ => continue,
                };
            }
            false
        }
        // Neither is a node-set
        _ => match (a, b) {
            (XPathValue::Boolean(_), _) | (_, XPathValue::Boolean(_)) => {
                a.as_boolean() == b.as_boolean()
            }
            (XPathValue::Number(_), _) | (_, XPathValue::Number(_)) => {
                let na = a.as_number();
                let nb = b.as_number();
                if na.is_nan() || nb.is_nan() {
                    false
                } else {
                    (na - nb).abs() < f64::EPSILON || na == nb
                }
            }
            _ => a.as_string() == b.as_string(),
        },
    }
}

/// Compare two XPath values for ordering (XPath 1.0 §3.4).
fn compare_ordered(_ctx: &mut XPathContext, a: &XPathValue, b: &XPathValue) -> std::cmp::Ordering {
    match (a, b) {
        // If both are node-sets, compare pairwise
        (XPathValue::NodeSet(ns_a), XPathValue::NodeSet(ns_b)) => {
            for node_a in ns_a.iter() {
                let num_a = string_to_number(&node_string_value(node_a));
                for node_b in ns_b.iter() {
                    let num_b = string_to_number(&node_string_value(node_b));
                    if num_a < num_b {
                        return std::cmp::Ordering::Less;
                    }
                    if num_a > num_b {
                        return std::cmp::Ordering::Greater;
                    }
                }
            }
            std::cmp::Ordering::Equal
        }
        // If one is a node-set
        (XPathValue::NodeSet(ns), other) | (other, XPathValue::NodeSet(ns)) => {
            let other_num = other.as_number();
            for node in ns.iter() {
                let node_num = string_to_number(&node_string_value(node));
                if node_num < other_num {
                    return std::cmp::Ordering::Less;
                }
                if node_num > other_num {
                    return std::cmp::Ordering::Greater;
                }
            }
            std::cmp::Ordering::Equal
        }
        // Neither is a node-set: compare as numbers
        _ => {
            let na = a.as_number();
            let nb = b.as_number();
            if na.is_nan() || nb.is_nan() {
                std::cmp::Ordering::Equal // NaN comparisons return false, so equal for ordering
            } else if na < nb {
                std::cmp::Ordering::Less
            } else if na > nb {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Top-level evaluation API
// ═══════════════════════════════════════════════════════════════════════════════

/// Evaluate an XPath expression string against a document.
///
/// This is the main entry point for XPath evaluation.
pub fn eval_xpath(ctx: &mut XPathContext, expression: &str) -> Result<XPathValue, String> {
    let expr = crate::xml::xpath::parser::parse_xpath(expression).map_err(|e| e.message)?;
    eval(ctx, &expr)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml::xpath::context::XPathContext;
    use crate::xml::xpath::functions;

    fn setup_context() -> XPathContext {
        let mut ctx = XPathContext::new(std::ptr::null_mut());
        // Register core functions
        let funcs = functions::core_functions();
        for (name, func) in funcs {
            ctx.register_function(&name, func);
        }
        ctx
    }

    #[test]
    fn test_eval_string_literal() {
        let mut ctx = setup_context();
        let result = eval_xpath(&mut ctx, "'hello'").unwrap();
        assert_eq!(result.as_string(), "hello");
    }

    #[test]
    fn test_eval_number_literal() {
        let mut ctx = setup_context();
        let result = eval_xpath(&mut ctx, "42").unwrap();
        assert_eq!(result.as_number(), 42.0);
    }

    #[test]
    fn test_eval_addition() {
        let mut ctx = setup_context();
        let result = eval_xpath(&mut ctx, "1 + 2").unwrap();
        assert_eq!(result.as_number(), 3.0);
    }

    #[test]
    fn test_eval_subtraction() {
        let mut ctx = setup_context();
        let result = eval_xpath(&mut ctx, "5 - 3").unwrap();
        assert_eq!(result.as_number(), 2.0);
    }

    #[test]
    fn test_eval_multiplication() {
        let mut ctx = setup_context();
        let result = eval_xpath(&mut ctx, "3 * 4").unwrap();
        assert_eq!(result.as_number(), 12.0);
    }

    #[test]
    fn test_eval_division() {
        let mut ctx = setup_context();
        let result = eval_xpath(&mut ctx, "10 div 3").unwrap();
        assert!((result.as_number() - 3.3333333333333335).abs() < 1e-10);
    }

    #[test]
    fn test_eval_modulo() {
        let mut ctx = setup_context();
        let result = eval_xpath(&mut ctx, "10 mod 3").unwrap();
        assert_eq!(result.as_number(), 1.0);
    }

    #[test]
    fn test_eval_equality() {
        let mut ctx = setup_context();
        assert!(eval_xpath(&mut ctx, "1 = 1").unwrap().as_boolean());
        assert!(!eval_xpath(&mut ctx, "1 = 2").unwrap().as_boolean());
        assert!(eval_xpath(&mut ctx, "1 != 2").unwrap().as_boolean());
    }

    #[test]
    fn test_eval_comparison() {
        let mut ctx = setup_context();
        assert!(eval_xpath(&mut ctx, "1 < 2").unwrap().as_boolean());
        assert!(eval_xpath(&mut ctx, "2 > 1").unwrap().as_boolean());
        assert!(eval_xpath(&mut ctx, "1 <= 1").unwrap().as_boolean());
        assert!(eval_xpath(&mut ctx, "2 >= 2").unwrap().as_boolean());
    }

    #[test]
    fn test_eval_and_or() {
        let mut ctx = setup_context();
        assert!(eval_xpath(&mut ctx, "true() and true()")
            .unwrap()
            .as_boolean());
        assert!(!eval_xpath(&mut ctx, "true() and false()")
            .unwrap()
            .as_boolean());
        assert!(eval_xpath(&mut ctx, "true() or false()")
            .unwrap()
            .as_boolean());
        assert!(!eval_xpath(&mut ctx, "false() or false()")
            .unwrap()
            .as_boolean());
    }

    #[test]
    fn test_eval_not() {
        let mut ctx = setup_context();
        assert!(!eval_xpath(&mut ctx, "not(true())").unwrap().as_boolean());
        assert!(eval_xpath(&mut ctx, "not(false())").unwrap().as_boolean());
    }

    #[test]
    fn test_eval_boolean() {
        let mut ctx = setup_context();
        assert!(eval_xpath(&mut ctx, "boolean('hello')")
            .unwrap()
            .as_boolean());
        assert!(!eval_xpath(&mut ctx, "boolean('')").unwrap().as_boolean());
        assert!(!eval_xpath(&mut ctx, "boolean(0)").unwrap().as_boolean());
        assert!(eval_xpath(&mut ctx, "boolean(1)").unwrap().as_boolean());
    }

    #[test]
    fn test_eval_number() {
        let mut ctx = setup_context();
        assert_eq!(
            eval_xpath(&mut ctx, "number('42')").unwrap().as_number(),
            42.0
        );
    }

    #[test]
    fn test_eval_string() {
        let mut ctx = setup_context();
        assert_eq!(
            eval_xpath(&mut ctx, "string(42)").unwrap().as_string(),
            "42"
        );
    }

    #[test]
    fn test_eval_concat() {
        let mut ctx = setup_context();
        assert_eq!(
            eval_xpath(&mut ctx, "concat('a', 'b', 'c')")
                .unwrap()
                .as_string(),
            "abc"
        );
    }

    #[test]
    fn test_eval_starts_with() {
        let mut ctx = setup_context();
        assert!(eval_xpath(&mut ctx, "starts-with('hello', 'he')")
            .unwrap()
            .as_boolean());
    }

    #[test]
    fn test_eval_contains() {
        let mut ctx = setup_context();
        assert!(eval_xpath(&mut ctx, "contains('hello', 'ell')")
            .unwrap()
            .as_boolean());
    }

    #[test]
    fn test_eval_substring() {
        let mut ctx = setup_context();
        assert_eq!(
            eval_xpath(&mut ctx, "substring('12345', 1, 3)")
                .unwrap()
                .as_string(),
            "123"
        );
        assert_eq!(
            eval_xpath(&mut ctx, "substring('12345', 2)")
                .unwrap()
                .as_string(),
            "2345"
        );
    }

    #[test]
    fn test_eval_string_length() {
        let mut ctx = setup_context();
        assert_eq!(
            eval_xpath(&mut ctx, "string-length('hello')")
                .unwrap()
                .as_number(),
            5.0
        );
    }

    #[test]
    fn test_eval_normalize_space() {
        let mut ctx = setup_context();
        assert_eq!(
            eval_xpath(&mut ctx, "normalize-space('  hello   world  ')")
                .unwrap()
                .as_string(),
            "hello world"
        );
    }

    #[test]
    fn test_eval_floor_ceiling_round() {
        let mut ctx = setup_context();
        assert_eq!(eval_xpath(&mut ctx, "floor(3.7)").unwrap().as_number(), 3.0);
        assert_eq!(
            eval_xpath(&mut ctx, "ceiling(3.2)").unwrap().as_number(),
            4.0
        );
        assert_eq!(eval_xpath(&mut ctx, "round(3.5)").unwrap().as_number(), 4.0);
    }

    #[test]
    fn test_eval_sum() {
        // sum() on an empty node-set should return 0
        let mut ctx = setup_context();
        ctx.document = std::ptr::null_mut();
        // Can't test sum directly without a node-set, but we can test it
        // with literal values when we add node-set construction
    }

    #[test]
    fn test_eval_variable_not_found() {
        let mut ctx = setup_context();
        let result = eval_xpath(&mut ctx, "$undefined_var");
        assert!(result.is_err());
    }

    #[test]
    fn test_eval_variable_found() {
        let mut ctx = setup_context();
        ctx.register_variable("x", XPathValue::Number(42.0));
        let result = eval_xpath(&mut ctx, "$x").unwrap();
        assert_eq!(result.as_number(), 42.0);
    }

    #[test]
    fn test_eval_union() {
        // Union requires node-sets, which need a document.
        // Test basic union of numbers (should error since numbers aren't node-sets).
        // Actually, union on non-node-sets would panic at as_node_set().
        // For now, this is a placeholder for when we have document support.
    }

    #[test]
    fn test_eval_operator_precedence() {
        let mut ctx = setup_context();
        // 1 + 2 * 3 should be 7 (multiplication before addition)
        let result = eval_xpath(&mut ctx, "1 + 2 * 3").unwrap();
        assert_eq!(result.as_number(), 7.0);

        // (1 + 2) * 3 should be 9
        let result = eval_xpath(&mut ctx, "(1 + 2) * 3").unwrap();
        assert_eq!(result.as_number(), 9.0);
    }

    #[test]
    fn test_eval_unary_minus() {
        let mut ctx = setup_context();
        let result = eval_xpath(&mut ctx, "-5").unwrap();
        assert_eq!(result.as_number(), -5.0);

        let result = eval_xpath(&mut ctx, "--5").unwrap();
        assert_eq!(result.as_number(), 5.0);
    }

    #[test]
    fn test_eval_true_false() {
        let mut ctx = setup_context();
        assert!(eval_xpath(&mut ctx, "true()").unwrap().as_boolean());
        assert!(!eval_xpath(&mut ctx, "false()").unwrap().as_boolean());
    }

    #[test]
    fn test_eval_translate() {
        let mut ctx = setup_context();
        assert_eq!(
            eval_xpath(
                &mut ctx,
                "translate('hello', 'abcdefghijklmnopqrstuvwxyz', 'ABCDEFGHIJKLMNOPQRSTUVWXYZ')"
            )
            .unwrap()
            .as_string(),
            "HELLO"
        );
    }

    #[test]
    fn test_eval_empty_expression() {
        let mut ctx = setup_context();
        let result = eval_xpath(&mut ctx, "");
        assert!(result.is_err());
    }
}
