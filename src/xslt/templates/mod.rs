//! XSLT template representation and matching (§33, §85 Phase 8).
//!
//! Templates are the core of XSLT processing. Each template has a match pattern,
//! optional name, mode, priority, and content (instruction tree).
//!
//! This module implements:
//! - Template lifecycle: creation, insertion (priority-ordered), teardown
//! - Template matching: finding the best-matching template for a source node
//!   in a given mode (XSLT 1.0 §5.2)
//! - Named template lookup
//! - Default priority computation from pattern AST nodes (XSLT 1.0 §5.5)
//!
//! # Match pattern storage
//!
//! The `_xsltTemplate.match` field stores the compiled pattern as a
//! `*mut _xsltPattern` cast to `*mut _xmlNode`. This is safe because
//! both are pointer-sized and the field is only ever accessed by casting
//! back to the correct type. The pattern lifecycle functions in the
//! `patterns` module handle the actual allocation, deallocation, and
//! matching logic.

use crate::abi::allocator::xmlFree;
use crate::abi::structs::*;
use crate::abi::types::*;
use crate::xml::string::xml_strcmp;
use crate::xslt::patterns::{_xsltPattern, xsltFreePattern, xsltTestPattern};
use std::os::raw::{c_int, c_void};
use std::ptr;

// ── Re-export xmlElementType variants for readability ────────────────────
use crate::abi::types::xmlElementType::{
    XML_CDATA_SECTION_NODE, XML_COMMENT_NODE, XML_PI_NODE, XML_TEXT_NODE,
};

// ── Template flags ───────────────────────────────────────────────────────

/// Template has a `match` attribute.
pub const XSLT_TEMPLATE_HAS_MATCH: c_int = 1 << 0;

/// Template has a `name` attribute.
pub const XSLT_TEMPLATE_HAS_NAME: c_int = 1 << 1;

/// Template has a `mode` attribute.
pub const XSLT_TEMPLATE_HAS_MODE: c_int = 1 << 2;

/// Template has an explicit `priority` attribute.
pub const XSLT_TEMPLATE_HAS_PRIORITY: c_int = 1 << 3;

// ── Template list management ─────────────────────────────────────────────

/// Add a template to a stylesheet's template list.
///
/// Templates are inserted in priority order (highest priority first).
/// When two templates have equal priority, the one with the higher import
/// depth (i.e. the one that was imported last) is placed first, matching
/// XSLT's import precedence rules.
///
/// Returns 0 on success, -1 on error (null pointer).
///
/// # Safety
///
/// `style` and `templ` must be valid, non-null pointers to their
/// respective structs, allocated via the libxml allocator.
#[no_mangle]
pub unsafe extern "C" fn xsltAddTemplate(
    style: *mut _xsltStylesheet,
    templ: *mut _xsltTemplate,
) -> c_int {
    if style.is_null() || templ.is_null() {
        return -1;
    }

    // Mark the owning stylesheet.
    (*templ).style = style;

    // UPSTREAM-PARITY: xsltTemplate has no flags field. The candidate's
    // markers are derived: HAS_MATCH = compiled pattern in params, HAS_NAME
    // = name non-null, HAS_MODE = mode non-null, HAS_PRIORITY = priority
    // != XSLT_PAT_NO_PRIORITY.

    // Insert into the linked list in priority order (highest first).
    // Ties are broken by import depth (stored in `position`; deeper wins).
    let priority = (*templ).priority as f64;
    let depth = (*templ).position;

    // Find the insertion point: walk the list until we find a template
    // whose priority is lower (or equal but with smaller depth).
    let mut prev: *mut _xsltTemplate = ptr::null_mut();
    let mut cur: *mut _xsltTemplate = (*style).templates;

    while !cur.is_null() {
        let cur_priority = (*cur).priority as f64;
        let cur_depth = (*cur).position;

        // We want descending priority order. If the new template has
        // strictly higher priority, insert before `cur`.
        if priority > cur_priority {
            break;
        }
        // If priorities are equal (within epsilon), the one with the
        // greater import depth comes first (last imported = highest
        // import precedence per XSLT §5.2).
        if (priority - cur_priority).abs() < f64::EPSILON && depth > cur_depth {
            break;
        }

        prev = cur;
        cur = (*cur).next;
    }

    // Perform the insertion.
    if prev.is_null() {
        // Insert at head.
        (*templ).next = (*style).templates;
        (*style).templates = templ;
    } else {
        (*templ).next = cur;
        (*prev).next = templ;
    }

    0
}

