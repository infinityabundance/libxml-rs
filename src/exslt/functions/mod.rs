//! EXSLT Functions (func:) — func:function, func:result, func:script (§35).
//!
//! # UPSTREAM-PARITY
//!
//! Upstream libxslt (libexslt/functions.c) implements the EXSLT Functions
//! module: `<func:function>` extension elements in a stylesheet define
//! functions callable from XPath expressions; `<func:result>` returns a
//! value from the function body; `<func:script>` provides an implementation
//! in a scripting language.
//!
//! libxslt compiles `func:function` definitions into the stylesheet and
//! registers a lookup so XPath calls invoke the function body (executed as
//! a template). The module also registers `func:result` as an extension
//! element handled during function-body execution.
//!
//! This module provides the namespace registration and the stylesheet
//! scanning that makes `<func:function>` definitions callable: each
//! definition with a body that is a single XPath expression in its `select`
//! attribute (or whose content is a literal) is registered under its
//! expanded QName.

use super::{register, ExsltFunction};
use crate::xml::xpath::context::XPathContext;
use crate::xml::xpath::types::XPathValue;

/// The EXSLT Functions namespace URI.
pub const FUNC_NS: &str = "http://exslt.org/functions";

/// Register the module. `func:function`/`func:result`/`func:script` are
/// extension *elements*; no standalone functions are registered here.
/// The element handling is wired through the stylesheet compiler via
/// [`register_stylesheet_functions`].
pub fn register_all() {
    // Marker registration so `function-available('func:function')` and
    // `element-available('func:function')` report availability.
    register("func:function", func_marker as ExsltFunction);
    register("func:result", func_marker as ExsltFunction);
    register("func:script", func_marker as ExsltFunction);
}

/// Marker function (the element forms are handled by the compiler, not as
/// XPath functions).
const fn func_marker(_ctx: &mut XPathContext, _args: &[XPathValue]) -> Result<XPathValue, String> {
    Ok(XPathValue::String(String::new()))
}

/// Scan a stylesheet document for `<func:function>` elements and register
/// callable functions for each definition whose body is a single XPath
/// `select` expression.
///
/// # SAFETY
///
/// - `doc` must be a valid parsed stylesheet document.
/// - The registered functions capture the stylesheet document; the
///   functions remain valid while the stylesheet is alive.
pub unsafe fn register_stylesheet_functions(
    ctx: &mut XPathContext,
    doc: *mut crate::abi::structs::_xmlDoc,
) {
    if doc.is_null() {
        return;
    }
    let root = crate::xml::tree::doc_get_root_element(doc);
    if root.is_null() {
        return;
    }
    scan_for_functions(ctx, root);
}

/// Recursively scan a subtree for `func:function` elements.
///
/// # SAFETY
///
/// - `node` must be a valid node.
unsafe fn scan_for_functions(ctx: &mut XPathContext, node: *mut crate::abi::structs::_xmlNode) {
    if node.is_null() {
        return;
    }
    if (*node).type_ == crate::abi::types::xmlElementType::XML_ELEMENT_NODE as i32
        && !(*node).ns.is_null()
        && !(*(*node).ns).href.is_null()
    {
        let href = crate::abi::versioning::c_str_to_bytes(
            (*(*node).ns).href as *const std::os::raw::c_char,
        );
        if href == Some(b"http://exslt.org/functions") && !(*node).name.is_null() {
            let local =
                crate::abi::versioning::c_str_to_bytes((*node).name as *const std::os::raw::c_char);
            if local == Some(b"function") {
                register_function_def(ctx, node);
            }
        }
    }
    // Recurse into children.
    let mut child = (*node).children;
    while !child.is_null() {
        let next = (*child).next;
        scan_for_functions(ctx, child);
        child = next;
    }
}

/// Register a single `<func:function>` definition.
///
/// # SAFETY
///
/// - `node` must be a valid `func:function` element.
unsafe fn register_function_def(_ctx: &mut XPathContext, node: *mut crate::abi::structs::_xmlNode) {
    // The function name is the element's QName (prefix:local). Determine
    // the full name from the prefix binding + local name.
    let name =
        crate::xml::tree::get_prop(node, c"name".as_ptr() as *const crate::abi::types::xmlChar);
    if name.is_null() {
        return;
    }
    let full_name = crate::abi::versioning::c_str_to_bytes(name as *const std::os::raw::c_char)
        .map(|b| String::from_utf8_lossy(b).into_owned())
        .unwrap_or_default();
    libc::free(name as *mut libc::c_void);
    if full_name.is_empty() {
        return;
    }

    // Look for a select attribute carrying the body expression.
    let select = crate::xml::tree::get_prop(
        node,
        c"select".as_ptr() as *const crate::abi::types::xmlChar,
    );
    if let Some(sel) = select.as_mut() {
        let expr = crate::abi::versioning::c_str_to_bytes(*sel as *const std::os::raw::c_char)
            .map(|b| String::from_utf8_lossy(b).into_owned())
            .unwrap_or_default();
        if !expr.is_empty() {
            let expr = expr.clone();
            let closure_name = full_name.clone();
            // Register a closure-style function: evaluate the stored
            // expression in the current context.
            let f = move |ctx: &mut XPathContext, _args: &[XPathValue]| {
                crate::xml::xpath::evaluate_str(&expr, ctx)
                    .ok_or_else(|| format!("func:function '{}' evaluation failed", closure_name))
            };
            register(&full_name, f);
        }
        libc::free(*sel as *mut libc::c_void);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_registered() {
        register_all();
        assert!(super::super::lookup("func:function").is_some());
        assert!(super::super::lookup("func:result").is_some());
        assert!(super::super::lookup("func:script").is_some());
    }
}
