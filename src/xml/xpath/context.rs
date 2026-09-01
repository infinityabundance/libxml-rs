//! XPath 1.0 Evaluation Context (§25).
//!
//! The evaluation context holds the state required to evaluate an XPath
//! expression: the current document, context node, context position/size,
//! variable bindings, namespace declarations, registered extension functions,
//! and recursion-depth tracking.
//!
//! # UPSTREAM-PARITY
//!
//! Mirrors `xmlXPathContext` from libxml2 with additional Rust-side state
//! for variable/function resolution and safe recursion guards.
//!
//! # Courts
//!
//! XPATH-CONTEXT-*
//!
//! # Upstream contract
//!
//! Mirrors `xmlXPathContext` (xpath.c / xpathInternals.h,
//! `SRC-LIBXML2-2.15.0-XPATH-C`, parity target libxml2 2.15.3 oracle):
//! context node/document, position/size, variable and function registries,
//! namespace scope, the C var/function lookup hooks, and the opLimit/
//! opCount fields (R-000128 fixed their widths in the C mirror).
//!
//! # Conceptual behavior
//!
//! Holds the state an evaluation needs: current document, context node,
//! context position/size, variable bindings, namespace declarations,
//! registered extension functions and recursion-depth tracking. The
//! C function bridge (R-000162) synthesizes an `xmlXPathParserContext`
//! around the value stack, pushes evaluated args, invokes the registered
//! C function and converts its result back — including the namespaced
//! function_lookup fallback the XSLT engine uses for prefix:local calls.
//!
//! # Ownership & safety invariants
//!
//! Callback user-data pointers (`var_lookup_data` / `func_lookup_data`)
//! are stored verbatim and passed back — the caller keeps them alive
//! (OWNERSHIP_ATLAS §6). A `VarLookupFunc` returning an
//! `_xmlXPathObject` transfers ownership to the caller. Context state is
//! single-threaded per evaluation; the recursion guard bounds nesting.
//!
//! # Historical quirks & epochs
//!
//! R-000162: the C XPath function registry was a stub that always errored
//! ('C extension function cannot be called') until the 11.1-L callback
//! audit built the parser-context bridge — registered functions now run
//! with oracle-verified semantics. The recursion guard mirrors upstream
//! depth handling introduced in the hardening epochs (SEC-0001 lineage).
//!
//! # Deliberate oddities
//!
//! The synthesized parser context is an internal adapter, not the full
//! upstream xmlXPathParserContext: only the value-stack operations that
//! C extension functions observe are modeled.
//!
//! # Proving courts
//!
//! XPATH-CONTEXT-* and CALLBACK-001 (courts/suites/data-abi/callback-
//! family-probe.c) verify registered C function invocation byte-identical
//! against the oracle; cargo test covers variable/function resolution.
//!
//! # Tempting simplifications that would break parity
//!
//! Do not drop the C-callback bridge back to a Rust-only registry: XSLT
//! extension functions and C consumers register raw function pointers
//! through xmlXPathRegisterFunc/xmlXPathRegisterFuncNS and observe them
//! firing (R-000162).
//! Do not remove the recursion guard — deep expressions must fail like
//! the oracle, not overflow the stack.

use crate::abi::structs::{_xmlDoc, _xmlNode, _xmlXPathContext, _xmlXPathObject};
use crate::abi::types::xmlChar;
use crate::xml::xpath::types::XPathValue;
use std::collections::HashMap;
use std::os::raw::c_void;

// ═══════════════════════════════════════════════════════════════════════════════
// Type Aliases
// ═══════════════════════════════════════════════════════════════════════════════

/// XPath extension function signature.
///
/// Registered extension functions receive a mutable reference to the current
/// evaluation context and a slice of already-evaluated argument values.
/// They return an `XPathValue` on success or an error string on failure.
pub type XPathFunction = fn(&mut XPathContext, &[XPathValue]) -> Result<XPathValue, String>;

/// A boxed, capture-capable XPath function implementation.
///
/// Plain function pointers coerce into this via boxing; capturing closures
/// (e.g. EXSLT `func:function` bodies) can also be stored. Used for the
/// extension-function registry and the EXSLT registry.
pub type BoxedXPathFunction =
    Box<dyn Fn(&mut XPathContext, &[XPathValue]) -> Result<XPathValue, String> + Send + Sync>;

/// A namespaced function-lookup fallback: consulted by `lookup_function`
/// after the exact-name registry misses, so engines (XSLT) can resolve
/// `prefix:local(...)` calls against their own extension registries.
pub type FunctionLookupFn =
    Box<dyn Fn(&XPathContext, &str) -> Option<BoxedXPathFunction> + Send + Sync>;

/// C callback for variable lookup.
///
/// SAFETY: This is called from C ABI boundaries (e.g. when libxml2's XPath
/// evaluator invokes the variable lookup hook). The implementation must not
/// panic and must handle null pointers gracefully.
///
/// * `data` — user-supplied data pointer (the `var_lookup_data` field).
/// * `ns`   — namespace URI of the variable (may be null for no namespace).
/// * `name` — local part of the variable name.
///
/// Returns a pointer to an `_xmlXPathObject` that the caller takes ownership
/// of, or null if the variable is not found.
pub type VarLookupFunc =
    unsafe extern "C" fn(*mut c_void, *const xmlChar, *const xmlChar) -> *mut _xmlXPathObject;

/// C callback for function lookup.
///
/// SAFETY: Called from C ABI boundaries. The implementation must not panic
/// and must handle null pointers gracefully.
///
/// * `data` — user-supplied data pointer (the `func_lookup_data` field).
/// * `ns`   — namespace URI of the function (may be null for no namespace).
/// * `name` — local part of the function name.
///
/// Returns an opaque pointer to a function implementation, or null if the
/// function is not found. The interpretation of the returned pointer is
/// defined by the caller that registered the callback.
pub type FuncLookupFunc =
    unsafe extern "C" fn(*mut c_void, *const xmlChar, *const xmlChar) -> *mut c_void;

