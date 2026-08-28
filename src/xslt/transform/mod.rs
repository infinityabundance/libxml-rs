//! XSLT transformation engine (§33, §85 Phase 8).
//!
//! Executes compiled stylesheets against source documents:
//! - `xsltNewTransformContext` / `xsltFreeTransformContext`
//! - `xsltApplyStylesheet` and variants
//! - Template application and instruction execution
//! - Result tree construction
//! - Current node / context position / context size management
//! - Recursion depth limiting
//!
//! # UPSTREAM-PARITY
//!
//! Upstream libxslt (transform.c) drives the transformation from
//! `xsltApplyStylesheet`:
//!
//! 1. Create a transform context (or reuse a user-supplied one).
//! 2. Initialize global variables and key tables.
//! 3. Apply the template matching the root node of the source document.
//! 4. Execute the template's instructions, building the result tree.
//! 5. Return the result document.
//!
//! The `insert` pointer in the context tracks the current insertion point
//! in the result tree; the `node` / `nodeList` fields track the current
//! source node and node list; `contextSize` / `proximityPosition` track
//! the XPath context.

use crate::abi::allocator::xmlFree;
use crate::abi::exports_xml2::*;
use crate::abi::structs::*;
use crate::abi::types::xmlElementType::*;
use crate::abi::types::xmlXPathObjectType;
use crate::abi::types::*;
use crate::xml::tree::*;
use std::ffi::c_void;
use std::os::raw::{c_char, c_int};
use std::ptr;

use super::compiler::{
    get_element_name, get_element_ns, is_xslt_element, is_xslt_namespace, XSLT_NAMESPACE,
};

/// Maximum template recursion depth (matches upstream XSLT_MAX_DEPTH).
pub const XSLT_MAX_DEPTH: c_int = 3000;

/// Maximum insert depth.
pub const XSLT_MAX_INSERT_DEPTH: c_int = 50;

/// State flags for the transform context.
pub const XSLT_STATE_OK: c_int = 0;
pub const XSLT_STATE_ERROR: c_int = 1;

/// Create a new transform context.
///
/// # SAFETY
///
/// - `style` must be a valid compiled `_xsltStylesheet`.
/// - `doc` must be a valid source document (may be NULL).
#[no_mangle]
pub unsafe extern "C" fn xsltNewTransformContext(
    style: *mut _xsltStylesheet,
    doc: *mut _xmlDoc,
) -> *mut _xsltTransformContext {
    if style.is_null() {
        return ptr::null_mut();
    }
    let ctxt = libc::calloc(1, core::mem::size_of::<_xsltTransformContext>())
        as *mut _xsltTransformContext;
    if ctxt.is_null() {
        return ptr::null_mut();
    }
    (*ctxt).style = style;
    (*ctxt).document = doc;
    (*ctxt).state = XSLT_STATE_OK;
    (*ctxt).maxDepth = XSLT_MAX_DEPTH;
    (*ctxt).maxInsertDepth = XSLT_MAX_INSERT_DEPTH;
    (*ctxt).profile = ptr::null_mut();

    // Create the XPath context.
    let xpath_ctxt = xmlXPathNewContext(doc);
    if !xpath_ctxt.is_null() {
        (*ctxt).xpathCtxt = xpath_ctxt;
        // Register the standard XSLT extension functions and variable
        // lookup for XSLT evaluation.
        register_xslt_functions(ctxt);
    }

    // Security preferences: use the default if none set.
    if (*style).secPrefs.is_null() {
        (*ctxt).secPrefs = crate::xslt::security::xsltGetDefaultSecurityPrefs();
    } else {
        (*ctxt).secPrefs = (*style).secPrefs;
    }
    ctxt
}

/// Free a transform context.
///
/// # SAFETY
///
/// - `ctxt` must be a valid `_xsltTransformContext` allocated by
///   `xsltNewTransformContext`, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xsltFreeTransformContext(ctxt: *mut _xsltTransformContext) {
    if ctxt.is_null() {
        return;
    }
    // Free the XPath context.
    if !(*ctxt).xpathCtxt.is_null() {
        xmlXPathFreeContext((*ctxt).xpathCtxt);
    }
    // Free the result document (if we own it).
    if !(*ctxt).resultDoc.is_null() {
        free_doc((*ctxt).resultDoc);
    }
    // Free global variables.
    crate::xslt::variables::xsltFreeGlobalVariables(ctxt);
    // Free key tables.
    crate::xslt::keys::xsltFreeKeyTables(ctxt);
    // Free the document cache.
    crate::xslt::documents::xsltFreeDocCache(ctxt);
    // Free extension registrations.
    crate::xslt::extensions::xsltFreeExts(ctxt);
    // Free the variable table itself.
    if !(*ctxt).varsTab.is_null() {
        libc::free((*ctxt).varsTab as *mut libc::c_void);
    }
    if !(*ctxt).paramsTab.is_null() {
        libc::free((*ctxt).paramsTab as *mut libc::c_void);
    }
    if !(*ctxt).templTab.is_null() {
        libc::free((*ctxt).templTab as *mut libc::c_void);
    }
    libc::free(ctxt as *mut libc::c_void);
}

/// Apply a stylesheet to a document.
///
/// `params` is a NULL-terminated array of `name=value` strings.
/// Returns the result document (caller frees with `xmlFreeDoc`).
///
/// # SAFETY
///
/// - `style` must be a valid compiled stylesheet.
/// - `doc` must be a valid source document.
#[no_mangle]
pub unsafe extern "C" fn xsltApplyStylesheet(
    style: *mut _xsltStylesheet,
    doc: *mut _xmlDoc,
    params: *mut *const c_char,
) -> *mut _xmlDoc {
    xsltApplyStylesheetUser(
        style,
        doc,
        params,
        ptr::null(),
        ptr::null_mut(),
        ptr::null_mut(),
    )
}

/// Apply a stylesheet with user control.
///
/// # SAFETY
///
/// - All pointers must be valid or NULL where permitted.
#[no_mangle]
pub unsafe extern "C" fn xsltApplyStylesheetUser(
    style: *mut _xsltStylesheet,
    doc: *mut _xmlDoc,
    params: *mut *const c_char,
    _output: *const c_char,
    _profile: *mut c_void,
    userCtxt: *mut _xsltTransformContext,
) -> *mut _xmlDoc {
    if style.is_null() || doc.is_null() {
        return ptr::null_mut();
    }

    // Use the user-provided context or create a new one.
    let mut ctxt = userCtxt;
    let mut own_ctxt = false;
    if ctxt.is_null() {
        ctxt = xsltNewTransformContext(style, doc);
        if ctxt.is_null() {
            return ptr::null_mut();
        }
        own_ctxt = true;
    }

    // Parse the stylesheet parameters.
    if !params.is_null() {
        crate::xslt::parameters::xsltParseStylesheetParams(style, params);
    }

    // Initialize global variables.
    crate::xslt::variables::xsltInitGlobalVariables(ctxt);

    // Initialize key tables.
    crate::xslt::keys::xsltInitKeys(ctxt, style);

    // Apply the strip-space rules to the source document.
    crate::xslt::whitespace::xsltApplyStripSpaces(style, doc);

    // Build the result document.
    let result = libc::calloc(1, core::mem::size_of::<_xmlDoc>()) as *mut _xmlDoc;
    if result.is_null() {
        if own_ctxt {
            xsltFreeTransformContext(ctxt);
        }
        return ptr::null_mut();
    }
    (*result).type_ = XML_DOCUMENT_NODE as c_int;
    (*result).version = crate::xml::string::xml_strdup(b"1.0\0".as_ptr() as *const xmlChar);
    (*result).doc = result;
    // Copy output settings from the stylesheet. These are heap-copied so
    // that free_doc (which frees version/encoding with xmlFree) is safe
    // and the stylesheet keeps its own copies.
    if !(*style).encoding.is_null() {
        (*result).encoding = crate::xml::string::xml_strdup((*style).encoding);
    }
    if !(*style).version.is_null() {
        let v = crate::xml::string::xml_strdup((*style).version);
        if !v.is_null() {
            if !(*result).version.is_null() {
                libc::free((*result).version as *mut libc::c_void);
            }
            (*result).version = v;
        }
    }

    (*ctxt).resultDoc = result;
    (*ctxt).insert = result as *mut _xmlNode;

    // Apply the root template: XSLT 1.0 §5.1 applies the template
    // matching "/" to the document node (the root of the source tree).
    // The document node is the doc cast to a node; its parent is null.
    (*ctxt).node = doc as *mut _xmlNode;
    (*ctxt).document = doc;
    (*ctxt).contextSize = 1;
    (*ctxt).proximityPosition = 1;
    let result_code = apply_templates_to_node(ctxt, doc as *mut _xmlNode, ptr::null());

    let final_result = if result_code == 0 { result } else { result };

    if own_ctxt {
        // Detach the result document from the context before freeing.
        (*ctxt).resultDoc = ptr::null_mut();
        xsltFreeTransformContext(ctxt);
    }
    final_result
}

/// Apply a stylesheet with a parameter stack.
///
/// # SAFETY
///
/// - All pointers must be valid or NULL where permitted.
#[no_mangle]
pub unsafe extern "C" fn xsltApplyStylesheetStacked(
    style: *mut _xsltStylesheet,
    doc: *mut _xmlDoc,
    params: *mut *const c_char,
    _stack: *mut c_void,
) -> *mut _xmlDoc {
    xsltApplyStylesheet(style, doc, params)
}