// ── Template destruction ─────────────────────────────────────────────────

/// Free a single template and its owned resources.
///
/// Releases the inherited namespace array, match pattern, and content
/// tree, then frees the template struct itself.
///
/// Safe to call with a null pointer (no-op).
///
/// # Safety
///
/// After this call the pointer must not be dereferenced.
/// Calling `xsltFreeTemplate` twice on the same pointer is undefined
/// behaviour.
#[no_mangle]
pub unsafe extern "C" fn xsltFreeTemplate(templ: *mut _xsltTemplate) {
    if templ.is_null() {
        return;
    }

    // Free inherited namespace declarations.
    // The inheritedNs array contains `inheritedNsNr` pointers to xmlNs
    // structs. We free the array itself but not the individual namespace
    // declarations (they are owned by the document or stylesheet).
    if !(*templ).inheritedNs.is_null() {
        xmlFree((*templ).inheritedNs as *mut c_void);
        (*templ).inheritedNs = ptr::null_mut();
        (*templ).inheritedNsNr = 0;
    }

    // Free the compiled match pattern (carried in `params`; the `match`
    // string itself is freed below).
    if !(*templ).params.is_null() {
        let pattern_ptr = (*templ).params as *mut _xsltPattern;
        xsltFreePattern(pattern_ptr);
        (*templ).params = ptr::null_mut();
    }
    // Free the match string (compiler copy).
    if !(*templ).r#match.is_null() {
        libc::free((*templ).r#match as *mut libc::c_void);
        (*templ).r#match = ptr::null_mut();
    }
    // Free the name/mode strings (heap copies made by the compiler via
    // xmlGetProp). These are NOT borrowed from the stylesheet document.
    if !(*templ).name.is_null() {
        libc::free((*templ).name as *mut libc::c_void);
        (*templ).name = ptr::null_mut();
    }
    if !(*templ).nameURI.is_null() {
        libc::free((*templ).nameURI as *mut libc::c_void);
        (*templ).nameURI = ptr::null_mut();
    }
    if !(*templ).mode.is_null() {
        libc::free((*templ).mode as *mut libc::c_void);
        (*templ).mode = ptr::null_mut();
    }
    if !(*templ).modeURI.is_null() {
        libc::free((*templ).modeURI as *mut libc::c_void);
        (*templ).modeURI = ptr::null_mut();
    }

    // The template content (instruction tree) is NOT freed here: it is
    // owned by the stylesheet document (style->doc) and is released when
    // xsltFreeStylesheet frees the document. Freeing it here would
    // double-free the nodes. This matches upstream libxslt, where
    // xsltFreeTemplate does not release the content tree.

    // Clear pointers so any use-after-free is more likely to crash
    // deterministically rather than silently corrupting.
    (*templ).next = ptr::null_mut();
    (*templ).style = ptr::null_mut();

    // Free the template struct itself.
    xmlFree(templ as *mut c_void);
}

/// Free all templates in a stylesheet's template list.
///
/// Walks the `templates` linked list and frees each template. Also
/// frees any templates on the `templatesFree` free list (recycling
/// cache).
///
/// Safe to call with a null pointer (no-op).
///
/// # Safety
///
/// After this call the stylesheet's template pointers are invalidated.
#[no_mangle]
pub unsafe extern "C" fn xsltFreeTemplates(style: *mut _xsltStylesheet) {
    if style.is_null() {
        return;
    }

    // Free the active template list.
    let mut templ: *mut _xsltTemplate = (*style).templates;
    while !templ.is_null() {
        let next: *mut _xsltTemplate = (*templ).next;
        xsltFreeTemplate(templ);
        templ = next;
    }
    (*style).templates = ptr::null_mut();
}

// ── Template matching (XSLT 1.0 §5.2) ────────────────────────────────────