// ═══════════════════════════════════════════════════════════════════════════════
// XPathContext
// ═══════════════════════════════════════════════════════════════════════════════

/// XPath 1.0 evaluation context.
///
/// Carries all state required to evaluate an XPath expression:
///
/// * **Document and node** — the current XML document and context node.
/// * **Context position/size** — for `position()` and `last()`.
/// * **Variable bindings** — in-scope XPath variables.
/// * **Namespace bindings** — prefix-to-URI mappings.
/// * **Extension functions** — registered extension functions.
/// * **Recursion guard** — depth counter to prevent infinite recursion.
/// * **C callbacks** — hooks for variable and function lookup from the C ABI.
///
/// # Lifetime / Safety
///
/// The context borrows raw pointers to the document and nodes. It is the
/// caller's responsibility to ensure those pointers remain valid for the
/// duration of evaluation. The context does **not** own the document tree.
pub struct XPathContext {
    /// The current XML document.
    pub document: *mut _xmlDoc,

    /// The current context node.
    pub context_node: *mut _xmlNode,

    /// Position of the context node within the context list (1-based).
    pub context_position: i32,

    /// Size of the context list.
    pub context_size: i32,

    /// Bound variables (name → value).
    pub variables: HashMap<String, XPathValue>,

    /// Namespace bindings (prefix → URI).
    pub namespaces: HashMap<String, String>,

    /// Registered extension functions (name → function).
    pub functions: HashMap<String, BoxedXPathFunction>,

    /// Namespaced function-lookup fallback (e.g. the XSLT extension
    /// function registry), consulted after `functions`.
    pub function_lookup: Option<FunctionLookupFn>,

    /// Last error message, if any.
    pub error: Option<String>,

    /// Current proximity position (for `last()` / `position()`).
    pub proximity_position: i32,

    /// The context list for `position()` / `last()`.
    pub context_list: Vec<*mut _xmlNode>,

    /// Recursion depth counter (to prevent infinite recursion).
    pub recursion_depth: u32,

    /// C callback for variable lookup.
    pub var_lookup_func: Option<VarLookupFunc>,

    /// Opaque data pointer passed to `var_lookup_func`.
    pub var_lookup_data: *mut c_void,

    /// C callback for function lookup.
    pub func_lookup_func: Option<FuncLookupFunc>,

    /// Opaque data pointer passed to `func_lookup_func`.
    pub func_lookup_data: *mut c_void,

    /// The C-visible `_xmlXPathContext` this internal context belongs to
    /// (set by `xmlXPathNewContext`); needed to invoke C-registered
    /// extension functions through the `xmlXPathParserContext` protocol.
    pub c_context: *mut _xmlXPathContext,
}

impl std::fmt::Debug for XPathContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The extension-function map holds boxed callables and cannot be
        // formatted; report the registered names instead.
        let names: Vec<&String> = self.functions.keys().collect();
        f.debug_struct("XPathContext")
            .field("document", &self.document)
            .field("context_node", &self.context_node)
            .field("context_position", &self.context_position)
            .field("context_size", &self.context_size)
            .field("variables", &self.variables)
            .field("namespaces", &self.namespaces)
            .field("functions", &names)
            .field("error", &self.error)
            .field("recursion_depth", &self.recursion_depth)
            .finish()
    }
}

impl Clone for XPathContext {
    fn clone(&self) -> Self {
        // Extension functions are not cloned (boxed callables cannot be
        // duplicated); the clone carries the same state otherwise.
        let mut cloned = XPathContext::new(self.document);
        cloned.context_node = self.context_node;
        cloned.context_position = self.context_position;
        cloned.context_size = self.context_size;
        cloned.variables = self.variables.clone();
        cloned.namespaces = self.namespaces.clone();
        cloned.error = self.error.clone();
        cloned.proximity_position = self.proximity_position;
        cloned.context_list = self.context_list.clone();
        cloned.recursion_depth = self.recursion_depth;
        cloned.var_lookup_func = self.var_lookup_func;
        cloned.var_lookup_data = self.var_lookup_data;
        cloned.func_lookup_func = self.func_lookup_func;
        cloned.func_lookup_data = self.func_lookup_data;
        cloned.c_context = self.c_context;
        cloned
    }
}

impl XPathContext {
    /// Create a new XPath evaluation context for the given document.
    ///
    /// The context is initialised with:
    /// * The document pointer set to `doc`.
    /// * No context node (`null`).
    /// * Context position = 1, context size = 1 (defaults per XPath 1.0).
    /// * Empty variable, namespace, and function tables.
    /// * No error.
    /// * Proximity position = 1.
    /// * Empty context list.
    /// * Recursion depth = 0.
    /// * No C callbacks registered.
    /// * Callback data pointers set to null.
    pub fn new(doc: *mut _xmlDoc) -> Self {
        Self {
            document: doc,
            context_node: std::ptr::null_mut(),
            context_position: 1,
            context_size: 1,
            variables: HashMap::new(),
            namespaces: HashMap::new(),
            functions: HashMap::new(),
            function_lookup: None,
            error: None,
            proximity_position: 1,
            context_list: Vec::new(),
            recursion_depth: 0,
            var_lookup_func: None,
            var_lookup_data: std::ptr::null_mut(),
            func_lookup_func: None,
            func_lookup_data: std::ptr::null_mut(),
            c_context: std::ptr::null_mut(),
        }
    }

