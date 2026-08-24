//! C ABI callback function type definitions — matching upstream callback signatures (§14, §20).
//!
//! This module defines all function pointer types used in public upstream structures:
//! SAX1 callbacks, SAX2 callbacks, error callbacks, validity callbacks, XPath callbacks,
//! I/O callbacks, resource loader callbacks, encoding callbacks, and the SAX locator.
//!
//! # Phase 1 status
//!
//! Complete — all callback types from upstream headers are defined.
//!
//! # Safety
//!
//! All callback types are `unsafe extern "C"` because they are called across the FFI boundary
//! with C calling conventions. The caller must ensure:
//! - Function pointers are non-null before invocation (unless nullable per upstream contract)
//! - Pointers passed to callbacks remain valid for the callback's duration
//! - Callbacks observe the upstream ownership/lifetime contract
//! - Thread safety matches upstream expectations

#![allow(non_camel_case_types)]

use core::ffi::c_void;
use std::os::raw::{c_char, c_int, c_uchar};

use crate::abi::structs::*;

// ═══════════════════════════════════════════════════════════════════════════════
// SAX1 Callbacks (xmlSAXHandler)
// ═══════════════════════════════════════════════════════════════════════════════

/// Callback for internal DTD subset notification.
///
/// # UPSTREAM-PARITY
///
/// Oracle behavior: Called when `<!DOCTYPE ... [ ... ]>` internal subset is parsed.
/// Parameters are the DOCTYPE name, external ID (or NULL), system ID (or NULL).
pub type internalSubsetSAXFunc = unsafe extern "C" fn(
    ctx: *mut c_void,
    name: *const crate::abi::types::xmlChar,
    ExternalID: *const crate::abi::types::xmlChar,
    SystemID: *const crate::abi::types::xmlChar,
);

/// Callback for standalone document declaration.
///
/// Returns 1 if standalone="yes", 0 if standalone="no", -1 if not declared.
pub type isStandaloneSAXFunc = unsafe extern "C" fn(ctx: *mut c_void) -> c_int;

/// Callback: does the document have an internal subset?
pub type hasInternalSubsetSAXFunc = unsafe extern "C" fn(ctx: *mut c_void) -> c_int;

/// Callback: does the document have an external subset?
pub type hasExternalSubsetSAXFunc = unsafe extern "C" fn(ctx: *mut c_void) -> c_int;

/// Callback to resolve an external entity.
///
/// Returns a newly allocated `xmlParserInputPtr` or NULL.
///
/// # UPSTREAM-PARITY
///
/// Ownership: The returned `xmlParserInputPtr` is owned by the parser context.
pub type resolveEntitySAXFunc = unsafe extern "C" fn(
    ctx: *mut c_void,
    publicId: *const crate::abi::types::xmlChar,
    systemId: *const crate::abi::types::xmlChar,
) -> *mut _xmlParserInput;

/// Callback to get an entity.
///
/// Returns a pointer to an entity or NULL.
///
/// # UPSTREAM-PARITY
///
/// Ownership: The returned entity is owned by the document's entity table.
/// The caller must not free it.
pub type getEntitySAXFunc = unsafe extern "C" fn(
    ctx: *mut c_void,
    name: *const crate::abi::types::xmlChar,
) -> *mut _xmlEntity;

/// Callback for entity declaration.
pub type entityDeclSAXFunc = unsafe extern "C" fn(
    ctx: *mut c_void,
    name: *const crate::abi::types::xmlChar,
    type_: c_int,
    publicId: *const crate::abi::types::xmlChar,
    systemId: *const crate::abi::types::xmlChar,
    content: *mut crate::abi::types::xmlChar,
);

/// Callback for notation declaration.
pub type notationDeclSAXFunc = unsafe extern "C" fn(
    ctx: *mut c_void,
    name: *const crate::abi::types::xmlChar,
    publicId: *const crate::abi::types::xmlChar,
    systemId: *const crate::abi::types::xmlChar,
);

/// Callback for attribute declaration.
pub type attributeDeclSAXFunc = unsafe extern "C" fn(
    ctx: *mut c_void,
    elem: *const crate::abi::types::xmlChar,
    name: *const crate::abi::types::xmlChar,
    type_: c_int,
    def: c_int,
    defaultValue: *const crate::abi::types::xmlChar,
    tree: *mut _xmlEnumeration,
);