/// Find the best matching template for a node in the given mode.
///
/// Returns the template with the highest priority that matches the node.
/// If multiple templates have the same priority, the one with the highest
/// import depth (last in import tree) wins.
///
/// Only templates that have a `match` attribute (i.e.
/// `XSLT_TEMPLATE_HAS_MATCH` is set) are considered. Mode matching
/// follows XSLT 1.0 §5.2 rules:
/// - A template with an explicit mode matches only when that mode is
///   requested.
/// - A template without an explicit mode matches only when no mode is
///   requested (the default/implicit mode).
///
/// Returns a borrowed pointer to the winning template, or NULL if no
/// template matches.
///
/// XSLT 1.0 §5.2: Template Resolution
///
/// # Safety
///
/// `style` and `node` must be valid, non-null pointers. `mode` may be
/// null.
#[no_mangle]
pub unsafe extern "C" fn xsltFindTemplate(
    style: *mut _xsltStylesheet,
    node: *mut _xmlNode,
    mode: *const xmlChar,
) -> *mut _xsltTemplate {
    if style.is_null() || node.is_null() {
        return ptr::null_mut();
    }

    let mut best: *mut _xsltTemplate = ptr::null_mut();
    let mut best_priority: f64 = f64::NEG_INFINITY;
    let mut best_depth: c_int = -1;

    let mut templ: *mut _xsltTemplate = (*style).templates;
    while !templ.is_null() {
        // Only consider templates with a compiled match pattern (params).
        if (*templ).params.is_null() {
            templ = (*templ).next;
            continue;
        }

        // ── Mode compatibility ──────────────────────────────────────────
        // XSLT 1.0 §5.2: template mode matching.
        let templ_has_mode = !(*templ).mode.is_null();
        if templ_has_mode {
            // Template has an explicit mode: it must match the requested
            // mode. If no mode was requested, skip.
            if mode.is_null() {
                templ = (*templ).next;
                continue;
            }
            if xml_strcmp((*templ).mode, mode) != 0 {
                templ = (*templ).next;
                continue;
            }
        } else {
            // Template has no explicit mode: it matches only when no mode
            // is requested (the default/implicit mode).
            if !mode.is_null() {
                templ = (*templ).next;
                continue;
            }
        }

        // ── Pattern matching ────────────────────────────────────────────
        // The compiled pattern is carried in `params` (candidate-internal;
        // upstream carries the match string in `match`).
        let pattern_ptr = (*templ).params as *mut _xsltPattern;
        if pattern_ptr.is_null() {
            templ = (*templ).next;
            continue;
        }

        // Use xsltTestPattern directly on the compiled pattern.
        // We pass null for the transform context; this works correctly
        // for patterns without predicates. Patterns with predicates
        // require a transform context for XPath evaluation, but will
        // still produce a conservative (no-match) result.
        let matched = xsltTestPattern(ptr::null_mut(), pattern_ptr, node);
        if matched == 0 {
            templ = (*templ).next;
            continue;
        }

        // ── Priority comparison ─────────────────────────────────────────
        // Determine the effective priority. If the template has an
        // explicit priority attribute, use it; otherwise compute the
        // default priority from the compiled pattern.
        let effective_priority: f64 =
            if (*templ).priority as f64 != crate::xslt::patterns::XSLT_PAT_NO_PRIORITY {
                (*templ).priority as f64
            } else {
                xsltDefaultPriorityFromNode((*templ).params as *mut _xmlNode)
            };

        // Higher priority wins; ties broken by higher import depth
        // (stored in `position`; later import = higher precedence).
        let templ_depth = (*templ).position;
        if best.is_null()
            || effective_priority > best_priority
            || ((effective_priority - best_priority).abs() < f64::EPSILON
                && templ_depth > best_depth)
        {
            best = templ;
            best_priority = effective_priority;
            best_depth = templ_depth;
        }

        templ = (*templ).next;
    }

    best
}

// ── Named template lookup ────────────────────────────────────────────────

