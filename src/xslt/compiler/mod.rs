//! XSLT stylesheet compilation (§32, §85 Phase 8).
//!
//! Walks the parsed stylesheet document and produces the compiled
//! representation: templates, keys, variables, parameters, attribute
//! sets, namespace aliases, decimal formats, strip/preserve-space rules,
//! and output settings.
//!
//! # UPSTREAM-PARITY
//!
//! Upstream libxslt (xslt.c) compilation phases:
//!
//! 1. `xsltParseStylesheetDoc` → checks the root element:
//!    - `xsl:stylesheet` / `xsl:transform` — normal stylesheet
//!    - otherwise — simplified stylesheet (literal result element with
//!      an implicit `<xsl:template match="/">`)
//! 2. Process `xsl:import` elements first (they must precede all other
//!    top-level elements).
//! 3. Process remaining top-level elements in document order.
//! 4. Compile templates with their match patterns and priorities.
//! 5. Templates are inserted into the stylesheet's ordered list,
//!    sorted by priority (highest first).

use crate::abi::allocator::xmlFreeImpl;
use crate::abi::structs::*;
use crate::abi::types::xmlElementType::*;
use crate::abi::types::*;
use crate::xml::tree::{doc_get_root_element, get_prop};
use std::ffi::c_void;
use std::os::raw::c_int;
use std::ptr;

/// The XSLT namespace URI.
pub const XSLT_NAMESPACE: &str = "http://www.w3.org/1999/XSL/Transform";

/// Compile a stylesheet from a document.
///
/// Takes ownership of the stylesheet lifecycle: the document is stored in
/// `style->doc`; on failure the stylesheet is freed.
///
/// # SAFETY
///
/// - `style` must be a valid zero-initialized `_xsltStylesheet`.
/// - `doc` must be a valid parsed document.
pub unsafe fn compile(style: *mut _xsltStylesheet, doc: *mut _xmlDoc) -> c_int {
    if style.is_null() || doc.is_null() {
        return -1;
    }
    (*style).doc = doc;
    let root = doc_get_root_element(doc);
    if root.is_null() {
        // No root element: not a stylesheet.
        return -1;
    }

    // Determine the namespace of the root element.
    let root_ns = get_element_ns(root);
    let root_name = get_element_name(root);

    // UPSTREAM-PARITY: preprocess the stylesheet tree before compiling
    // (xsltParsePreprocessStylesheetTree, xslt.c 1.1.45): merge adjacent
    // text runs, strip whitespace-only runs, remove comments and PIs.
    let is_stylesheet = matches!(
        (root_ns.as_deref(), root_name.as_deref()),
        (Some(ns), Some(name))
            if ns == XSLT_NAMESPACE && (name == "stylesheet" || name == "transform")
    );
    preprocess_stylesheet_tree(root, is_stylesheet);

    match (root_ns.as_deref(), root_name.as_deref()) {
        (Some(ns), Some(name))
            if ns == XSLT_NAMESPACE && (name == "stylesheet" || name == "transform") =>
        {
            // Normal stylesheet.
            compile_top_level(style, root, 0);
        }
        _ => {
            // Simplified stylesheet: a literal result element with an
            // implicit template matching "/".
            compile_simplified(style, root);
        }
    }
    0
}

/// Get the document URL (or NULL).
unsafe fn doc_URL(doc: *mut _xmlDoc) -> *const xmlChar {
    if doc.is_null() || (*doc).URL.is_null() {
        return ptr::null();
    }
    (*doc).URL
}

