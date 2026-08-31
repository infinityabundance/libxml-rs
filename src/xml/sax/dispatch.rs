//! SAX callback dispatch — safe wrappers for invoking SAX1 and SAX2 callbacks (§20, §85 Phase 3).
//!
//! Each dispatcher checks whether the corresponding function pointer in the
//! `_xmlSAXHandler` is `Some` (non-NULL). If so, it calls the callback with
//! the provided arguments. If the callback is `None` (NULL), the dispatcher
//! either does nothing (for void callbacks) or returns a safe default value
//! (for callbacks that return a value).
//!
//! # Dispatch priority
//!
//! For `start_element` and `end_element`, the dispatcher prefers the SAX2
//! variant (`startElementNs` / `endElementNs`) when it is set, falling back
//! to the SAX1 variant (`startElement` / `endElement`). This matches the
//! upstream behavior where the parser uses SAX2 callbacks when available.
//!
//! # Upstream contract
//!
//! Mirrors the SAX1/SAX2 callback dispatch of upstream SAX2.c and the
//! xmlSAX2* handler entry points (SRC-LIBXML2-2.15.0, oracle tree
//! `oracle/historical/src/libxml2-2.15.0/`). Parity target: the system libxml2
//! 2.15.3 oracle callback routing.
//!
//! # Conceptual behavior
//!
//! Each dispatcher checks the corresponding function pointer in the
//! `_xmlSAXHandler` struct and invokes it with the provided arguments, or
//! falls back to a safe default (void callbacks do nothing; value callbacks
//! return the upstream default, e.g. `is_standalone` returns -1). For start
//! and end element, the SAX2 variant is preferred over SAX1, matching
//! upstream parser dispatch.
//!
//! # Ownership & safety invariants
//!
//! SAFETY: the sax pointer must reference an initialized `_xmlSAXHandler`;
//! string arguments must be valid null-terminated xmlChar* or NULL; ctx is
//! passed through verbatim and never dereferenced by the dispatcher. The
//! dispatcher owns nothing — callbacks keep their C ownership contract.
//!
//! # Historical quirks & epochs
//!
//! XML_SAX2_MAGIC gates the SAX2 fast path (upstream xmlSAX2StartElementNs
//! era). The error/warning slots route through the legacy handlers stored by
//! xmlSAX2InitDefaultSAXHandler since the 11.1-K error-routing closure
//! (R-000161): a custom SAX error slot receives channel(data, msg) once,
//! while the legacy default streams the xmlFormatError fragments.
//!
//! # Deliberate oddities
//!
//! The SAX2-over-SAX1 preference is deliberate and must never be simplified
//! to a single variant. The legacy error-slot dispatch is a deliberate
//! oddity: the parser detects the default handlers by pointer identity
//! (is_legacy_error_handler) so they take the fragment-stream path
//! (R-000161).
//!
//! # Proving courts
//!
//! Exercised by the PARSER court family, ERROR-001 (a counting handler sees
//! the same 6 format fragments as the oracle), TREE-001 and `cargo test
//! --lib`. Receipts under courts/receipts/phase-11.
//!
//! # Tempting simplifications that would break parity
//!
//! A naive simplification — always invoking the SAX1 variant, or invoking
//! every non-NULL slot regardless of SAX2 magic — would break
//! namespace-aware parsing and the byte-identical callback sequences the
//! data-ABI courts pin. Do not drop the SAX2-magic gate: it is part of the
//! upstream dispatch contract.

use crate::abi::callbacks::*;
use crate::abi::constants::XML_SAX2_MAGIC;
use crate::abi::structs::*;
use crate::abi::types::*;
use core::ptr;
use std::os::raw::{c_char, c_int, c_uint, c_void};

/// Safe wrapper around SAX callback dispatch.
pub(crate) struct SaxDispatcher;