    /// Set the context node and update context position / size.
    ///
    /// If `node` is non-null, the context list is set to a single-element
    /// list containing only that node, and both `context_position` and
    /// `context_size` are set to 1.
    ///
    /// If `node` is null, the context list is cleared and both
    /// `context_position` and `context_size` are set to 1.
    pub fn set_context_node(&mut self, node: *mut _xmlNode) {
        self.context_node = node;
        if node.is_null() {
            self.context_list.clear();
            self.context_position = 1;
            self.context_size = 1;
            self.proximity_position = 1;
        } else {
            self.context_list = vec![node];
            self.context_position = 1;
            self.context_size = 1;
            self.proximity_position = 1;
        }
    }

    /// Set the context list for `position()` / `last()`.
    ///
    /// Updates `context_list`, `context_size`, and resets
    /// `context_position` and `proximity_position` to 1.
    ///
    /// The context node is not changed by this call; use
    /// [`set_context_node`](Self::set_context_node) to update it.
    pub fn set_context_list(&mut self, nodes: Vec<*mut _xmlNode>) {
        self.context_size = nodes.len() as i32;
        self.context_list = nodes;
        self.context_position = 1;
        self.proximity_position = 1;
    }

    /// Look up a variable by name.
    ///
    /// Checks the local `variables` map first. If the variable is not found
    /// there, and a `var_lookup_func` callback is registered, the callback
    /// is invoked with the variable name and its namespace (currently passed
    /// as null since our Rust-side variables have no namespace component).
    ///
    /// Returns `None` if the variable is not bound.
    ///
    /// # Note
    ///
    /// When the C callback path is used, the returned `_xmlXPathObject` is
    /// converted into an `XPathValue`. Currently this path is a placeholder;
    /// a full implementation would call into `xmlXPathObject` conversion
    /// routines.
    pub fn resolve_variable(&self, name: &str) -> Option<XPathValue> {
        // Check local Rust-side variables first.
        if let Some(value) = self.variables.get(name) {
            return Some(value.clone());
        }

        // Fall back to the C callback if registered.
        if let Some(lookup) = self.var_lookup_func {
            // Convert the name to a C string (xmlChar*).
            let c_name: Vec<xmlChar> = name.bytes().collect();
            // SAFETY: We call the C callback with the user-provided data pointer.
            // The callback must not panic and must handle null inputs gracefully.
            let result = unsafe { lookup(self.var_lookup_data, std::ptr::null(), c_name.as_ptr()) };
            if !result.is_null() {
                // TODO: Convert _xmlXPathObject to XPathValue.
                // For now, free the object and return a placeholder.
                // In a full implementation this would inspect result.type_
                // and extract the appropriate value.
                {
                    // We cannot easily convert without more ABI support.
                    // Return None for now — the C callback path is for
                    // interop scenarios where the caller handles conversion.
                    let _ = result; // would free with xmlXPathFreeObject
                }
            }
        }

        None
    }

    /// Look up a namespace URI by prefix.
    ///
    /// Checks the local `namespaces` map first. If the prefix is not found
    /// there, it falls back to scanning the namespace definitions on the
    /// context node (`nsDef` chain) and its ancestors.
    ///
    /// Returns `None` if the prefix is not bound.
    pub fn resolve_namespace(&self, prefix: &str) -> Option<String> {
        // Check local bindings first.
        if let Some(uri) = self.namespaces.get(prefix) {
            return Some(uri.clone());
        }

        // Fall back to scanning the node's namespace definitions.
        // Walk up the ancestor chain looking for nsDef declarations.
        let mut current = self.context_node;
        while !current.is_null() {
            // SAFETY: We dereference raw pointers up the parent chain.
            // The caller guarantees these pointers remain valid.
            unsafe {
                let mut ns = (*current).nsDef;
                while !ns.is_null() {
                    let ns_prefix = (*ns).prefix;
                    let ns_href = (*ns).href;

                    // Compare prefix.
                    let prefix_matches = if ns_prefix.is_null() {
                        // Default namespace (no prefix) — only matches
                        // if the caller is asking for the default namespace.
                        prefix.is_empty()
                    } else {
                        // Read the prefix as a C string and compare.
                        let mut len = 0;
                        while *ns_prefix.add(len) != 0 {
                            len += 1;
                        }
                        let slice = std::slice::from_raw_parts(ns_prefix, len);
                        slice == prefix.as_bytes()
                    };

                    if prefix_matches {
                        // Read the href as a Rust String.
                        let mut len = 0;
                        while *ns_href.add(len) != 0 {
                            len += 1;
                        }
                        let slice = std::slice::from_raw_parts(ns_href, len);
                        return Some(String::from_utf8_lossy(slice).into_owned());
                    }

                    ns = (*ns).next;
                }
            }

            // Move to parent.
            // SAFETY: The node tree is valid for the lifetime of the context.
            unsafe {
                current = (*current).parent;
            }
        }

        None
    }

    /// Look up a registered extension function by name.
    ///
    /// Checks the local `functions` map first. If not found, and a
    /// `func_lookup_func` callback is registered, the callback is invoked.
    ///
    /// Returns `None` if no such function is registered.
    pub fn lookup_function(&mut self, name: &str) -> Option<&BoxedXPathFunction> {
        // Check local Rust-side functions first.
        if self.functions.contains_key(name) {
            return self.functions.get(name);
        }

        // Namespaced fallback (XSLT extension functions, EXSLT, ...): the
        // resolved closure is memoised into the registry.
        if let Some(lookup) = &self.function_lookup {
            if let Some(func) = lookup(self, name) {
                self.functions.insert(name.to_string(), func);
                return self.functions.get(name);
            }
        }

        // C-registered extension functions (xmlXPathRegisterFuncLookup) are
        // resolved and invoked through the parser-context protocol in
        // eval_function_call — they are NOT resolved here (a bare callback
        // call with a non-NUL-terminated name crashes C consumers like
        // nokogiri's handler lookup).
        None
    }

