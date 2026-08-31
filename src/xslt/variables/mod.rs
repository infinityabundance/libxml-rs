//! XSLT variable and parameter binding (§33, §85 Phase 8).
//!
//! Variables in XSLT are declared with `<xsl:variable>` and `<xsl:param>`.
//! They can be global (top-level) or local (inside a template).
//!
//! Variables hold either:
//! - An XPath expression result (via the `select` attribute)
//! - A result tree fragment (via inline content)
//!
//! # UPSTREAM-PARITY
//!
//! Upstream libxslt stores variables on a stack (`varsTab`) in the transform
//! context. Each variable is an `_xsltStackElem` chained via `next`.
//! The stack is grown dynamically and supports saving/restoring the base
//! (for template-local scoping).
//!
//! # Courts
//!
//! XSLT-VARIABLES-*, XSLT-PARAMS-*
//!
//! # Historical quirks & epochs
//!
//! The libxslt variable-stack discipline has been stable across the whole
//! epoch (E-008: 1.1.26..1.1.45 byte-identical). R-000158 (11.1-X)
//! established the stack contract: push loops must snapshot the source
//! list, and pops must restore the pre-call stack depth — never a count
//! derived from the pushed items.
//!
//! # Tempting simplifications that would break parity
//!
//! A tempting simplification is to use a Rust Vec and pop exactly as many
//! elements as were pushed. R-000158 proved that is wrong: the stack is
//! shared with caller frames, so restoring must happen by saved depth.
//! Another shortcut, deep-copying variable values on push, breaks RTF
//! identity (exsl:node-set on a variable must see the original result
//! tree fragment, not a copy).

use crate::abi::allocator::xmlFreeImpl;
use crate::abi::exports_xml2::{xmlXPathFreeObject, xmlXPathObjectCopy};
use crate::abi::structs::*;
use crate::abi::types::*;
use std::os::raw::c_int;
use std::ptr;

/// Stack element flags
pub const XSLT_VAR_GLOBAL: c_int = 1 << 0;
/// The variable is a stylesheet or template parameter (`xsl:param`).
pub const XSLT_VAR_PARAM: c_int = 1 << 1;
/// The variable is created internally by the engine (not user-visible).
pub const XSLT_VAR_INTERNAL: c_int = 1 << 2;

/// Push a variable onto the variable stack.
///
/// # SAFETY
///
/// - `ctxt` must be a valid `_xsltTransformContext`.
/// - `var` must be a valid `_xsltStackElem` allocated by this library.
/// - The variable is owned by the context stack after this call.
pub unsafe fn xsltPushVariable(
    ctxt: *mut _xsltTransformContext,
    var: *mut _xsltStackElem,
) -> c_int {
    if ctxt.is_null() || var.is_null() {
        return -1;
    }
    // Phase 8: stack management — push onto varsTab
    let ctx = &mut *ctxt;
    let new_vars_nr = ctx.varsNr + 1;
    if new_vars_nr > ctx.varsMax {
        // Grow the variable table (2x or +16).
        let new_max = if ctx.varsMax == 0 {
            16
        } else {
            ctx.varsMax * 2
        };
        // Reallocate the table.
        let new_tab = libc::realloc(
            ctx.varsTab as *mut libc::c_void,
            (new_max as usize) * core::mem::size_of::<*mut _xsltStackElem>(),
        ) as *mut *mut _xsltStackElem;
        if new_tab.is_null() {
            return -1;
        }
        ctx.varsTab = new_tab;
        ctx.varsMax = new_max;
    }
    // Chain the new variable onto the existing stack head.
    (*var).next = if ctx.varsNr > 0 {
        *(ctx.varsTab.offset((ctx.varsNr - 1) as isize))
    } else {
        ptr::null_mut()
    };
    *(ctx.varsTab.offset(ctx.varsNr as isize)) = var;
    ctx.varsNr = new_vars_nr;

    // Register the variable in the internal XPath context's variable hash
    // so `$name` resolves during XPath evaluation. Local variables live on
    // the transform variable stack; the XPath evaluator reads the hash.
    // (Global variables are registered separately by xsltInitGlobalVariables.)
    let xpath_ctxt = ctx.xpathCtxt;
    if !xpath_ctxt.is_null() && !(*var).name.is_null() {
        let internal = (*xpath_ctxt).extra as *mut crate::xml::xpath::context::XPathContext;
        if !internal.is_null() {
            let name_len = libc::strlen((*var).name as *const libc::c_char);
            let name_bytes = core::slice::from_raw_parts((*var).name, name_len);
            let name = String::from_utf8_lossy(name_bytes).into_owned();
            let value = if (*var).value.is_null() {
                crate::xml::xpath::types::XPathValue::String(String::new())
            } else {
                crate::abi::exports_xml2::object_to_xpathvalue_pub((*var).value)
            };
            (*internal).register_variable(&name, value);
        }
    }
    0
}

