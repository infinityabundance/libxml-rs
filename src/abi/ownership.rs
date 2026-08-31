//! C ABI ownership contracts — who frees what, when, and under what conditions (§18).
//!
//! This module documents the ownership rules governing the libxml2/libxslt C API.
//! It does not export any `#[no_mangle]` functions — it exists to:
//!
//! 1. Document ownership contracts for every public API function
//! 2. Define Rust types that track ownership state across the ABI membrane
//! 3. Provide safe wrappers around raw pointer ownership transfers
//! 4. Define the `Owned`, `Borrowed`, and `Transferred` markers
//!
//! # Phase 1 status
//!
//! Complete — ownership contracts are documented and the ownership tracking
//! infrastructure is defined. Active enforcement will be implemented in Phase 2
//! (Tree and ownership).
//!
//! # Ownership categories
//!
//! Every pointer parameter and return value in the libxml2 C API falls into
//! one of these categories:
//!
//! | Category | Description |
//! |---|---|
//! | `Owned` | Caller owns the pointer and must free it |
//! | `Borrowed` | Caller borrows the pointer; must not free it |
//! | `Transferred` | Ownership transfers from caller to callee (or vice versa) |
//! | `ConsumedOnSuccess` | Consumed only if function succeeds; caller must free on failure |
//! | `Nullable` | Pointer may be NULL |
//! | `Static` | Pointer to a static/global object; must not be freed |
//! | `Opaque` | Internal pointer; caller must not dereference or free |
//!
//! # UPSTREAM-PARITY
//!
//! These ownership rules are derived from:
//! - Upstream source code analysis
//! - API documentation
//! - Historical bug reports about double-free / use-after-free
//! - Empirical testing with the oracle
//!
//! See `atlas/LORE.md` for detailed ownership archaeology.
//!
//! # Upstream contract
//!
//! The ownership rules documented here mirror upstream libxml2 2.15.3
//! (`SRC-LIBXML2-2.15.0-TREE-C` tree.c and `SRC-LIBXML2-2.15.0-XMLMEMORY-C`
//! xmlmemory.c) and libxslt 1.1.45 conventions, cross-checked in
//! `atlas/OWNERSHIP_ATLAS.md` — the authoritative ownership record.
//!
//! # Conceptual behavior
//!
//! This module implements the ownership membrane of the C ABI: it classifies
//! every pointer crossing the FFI boundary as Owned, Borrowed, Transferred,
//! ConsumedOnSuccess, Nullable, Static or Opaque, and provides the marker
//! types (`Owned`, `Borrowed`, ...) that track ownership state across the ABI.
//! It exports no `#[no_mangle]` functions; it exists to make the free-with
//! contract explicit (freed with `xmlFree`, caller frees, borrowed never
//! freed).
//!
//! # Ownership & safety invariants
//!
//! The core invariant: a pointer returned by an xml* allocator must be freed
//! with `xmlFree`; a borrowed pointer (node->parent, node->doc, node->ns, dict
//! lookups) is never freed by the reader; a transferred pointer changes owner
//! at the documented call. Callback user-data is borrowed by convention only.
//! See OWNERSHIP_ATLAS sections 1-6 for the per-surface tables.
//!
//! # Historical quirks & epochs
//!
//! The ownership model is the accumulation of upstream fixes: LORE-0006 /
//! QUIRK-0002 record that namespace nodes have no parent (commit `044fc6b7`,
//! 2002-03-04) — an ownership divergence downstream code depends on;
//! R-000139/R-000140 (11.1-I) were struct-mirror defects where Rust layouts
//! diverged from the C headers, changing which fields exist to free; the
//! double-free protection (xmlFree on an unknown pointer is a no-op) is a
//! documented safe divergence from OWNERSHIP_ATLAS section 8.
//!
//! # Deliberate oddities
//!
//! The marker types (`Owned<T>` etc.) are deliberately kept minimal: they are
//! the compile-time documentation of the ownership categories, not a full
//! arena system — wrapping every ABI pointer would change the exported
//! layouts.
//!
//! # Proving courts
//!
//! The OWNERSHIP and TREE-STRUCTURE court families exercise the contracts
//! documented here (`courts/suites/data-abi/tree-structure-probe.c` — the
//! TREE-001 differential probe requires byte-identical output); the
//! RUST-MIRROR-ABI court verifies the struct mirrors these rules operate on,
//! and cargo test runs the ownership unit tests.
//!
//! # Tempting simplifications that would break parity
//!
//! A tempting simplification is to treat every returned pointer as owned by
//! the caller and free it eagerly — that would double-free the borrowed
//! pointers (node->ns, dict strings, node->doc) that upstream keeps borrowed,
//! reproducing the exact double-free class R-000170 chased out of the parallel
//! test suite. The categories must not be collapsed.

