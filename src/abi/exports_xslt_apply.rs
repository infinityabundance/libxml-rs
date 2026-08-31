//! C ABI exports for libxslt.so.1 — the "apply" family (§16, Phase 8).
//!
//! This module implements the template-application and pattern-matching
//! entry points of the libxslt 1.1.45 C ABI:
//!
//! - Template application: `xsltApplyTemplates`, `xsltProcessOneNode`,
//!   `xsltApplyOneTemplate`, `xsltApplyImports`, `xsltCallTemplate`
//! - Attribute sets: `xsltApplyAttributeSet`,
//!   `xsltResolveStylesheetAttributeSet`, `xsltFreeAttributeSetsHashes`
//! - Template lookup/cleanup: `xsltGetTemplate`, `xsltTemplateProcess`,
//!   `xsltNextImport`, `xsltCleanupTemplates`, `xsltFreeTemplateHashes`
//! - Whitespace stripping: `xsltApplyStripSpaces`,
//!   `xsltNeedElemSpaceHandling`, `xsltFindElemSpaceHandling`
//! - Compiled-pattern (`xsltCompMatch`) API: `xsltCompilePattern`,
//!   `xsltCompMatchClearCache`, `xsltFreeCompMatchList`,
//!   `xsltTestCompMatchList`
//!
//! Where the native-Rust XSLT engine in `src/xslt/*` already implements the
//! upstream behaviour (the engine is oracle-tested through the xsltproc
//! CLI), the exports below are wired to it; the rest are faithful ports of
//! the upstream C sources in `archaeology/libxslt-git/libxslt/`.

#![allow(non_snake_case)]
#![allow(unused_variables)]

use core::ptr;
use std::os::raw::{c_int, c_void};

use crate::abi::structs::*;
use crate::abi::types::xmlElementType::*;
use crate::abi::types::*;

// ═══════════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Compare two NUL-terminated xmlChar strings for equality.
unsafe fn xml_chars_equal(a: *const xmlChar, b: *const xmlChar) -> bool {
    if a.is_null() || b.is_null() {
        return false;
    }
    libc::strcmp(a as *const libc::c_char, b as *const libc::c_char) == 0
}

/// IS_XSLT_REAL_NODE (xsltutils.h): document, element, text, cdata,
/// attribute, comment or PI node.
unsafe fn is_real_node(node: *mut _xmlNode) -> bool {
    if node.is_null() {
        return false;
    }
    let t = (*node).type_;
    t == XML_ELEMENT_NODE as c_int
        || t == XML_TEXT_NODE as c_int
        || t == XML_CDATA_SECTION_NODE as c_int
        || t == XML_ATTRIBUTE_NODE as c_int
        || t == XML_DOCUMENT_NODE as c_int
        || t == XML_HTML_DOCUMENT_NODE as c_int
        || t == XML_COMMENT_NODE as c_int
        || t == XML_PI_NODE as c_int
}

/// IS_BLANK_NODE: a text node whose content is only XML whitespace.
unsafe fn is_blank_node(node: *mut _xmlNode) -> bool {
    if node.is_null() || (*node).type_ != XML_TEXT_NODE as c_int {
        return false;
    }
    let content = (*node).content;
    if content.is_null() {
        return false;
    }
    let mut p = content;
    while *p != 0 {
        if !matches!(*p, b' ' | b'\t' | b'\n' | b'\r') {
            return false;
        }
        p = p.add(1);
    }
    true
}

/// Match a strip/preserve-space name test against an element name.
///
/// Upstream (imports.c `xsltFindElemSpaceHandling`) hashes on
/// `(node->name, node->ns->href)` and supports the `"*"` wildcard; the
/// candidate's compiler (`compile_space_rules`) stores only the raw name
/// token from the `elements` attribute, so matching is lexical: `"*"` or
/// exact string equality against the node's name.
unsafe fn space_name_matches(pattern: *const xmlChar, name: *const xmlChar) -> bool {
    if pattern.is_null() || name.is_null() {
        return false;
    }
    if *pattern == b'*' && *pattern.add(1) == 0 {
        return true;
    }
    libc::strcmp(pattern as *const libc::c_char, name as *const libc::c_char) == 0
}