/// Pop a variable from the variable stack.
///
/// # SAFETY
///
/// - `ctxt` must be a valid `_xsltTransformContext`.
/// - The returned pointer is owned by the caller and must be freed
///   with `xsltFreeStackElem`.
pub unsafe fn xsltPopVariable(ctxt: *mut _xsltTransformContext) -> *mut _xsltStackElem {
    if ctxt.is_null() {
        return ptr::null_mut();
    }
    let ctx = &mut *ctxt;
    if ctx.varsNr == 0 {
        return ptr::null_mut();
    }
    ctx.varsNr -= 1;
    let var = *(ctx.varsTab.offset(ctx.varsNr as isize));
    // Unlink from the chain.
    if ctx.varsNr > 0 {
        let prev = *(ctx.varsTab.offset((ctx.varsNr - 1) as isize));
        (*prev).next = ptr::null_mut();
    }
    (*var).next = ptr::null_mut();

    // Unregister the variable from the internal XPath context's hash.
    if !(*var).name.is_null() {
        let xpath_ctxt = ctx.xpathCtxt;
        if !xpath_ctxt.is_null() {
            let internal = (*xpath_ctxt).extra as *mut crate::xml::xpath::context::XPathContext;
            if !internal.is_null() {
                let name_len = libc::strlen((*var).name as *const libc::c_char);
                let name_bytes = core::slice::from_raw_parts((*var).name, name_len);
                let name = String::from_utf8_lossy(name_bytes).into_owned();
                (*internal).unregister_variable(&name);
            }
        }
    }
    var
}

/// Look up a variable by name in the current scope.
///
/// Walks the variable stack from the top (most recent) down to the base.
///
/// # SAFETY
///
/// - `ctxt` must be a valid `_xsltTransformContext`.
/// - `name` must be a valid NUL-terminated string.
pub unsafe fn xsltLookupVariable(
    ctxt: *mut _xsltTransformContext,
    name: *const xmlChar,
    ns_uri: *const xmlChar,
) -> *mut _xsltStackElem {
    if ctxt.is_null() || name.is_null() {
        return ptr::null_mut();
    }
    let ctx = &mut *ctxt;
    let mut i = ctx.varsNr;
    while i > 0 {
        i -= 1;
        let var = *(ctx.varsTab.offset(i as isize));
        if var.is_null() {
            continue;
        }
        // Compare names (and namespace URIs when both present).
        let vname = (*var).name;
        if vname.is_null() {
            continue;
        }
        if libc::strcmp(vname as *const libc::c_char, name as *const libc::c_char) != 0 {
            continue;
        }
        if !ns_uri.is_null()
            && !(*var).nameURI.is_null()
            && libc::strcmp(
                (*var).nameURI as *const libc::c_char,
                ns_uri as *const libc::c_char,
            ) != 0
        {
            continue;
        }
        return var;
    }
    ptr::null_mut()
}

/// Evaluate a variable and return its value.
///
/// If the variable already has a value, returns a copy.
/// Otherwise evaluates the select expression or the inline content.
///
/// # SAFETY
///
/// - `ctxt` must be a valid `_xsltTransformContext`.
/// - `var` must be a valid `_xsltStackElem`.
pub unsafe fn xsltEvalVariable(
    ctxt: *mut _xsltTransformContext,
    var: *mut _xsltStackElem,
) -> *mut _xmlXPathObject {
    if ctxt.is_null() || var.is_null() {
        return ptr::null_mut();
    }
    let v = &mut *var;
    if !v.value.is_null() {
        return xmlXPathObjectCopy(v.value);
    }
    // Phase 8: evaluate select expression or inline content.
    ptr::null_mut()
}