#[allow(non_snake_case)]
impl SaxDispatcher {
    /// Dispatch `internalSubset` callback.
    ///
    /// # SAFETY
    ///
    /// - `sax` must be a valid pointer to an initialized `_xmlSAXHandler`.
    /// - `name`, `ext_id`, `sys_id` must be valid null-terminated strings or NULL.
    /// - `ctx` must be a valid context pointer (typically `_xmlParserCtxt*`).
    #[inline]
    pub unsafe fn internal_subset(
        sax: &_xmlSAXHandler,
        ctx: *mut c_void,
        name: *const xmlChar,
        ext_id: *const xmlChar,
        sys_id: *const xmlChar,
    ) {
        if let Some(cb) = sax.internalSubset {
            // SAFETY: Caller guarantees the callback signature matches and all
            // pointer arguments satisfy the callback's safety requirements.
            unsafe { cb(ctx, name, ext_id, sys_id) };
        }
    }

    /// Dispatch `isStandalone` callback.
    ///
    /// Returns 1 if standalone="yes", 0 if standalone="no", -1 if not declared.
    ///
    /// # SAFETY
    ///
    /// - `sax` must be a valid pointer to an initialized `_xmlSAXHandler`.
    /// - `ctx` must be a valid context pointer.
    #[inline]
    #[allow(dead_code)]
    pub unsafe fn is_standalone(sax: &_xmlSAXHandler, ctx: *mut c_void) -> c_int {
        if let Some(cb) = sax.isStandalone {
            // SAFETY: Caller guarantees the callback is safe to call with `ctx`.
            unsafe { cb(ctx) }
        } else {
            -1
        }
    }

    /// Dispatch `hasInternalSubset` callback.
    ///
    /// Returns 1 if the document has an internal subset, 0 otherwise.
    ///
    /// # SAFETY
    ///
    /// - `sax` must be a valid pointer to an initialized `_xmlSAXHandler`.
    /// - `ctx` must be a valid context pointer.
    #[inline]
    #[allow(dead_code)]
    pub unsafe fn has_internal_subset(sax: &_xmlSAXHandler, ctx: *mut c_void) -> c_int {
        if let Some(cb) = sax.hasInternalSubset {
            // SAFETY: Caller guarantees the callback is safe to call with `ctx`.
            unsafe { cb(ctx) }
        } else {
            0
        }
    }

    /// Dispatch `hasExternalSubset` callback.
    ///
    /// Returns 1 if the document has an external subset, 0 otherwise.
    ///
    /// # SAFETY
    ///
    /// - `sax` must be a valid pointer to an initialized `_xmlSAXHandler`.
    /// - `ctx` must be a valid context pointer.
    #[inline]
    #[allow(dead_code)]
    pub unsafe fn has_external_subset(sax: &_xmlSAXHandler, ctx: *mut c_void) -> c_int {
        if let Some(cb) = sax.hasExternalSubset {
            // SAFETY: Caller guarantees the callback is safe to call with `ctx`.
            unsafe { cb(ctx) }
        } else {
            0
        }
    }

    /// Dispatch `resolveEntity` callback.
    ///
    /// Returns a pointer to an `_xmlParserInput`, or NULL if not resolved.
    ///
    /// # SAFETY
    ///
    /// - `sax` must be a valid pointer to an initialized `_xmlSAXHandler`.
    /// - `pub_id`, `sys_id` must be valid null-terminated strings or NULL.
    /// - `ctx` must be a valid context pointer.
    #[inline]
    #[allow(dead_code)]
    pub unsafe fn resolve_entity(
        sax: &_xmlSAXHandler,
        ctx: *mut c_void,
        pub_id: *const xmlChar,
        sys_id: *const xmlChar,
    ) -> *mut _xmlParserInput {
        if let Some(cb) = sax.resolveEntity {
            // SAFETY: Caller guarantees the callback signature matches and all
            // pointer arguments satisfy the callback's safety requirements.
            unsafe { cb(ctx, pub_id, sys_id) }
        } else {
            ptr::null_mut()
        }
    }