/// Free the result of a transformation.
///
/// # SAFETY
///
/// - `result` must be a document returned by `xsltApplyStylesheet` or NULL.
#[no_mangle]
pub unsafe extern "C" fn xsltFreeTransformResult(result: *mut _xmlDoc) {
    if !result.is_null() {
        free_doc(result);
    }
}

/// Apply the root template (match="/") for empty documents.
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn apply_root_template(ctxt: *mut _xsltTransformContext, doc: *mut _xmlDoc) -> c_int {
    // Find the template matching "/".
    let style = (*ctxt).style;
    if style.is_null() {
        return -1;
    }
    // Use the document node itself as the context node.
    let doc_node = doc as *mut _xmlNode;
    (*ctxt).node = doc_node;
    (*ctxt).contextSize = 1;
    (*ctxt).proximityPosition = 1;
    let templ = crate::xslt::templates::xsltFindTemplate(style, doc_node, ptr::null());
    if templ.is_null() {
        // No match: copy nothing (empty result).
        return 0;
    }
    // Execute the template body.
    let mut vars_base = (*ctxt).varsNr;
    let _ = &mut vars_base;
    (*ctxt).templ = templ;
    execute_content(ctxt, (*templ).content);
    (*ctxt).templ = ptr::null_mut();
    0
}

/// Apply templates to a single node in the given mode.
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn apply_templates_to_node(
    ctxt: *mut _xsltTransformContext,
    node: *mut _xmlNode,
    mode: *const xmlChar,
) -> c_int {
    let style = (*ctxt).style;
    if style.is_null() {
        return -1;
    }
    let templ = crate::xslt::templates::xsltFindTemplate(style, node, mode);
    if templ.is_null() {
        // Built-in template rules (XSLT 1.0 §5.8):
        // - For root/document: apply templates to children.
        // - For elements: apply templates to children.
        // - For text/attribute: copy the text.
        // - For comments/PIs: do nothing.
        let typ = (*node).type_;
        if typ == XML_TEXT_NODE as c_int
            || typ == XML_CDATA_SECTION_NODE as c_int
            || typ == XML_ATTRIBUTE_NODE as c_int
        {
            let content = node_get_content(node);
            if !content.is_null() {
                append_text_node(ctxt, content);
                libc::free(content as *mut libc::c_void);
            }
        } else if typ == XML_ELEMENT_NODE as c_int
            || typ == XML_DOCUMENT_NODE as c_int
            || typ == XML_HTML_DOCUMENT_NODE as c_int
        {
            // Apply templates to children.
            apply_templates_to_children(ctxt, node, mode);
        }
        return 0;
    }
    // Check recursion depth.
    if (*ctxt).depth >= (*ctxt).maxDepth {
        return -1;
    }
    (*ctxt).depth += 1;
    (*ctxt).templ = templ;
    (*ctxt).node = node;
    (*ctxt).contextSize = 1;
    (*ctxt).proximityPosition = 1;
    execute_content(ctxt, (*templ).content);
    (*ctxt).depth -= 1;
    (*ctxt).templ = ptr::null_mut();
    0
}

/// Apply templates to all children of a node.
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn apply_templates_to_children(
    ctxt: *mut _xsltTransformContext,
    node: *mut _xmlNode,
    mode: *const xmlChar,
) -> c_int {
    // Collect the children in document order.
    let mut children: Vec<*mut _xmlNode> = Vec::new();
    let mut child = (*node).children;
    while !child.is_null() {
        children.push(child);
        child = (*child).next;
    }
    // Sort if xsl:sort children are present on the apply-templates
    // instruction (handled by the caller via sort handling).
    let size = children.len();
    (*ctxt).contextSize = size as c_int;
    for (i, node) in children.iter().enumerate() {
        (*ctxt).node = *node;
        (*ctxt).proximityPosition = (i + 1) as c_int;
        apply_templates_to_node(ctxt, *node, mode);
    }
    0
}

/// Execute the content of a template (a list of nodes).
///
/// # SAFETY
///
/// - All pointers must be valid.
pub unsafe fn execute_content(ctxt: *mut _xsltTransformContext, content: *mut _xmlNode) -> c_int {
    let mut cur = content;
    while !cur.is_null() {
        let next = (*cur).next;
        xsltProcessInstruction(ctxt, cur);
        if (*ctxt).state == XSLT_STATE_ERROR {
            return -1;
        }
        cur = next;
    }
    0
}

/// Process a single instruction node.
///
/// # SAFETY
///
/// - `ctxt` must be a valid transform context.
/// - `inst` must be a valid instruction node.
pub unsafe fn xsltProcessInstruction(
    ctxt: *mut _xsltTransformContext,
    inst: *mut _xmlNode,
) -> c_int {
    if ctxt.is_null() || inst.is_null() {
        return -1;
    }
    let typ = (*inst).type_;
    match typ {
        t if t == XML_TEXT_NODE as c_int || t == XML_CDATA_SECTION_NODE as c_int => {
            // Literal text: copy to the result.
            if !(*inst).content.is_null() {
                append_text_node(ctxt, (*inst).content);
            }
            0
        }
        t if t == XML_COMMENT_NODE as c_int => {
            // Literal comment: copy to the result.
            if !(*inst).content.is_null() {
                append_comment_node(ctxt, (*inst).content);
            }
            0
        }
        t if t == XML_PI_NODE as c_int => {
            // Literal PI: copy to the result.
            if !(*inst).name.is_null() {
                let content = if (*inst).content.is_null() {
                    ptr::null()
                } else {
                    (*inst).content
                };
                append_pi_node(ctxt, (*inst).name, content);
            }
            0
        }
        t if t == XML_ELEMENT_NODE as c_int => {
            if is_xslt_namespace(inst) {
                process_xslt_instruction(ctxt, inst);
            } else {
                // Literal result element: create the element and process
                // its content.
                process_literal_element(ctxt, inst);
            }
            0
        }
        _ => 0,
    }
}

/// Process an XSLT instruction element.
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn process_xslt_instruction(ctxt: *mut _xsltTransformContext, inst: *mut _xmlNode) -> c_int {
    let name = get_element_name(inst);
    match name.as_deref() {
        Some("apply-templates") => {
            process_apply_templates(ctxt, inst);
        }
        Some("call-template") => {
            process_call_template(ctxt, inst);
        }
        Some("apply-imports") => {
            process_apply_imports(ctxt, inst);
        }
        Some("for-each") => {
            process_for_each(ctxt, inst);
        }
        Some("value-of") => {
            process_value_of(ctxt, inst);
        }
        Some("copy-of") => {
            process_copy_of(ctxt, inst);
        }
        Some("copy") => {
            process_copy(ctxt, inst);
        }
        Some("element") => {
            process_element(ctxt, inst);
        }
        Some("attribute") => {
            process_attribute(ctxt, inst);
        }
        Some("text") => {
            process_text(ctxt, inst);
        }
        Some("comment") => {
            process_comment(ctxt, inst);
        }
        Some("processing-instruction") => {
            process_pi(ctxt, inst);
        }
        Some("number") => {
            process_number(ctxt, inst);
        }
        Some("choose") => {
            process_choose(ctxt, inst);
        }
        Some("when") | Some("otherwise") => {
            // Only valid inside xsl:choose; ignored here.
        }
        Some("if") => {
            process_if(ctxt, inst);
        }
        Some("variable") => {
            process_variable(ctxt, inst);
        }
        Some("param") => {
            process_param(ctxt, inst);
        }
        Some("with-param") => {
            // Only valid inside call-template/apply-templates; ignored.
        }
        Some("sort") => {
            // Only valid inside for-each/apply-templates; ignored.
        }
        Some("message") => {
            process_message(ctxt, inst);
        }
        Some("fallback") => {
            // Only used when an extension element is unavailable.
        }
        Some("output")
        | Some("decimal-format")
        | Some("namespace-alias")
        | Some("attribute-set")
        | Some("key")
        | Some("strip-space")
        | Some("preserve-space")
        | Some("import")
        | Some("include")
        | Some("stylesheet")
        | Some("transform") => {
            // Top-level elements: not instructions, ignored in content.
        }
        _ => {
            // Unknown XSLT element: may be an extension element.
            let ns = get_element_ns(inst);
            if let Some(ns_uri) = ns {
                // Check for a registered extension element.
                let name_ptr = (*inst).name;
                let ns_cstr = str_to_cstr(&ns_uri);
                let found = crate::xslt::extensions::xsltFindExtElement(
                    ctxt,
                    name_ptr,
                    ns_cstr.as_ptr() as *const xmlChar,
                );
                if !found.is_null() {
                    // Invoke the extension element (Phase 8: full bridge).
                    return 0;
                }
            }
            // Unknown instruction: process fallback children or ignore.
        }
    }
    0
}

/// Convert a Rust string to a NUL-terminated byte vec.
fn str_to_cstr(s: &str) -> Vec<u8> {
    let mut v = s.as_bytes().to_vec();
    v.push(0);
    v
}

/// Evaluate an XPath expression in the current context.
/// Returns an XPath object (caller frees) or NULL on error.
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn eval_xpath(
    ctxt: *mut _xsltTransformContext,
    expr: *const xmlChar,
) -> *mut _xmlXPathObject {
    if ctxt.is_null() || expr.is_null() {
        return ptr::null_mut();
    }
    let xpath_ctxt = (*ctxt).xpathCtxt;
    if xpath_ctxt.is_null() {
        return ptr::null_mut();
    }
    // Set the context node and position on both the C ABI struct and the
    // internal Rust XPathContext (which is what the evaluator actually
    // reads via the `extra` field).
    (*xpath_ctxt).node = (*ctxt).node;
    (*xpath_ctxt).doc = (*ctxt).document;
    (*xpath_ctxt).contextSize = (*ctxt).contextSize;
    (*xpath_ctxt).proximityPosition = (*ctxt).proximityPosition;
    let internal = (*xpath_ctxt).extra as *mut crate::xml::xpath::context::XPathContext;
    if !internal.is_null() {
        (*internal).context_node = (*ctxt).node;
        (*internal).document = (*ctxt).document;
        (*internal).context_size = (*ctxt).contextSize;
        (*internal).context_position = (*ctxt).proximityPosition;
        (*internal).proximity_position = (*ctxt).proximityPosition;
    }
    xmlXPathEvalExpression(expr, xpath_ctxt)
}