#![allow(dead_code)]

use core::marker::PhantomData;
use core::ptr::NonNull;

// ═══════════════════════════════════════════════════════════════════════════════
// Ownership Marker Types
// ═══════════════════════════════════════════════════════════════════════════════

/// Marker type for owned pointers.
///
/// An `Owned<T>` wraps a `NonNull<T>` and indicates that the holder
/// is responsible for freeing the underlying object at the end of its lifetime.
///
/// # SAFETY
///
/// The `Drop` implementation will free the wrapped pointer using the appropriate
/// `xmlFree*` function. The caller must ensure:
/// - The pointer was allocated by libxml2's allocator
/// - No other code holds a mutable reference to the pointed-to data
/// - The pointer is not freed twice
#[derive(Debug)]
pub struct Owned<T: ?Sized> {
    ptr: NonNull<T>,
    _marker: PhantomData<T>,
}

/// Marker type for borrowed pointers.
///
/// A `Borrowed<T>` wraps a `*const T` or `*mut T` and indicates that
/// the holder is NOT responsible for freeing the underlying object.
///
/// # SAFETY
///
/// The borrower must ensure:
/// - The pointer remains valid for the duration of the borrow
/// - No mutable access occurs through a shared borrow
#[derive(Debug)]
pub struct Borrowed<T: ?Sized> {
    ptr: *const T,
    _marker: PhantomData<T>,
}

/// Marker type for pointers that transfer ownership.
///
/// A `Transferred<T>` wraps a `*mut T` that is being transferred
/// between caller and callee. The recipient assumes ownership.
#[derive(Debug)]
pub struct Transferred<T: ?Sized> {
    ptr: *mut T,
    _marker: PhantomData<T>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Ownership Tracking for Tree Nodes
// ═══════════════════════════════════════════════════════════════════════════════

/// The ownership state of a tree node.
///
/// This is used internally to track whether a node pointer is:
/// - Part of a document tree (owned by the document)
/// - A standalone node (caller-owned, must be freed or adopted)
/// - A borrowed reference (must not be freed)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeOwnership {
    /// The node is owned by its parent document or parent node.
    /// It will be freed when the document is freed.
    /// Callers must NOT free it directly.
    TreeOwned,
    /// The node is standalone (not attached to any document).
    /// The caller owns it and must free it or attach it to a tree.
    CallerOwned,
    /// The node is borrowed from a tree. The caller must NOT free it.
    Borrowed,
    /// The node has been unlinked from its tree.
    /// The caller now owns it and must free it or re-attach it.
    Unlinked,
}

/// The ownership state of a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocOwnership {
    /// The document is owned by the caller.
    /// It must be freed with `xmlFreeDoc`.
    Owned,
    /// The document is owned by a parser context.
    /// It will be freed when the context is freed.
    ParserOwned,
    /// The document is borrowed. Caller must not free it.
    Borrowed,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Ownership Contracts for Key API Functions
// ═══════════════════════════════════════════════════════════════════════════════

// ─── Document functions ───

/// `xmlNewDoc(const xmlChar *version)`
///
/// # Ownership
///
/// - `version`: Borrowed (nullable). If non-NULL, the string is copied internally.
///   The caller retains ownership of the original string.
/// - **Returns**: Owned. Caller must free with `xmlFreeDoc`.
///   On failure, returns NULL.
pub const DOC_NEWDOC_OWNERSHIP: &str = "version: Borrowed (copied). Return: Owned (xmlFreeDoc).";

/// `xmlFreeDoc(xmlDocPtr doc)`
///
/// # Ownership
///
/// - `doc`: Consumed. The document and all its contents are freed.
///   After this call, `doc` must not be dereferenced.
pub const DOC_FREEDOC_OWNERSHIP: &str = "doc: Consumed. Must not be used after call.";

/// `xmlDocSetRootElement(xmlDocPtr doc, xmlNodePtr root)`
///
/// # Ownership
///
/// - `doc`: Borrowed (mutated). Caller retains ownership.
/// - `root`: Transferred. The document takes ownership of the root element.
///   If the document already had a root element, the old root is returned
///   as owned by the caller.
/// - **Returns**: Owned (nullable). The old root element, or NULL if there was none.
///   Caller must free the old root with `xmlFreeNode` if non-NULL.
pub const DOC_SETROOT_OWNERSHIP: &str =
    "doc: Borrowed(mut). root: Transferred to doc. Return: Owned old root (xmlFreeNode).";