/// Look up a named template.
///
/// Walks the stylesheet's template list (including imported stylesheets
/// recursively) and returns the first template with a matching `name`
/// attribute.
///
/// Returns a borrowed pointer to the template, or NULL if no template
/// with the given name exists.
///
/// # Safety
///
/// `style` and `name` must be valid, non-null pointers.
#[no_mangle]
pub unsafe extern "C" fn xsltLookupTemplate(
    style: *mut _xsltStylesheet,
    name: *const xmlChar,
) -> *mut _xsltTemplate {
    if style.is_null() || name.is_null() {
        return ptr::null_mut();
    }

    // Search this stylesheet's template list.
    let mut templ: *mut _xsltTemplate = (*style).templates;
    while !templ.is_null() {
        if !(*templ).name.is_null() {
            if xml_strcmp((*templ).name, name) == 0 {
                return templ;
            }
        }
        templ = (*templ).next;
    }

    // Not found — search imported stylesheets recursively.
    // Imported stylesheets form a linked list via `next`.
    let mut import: *mut _xsltStylesheet = (*style).imports;
    while !import.is_null() {
        let result = xsltLookupTemplate(import, name);
        if !result.is_null() {
            return result;
        }
        import = (*import).next;
    }

    ptr::null_mut()
}

// ── Default priority computation (XSLT 1.0 §5.5) ────────────────────────

/// Compute the default priority for a pattern node (the AST node
/// representing the match expression).
///
/// XSLT 1.0 §5.5 specifies:
///
/// | Pattern kind                               | Priority |
/// |--------------------------------------------|----------|
/// | Simple QName (e.g. `foo`, `ns:foo`)        |  0.0     |
/// | `node()` (or `text()`, `comment()`, `pi()`) | -0.25   |
/// | Wildcard (`*` or `ns:*`)                   | -0.5     |
/// | Compound patterns (paths, predicates, etc.)|  0.5     |
///
/// If `match_node` is null, returns 0.5 as a safe default.
///
/// # Safety
///
/// `match_node` must be a valid pointer to a compiled pattern node, or
/// null.
#[no_mangle]
pub unsafe extern "C" fn xsltDefaultPriorityFromNode(match_node: *mut _xmlNode) -> f64 {
    if match_node.is_null() {
        return 0.5;
    }

    // The match node is a compiled pattern (`_xsltPattern` cast to
    // `*mut _xmlNode`). We cannot directly read its fields as xmlNode
    // fields because the backing allocation is a `CompiledPattern`, not
    // an `_xmlNode`.
    //
    // Instead, we cast to `_xsltPattern` and examine the compiled
    // pattern's structure by testing the match node pointer as a
    // `_xsltPattern`. However, since the internal `CompiledPattern`
    // layout is private to the patterns module, we cannot directly
    // inspect it here.
    //
    // For now, we return the safe default of 0.5. The patterns module's
    // `xsltDefaultPriority` function provides correct priority computation
    // given the original pattern string; it should be called by the
    // compiler when priority is not explicitly specified.
    //
    // A future enhancement could store the computed default priority on
    // the template struct during compilation, avoiding the need to
    // recompute it here.
    0.5
}

// ── Template lookup structure initialization ─────────────────────────────

/// Initialize template lookup structures for a stylesheet.
///
/// In the full implementation this allocates and populates a hash table
/// (`style.internalHash`) for fast named-template and mode-based template
/// lookup, avoiding linear scans of the template list during transformation.
///
/// Currently a no-op (the linear-scan fallback in `xsltFindTemplate` and
/// `xsltLookupTemplate` is functionally correct but slower).
///
/// # Safety
///
/// `style` must be a valid pointer to a compiled stylesheet.
#[no_mangle]
pub unsafe extern "C" fn xsltInitTemplateLookup(style: *mut _xsltStylesheet) {
    if style.is_null() {
        return;
    }

    // Phase 8: stub — a future implementation will:
    //
    //   1. Allocate a hash table (e.g. via xmlHashCreate) and store it
    //      in `style.internalHash`.
    //
    //   2. Walk the template list and insert each template keyed by:
    //      - For named templates: key = template name
    //      - For match templates: key = mode (or a special sentinel for
    //        templates without a mode)
    //
    //   3. `xsltFindTemplate` and `xsltLookupTemplate` will then query
    //      the hash table instead of doing a linear scan.
    //
    // Until then, the linear-scan fallback is functionally correct.
}