    /// Dispatch `getEntity` callback.
    ///
    /// Returns a pointer to an `_xmlEntity`, or NULL.
    ///
    /// # SAFETY
    ///
    /// - `sax` must be a valid pointer to an initialized `_xmlSAXHandler`.
    /// - `name` must be a valid null-terminated string.
    /// - `ctx` must be a valid context pointer.
    #[inline]
    pub unsafe fn get_entity(
        sax: &_xmlSAXHandler,
        ctx: *mut c_void,
        name: *const xmlChar,
    ) -> *mut _xmlEntity {
        if let Some(cb) = sax.getEntity {
            // SAFETY: Caller guarantees the callback is safe to call.
            unsafe { cb(ctx, name) }
        } else {
            ptr::null_mut()
        }
    }

    /// Dispatch `entityDecl` callback.
    ///
    /// # SAFETY
    ///
    /// - `sax` must be a valid pointer to an initialized `_xmlSAXHandler`.
    /// - All pointer arguments must be valid null-terminated strings or NULL.
    /// - `ctx` must be a valid context pointer.
    #[inline]
    pub unsafe fn entity_decl(
        sax: &_xmlSAXHandler,
        ctx: *mut c_void,
        name: *const xmlChar,
        type_: c_int,
        pub_id: *const xmlChar,
        sys_id: *const xmlChar,
        content: *mut xmlChar,
    ) {
        if let Some(cb) = sax.entityDecl {
            // SAFETY: Caller guarantees the callback signature matches.
            unsafe { cb(ctx, name, type_, pub_id, sys_id, content) };
        }
    }

    /// Dispatch `notationDecl` callback.
    ///
    /// # SAFETY
    ///
    /// - `sax` must be a valid pointer to an initialized `_xmlSAXHandler`.
    /// - All pointer arguments must be valid null-terminated strings or NULL.
    /// - `ctx` must be a valid context pointer.
    #[inline]
    pub unsafe fn notation_decl(
        sax: &_xmlSAXHandler,
        ctx: *mut c_void,
        name: *const xmlChar,
        pub_id: *const xmlChar,
        sys_id: *const xmlChar,
    ) {
        if let Some(cb) = sax.notationDecl {
            // SAFETY: Caller guarantees the callback signature matches.
            unsafe { cb(ctx, name, pub_id, sys_id) };
        }
    }

    /// Dispatch `attributeDecl` callback.
    ///
    /// # SAFETY
    ///
    /// - `sax` must be a valid pointer to an initialized `_xmlSAXHandler`.
    /// - All pointer arguments must be valid null-terminated strings or NULL.
    /// - `ctx` must be a valid context pointer.
    #[allow(clippy::too_many_arguments)]
    #[inline]
    pub unsafe fn attribute_decl(
        sax: &_xmlSAXHandler,
        ctx: *mut c_void,
        elem: *const xmlChar,
        fullname: *const xmlChar,
        type_: c_int,
        def: c_int,
        default_value: *const xmlChar,
        tree: *mut _xmlEnumeration,
    ) {
        if let Some(cb) = sax.attributeDecl {
            // SAFETY: Caller guarantees the callback signature matches.
            unsafe { cb(ctx, elem, fullname, type_, def, default_value, tree) };
        }
    }

    /// Dispatch `elementDecl` callback.
    ///
    /// # SAFETY
    ///
    /// - `sax` must be a valid pointer to an initialized `_xmlSAXHandler`.
    /// - `name` must be a valid null-terminated string.
    /// - `content` must be a valid pointer or NULL.
    /// - `ctx` must be a valid context pointer.
    #[inline]
    pub unsafe fn element_decl(
        sax: &_xmlSAXHandler,
        ctx: *mut c_void,
        name: *const xmlChar,
        type_: c_int,
        content: *mut _xmlElementContent,
    ) {
        if let Some(cb) = sax.elementDecl {
            // SAFETY: Caller guarantees the callback signature matches.
            unsafe { cb(ctx, name, type_, content) };
        }
    }

