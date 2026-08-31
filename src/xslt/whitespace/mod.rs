//! XSLT whitespace stripping (§33, §85 Phase 8).
//!
//! `<xsl:strip-space>` and `<xsl:preserve-space>` control which whitespace-
//! only text nodes are removed from the source document before processing.
//!
//! # UPSTREAM-PARITY
//!
//! Upstream libxslt (xslt.c `xsltStripSpaces` / `xsltApplyStripSpaces`)
//! processes whitespace-only text nodes in the source document: any text
//! node consisting solely of whitespace is removed unless its parent
//! element matches a preserve-space rule and does not match a strip-space
//! rule with higher precedence.
//!
//! Rules are stored on the stylesheet as linked lists (`stripSpaces`,
//! `preserveSpaces`) of `_xsltStripSpace` entries holding element names
//! (QName patterns) and import depths.

use crate::abi::allocator::xmlFreeImpl;
use crate::abi::structs::*;
use crate::abi::types::xmlElementType::*;
use crate::abi::types::*;
use std::os::raw::c_int;
use std::ptr;

/// A strip-space / preserve-space rule entry.
#[derive(Debug)]
#[repr(C)]
pub struct _xsltStripSpace {
    /// Next rule in the strip/preserve-space list.
    pub next: *mut _xsltStripSpace,
    /// Element name pattern (QName) the rule matches.
    pub name: *mut xmlChar,
    /// Import depth of the stylesheet that defined the rule, used for
    /// precedence between strip-space and preserve-space rules.
    pub depth: c_int,
}

/// Register a strip-space or preserve-space rule.
///
/// `strip` selects the list: 1 = strip, 0 = preserve.
///
/// # SAFETY
///
/// - `style` must be a valid `_xsltStylesheet`.
/// - `name` must be a valid NUL-terminated element name pattern.
pub unsafe fn xsltAddStripSpace(
    style: *mut _xsltStylesheet,
    name: *const xmlChar,
    strip: c_int,
) -> c_int {
    if style.is_null() || name.is_null() {
        return -1;
    }
    let entry = libc::calloc(1, core::mem::size_of::<_xsltStripSpace>()) as *mut _xsltStripSpace;
    if entry.is_null() {
        return -1;
    }
    let len = libc::strlen(name as *const libc::c_char);
    let copy = libc::malloc(len + 1) as *mut xmlChar;
    if copy.is_null() {
        xmlFreeImpl(entry as *mut libc::c_void);
        return -1;
    }
    libc::memcpy(copy as *mut libc::c_void, name as *const libc::c_void, len);
    *copy.add(len) = 0;
    (*entry).name = copy;
    (*entry).depth = 0;
    if strip != 0 {
        (*entry).next = (*style).stripSpaces as *mut _xsltStripSpace;
        (*style).stripSpaces = entry as *mut c_void;
    } else {
        // UPSTREAM-PARITY: upstream has no preserveSpaces field (only a
        // stripSpaces hash + stripAll); the candidate's preserve-list head
        // lives in the unused nsDefs void* slot (documented divergence).
        (*entry).next = (*style).nsDefs as *mut _xsltStripSpace;
        (*style).nsDefs = entry as *mut c_void;
    }
    0
}

/// Free a single strip-space rule.
///
/// # SAFETY
///
/// - `entry` must be a valid `_xsltStripSpace` allocated by this library.
pub unsafe fn xsltFreeStripSpaceEntry(entry: *mut _xsltStripSpace) {
    if entry.is_null() {
        return;
    }
    if !(*entry).name.is_null() {
        libc::free((*entry).name as *mut libc::c_void);
    }
    xmlFreeImpl(entry as *mut libc::c_void);
}

/// Free all strip/preserve-space rules in a stylesheet.
///
/// # SAFETY
///
/// - `style` must be a valid `_xsltStylesheet`.
pub unsafe fn xsltFreeStripSpaces(style: *mut _xsltStylesheet) {
    if style.is_null() {
        return;
    }
    let mut cur = (*style).stripSpaces as *mut _xsltStripSpace;
    (*style).stripSpaces = ptr::null_mut();
    while !cur.is_null() {
        let next = (*cur).next;
        xsltFreeStripSpaceEntry(cur);
        cur = next;
    }
    let mut cur = (*style).nsDefs as *mut _xsltStripSpace;
    (*style).nsDefs = ptr::null_mut();
    while !cur.is_null() {
        let next = (*cur).next;
        xsltFreeStripSpaceEntry(cur);
        cur = next;
    }
}

/// Check whether an element name matches a space rule pattern.
///
/// Patterns may be `*` (match any element) or a QName.
unsafe fn name_matches(pattern: *const xmlChar, name: *const xmlChar) -> bool {
    if pattern.is_null() || name.is_null() {
        return false;
    }
    let p = core::slice::from_raw_parts(
        pattern,
        libc::strlen(pattern as *const libc::c_char) as usize,
    );
    if p == b"*" {
        return true;
    }
    libc::strcmp(pattern as *const libc::c_char, name as *const libc::c_char) == 0
}