    /// Register an extension function.
    ///
    /// The function is stored in the local `functions` map under `name`.
    /// It will be found by [`lookup_function`](Self::lookup_function) before
    /// any C callback is consulted. Accepts both fn pointers and capturing
    /// closures.
    pub fn register_function<F>(&mut self, name: &str, func: F)
    where
        F: Fn(&mut XPathContext, &[XPathValue]) -> Result<XPathValue, String>
            + Send
            + Sync
            + 'static,
    {
        self.functions.insert(name.to_string(), Box::new(func));
    }

    /// Register a variable binding.
    ///
    /// The variable is stored in the local `variables` map under `name`.
    /// It will be found by [`resolve_variable`](Self::resolve_variable) before
    /// any C callback is consulted.
    pub fn register_variable(&mut self, name: &str, value: XPathValue) {
        self.variables.insert(name.to_string(), value);
    }

    /// Remove a variable binding from the context's variable hash.
    ///
    /// Used to unwind local XSLT variable scopes when a variable is popped
    /// from the transform variable stack.
    pub fn unregister_variable(&mut self, name: &str) {
        self.variables.remove(name);
    }

    /// Register a namespace binding.
    ///
    /// Maps `prefix` to `uri` in the local `namespaces` map.
    /// An empty prefix registers the default namespace.
    pub fn register_namespace(&mut self, prefix: &str, uri: &str) {
        self.namespaces.insert(prefix.to_string(), uri.to_string());
    }

    /// Record an error message.
    ///
    /// Overwrites any previously recorded error. Use `clear_error` to reset.
    pub fn set_error(&mut self, msg: &str) {
        self.error = Some(msg.to_string());
    }

    /// Clear any recorded error.
    pub fn clear_error(&mut self) {
        self.error = None;
    }

    /// Push onto the recursion stack.
    ///
    /// Increments `recursion_depth`. If the depth exceeds a reasonable limit
    /// (currently 1000), returns `Err` with an overflow message.
    ///
    /// Callers should invoke this before recursing into expression evaluation
    /// and call [`pop_recursion`](Self::pop_recursion) after returning.
    pub fn push_recursion(&mut self) -> Result<(), String> {
        const MAX_RECURSION_DEPTH: u32 = 1000;
        if self.recursion_depth >= MAX_RECURSION_DEPTH {
            return Err(
                "XPath evaluation recursion depth exceeded (infinite recursion?)".to_string(),
            );
        }
        self.recursion_depth += 1;
        Ok(())
    }

    /// Pop from the recursion stack.
    ///
    /// Decrements `recursion_depth`. Must be called after a corresponding
    /// [`push_recursion`](Self::push_recursion).
    ///
    /// # Panics
    ///
    /// Panics if `recursion_depth` is already 0 (indicating unbalanced
    /// push/pop calls).
    pub fn pop_recursion(&mut self) {
        assert!(
            self.recursion_depth > 0,
            "unbalanced pop_recursion: recursion_depth is already 0"
        );
        self.recursion_depth -= 1;
    }

    /// Returns `true` if a context node is set (non-null).
    pub const fn has_context_node(&self) -> bool {
        !self.context_node.is_null()
    }

    /// Reset the context to its initial state, keeping the document pointer.
    ///
    /// Clears the context node, context list, error, and recursion depth.
    /// Variable, namespace, and function bindings are preserved.
    pub fn reset(&mut self) {
        self.context_node = std::ptr::null_mut();
        self.context_position = 1;
        self.context_size = 1;
        self.error = None;
        self.proximity_position = 1;
        self.context_list.clear();
        self.recursion_depth = 0;
    }

    /// Returns the current proximity position (1-based).
    ///
    /// Equivalent to the XPath `position()` function.
    pub const fn position(&self) -> i32 {
        self.proximity_position
    }

    /// Returns the context size.
    ///
    /// Equivalent to the XPath `last()` function.
    pub const fn last(&self) -> i32 {
        self.context_size
    }

    /// Advance the proximity position by one.
    ///
    /// Called when iterating over the context list during predicate
    /// evaluation.
    pub const fn advance_position(&mut self) {
        self.proximity_position += 1;
        self.context_position = self.proximity_position;
    }

    /// Rewind the proximity position to 1.
    pub const fn reset_position(&mut self) {
        self.proximity_position = 1;
        self.context_position = 1;
    }
}