/// Find the highest-priority template in one stylesheet's template list
/// that matches `node` in the given mode.
///
/// This is the per-stylesheet search upstream performs inside
/// `xsltGetTemplate` (pattern.c 1.1.45): only templates carrying a
/// compiled match pattern are considered (the candidate carries it in
/// `templ->params`, see `compile_template`), mode compatibility follows
/// XSLT 1.0 §5.2, and the pattern is tested with `xsltTestPattern`, which
/// evaluates predicates against `ctxt` (upstream tests with the runtime
/// context as well). Priority ties are broken by import depth (`position`).
unsafe fn best_template_in_style(
    ctxt: *mut _xsltTransformContext,
    style: *mut _xsltStylesheet,
    node: *mut _xmlNode,
    mode: *const xmlChar,
) -> *mut _xsltTemplate {
    let mut best: *mut _xsltTemplate = ptr::null_mut();
    let mut best_priority: f64 = f64::NEG_INFINITY;
    let mut best_depth: c_int = -1;
    let mut templ = (*style).templates;
    while !templ.is_null() {
        // Named-only templates (no match attribute) carry no compiled
        // pattern and never match a node.
        if (*templ).params.is_null() {
            templ = (*templ).next;
            continue;
        }
        // ── Mode compatibility (XSLT 1.0 §5.2) ────────────────────────
        let has_mode = !(*templ).mode.is_null();
        if has_mode {
            if mode.is_null() || !xml_chars_equal((*templ).mode, mode) {
                templ = (*templ).next;
                continue;
            }
        } else if !mode.is_null() {
            templ = (*templ).next;
            continue;
        }
        // ── Pattern test ──────────────────────────────────────────────
        let pattern_ptr = (*templ).params as *mut crate::xslt::patterns::_xsltPattern;
        if crate::xslt::patterns::xsltTestPattern(ctxt, pattern_ptr, node) == 0 {
            templ = (*templ).next;
            continue;
        }
        // ── Priority ──────────────────────────────────────────────────
        // The compiler stores the (possibly default) priority on the
        // template; XSLT_PAT_NO_PRIORITY falls back to 0.5, mirroring the
        // engine's xsltFindTemplate.
        let raw = (*templ).priority as f64;
        let priority = if raw != crate::xslt::patterns::XSLT_PAT_NO_PRIORITY {
            raw
        } else {
            0.5
        };
        let depth = (*templ).position;
        if best.is_null()
            || priority > best_priority
            || ((priority - best_priority).abs() < f64::EPSILON && depth > best_depth)
        {
            best = templ;
            best_priority = priority;
            best_depth = depth;
        }
        templ = (*templ).next;
    }
    best
}

// ═══════════════════════════════════════════════════════════════════════════════
// Template application (transform.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// Process an `xsl:apply-templates` instruction on the current node.
///
/// Wired to the engine's `process_apply_templates`, which re-derives the
/// `select`/`mode` attributes and `xsl:with-param`/`xsl:sort` children from
/// the instruction node at run time (the candidate does not populate
/// `xsltElemPreComp` structs, so `castedComp` is ignored; upstream uses it
/// only to retrieve the precompiled `select`/`mode`/`with-param` data).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void
/// xsltApplyTemplates(xsltTransformContextPtr ctxt, xmlNodePtr node,
///                    xmlNodePtr inst, xsltElemPreCompPtr castedComp);
/// ```
///
/// # SAFETY
///
/// - `ctxt`, `node` and `inst` must be valid pointers.
#[no_mangle]
pub unsafe extern "C" fn xsltApplyTemplates(
    ctxt: *mut _xsltTransformContext,
    node: *mut _xmlNode,
    inst: *mut _xmlNode,
    castedComp: *mut c_void,
) {
    let _ = castedComp;
    if ctxt.is_null() || node.is_null() || inst.is_null() {
        return;
    }
    // The engine evaluates `select`/`mode`/`with-param` relative to the
    // context node stored in `ctxt->node` (upstream sets the current node
    // before processing each selected source node).
    (*ctxt).node = node;
    crate::xslt::transform::process_apply_templates(ctxt, inst);
}