/// Callback for element declaration.
pub type elementDeclSAXFunc = unsafe extern "C" fn(
    ctx: *mut c_void,
    name: *const crate::abi::types::xmlChar,
    type_: c_int,
    content: *mut _xmlElementContent,
);

/// Callback for unparsed entity declaration.
pub type unparsedEntityDeclSAXFunc = unsafe extern "C" fn(
    ctx: *mut c_void,
    name: *const crate::abi::types::xmlChar,
    publicId: *const crate::abi::types::xmlChar,
    systemId: *const crate::abi::types::xmlChar,
    notationName: *const crate::abi::types::xmlChar,
);

/// Callback to set the document locator.
///
/// # UPSTREAM-PARITY
///
/// The locator is an opaque structure that provides line/column information.
pub type setDocumentLocatorSAXFunc =
    unsafe extern "C" fn(ctx: *mut c_void, loc: *mut _xmlSAXLocator);

/// Callback for document start.
pub type startDocumentSAXFunc = unsafe extern "C" fn(ctx: *mut c_void);

/// Callback for document end.
pub type endDocumentSAXFunc = unsafe extern "C" fn(ctx: *mut c_void);

/// Callback for element start (SAX1).
///
/// # Parameters
/// - `name`: element name
/// - `atts`: NULL-terminated array of [name, value, name, value, ..., NULL]
pub type startElementSAXFunc = unsafe extern "C" fn(
    ctx: *mut c_void,
    name: *const crate::abi::types::xmlChar,
    atts: *mut *const crate::abi::types::xmlChar,
);

/// Callback for element end (SAX1).
pub type endElementSAXFunc =
    unsafe extern "C" fn(ctx: *mut c_void, name: *const crate::abi::types::xmlChar);

/// Callback for entity reference.
pub type referenceSAXFunc =
    unsafe extern "C" fn(ctx: *mut c_void, name: *const crate::abi::types::xmlChar);

/// Callback for character data.
pub type charactersSAXFunc =
    unsafe extern "C" fn(ctx: *mut c_void, ch: *const crate::abi::types::xmlChar, len: c_int);

/// Callback for ignorable whitespace.
pub type ignorableWhitespaceSAXFunc =
    unsafe extern "C" fn(ctx: *mut c_void, ch: *const crate::abi::types::xmlChar, len: c_int);

/// Callback for processing instructions.
pub type processingInstructionSAXFunc = unsafe extern "C" fn(
    ctx: *mut c_void,
    target: *const crate::abi::types::xmlChar,
    data: *const crate::abi::types::xmlChar,
);

/// Callback for comments.
pub type commentSAXFunc =
    unsafe extern "C" fn(ctx: *mut c_void, value: *const crate::abi::types::xmlChar);

/// Callback for warnings (printf-style, variadic at C call site).
///
/// # UPSTREAM-PARITY
///
/// The `...` is implicit in C; Rust type cannot express variadic extern "C"
/// on stable. The function pointer ABI is identical — C callers pass variadic
/// arguments and the callee uses va_list internally.
pub type warningSAXFunc = unsafe extern "C" fn(ctx: *mut c_void, msg: *const c_char);

/// Callback for errors (printf-style, variadic at C call site).
pub type errorSAXFunc = unsafe extern "C" fn(ctx: *mut c_void, msg: *const c_char);

/// Callback for fatal errors (printf-style, variadic at C call site).
pub type fatalErrorSAXFunc = unsafe extern "C" fn(ctx: *mut c_void, msg: *const c_char);

/// Callback to get a parameter entity.
///
/// Returns a pointer to a parameter entity or NULL.
pub type getParameterEntitySAXFunc = unsafe extern "C" fn(
    ctx: *mut c_void,
    name: *const crate::abi::types::xmlChar,
) -> *mut _xmlEntity;

/// Callback for CDATA block.
pub type cdataBlockSAXFunc =
    unsafe extern "C" fn(ctx: *mut c_void, value: *const crate::abi::types::xmlChar, len: c_int);