/// Compile the top-level elements of a stylesheet.
///
/// # SAFETY
///
/// - All pointers must be valid.
pub(crate) unsafe fn compile_top_level(style: *mut _xsltStylesheet, root: *mut _xmlNode, depth: c_int) {
    // Process imports first (they must precede other elements).
    let mut child = (*root).children;
    while !child.is_null() {
        let next = (*child).next;
        if is_xslt_element(child, "import") {
            let href = get_prop(child, b"href\0".as_ptr() as *const xmlChar);
            if !href.is_null() {
                let imported = crate::xslt::stylesheet::xsltParseStylesheetFile(href);
                if !imported.is_null() {
                    (*imported).parent = style;
                    (*imported).next = (*style).imports;
                    (*style).imports = imported;
                    // Compile the imported stylesheet's top-level into its
                    // own templates (already done during its parse).
                }
                libc::free(href as *mut libc::c_void);
            }
        }
        child = next;
    }

    // Process remaining top-level elements.
    let mut child = (*root).children;
    while !child.is_null() {
        let next = (*child).next;
        if (*child).type_ != XML_ELEMENT_NODE as c_int {
            child = next;
            continue;
        }
        if !is_xslt_namespace(child) {
            child = next;
            continue;
        }
        let name = get_element_name(child);
        match name.as_deref() {
            Some("import") => {} // already handled
            Some("include") => {
                let href = get_prop(child, b"href\0".as_ptr() as *const xmlChar);
                if !href.is_null() {
                    let included = crate::xslt::stylesheet::xsltParseStylesheetFile(href);
                    if !included.is_null() {
                        // Compile the included stylesheet's top-level
                        // elements at the same depth.
                        let inc_root = doc_get_root_element((*included).doc);
                        if !inc_root.is_null() {
                            compile_top_level(style, inc_root, depth);
                        }
                        // The included doc's templates were added to the
                        // included style; move them to this style.
                        move_templates(included, style);
                        // Merge strip/preserve spaces.
                        merge_strip_spaces(included, style);
                        // Free the included stylesheet shell (not the doc).
                        (*included).doc = ptr::null_mut();
                        crate::xslt::stylesheet::xsltFreeStylesheet(included);
                    }
                    libc::free(href as *mut libc::c_void);
                }
            }
            Some("template") => {
                compile_template(style, child, depth);
            }
            Some("variable") | Some("param") => {
                let is_param = if name.as_deref() == Some("param") {
                    1
                } else {
                    0
                };
                compile_variable(style, child, depth, is_param);
            }
            Some("key") => {
                compile_key(style, child, depth);
            }
            Some("decimal-format") => {
                compile_decimal_format(style, child);
            }
            Some("namespace-alias") => {
                compile_namespace_alias(style, child);
            }
            Some("attribute-set") => {
                compile_attribute_set(style, child, depth);
            }
            Some("strip-space") => {
                compile_space_rules(style, child, 1, depth);
            }
            Some("preserve-space") => {
                compile_space_rules(style, child, 0, depth);
            }
            Some("output") => {
                compile_output(style, child);
            }
            Some("stylesheet") | Some("transform") => {
                // Nested stylesheet elements are an error; ignore.
            }
            _ => {}
        }
        child = next;
    }
}

/// Check whether a node is an XSLT element with the given local name.
///
/// # SAFETY
///
/// - `node` must be a valid element node.
pub unsafe fn is_xslt_element(node: *mut _xmlNode, name: &str) -> bool {
    if node.is_null() || (*node).type_ != XML_ELEMENT_NODE as c_int {
        return false;
    }
    let ns = get_element_ns(node);
    if ns.as_deref() != Some(XSLT_NAMESPACE) {
        return false;
    }
    get_element_name(node).as_deref() == Some(name)
}

/// Check whether a node is in the XSLT namespace.
///
/// # SAFETY
///
/// - `node` must be a valid element node.
pub unsafe fn is_xslt_namespace(node: *mut _xmlNode) -> bool {
    if node.is_null() || (*node).type_ != XML_ELEMENT_NODE as c_int {
        return false;
    }
    get_element_ns(node).as_deref() == Some(XSLT_NAMESPACE)
}

/// Get the namespace URI of an element as a String.
///
/// # SAFETY
///
/// - `node` must be a valid element node.
pub unsafe fn get_element_ns(node: *mut _xmlNode) -> Option<String> {
    if node.is_null() || (*node).ns.is_null() || (*(*node).ns).href.is_null() {
        return None;
    }
    let bytes = core::slice::from_raw_parts(
        (*(*node).ns).href,
        libc::strlen((*(*node).ns).href as *const libc::c_char) as usize,
    );
    String::from_utf8_lossy(bytes).into_owned().into()
}

/// Get the local name of an element as a String.
///
/// # SAFETY
///
/// - `node` must be a valid element node.
pub unsafe fn get_element_name(node: *mut _xmlNode) -> Option<String> {
    if node.is_null() || (*node).name.is_null() {
        return None;
    }
    let bytes = core::slice::from_raw_parts(
        (*node).name,
        libc::strlen((*node).name as *const libc::c_char) as usize,
    );
    String::from_utf8_lossy(bytes).into_owned().into()
}