/// Process a single source node: find the matching template and apply it.
///
/// If no template matches, the engine's `apply_templates_to_node` applies
/// the built-in template rules (XSLT 1.0 §5.8), which is what upstream's
/// `xsltDefaultProcessOneNode` fallback does. Parameters are pushed onto
/// the variable stack for the duration of the template instantiation.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void
/// xsltProcessOneNode(xsltTransformContextPtr ctxt, xmlNodePtr contextNode,
///                    xsltStackElemPtr withParams);
/// ```
///
/// # SAFETY
///
/// - `ctxt` and `contextNode` must be valid pointers; `withParams` may be
///   NULL.
#[no_mangle]
pub unsafe extern "C" fn xsltProcessOneNode(
    ctxt: *mut _xsltTransformContext,
    contextNode: *mut _xmlNode,
    withParams: *mut _xsltStackElem,
) {
    if ctxt.is_null() || contextNode.is_null() {
        return;
    }
    // Upstream resolves the template with `ctxt->mode`/`ctxt->modeURI`;
    // the candidate's mode matching lives on the engine side, so pass the
    // current mode through.
    crate::xslt::transform::apply_templates_with_params(
        ctxt,
        contextNode,
        (*ctxt).mode,
        withParams,
    );
}

/// Process a sequence constructor (`list`) on the current node, pushing
/// `params` onto the variable stack for the duration and popping them
/// afterwards without freeing them.
///
/// Wired to the engine's `execute_content` (the candidate's equivalent of
/// `xsltApplySequenceConstructor`). `templ` is unused — upstream marks it
/// `ATTRIBUTE_UNUSED` as well.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void
/// xsltApplyOneTemplate(xsltTransformContextPtr ctxt, xmlNodePtr contextNode,
///                      xmlNodePtr list, xsltTemplatePtr templ ATTRIBUTE_UNUSED,
///                      xsltStackElemPtr params);
/// ```
///
/// # SAFETY
///
/// - `ctxt` and `list` must be valid pointers; `params` may be NULL.
#[allow(clippy::while_immutable_condition)]
#[no_mangle]
pub unsafe extern "C" fn xsltApplyOneTemplate(
    ctxt: *mut _xsltTransformContext,
    contextNode: *mut _xmlNode,
    list: *mut _xmlNode,
    templ: *mut _xsltTemplate,
    params: *mut _xsltStackElem,
) {
    let _ = templ;
    if ctxt.is_null() || list.is_null() {
        return;
    }
    // CHECK_STOPPED
    if (*ctxt).state == crate::xslt::transform::XSLT_STATE_STOPPED {
        return;
    }
    (*ctxt).node = contextNode;
    // Push the given xsl:param(s) onto the variable stack. Note that the
    // engine's xsltPushVariable rewrites each element's `next` link (it
    // chains onto the stack head), so the chain is walked by saving the
    // link before pushing.
    let old_vars_nr = (*ctxt).varsNr;
    let mut p = params;
    while !p.is_null() {
        let next = (*p).next;
        crate::xslt::parameters::xsltPushParam(ctxt, p);
        p = next;
    }
    // Instantiate the sequence constructor.
    crate::xslt::transform::execute_content(ctxt, list);
    // Pop the xsl:param(s) again but don't free them (upstream
    // xsltLocalVariablePop(ctxt, oldVarsNr, -2)).
    while (*ctxt).varsNr > old_vars_nr {
        crate::xslt::parameters::xsltPopParam(ctxt);
    }
}

/// Process an `xsl:apply-imports` instruction.
///
/// Wired to the engine's `process_apply_imports`, which applies the next
/// template in import precedence order that matches the current node
/// (skipping templates at the same or higher import precedence as the
/// current template rule). The engine reads the current node and current
/// template rule from `ctxt->node` / `ctxt->templ`, so the context node is
/// set first. `comp` is ignored (no precompiled instruction data in the
/// candidate).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void
/// xsltApplyImports(xsltTransformContextPtr ctxt, xmlNodePtr contextNode,
///                  xmlNodePtr inst, xsltElemPreCompPtr comp ATTRIBUTE_UNUSED);
/// ```
///
/// # SAFETY
///
/// - `ctxt` and `inst` must be valid pointers.
#[no_mangle]
pub unsafe extern "C" fn xsltApplyImports(
    ctxt: *mut _xsltTransformContext,
    contextNode: *mut _xmlNode,
    inst: *mut _xmlNode,
    comp: *mut c_void,
) {
    let _ = comp;
    if ctxt.is_null() || inst.is_null() {
        return;
    }
    (*ctxt).node = contextNode;
    crate::xslt::transform::process_apply_imports(ctxt, inst);
}