/// Callback for external subset notification.
pub type externalSubsetSAXFunc = unsafe extern "C" fn(
    ctx: *mut c_void,
    name: *const crate::abi::types::xmlChar,
    ExternalID: *const crate::abi::types::xmlChar,
    SystemID: *const crate::abi::types::xmlChar,
);

/// Callback for initializing the SAX handler.
///
/// # UPSTREAM-PARITY
///
/// This is a libxml2-internal callback used to set SAX2 callbacks when SAX1 callbacks
/// are not provided. Not typically set by downstream users.
pub type initSAXFunc = unsafe extern "C" fn(ctx: *mut c_void, handler: *mut _xmlSAXHandler);

// ═══════════════════════════════════════════════════════════════════════════════
// SAX2 Callbacks
// ═══════════════════════════════════════════════════════════════════════════════

/// Callback for element start (SAX2/namespaced).
///
/// # Parameters
/// - `localname`: element local name
/// - `prefix`: element namespace prefix (or NULL)
/// - `URI`: element namespace URI (or NULL)
/// - `nb_namespaces`: number of namespace declarations
/// - `namespaces`: array of [prefix, URI, prefix, URI, ...] (size 2*nb_namespaces)
/// - `nb_attributes`: total number of attributes
/// - `nb_defaulted`: number of defaulted attributes (from DTD)
/// - `attributes`: array of [localname, prefix, URI, value, value_end, ...]
///   each attribute is 5 entries, value_end is pointer past last char of value
pub type startElementNsSAX2Func = unsafe extern "C" fn(
    ctx: *mut c_void,
    localname: *const crate::abi::types::xmlChar,
    prefix: *const crate::abi::types::xmlChar,
    URI: *const crate::abi::types::xmlChar,
    nb_namespaces: c_int,
    namespaces: *mut *const crate::abi::types::xmlChar,
    nb_attributes: c_int,
    nb_defaulted: c_int,
    attributes: *mut *const crate::abi::types::xmlChar,
);

/// Callback for element end (SAX2/namespaced).
pub type endElementNsSAX2Func = unsafe extern "C" fn(
    ctx: *mut c_void,
    localname: *const crate::abi::types::xmlChar,
    prefix: *const crate::abi::types::xmlChar,
    URI: *const crate::abi::types::xmlChar,
);

// ═══════════════════════════════════════════════════════════════════════════════
// SAX Locator
// ═══════════════════════════════════════════════════════════════════════════════

/// The SAX locator structure providing line/column information.
///
/// # UPSTREAM-PARITY
///
/// This is an opaque structure from the perspective of SAX handlers.
/// Upstream defines it as:
/// ```c
/// typedef struct _xmlSAXLocator xmlSAXLocator;
/// typedef xmlSAXLocator *xmlSAXLocatorPtr;
/// struct _xmlSAXLocator {
///     xmlChar *(*getPublicId)(void *ctx);
///     xmlChar *(*getSystemId)(void *ctx);
///     int      (*getLineNumber)(void *ctx);
///     int      (*getColumnNumber)(void *ctx);
/// };
/// ```
#[repr(C)]
pub struct _xmlSAXLocator {
    /// Get the public ID of the current document position.
    pub getPublicId:
        Option<unsafe extern "C" fn(ctx: *mut c_void) -> *const crate::abi::types::xmlChar>,
    /// Get the system ID of the current document position.
    pub getSystemId:
        Option<unsafe extern "C" fn(ctx: *mut c_void) -> *const crate::abi::types::xmlChar>,
    /// Get the line number of the current document position.
    pub getLineNumber: Option<unsafe extern "C" fn(ctx: *mut c_void) -> c_int>,
    /// Get the column number of the current document position.
    pub getColumnNumber: Option<unsafe extern "C" fn(ctx: *mut c_void) -> c_int>,
}

/// Pointer to a SAX locator.
pub type xmlSAXLocatorPtr = *mut _xmlSAXLocator;

// ═══════════════════════════════════════════════════════════════════════════════
// Error Callbacks
// ═══════════════════════════════════════════════════════════════════════════════

/// Structured error handler callback.
///
/// Called with a pointer to the error structure. The error structure is valid
/// only during the callback invocation.
pub type xmlStructuredErrorFunc = unsafe extern "C" fn(ctx: *mut c_void, error: *const _xmlError);