/// `xmlDocGetRootElement(const xmlDoc *doc)`
///
/// # Ownership
///
/// - `doc`: Borrowed.
/// - **Returns**: Borrowed. Must not be freed. The pointer is valid
///   until the document is freed.
pub const DOC_GETROOT_OWNERSHIP: &str = "doc: Borrowed. Return: Borrowed (valid until doc freed).";

// ─── Node functions ───

/// `xmlNewNode(xmlNsPtr ns, const xmlChar *name)`
///
/// # Ownership
///
/// - `ns`: Borrowed (nullable). If non-NULL, the namespace is not copied;
///   the node holds a pointer to it. The namespace must remain valid
///   as long as the node exists.
/// - `name`: Borrowed. The name is copied into the dictionary.
/// - **Returns**: Owned. Caller must free with `xmlFreeNode` or adopt into a tree.
pub const NODE_NEWNODE_OWNERSHIP: &str =
    "ns: Borrowed (must outlive node). name: Borrowed (copied). Return: Owned (xmlFreeNode).";

/// `xmlFreeNode(xmlNodePtr node)`
///
/// # Ownership
///
/// - `node`: Consumed (nullable). If non-NULL, the node and its subtree are freed.
///   The node must NOT be part of a document tree (must be unlinked first).
pub const NODE_FREENODE_OWNERSHIP: &str = "node: Consumed (must be unlinked). NULL-safe.";

/// `xmlUnlinkNode(xmlNodePtr node)`
///
/// # Ownership
///
/// - `node`: Borrowed (mutated). After unlinking, the node becomes caller-owned
///   and must be freed or re-attached. The node is removed from its parent
///   and sibling chain but its memory is not freed.
pub const NODE_UNLINK_OWNERSHIP: &str =
    "node: Borrowed(mut). After: caller owns unlinked node (must free or reattach).";

/// `xmlAddChild(xmlNodePtr parent, xmlNodePtr cur)`
///
/// # Ownership
///
/// - `parent`: Borrowed (mutated).
/// - `cur`: Transferred. Parent takes ownership of the child node.
/// - **Returns**: Borrowed. Pointer to the added child (or NULL on error).
///   Caller must NOT free the returned pointer.
pub const NODE_ADDCHILD_OWNERSHIP: &str =
    "parent: Borrowed(mut). cur: Transferred to parent. Return: Borrowed (do not free).";

/// `xmlAddSibling(xmlNodePtr cur, xmlNodePtr sibling)`
///
/// # Ownership
///
/// - `cur`: Borrowed (mutated).
/// - `sibling`: Transferred. The sibling list takes ownership.
/// - **Returns**: Borrowed. Pointer to the added sibling.
pub const NODE_ADDSIBLING_OWNERSHIP: &str =
    "cur: Borrowed(mut). sibling: Transferred. Return: Borrowed.";

/// `xmlCopyNode(const xmlNodePtr node, int extended)`
///
/// # Ownership
///
/// - `node`: Borrowed.
/// - **Returns**: Owned. A deep or shallow copy. Caller must free with `xmlFreeNode`.
pub const NODE_COPYNODE_OWNERSHIP: &str = "node: Borrowed. Return: Owned (xmlFreeNode).";

/// `xmlCopyDoc(const xmlDocPtr doc, int recursive)`
///
/// # Ownership
///
/// - `doc`: Borrowed.
/// - **Returns**: Owned. Caller must free with `xmlFreeDoc`.
pub const DOC_COPYDOC_OWNERSHIP: &str = "doc: Borrowed. Return: Owned (xmlFreeDoc).";

// ─── Attribute functions ───

/// `xmlSetProp(xmlNodePtr node, const xmlChar *name, const xmlChar *value)`
///
/// # Ownership
///
/// - `node`: Borrowed (mutated).
/// - `name`: Borrowed (copied).
/// - `value`: Borrowed (copied, nullable).
/// - **Returns**: Borrowed. Pointer to the attribute (or NULL on error).
pub const ATTR_SETPROP_OWNERSHIP: &str =
    "node: Borrowed(mut). name/value: Borrowed(copied). Return: Borrowed.";

/// `xmlGetProp(const xmlNode *node, const xmlChar *name)`
///
/// # Ownership
///
/// - `node`: Borrowed.
/// - `name`: Borrowed.
/// - **Returns**: Owned (nullable). The property value string.
///   Caller must free with `xmlFree`.
pub const ATTR_GETPROP_OWNERSHIP: &str = "node/name: Borrowed. Return: Owned string (xmlFree).";