/// Process an `xsl:call-template` instruction.
///
/// Wired to the engine's `process_call_template` (which resolves the named
/// template, collects and pushes `xsl:with-param` values, and instantiates
/// the template body). `castedComp` is ignored — the engine looks the
/// template up by the `name` attribute at run time.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void
/// xsltCallTemplate(xsltTransformContextPtr ctxt, xmlNodePtr node,
///                  xmlNodePtr inst, xsltElemPreCompPtr castedComp);
/// ```
///
/// # SAFETY
///
/// - `ctxt`, `node` and `inst` must be valid pointers.
#[no_mangle]
pub unsafe extern "C" fn xsltCallTemplate(
    ctxt: *mut _xsltTransformContext,
    node: *mut _xmlNode,
    inst: *mut _xmlNode,
    castedComp: *mut c_void,
) {
    let _ = castedComp;
    if ctxt.is_null() || node.is_null() || inst.is_null() {
        return;
    }
    // Upstream: a call-template is only meaningful while a result tree is
    // being constructed.
    if (*ctxt).insert.is_null() {
        return;
    }
    (*ctxt).node = node;
    crate::xslt::transform::process_call_template(ctxt, inst);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Attribute sets (attributes.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// Apply the attribute sets named in `attrSets` (a whitespace-separated
/// list of QNames) to the current result element.
///
/// If `attrSets` is NULL the value is extracted from `inst` when `inst` is
/// an attribute node (upstream reads the attribute value from the node's
/// text child). The lookup/application is delegated to the engine's
/// `xsltApplyAttrSets`.
///
/// Simplified relative to upstream: the candidate's compiler stores
/// attribute-set names as raw QName tokens (no prefix/namespace
/// resolution), so the name comparison is lexical rather than
/// namespace-aware, and invalid-QName validation is not performed. This
/// matches the candidate's compile-time representation.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void
/// xsltApplyAttributeSet(xsltTransformContextPtr ctxt, xmlNodePtr node,
///                       xmlNodePtr inst, const xmlChar *attrSets);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be a valid pointer; `node`, `inst` and `attrSets` may be
///   NULL (see upstream's extraction rules).
#[no_mangle]
pub unsafe extern "C" fn xsltApplyAttributeSet(
    ctxt: *mut _xsltTransformContext,
    node: *mut _xmlNode,
    inst: *mut _xmlNode,
    attrSets: *const xmlChar,
) {
    if ctxt.is_null() {
        return;
    }
    let mut sets = attrSets;
    if sets.is_null() {
        // Extract the value from @inst (only meaningful for an attribute
        // node; upstream leaves it NULL for element nodes and does nothing).
        if inst.is_null() {
            return;
        }
        if (*inst).type_ == XML_ATTRIBUTE_NODE as c_int {
            let attr = inst as *mut _xmlAttr;
            if !(*attr).children.is_null() {
                sets = (*(*attr).children).content;
            }
        }
        if sets.is_null() {
            return;
        }
    }
    crate::xslt::attributes::xsltApplyAttrSets(ctxt, node, sets);
}

/// Resolve the `use-attribute-sets` references of a stylesheet.
///
/// Upstream (attributes.c 1.1.45) walks the stylesheet and its imports,
/// merges referenced attribute sets into the referencing ones at compile
/// time, and moves every imported stylesheet's sets into the top
/// stylesheet's hash so apply-time lookup only consults the top level.
///
/// The candidate keeps attribute sets as a per-stylesheet linked list
/// keyed by name and resolves references at apply time (`xsltApplyAttrSets`
/// looks each referenced set up by name), so the compile-time work needed
/// here is the migration only: imported sets must live on the top
/// stylesheet's list or apply-time lookup (which consults only
/// `style->attributeSets`) will miss them. Sets are moved so that
/// higher-precedence stylesheets (earlier in the `xsltNextImport` walk)
/// end up closer to the list head, preserving "higher import precedence
/// wins" for duplicate names.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void
/// xsltResolveStylesheetAttributeSet(xsltStylesheetPtr style);
/// ```
///
/// # SAFETY
///
/// - `style` must be a valid `_xsltStylesheet`, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xsltResolveStylesheetAttributeSet(style: *mut _xsltStylesheet) {
    if style.is_null() {
        return;
    }
    // Collect the stylesheets that own attribute sets, in import walk
    // order (decreasing import precedence).
    let mut visited: Vec<*mut _xsltStylesheet> = Vec::new();
    let mut cur = xsltNextImport(style);
    while !cur.is_null() {
        if !(*cur).attributeSets.is_null() {
            visited.push(cur);
        }
        cur = xsltNextImport(cur);
    }
    // Move the lists, lowest precedence first, so that the highest
    // precedence sets end up at the head of the top stylesheet's list.
    for st in visited.iter().rev() {
        // Detach the whole list first (upstream frees the imported hash
        // entry after migrating it).
        let mut set = (*(*st)).attributeSets as *mut _xsltAttrSet;
        (*(*st)).attributeSets = ptr::null_mut();
        while !set.is_null() {
            let next = (*set).next;
            (*set).next = (*style).attributeSets as *mut _xsltAttrSet;
            (*style).attributeSets = set as *mut c_void;
            set = next;
        }
    }
}

/// Free the memory used by a stylesheet's attribute sets.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void
/// xsltFreeAttributeSetsHashes(xsltStylesheetPtr style);
/// ```
///
/// # SAFETY
///
/// - `style` must be a valid `_xsltStylesheet`, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xsltFreeAttributeSetsHashes(style: *mut _xsltStylesheet) {
    crate::xslt::attributes::xsltFreeAttrSets(style);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Template lookup / cleanup (pattern.c, templates.c, imports.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// Obsolete entry point (upstream templates.c): always returns NULL.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNodePtr *
/// xsltTemplateProcess(xsltTransformContextPtr ctxt ATTRIBUTE_UNUSED,
///                     xmlNodePtr node) {
///     if (node == NULL)
///         return(NULL);
///     return(0);
/// }
/// ```
///
/// # SAFETY
///
/// - `node` may be any pointer; it is only null-checked.
#[no_mangle]
pub const unsafe extern "C" fn xsltTemplateProcess(
    ctxt: *mut _xsltTransformContext,
    node: *mut _xmlNode,
) -> *mut *mut _xmlNode {
    let _ = ctxt;
    if node.is_null() {
        return ptr::null_mut();
    }
    ptr::null_mut()
}

/// Find the next stylesheet in import precedence.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xsltStylesheetPtr
/// xsltNextImport(xsltStylesheetPtr cur) {
///     if (cur == NULL)
///         return(NULL);
///     if (cur->imports != NULL)
///         return(cur->imports);
///     if (cur->next != NULL)
///         return(cur->next) ;
///     do {
///         cur = cur->parent;
///         if (cur == NULL) break;
///         if (cur->next != NULL) return(cur->next);
///     } while (cur != NULL);
///     return(cur);
/// }
/// ```
///
/// # SAFETY
///
/// - `cur` must be a valid `_xsltStylesheet` pointer, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xsltNextImport(cur: *mut _xsltStylesheet) -> *mut _xsltStylesheet {
    if cur.is_null() {
        return ptr::null_mut();
    }
    if !(*cur).imports.is_null() {
        return (*cur).imports;
    }
    if !(*cur).next.is_null() {
        return (*cur).next;
    }
    let mut c = cur;
    loop {
        c = (*c).parent;
        if c.is_null() {
            break;
        }
        if !(*c).next.is_null() {
            return (*c).next;
        }
    }
    ptr::null_mut()
}

/// Find the template applying to `node`, walking the import chain in
/// import-precedence order.
///
/// If `style` is NULL the search starts at `ctxt->style` (the ordinary
/// template resolution path). If `style` is non-NULL the search starts at
/// `xsltNextImport(style)` and excludes `style` itself — the
/// `xsl:apply-imports` path, which looks only at stylesheets with lower
/// import precedence than the current template rule's stylesheet.
///
/// Import precedence dominates priority: the first stylesheet in the walk
/// that contains a matching template wins (upstream returns immediately on
/// the first match per stylesheet). Within a stylesheet the highest-priority
/// matching template is selected (see [`best_template_in_style`]).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xsltTemplatePtr
/// xsltGetTemplate(xsltTransformContextPtr ctxt, xmlNodePtr node,
///                 xsltStylesheetPtr style);
/// ```
///
/// # SAFETY
///
/// - `ctxt` and `node` must be valid pointers; `style` may be NULL.
#[no_mangle]
pub unsafe extern "C" fn xsltGetTemplate(
    ctxt: *mut _xsltTransformContext,
    node: *mut _xmlNode,
    style: *mut _xsltStylesheet,
) -> *mut _xsltTemplate {
    if ctxt.is_null() || node.is_null() {
        return ptr::null_mut();
    }
    let stop = style;
    let mut curstyle = if style.is_null() {
        (*ctxt).style
    } else {
        xsltNextImport(style)
    };
    while !curstyle.is_null() && curstyle != stop {
        let best = best_template_in_style(ctxt, curstyle, node, (*ctxt).mode);
        if !best.is_null() {
            return best;
        }
        curstyle = xsltNextImport(curstyle);
    }
    ptr::null_mut()
}

/// Clean up the state of the templates used by the stylesheet.
///
/// # UPSTREAM-PARITY
///
/// Upstream 1.1.45 (pattern.c) has an empty body:
///
/// ```c
/// void
/// xsltCleanupTemplates(xsltStylesheetPtr style ATTRIBUTE_UNUSED) {
/// }
/// ```
///
/// # SAFETY
///
/// - `style` may be any pointer; it is unused.
#[no_mangle]
pub const unsafe extern "C" fn xsltCleanupTemplates(style: *mut _xsltStylesheet) {
    let _ = style;
}

/// Free the memory used by the `xsltAddTemplate`/`xsltGetTemplate`
/// mechanism (the template lookup hashes and generic match lists).
///
/// Upstream frees `templatesHash`, the `*Match` lists and `namedTemplates`;
/// the template list itself is freed separately by `xsltFreeTemplates`
/// (called by `xsltFreeStylesheet`), so this function must not touch
/// `style->templates`. The candidate's compiler registers templates on the
/// `templates` linked list and never populates the hash/match fields, so in
/// the candidate these fields are NULL and the frees below are defensive;
/// they mirror upstream's structure exactly.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void
/// xsltFreeTemplateHashes(xsltStylesheetPtr style);
/// ```
///
/// # SAFETY
///
/// - `style` must be a valid `_xsltStylesheet`, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xsltFreeTemplateHashes(style: *mut _xsltStylesheet) {
    if style.is_null() {
        return;
    }
    if !(*style).templatesHash.is_null() {
        crate::xml::hash::hash_free(
            (*style).templatesHash as *mut crate::xml::hash::HashTable,
            None,
        );
        (*style).templatesHash = ptr::null_mut();
    }
    // Generic (non name-keyed) match lists: rootMatch, keyMatch, elemMatch,
    // attrMatch, parentMatch, textMatch, piMatch, commentMatch.
    let mut match_fields = [
        (*style).rootMatch,
        (*style).keyMatch,
        (*style).elemMatch,
        (*style).attrMatch,
        (*style).parentMatch,
        (*style).textMatch,
        (*style).piMatch,
        (*style).commentMatch,
    ];
    for f in match_fields.iter_mut() {
        if !f.is_null() {
            xsltFreeCompMatchList(*f as xsltCompMatchPtr);
            *f = ptr::null_mut();
        }
    }
    if !(*style).namedTemplates.is_null() {
        crate::xml::hash::hash_free(
            (*style).namedTemplates as *mut crate::xml::hash::HashTable,
            None,
        );
        (*style).namedTemplates = ptr::null_mut();
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Whitespace stripping (transform.c, imports.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// Strip ignorable whitespace-only text nodes from the subtree rooted at
/// `node`, according to the stylesheet's strip/preserve-space rules.
///
/// Faithful port of the upstream node-level walk (transform.c 1.1.45),
/// including the per-style decision delegated to
/// [`xsltFindElemSpaceHandling`] (import precedence across the chain).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void
/// xsltApplyStripSpaces(xsltTransformContextPtr ctxt, xmlNodePtr node);
/// ```
///
/// # SAFETY
///
/// - `ctxt` and `node` must be valid pointers, or NULL (no-op).
#[no_mangle]
pub unsafe extern "C" fn xsltApplyStripSpaces(
    ctxt: *mut _xsltTransformContext,
    node: *mut _xmlNode,
) {
    if ctxt.is_null() || node.is_null() {
        return;
    }
    let mut current = node;
    'walk: while !current.is_null() {
        // Cleanup blank children if this element's whitespace is to be
        // stripped.
        if is_real_node(current)
            && !(*current).children.is_null()
            && xsltFindElemSpaceHandling(ctxt, current) != 0
        {
            let mut cur = (*current).children;
            while !cur.is_null() {
                let next = (*cur).next;
                if is_blank_node(cur) {
                    crate::xml::tree::unlink_node(cur);
                    crate::xml::tree::free_node(cur);
                }
                cur = next;
            }
        }
        // Skip to the next node in document order.
        if (*node).type_ == XML_ENTITY_REF_NODE as c_int {
            // Process deep inside entities (upstream recurses on the root
            // argument's children).
            xsltApplyStripSpaces(ctxt, (*node).children);
        }
        if !(*current).children.is_null() && (*current).type_ != XML_ENTITY_REF_NODE as c_int {
            current = (*current).children;
        } else if !(*current).next.is_null() {
            current = (*current).next;
        } else {
            loop {
                current = (*current).parent;
                if current.is_null() {
                    break;
                }
                if current == node {
                    break 'walk;
                }
                if !(*current).next.is_null() {
                    current = (*current).next;
                    break;
                }
            }
        }
    }
}

/// Check whether the stylesheet (or any of its imports) has strip-space
/// rules requiring whitespace handling.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int
/// xsltNeedElemSpaceHandling(xsltTransformContextPtr ctxt) {
///     xsltStylesheetPtr style;
///     if (ctxt == NULL)
///         return(0);
///     style = ctxt->style;
///     while (style != NULL) {
///         if (style->stripSpaces != NULL)
///             return(1);
///         style = xsltNextImport(style);
///     }
///     return(0);
/// }
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be a valid pointer, or NULL (returns 0).
#[no_mangle]
pub unsafe extern "C" fn xsltNeedElemSpaceHandling(ctxt: *mut _xsltTransformContext) -> c_int {
    if ctxt.is_null() {
        return 0;
    }
    let mut style = (*ctxt).style;
    while !style.is_null() {
        if !(*style).stripSpaces.is_null() {
            return 1;
        }
        style = xsltNextImport(style);
    }
    0
}

/// Find strip-space or preserve-space information for an element, walking
/// the import chain in import-precedence order.
///
/// Returns 1 if whitespace-only children should be stripped, 0 if
/// preserved. Within a stylesheet the decision mirrors the engine's
/// `xsltShouldStripSpace`: the deepest-matching strip rule wins over a
/// same-depth preserve rule, and preserve wins ties (the candidate stores
/// strip and preserve rules in separate lists; upstream stores both values
/// in one hash where the last declaration wins — see the note in
/// `src/xslt/whitespace/mod.rs`).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int
/// xsltFindElemSpaceHandling(xsltTransformContextPtr ctxt, xmlNodePtr node);
/// ```
///
/// # SAFETY
///
/// - `ctxt` and `node` must be valid pointers, or NULL (returns 0).
#[no_mangle]
pub unsafe extern "C" fn xsltFindElemSpaceHandling(
    ctxt: *mut _xsltTransformContext,
    node: *mut _xmlNode,
) -> c_int {
    if ctxt.is_null() || node.is_null() {
        return 0;
    }
    let name = (*node).name;
    let mut style = (*ctxt).style;
    while !style.is_null() {
        // Best matching strip rule for this name (by import depth).
        let mut strip_depth = -1;
        let mut e = (*style).stripSpaces as *mut crate::xslt::whitespace::_xsltStripSpace;
        while !e.is_null() {
            if space_name_matches((*e).name, name) && (*e).depth > strip_depth {
                strip_depth = (*e).depth;
            }
            e = (*e).next;
        }
        // Best matching preserve rule (preserve-list head carried in the
        // unused nsDefs slot, candidate-internal).
        let mut preserve_depth = -1;
        let mut e = (*style).nsDefs as *mut crate::xslt::whitespace::_xsltStripSpace;
        while !e.is_null() {
            if space_name_matches((*e).name, name) && (*e).depth > preserve_depth {
                preserve_depth = (*e).depth;
            }
            e = (*e).next;
        }
        if strip_depth >= 0 || preserve_depth >= 0 {
            if strip_depth > preserve_depth {
                return 1;
            }
            return 0;
        }
        // Upstream fallbacks (never set by the candidate's compiler):
        // stripAll == 1 strips everything, -1 preserves everything.
        if (*style).stripAll == 1 {
            return 1;
        }
        if (*style).stripAll == -1 {
            return 0;
        }
        style = xsltNextImport(style);
    }
    0
}

// ═══════════════════════════════════════════════════════════════════════════════
// Compiled patterns — xsltCompMatch API (pattern.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// Opaque compiled-pattern handle (`xsltCompMatch`).
///
/// Upstream `struct _xsltCompMatch` holds a compiled step list plus the
/// associated template and priority; callers only ever hold an opaque
/// pointer. The candidate wraps the engine's compiled `_xsltPattern`
/// (which already handles union patterns internally) behind this handle;
/// the layout is opaque to callers.
#[derive(Debug)]
#[repr(C)]
pub struct _xsltCompMatch {
    /// The candidate's compiled pattern (opaque to callers).
    pub pattern: *mut c_void,
}

/// Pointer to a compiled XSLT pattern (`xsltCompMatchPtr`).
pub type xsltCompMatchPtr = *mut _xsltCompMatch;

/// Compile an XSLT pattern into a list of precompiled form suitable for
/// fast matching.
///
/// Delegates to the engine's two-argument `xsltCompilePattern` (the extra
/// `node`/`style`/`runtime` parameters are unused by the candidate's
/// compiler) and wraps the result in an opaque `xsltCompMatch`. Returns
/// NULL on failure, like upstream. Upstream returns one list element per
/// union alternative; the engine's compiled pattern is a single object
/// that internally holds all alternatives, so the wrapper is always a
/// single element.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xsltCompMatchPtr
/// xsltCompilePattern(const xmlChar *pattern, xmlDocPtr doc,
///                    xmlNodePtr node, xsltStylesheetPtr style,
///                    xsltTransformContextPtr runtime);
/// ```
///
/// # SAFETY
///
/// - `pattern` must be a valid NUL-terminated string, or NULL; `doc` may
///   be NULL.
#[no_mangle]
pub unsafe extern "C" fn xsltCompilePattern(
    pattern: *const xmlChar,
    doc: *mut _xmlDoc,
    node: *mut _xmlNode,
    style: *mut _xsltStylesheet,
    runtime: *mut _xsltTransformContext,
) -> xsltCompMatchPtr {
    let _ = (node, style, runtime);
    let compiled = crate::xslt::patterns::xsltCompilePattern(pattern, doc);
    if compiled.is_null() {
        return ptr::null_mut();
    }
    let wrapper = crate::abi::allocator::xmlMallocZero(core::mem::size_of::<_xsltCompMatch>())
        as *mut _xsltCompMatch;
    if wrapper.is_null() {
        crate::xslt::patterns::xsltFreePattern(compiled);
        return ptr::null_mut();
    }
    (*wrapper).pattern = compiled as *mut c_void;
    wrapper
}

/// Clear the pattern match cache.
///
/// Upstream clears a per-context cache slot referenced from the compiled
/// match's first step (`comp->steps[0]` runtime-extras indices). The
/// candidate's compiled patterns are self-contained and keep no per-context
/// cache, so there is nothing to clear; the null checks are kept for
/// upstream parity.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void
/// xsltCompMatchClearCache(xsltTransformContextPtr ctxt, xsltCompMatchPtr comp);
/// ```
///
/// # SAFETY
///
/// - `ctxt` and `comp` may be any pointers; both are only null-checked.
#[no_mangle]
pub const unsafe extern "C" fn xsltCompMatchClearCache(
    ctxt: *mut _xsltTransformContext,
    comp: xsltCompMatchPtr,
) {
    if ctxt.is_null() || comp.is_null() {}
    // No per-context match cache in the candidate (see above).
}

/// Free up the memory allocated for a compiled pattern.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void
/// xsltFreeCompMatchList(xsltCompMatchPtr comp);
/// ```
///
/// # SAFETY
///
/// - `comp` must have been returned by `xsltCompilePattern` and not already
///   freed, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xsltFreeCompMatchList(comp: xsltCompMatchPtr) {
    if comp.is_null() {
        return;
    }
    if !(*comp).pattern.is_null() {
        crate::xslt::patterns::xsltFreePattern(
            (*comp).pattern as *mut crate::xslt::patterns::_xsltPattern,
        );
    }
    crate::abi::allocator::xmlFreeImpl(comp as *mut c_void);
}

/// Test whether a node matches a compiled pattern list.
///
/// Delegates to the engine's `xsltTestPattern`. Upstream returns -1 when
/// `ctxt` or `node` is NULL, 1 on the first matching pattern, 0 otherwise.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int
/// xsltTestCompMatchList(xsltTransformContextPtr ctxt, xmlNodePtr node,
///                       xsltCompMatchPtr comp);
/// ```
///
/// # SAFETY
///
/// - `ctxt` and `node` must be valid pointers; `comp` may be NULL.
#[no_mangle]
pub unsafe extern "C" fn xsltTestCompMatchList(
    ctxt: *mut _xsltTransformContext,
    node: *mut _xmlNode,
    comp: xsltCompMatchPtr,
) -> c_int {
    if ctxt.is_null() || node.is_null() {
        return -1;
    }
    if comp.is_null() {
        return 0;
    }
    crate::xslt::patterns::xsltTestPattern(
        ctxt,
        (*comp).pattern as *mut crate::xslt::patterns::_xsltPattern,
        node,
    )
}