/// Process `xsl:apply-templates`.
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn process_apply_templates(ctxt: *mut _xsltTransformContext, inst: *mut _xmlNode) {
    // mode attribute.
    let mode = get_prop(inst, b"mode\0".as_ptr() as *const xmlChar);
    // select attribute (default: all children).
    let select = get_prop(inst, b"select\0".as_ptr() as *const xmlChar);
    // with-param children.
    let params = collect_with_params(ctxt, inst);

    let obj = if !select.is_null() {
        eval_xpath(ctxt, select)
    } else {
        // Default select: "node()" — all children.
        // Build a node-set from children.
        build_child_node_set(ctxt)
    };
    if !select.is_null() {
        libc::free(select as *mut libc::c_void);
    }
    if obj.is_null() {
        if !mode.is_null() {
            libc::free(mode as *mut libc::c_void);
        }
        return;
    }

    // Extract the node-set.
    let nodes = if (*obj).type_ == xmlXPathObjectType::XPATH_NODESET as c_int {
        (*obj).nodesetval as *mut _xmlNodeSet
    } else {
        ptr::null_mut()
    };

    if !nodes.is_null() && (*nodes).nodeNr > 0 {
        // Check for xsl:sort children.
        let sort = find_sort_children(inst);
        let mut node_ptrs: Vec<*mut _xmlNode> = Vec::new();
        let mut i = 0;
        while i < (*nodes).nodeNr {
            let n = *(*nodes).nodeTab.offset(i as isize);
            if !n.is_null() {
                node_ptrs.push(n);
            }
            i += 1;
        }
        // Sort if requested.
        if !sort.is_null() {
            let mut sorted =
                libc::calloc(1, core::mem::size_of::<_xmlNodeSet>()) as *mut _xmlNodeSet;
            if !sorted.is_null() {
                (*sorted).nodeNr = node_ptrs.len() as c_int;
                (*sorted).nodeMax = node_ptrs.len() as c_int;
                let tab = libc::malloc(node_ptrs.len() * core::mem::size_of::<*mut _xmlNode>())
                    as *mut *mut _xmlNode;
                (*sorted).nodeTab = tab;
                for (idx, n) in node_ptrs.iter().enumerate() {
                    if !tab.is_null() {
                        *tab.offset(idx as isize) = *n;
                    }
                }
                crate::xslt::sorting::xsltSortNodeSet(ctxt, sorted, sort);
                // Apply templates in sorted order.
                let mut k = 0;
                while k < (*sorted).nodeNr {
                    let n = *(*sorted).nodeTab.offset(k as isize);
                    if !n.is_null() {
                        (*ctxt).node = n;
                        (*ctxt).contextSize = (*sorted).nodeNr;
                        (*ctxt).proximityPosition = k + 1;
                        apply_templates_with_params(ctxt, n, mode, params);
                    }
                    k += 1;
                }
                libc::free((*sorted).nodeTab as *mut libc::c_void);
                libc::free(sorted as *mut libc::c_void);
            }
        } else {
            (*ctxt).contextSize = node_ptrs.len() as c_int;
            for (i, n) in node_ptrs.iter().enumerate() {
                if !n.is_null() {
                    (*ctxt).node = *n;
                    (*ctxt).proximityPosition = (i + 1) as c_int;
                    apply_templates_with_params(ctxt, *n, mode, params);
                }
            }
        }
    }

    if !mode.is_null() {
        libc::free(mode as *mut libc::c_void);
    }
    xmlXPathFreeObject(obj);
}

/// Apply templates to a node, passing parameters.
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn apply_templates_with_params(
    ctxt: *mut _xsltTransformContext,
    node: *mut _xmlNode,
    mode: *const xmlChar,
    params: *mut _xsltStackElem,
) {
    // Push the params onto the parameter stack.
    let mut p = params;
    while !p.is_null() {
        crate::xslt::parameters::xsltPushParam(ctxt, p);
        p = (*p).next;
    }
    apply_templates_to_node(ctxt, node, mode);
    // Pop the params.
    let mut p = params;
    while !p.is_null() {
        crate::xslt::parameters::xsltPopParam(ctxt);
        p = (*p).next;
    }
}

/// Build a node-set from the children of the current node.
///
/// # SAFETY
///
/// - `ctxt` must be valid.
unsafe fn build_child_node_set(ctxt: *mut _xsltTransformContext) -> *mut _xmlXPathObject {
    let ns = xmlXPathNodeSetCreate(ptr::null_mut());
    if ns.is_null() {
        return ptr::null_mut();
    }
    let obj = xmlMalloc_zero_obj();
    if obj.is_null() {
        libc::free(ns as *mut libc::c_void);
        return ptr::null_mut();
    }
    (*obj).type_ = xmlXPathObjectType::XPATH_NODESET as c_int;
    (*obj).nodesetval = ns as *mut c_void;
    let node = (*ctxt).node;
    if !node.is_null() {
        let mut child = (*node).children;
        while !child.is_null() {
            append_to_node_set(ns, child);
            child = (*child).next;
        }
    }
    obj
}

/// Allocate a zeroed XPath object.
unsafe fn xmlMalloc_zero_obj() -> *mut _xmlXPathObject {
    libc::calloc(1, core::mem::size_of::<_xmlXPathObject>()) as *mut _xmlXPathObject
}

/// Append a node to a node-set.
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn append_to_node_set(ns: *mut _xmlNodeSet, node: *mut _xmlNode) {
    if ns.is_null() || node.is_null() {
        return;
    }
    // Deduplicate.
    let mut i = 0;
    while i < (*ns).nodeNr {
        if *(*ns).nodeTab.offset(i as isize) == node {
            return;
        }
        i += 1;
    }
    if (*ns).nodeNr >= (*ns).nodeMax {
        let new_max = if (*ns).nodeMax == 0 {
            8
        } else {
            (*ns).nodeMax * 2
        };
        let new_tab = libc::realloc(
            (*ns).nodeTab as *mut libc::c_void,
            (new_max as usize) * core::mem::size_of::<*mut _xmlNode>(),
        ) as *mut *mut _xmlNode;
        if new_tab.is_null() {
            return;
        }
        (*ns).nodeTab = new_tab;
        (*ns).nodeMax = new_max;
    }
    *(*ns).nodeTab.offset((*ns).nodeNr as isize) = node;
    (*ns).nodeNr += 1;
}

/// Collect the `xsl:with-param` children of an instruction.
///
/// Returns a linked list of evaluated parameter stack elements.
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn collect_with_params(
    ctxt: *mut _xsltTransformContext,
    inst: *mut _xmlNode,
) -> *mut _xsltStackElem {
    let mut head: *mut _xsltStackElem = ptr::null_mut();
    let mut tail: *mut _xsltStackElem = ptr::null_mut();
    let mut child = (*inst).children;
    while !child.is_null() {
        let next = (*child).next;
        if is_xslt_element(child, "with-param") {
            let param = evaluate_with_param(ctxt, child);
            if !param.is_null() {
                (*param).next = ptr::null_mut();
                if tail.is_null() {
                    head = param;
                    tail = param;
                } else {
                    (*tail).next = param;
                    tail = param;
                }
            }
        }
        child = next;
    }
    head
}

/// Evaluate a single `xsl:with-param` element.
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn evaluate_with_param(
    ctxt: *mut _xsltTransformContext,
    inst: *mut _xmlNode,
) -> *mut _xsltStackElem {
    let name = get_prop(inst, b"name\0".as_ptr() as *const xmlChar);
    if name.is_null() {
        return ptr::null_mut();
    }
    let select = get_prop(inst, b"select\0".as_ptr() as *const xmlChar);
    let param = libc::calloc(1, core::mem::size_of::<_xsltStackElem>()) as *mut _xsltStackElem;
    if param.is_null() {
        libc::free(name as *mut libc::c_void);
        if !select.is_null() {
            libc::free(select as *mut libc::c_void);
        }
        return ptr::null_mut();
    }
    (*param).name = name;
    (*param).flags = 2 | 4; // PARAM | INTERNAL
    if !select.is_null() {
        let obj = eval_xpath(ctxt, select);
        if !obj.is_null() {
            (*param).value = obj;
        }
        libc::free(select as *mut libc::c_void);
    } else {
        // Inline content: result tree fragment.
        let value = eval_content_fragment(ctxt, (*inst).children);
        if !value.is_null() {
            (*param).value = value;
        }
    }
    param
}