impl Default for XPathContext {
    /// Create a default context with a null document pointer.
    ///
    /// This is useful when you need a context for testing or when the
    /// document will be set later via [`set_context_node`](Self::set_context_node).
    fn default() -> Self {
        Self::new(std::ptr::null_mut())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    use crate::xml::xpath::types::NodeSet;

    // ── Helpers ──────────────────────────────────────────────────────────

    /// Create a minimal _xmlDoc for testing.
    ///
    /// SAFETY: The caller is responsible for freeing the allocated doc.
    unsafe fn create_test_doc() -> *mut _xmlDoc {
        // Allocate zeroed memory for a minimal document.
        let layout = std::alloc::Layout::new::<_xmlDoc>();
        let ptr = std::alloc::alloc_zeroed(layout) as *mut _xmlDoc;
        assert!(!ptr.is_null(), "failed to allocate test document");
        ptr
    }

    /// Create a minimal _xmlNode for testing.
    ///
    /// SAFETY: The caller is responsible for freeing the allocated node.
    unsafe fn create_test_node() -> *mut _xmlNode {
        let layout = std::alloc::Layout::new::<_xmlNode>();
        let ptr = std::alloc::alloc_zeroed(layout) as *mut _xmlNode;
        assert!(!ptr.is_null(), "failed to allocate test node");
        ptr
    }

    /// SAFETY: Frees a test document allocated with `create_test_doc`.
    unsafe fn free_test_doc(doc: *mut _xmlDoc) {
        if !doc.is_null() {
            let layout = std::alloc::Layout::new::<_xmlDoc>();
            std::alloc::dealloc(doc as *mut u8, layout);
        }
    }

    /// SAFETY: Frees a test node allocated with `create_test_node`.
    unsafe fn free_test_node(node: *mut _xmlNode) {
        if !node.is_null() {
            let layout = std::alloc::Layout::new::<_xmlNode>();
            std::alloc::dealloc(node as *mut u8, layout);
        }
    }

    // ── Construction ─────────────────────────────────────────────────────

    #[test]
    fn test_new_context() {
        let ctx = XPathContext::new(std::ptr::null_mut());
        assert!(ctx.document.is_null());
        assert!(ctx.context_node.is_null());
        assert_eq!(ctx.context_position, 1);
        assert_eq!(ctx.context_size, 1);
        assert!(ctx.variables.is_empty());
        assert!(ctx.namespaces.is_empty());
        assert!(ctx.functions.is_empty());
        assert!(ctx.error.is_none());
        assert_eq!(ctx.proximity_position, 1);
        assert!(ctx.context_list.is_empty());
        assert_eq!(ctx.recursion_depth, 0);
        assert!(ctx.var_lookup_func.is_none());
        assert!(ctx.var_lookup_data.is_null());
        assert!(ctx.func_lookup_func.is_none());
        assert!(ctx.func_lookup_data.is_null());
    }

    #[test]
    fn test_default_context() {
        let ctx = XPathContext::default();
        assert!(ctx.document.is_null());
        assert_eq!(ctx.context_position, 1);
    }

    /// Create a context with a document and check it is recorded.
    ///
    /// # Safety
    ///
    /// - `doc` is a valid, aligned `_xmlDoc` allocated by `create_test_doc`
    ///   and freed with `free_test_doc` exactly once; it is only stored as
    ///   a pointer, never dereferenced, during the test.
    #[test]
    fn test_new_with_doc() {
        unsafe {
            let doc = create_test_doc();
            let ctx = XPathContext::new(doc);
            assert_eq!(ctx.document, doc);
            free_test_doc(doc);
        }
    }

    // ── set_context_node ─────────────────────────────────────────────────

    /// Set a non-NULL context node and verify position state.
    ///
    /// # Safety
    ///
    /// - `node` is a valid, aligned `_xmlNode` allocated by
    ///   `create_test_node` and freed with `free_test_node` exactly once;
    ///   the context only stores and compares the pointer.
    #[test]
    fn test_set_context_node_non_null() {
        unsafe {
            let node = create_test_node();
            let mut ctx = XPathContext::new(std::ptr::null_mut());
            ctx.set_context_node(node);

            assert_eq!(ctx.context_node, node);
            assert_eq!(ctx.context_position, 1);
            assert_eq!(ctx.context_size, 1);
            assert_eq!(ctx.proximity_position, 1);
            assert_eq!(ctx.context_list.len(), 1);
            assert_eq!(ctx.context_list[0], node);

            free_test_node(node);
        }
    }

    #[test]
    fn test_set_context_node_null() {
        let mut ctx = XPathContext::new(std::ptr::null_mut());
        // Set a non-null node first.
        // SAFETY: We use a dangling pointer as a sentinel — it won't be dereferenced.
        let sentinel = std::ptr::dangling_mut::<_xmlNode>();
        ctx.context_list = vec![sentinel];
        ctx.context_position = 5;
        ctx.context_size = 5;
        ctx.proximity_position = 5;

        // Now set to null — should reset everything.
        ctx.set_context_node(std::ptr::null_mut());
        assert!(ctx.context_node.is_null());
        assert!(ctx.context_list.is_empty());
        assert_eq!(ctx.context_position, 1);
        assert_eq!(ctx.context_size, 1);
        assert_eq!(ctx.proximity_position, 1);
    }

    // ── set_context_list ─────────────────────────────────────────────────

    /// Set a context list and verify size and position initialization.
    ///
    /// # Safety
    ///
    /// - `node1`/`node2` are valid, aligned `_xmlNode`s allocated by
    ///   `create_test_node` and freed with `free_test_node`; the context
    ///   stores the pointers in a `Vec` without dereferencing them.
    #[test]
    fn test_set_context_list() {
        unsafe {
            let node1 = create_test_node();
            let node2 = create_test_node();
            let nodes = vec![node1, node2];

            let mut ctx = XPathContext::new(std::ptr::null_mut());
            ctx.set_context_list(nodes.clone());

            assert_eq!(ctx.context_list.len(), 2);
            assert_eq!(ctx.context_size, 2);
            assert_eq!(ctx.context_position, 1);
            assert_eq!(ctx.proximity_position, 1);

            free_test_node(node1);
            free_test_node(node2);
        }
    }

    #[test]
    fn test_set_context_list_empty() {
        let mut ctx = XPathContext::new(std::ptr::null_mut());
        ctx.set_context_list(vec![]);

        assert!(ctx.context_list.is_empty());
        assert_eq!(ctx.context_size, 0);
        assert_eq!(ctx.context_position, 1);
    }

    // ── Variables ────────────────────────────────────────────────────────

    #[test]
    fn test_register_and_resolve_variable() {
        let mut ctx = XPathContext::new(std::ptr::null_mut());
        ctx.register_variable("foo", XPathValue::String("bar".to_string()));

        let result = ctx.resolve_variable("foo");
        assert!(result.is_some());
        assert_eq!(result.unwrap().as_string(), "bar");
    }

    #[test]
    fn test_resolve_unknown_variable() {
        let ctx = XPathContext::new(std::ptr::null_mut());
        assert!(ctx.resolve_variable("nonexistent").is_none());
    }
    #[allow(clippy::approx_constant)]
    #[test]
    fn test_register_variable_number() {
        let mut ctx = XPathContext::new(std::ptr::null_mut());
        ctx.register_variable("pi", XPathValue::Number(3.14159));

        let result = ctx.resolve_variable("pi");
        assert!(result.is_some());
        let val = result.unwrap();
        assert!((val.as_number() - 3.14159).abs() < 1e-10);
    }

    #[test]
    fn test_register_variable_boolean() {
        let mut ctx = XPathContext::new(std::ptr::null_mut());
        ctx.register_variable("flag", XPathValue::Boolean(true));

        let result = ctx.resolve_variable("flag");
        assert!(result.is_some());
        assert!(result.unwrap().as_boolean());
    }

    #[test]
    fn test_register_variable_nodeset() {
        let mut ctx = XPathContext::new(std::ptr::null_mut());
        let ns = NodeSet::new();
        ctx.register_variable("nodes", XPathValue::NodeSet(ns));

        let result = ctx.resolve_variable("nodes");
        assert!(result.is_some());
        assert!(matches!(result.unwrap(), XPathValue::NodeSet(_)));
    }

    #[test]
    fn test_variable_overwrite() {
        let mut ctx = XPathContext::new(std::ptr::null_mut());
        ctx.register_variable("x", XPathValue::Number(1.0));
        ctx.register_variable("x", XPathValue::Number(2.0));

        let result = ctx.resolve_variable("x");
        assert!(result.is_some());
        assert!((result.unwrap().as_number() - 2.0).abs() < 1e-10);
    }

    // ── Namespaces ───────────────────────────────────────────────────────

    #[test]
    fn test_register_and_resolve_namespace() {
        let mut ctx = XPathContext::new(std::ptr::null_mut());
        ctx.register_namespace("xslt", "http://www.w3.org/1999/XSL/Transform");

        let result = ctx.resolve_namespace("xslt");
        assert!(result.is_some());
        assert_eq!(result.unwrap(), "http://www.w3.org/1999/XSL/Transform");
    }

    #[test]
    fn test_resolve_unknown_namespace() {
        let ctx = XPathContext::new(std::ptr::null_mut());
        // With no context node and no bindings, this should return None.
        assert!(ctx.resolve_namespace("unknown").is_none());
    }

    #[test]
    fn test_register_default_namespace() {
        let mut ctx = XPathContext::new(std::ptr::null_mut());
        ctx.register_namespace("", "http://example.com/default");

        let result = ctx.resolve_namespace("");
        assert!(result.is_some());
        assert_eq!(result.unwrap(), "http://example.com/default");
    }

    #[test]
    fn test_namespace_overwrite() {
        let mut ctx = XPathContext::new(std::ptr::null_mut());
        ctx.register_namespace("a", "http://example.com/1");
        ctx.register_namespace("a", "http://example.com/2");

        let result = ctx.resolve_namespace("a");
        assert_eq!(result.unwrap(), "http://example.com/2");
    }

    // ── Functions ────────────────────────────────────────────────────────

    #[test]
    fn test_register_and_lookup_function() {
        fn test_func(_ctx: &mut XPathContext, _args: &[XPathValue]) -> Result<XPathValue, String> {
            Ok(XPathValue::String("test".to_string()))
        }

        let mut ctx = XPathContext::new(std::ptr::null_mut());
        ctx.register_function("test:func", test_func);

        let result = ctx.lookup_function("test:func");
        assert!(result.is_some());
    }

    #[test]
    fn test_lookup_unknown_function() {
        let mut ctx = XPathContext::new(std::ptr::null_mut());
        assert!(ctx.lookup_function("nonexistent").is_none());
    }

    #[test]
    fn test_function_overwrite() {
        fn func_a(_ctx: &mut XPathContext, _args: &[XPathValue]) -> Result<XPathValue, String> {
            Ok(XPathValue::String("a".to_string()))
        }
        fn func_b(_ctx: &mut XPathContext, _args: &[XPathValue]) -> Result<XPathValue, String> {
            Ok(XPathValue::String("b".to_string()))
        }

        let mut ctx = XPathContext::new(std::ptr::null_mut());
        ctx.register_function("f", func_a);
        ctx.register_function("f", func_b);

        let result = ctx.lookup_function("f");
        assert!(result.is_some());

        // The overwritten function should be func_b.
        if let Some(f) = result {
            let mut tmp_ctx = XPathContext::new(std::ptr::null_mut());
            let value = f(&mut tmp_ctx, &[]).unwrap();
            assert_eq!(value.as_string(), "b");
        }
    }

    // ── Error handling ───────────────────────────────────────────────────

    #[test]
    fn test_set_and_get_error() {
        let mut ctx = XPathContext::new(std::ptr::null_mut());
        assert!(ctx.error.is_none());

        ctx.set_error("something went wrong");
        assert_eq!(ctx.error.as_deref(), Some("something went wrong"));
    }

    #[test]
    fn test_clear_error() {
        let mut ctx = XPathContext::new(std::ptr::null_mut());
        ctx.set_error("an error");
        assert!(ctx.error.is_some());

        ctx.clear_error();
        assert!(ctx.error.is_none());
    }

    #[test]
    fn test_error_overwrite() {
        let mut ctx = XPathContext::new(std::ptr::null_mut());
        ctx.set_error("first error");
        ctx.set_error("second error");
        assert_eq!(ctx.error.as_deref(), Some("second error"));
    }

    // ── Recursion depth ──────────────────────────────────────────────────

    #[test]
    fn test_push_pop_recursion() {
        let mut ctx = XPathContext::new(std::ptr::null_mut());
        assert_eq!(ctx.recursion_depth, 0);

        assert!(ctx.push_recursion().is_ok());
        assert_eq!(ctx.recursion_depth, 1);

        ctx.pop_recursion();
        assert_eq!(ctx.recursion_depth, 0);
    }

    #[test]
    fn test_recursion_depth_limit() {
        let mut ctx = XPathContext::new(std::ptr::null_mut());

        // Push to the limit (1000).
        for _ in 0..1000 {
            assert!(ctx.push_recursion().is_ok());
        }
        assert_eq!(ctx.recursion_depth, 1000);

        // The next push should fail.
        let result = ctx.push_recursion();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("recursion depth exceeded"));

        // Pop back down.
        for _ in 0..1000 {
            ctx.pop_recursion();
        }
        assert_eq!(ctx.recursion_depth, 0);
    }