/// `xmlSetNsProp(xmlNodePtr node, xmlNsPtr ns, const xmlChar *name, const xmlChar *value)`
///
/// # Ownership
///
/// - `node`: Borrowed (mutated).
/// - `ns`: Borrowed (nullable).
/// - `name`, `value`: Borrowed (copied).
/// - **Returns**: Borrowed.
pub const ATTR_SETNSPROP_OWNERSHIP: &str =
    "node: Borrowed(mut). ns: Borrowed(nullable). name/value: Borrowed. Return: Borrowed.";

// ─── Namespace functions ───

/// `xmlNewNs(xmlNodePtr node, const xmlChar *href, const xmlChar *prefix)`
///
/// # Ownership
///
/// - `node`: Borrowed (mutated, nullable).
/// - `href`: Borrowed (copied, nullable).
/// - `prefix`: Borrowed (copied, nullable).
/// - **Returns**: Owned (nullable). The new namespace definition.
///   The namespace is owned by the node it is attached to.
///   Caller must NOT free it directly.
pub const NS_NEWNS_OWNERSHIP: &str =
    "node: Borrowed(mut). href/prefix: Borrowed(copied). Return: Borrowed (owned by node).";

/// `xmlSetNs(xmlNodePtr node, xmlNsPtr ns)`
///
/// # Ownership
///
/// - `node`: Borrowed (mutated).
/// - `ns`: Borrowed. The namespace must be valid for the node's document.
pub const NS_SETNS_OWNERSHIP: &str = "node/ns: Borrowed.";

// ─── Entity functions ───

/// `xmlNewEntity(xmlDocPtr doc, const xmlChar *name, int type,
///               const xmlChar *ExternalID, const xmlChar *SystemID,
///               const xmlChar *content)`
///
/// # Ownership
///
/// - `doc`: Borrowed (mutated). Entity is added to the document's entities table.
/// - `name`, `ExternalID`, `SystemID`, `content`: Borrowed (copied).
/// - **Returns**: Borrowed. The entity is owned by the document.
pub const ENTITY_NEWENTITY_OWNERSHIP: &str =
    "doc: Borrowed(mut). name/IDs/content: Borrowed(copied). Return: Borrowed (owned by doc).";

// ─── DTD functions ───

/// `xmlNewDtd(xmlDocPtr doc, const xmlChar *name,
///            const xmlChar *ExternalID, const xmlChar *SystemID)`
///
/// # Ownership
///
/// - `doc`: Borrowed (mutated, nullable).
/// - `name`, `ExternalID`, `SystemID`: Borrowed (copied).
/// - **Returns**: Owned (nullable). The DTD is owned by the document.
pub const DTD_NEWDTD_OWNERSHIP: &str =
    "doc: Borrowed(mut). name/IDs: Borrowed(copied). Return: Borrowed (owned by doc).";

// ─── Parser context functions ───

/// `xmlCreateFileParserCtxt(const char *filename)`
///
/// # Ownership
///
/// - `filename`: Borrowed.
/// - **Returns**: Owned. Caller must free with `xmlFreeParserCtxt`.
pub const PARSE_CREATEFILE_OWNERSHIP: &str =
    "filename: Borrowed. Return: Owned (xmlFreeParserCtxt).";

/// `xmlFreeParserCtxt(xmlParserCtxtPtr ctxt)`
///
/// # Ownership
///
/// - `ctxt`: Consumed.
pub const PARSE_FREECTXT_OWNERSHIP: &str = "ctxt: Consumed.";

// ─── XPath functions ───

/// `xmlXPathNewContext(xmlDocPtr doc)`
///
/// # Ownership
///
/// - `doc`: Borrowed. The document must outlive the XPath context.
/// - **Returns**: Owned. Caller must free with `xmlXPathFreeContext`.
pub const XPATH_NEWCTX_OWNERSHIP: &str =
    "doc: Borrowed (must outlive context). Return: Owned (xmlXPathFreeContext).";

/// `xmlXPathFreeContext(xmlXPathContextPtr ctxt)`
///
/// # Ownership
///
/// - `ctxt`: Consumed.
pub const XPATH_FREECTX_OWNERSHIP: &str = "ctxt: Consumed.";

/// `xmlXPathEvalExpression(const xmlChar *str, xmlXPathContextPtr ctxt)`
///
/// # Ownership
///
/// - `str`: Borrowed.
/// - `ctxt`: Borrowed (mutated).
/// - **Returns**: Owned. Caller must free with `xmlXPathFreeObject`.
pub const XPATH_EVAL_OWNERSHIP: &str =
    "str: Borrowed. ctxt: Borrowed(mut). Return: Owned (xmlXPathFreeObject).";