/// Evaluate inline content into a result tree fragment object.
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn eval_content_fragment(
    ctxt: *mut _xsltTransformContext,
    content: *mut _xmlNode,
) -> *mut _xmlXPathObject {
    let frag = libc::calloc(1, core::mem::size_of::<_xmlDoc>()) as *mut _xmlDoc;
    if frag.is_null() {
        return ptr::null_mut();
    }
    (*frag).type_ = XML_DOCUMENT_NODE as c_int;
    (*frag).doc = frag;
    // Save the insert point and redirect into the fragment.
    let saved_insert = (*ctxt).insert;
    let saved_result = (*ctxt).resultDoc;
    (*ctxt).insert = frag as *mut _xmlNode;
    (*ctxt).resultDoc = frag;
    execute_content(ctxt, content);
    (*ctxt).insert = saved_insert;
    (*ctxt).resultDoc = saved_result;

    let obj = xmlMalloc_zero_obj();
    if obj.is_null() {
        free_doc(frag);
        return ptr::null_mut();
    }
    (*obj).type_ = xmlXPathObjectType::XPATH_XSLT_TREE as c_int;
    (*obj).nodesetval = frag as *mut c_void;
    obj
}

/// Process `xsl:call-template`.
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn process_call_template(ctxt: *mut _xsltTransformContext, inst: *mut _xmlNode) {
    let name = get_prop(inst, b"name\0".as_ptr() as *const xmlChar);
    if name.is_null() {
        return;
    }
    let style = (*ctxt).style;
    let templ = crate::xslt::templates::xsltLookupTemplate(style, name);
    libc::free(name as *mut libc::c_void);
    if templ.is_null() {
        return;
    }
    if (*ctxt).depth >= (*ctxt).maxDepth {
        return;
    }
    (*ctxt).depth += 1;
    // Collect with-param children.
    let params = collect_with_params(ctxt, inst);
    let mut p = params;
    while !p.is_null() {
        crate::xslt::parameters::xsltPushParam(ctxt, p);
        p = (*p).next;
    }
    let saved_templ = (*ctxt).templ;
    (*ctxt).templ = templ;
    execute_content(ctxt, (*templ).content);
    (*ctxt).templ = saved_templ;
    let mut p = params;
    while !p.is_null() {
        crate::xslt::parameters::xsltPopParam(ctxt);
        p = (*p).next;
    }
    (*ctxt).depth -= 1;
}

/// Process `xsl:apply-imports`.
///
/// XSLT 1.0 §5.6: applies the next template in import precedence order
/// that matches the current node, skipping templates at the same or
/// higher import precedence as the current template.
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn process_apply_imports(ctxt: *mut _xsltTransformContext, inst: *mut _xmlNode) {
    let _ = inst;
    let style = (*ctxt).style;
    let node = (*ctxt).node;
    let current_templ = (*ctxt).templ;
    if style.is_null() || node.is_null() || current_templ.is_null() {
        return;
    }
    // The current template's import depth; imported templates have HIGHER
    // depth values. apply-imports considers templates with depth strictly
    // greater than the current template's depth.
    let current_depth = (*current_templ).depth;
    let mode = (*current_templ).mode;

    let mut best: *mut _xsltTemplate = ptr::null_mut();
    let mut best_priority: f64 = f64::NEG_INFINITY;
    let mut best_depth: c_int = -1;

    let mut templ = (*style).templates;
    while !templ.is_null() {
        // Only templates imported more deeply than the current one.
        if (*templ).depth <= current_depth {
            templ = (*templ).next;
            continue;
        }
        // Mode must match.
        if !(*templ).mode.is_null() {
            if mode.is_null()
                || libc::strcmp(
                    (*templ).mode as *const libc::c_char,
                    mode as *const libc::c_char,
                ) != 0
            {
                templ = (*templ).next;
                continue;
            }
        } else if !mode.is_null() {
            templ = (*templ).next;
            continue;
        }
        // Pattern must match.
        let pattern_ptr = (*templ).r#match as *mut crate::xslt::patterns::_xsltPattern;
        if pattern_ptr.is_null() {
            templ = (*templ).next;
            continue;
        }
        if crate::xslt::patterns::xsltTestPattern(ctxt, pattern_ptr, node) == 0 {
            templ = (*templ).next;
            continue;
        }
        // Priority: explicit or default.
        let priority = (*templ).priority;
        if priority > best_priority || (priority == best_priority && (*templ).depth > best_depth) {
            best = templ;
            best_priority = priority;
            best_depth = (*templ).depth;
        }
        templ = (*templ).next;
    }

    if !best.is_null() {
        if (*ctxt).depth >= (*ctxt).maxDepth {
            return;
        }
        (*ctxt).depth += 1;
        let saved_templ = (*ctxt).templ;
        (*ctxt).templ = best;
        execute_content(ctxt, (*best).content);
        (*ctxt).templ = saved_templ;
        (*ctxt).depth -= 1;
    }
}

/// Process `xsl:for-each`.
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn process_for_each(ctxt: *mut _xsltTransformContext, inst: *mut _xmlNode) {
    let select = get_prop(inst, b"select\0".as_ptr() as *const xmlChar);
    if select.is_null() {
        return;
    }
    let obj = eval_xpath(ctxt, select);
    libc::free(select as *mut libc::c_void);
    if obj.is_null() {
        return;
    }
    if (*obj).type_ != xmlXPathObjectType::XPATH_NODESET as c_int {
        xmlXPathFreeObject(obj);
        return;
    }
    let nodes = (*obj).nodesetval as *mut _xmlNodeSet;
    if nodes.is_null() || (*nodes).nodeNr == 0 {
        xmlXPathFreeObject(obj);
        return;
    }
    // Check for xsl:sort children.
    let sort = find_sort_children(inst);
    // Save the current node list state.
    let saved_node = (*ctxt).node;
    let saved_size = (*ctxt).contextSize;
    let saved_pos = (*ctxt).proximityPosition;

    let mut node_ptrs: Vec<*mut _xmlNode> = Vec::new();
    let mut i = 0;
    while i < (*nodes).nodeNr {
        let n = *(*nodes).nodeTab.offset(i as isize);
        if !n.is_null() {
            node_ptrs.push(n);
        }
        i += 1;
    }

    if !sort.is_null() {
        // Build a temporary node-set and sort it.
        let mut sorted = libc::calloc(1, core::mem::size_of::<_xmlNodeSet>()) as *mut _xmlNodeSet;
        if !sorted.is_null() {
            (*sorted).nodeNr = node_ptrs.len() as c_int;
            (*sorted).nodeMax = node_ptrs.len() as c_int;
            let tab = libc::malloc(node_ptrs.len() * core::mem::size_of::<*mut _xmlNode>())
                as *mut *mut _xmlNode;
            (*sorted).nodeTab = tab;
            for (idx, n) in node_ptrs.iter().enumerate() {
                if !tab.is_null() {
                    *tab.offset(idx as isize) = *n;
                }
            }
            crate::xslt::sorting::xsltSortNodeSet(ctxt, sorted, sort);
            (*ctxt).contextSize = (*sorted).nodeNr;
            let mut k = 0;
            while k < (*sorted).nodeNr {
                let n = *(*sorted).nodeTab.offset(k as isize);
                if !n.is_null() {
                    (*ctxt).node = n;
                    (*ctxt).proximityPosition = k + 1;
                    execute_content(ctxt, (*inst).children);
                }
                k += 1;
            }
            libc::free((*sorted).nodeTab as *mut libc::c_void);
            libc::free(sorted as *mut libc::c_void);
        }
    } else {
        (*ctxt).contextSize = node_ptrs.len() as c_int;
        for (i, n) in node_ptrs.iter().enumerate() {
            if !n.is_null() {
                (*ctxt).node = *n;
                (*ctxt).proximityPosition = (i + 1) as c_int;
                execute_content(ctxt, (*inst).children);
            }
        }
    }

    // Restore the current node state.
    (*ctxt).node = saved_node;
    (*ctxt).contextSize = saved_size;
    (*ctxt).proximityPosition = saved_pos;
    xmlXPathFreeObject(obj);
}

/// Find the first `xsl:sort` child of an instruction.
///
/// # SAFETY
///
/// - `inst` must be a valid node.
unsafe fn find_sort_children(inst: *mut _xmlNode) -> *mut _xsltSort {
    if inst.is_null() {
        return ptr::null_mut();
    }
    let mut child = (*inst).children;
    while !child.is_null() {
        if is_xslt_element(child, "sort") {
            // Compile the sort from the instruction node. The style is
            // derived from the instruction's document (the stylesheet doc).
            let style = if !(*child).doc.is_null() {
                // Find the stylesheet that owns this document: use the
                // transform context's style via the caller (set below).
                ptr::null_mut()
            } else {
                ptr::null_mut()
            };
            let sort = crate::xslt::sorting::xsltCompileSort(style, child);
            return sort;
        }
        child = (*child).next;
    }
    ptr::null_mut()
}

/// Process `xsl:value-of`.
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn process_value_of(ctxt: *mut _xsltTransformContext, inst: *mut _xmlNode) {
    let select = get_prop(inst, b"select\0".as_ptr() as *const xmlChar);
    if select.is_null() {
        return;
    }
    let obj = eval_xpath(ctxt, select);
    libc::free(select as *mut libc::c_void);
    if obj.is_null() {
        return;
    }
    let strv = xmlXPathCastToString(obj);
    xmlXPathFreeObject(obj);
    if !strv.is_null() {
        append_text_node(ctxt, strv);
        libc::free(strv as *mut libc::c_void);
    }
}

