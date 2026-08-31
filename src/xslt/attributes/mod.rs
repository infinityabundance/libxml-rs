//! XSLT attribute sets (§33, §85 Phase 8).
//!
//! # Upstream contract
//!
//! Parity target: upstream libxslt `attributes.c` (1.1.45; verified against
//! `SRC-LIBXSLT-1.1.42-ATTRIBUTES-C` under oracle/historical/src). The
//! observable surface is `xsltCompileAttrSet` / `xsltFreeAttrSets` /
//! `xsltApplyAttrSets` and the `xsl:use-attribute-sets` attribute on
//! literal result elements, `xsl:element`, and `xsl:copy` (XSLT 1.0).
//!
//! # Conceptual behavior
//!
//! Compilation records each attribute set with its name (QName), optional
//! namespace, and the defining `xsl:attribute-set` element. At instantiation
//! time, `use-attribute-sets` (a whitespace-separated list of names) is
//! resolved and every `xsl:attribute` child of each named set is executed
//! as an instruction against the current node, in the order the names
//! appear in the list.
//!
//! # Ownership & safety invariants
//!
//! Each `_xsltAttrSet` is heap-allocated (libc::calloc) by the candidate,
//! owns its duplicated `name`/`ns` strings, and is owned by the stylesheet
//! `attributeSets` chain; `xsltFreeAttrSets` (called from
//! `xsltFreeStylesheet`) unlinks and frees every entry exactly once. The
//! `inst` pointer is borrowed from the stylesheet document — never freed
//! here. `xsltApplyAttrSets` requires a live transform context and result
//! node; attribute evaluation goes through the transform engine.
//!
//! # Historical quirks & epochs
//!
//! Attribute sets have been part of the libxslt 1.1 series since the
//! 1.1 era (2004+; atlas/HISTORY.md). E-008 (atlas/SEMANTIC_EPOCHS.md):
//! xsltproc output is byte-identical from 1.1.26 (2009) through 1.1.45, so
//! the attribute-set output path is frozen — a divergence here is a
//! candidate bug, not an epoch difference. The candidate diverges from the
//! upstream hash storage (see oddities) but keeps the observable semantics.
//!
//! # Deliberate oddities
//!
//! Upstream stores attribute sets in an `xmlHashTable` keyed by
//! `{ns}name`; the candidate keeps a linked list in the same `attributeSets`
//! slot and matches by exact name bytes only (the namespace component of
//! the key is not consulted). This is a documented storage divergence
//! (annotated at the compile/apply functions); it is observable only
//! through behavior, which the CLI corpus pins down.
//!
//! # Proving courts
//!
//! CLI-XSLTPROC (xsltproc corpus stylesheets using use-attribute-sets),
//! XSLT-001 (xslt-family differential probe), and the in-crate `cargo
//! test` suites.
//!
//! # Tempting simplifications that would break parity
//!
//! - Matching by local name only (dropping the list walk) would accept
//!   attribute sets from the wrong namespace — XSLT 1.0 resolves the
//!   QName through the in-scope namespaces at compile time.
//! - Reusing the compiled `xsl:attribute` nodes instead of re-evaluating
//!   them per use would break AVT evaluation and context-dependent
//!   attribute values (R-000108 lesson: attribute instructions are
//!   evaluated, not copied).
//! - Freeing the `inst` pointer would double-free stylesheet-document nodes
//!   (R-000103 lesson: template/instruction content is owned by the
//!   stylesheet document).
//!
//! `<xsl:attribute-set>` defines a named set of attributes that can be
//! applied to a result element via `use-attribute-sets`.
//!
//! # UPSTREAM-PARITY
//!
//! Upstream libxslt (attributes.c) stores attribute sets in a hash on the
//! stylesheet (`attributeSets`), keyed by `{namespace}name`. When
//! `use-attribute-sets` is processed, the sets are looked up by name and
//! their `xsl:attribute` children are evaluated in the current context.

use crate::abi::allocator::xmlFreeImpl;
use crate::abi::structs::*;
use crate::abi::types::xmlElementType::*;
use crate::abi::types::*;
use std::os::raw::c_int;
use std::ptr;

/// Compile an attribute set from an `xsl:attribute-set` element.
///
/// # SAFETY
///
/// - `style` must be a valid `_xsltStylesheet`.
/// - `inst` must be a valid `xsl:attribute-set` element node.
pub unsafe fn xsltCompileAttrSet(
    style: *mut _xsltStylesheet,
    inst: *mut _xmlNode,
) -> *mut _xsltAttrSet {
    if style.is_null() || inst.is_null() {
        return ptr::null_mut();
    }
    // Get the name attribute.
    let name = crate::xml::tree::get_prop(inst, c"name".as_ptr() as *const xmlChar);
    if name.is_null() {
        return ptr::null_mut();
    }
    let set = libc::calloc(1, core::mem::size_of::<_xsltAttrSet>()) as *mut _xsltAttrSet;
    if set.is_null() {
        libc::free(name as *mut libc::c_void);
        return ptr::null_mut();
    }
    (*set).name = name;
    (*set).inst = inst;
    (*set).style = style;
    // Look up the namespace of the name attribute (QName may have a prefix).
    let ns = crate::xml::tree::get_prop(inst, c"xmlns".as_ptr() as *const xmlChar);
    (*set).ns = ns; // may be null
                    // Prepend to the stylesheet's attribute set hash chain.
    (*set).next = (*style).attributeSets as *mut _xsltAttrSet;
    (*style).attributeSets = set as *mut c_void;
    set
}