    #[test]
    #[should_panic(expected = "unbalanced pop_recursion")]
    fn test_pop_recursion_underflow() {
        let mut ctx = XPathContext::new(std::ptr::null_mut());
        ctx.pop_recursion(); // depth is 0 — should panic
    }

    #[test]
    fn test_recursion_nesting() {
        let mut ctx = XPathContext::new(std::ptr::null_mut());

        // Simulate nested evaluation.
        assert!(ctx.push_recursion().is_ok());
        assert!(ctx.push_recursion().is_ok());
        assert!(ctx.push_recursion().is_ok());
        assert_eq!(ctx.recursion_depth, 3);

        ctx.pop_recursion();
        assert_eq!(ctx.recursion_depth, 2);

        ctx.pop_recursion();
        assert_eq!(ctx.recursion_depth, 1);

        ctx.pop_recursion();
        assert_eq!(ctx.recursion_depth, 0);
    }

    // ── Position / Size ──────────────────────────────────────────────────

    #[test]
    fn test_position_and_last() {
        let ctx = XPathContext::new(std::ptr::null_mut());
        assert_eq!(ctx.position(), 1);
        assert_eq!(ctx.last(), 1);
    }

    #[test]
    fn test_advance_position() {
        let mut ctx = XPathContext::new(std::ptr::null_mut());
        ctx.advance_position();
        assert_eq!(ctx.position(), 2);
        assert_eq!(ctx.proximity_position, 2);
        assert_eq!(ctx.context_position, 2);
    }