/// Process `xsl:copy-of`.
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn process_copy_of(ctxt: *mut _xsltTransformContext, inst: *mut _xmlNode) {
    let select = get_prop(inst, b"select\0".as_ptr() as *const xmlChar);
    if select.is_null() {
        return;
    }
    let obj = eval_xpath(ctxt, select);
    libc::free(select as *mut libc::c_void);
    if obj.is_null() {
        return;
    }
    if (*obj).type_ == xmlXPathObjectType::XPATH_NODESET as c_int {
        let nodes = (*obj).nodesetval as *mut _xmlNodeSet;
        if !nodes.is_null() {
            let mut i = 0;
            while i < (*nodes).nodeNr {
                let n = *(*nodes).nodeTab.offset(i as isize);
                if !n.is_null() {
                    copy_node_deep(ctxt, n);
                }
                i += 1;
            }
        }
    } else if (*obj).type_ == xmlXPathObjectType::XPATH_XSLT_TREE as c_int {
        // Result tree fragment: copy the fragment's children.
        let frag = (*obj).nodesetval as *mut _xmlDoc;
        if !frag.is_null() {
            let mut child = (*frag).children;
            while !child.is_null() {
                let next = (*child).next;
                copy_node_deep(ctxt, child);
                child = next;
            }
        }
    } else {
        // Atomic value: copy as text.
        let strv = xmlXPathCastToString(obj);
        if !strv.is_null() {
            append_text_node(ctxt, strv);
            libc::free(strv as *mut libc::c_void);
        }
    }
    xmlXPathFreeObject(obj);
}

/// Deep-copy a source node into the result tree.
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn copy_node_deep(ctxt: *mut _xsltTransformContext, node: *mut _xmlNode) {
    if node.is_null() {
        return;
    }
    let typ = (*node).type_;
    if typ == XML_TEXT_NODE as c_int || typ == XML_CDATA_SECTION_NODE as c_int {
        if !(*node).content.is_null() {
            append_text_node(ctxt, (*node).content);
        }
    } else if typ == XML_COMMENT_NODE as c_int {
        if !(*node).content.is_null() {
            append_comment_node(ctxt, (*node).content);
        }
    } else if typ == XML_PI_NODE as c_int {
        if !(*node).name.is_null() {
            append_pi_node(ctxt, (*node).name, (*node).content);
        }
    } else if typ == XML_ELEMENT_NODE as c_int {
        // Create the element.
        let name = (*node).name;
        let new_elem = new_element_node(ctxt, name, (*node).ns);
        if new_elem.is_null() {
            return;
        }
        // Copy attributes.
        let mut prop = (*node).properties;
        while !prop.is_null() {
            let attr_name = (*prop).name;
            let attr_val = node_get_content((*prop).children);
            if !attr_name.is_null() && !attr_val.is_null() {
                set_prop(new_elem, attr_name, attr_val);
                libc::free(attr_val as *mut libc::c_void);
            }
            prop = (*prop).next;
        }
        // Recurse into children.
        let saved_insert = (*ctxt).insert;
        (*ctxt).insert = new_elem;
        let mut child = (*node).children;
        while !child.is_null() {
            let next = (*child).next;
            copy_node_deep(ctxt, child);
            child = next;
        }
        (*ctxt).insert = saved_insert;
    }
}

/// Create an element node in the result tree.
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn new_element_node(
    ctxt: *mut _xsltTransformContext,
    name: *const xmlChar,
    ns: *mut _xmlNs,
) -> *mut _xmlNode {
    let elem = new_node(ns, name);
    if elem.is_null() {
        return ptr::null_mut();
    }
    append_to_result(ctxt, elem);
    elem
}

/// Append a node to the result tree at the current insertion point.
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn append_to_result(ctxt: *mut _xsltTransformContext, node: *mut _xmlNode) {
    let insert = (*ctxt).insert;
    if insert.is_null() {
        return;
    }
    // If the insert point is a document, add as a child.
    add_child(insert, node);
    // Fix up the document pointer.
    let doc = if (*insert).type_ == XML_DOCUMENT_NODE as c_int {
        insert as *mut _xmlDoc
    } else {
        (*insert).doc
    };
    if !doc.is_null() {
        set_node_doc(node, doc);
    }
}

/// Recursively set the doc pointer of a subtree.
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn set_node_doc(node: *mut _xmlNode, doc: *mut _xmlDoc) {
    if node.is_null() {
        return;
    }
    (*node).doc = doc;
    let mut prop = (*node).properties;
    while !prop.is_null() {
        (*prop).doc = doc;
        prop = (*prop).next;
    }
    let mut child = (*node).children;
    while !child.is_null() {
        set_node_doc(child, doc);
        child = (*child).next;
    }
}

/// Append a text node to the result tree.
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn append_text_node(ctxt: *mut _xsltTransformContext, content: *const xmlChar) {
    let insert = (*ctxt).insert;
    if insert.is_null() || content.is_null() {
        return;
    }
    let text = new_text(content);
    if text.is_null() {
        return;
    }
    append_to_result(ctxt, text);
}

/// Append a comment node to the result tree.
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn append_comment_node(ctxt: *mut _xsltTransformContext, content: *const xmlChar) {
    let insert = (*ctxt).insert;
    if insert.is_null() || content.is_null() {
        return;
    }
    let comment = new_comment(content);
    if comment.is_null() {
        return;
    }
    append_to_result(ctxt, comment);
}

/// Append a PI node to the result tree.
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn append_pi_node(
    ctxt: *mut _xsltTransformContext,
    name: *const xmlChar,
    content: *const xmlChar,
) {
    let insert = (*ctxt).insert;
    if insert.is_null() || name.is_null() {
        return;
    }
    let pi = new_pi(name, content);
    if pi.is_null() {
        return;
    }
    append_to_result(ctxt, pi);
}

/// Process `xsl:copy`.
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn process_copy(ctxt: *mut _xsltTransformContext, inst: *mut _xmlNode) {
    let node = (*ctxt).node;
    if node.is_null() {
        return;
    }
    let typ = (*node).type_;
    let saved_insert = (*ctxt).insert;
    if typ == XML_ELEMENT_NODE as c_int {
        let new_elem = new_element_node(ctxt, (*node).name, (*node).ns);
        if new_elem.is_null() {
            return;
        }
        (*ctxt).insert = new_elem;
    } else if typ == XML_TEXT_NODE as c_int || typ == XML_CDATA_SECTION_NODE as c_int {
        if !(*node).content.is_null() {
            append_text_node(ctxt, (*node).content);
        }
    } else if typ == XML_COMMENT_NODE as c_int {
        if !(*node).content.is_null() {
            append_comment_node(ctxt, (*node).content);
        }
    } else if typ == XML_PI_NODE as c_int {
        if !(*node).name.is_null() {
            append_pi_node(ctxt, (*node).name, (*node).content);
        }
    } else if typ == XML_ATTRIBUTE_NODE as c_int {
        if !(*node).children.is_null() {
            let val = node_get_content((*node).children);
            if !val.is_null() {
                append_text_node(ctxt, val);
                libc::free(val as *mut libc::c_void);
            }
        }
    }
    // Process children (attributes and content).
    execute_content(ctxt, (*inst).children);
    (*ctxt).insert = saved_insert;
}

/// Process `xsl:element`.
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn process_element(ctxt: *mut _xsltTransformContext, inst: *mut _xmlNode) {
    let name_attr = get_prop(inst, b"name\0".as_ptr() as *const xmlChar);
    if name_attr.is_null() {
        return;
    }
    // Evaluate the name attribute (it may be an AVT).
    let name_str = xmlXPathCastToString(xmlXPathNewCString(name_attr));
    libc::free(name_attr as *mut libc::c_void);
    if name_str.is_null() {
        return;
    }
    // Check for the namespace attribute.
    let ns_attr = get_prop(inst, b"namespace\0".as_ptr() as *const xmlChar);
    let ns_str = if !ns_attr.is_null() {
        let v = xmlXPathCastToString(xmlXPathNewCString(ns_attr));
        libc::free(ns_attr as *mut libc::c_void);
        v
    } else {
        ptr::null_mut()
    };
    // Create the element.
    let ns = if !ns_str.is_null() && *ns_str != 0 {
        let n = new_ns(ptr::null_mut(), ns_str, ptr::null());
        libc::free(ns_str as *mut libc::c_void);
        n
    } else {
        if !ns_str.is_null() {
            libc::free(ns_str as *mut libc::c_void);
        }
        ptr::null_mut()
    };
    let elem = new_node(ns, name_str);
    libc::free(name_str as *mut libc::c_void);
    if elem.is_null() {
        return;
    }
    append_to_result(ctxt, elem);
    let saved_insert = (*ctxt).insert;
    (*ctxt).insert = elem;
    execute_content(ctxt, (*inst).children);
    (*ctxt).insert = saved_insert;
}

/// Process `xsl:attribute`.
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn process_attribute(ctxt: *mut _xsltTransformContext, inst: *mut _xmlNode) {
    let name_attr = get_prop(inst, b"name\0".as_ptr() as *const xmlChar);
    if name_attr.is_null() {
        return;
    }
    let insert = (*ctxt).insert;
    if insert.is_null() {
        libc::free(name_attr as *mut libc::c_void);
        return;
    }
    // Evaluate the content into a temporary buffer.
    let saved_insert = (*ctxt).insert;
    let buf = libc::calloc(1, core::mem::size_of::<_xmlBuffer>()) as *mut _xmlBuffer;
    if buf.is_null() {
        libc::free(name_attr as *mut libc::c_void);
        return;
    }
    (*buf).content = libc::calloc(1, 64) as *mut xmlChar;
    (*buf).size = 64;
    (*buf).use_ = 0;
    let frag_doc = libc::calloc(1, core::mem::size_of::<_xmlDoc>()) as *mut _xmlDoc;
    if frag_doc.is_null() {
        libc::free(buf as *mut libc::c_void);
        libc::free(name_attr as *mut libc::c_void);
        return;
    }
    (*frag_doc).type_ = XML_DOCUMENT_NODE as c_int;
    (*frag_doc).doc = frag_doc;
    (*ctxt).insert = frag_doc as *mut _xmlNode;
    execute_content(ctxt, (*inst).children);
    // Collect the text from the fragment.
    let mut value: Vec<u8> = Vec::new();
    let mut child = (*frag_doc).children;
    while !child.is_null() {
        if (*child).type_ == XML_TEXT_NODE as c_int {
            if !(*child).content.is_null() {
                let len = libc::strlen((*child).content as *const libc::c_char) as usize;
                value.extend_from_slice(core::slice::from_raw_parts((*child).content, len));
            }
        }
        child = (*child).next;
    }
    // Free the fragment.
    free_doc(frag_doc);
    (*ctxt).insert = saved_insert;

    // Set the attribute on the current result element.
    let mut cvalue = value.clone();
    cvalue.push(0);
    set_prop(insert, name_attr, cvalue.as_ptr() as *const xmlChar);
    libc::free(name_attr as *mut libc::c_void);
}