/// `xmlXPathFreeObject(xmlXPathObjectPtr obj)`
///
/// # Ownership
///
/// - `obj`: Consumed (nullable).
pub const XPATH_FREEOBJ_OWNERSHIP: &str = "obj: Consumed. NULL-safe.";

// ─── XSLT functions ───

/// `xsltParseStylesheetFile(const xmlChar *filename)`
///
/// # Ownership
///
/// - `filename`: Borrowed.
/// - **Returns**: Owned. Caller must free with `xsltFreeStylesheet`.
pub const XSLT_PARSE_OWNERSHIP: &str = "filename: Borrowed. Return: Owned (xsltFreeStylesheet).";

/// `xsltFreeStylesheet(xsltStylesheetPtr style)`
///
/// # Ownership
///
/// - `style`: Consumed (nullable).
pub const XSLT_FREESTYLE_OWNERSHIP: &str = "style: Consumed. NULL-safe.";

/// `xsltApplyStylesheet(xsltStylesheetPtr style, xmlDocPtr doc, const char **params)`
///
/// # Ownership
///
/// - `style`: Borrowed.
/// - `doc`: Borrowed. The source document is not modified.
/// - `params`: Borrowed (nullable). NULL-terminated array of name=value strings.
/// - **Returns**: Owned. The result document. Caller must free with `xmlFreeDoc`.
pub const XSLT_APPLY_OWNERSHIP: &str =
    "style/doc: Borrowed. params: Borrowed(nullable). Return: Owned (xmlFreeDoc).";

// ═══════════════════════════════════════════════════════════════════════════════
// Ownership Enforcement Helpers (for use in Phase 2+)
// ═══════════════════════════════════════════════════════════════════════════════

/// A wrapper around a raw pointer that enforces single ownership.
///
/// When `UniquePtr<T>` is dropped, it frees the underlying object
/// using the provided `DropFn`.
///
/// # Safety
///
/// This is an internal helper for the ABI membrane. It should not be
/// exposed to external callers.
#[derive(Debug)]
pub struct UniquePtr<T> {
    ptr: Option<NonNull<T>>,
    drop_fn: unsafe fn(*mut T),
}

impl<T> UniquePtr<T> {
    /// Create a new `UniquePtr` from a raw pointer.
    ///
    /// # Safety
    ///
    /// - `ptr` must be a valid, uniquely-owned pointer or NULL.
    /// - `drop_fn` must correctly free the pointed-to object.
    /// - No other code may hold a reference to this pointer.
    pub unsafe fn new(ptr: *mut T, drop_fn: unsafe fn(*mut T)) -> Self {
        Self {
            ptr: NonNull::new(ptr),
            drop_fn,
        }
    }

    /// Get a raw pointer (for FFI calls).
    /// The caller must not free the pointer.
    pub fn as_ptr(&self) -> *const T {
        self.ptr
            .map_or(core::ptr::null(), |p| p.as_ptr() as *const T)
    }

    /// Get a mutable raw pointer (for FFI calls that mutate).
    /// The caller must not free the pointer.
    pub fn as_mut_ptr(&mut self) -> *mut T {
        self.ptr.map_or(core::ptr::null_mut(), |p| p.as_ptr())
    }

    /// Release ownership and return the raw pointer.
    /// The caller is now responsible for freeing it.
    pub fn into_raw(mut self) -> *mut T {
        let ptr = self
            .ptr
            .take()
            .map_or(core::ptr::null_mut(), |p| p.as_ptr());
        core::mem::forget(self);
        ptr
    }
}

impl<T> Drop for UniquePtr<T> {
    fn drop(&mut self) {
        if let Some(ptr) = self.ptr.take() {
            unsafe { (self.drop_fn)(ptr.as_ptr()) };
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Ownership Assertions
// ═══════════════════════════════════════════════════════════════════════════════

/// Assert that a pointer is non-null and valid.
///
/// Used in debug builds to catch NULL pointer dereferences early.
#[inline]
pub fn assert_non_null<T>(ptr: *const T, what: &str) {
    debug_assert!(!ptr.is_null(), "{} must not be NULL", what);
}

/// Assert that a mutable pointer is non-null and valid.
#[inline]
pub fn assert_non_null_mut<T>(ptr: *mut T, what: &str) {
    debug_assert!(!ptr.is_null(), "{} must not be NULL", what);
}