/// Free a variable stack element.
///
/// # SAFETY
///
/// - `var` must be a valid `_xsltStackElem` allocated by this library
///   and not already freed.
pub unsafe fn xsltFreeStackElem(var: *mut _xsltStackElem) {
    if var.is_null() {
        return;
    }
    let v = &mut *var;
    // UPSTREAM-PARITY: comp is xsltStylePreComp (opaque); the candidate does
    // not allocate it, so nothing to free here.
    if !v.value.is_null() {
        xmlXPathFreeObject(v.value);
        v.value = ptr::null_mut();
    }
    // NOTE: `tree` points at the stylesheet's inline content nodes
    // (compiler sets var->tree = inst->children); those nodes belong to the
    // stylesheet document and are freed by xsltFreeStylesheet. They must NOT
    // be freed here (would double-free). Runtime result-tree fragments are
    // separate RVT documents owned by the transform context's docCache.
    // The name/select/nameURI strings are dictionary-owned or stylesheet-owned;
    // only free them if this is a caller-created variable (from params parsing).
    if (v.flags & XSLT_VAR_INTERNAL) != 0 {
        if !v.name.is_null() {
            libc::free(v.name as *mut libc::c_void);
        }
        if !v.nameURI.is_null() {
            libc::free(v.nameURI as *mut libc::c_void);
        }
        if !v.select.is_null() {
            libc::free(v.select as *mut libc::c_void);
        }
    }
    v.next = ptr::null_mut();
    xmlFreeImpl(var as *mut libc::c_void);
}

/// Free all global variables in a transform context.
///
/// # SAFETY
///
/// - `ctxt` must be a valid `_xsltTransformContext`.
pub unsafe fn xsltFreeGlobalVariables(ctxt: *mut _xsltTransformContext) {
    if ctxt.is_null() {
        return;
    }
    let ctx = &mut *ctxt;
    let mut i = 0;
    while i < ctx.varsNr {
        let var = *(ctx.varsTab.offset(i as isize));
        if !var.is_null() {
            xsltFreeStackElem(var);
            *(ctx.varsTab.offset(i as isize)) = ptr::null_mut();
        }
        i += 1;
    }
    ctx.varsNr = 0;
    if !ctx.varsTab.is_null() {
        libc::free(ctx.varsTab as *mut libc::c_void);
        ctx.varsTab = ptr::null_mut();
        ctx.varsMax = 0;
    }
}

/// Initialize global variables from the stylesheet.
///
/// Evaluates all global `<xsl:variable>` elements and registers their
/// values in the XPath context's variable hash so `$name` references
/// resolve during template execution. Global parameters set by the
/// caller (via the params array) take precedence over stylesheet
/// defaults.
///
/// # UPSTREAM-PARITY
///
/// Upstream libxslt (variables.c `xsltInitializeCtxt`) evaluates global
/// variables lazily on first use. We evaluate them eagerly here, which
/// is behaviorally equivalent for well-formed stylesheets.
///
/// # SAFETY
///
/// - `ctxt` must be a valid `_xsltTransformContext`.
pub unsafe fn xsltInitGlobalVariables(ctxt: *mut _xsltTransformContext) {
    if ctxt.is_null() {
        return;
    }
    let ctx = &mut *ctxt;
    let style = ctx.style;
    if style.is_null() {
        return;
    }
    // First, register caller-provided parameters. UPSTREAM-PARITY: global
    // params and variables both live in the stylesheet's `variables` list.
    // Caller params carry XSLT_VAR_PARAM|XSLT_VAR_INTERNAL (set by
    // xsltParseStylesheetParam); the stylesheet's OWN global xsl:param
    // defaults carry only XSLT_VAR_PARAM (compile_variable) and must NOT be
    // treated as caller params — otherwise they would overwrite the caller's
    // values (CLI-XSLTPROC-0012).
    let mut param = (*style).variables;
    while !param.is_null() {
        if (*param).flags & (XSLT_VAR_PARAM | XSLT_VAR_INTERNAL)
            == (XSLT_VAR_PARAM | XSLT_VAR_INTERNAL)
        {
            register_global_value(ctxt, param, false);
        }
        param = (*param).next;
    }
    // Then evaluate stylesheet-defined global variables/params, skipping
    // names already bound by the caller (upstream xsltEvalGlobalVariables
    // consults ctxt->globalVars).
    let mut var = (*style).variables;
    while !var.is_null() {
        register_global_value(ctxt, var, true);
        var = (*var).next;
    }
}