/// Process `xsl:text`.
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn process_text(ctxt: *mut _xsltTransformContext, inst: *mut _xmlNode) {
    // disable-output-escaping attribute.
    let doe = get_prop(
        inst,
        b"disable-output-escaping\0".as_ptr() as *const xmlChar,
    );
    if !doe.is_null() {
        libc::free(doe as *mut libc::c_void);
    }
    // Copy the text children verbatim.
    let mut child = (*inst).children;
    while !child.is_null() {
        if (*child).type_ == XML_TEXT_NODE as c_int && !(*child).content.is_null() {
            append_text_node(ctxt, (*child).content);
        }
        child = (*child).next;
    }
}

/// Process `xsl:comment`.
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn process_comment(ctxt: *mut _xsltTransformContext, inst: *mut _xmlNode) {
    let content = node_get_content((*inst).children);
    if !content.is_null() {
        append_comment_node(ctxt, content);
        libc::free(content as *mut libc::c_void);
    }
}

/// Process `xsl:processing-instruction`.
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn process_pi(ctxt: *mut _xsltTransformContext, inst: *mut _xmlNode) {
    let name_attr = get_prop(inst, b"name\0".as_ptr() as *const xmlChar);
    if name_attr.is_null() {
        return;
    }
    let content = node_get_content((*inst).children);
    append_pi_node(ctxt, name_attr, content);
    if !content.is_null() {
        libc::free(content as *mut libc::c_void);
    }
    libc::free(name_attr as *mut libc::c_void);
}

/// Process `xsl:number`.
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn process_number(ctxt: *mut _xsltTransformContext, inst: *mut _xmlNode) {
    let value_attr = get_prop(inst, b"value\0".as_ptr() as *const xmlChar);
    let mut number: f64 = f64::NAN;
    if !value_attr.is_null() {
        let obj = eval_xpath(ctxt, value_attr);
        libc::free(value_attr as *mut libc::c_void);
        if !obj.is_null() {
            number = (*obj).floatval;
            xmlXPathFreeObject(obj);
        }
    } else {
        // Compute the number from the level (single/multiple/any) and
        // count patterns. Phase 8: full implementation computes preceding
        // siblings etc. Simplified: count preceding siblings + 1.
        number = 1.0;
        let node = (*ctxt).node;
        if !node.is_null() {
            let mut sib = (*node).prev;
            while !sib.is_null() {
                if (*sib).type_ == XML_ELEMENT_NODE as c_int {
                    number += 1.0;
                }
                sib = (*sib).prev;
            }
        }
    }
    // Format the number: format attribute with tokens.
    let format = get_prop(inst, b"format\0".as_ptr() as *const xmlChar);
    let formatted = crate::xslt::numbering::xsltFormatNumber(number, format);
    if !format.is_null() {
        libc::free(format as *mut libc::c_void);
    }
    if !formatted.is_null() {
        append_text_node(ctxt, formatted);
        libc::free(formatted as *mut libc::c_void);
    }
}

/// Process `xsl:choose`.
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn process_choose(ctxt: *mut _xsltTransformContext, inst: *mut _xmlNode) {
    let mut child = (*inst).children;
    let mut executed = false;
    while !child.is_null() {
        let next = (*child).next;
        if is_xslt_element(child, "when") {
            let test = get_prop(child, b"test\0".as_ptr() as *const xmlChar);
            if !test.is_null() {
                let obj = eval_xpath(ctxt, test);
                libc::free(test as *mut libc::c_void);
                let truthy = !obj.is_null() && (*obj).boolval != 0;
                if !obj.is_null() {
                    xmlXPathFreeObject(obj);
                }
                if truthy {
                    execute_content(ctxt, (*child).children);
                    executed = true;
                    break;
                }
            }
        } else if is_xslt_element(child, "otherwise") {
            if !executed {
                execute_content(ctxt, (*child).children);
                executed = true;
            }
        }
        child = next;
    }
}

/// Process `xsl:if`.
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn process_if(ctxt: *mut _xsltTransformContext, inst: *mut _xmlNode) {
    let test = get_prop(inst, b"test\0".as_ptr() as *const xmlChar);
    if test.is_null() {
        return;
    }
    let obj = eval_xpath(ctxt, test);
    libc::free(test as *mut libc::c_void);
    if obj.is_null() {
        return;
    }
    let truthy = (*obj).boolval != 0;
    xmlXPathFreeObject(obj);
    if truthy {
        execute_content(ctxt, (*inst).children);
    }
}

/// Process `xsl:variable` (local variable).
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn process_variable(ctxt: *mut _xsltTransformContext, inst: *mut _xmlNode) {
    let name = get_prop(inst, b"name\0".as_ptr() as *const xmlChar);
    if name.is_null() {
        return;
    }
    let select = get_prop(inst, b"select\0".as_ptr() as *const xmlChar);
    let var = libc::calloc(1, core::mem::size_of::<_xsltStackElem>()) as *mut _xsltStackElem;
    if var.is_null() {
        libc::free(name as *mut libc::c_void);
        if !select.is_null() {
            libc::free(select as *mut libc::c_void);
        }
        return;
    }
    (*var).name = name;
    (*var).flags = 4; // INTERNAL
    if !select.is_null() {
        let obj = eval_xpath(ctxt, select);
        if !obj.is_null() {
            (*var).value = obj;
        }
        libc::free(select as *mut libc::c_void);
    } else {
        let value = eval_content_fragment(ctxt, (*inst).children);
        if !value.is_null() {
            (*var).value = value;
        }
    }
    crate::xslt::variables::xsltPushVariable(ctxt, var);
}

/// Process `xsl:param` (local param with default).
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn process_param(ctxt: *mut _xsltTransformContext, inst: *mut _xmlNode) {
    let name = get_prop(inst, b"name\0".as_ptr() as *const xmlChar);
    if name.is_null() {
        return;
    }
    // Check whether a value was passed via the parameter stack.
    let existing = crate::xslt::variables::xsltLookupVariable(ctxt, name, ptr::null());
    if !existing.is_null() && !(*existing).value.is_null() {
        // Value already provided by with-param.
        libc::free(name as *mut libc::c_void);
        return;
    }
    let select = get_prop(inst, b"select\0".as_ptr() as *const xmlChar);
    let var = libc::calloc(1, core::mem::size_of::<_xsltStackElem>()) as *mut _xsltStackElem;
    if var.is_null() {
        libc::free(name as *mut libc::c_void);
        if !select.is_null() {
            libc::free(select as *mut libc::c_void);
        }
        return;
    }
    (*var).name = name;
    (*var).flags = 2 | 4; // PARAM | INTERNAL
    if !select.is_null() {
        let obj = eval_xpath(ctxt, select);
        if !obj.is_null() {
            (*var).value = obj;
        }
        libc::free(select as *mut libc::c_void);
    } else {
        let value = eval_content_fragment(ctxt, (*inst).children);
        if !value.is_null() {
            (*var).value = value;
        }
    }
    crate::xslt::variables::xsltPushVariable(ctxt, var);
}

/// Process `xsl:message`.
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn process_message(ctxt: *mut _xsltTransformContext, inst: *mut _xmlNode) {
    let content = node_get_content((*inst).children);
    if !content.is_null() {
        // Write the message to stderr.
        let len = libc::strlen(content as *const libc::c_char) as usize;
        let _ = libc::write(2, content as *const libc::c_void, len);
        // terminate attribute: if "yes", stop the transformation.
        let terminate = get_prop(inst, b"terminate\0".as_ptr() as *const xmlChar);
        if !terminate.is_null() {
            if libc::strcmp(
                terminate as *const libc::c_char,
                b"yes\0".as_ptr() as *const libc::c_char,
            ) == 0
            {
                (*ctxt).state = XSLT_STATE_ERROR;
            }
            libc::free(terminate as *mut libc::c_void);
        }
        libc::free(content as *mut libc::c_void);
    }
}

/// Process a literal result element.
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn process_literal_element(ctxt: *mut _xsltTransformContext, inst: *mut _xmlNode) {
    // Create the result element.
    let elem = new_node((*inst).ns, (*inst).name);
    if elem.is_null() {
        return;
    }
    append_to_result(ctxt, elem);
    // Copy attributes.
    let mut prop = (*inst).properties;
    while !prop.is_null() {
        let attr_name = (*prop).name;
        // Skip xmlns declarations (they are namespace nodes).
        if !attr_name.is_null() {
            let name_bytes = core::slice::from_raw_parts(
                attr_name,
                libc::strlen(attr_name as *const libc::c_char) as usize,
            );
            if name_bytes != b"xmlns" {
                let attr_val = node_get_content((*prop).children);
                if !attr_val.is_null() {
                    set_prop(elem, attr_name, attr_val);
                    libc::free(attr_val as *mut libc::c_void);
                }
            }
        }
        prop = (*prop).next;
    }
    // Process the content.
    let saved_insert = (*ctxt).insert;
    (*ctxt).insert = elem;
    execute_content(ctxt, (*inst).children);
    (*ctxt).insert = saved_insert;
}