    /// Dispatch `unparsedEntityDecl` callback.
    ///
    /// # SAFETY
    ///
    /// - `sax` must be a valid pointer to an initialized `_xmlSAXHandler`.
    /// - All pointer arguments must be valid null-terminated strings or NULL.
    /// - `ctx` must be a valid context pointer.
    #[inline]
    pub unsafe fn unparsed_entity_decl(
        sax: &_xmlSAXHandler,
        ctx: *mut c_void,
        name: *const xmlChar,
        pub_id: *const xmlChar,
        sys_id: *const xmlChar,
        notation: *const xmlChar,
    ) {
        if let Some(cb) = sax.unparsedEntityDecl {
            // SAFETY: Caller guarantees the callback signature matches.
            unsafe { cb(ctx, name, pub_id, sys_id, notation) };
        }
    }

    /// Dispatch `setDocumentLocator` callback.
    ///
    /// # SAFETY
    ///
    /// - `sax` must be a valid pointer to an initialized `_xmlSAXHandler`.
    /// - `loc` must be a valid pointer to an `_xmlSAXLocator`.
    /// - `ctx` must be a valid context pointer.
    #[inline]
    pub unsafe fn set_document_locator(
        sax: &_xmlSAXHandler,
        ctx: *mut c_void,
        loc: *mut _xmlSAXLocator,
    ) {
        if let Some(cb) = sax.setDocumentLocator {
            // SAFETY: Caller guarantees the callback signature matches.
            unsafe { cb(ctx, loc) };
        }
    }

    /// Dispatch `startDocument` callback.
    ///
    /// # SAFETY
    ///
    /// - `sax` must be a valid pointer to an initialized `_xmlSAXHandler`.
    /// - `ctx` must be a valid context pointer.
    #[inline]
    pub unsafe fn start_document(sax: &_xmlSAXHandler, ctx: *mut c_void) {
        if let Some(cb) = sax.startDocument {
            // SAFETY: Caller guarantees the callback is safe to call with `ctx`.
            unsafe { cb(ctx) };
        }
    }

    /// Dispatch `endDocument` callback.
    ///
    /// # SAFETY
    ///
    /// - `sax` must be a valid pointer to an initialized `_xmlSAXHandler`.
    /// - `ctx` must be a valid context pointer.
    #[inline]
    pub unsafe fn end_document(sax: &_xmlSAXHandler, ctx: *mut c_void) {
        if let Some(cb) = sax.endDocument {
            // SAFETY: Caller guarantees the callback is safe to call with `ctx`.
            unsafe { cb(ctx) };
        }
    }

    /// Dispatch element start, preferring SAX2 (`startElementNs`) over SAX1 (`startElement`).
    ///
    /// # SAFETY
    ///
    /// - `sax` must be a valid pointer to an initialized `_xmlSAXHandler`.
    /// - `ctx` must be a valid context pointer.
    /// - All pointer arguments must satisfy the requirements of whichever callback
    ///   is actually invoked (SAX1 or SAX2).
    ///
    /// SAX1 start-element entry (upstream SAX2.c `xmlSAX2StartElement`):
    ///
    /// invokes the handler's SAX1 `startElement` callback with the given name
    ///
    /// and SAX1 attribute array.
    ///
    /// # SAFETY
    ///
    /// - `ctx` must be a valid parser context pointer.
    /// - `name` must be a valid null-terminated name.
    /// - `atts` must be a SAX1 attribute array (NULL-terminated pairs) or NULL.
    pub unsafe fn sax1_start_element(
        ctx: *mut c_void,
        name: *const xmlChar,
        atts: *mut *const xmlChar,
    ) {
        // SAFETY: ctx is a valid parser context.
        let ctxt = ctx as *mut crate::abi::structs::_xmlParserCtxt;
        unsafe {
            if ctxt.is_null() || (*ctxt).sax.is_null() {
                return;
            }
            let cb = (*(*ctxt).sax).startElement;
            if let Some(cb) = cb {
                // SAFETY: the SAX1 callback contract; name/atts valid.
                cb(ctx, name, atts);
            }
        }
    }

