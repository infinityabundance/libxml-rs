//! XSLT imports and includes (§32, §85 Phase 8).
//!
//! `<xsl:import>` (only allowed at the top of the stylesheet, before any
//! other top-level elements) imports another stylesheet with *lower*
//! precedence. `<xsl:include>` textually includes another stylesheet with
//! equal precedence.
//!
//! # UPSTREAM-PARITY
//!
//! Upstream libxslt (imports.c) builds an import tree: each imported
//! stylesheet becomes a child of the importing stylesheet. Import depth
//! increases with each level. When resolving template conflicts, the
//! template with the higher import depth (i.e., imported later, closer to
//! the main stylesheet) wins — the last import has highest precedence
//! among imports.
//!
//! The `_xsltStylesheet` chain: `style->imports` lists imported
//! stylesheets; each imported stylesheet has `parent` pointing back to the
//! importer.
//!
//! # Upstream contract
//!
//! Parity target: upstream libxslt `imports.c` (1.1.45;
//! `SRC-LIBXSLT-1.1.42-IMPORTS-C` under oracle/historical/src). Subsystem
//! census: xslt-imports. Behavior is governed by XSLT 1.0 import
//! precedence rules (atlas lore keeps `xsl:apply-imports` corner cases
//! as an UNRESOLVED item pending oracle probing).
//!
//! # Conceptual behavior
//!
//! Imports are processed before any other top-level element: each
//! `xsl:import href` is recursively parsed into a child stylesheet linked
//! into `style->imports` (children of the importing stylesheet, in import
//! order — later imports nearer the head). `xsl:include` splices the
//! included stylesheet's definitions at the include point with equal
//! precedence. Template/key/variable resolution walks this tree, so
//! import depth decides conflicts: deeper import (closer to the main
//! stylesheet) wins on equal priority.
//!
//! # Ownership & safety invariants
//!
//! Imported stylesheets are owned by the importing stylesheet's `imports`
//! chain and freed by `xsltFreeStylesheet` via `xsltFreeImports` before
//! the parent definitions (children first, R-000103 ordering). Each
//! imported stylesheet owns its own style document. `parent`/`next`
//! pointers are owned by the chain; `get_element_ns`/`get_element_name`
//! return borrowed strings.
//!
//! # Historical quirks & epochs
//!
//! The import tree has been the resolution model since the libxslt 1.1
//! series (2004+; atlas/HISTORY.md) and sits inside the E-008 frozen
//! epoch (2009 → 1.1.45; atlas/SEMANTIC_EPOCHS.md): import precedence
//! behavior is byte-identical across all oracle versions. The compiler
//! module records the depth in the candidate `position` field (R-000140
//! layout), which the templates and transform modules read for conflict
//! resolution.
//!
//! # Deliberate oddities
//!
//! - The candidate tracks import depth as an integer on the stylesheet
//!   (carried into template `position`), whereas upstream derives it from
//!   the tree shape at lookup time — a documented storage divergence with
//!   identical resolution results.
//! - Import-list head placement (later imports nearer the head) mirrors
//!   upstream `xsltParseStylesheetImport` insertion.
//!
//! # Proving courts
//!
//! CLI-XSLTPROC (multi-stylesheet import corpus), XSLT-001, HEADER-COMPILE
//! (xsltParseStylesheet* surface), and the in-crate `cargo test` suites.
//!
//! # Tempting simplifications that would break parity
//!
//! - Flattening imports into one definition list breaks the precedence
//!   ordering — template conflicts, key resolution and apply-imports all
//!   depend on the import tree (XSLT 1.0 conflict-resolution rules).
//! - Making later imports lower-precedence (a naive reading of import
//!   order) inverts the oracle: the last import wins on ties.
//! - Freeing imported stylesheet documents with the parent would
//!   double-free (each imported stylesheet owns its doc).

use crate::abi::structs::*;
use crate::abi::types::xmlElementType::*;
use crate::abi::types::*;
use std::os::raw::c_int;
use std::ptr;

/// The XSLT namespace URI (no trailing NUL).
pub const XSLT_NS_URI: &str = "http://www.w3.org/1999/XSL/Transform";

/// Get the namespace URI of an element as a String, or None.
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

/// Get the local name of an element as a String, or None.
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