/// Generic error handler callback (printf-style, variadic at C call site).
///
/// # UPSTREAM-PARITY
///
/// This is the older error reporting mechanism. New code should use the structured
/// error handler (`xmlStructuredErrorFunc`) instead.
pub type xmlGenericErrorFunc = unsafe extern "C" fn(ctx: *mut c_void, msg: *const c_char);

// ═══════════════════════════════════════════════════════════════════════════════
// Validity Callbacks
// ═══════════════════════════════════════════════════════════════════════════════

/// Validity error handler callback (printf-style, variadic at C call site).
pub type xmlValidityErrorFunc = unsafe extern "C" fn(ctx: *mut c_void, msg: *const c_char);

/// Validity warning handler callback (printf-style, variadic at C call site).
pub type xmlValidityWarningFunc = unsafe extern "C" fn(ctx: *mut c_void, msg: *const c_char);

// ═══════════════════════════════════════════════════════════════════════════════
// I/O Callbacks
// ═══════════════════════════════════════════════════════════════════════════════

/// Read callback for custom input.
///
/// Should fill `buffer` with up to `len` bytes.
/// Returns the number of bytes read, 0 on EOF, or -1 on error.
pub type xmlInputReadCallback =
    unsafe extern "C" fn(context: *mut c_void, buffer: *mut c_char, len: c_int) -> c_int;

/// Close callback for custom input.
///
/// Returns 0 on success, -1 on error.
pub type xmlInputCloseCallback = unsafe extern "C" fn(context: *mut c_void) -> c_int;

/// Write callback for custom output.
///
/// Should write up to `len` bytes from `buffer`.
/// Returns the number of bytes written, or -1 on error.
pub type xmlOutputWriteCallback =
    unsafe extern "C" fn(context: *mut c_void, buffer: *const c_char, len: c_int) -> c_int;

/// Close callback for custom output.
///
/// Returns 0 on success, -1 on error.
pub type xmlOutputCloseCallback = unsafe extern "C" fn(context: *mut c_void) -> c_int;

// ═══════════════════════════════════════════════════════════════════════════════
// XPath Callbacks
// ═══════════════════════════════════════════════════════════════════════════════

/// Variable lookup function for XPath.
///
/// Returns an `xmlXPathObjectPtr` representing the variable's value, or NULL.
///
/// # UPSTREAM-PARITY
///
/// Ownership: The returned object is owned by the caller (must be freed).
pub type xmlXPathVariableLookupFunc = unsafe extern "C" fn(
    ctxt: *mut c_void,
    name: *const crate::abi::types::xmlChar,
    ns_uri: *const crate::abi::types::xmlChar,
) -> *mut _xmlXPathObject;

/// Function lookup function for XPath extensions.
///
/// Returns a function pointer or NULL.
pub type xmlXPathFuncLookupFunc = unsafe extern "C" fn(
    ctxt: *mut c_void,
    name: *const crate::abi::types::xmlChar,
    ns_uri: *const crate::abi::types::xmlChar,
) -> *mut c_void;

// ═══════════════════════════════════════════════════════════════════════════════
// Resource Loader Callbacks
// ═══════════════════════════════════════════════════════════════════════════════

/// Resource loader callback.
///
/// Loads a resource identified by `url` and returns a parser input.
///
/// # UPSTREAM-PARITY
///
/// - `type_`: The type of resource being loaded
///   (1 = parser entity, 2 = stylesheet include, 3 = stylesheet import, 4 = document)
pub type xmlResourceLoader = unsafe extern "C" fn(
    ctxt: *mut c_void,
    url: *const c_char,
    options: c_int,
    type_: c_int,
) -> *mut _xmlParserInput;

// ═══════════════════════════════════════════════════════════════════════════════
// Encoding Callbacks
// ═══════════════════════════════════════════════════════════════════════════════

/// Character encoding input conversion function.
///
/// Converts from the handler's input encoding to UTF-8.
/// `in` and `inlen` describe input; `out` and `outlen` describe output buffer.
/// Returns the number of bytes written, or -1 on error.
pub type xmlCharEncodingInputFunc = unsafe extern "C" fn(
    out: *mut c_uchar,
    outlen: *mut c_int,
    in_: *const c_uchar,
    inlen: *mut c_int,
) -> c_int;