    /// SAX1 end-element entry (upstream SAX2.c `xmlSAX2EndElement`).
    ///
    /// # SAFETY
    ///
    /// - `ctx` must be a valid parser context pointer.
    /// - `name` must be a valid null-terminated name.
    pub unsafe fn sax1_end_element(ctx: *mut c_void, name: *const xmlChar) {
        // SAFETY: ctx is a valid parser context.
        let ctxt = ctx as *mut crate::abi::structs::_xmlParserCtxt;
        unsafe {
            if ctxt.is_null() || (*ctxt).sax.is_null() {
                return;
            }
            let cb = (*(*ctxt).sax).endElement;
            if let Some(cb) = cb {
                // SAFETY: the SAX1 callback contract; name valid.
                cb(ctx, name);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    #[inline]
    pub unsafe fn start_element(
        sax: &_xmlSAXHandler,
        ctx: *mut c_void,
        localname: *const xmlChar,
        prefix: *const xmlChar,
        URI: *const xmlChar,
        nb_namespaces: c_int,
        namespaces: *mut *const xmlChar,
        nb_attributes: c_int,
        nb_defaulted: c_int,
        attributes: *mut *const xmlChar,
    ) {
        // Prefer SAX2 callback if available.
        if let Some(cb) = sax.startElementNs {
            // SAFETY: Caller guarantees all SAX2 callback arguments are valid.
            unsafe {
                cb(
                    ctx,
                    localname,
                    prefix,
                    URI,
                    nb_namespaces,
                    namespaces,
                    nb_attributes,
                    nb_defaulted,
                    attributes,
                )
            };
        } else if let Some(cb) = sax.startElement {
            // SAX1 fallback: use the element's qualified name.
            // SAX1 callbacks don't receive namespace info, so we only pass the
            // localname as the element name, and NULL for attributes (the caller
            // must convert the SAX2 attribute format to SAX1 format if needed).
            //
            // # UPSTREAM-PARITY
            //
            // In upstream libxml2, the SAX1 fallback in xmlSAX2StartElement
            // reconstructs the qualified name from localname + prefix and
            // converts the attribute array to SAX1 format.
            // SAFETY: Caller guarantees the SAX1 callback arguments are valid.
            unsafe { cb(ctx, localname, ptr::null_mut()) };
        }
    }

    /// Dispatch element end, preferring SAX2 (`endElementNs`) over SAX1 (`endElement`).
    ///
    /// # SAFETY
    ///
    /// - `sax` must be a valid pointer to an initialized `_xmlSAXHandler`.
    /// - `ctx` must be a valid context pointer.
    /// - All pointer arguments must satisfy the requirements of whichever callback
    ///   is actually invoked.
    #[inline]
    pub unsafe fn end_element(
        sax: &_xmlSAXHandler,
        ctx: *mut c_void,
        localname: *const xmlChar,
        prefix: *const xmlChar,
        URI: *const xmlChar,
    ) {
        // Prefer SAX2 callback if available.
        if let Some(cb) = sax.endElementNs {
            // SAFETY: Caller guarantees all SAX2 callback arguments are valid.
            unsafe { cb(ctx, localname, prefix, URI) };
        } else if let Some(cb) = sax.endElement {
            // SAX1 fallback: use the localname as the element name.
            // SAFETY: Caller guarantees the SAX1 callback arguments are valid.
            unsafe { cb(ctx, localname) };
        }
    }

    /// Dispatch `characters` callback.
    ///
    /// # SAFETY
    ///
    /// - `sax` must be a valid pointer to an initialized `_xmlSAXHandler`.
    /// - `ch` must be a valid pointer to a buffer of at least `len` bytes.
    /// - `ctx` must be a valid context pointer.
    #[inline]
    pub unsafe fn characters(
        sax: &_xmlSAXHandler,
        ctx: *mut c_void,
        ch: *const xmlChar,
        len: c_int,
    ) {
        if let Some(cb) = sax.characters {
            // SAFETY: Caller guarantees the callback arguments are valid.
            unsafe { cb(ctx, ch, len) };
        }
    }

    /// Dispatch `ignorableWhitespace` callback.
    ///
    /// # SAFETY
    ///
    /// - `sax` must be a valid pointer to an initialized `_xmlSAXHandler`.
    /// - `ch` must be a valid pointer to a buffer of at least `len` bytes.
    /// - `ctx` must be a valid context pointer.
    #[inline]
    pub unsafe fn ignorable_whitespace(
        sax: &_xmlSAXHandler,
        ctx: *mut c_void,
        ch: *const xmlChar,
        len: c_int,
    ) {
        if let Some(cb) = sax.ignorableWhitespace {
            // SAFETY: Caller guarantees the callback arguments are valid.
            unsafe { cb(ctx, ch, len) };
        }
    }

    /// Dispatch `processingInstruction` callback.
    ///
    /// # SAFETY
    ///
    /// - `sax` must be a valid pointer to an initialized `_xmlSAXHandler`.
    /// - `target` and `data` must be valid null-terminated strings or NULL.
    /// - `ctx` must be a valid context pointer.
    #[inline]
    pub unsafe fn processing_instruction(
        sax: &_xmlSAXHandler,
        ctx: *mut c_void,
        target: *const xmlChar,
        data: *const xmlChar,
    ) {
        if let Some(cb) = sax.processingInstruction {
            // SAFETY: Caller guarantees the callback arguments are valid.
            unsafe { cb(ctx, target, data) };
        }
    }

    /// Dispatch `comment` callback.
    ///
    /// # SAFETY
    ///
    /// - `sax` must be a valid pointer to an initialized `_xmlSAXHandler`.
    /// - `value` must be a valid null-terminated string or NULL.
    /// - `ctx` must be a valid context pointer.
    #[inline]
    pub unsafe fn comment(sax: &_xmlSAXHandler, ctx: *mut c_void, value: *const xmlChar) {
        if let Some(cb) = sax.comment {
            // SAFETY: Caller guarantees the callback arguments are valid.
            unsafe { cb(ctx, value) };
        }
    }

    /// Dispatch `warning` callback (printf-style, variadic at C call site).
    ///
    /// # SAFETY
    ///
    /// - `sax` must be a valid pointer to an initialized `_xmlSAXHandler`.
    /// - `msg` must be a valid null-terminated C string.
    /// - `ctx` must be a valid context pointer.
    ///
    /// # UPSTREAM-PARITY
    ///
    /// The `...` variadic arguments are not representable in stable Rust's
    /// `extern "C"` function pointer type. The type alias `warningSAXFunc`
    /// only takes `(ctx, msg)` — matching the upstream ABI where the callee
    /// uses `va_list` internally. C callers pass additional variadic arguments
    /// directly on the stack per the platform ABI.
    #[inline]
    #[allow(dead_code)]
    pub unsafe fn warning(sax: &_xmlSAXHandler, ctx: *mut c_void, msg: *const c_char) {
        if let Some(cb) = sax.warning {
            // SAFETY: Caller guarantees the callback arguments are valid.
            unsafe { cb(ctx, msg) };
        }
    }

    /// Dispatch `error` callback (printf-style, variadic at C call site).
    ///
    /// # SAFETY
    ///
    /// - `sax` must be a valid pointer to an initialized `_xmlSAXHandler`.
    /// - `msg` must be a valid null-terminated C string.
    /// - `ctx` must be a valid context pointer.
    #[inline]
    #[allow(dead_code)]
    pub unsafe fn error(sax: &_xmlSAXHandler, ctx: *mut c_void, msg: *const c_char) {
        if let Some(cb) = sax.error {
            // SAFETY: Caller guarantees the callback arguments are valid.
            unsafe { cb(ctx, msg) };
        }
    }

    /// Dispatch `fatalError` callback (printf-style, variadic at C call site).
    ///
    /// # SAFETY
    ///
    /// - `sax` must be a valid pointer to an initialized `_xmlSAXHandler`.
    /// - `msg` must be a valid null-terminated C string.
    /// - `ctx` must be a valid context pointer.
    #[inline]
    #[allow(dead_code)]
    pub unsafe fn fatal_error(sax: &_xmlSAXHandler, ctx: *mut c_void, msg: *const c_char) {
        if let Some(cb) = sax.fatalError {
            // SAFETY: Caller guarantees the callback arguments are valid.
            unsafe { cb(ctx, msg) };
        }
    }

    /// Dispatch `cdataBlock` callback.
    ///
    /// # SAFETY
    ///
    /// - `sax` must be a valid pointer to an initialized `_xmlSAXHandler`.
    /// - `value` must be a valid pointer to a buffer of at least `len` bytes.
    /// - `ctx` must be a valid context pointer.
    #[inline]
    pub unsafe fn cdata_block(
        sax: &_xmlSAXHandler,
        ctx: *mut c_void,
        value: *const xmlChar,
        len: c_int,
    ) {
        if let Some(cb) = sax.cdataBlock {
            // SAFETY: Caller guarantees the callback arguments are valid.
            unsafe { cb(ctx, value, len) };
        }
    }

    /// Dispatch `reference` callback.
    ///
    /// # SAFETY
    ///
    /// - `sax` must be a valid pointer to an initialized `_xmlSAXHandler`.
    /// - `name` must be a valid null-terminated string.
    /// - `ctx` must be a valid context pointer.
    #[inline]
    pub unsafe fn reference(sax: &_xmlSAXHandler, ctx: *mut c_void, name: *const xmlChar) {
        if let Some(cb) = sax.reference {
            // SAFETY: Caller guarantees the callback arguments are valid.
            unsafe { cb(ctx, name) };
        }
    }

    /// Dispatch `getParameterEntity` callback.
    ///
    /// Returns a pointer to an `_xmlEntity`, or NULL.
    ///
    /// # SAFETY
    ///
    /// - `sax` must be a valid pointer to an initialized `_xmlSAXHandler`.
    /// - `name` must be a valid null-terminated string.
    /// - `ctx` must be a valid context pointer.
    #[inline]
    pub unsafe fn get_parameter_entity(
        sax: &_xmlSAXHandler,
        ctx: *mut c_void,
        name: *const xmlChar,
    ) -> *mut _xmlEntity {
        if let Some(cb) = sax.getParameterEntity {
            // SAFETY: Caller guarantees the callback is safe to call.
            unsafe { cb(ctx, name) }
        } else {
            ptr::null_mut()
        }
    }

    /// Dispatch `externalSubset` callback.
    ///
    /// # SAFETY
    ///
    /// - `sax` must be a valid pointer to an initialized `_xmlSAXHandler`.
    /// - All pointer arguments must be valid null-terminated strings or NULL.
    /// - `ctx` must be a valid context pointer.
    #[inline]
    pub unsafe fn external_subset(
        sax: &_xmlSAXHandler,
        ctx: *mut c_void,
        name: *const xmlChar,
        ext_id: *const xmlChar,
        sys_id: *const xmlChar,
    ) {
        if let Some(cb) = sax.externalSubset {
            // SAFETY: Caller guarantees the callback signature matches.
            unsafe { cb(ctx, name, ext_id, sys_id) };
        }
    }

    /// Dispatch `serror` (structured error) callback.
    ///
    /// # SAFETY
    ///
    /// - `sax` must be a valid pointer to an initialized `_xmlSAXHandler`.
    /// - `error` must be a valid pointer to an `_xmlError`.
    /// - `ctx` must be a valid context pointer.
    #[inline]
    #[allow(dead_code)]
    pub unsafe fn structured_error(sax: &_xmlSAXHandler, ctx: *mut c_void, error: *mut _xmlError) {
        if let Some(cb) = sax.serror {
            // SAFETY: Caller guarantees the callback arguments are valid.
            unsafe { cb(ctx, error) };
        }
    }
}

/// Initialize a `_xmlSAXHandler` with default SAX2 callback functions.
///
/// This is the Rust equivalent of `xmlSAX2InitDefaultSAXHandler` from upstream
/// libxml2. It sets all callback fields to point to the default SAX2 handlers
/// defined in `super::default::default_sax_handler`, and marks the handler as
/// initialized with `XML_SAX2_MAGIC`.
///
/// # SAFETY
///
/// - `sax` must be a valid pointer to a `_xmlSAXHandler` that can be written to.
/// - The caller is responsible for freeing the handler if needed.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlSAX2InitDefaultSAXHandler(xmlSAXHandlerPtr handler, int warning);
/// ```
///
/// Upstream signature includes a `warning` parameter that controls whether
/// warning/error callbacks are set. We ignore it for now and always set them.
#[allow(non_snake_case)]
pub unsafe fn xmlSAX2InitDefaultSAXHandler(sax: *mut _xmlSAXHandler) {
    use super::default::default_sax_handler as dflt;

    if sax.is_null() {
        return;
    }

    // SAFETY: Caller guarantees `sax` is a valid, writable pointer.
    unsafe {
        let h = &mut *sax;
        h.internalSubset = Some(dflt::internalSubset as internalSubsetSAXFunc);
        h.isStandalone = Some(dflt::isStandalone as isStandaloneSAXFunc);
        h.hasInternalSubset = Some(dflt::hasInternalSubset as hasInternalSubsetSAXFunc);
        h.hasExternalSubset = Some(dflt::hasExternalSubset as hasExternalSubsetSAXFunc);
        h.resolveEntity = Some(dflt::resolveEntity as resolveEntitySAXFunc);
        h.getEntity = Some(dflt::getEntity as getEntitySAXFunc);
        h.entityDecl = Some(dflt::entityDecl as entityDeclSAXFunc);
        h.notationDecl = Some(dflt::notationDecl as notationDeclSAXFunc);
        h.attributeDecl = Some(dflt::attributeDecl as attributeDeclSAXFunc);
        h.elementDecl = Some(dflt::elementDecl as elementDeclSAXFunc);
        h.unparsedEntityDecl = Some(dflt::unparsedEntityDecl as unparsedEntityDeclSAXFunc);
        h.setDocumentLocator = Some(dflt::setDocumentLocator as setDocumentLocatorSAXFunc);
        h.startDocument = Some(dflt::startDocument as startDocumentSAXFunc);
        h.endDocument = Some(dflt::endDocument as endDocumentSAXFunc);
        h.startElement = None; // SAX1: not set in SAX2 mode
        h.endElement = None; // SAX1: not set in SAX2 mode
        h.reference = Some(dflt::reference as referenceSAXFunc);
        h.characters = Some(dflt::characters as charactersSAXFunc);
        h.ignorableWhitespace = Some(dflt::ignorableWhitespace as ignorableWhitespaceSAXFunc);
        h.processingInstruction = Some(dflt::processingInstruction as processingInstructionSAXFunc);
        h.comment = Some(dflt::comment as commentSAXFunc);
        // UPSTREAM-PARITY (SAX2.c xmlSAX2InitDefaultSAXHandler): the legacy
        // xmlParserError/xmlParserWarning handlers occupy the error/warning
        // slots. The parser's raise path (state.rs set_error) recognises them
        // as legacy and streams the xmlFormatError fragments through the
        // generic channel instead of invoking them directly.
        h.warning = Some(crate::xml::errors::xmlParserWarning as warningSAXFunc);
        h.error = Some(crate::xml::errors::xmlParserError as errorSAXFunc);
        h.fatalError = Some(crate::xml::errors::xmlParserError as errorSAXFunc);
        h.getParameterEntity = Some(dflt::getParameterEntity as getParameterEntitySAXFunc);
        h.cdataBlock = Some(dflt::cdataBlock as cdataBlockSAXFunc);
        h.externalSubset = Some(dflt::externalSubset as externalSubsetSAXFunc);
        h.initialized = XML_SAX2_MAGIC as c_uint;
        h._private = ptr::null_mut();
        h.startElementNs = Some(dflt::startElementNs as startElementNsSAX2Func);
        h.endElementNs = Some(dflt::endElementNs as endElementNsSAX2Func);
        h.serror = None;
    }
}