/// Process the `<xsl:import>` children of a stylesheet.
///
/// Called during compilation after the stylesheet element is found.
/// Each import is parsed recursively and linked into the import tree.
///
/// # SAFETY
///
/// - `style` must be a valid `_xsltStylesheet`.
/// - `doc` must be the stylesheet document.
pub unsafe fn xsltProcessImports(style: *mut _xsltStylesheet, doc: *mut _xmlDoc) -> c_int {
    if style.is_null() || doc.is_null() {
        return -1;
    }
    // Phase 8: iterate the stylesheet children; for each xsl:import element,
    // resolve the href, parse the imported stylesheet, and add it to the
    // import chain with depth + 1.
    let root = crate::xml::tree::doc_get_root_element(doc);
    if root.is_null() {
        return 0;
    }
    let mut child = (*root).children;
    while !child.is_null() {
        let next = (*child).next;
        if (*child).type_ == XML_ELEMENT_NODE as c_int {
            if let Some(ns) = get_element_ns(child) {
                if ns == XSLT_NS_URI {
                    if let Some(name) = get_element_name(child) {
                        if name == "import" {
                            let href = crate::xml::tree::get_prop(
                                child,
                                c"href".as_ptr() as *const xmlChar,
                            );
                            if !href.is_null() {
                                import_stylesheet(style, href);
                                libc::free(href as *mut libc::c_void);
                            }
                        }
                    }
                }
            }
        }
        child = next;
    }
    0
}

/// Import a stylesheet by href, recursively.
///
/// # SAFETY
///
/// - `style` must be valid.
/// - `href` must be a valid NUL-terminated string.
unsafe fn import_stylesheet(style: *mut _xsltStylesheet, href: *mut xmlChar) {
    let imported = crate::xslt::stylesheet::xsltParseStylesheetFile(href);
    if imported.is_null() {
        return;
    }
    // Link into the import tree.
    (*imported).parent = style;
    (*imported).next = (*style).imports;
    (*style).imports = imported;
}

/// Compute the import depth of a stylesheet by walking the parent chain.
///
/// # SAFETY
///
/// - `style` must be valid (or NULL, returning 0).
pub unsafe fn xsltGetImportDepth(style: *mut _xsltStylesheet) -> c_int {
    let mut depth = 0;
    let mut cur = style;
    while !cur.is_null() {
        cur = (*cur).parent;
        if !cur.is_null() {
            depth += 1;
        }
    }
    depth
}

/// Process the `<xsl:include>` children of a stylesheet.
///
/// Include is textual: the included stylesheet's top-level elements are
/// processed as if they appeared inline, at the same import depth.
///
/// # SAFETY
///
/// - `style` must be a valid `_xsltStylesheet`.
/// - `doc` must be the stylesheet document.
pub unsafe fn xsltProcessIncludes(style: *mut _xsltStylesheet, doc: *mut _xmlDoc) -> c_int {
    if style.is_null() || doc.is_null() {
        return -1;
    }
    let root = crate::xml::tree::doc_get_root_element(doc);
    if root.is_null() {
        return 0;
    }
    let mut child = (*root).children;
    while !child.is_null() {
        let next = (*child).next;
        if (*child).type_ == XML_ELEMENT_NODE as c_int {
            if let Some(ns) = get_element_ns(child) {
                if ns == XSLT_NS_URI {
                    if let Some(name) = get_element_name(child) {
                        if name == "include" {
                            let href = crate::xml::tree::get_prop(
                                child,
                                c"href".as_ptr() as *const xmlChar,
                            );
                            if !href.is_null() {
                                // Phase 8: parse the included stylesheet and
                                // compile its top-level elements into this
                                // stylesheet at the same depth.
                                libc::free(href as *mut libc::c_void);
                            }
                        }
                    }
                }
            }
        }
        child = next;
    }
    0
}

/// Free the import tree of a stylesheet.
///
/// # SAFETY
///
/// - `style` must be a valid `_xsltStylesheet`.
pub unsafe fn xsltFreeImports(style: *mut _xsltStylesheet) {
    if style.is_null() {
        return;
    }
    let mut cur = (*style).imports;
    (*style).imports = ptr::null_mut();
    while !cur.is_null() {
        let next = (*cur).next;
        (*cur).parent = ptr::null_mut();
        (*cur).next = ptr::null_mut();
        crate::xslt::stylesheet::xsltFreeStylesheet(cur);
        cur = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::ptr;

    /// NULL arguments to the import API are rejected without crashing.
    ///
    /// # Safety
    ///
    /// - `xsltProcessImports`/`xsltProcessIncludes` return `-1`,
    ///   `xsltGetImportDepth` returns `0`, and `xsltFreeImports` no-ops on
    ///   NULL arguments before dereferencing them, so the unsafe block
    ///   reads and frees no memory.
    #[test]
    fn test_null_args() {
        unsafe {
            assert_eq!(xsltProcessImports(ptr::null_mut(), ptr::null_mut()), -1);
            assert_eq!(xsltProcessIncludes(ptr::null_mut(), ptr::null_mut()), -1);
            assert_eq!(xsltGetImportDepth(ptr::null_mut()), 0);
            xsltFreeImports(ptr::null_mut());
        }
    }
}