    #[test]
    fn test_reset_position() {
        let mut ctx = XPathContext::new(std::ptr::null_mut());
        ctx.advance_position();
        ctx.advance_position();
        ctx.advance_position();
        assert_eq!(ctx.position(), 4);

        ctx.reset_position();
        assert_eq!(ctx.position(), 1);
        assert_eq!(ctx.context_position, 1);
    }

    /// Advance the position across a three-node context list.
    ///
    /// # Safety
    ///
    /// - The three nodes are valid, aligned `_xmlNode`s allocated by
    ///   `create_test_node` and freed with `free_test_node`; the context
    ///   only stores and counts the pointers.
    #[test]
    fn test_position_with_context_list() {
        unsafe {
            let node1 = create_test_node();
            let node2 = create_test_node();
            let node3 = create_test_node();
            let nodes = vec![node1, node2, node3];

            let mut ctx = XPathContext::new(std::ptr::null_mut());
            ctx.set_context_list(nodes);

            assert_eq!(ctx.last(), 3);
            assert_eq!(ctx.position(), 1);

            ctx.advance_position();
            assert_eq!(ctx.position(), 2);

            ctx.advance_position();
            assert_eq!(ctx.position(), 3);

            free_test_node(node1);
            free_test_node(node2);
            free_test_node(node3);
        }
    }

    // ── has_context_node ─────────────────────────────────────────────────

    /// Check `has_context_node` before and after setting a node.
    ///
    /// # Safety
    ///
    /// - `node` is a valid, aligned `_xmlNode` allocated by
    ///   `create_test_node` and freed with `free_test_node` exactly once;
    ///   `has_context_node` only checks the stored pointer for NULL.
    #[test]
    fn test_has_context_node() {
        let mut ctx = XPathContext::new(std::ptr::null_mut());
        assert!(!ctx.has_context_node());

        unsafe {
            let node = create_test_node();
            ctx.set_context_node(node);
            assert!(ctx.has_context_node());
            free_test_node(node);
        }
    }

    // ── reset ────────────────────────────────────────────────────────────

    #[test]
    fn test_reset() {
        let mut ctx = XPathContext::new(std::ptr::null_mut());

        // Set up some state.
        ctx.set_error("test error");
        ctx.proximity_position = 5;
        ctx.context_position = 5;
        ctx.context_size = 10;
        ctx.recursion_depth = 3;
        {
            let sentinel = std::ptr::dangling_mut::<_xmlNode>();
            ctx.context_list = vec![sentinel];
        }

        // Register some bindings — these should survive reset.
        ctx.register_variable("x", XPathValue::Number(42.0));
        ctx.register_namespace("p", "http://example.com/ns");
        fn dummy(_: &mut XPathContext, _: &[XPathValue]) -> Result<XPathValue, String> {
            Ok(XPathValue::Boolean(true))
        }
        ctx.register_function("f", dummy);

        ctx.reset();

        // Context node and position should be reset.
        assert!(ctx.context_node.is_null());
        assert_eq!(ctx.context_position, 1);
        assert_eq!(ctx.context_size, 1);
        assert_eq!(ctx.proximity_position, 1);
        assert!(ctx.error.is_none());
        assert!(ctx.context_list.is_empty());
        assert_eq!(ctx.recursion_depth, 0);

        // Bindings should be preserved.
        assert!(ctx.resolve_variable("x").is_some());
        assert!(ctx.resolve_namespace("p").is_some());
        assert!(ctx.lookup_function("f").is_some());
    }

    // ── C callback fields ────────────────────────────────────────────────

    #[test]
    fn test_callback_fields_default_to_none() {
        let ctx = XPathContext::new(std::ptr::null_mut());
        assert!(ctx.var_lookup_func.is_none());
        assert!(ctx.var_lookup_data.is_null());
        assert!(ctx.func_lookup_func.is_none());
        assert!(ctx.func_lookup_data.is_null());
    }