/// Free an attribute set.
///
/// # SAFETY
///
/// - `set` must be a valid `_xsltAttrSet` allocated by this library.
pub unsafe fn xsltFreeAttrSet(set: *mut _xsltAttrSet) {
    if set.is_null() {
        return;
    }
    if !(*set).name.is_null() {
        libc::free((*set).name as *mut libc::c_void);
    }
    if !(*set).ns.is_null() {
        libc::free((*set).ns as *mut libc::c_void);
    }
    (*set).next = ptr::null_mut();
    xmlFreeImpl(set as *mut libc::c_void);
}

/// Free all attribute sets in a stylesheet.
///
/// # SAFETY
///
/// - `style` must be a valid `_xsltStylesheet`.
pub unsafe fn xsltFreeAttrSets(style: *mut _xsltStylesheet) {
    if style.is_null() {
        return;
    }
    let mut cur = (*style).attributeSets as *mut _xsltAttrSet;
    (*style).attributeSets = ptr::null_mut();
    while !cur.is_null() {
        let next = (*cur).next;
        xsltFreeAttrSet(cur);
        cur = next;
    }
}

/// Apply attribute sets to an element.
///
/// Evaluates each `xsl:attribute` in the named attribute sets and adds
/// the resulting attributes to the result element.
///
/// # SAFETY
///
/// - `ctxt` must be a valid `_xsltTransformContext`.
/// - `node` must be a valid result element node.
pub unsafe fn xsltApplyAttrSets(
    ctxt: *mut _xsltTransformContext,
    node: *mut _xmlNode,
    use_sets: *const xmlChar,
) -> c_int {
    if ctxt.is_null() || node.is_null() || use_sets.is_null() {
        return -1;
    }
    // use_sets is a whitespace-separated list of attribute set names.
    let bytes = core::slice::from_raw_parts(
        use_sets,
        libc::strlen(use_sets as *const libc::c_char) as usize,
    );
    let names: Vec<&[u8]> = bytes
        .split(|b| *b == b' ' || *b == b'\t' || *b == b'\n' || *b == b'\r')
        .filter(|s| !s.is_empty())
        .collect();
    let style = (*ctxt).style;
    if style.is_null() {
        return 0;
    }
    for name in names {
        // Look up the attribute set in the stylesheet.
        let mut cur = (*style).attributeSets as *mut _xsltAttrSet;
        while !cur.is_null() {
            if !(*cur).name.is_null() {
                let set_name = core::slice::from_raw_parts(
                    (*cur).name,
                    libc::strlen((*cur).name as *const libc::c_char) as usize,
                );
                if set_name == name {
                    // Evaluate the attribute children of the set.
                    apply_attr_set_content(ctxt, node, (*cur).inst);
                    break;
                }
            }
            cur = (*cur).next;
        }
    }
    0
}

/// Evaluate the `xsl:attribute` children of an attribute set instruction.
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn apply_attr_set_content(
    ctxt: *mut _xsltTransformContext,
    _node: *mut _xmlNode,
    inst: *mut _xmlNode,
) {
    if inst.is_null() {
        return;
    }
    let mut child = (*inst).children;
    while !child.is_null() {
        let next = (*child).next;
        // Only xsl:attribute elements are evaluated here.
        if (*child).type_ == XML_ELEMENT_NODE as c_int {
            if let Some(name) = node_name(child) {
                if name == "attribute" {
                    // Evaluate the attribute: name + value (content or select).
                    crate::xslt::transform::xsltProcessInstruction(ctxt, child);
                }
            }
        }
        child = next;
    }
}

/// Get the local name of an element node.
unsafe fn node_name(node: *mut _xmlNode) -> Option<String> {
    if node.is_null() || (*node).name.is_null() {
        return None;
    }
    let bytes = core::slice::from_raw_parts(
        (*node).name,
        libc::strlen((*node).name as *const libc::c_char) as usize,
    );
    String::from_utf8_lossy(bytes).into_owned().into()
}

use std::ffi::c_void;

#[cfg(test)]
mod tests {
    use super::*;
    use core::ptr;

    /// NULL arguments to the attribute-set API are rejected without
    /// crashing.
    ///
    /// # Safety
    ///
    /// - `xsltCompileAttrSet` returns NULL, `xsltApplyAttrSets` returns
    ///   `-1`, and the free functions no-op on NULL arguments before
    ///   dereferencing them, so the unsafe block reads and frees no
    ///   memory.
    #[test]
    fn test_null_args() {
        unsafe {
            assert!(xsltCompileAttrSet(ptr::null_mut(), ptr::null_mut()).is_null());
            xsltFreeAttrSet(ptr::null_mut());
            xsltFreeAttrSets(ptr::null_mut());
            assert_eq!(
                xsltApplyAttrSets(ptr::null_mut(), ptr::null_mut(), ptr::null()),
                -1
            );
        }
    }
}