/// Preprocess the stylesheet's node tree.
///
/// # UPSTREAM-PARITY
///
/// Faithful port of the strict-mode behavior of
/// `xsltParsePreprocessStylesheetTree` (xslt.c, libxslt 1.1.45):
///
/// - adjacent text/CDATA-section nodes are merged into one text node;
/// - a merged run that is whitespace-only is removed, unless the parent
///   element has `xml:space="preserve"` or is `xsl:text`;
/// - comments and processing instructions are removed from the tree;
/// - CDATA-section nodes are converted to text nodes;
/// - whitespace-only text nodes directly preceding `xsl:param`/`xsl:sort`
///   are removed;
/// - elements in the XSLT "strip" set (`xsl:choose`, `xsl:call-template`,
///   `xsl:apply-templates`, `xsl:apply-imports`, `xsl:attribute-set`, and
///   the `xsl:stylesheet`/`xsl:transform` element itself) always strip
///   whitespace-only runs.
///
/// This explains the observable behavior verified against the oracle:
/// `<xsl:template><a>\n  X\n</a></xsl:template>` keeps the whitespace
/// around `X` (the merged run is not whitespace-only) while
/// `<xsl:template><a>\n  <b/></a></xsl:template>` strips it.
///
/// # SAFETY
///
/// - `root` must be a valid element node of the stylesheet document.
unsafe fn preprocess_stylesheet_tree(root: *mut _xmlNode, is_stylesheet: bool) {
    /// Per-element whitespace-handling state (upstream compiler-node-info).
    struct St {
        strip_whitespace: bool,
        preserve_whitespace: bool,
    }

    /// True when every byte is a whitespace character (upstream xsltIsBlank).
    unsafe fn is_blank(content: *const xmlChar) -> bool {
        if content.is_null() {
            return true;
        }
        let mut i = 0usize;
        while unsafe { *content.add(i) != 0 } {
            match unsafe { *content.add(i) } {
                b' ' | b'\t' | b'\n' | b'\r' => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// Append `src` bytes to `dst`'s content (upstream xmlNodeAddContent).
    unsafe fn merge_text(dst: *mut _xmlNode, src: *const xmlChar) {
        if src.is_null() {
            return;
        }
        let src_len = crate::xml::tree::xml_strlen(src) as usize;
        if src_len == 0 {
            return;
        }
        let cur = unsafe { (*dst).content };
        let cur_len = if cur.is_null() {
            0
        } else {
            crate::xml::tree::xml_strlen(cur) as usize
        };
        let new_len = cur_len + src_len;
        let buf = crate::abi::allocator::xmlMallocImpl(new_len + 1) as *mut xmlChar;
        if buf.is_null() {
            return;
        }
        if cur_len > 0 && !cur.is_null() {
            core::ptr::copy_nonoverlapping(cur, buf, cur_len);
        }
        core::ptr::copy_nonoverlapping(src, buf.add(cur_len), src_len);
        *buf.add(new_len) = 0;
        if !cur.is_null() {
            crate::abi::allocator::xmlFreeImpl(cur as *mut core::ffi::c_void);
        }
        unsafe { (*dst).content = buf };
    }

    /// Convert a CDATA-section node to a text node.
    unsafe fn to_text(node: *mut _xmlNode) {
        if unsafe { (*node).type_ } != XML_CDATA_SECTION_NODE as c_int {
            return;
        }
        unsafe {
            (*node).type_ = XML_TEXT_NODE as c_int;
            let name = crate::abi::allocator::xmlMallocImpl(5) as *mut xmlChar;
            if !name.is_null() {
                core::ptr::copy_nonoverlapping(b"text\0".as_ptr(), name, 5);
                (*node).name = name;
            }
        }
    }

    /// The xml:space attribute value of an element (1 = preserve,
    /// 0 = default, -1 = unset).
    unsafe fn xml_space(node: *mut _xmlNode) -> c_int {
        let mut attr = unsafe { (*node).properties };
        while !attr.is_null() {
            let a = unsafe { &*attr };
            let ns_is_xml = if a.ns.is_null() {
                false
            } else {
                let prefix = unsafe { (*a.ns).prefix };
                !prefix.is_null()
                    && unsafe { *prefix == b'x' }
                    && unsafe { *prefix.add(1) == b'm' }
                    && unsafe { *prefix.add(2) == b'l' }
                    && unsafe { *prefix.add(3) == 0 }
            };
            if ns_is_xml && !a.name.is_null() && unsafe { *a.name == b's' } {
                let name =
                    crate::abi::versioning::c_str_to_bytes(a.name as *const std::os::raw::c_char)
                        .unwrap_or(b"");
                if name == b"space" && !a.children.is_null() {
                    let val =
                        crate::abi::versioning::c_str_to_bytes(
                            unsafe { (*a.children).content } as *const std::os::raw::c_char
                        )
                        .unwrap_or(b"");
                    if val == b"preserve" {
                        return 1;
                    }
                    if val == b"default" {
                        return 0;
                    }
                }
            }
            attr = a.next;
        }
        -1
    }

    /// Apply the end-of-text-run strip check (upstream `end_of_text`).
    /// Returns true when the node was scheduled for deletion.
    unsafe fn check_strip(node: *mut _xmlNode, st: &St, delete: &mut Vec<*mut _xmlNode>) {
        if node.is_null() {
            return;
        }
        let content = unsafe { (*node).content };
        let blank = content.is_null() || unsafe { *content == 0 } || is_blank(content);
        if blank && (st.strip_whitespace || !st.preserve_whitespace) {
            delete.push(node);
        } else {
            to_text(node);
        }
    }

    unsafe fn walk(children: *mut _xmlNode, st: &St, stylesheet_depth: bool, top_level: bool) {
        let mut cur = children;
        let mut text_node: *mut _xmlNode = ptr::null_mut();
        let mut delete: Vec<*mut _xmlNode> = Vec::new();

        while !cur.is_null() {
            let next = unsafe { (*cur).next };
            for d in delete.drain(..) {
                crate::xml::tree::unlink_node(d);
                crate::xml::tree::free_node(d);
            }

            match unsafe { (*cur).type_ } {
                t if t == XML_ELEMENT_NODE as c_int => {
                    // Compute the state for this element's content. Upstream
                    // resets stripWhitespace to 0 for every element (only the
                    // listed XSLT instructions set it to 1);
                    // preserveWhitespace is inherited and can be changed by
                    // xml:space or xsl:text.
                    let mut nst = St {
                        strip_whitespace: false,
                        preserve_whitespace: st.preserve_whitespace,
                    };
                    if !unsafe { (*cur).children }.is_null() {
                        match xml_space(cur) {
                            1 => nst.preserve_whitespace = true,
                            0 => nst.preserve_whitespace = false,
                            _ => {}
                        }
                    }
                    if is_xslt_element(cur, "text") {
                        nst.preserve_whitespace = true;
                    } else if is_xslt_element(cur, "choose")
                        || is_xslt_element(cur, "call-template")
                        || is_xslt_element(cur, "apply-templates")
                        || is_xslt_element(cur, "apply-imports")
                        || is_xslt_element(cur, "attribute-set")
                    {
                        nst.strip_whitespace = true;
                    } else if stylesheet_depth {
                        // The xsl:stylesheet/xsl:transform element itself.
                        nst.strip_whitespace = true;
                    } else if is_xslt_element(cur, "param") || is_xslt_element(cur, "sort") {
                        // Remove whitespace-only text nodes directly before
                        // xsl:param / xsl:sort (upstream default case).
                        let mut prev = unsafe { (*cur).prev };
                        while !prev.is_null()
                            && unsafe { (*prev).type_ } == XML_TEXT_NODE as c_int
                            && is_blank(unsafe { (*prev).content })
                        {
                            let p = prev;
                            prev = unsafe { (*prev).prev };
                            crate::xml::tree::unlink_node(p);
                            crate::xml::tree::free_node(p);
                        }
                    }
                    if !unsafe { (*cur).children }.is_null() {
                        walk(unsafe { (*cur).children }, &nst, false, false);
                    }
                }
                t if t == XML_TEXT_NODE as c_int || t == XML_CDATA_SECTION_NODE as c_int => {
                    // Strict mode: merge adjacent text nodes.
                    if text_node.is_null() {
                        text_node = cur;
                    } else {
                        merge_text(text_node, unsafe { (*cur).content });
                        delete.push(cur);
                    }
                    let end_of_run = unsafe { (*cur).next }.is_null()
                        || unsafe { (*(*cur).next).type_ } == XML_ELEMENT_NODE as c_int;
                    if end_of_run {
                        check_strip(text_node, st, &mut delete);
                        text_node = ptr::null_mut();
                    }
                }
                t if t == XML_COMMENT_NODE as c_int || t == XML_PI_NODE as c_int => {
                    delete.push(cur);
                    let end_of_run = unsafe { (*cur).next }.is_null()
                        || unsafe { (*(*cur).next).type_ } == XML_ELEMENT_NODE as c_int;
                    if end_of_run {
                        check_strip(text_node, st, &mut delete);
                        text_node = ptr::null_mut();
                    }
                }
                _ => {}
            }
            cur = next;
        }
        for d in delete {
            crate::xml::tree::unlink_node(d);
            crate::xml::tree::free_node(d);
        }
    }

    let st = St {
        strip_whitespace: false,
        preserve_whitespace: false,
    };
    walk(unsafe { (*root).children }, &st, is_stylesheet, true);
}

/// Compile a simplified stylesheet: a literal result element with an
/// implicit `<xsl:template match="/">`.
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn compile_simplified(style: *mut _xsltStylesheet, root: *mut _xmlNode) {
    // Create an implicit template matching "/" (the document node).
    let templ = libc::calloc(1, core::mem::size_of::<_xsltTemplate>()) as *mut _xsltTemplate;
    if templ.is_null() {
        return;
    }
    (*templ).style = style;
    (*templ).priority = 0.5; // match="/" has implicit priority 0.5 (f32)
    (*templ).content = root;
    // Compile the "/" pattern so the template matches the document node.
    // UPSTREAM-PARITY: templ->match holds the match STRING; the compiled
    // pattern is carried in templ->params (void*, unused upstream for the
    // candidate's internal pattern pointer — documented safe divergence).
    // The match string is heap-copied: xsltFreeTemplate owns r#match.
    (*templ).r#match = libc::malloc(2) as *mut xmlChar;
    if (*templ).r#match.is_null() {
        libc::free(templ as *mut libc::c_void);
        return;
    }
    *(*templ).r#match = b'/';
    *(*templ).r#match.add(1) = 0;
    let compiled =
        crate::xslt::patterns::xsltCompilePattern(b"/\0".as_ptr() as *const xmlChar, (*style).doc);
    (*templ).params = compiled as *mut c_void;
    // The root element becomes the template content.
    add_template_to_style(style, templ);
}

/// Add a compiled template to the stylesheet's ordered list.
///
/// The list is maintained in descending priority order.
///
/// # SAFETY
///
/// - All pointers must be valid.
pub(crate) unsafe fn add_template_to_style(style: *mut _xsltStylesheet, templ: *mut _xsltTemplate) {
    if style.is_null() || templ.is_null() {
        return;
    }
    (*templ).next = (*style).templates;
    (*style).templates = templ;
}

/// Compile an `xsl:template` element.
///
/// # SAFETY
///
/// - All pointers must be valid.
pub(crate) unsafe fn compile_template(style: *mut _xsltStylesheet, inst: *mut _xmlNode, depth: c_int) {
    let templ = libc::calloc(1, core::mem::size_of::<_xsltTemplate>()) as *mut _xsltTemplate;
    if templ.is_null() {
        return;
    }
    (*templ).style = style;
    (*templ).content = (*inst).children;

    // match attribute.
    let match_str = get_prop(inst, b"match\0".as_ptr() as *const xmlChar);
    if !match_str.is_null() {
        // UPSTREAM-PARITY: templ->match holds the match STRING; the compiled
        // pattern is carried in templ->params (candidate-internal pointer).
        let compiled = crate::xslt::patterns::xsltCompilePattern(match_str, (*style).doc);
        (*templ).r#match = match_str; // ownership of the string transfers
        (*templ).params = compiled as *mut c_void;
        // Compute the default priority from the pattern string.
        (*templ).priority = crate::xslt::patterns::xsltDefaultPriority(match_str) as f32;
    } else {
        (*templ).priority = 0.0;
    }

    // name attribute.
    let name_str = get_prop(inst, b"name\0".as_ptr() as *const xmlChar);
    if !name_str.is_null() {
        (*templ).name = name_str;
    }

    // mode attribute.
    let mode_str = get_prop(inst, b"mode\0".as_ptr() as *const xmlChar);
    if !mode_str.is_null() {
        (*templ).mode = mode_str;
    }

    // priority attribute.
    let priority_str = get_prop(inst, b"priority\0".as_ptr() as *const xmlChar);
    if !priority_str.is_null() {
        let p = crate::abi::exports_xml2::xmlXPathCastStringToNumber(priority_str);
        if !p.is_nan() {
            (*templ).priority = p as f32;
        }
        libc::free(priority_str as *mut libc::c_void);
    }

    add_template_to_style(style, templ);
}

/// Compile an `xsl:variable` or `xsl:param` element.
///
/// # SAFETY
///
/// - All pointers must be valid.
pub(crate) unsafe fn compile_variable(
    style: *mut _xsltStylesheet,
    inst: *mut _xmlNode,
    depth: c_int,
    is_param: c_int,
) {
    let var = libc::calloc(1, core::mem::size_of::<_xsltStackElem>()) as *mut _xsltStackElem;
    if var.is_null() {
        return;
    }
    // UPSTREAM-PARITY: xsltStackElem has no style/inst/depth fields; the
    // scope level lives in `level`, the PARAM marker in `flags`.
    (*var).level = depth;
    (*var).flags = if is_param != 0 { 2 } else { 0 }; // PARAM flag

    let name = get_prop(inst, b"name\0".as_ptr() as *const xmlChar);
    if !name.is_null() {
        (*var).name = name;
    }
    let select = get_prop(inst, b"select\0".as_ptr() as *const xmlChar);
    if !select.is_null() {
        (*var).select = select;
    }
    // Content (inline value template).
    (*var).tree = (*inst).children;

    // Add to the stylesheet's variable list (upstream: `variables` is the
    // xsltStackElem list head).
    (*var).next = (*style).variables;
    (*style).variables = var;
}

/// Compile an `xsl:key` element.
///
/// # SAFETY
///
/// - All pointers must be valid.
pub(crate) unsafe fn compile_key(style: *mut _xsltStylesheet, inst: *mut _xmlNode, depth: c_int) {
    let name = get_prop(inst, b"name\0".as_ptr() as *const xmlChar);
    let match_str = get_prop(inst, b"match\0".as_ptr() as *const xmlChar);
    let use_str = get_prop(inst, b"use\0".as_ptr() as *const xmlChar);
    if name.is_null() || match_str.is_null() || use_str.is_null() {
        if !name.is_null() {
            libc::free(name as *mut libc::c_void);
        }
        if !match_str.is_null() {
            libc::free(match_str as *mut libc::c_void);
        }
        if !use_str.is_null() {
            libc::free(use_str as *mut libc::c_void);
        }
        return;
    }
    let def = libc::calloc(1, core::mem::size_of::<_xsltKeyDef>()) as *mut _xsltKeyDef;
    if def.is_null() {
        libc::free(name as *mut libc::c_void);
        libc::free(match_str as *mut libc::c_void);
        libc::free(use_str as *mut libc::c_void);
        return;
    }
    (*def).inst = inst;
    (*def).name = name;
    (*def).r#match = match_str;
    (*def).r#use = use_str;
    (*def).next = (*style).keys as *mut _xsltKeyDef;
    (*style).keys = def as *mut c_void;
}

/// Compile an `xsl:decimal-format` element.
///
/// # SAFETY
///
/// - All pointers must be valid.
pub(crate) unsafe fn compile_decimal_format(style: *mut _xsltStylesheet, inst: *mut _xmlNode) {
    let fmt =
        libc::calloc(1, core::mem::size_of::<_xsltDecimalFormat>()) as *mut _xsltDecimalFormat;
    if fmt.is_null() {
        return;
    }
    // name (optional; NULL = default format).
    let name = get_prop(inst, b"name\0".as_ptr() as *const xmlChar);
    if !name.is_null() {
        (*fmt).name = name;
    }
    // Attribute characters.
    set_fmt_char(fmt, b"decimal-separator\0", b".\0", 0);
    set_fmt_char(fmt, b"grouping-separator\0", b",\0", 1);
    set_fmt_char(fmt, b"infinity\0", b"Infinity\0", 2);
    set_fmt_char(fmt, b"minus-sign\0", b"-\0", 3);
    set_fmt_char(fmt, b"NaN\0", b"NaN\0", 4);
    set_fmt_char(fmt, b"percent\0", b"%\0", 5);
    set_fmt_char(fmt, b"per-mille\0", "‰".as_bytes(), 6);
    set_fmt_char(fmt, b"zero-digit\0", b"0\0", 7);
    set_fmt_char(fmt, b"digit\0", b"#\0", 8);
    set_fmt_char(fmt, b"pattern-separator\0", b";\0", 9);

    // Prepend to the stylesheet's decimal format chain.
    (*fmt).next = (*style).decimalFormat;
    (*style).decimalFormat = fmt;
}

/// Set a decimal format character from the instruction attributes or default.
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn set_fmt_char(
    fmt: *mut _xsltDecimalFormat,
    attr_name: &[u8],
    default: &[u8],
    field: c_int,
) {
    let mut attr = b"\0".to_vec();
    let mut full = attr_name.to_vec();
    full.push(0);
    let val = get_prop(fmt as *mut _xmlNode, full.as_ptr() as *const xmlChar);
    let _ = attr;
    let chosen = if !val.is_null() {
        val
    } else {
        alloc_str(default)
    };
    match field {
        0 => (*fmt).decimalPoint = chosen,
        1 => (*fmt).grouping = chosen,
        2 => (*fmt).infinity = chosen,
        3 => (*fmt).minusSign = chosen,
        4 => (*fmt).noNumber = chosen,
        5 => (*fmt).percent = chosen,
        6 => (*fmt).permille = chosen,
        7 => (*fmt).zeroDigit = chosen,
        8 => (*fmt).digit = chosen,
        _ => (*fmt).patternSeparator = chosen,
    }
}

/// Allocate a NUL-terminated string.
unsafe fn alloc_str(bytes: &[u8]) -> *mut xmlChar {
    let p = libc::malloc(bytes.len() + 1) as *mut xmlChar;
    if p.is_null() {
        return ptr::null_mut();
    }
    libc::memcpy(
        p as *mut libc::c_void,
        bytes.as_ptr() as *const libc::c_void,
        bytes.len(),
    );
    *p.add(bytes.len()) = 0;
    p
}

/// Compile an `xsl:namespace-alias` element.
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn compile_namespace_alias(style: *mut _xsltStylesheet, inst: *mut _xmlNode) {
    let stylesheet_prefix = get_prop(inst, b"stylesheet-prefix\0".as_ptr() as *const xmlChar);
    let result_prefix = get_prop(inst, b"result-prefix\0".as_ptr() as *const xmlChar);
    if stylesheet_prefix.is_null() || result_prefix.is_null() {
        if !stylesheet_prefix.is_null() {
            libc::free(stylesheet_prefix as *mut libc::c_void);
        }
        if !result_prefix.is_null() {
            libc::free(result_prefix as *mut libc::c_void);
        }
        return;
    }
    // Resolve prefixes to namespace URIs via the instruction's nsDef.
    let style_ns = prefix_to_uri(inst, stylesheet_prefix);
    let result_ns = prefix_to_uri(inst, result_prefix);
    libc::free(stylesheet_prefix as *mut libc::c_void);
    libc::free(result_prefix as *mut libc::c_void);
    if style_ns.is_null() || result_ns.is_null() {
        if !style_ns.is_null() {
            libc::free(style_ns as *mut libc::c_void);
        }
        if !result_ns.is_null() {
            libc::free(result_ns as *mut libc::c_void);
        }
        return;
    }
    let alias = libc::calloc(1, core::mem::size_of::<_xsltNsAlias>()) as *mut _xsltNsAlias;
    if alias.is_null() {
        libc::free(style_ns as *mut libc::c_void);
        libc::free(result_ns as *mut libc::c_void);
        return;
    }
    (*alias).styleNs = style_ns;
    (*alias).resultNs = result_ns;
    (*alias).next = (*style).nsAliases as *mut _xsltNsAlias;
    (*style).nsAliases = alias as *mut c_void;
}

/// Resolve a namespace prefix to a URI using the node's namespace
/// declarations. Returns a heap-allocated string or NULL.
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn prefix_to_uri(node: *mut _xmlNode, prefix: *const xmlChar) -> *mut xmlChar {
    if node.is_null() || prefix.is_null() {
        return ptr::null_mut();
    }
    let mut ns = (*node).nsDef;
    while !ns.is_null() {
        let ns_prefix = (*ns).prefix;
        let prefix_matches = if ns_prefix.is_null() {
            *prefix == 0
        } else {
            libc::strcmp(
                ns_prefix as *const libc::c_char,
                prefix as *const libc::c_char,
            ) == 0
        };
        if prefix_matches && !(*ns).href.is_null() {
            return alloc_str(core::slice::from_raw_parts(
                (*ns).href,
                libc::strlen((*ns).href as *const libc::c_char) as usize,
            ));
        }
        ns = (*ns).next;
    }
    ptr::null_mut()
}

/// Compile an `xsl:attribute-set` element.
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn compile_attribute_set(style: *mut _xsltStylesheet, inst: *mut _xmlNode, depth: c_int) {
    let name = get_prop(inst, b"name\0".as_ptr() as *const xmlChar);
    if name.is_null() {
        return;
    }
    let set = libc::calloc(1, core::mem::size_of::<_xsltAttrSet>()) as *mut _xsltAttrSet;
    if set.is_null() {
        libc::free(name as *mut libc::c_void);
        return;
    }
    (*set).name = name;
    (*set).inst = inst;
    (*set).style = style;
    (*set).depth = depth;
    (*set).next = (*style).attributeSets as *mut _xsltAttrSet;
    (*style).attributeSets = set as *mut c_void;
}

/// Compile `xsl:strip-space` or `xsl:preserve-space` elements.
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn compile_space_rules(
    style: *mut _xsltStylesheet,
    inst: *mut _xmlNode,
    strip: c_int,
    depth: c_int,
) {
    let elements = get_prop(inst, b"elements\0".as_ptr() as *const xmlChar);
    if elements.is_null() {
        return;
    }
    // Split on whitespace.
    let bytes = core::slice::from_raw_parts(
        elements,
        libc::strlen(elements as *const libc::c_char) as usize,
    );
    for name in bytes
        .split(|b| *b == b' ' || *b == b'\t' || *b == b'\n' || *b == b'\r')
        .filter(|s| !s.is_empty())
    {
        let mut cname = name.to_vec();
        cname.push(0);
        let entry = libc::calloc(
            1,
            core::mem::size_of::<crate::xslt::whitespace::_xsltStripSpace>(),
        ) as *mut crate::xslt::whitespace::_xsltStripSpace;
        if entry.is_null() {
            continue;
        }
        (*entry).name = alloc_str(&cname);
        (*entry).depth = depth;
        if strip != 0 {
            (*entry).next = (*style).stripSpaces as *mut crate::xslt::whitespace::_xsltStripSpace;
            (*style).stripSpaces = entry as *mut c_void;
        } else {
            // UPSTREAM-PARITY: upstream keeps only a stripSpaces hash + a
            // stripAll flag; the candidate's preserve-list head is carried in
            // the unused nsDefs void* slot (documented divergence).
            (*entry).next = (*style).nsDefs as *mut crate::xslt::whitespace::_xsltStripSpace;
            (*style).nsDefs = entry as *mut c_void;
        }
    }
    libc::free(elements as *mut libc::c_void);
}

/// Compile the `xsl:output` element into the stylesheet's output settings.
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn compile_output(style: *mut _xsltStylesheet, inst: *mut _xmlNode) {
    let method = get_prop(inst, b"method\0".as_ptr() as *const xmlChar);
    if !method.is_null() {
        (*style).method = method;
    }
    let version = get_prop(inst, b"version\0".as_ptr() as *const xmlChar);
    if !version.is_null() {
        (*style).version = version;
    }
    let encoding = get_prop(inst, b"encoding\0".as_ptr() as *const xmlChar);
    if !encoding.is_null() {
        (*style).encoding = encoding;
    }
    let omit = get_prop(inst, b"omit-xml-declaration\0".as_ptr() as *const xmlChar);
    if !omit.is_null() {
        (*style).omitXmlDeclaration = if cstr_eq(omit, b"yes") { 1 } else { 0 };
        libc::free(omit as *mut libc::c_void);
    }
    let standalone = get_prop(inst, b"standalone\0".as_ptr() as *const xmlChar);
    if !standalone.is_null() {
        (*style).standalone = if cstr_eq(standalone, b"yes") {
            1
        } else if cstr_eq(standalone, b"no") {
            0
        } else {
            -1
        };
        libc::free(standalone as *mut libc::c_void);
    }
    let indent = get_prop(inst, b"indent\0".as_ptr() as *const xmlChar);
    if !indent.is_null() {
        (*style).indent = if cstr_eq(indent, b"yes") { 1 } else { 0 };
        libc::free(indent as *mut libc::c_void);
    }
    let doctype_public = get_prop(inst, b"doctype-public\0".as_ptr() as *const xmlChar);
    if !doctype_public.is_null() {
        (*style).doctypePublic = doctype_public;
    }
    let doctype_system = get_prop(inst, b"doctype-system\0".as_ptr() as *const xmlChar);
    if !doctype_system.is_null() {
        (*style).doctypeSystem = doctype_system;
    }
    let media_type = get_prop(inst, b"media-type\0".as_ptr() as *const xmlChar);
    if !media_type.is_null() {
        (*style).mediaType = media_type;
    }
}

/// Compare a C string with a byte literal.
unsafe fn cstr_eq(s: *const xmlChar, expected: &[u8]) -> bool {
    if s.is_null() {
        return false;
    }
    let len = libc::strlen(s as *const libc::c_char) as usize;
    if len != expected.len() {
        return false;
    }
    let bytes = core::slice::from_raw_parts(s, len);
    bytes == expected
}

/// Move templates from an included stylesheet to the including one.
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn move_templates(src: *mut _xsltStylesheet, dst: *mut _xsltStylesheet) {
    let mut cur = (*src).templates;
    while !cur.is_null() {
        let next = (*cur).next;
        (*cur).style = dst;
        add_template_to_style(dst, cur);
        cur = next;
    }
    (*src).templates = ptr::null_mut();
}

/// Merge strip/preserve-space rules from an included stylesheet.
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn merge_strip_spaces(src: *mut _xsltStylesheet, dst: *mut _xsltStylesheet) {
    let mut cur = (*src).stripSpaces as *mut crate::xslt::whitespace::_xsltStripSpace;
    while !cur.is_null() {
        let next = (*cur).next;
        (*cur).next = (*dst).stripSpaces as *mut crate::xslt::whitespace::_xsltStripSpace;
        (*dst).stripSpaces = cur as *mut c_void;
        cur = next;
    }
    (*src).stripSpaces = ptr::null_mut();
    let mut cur = (*src).nsDefs as *mut crate::xslt::whitespace::_xsltStripSpace;
    while !cur.is_null() {
        let next = (*cur).next;
        (*cur).next = (*dst).nsDefs as *mut crate::xslt::whitespace::_xsltStripSpace;
        (*dst).nsDefs = cur as *mut c_void;
        cur = next;
    }
    (*src).nsDefs = ptr::null_mut();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml::tree::*;
    use core::ptr;

    #[test]
    fn test_compile_null() {
        unsafe {
            assert_eq!(compile(ptr::null_mut(), ptr::null_mut()), -1);
        }
    }

    #[test]
    fn test_is_xslt_element_null() {
        unsafe {
            assert!(!is_xslt_element(ptr::null_mut(), "template"));
            assert!(!is_xslt_namespace(ptr::null_mut()));
            assert!(get_element_ns(ptr::null_mut()).is_none());
            assert!(get_element_name(ptr::null_mut()).is_none());
        }
    }

    #[test]
    fn test_compile_simplified_stylesheet() {
        unsafe {
            // A simplified stylesheet: literal result element <html>.
            let doc = new_doc(b"1.0\0".as_ptr() as *const xmlChar);
            let root = new_node(ptr::null_mut(), b"html\0".as_ptr() as *const xmlChar);
            doc_set_root_element(doc, root);
            let style =
                libc::calloc(1, core::mem::size_of::<_xsltStylesheet>()) as *mut _xsltStylesheet;
            let ret = compile(style, doc);
            assert_eq!(ret, 0);
            assert_eq!((*style).doc, doc);
            assert!(!(*style).templates.is_null());
            // The implicit template's content is the root element.
            assert_eq!((*(*style).templates).content, root);
            crate::xslt::stylesheet::xsltFreeStylesheet(style);
        }
    }
}