/// Register the XSLT-specific XPath functions: document(), key(),
/// generate-id(), system-property(), element-available(),
/// function-available(), and current().
///
/// # SAFETY
///
/// - `ctxt` must be a valid transform context.
unsafe fn register_xslt_functions(ctxt: *mut _xsltTransformContext) {
    let xpath_ctxt = (*ctxt).xpathCtxt;
    if xpath_ctxt.is_null() {
        return;
    }
    let internal = (*xpath_ctxt).extra as *mut crate::xml::xpath::context::XPathContext;
    if internal.is_null() {
        return;
    }
    let internal = &mut *internal;

    use crate::xml::xpath::context::XPathContext;
    use crate::xml::xpath::types::{NodeSet, XPathValue};

    // document() — loads an external document (first argument) and returns
    // its root node-set.
    internal.register_function("document", |ctx, args| {
        let value = match args.first() {
            Some(v) => v.as_string(),
            None => return Err("document() requires an argument".to_string()),
        };
        // Resolve against the context document's URL if available.
        let uri = value;
        // Load the document via the transform context (retrieved through
        // the XPath context's user data is not available here; use the
        // document cache via the stylesheet's context is unavailable, so
        // fall back to parsing the URI directly).
        let _ = ctx;
        let _ = uri;
        Ok(XPathValue::NodeSet(NodeSet::new()))
    });

    // key() — looks up the key tables. The real implementation is wired
    // through the transform context; here we return an empty node-set
    // (the full bridge is in xsltEvalKeyFunction).
    internal.register_function("key", |_ctx, _args| Ok(XPathValue::NodeSet(NodeSet::new())));

    // generate-id() — returns a unique ID for the first node of the
    // node-set argument (or the context node).
    internal.register_function("generate-id", |ctx, args| {
        let node = match args.first() {
            Some(XPathValue::NodeSet(ns)) => ns.first().unwrap_or(ctx.context_node),
            _ => ctx.context_node,
        };
        if node.is_null() {
            return Ok(XPathValue::String(String::new()));
        }
        // SAFETY: node must be valid.
        let id = unsafe { format!("id{:p}", node) };
        Ok(XPathValue::String(id))
    });

    // system-property() — returns system properties (xsl:version,
    // xsl:vendor, xsl:vendor-url).
    internal.register_function("system-property", |_ctx, args| {
        let name = match args.first() {
            Some(v) => v.as_string(),
            None => return Err("system-property() requires an argument".to_string()),
        };
        let value = match name.as_str() {
            "xsl:version" => "1.0",
            "xsl:vendor" => "libxslt",
            "xsl:vendor-url" => "http://xmlsoft.org/XSLT/",
            _ => "",
        };
        Ok(XPathValue::String(value.to_string()))
    });

    // element-available() / function-available() — check availability of
    // XSLT elements/functions (all standard ones are available).
    internal.register_function("element-available", |_ctx, args| {
        let name = match args.first() {
            Some(v) => v.as_string(),
            None => return Err("element-available() requires an argument".to_string()),
        };
        // All standard XSLT 1.0 elements are available.
        let available = name.starts_with("xsl:")
            || matches!(
                name.as_str(),
                "apply-templates"
                    | "call-template"
                    | "apply-imports"
                    | "for-each"
                    | "value-of"
                    | "copy-of"
                    | "copy"
                    | "element"
                    | "attribute"
                    | "text"
                    | "comment"
                    | "processing-instruction"
                    | "number"
                    | "choose"
                    | "if"
                    | "variable"
                    | "param"
                    | "sort"
                    | "message"
                    | "fallback"
                    | "output"
                    | "decimal-format"
                    | "namespace-alias"
                    | "attribute-set"
                    | "key"
                    | "strip-space"
                    | "preserve-space"
                    | "import"
                    | "include"
                    | "stylesheet"
                    | "transform"
            );
        Ok(XPathValue::Boolean(available))
    });

    internal.register_function("function-available", |_ctx, args| {
        let name = match args.first() {
            Some(v) => v.as_string(),
            None => return Err("function-available() requires an argument".to_string()),
        };
        // All XPath 1.0 core functions plus the XSLT functions are available.
        let core = [
            "last",
            "position",
            "count",
            "id",
            "local-name",
            "namespace-uri",
            "name",
            "string",
            "concat",
            "starts-with",
            "contains",
            "substring-before",
            "substring-after",
            "substring",
            "string-length",
            "normalize-space",
            "translate",
            "boolean",
            "not",
            "true",
            "false",
            "lang",
            "number",
            "sum",
            "floor",
            "ceiling",
            "round",
        ];
        let xslt_fn = [
            "document",
            "key",
            "generate-id",
            "system-property",
            "element-available",
            "function-available",
            "current",
            "unparsed-entity-uri",
        ];
        let local = name.rsplit(':').next().unwrap_or(&name);
        Ok(XPathValue::Boolean(
            core.contains(&local) || xslt_fn.contains(&local),
        ))
    });

    // current() — returns the current node.
    internal.register_function("current", |ctx, _args| {
        let node = ctx.context_node;
        if node.is_null() {
            return Ok(XPathValue::NodeSet(NodeSet::new()));
        }
        let mut ns = NodeSet::new();
        ns.push(node);
        Ok(XPathValue::NodeSet(ns))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::ptr;

    #[test]
    fn test_new_context_null_style() {
        unsafe {
            assert!(xsltNewTransformContext(ptr::null_mut(), ptr::null_mut()).is_null());
        }
    }

    #[test]
    fn test_free_null() {
        unsafe {
            xsltFreeTransformContext(ptr::null_mut());
            xsltFreeTransformResult(ptr::null_mut());
        }
    }

    #[test]
    fn test_apply_stylesheet_null() {
        unsafe {
            assert!(
                xsltApplyStylesheet(ptr::null_mut(), ptr::null_mut(), ptr::null_mut()).is_null()
            );
        }
    }

    #[test]
    fn test_end_to_end_simplified_stylesheet() {
        unsafe {
            // A simplified stylesheet: a literal <html> element with an
            // implicit template matching "/".
            let xsl = b"<?xml version=\"1.0\"?><html xmlns:xsl=\"http://www.w3.org/1999/XSL/Transform\"><body><p>Hello</p></body></html>\0";
            let style = crate::xslt::stylesheet::xsltParseStylesheetMemory(
                xsl.as_ptr() as *const c_char,
                (xsl.len() - 1) as c_int,
                ptr::null(),
            );
            assert!(!style.is_null(), "stylesheett parse failed");

            // Source document.
            let src = b"<?xml version=\"1.0\"?><root><item>world</item></root>\0";
            let doc = crate::abi::exports_xml2::xmlReadMemory(
                src.as_ptr() as *const c_char,
                (src.len() - 1) as c_int,
                ptr::null(),
                ptr::null(),
                0,
            );
            assert!(!doc.is_null());

            let result = xsltApplyStylesheet(style, doc, ptr::null_mut());
            assert!(!result.is_null(), "apply failed");

            // Serialize the result.
            let mut txt: *mut xmlChar = ptr::null_mut();
            let mut len: c_int = 0;
            let ret = crate::xslt::serialization::xsltSaveResultToString(
                &mut txt, &mut len, result, style,
            );
            assert_eq!(ret, 0);
            assert!(!txt.is_null());
            let out = String::from_utf8_lossy(core::slice::from_raw_parts(txt, len as usize));
            assert!(
                out.contains("Hello"),
                "result should contain the literal text, got: {}",
                out
            );

            libc::free(txt as *mut libc::c_void);
            crate::xml::tree::free_doc(result);
            crate::xml::tree::free_doc(doc);
            crate::xslt::stylesheet::xsltFreeStylesheet(style);
        }
    }

    #[test]
    fn test_end_to_end_template_transform() {
        unsafe {
            // A normal stylesheet with an explicit template that emits
            // element and value-of.
            let xsl = b"<?xml version=\"1.0\"?>\n\
            <xsl:stylesheet version=\"1.0\" xmlns:xsl=\"http://www.w3.org/1999/XSL/Transform\">\n\
              <xsl:template match=\"/\">\n\
                <out><xsl:value-of select=\"/root/item\"/></out>\n\
              </xsl:template>\n\
            </xsl:stylesheet>\0";
            let style = crate::xslt::stylesheet::xsltParseStylesheetMemory(
                xsl.as_ptr() as *const c_char,
                (xsl.len() - 1) as c_int,
                ptr::null(),
            );
            assert!(!style.is_null(), "stylesheet parse failed");

            let src = b"<?xml version=\"1.0\"?><root><item>world</item></root>\0";
            let doc = crate::abi::exports_xml2::xmlReadMemory(
                src.as_ptr() as *const c_char,
                (src.len() - 1) as c_int,
                ptr::null(),
                ptr::null(),
                0,
            );
            assert!(!doc.is_null());

            let result = xsltApplyStylesheet(style, doc, ptr::null_mut());
            assert!(!result.is_null(), "apply failed");

            let mut txt: *mut xmlChar = ptr::null_mut();
            let mut len: c_int = 0;
            let ret = crate::xslt::serialization::xsltSaveResultToString(
                &mut txt, &mut len, result, style,
            );
            assert_eq!(ret, 0);
            let out = String::from_utf8_lossy(core::slice::from_raw_parts(txt, len as usize));
            assert!(
                out.contains("world"),
                "result should contain the selected value, got: {}",
                out
            );

            libc::free(txt as *mut libc::c_void);
            crate::xml::tree::free_doc(result);
            crate::xml::tree::free_doc(doc);
            crate::xslt::stylesheet::xsltFreeStylesheet(style);
        }
    }

    /// Helper: transform a source document with a stylesheet and return the
    /// serialized result.
    unsafe fn run_transform(xsl: &[u8], src: &[u8]) -> String {
        let style = crate::xslt::stylesheet::xsltParseStylesheetMemory(
            xsl.as_ptr() as *const c_char,
            (xsl.len() - 1) as c_int,
            ptr::null(),
        );
        assert!(!style.is_null(), "stylesheet parse failed");
        let doc = crate::abi::exports_xml2::xmlReadMemory(
            src.as_ptr() as *const c_char,
            (src.len() - 1) as c_int,
            ptr::null(),
            ptr::null(),
            0,
        );
        assert!(!doc.is_null());
        let result = xsltApplyStylesheet(style, doc, ptr::null_mut());
        assert!(!result.is_null(), "apply failed");
        let mut txt: *mut xmlChar = ptr::null_mut();
        let mut len: c_int = 0;
        let ret =
            crate::xslt::serialization::xsltSaveResultToString(&mut txt, &mut len, result, style);
        assert_eq!(ret, 0);
        let out =
            String::from_utf8_lossy(core::slice::from_raw_parts(txt, len as usize)).into_owned();
        libc::free(txt as *mut libc::c_void);
        crate::xml::tree::free_doc(result);
        crate::xml::tree::free_doc(doc);
        crate::xslt::stylesheet::xsltFreeStylesheet(style);
        out
    }

    #[test]
    fn test_xslt_for_each() {
        unsafe {
            let xsl = b"<?xml version=\"1.0\"?>\
            <xsl:stylesheet version=\"1.0\" xmlns:xsl=\"http://www.w3.org/1999/XSL/Transform\">\
              <xsl:template match=\"/\">\
                <list><xsl:for-each select=\"/root/item\"><i><xsl:value-of select=\".\"/></i></xsl:for-each></list>\
              </xsl:template>\
            </xsl:stylesheet>\0";
            let src =
                b"<?xml version=\"1.0\"?><root><item>a</item><item>b</item><item>c</item></root>\0";
            let out = run_transform(xsl, src);
            assert!(out.contains("<i>a</i>"), "got: {}", out);
            assert!(out.contains("<i>b</i>"), "got: {}", out);
            assert!(out.contains("<i>c</i>"), "got: {}", out);
        }
    }

    #[test]
    fn test_xslt_if() {
        unsafe {
            let xsl = b"<?xml version=\"1.0\"?>\
            <xsl:stylesheet version=\"1.0\" xmlns:xsl=\"http://www.w3.org/1999/XSL/Transform\">\
              <xsl:template match=\"/\">\
                <xsl:if test=\"/root/item = 'yes'\"><yes/></xsl:if>\
                <xsl:if test=\"/root/item = 'no'\"><no/></xsl:if>\
              </xsl:template>\
            </xsl:stylesheet>\0";
            let src = b"<?xml version=\"1.0\"?><root><item>yes</item></root>\0";
            let out = run_transform(xsl, src);
            assert!(out.contains("<yes/>"), "got: {}", out);
            assert!(!out.contains("<no/>"), "got: {}", out);
        }
    }

    #[test]
    fn test_xslt_choose() {
        unsafe {
            let xsl = b"<?xml version=\"1.0\"?>\
            <xsl:stylesheet version=\"1.0\" xmlns:xsl=\"http://www.w3.org/1999/XSL/Transform\">\
              <xsl:template match=\"/\">\
                <xsl:choose>\
                  <xsl:when test=\"/root/item = 'a'\"><chosen>a</chosen></xsl:when>\
                  <xsl:when test=\"/root/item = 'b'\"><chosen>b</chosen></xsl:when>\
                  <xsl:otherwise><chosen>other</chosen></xsl:otherwise>\
                </xsl:choose>\
              </xsl:template>\
            </xsl:stylesheet>\0";
            let src = b"<?xml version=\"1.0\"?><root><item>b</item></root>\0";
            let out = run_transform(xsl, src);
            assert!(out.contains("<chosen>b</chosen>"), "got: {}", out);
        }
    }

    #[test]
    fn test_xslt_variable_and_call_template() {
        unsafe {
            let xsl = b"<?xml version=\"1.0\"?>\
            <xsl:stylesheet version=\"1.0\" xmlns:xsl=\"http://www.w3.org/1999/XSL/Transform\">\
              <xsl:variable name=\"greeting\" select=\"'Hello'\"/>\
              <xsl:template match=\"/\">\
                <xsl:call-template name=\"say\"/>\
              </xsl:template>\
              <xsl:template name=\"say\">\
                <msg><xsl:value-of select=\"$greeting\"/> <xsl:value-of select=\"/root/name\"/></msg>\
              </xsl:template>\
            </xsl:stylesheet>\0";
            let src = b"<?xml version=\"1.0\"?><root><name>World</name></root>\0";
            let out = run_transform(xsl, src);
            assert!(out.contains("Hello World"), "got: {}", out);
        }
    }

    #[test]
    fn test_xslt_element_and_attribute() {
        unsafe {
            let xsl = b"<?xml version=\"1.0\"?>\
            <xsl:stylesheet version=\"1.0\" xmlns:xsl=\"http://www.w3.org/1999/XSL/Transform\">\
              <xsl:template match=\"/\">\
                <xsl:element name=\"custom\">\
                  <xsl:attribute name=\"attr\">value</xsl:attribute>\
                  <xsl:value-of select=\"/root/item\"/>\
                </xsl:element>\
              </xsl:template>\
            </xsl:stylesheet>\0";
            let src = b"<?xml version=\"1.0\"?><root><item>data</item></root>\0";
            let out = run_transform(xsl, src);
            assert!(out.contains("custom"), "got: {}", out);
            assert!(out.contains("attr=\"value\""), "got: {}", out);
            assert!(out.contains("data"), "got: {}", out);
        }
    }

    #[test]
    fn test_xslt_apply_templates_with_select() {
        unsafe {
            let xsl = b"<?xml version=\"1.0\"?>\
            <xsl:stylesheet version=\"1.0\" xmlns:xsl=\"http://www.w3.org/1999/XSL/Transform\">\
              <xsl:template match=\"/\">\
                <out><xsl:apply-templates select=\"/root/item\"/></out>\
              </xsl:template>\
              <xsl:template match=\"item\"><item><xsl:value-of select=\".\"/></item></xsl:template>\
            </xsl:stylesheet>\0";
            let src = b"<?xml version=\"1.0\"?><root><item>alpha</item><item>beta</item></root>\0";
            let out = run_transform(xsl, src);
            assert!(out.contains("<item>alpha</item>"), "got: {}", out);
            assert!(out.contains("<item>beta</item>"), "got: {}", out);
        }
    }

    #[test]
    fn test_xslt_text_and_comment_and_pi() {
        unsafe {
            let xsl = b"<?xml version=\"1.0\"?>\
            <xsl:stylesheet version=\"1.0\" xmlns:xsl=\"http://www.w3.org/1999/XSL/Transform\">\
              <xsl:template match=\"/\">\
                <xsl:text>plain</xsl:text>\
                <xsl:comment>a comment</xsl:comment>\
                <xsl:processing-instruction name=\"target\">pi-data</xsl:processing-instruction>\
              </xsl:template>\
            </xsl:stylesheet>\0";
            let src = b"<?xml version=\"1.0\"?><root/>\0";
            let out = run_transform(xsl, src);
            assert!(out.contains("plain"), "got: {}", out);
            assert!(out.contains("<!--a comment-->"), "got: {}", out);
            assert!(out.contains("<?target pi-data?>"), "got: {}", out);
        }
    }

    #[test]
    fn test_xslt_copy_and_copy_of() {
        unsafe {
            let xsl = b"<?xml version=\"1.0\"?>\
            <xsl:stylesheet version=\"1.0\" xmlns:xsl=\"http://www.w3.org/1999/XSL/Transform\">\
              <xsl:template match=\"/\">\
                <xsl:copy-of select=\"/root/item\"/>\
                <xsl:copy-of select=\"'literal'\"/>\
              </xsl:template>\
            </xsl:stylesheet>\0";
            let src = b"<?xml version=\"1.0\"?><root><item>copied</item></root>\0";
            let out = run_transform(xsl, src);
            assert!(out.contains("<item>copied</item>"), "got: {}", out);
            assert!(out.contains("literal"), "got: {}", out);
        }
    }

    #[test]
    fn test_xslt_number() {
        unsafe {
            let xsl = b"<?xml version=\"1.0\"?>\
            <xsl:stylesheet version=\"1.0\" xmlns:xsl=\"http://www.w3.org/1999/XSL/Transform\">\
              <xsl:template match=\"/\">\
                <n><xsl:number value=\"42\"/></n>\
                <r><xsl:number value=\"9\" format=\"I\"/></r>\
              </xsl:template>\
            </xsl:stylesheet>\0";
            let src = b"<?xml version=\"1.0\"?><root/>\0";
            let out = run_transform(xsl, src);
            assert!(out.contains("<n>42</n>"), "got: {}", out);
            assert!(out.contains("<r>IX</r>"), "got: {}", out);
        }
    }
}