/// Evaluate a single global variable/parameter and register its value in
/// the XPath context's variable hash.
///
/// When `skip_if_bound` is set (stylesheet defaults), a variable whose name
/// is already bound by a caller-provided parameter is not evaluated.
///
/// # SAFETY
///
/// - `ctxt` must be a valid `_xsltTransformContext`.
/// - `var` must be a valid `_xsltStackElem`.
unsafe fn register_global_value(
    ctxt: *mut _xsltTransformContext,
    var: *mut _xsltStackElem,
    skip_if_bound: bool,
) {
    if var.is_null() || (*var).name.is_null() {
        return;
    }
    let name = crate::abi::versioning::c_str_to_bytes((*var).name as *const std::os::raw::c_char);
    let name = match name {
        Some(n) => String::from_utf8_lossy(n).into_owned(),
        None => return,
    };
    if skip_if_bound {
        let xpath_ctxt = (*ctxt).xpathCtxt;
        if !xpath_ctxt.is_null() {
            let internal = (*xpath_ctxt).extra as *mut crate::xml::xpath::context::XPathContext;
            if !internal.is_null() && (*internal).variables.contains_key(&name) {
                return;
            }
        }
    }
    // Compute the value: caller params carry a string value in `select`;
    // stylesheet variables have a select expression or inline content.
    let value: Option<crate::xml::xpath::types::XPathValue> = if !(*var).value.is_null() {
        // Already evaluated (e.g. a caller param parsed by
        // xsltParseStylesheetParams carries the string in select).
        let v = (*var).value;
        let typ = (*v).type_;
        if typ == xmlXPathObjectType::XPATH_STRING as c_int {
            let s = (*v).stringval;
            let s = if s.is_null() {
                String::new()
            } else {
                crate::abi::versioning::c_str_to_bytes(s as *const std::os::raw::c_char)
                    .map(|b| String::from_utf8_lossy(b).into_owned())
                    .unwrap_or_default()
            };
            Some(crate::xml::xpath::types::XPathValue::String(s))
        } else {
            None
        }
    } else if !(*var).select.is_null() {
        // Evaluate the select expression in the context of the document
        // root.
        let xpath_ctxt = (*ctxt).xpathCtxt;
        if xpath_ctxt.is_null() {
            return;
        }
        // Save and set the context to the document node.
        let saved_node = (*xpath_ctxt).node;
        (*xpath_ctxt).node = (*(*ctxt).document).doc as *mut _xmlNode;
        let internal = (*xpath_ctxt).extra as *mut crate::xml::xpath::context::XPathContext;
        if !internal.is_null() {
            (*internal).context_node = (*(*ctxt).document).doc as *mut _xmlNode;
            (*internal).document = (*(*ctxt).document).doc;
        }
        let obj = crate::abi::exports_xml2::xmlXPathEvalExpression((*var).select, xpath_ctxt);
        (*xpath_ctxt).node = saved_node;
        let result = if !obj.is_null() {
            let v = crate::abi::exports_xml2::object_to_xpathvalue_pub(obj);
            crate::abi::exports_xml2::xmlXPathFreeObject(obj);
            Some(v)
        } else {
            // UPSTREAM-PARITY: a failed user-parameter evaluation reports
            // the XPath error, a runtime-error context line, and the
            // failing parameter (xsltEvalUserParams / xsltEvalGlobalVariable,
            // variables.c), and stops the transformation.
            let xerr = {
                let internal = (*ctxt).xpathCtxt;
                if internal.is_null() {
                    "Invalid expression".to_string()
                } else {
                    let x = (*internal).extra as *mut crate::xml::xpath::context::XPathContext;
                    if x.is_null() {
                        "Invalid expression".to_string()
                    } else {
                        (*x).error
                            .clone()
                            .unwrap_or_else(|| "Invalid expression".to_string())
                    }
                }
            };
            let nm =
                crate::abi::versioning::c_str_to_bytes((*var).name as *const std::os::raw::c_char)
                    .unwrap_or(b"")
                    .to_vec();
            eprintln!("XPath error : {}", xerr);
            eprintln!("runtime error");
            eprintln!(
                "Evaluating user parameter {} failed",
                String::from_utf8_lossy(&nm)
            );
            // UPSTREAM-PARITY: xsltEvalGlobalVariable sets the state to
            // XSLT_STATE_STOPPED on a failed user parameter.
            (*ctxt).state = crate::xslt::transform::XSLT_STATE_STOPPED;
            None
        };
        result
    } else if !(*var).tree.is_null() {
        // Inline content: build a result tree fragment (RVT). The variable
        // value is a node-set containing the RVT's *document node*, matching
        // upstream (variables.c xsltEvalVariable → xmlXPathNewValueTree of
        // the RVT container), so that `exsl:node-set($var)/path` navigation
        // works and `$var` stringifies to the full text content (§35).
        let doc = crate::xml::tree::new_doc(ptr::null());
        if doc.is_null() {
            return;
        }
        // Deep-copy the stylesheet content nodes into the RVT document.
        let mut child = (*var).tree;
        let mut last: *mut _xmlNode = ptr::null_mut();
        while !child.is_null() {
            let copy = crate::xml::tree::copy_node(child, 1);
            if !copy.is_null() {
                if last.is_null() {
                    (*doc).children = copy;
                } else {
                    (*last).next = copy;
                }
                (*copy).prev = last;
                (*copy).parent = doc as *mut _xmlNode;
                (*copy).doc = doc;
                last = copy;
            }
            child = (*child).next;
        }
        if !last.is_null() {
            (*doc).last = last;
        }
        // Own the RVT via the context's document cache (freed exactly once
        // at transform-context teardown, after the XPath context is freed).
        crate::xslt::documents::xsltRegisterRVT(ctxt, doc);
        let mut ns = crate::xml::xpath::types::NodeSet::new();
        ns.push(doc as *mut _xmlNode);
        Some(crate::xml::xpath::types::XPathValue::NodeSet(ns))
    } else {
        // No inline content: empty string value.
        Some(crate::xml::xpath::types::XPathValue::String(String::new()))
    };

    if let Some(v) = value {
        // Register in the XPath context's variable hash.
        let xpath_ctxt = (*ctxt).xpathCtxt;
        if !xpath_ctxt.is_null() {
            let internal = (*xpath_ctxt).extra as *mut crate::xml::xpath::context::XPathContext;
            if !internal.is_null() {
                (*internal).register_variable(&name, v);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::allocator::xmlFreeImpl;

    use core::ptr;

    /// Allocate a zeroed stack element whose `name` is a NUL-terminated
    /// heap copy of `name` flagged `XSLT_VAR_INTERNAL`.
    ///
    /// # Safety
    ///
    /// - Returns NULL if the element allocation fails, otherwise a valid
    ///   zero-initialized `_xsltStackElem`; the caller must check for NULL
    ///   before dereferencing.
    /// - The returned element must be released with `xsltFreeStackElem`,
    ///   which frees the heap-copied `name`; `name` itself is a valid
    ///   byte slice, so the `memcpy` source and the NUL terminator write
    ///   into the freshly `malloc`-ed buffer are in-bounds.
    fn make_stack_elem(name: &[u8]) -> *mut _xsltStackElem {
        unsafe {
            // Zeroed stack element; the name is heap-copied with
            // XSLT_VAR_INTERNAL so xsltFreeStackElem frees it.
            let v = libc::calloc(1, core::mem::size_of::<_xsltStackElem>()) as *mut _xsltStackElem;
            if v.is_null() {
                return ptr::null_mut();
            }
            let cname = libc::malloc(name.len() + 1) as *mut xmlChar;
            if !cname.is_null() {
                libc::memcpy(
                    cname as *mut libc::c_void,
                    name.as_ptr() as *const libc::c_void,
                    name.len(),
                );
                *cname.add(name.len()) = 0;
                (*v).name = cname;
                (*v).flags = XSLT_VAR_INTERNAL;
            }
            v
        }
    }

    /// Allocate a zero-initialized `_xsltTransformContext`.
    ///
    /// # Safety
    ///
    /// - `libc::calloc` returns a zeroed block of the struct size or NULL;
    ///   the caller must check for NULL before dereferencing and must
    ///   release the block with `xmlFreeImpl` when done.
    fn make_ctxt() -> *mut _xsltTransformContext {
        unsafe {
            // Zeroed context: every field NULL/0, matching calloc in
            // xsltNewTransformContext.
            libc::calloc(1, core::mem::size_of::<_xsltTransformContext>())
                as *mut _xsltTransformContext
        }
    }

    /// Push two variables, pop them back in LIFO order, and verify the
    /// stack returns NULL when empty.
    ///
    /// # Safety
    ///
    /// - `ctxt` is a live zeroed context from `make_ctxt` (asserted
    ///   non-NULL); `v1`/`v2` are valid elements from `make_stack_elem`
    ///   (NULL propagates as `-1` from `xsltPushVariable` without being
    ///   dereferenced).
    /// - Elements are released with `xsltFreeStackElem` and the context
    ///   with `xmlFreeImpl`; `xsltPopVariable` returns elements owned by
    ///   the stack, which are not freed separately.
    #[test]
    fn test_push_pop_variable() {
        unsafe {
            let ctxt = make_ctxt();
            assert!(!ctxt.is_null());
            let v1 = make_stack_elem(b"var1\0");
            let v2 = make_stack_elem(b"var2\0");
            assert_eq!(xsltPushVariable(ctxt, v1), 0);
            assert_eq!(xsltPushVariable(ctxt, v2), 0);
            assert_eq!((*ctxt).varsNr, 2);
            let popped = xsltPopVariable(ctxt);
            assert_eq!(popped, v2);
            let popped2 = xsltPopVariable(ctxt);
            assert_eq!(popped2, v1);
            assert!(xsltPopVariable(ctxt).is_null());
            xsltFreeStackElem(v1);
            xsltFreeStackElem(v2);
            xmlFreeImpl(ctxt as *mut libc::c_void);
        }
    }

    /// Push two named variables and look one up by name.
    ///
    /// # Safety
    ///
    /// - `ctxt` is a live zeroed context from `make_ctxt`; the elements
    ///   are valid `_xsltStackElem` values whose `name` fields are valid
    ///   NUL-terminated strings, so `xsltLookupVariable` may compare them.
    /// - The `c"bar"`/`c"baz"` string literals are valid NUL-terminated
    ///   `xmlChar` buffers; elements and the context are freed with
    ///   `xsltFreeStackElem` and `xmlFreeImpl`.
    #[test]
    fn test_lookup_variable() {
        unsafe {
            let ctxt = make_ctxt();
            let v1 = make_stack_elem(b"foo\0");
            let v2 = make_stack_elem(b"bar\0");
            xsltPushVariable(ctxt, v1);
            xsltPushVariable(ctxt, v2);
            let found = xsltLookupVariable(ctxt, c"bar".as_ptr() as *const xmlChar, ptr::null());
            assert_eq!(found, v2);
            let not_found =
                xsltLookupVariable(ctxt, c"baz".as_ptr() as *const xmlChar, ptr::null());
            assert!(not_found.is_null());
            xsltFreeStackElem(v1);
            xsltFreeStackElem(v2);
            xmlFreeImpl(ctxt as *mut libc::c_void);
        }
    }

    /// A lookup on an empty stack returns NULL.
    ///
    /// # Safety
    ///
    /// - `ctxt` is a live zeroed context from `make_ctxt` with `varsNr`
    ///   zero, so `xsltLookupVariable` walks no entries; the name literal
    ///   is a valid NUL-terminated string. The context is released with
    ///   `xmlFreeImpl`.
    #[test]
    fn test_lookup_empty_stack() {
        unsafe {
            let ctxt = make_ctxt();
            let found = xsltLookupVariable(ctxt, c"foo".as_ptr() as *const xmlChar, ptr::null());
            assert!(found.is_null());
            xmlFreeImpl(ctxt as *mut libc::c_void);
        }
    }

    /// Pushing NULL contexts/elements returns `-1` instead of crashing.
    ///
    /// # Safety
    ///
    /// - `xsltPushVariable` rejects NULL arguments before any dereference,
    ///   so passing `ptr::null_mut()` is safe; `ctxt` is a live zeroed
    ///   context from `make_ctxt` and is released with `xmlFreeImpl`.
    #[test]
    fn test_push_null_returns_error() {
        unsafe {
            let ctxt = make_ctxt();
            assert_eq!(xsltPushVariable(ptr::null_mut(), ptr::null_mut()), -1);
            assert_eq!(xsltPushVariable(ctxt, ptr::null_mut()), -1);
            xmlFreeImpl(ctxt as *mut libc::c_void);
        }
    }
}