/// Determine whether whitespace-only children should be stripped from an
/// element. Returns 1 if stripped, 0 if preserved.
///
/// Precedence (XSLT 1.0 §3.4):
/// 1. An element with a matching strip-space rule at a deeper import depth
///    wins over preserve-space at a shallower depth.
/// 2. Default is preserve (no stripping).
///
/// # SAFETY
///
/// - `style` must be a valid `_xsltStylesheet`.
/// - `name` must be a valid element local name.
pub unsafe fn xsltShouldStripSpace(style: *mut _xsltStylesheet, name: *const xmlChar) -> c_int {
    if style.is_null() || name.is_null() {
        return 0;
    }
    // Find the best (deepest import depth) matching strip rule.
    let mut strip_depth = -1;
    let mut cur = (*style).stripSpaces as *mut _xsltStripSpace;
    while !cur.is_null() {
        if name_matches((*cur).name, name) && (*cur).depth > strip_depth {
            strip_depth = (*cur).depth;
        }
        cur = (*cur).next;
    }
    if strip_depth < 0 {
        return 0;
    }
    // Find the best matching preserve rule (list head in nsDefs).
    let mut preserve_depth = -1;
    let mut cur = (*style).nsDefs as *mut _xsltStripSpace;
    while !cur.is_null() {
        if name_matches((*cur).name, name) && (*cur).depth > preserve_depth {
            preserve_depth = (*cur).depth;
        }
        cur = (*cur).next;
    }
    if preserve_depth >= strip_depth {
        return 0;
    }
    1
}

/// Strip whitespace-only text nodes from the source document according to
/// the stylesheet's rules.
///
/// # SAFETY
///
/// - `style` must be a valid `_xsltStylesheet`.
/// - `doc` must be a valid source document.
pub unsafe fn xsltApplyStripSpaces(style: *mut _xsltStylesheet, doc: *mut _xmlDoc) {
    if style.is_null() || doc.is_null() {
        return;
    }
    let root = crate::xml::tree::doc_get_root_element(doc);
    if root.is_null() {
        return;
    }
    strip_recursive(style, root);
}

/// Recursively strip whitespace-only text children.
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn strip_recursive(style: *mut _xsltStylesheet, node: *mut _xmlNode) {
    if node.is_null() {
        return;
    }
    if (*node).type_ == XML_ELEMENT_NODE as c_int
        && !(*node).name.is_null()
        && xsltShouldStripSpace(style, (*node).name) != 0
    {
        // Remove whitespace-only text children.
        let mut child = (*node).children;
        while !child.is_null() {
            let next = (*child).next;
            if (*child).type_ == XML_TEXT_NODE as c_int && is_whitespace_only((*child).content) {
                crate::xml::tree::unlink_node(child);
                crate::xml::tree::free_node(child);
            }
            child = next;
        }
    }
    // Recurse into children.
    let mut child = (*node).children;
    while !child.is_null() {
        let next = (*child).next;
        strip_recursive(style, child);
        child = next;
    }
}

/// Check whether a string consists solely of XML whitespace.
unsafe fn is_whitespace_only(content: *const xmlChar) -> bool {
    if content.is_null() {
        return false;
    }
    let bytes = core::slice::from_raw_parts(
        content,
        libc::strlen(content as *const libc::c_char) as usize,
    );
    !bytes.is_empty()
        && bytes
            .iter()
            .all(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r'))
}

use std::ffi::c_void;

#[cfg(test)]
mod tests {
    use super::*;
    use core::ptr;

    fn make_style() -> *mut _xsltStylesheet {
        unsafe { libc::calloc(1, core::mem::size_of::<_xsltStylesheet>()) as *mut _xsltStylesheet }
    }

    #[test]
    fn test_should_strip_space() {
        unsafe {
            let style = make_style();
            // No rules: preserve by default.
            assert_eq!(
                xsltShouldStripSpace(style, c"foo".as_ptr() as *const xmlChar),
                0
            );
            // Strip rule for "foo".
            xsltAddStripSpace(style, c"foo".as_ptr() as *const xmlChar, 1);
            assert_eq!(
                xsltShouldStripSpace(style, c"foo".as_ptr() as *const xmlChar),
                1
            );
            assert_eq!(
                xsltShouldStripSpace(style, c"bar".as_ptr() as *const xmlChar),
                0
            );
            // Preserve rule overrides strip (same depth).
            xsltAddStripSpace(style, c"foo".as_ptr() as *const xmlChar, 0);
            assert_eq!(
                xsltShouldStripSpace(style, c"foo".as_ptr() as *const xmlChar),
                0
            );
            // Wildcard rule strips everything not preserved.
            xsltAddStripSpace(style, c"*".as_ptr() as *const xmlChar, 1);
            assert_eq!(
                xsltShouldStripSpace(style, c"bar".as_ptr() as *const xmlChar),
                1
            );
            assert_eq!(
                xsltShouldStripSpace(style, c"foo".as_ptr() as *const xmlChar),
                0 // preserved
            );
            xsltFreeStripSpaces(style);
            libc::free(style as *mut libc::c_void);
        }
    }

    #[test]
    fn test_null_args() {
        unsafe {
            assert_eq!(xsltAddStripSpace(ptr::null_mut(), ptr::null(), 1), -1);
            xsltFreeStripSpaceEntry(ptr::null_mut());
            xsltFreeStripSpaces(ptr::null_mut());
            assert_eq!(xsltShouldStripSpace(ptr::null_mut(), ptr::null()), 0);
        }
    }
}