    #[test]
    fn test_set_callback_fields() {
        let mut ctx = XPathContext::new(std::ptr::null_mut());

        /// A no-op variable-lookup callback returning NULL.
        ///
        /// # Safety
        ///
        /// - The callback is never invoked by this test; if installed it
        ///   must be a valid function pointer, and `data`/`ns`/`name`
        ///   would need to be valid pointers if it were called.
        unsafe extern "C" fn dummy_var_lookup(
            _data: *mut c_void,
            _ns: *const xmlChar,
            _name: *const xmlChar,
        ) -> *mut _xmlXPathObject {
            std::ptr::null_mut()
        }

        /// A no-op function-lookup callback returning NULL.
        ///
        /// # Safety
        ///
        /// - The callback is never invoked by this test; if installed it
        ///   must be a valid function pointer, and `data`/`ns`/`name`
        ///   would need to be valid pointers if it were called.
        unsafe extern "C" fn dummy_func_lookup(
            _data: *mut c_void,
            _ns: *const xmlChar,
            _name: *const xmlChar,
        ) -> *mut c_void {
            std::ptr::null_mut()
        }

        let data_ptr = &mut 42u32 as *mut u32 as *mut c_void;

        ctx.var_lookup_func = Some(dummy_var_lookup);
        ctx.var_lookup_data = data_ptr;
        ctx.func_lookup_func = Some(dummy_func_lookup);
        ctx.func_lookup_data = data_ptr;

        assert!(ctx.var_lookup_func.is_some());
        assert!(!ctx.var_lookup_data.is_null());
        assert!(ctx.func_lookup_func.is_some());
        assert!(!ctx.func_lookup_data.is_null());
    }

    // ── Clone ────────────────────────────────────────────────────────────

    #[test]
    fn test_context_clone() {
        let mut ctx = XPathContext::new(std::ptr::null_mut());
        ctx.register_variable("x", XPathValue::Number(10.0));
        ctx.register_namespace("ns", "http://example.com/ns");
        ctx.set_error("clone test");

        let cloned = ctx.clone();
        assert_eq!(cloned.document, ctx.document);
        assert_eq!(cloned.context_node, ctx.context_node);
        assert_eq!(cloned.context_position, ctx.context_position);
        assert_eq!(cloned.context_size, ctx.context_size);
        assert_eq!(cloned.error, ctx.error);

        // Verify the clone has independent state.
        let var = cloned.resolve_variable("x");
        assert!(var.is_some());
        assert!((var.unwrap().as_number() - 10.0).abs() < 1e-10);

        let ns = cloned.resolve_namespace("ns");
        assert!(ns.is_some());
        assert_eq!(ns.unwrap(), "http://example.com/ns");
    }

    // ── Debug ────────────────────────────────────────────────────────────

    #[test]
    fn test_context_debug_format() {
        let ctx = XPathContext::new(std::ptr::null_mut());
        let debug_str = format!("{:?}", ctx);
        assert!(debug_str.contains("context_position"));
        assert!(debug_str.contains("context_size"));
        assert!(debug_str.contains("recursion_depth"));
    }

    // ── Edge cases ───────────────────────────────────────────────────────

    #[test]
    fn test_context_size_zero() {
        let mut ctx = XPathContext::new(std::ptr::null_mut());
        ctx.set_context_list(vec![]);
        assert_eq!(ctx.last(), 0);
        assert_eq!(ctx.position(), 1);
    }

    #[test]
    fn test_multiple_advancements() {
        let mut ctx = XPathContext::new(std::ptr::null_mut());
        for i in 1..=10 {
            assert_eq!(ctx.position(), i);
            ctx.advance_position();
        }
        assert_eq!(ctx.position(), 11);
    }

    #[test]
    fn test_register_multiple_variables() {
        let mut ctx = XPathContext::new(std::ptr::null_mut());
        ctx.register_variable("a", XPathValue::Number(1.0));
        ctx.register_variable("b", XPathValue::String("two".to_string()));
        ctx.register_variable("c", XPathValue::Boolean(true));

        assert_eq!(ctx.variables.len(), 3);
        assert_eq!(ctx.resolve_variable("a").unwrap().as_number(), 1.0);
        assert_eq!(ctx.resolve_variable("b").unwrap().as_string(), "two");
        assert!(ctx.resolve_variable("c").unwrap().as_boolean());
    }

    #[test]
    fn test_register_multiple_namespaces() {
        let mut ctx = XPathContext::new(std::ptr::null_mut());
        ctx.register_namespace("a", "http://example.com/a");
        ctx.register_namespace("b", "http://example.com/b");
        ctx.register_namespace("c", "http://example.com/c");

        assert_eq!(ctx.namespaces.len(), 3);
        assert_eq!(ctx.resolve_namespace("a").unwrap(), "http://example.com/a");
        assert_eq!(ctx.resolve_namespace("b").unwrap(), "http://example.com/b");
        assert_eq!(ctx.resolve_namespace("c").unwrap(), "http://example.com/c");
    }

    #[test]
    fn test_register_multiple_functions() {
        fn f1(_: &mut XPathContext, _: &[XPathValue]) -> Result<XPathValue, String> {
            Ok(XPathValue::Number(1.0))
        }
        fn f2(_: &mut XPathContext, _: &[XPathValue]) -> Result<XPathValue, String> {
            Ok(XPathValue::Number(2.0))
        }
        fn f3(_: &mut XPathContext, _: &[XPathValue]) -> Result<XPathValue, String> {
            Ok(XPathValue::Number(3.0))
        }

        let mut ctx = XPathContext::new(std::ptr::null_mut());
        ctx.register_function("f1", f1);
        ctx.register_function("f2", f2);
        ctx.register_function("f3", f3);

        assert_eq!(ctx.functions.len(), 3);
    }
}