/// Character encoding output conversion function.
///
/// Converts from UTF-8 to the handler's output encoding.
/// `in` and `inlen` describe input; `out` and `outlen` describe output buffer.
/// Returns the number of bytes written, or -1 on error.
pub type xmlCharEncodingOutputFunc = unsafe extern "C" fn(
    out: *mut c_uchar,
    outlen: *mut c_int,
    in_: *const c_uchar,
    inlen: *mut c_int,
) -> c_int;

/// Character encoding conversion implementation.
///
/// # UPSTREAM-PARITY
///
/// This is used to plug custom encoding converters into the I/O subsystem.
/// Returns the number of bytes written, or -1 on error.
pub type xmlCharEncConvImpl = unsafe extern "C" fn(
    name: *mut *const crate::abi::types::xmlChar,
    out: *mut *mut crate::abi::types::xmlChar,
    outlen: *mut c_int,
    in_: *const crate::abi::types::xmlChar,
    inlen: *mut c_int,
) -> c_int;

// ═══════════════════════════════════════════════════════════════════════════════
// Catalog Callbacks
// ═══════════════════════════════════════════════════════════════════════════════

/// Catalog preference callback.
///
/// Returns 1 if the system prefers XML catalogs, 0 otherwise.
pub type xmlCatalogPreferFunc = unsafe extern "C" fn() -> c_int;

// ═══════════════════════════════════════════════════════════════════════════════
// Allocator Callback Types
// ═══════════════════════════════════════════════════════════════════════════════

/// Free function type for allocator hooks.
pub type xmlFreeFunc = unsafe extern "C" fn(ptr: *mut c_void);

/// Malloc function type for allocator hooks.
pub type xmlMallocFunc = unsafe extern "C" fn(size: usize) -> *mut c_void;

/// Realloc function type for allocator hooks.
pub type xmlReallocFunc = unsafe extern "C" fn(ptr: *mut c_void, size: usize) -> *mut c_void;

/// Strdup function type for allocator hooks.
pub type xmlStrdupFunc = unsafe extern "C" fn(str: *const c_char) -> *mut c_void;

// ═══════════════════════════════════════════════════════════════════════════════
// Module Callbacks
// ═══════════════════════════════════════════════════════════════════════════════

/// Module register/unregister callback.
pub type xmlModuleRegisterFunc = unsafe extern "C" fn(module: *mut c_void) -> c_int;

// ═══════════════════════════════════════════════════════════════════════════════
// Pattern Callbacks
// ═══════════════════════════════════════════════════════════════════════════════

/// Stream callback for pattern matching.
pub type xmlStreamCtxtPtr = *mut c_void;

// ═══════════════════════════════════════════════════════════════════════════════
// Hash Table Callbacks
// ═══════════════════════════════════════════════════════════════════════════════

/// Deallocator function for hash table entries.
///
/// Called when removing an entry from a hash table.
/// The function receives the payload and the name (key) of the entry.
pub type xmlHashDeallocator =
    unsafe extern "C" fn(payload: *mut c_void, name: *mut crate::abi::types::xmlChar);

/// Copier function for hash table entries.
///
/// Called when copying a hash table. Returns a copy of the payload.
pub type xmlHashCopier = unsafe extern "C" fn(
    payload: *mut c_void,
    name: *const crate::abi::types::xmlChar,
) -> *mut c_void;

/// Scanner function for hash table entries.
///
/// Called for each entry during xmlHashScan.
pub type xmlHashScanner = unsafe extern "C" fn(
    payload: *mut c_void,
    data: *mut c_void,
    name: *const crate::abi::types::xmlChar,
);

/// Full scanner function for hash table entries.
///
/// Called for each entry during xmlHashScanFull. Includes all three key parts.
pub type xmlHashScannerFull = unsafe extern "C" fn(
    payload: *mut c_void,
    data: *mut c_void,
    name: *const crate::abi::types::xmlChar,
    name2: *const crate::abi::types::xmlChar,
    name3: *const crate::abi::types::xmlChar,
);
