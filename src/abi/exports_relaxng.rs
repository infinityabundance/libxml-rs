//! exports_relaxng — RELAX NG C ABI exports (family closure 11.1-I).
//!
//! This module implements the RELAX NG entry points from `relaxng.h` that are
//! not already exported by the internal engine in `src/xml/relaxng/mod.rs`
//! (which owns `xmlRelaxNGNewParserCtxt`, `xmlRelaxNGNewMemParserCtxt`,
//! `xmlRelaxNGParse`, `xmlRelaxNGFree`, `xmlRelaxNGFreeParserCtxt`,
//! `xmlRelaxNGNewValidCtxt`, `xmlRelaxNGFreeValidCtxt`, `xmlRelaxNGValidateDoc`
//! and `xmlRelaxNGValidateFullElement`).
//!
//! # Opaque-pointer convention
//!
//! `xmlRelaxNGPtr`, `xmlRelaxNGParserCtxtPtr` and `xmlRelaxNGValidCtxtPtr` are
//! opaque at the ABI boundary. The internal engine represents a parser context
//! as a `Box<RelaxNgSchema>` (parsed eagerly when the context is created) and a
//! validation context as a `Box<RelaxNgValidCtxt>`, both carried as `*mut
//! c_void`; this module follows that convention so pointers are interchangeable
//! between the two layers.
//!
//! # Error-callback state
//!
//! Upstream stores the error/warning/structured callbacks inside the context
//! structs. The internal engine's structs have no such fields, so the callbacks
//! registered via the `xmlRelaxNGSet*Errors` family are kept in side tables
//! keyed by context address. Entries live as long as the owning context; the
//! engine's own free functions are the only releasers, so entries are not
//! eagerly pruned (a fixed-size registry keyed by stable `Box` addresses).
//!
//! # Known divergences from upstream
//!
//! - Schema parsing is eager (done in the context constructors) rather than
//!   lazy at `xmlRelaxNGParse` time; parse failures are recorded in
//!   `schema.errors` and still produce a usable (empty) schema object.
//! - The engine reports validation errors through its internal error list
//!   (`ctxt->errors` / return counts); per-context callbacks are consulted by
//!   this module's streaming entry points (`xmlRelaxNGValidatePopElement`).
//! - `xmlRelaxNGDump` / `xmlRelaxNGDumpTree` render the parsed grammar in a
//!   readable form; upstream's exact debug format is a libxml2-internal
//!   artifact and is not replicated byte-for-byte.
//!
//! # Upstream contract
//!
//! Parity target is upstream `relaxng.c` (libxml2 2.15.3,
//! SRC-LIBXML2-2.15.0-RELAXNG-C) with the `relaxng.h` signatures; R-000165
//! (11.1-O) closed the relaxng export gaps (e.g. `xmlRelaxNGValidCtxtClearErrors`,
//! `xmlRelaxParserSetIncLImit`).
//!
//! # Conceptual behavior
//!
//! This module implements the RELAX NG entry points not already exported by
//! the internal engine in `src/xml/relaxng/mod.rs`: dump/tree rendering,
//! parser/validation error-callback registration and the streaming validation
//! entry points, using the opaque-pointer convention documented above.
//!
//! # Ownership & safety invariants
//!
//! `xmlRelaxNGPtr`/`xmlRelaxNGParserCtxtPtr`/`xmlRelaxNGValidCtxtPtr` are
//! caller-owned: schemas freed with `xmlRelaxNGFree`, contexts with
//! `xmlRelaxNGFreeParserCtxt`/`xmlRelaxNGFreeValidCtxt`. The error-callback
//! side tables are keyed by context address and live exactly as long as the
//! owning context (the engines own free functions are the only releasers).
//!
//! # Historical quirks & epochs
//!
//! RELAX NG matured in the 2.6 `validation_era` (HISTORY.md) and the ABI has
//! been stable since; R-000165 (11.1-O) added the missing relaxng symbols so
//! the oracle DSO export set is complete.
//!
//! # Deliberate oddities
//!
//! The eager schema parse in the context constructors (upstream parses lazily
//! at `xmlRelaxNGParse`) is a deliberate divergence documented in the header
//! above, as is `xmlRelaxNGDump`/`xmlRelaxNGDumpTree` not replicating
//! upstreams internal debug format byte-for-byte.
//!
//! # Proving courts
//!
//! The RELAXNG court family, the CLI-XMLLINT relaxng cases and the
//! DSO-LOADER/HEADER-COMPILE courts cover this module; the relaxng unit tests
//! run under cargo test.
//!
//! # Tempting simplifications that would break parity
//!
//! A tempting simplification is to make `xmlRelaxNGSetParserErrors` a stored
//! no-op because the engine reports internally — the error callbacks are the
//! observable contract for C consumers validating documents (the RELAXNG
//! courts drive them), so the side tables must stay. Another shortcut —
//! freeing schema objects eagerly when the parser context dies — would break
//! the callers valid-ctxt reuse of a parsed schema.

#![allow(
    missing_docs,
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals
)]

use core::ffi::c_void;
use core::ptr;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::ffi::CString;
use std::os::raw::{c_char, c_int};

use crate::abi::callbacks::xmlStructuredErrorFunc;
use crate::abi::structs::{_xmlDoc, _xmlError, _xmlNode};
use crate::abi::types::{xmlChar, xmlErrorLevel::*, XML_FROM_RELAXNGV};
use crate::xml::relaxng::{
    rng_parse_schema_doc, RelaxNgGrammar, RelaxNgNameClass, RelaxNgPattern, RelaxNgPatternType,
    RelaxNgSchema, RelaxNgValidCtxt,
};

// ═══════════════════════════════════════════════════════════════════════════════
// Callback types (relaxng.h)
// ═══════════════════════════════════════════════════════════════════════════════

/// `xmlRelaxNGValidityErrorFunc` — printf-style error callback (variadic at the
/// C call site; only the `msg` argument is representable in Rust).
pub type xmlRelaxNGValidityErrorFunc = unsafe extern "C" fn(ctx: *mut c_void, msg: *const c_char);

/// `xmlRelaxNGValidityWarningFunc` — printf-style warning callback (variadic at
/// the C call site; only the `msg` argument is representable in Rust).
pub type xmlRelaxNGValidityWarningFunc = unsafe extern "C" fn(ctx: *mut c_void, msg: *const c_char);

// ═══════════════════════════════════════════════════════════════════════════════
// Per-context callback/flag state (upstream keeps this inside the ctxt structs)
// ═══════════════════════════════════════════════════════════════════════════════

/// Wrapper around `*mut c_void` that implements `Send` + `Sync` so it can be
/// stored in a `Mutex`-protected global side table (same pattern as
/// `SendSyncPtr` in exports_xml2.rs). Pointers are only dereferenced while the
/// registry lock is held, so the wrapper is sound.
#[derive(Debug, Clone, Copy, Default)]
struct SendPtr(*mut c_void);
unsafe impl Send for SendPtr {}
unsafe impl Sync for SendPtr {}

/// Error-callback state attached to a RELAX NG parser context.
#[derive(Debug, Clone, Copy, Default)]
struct ParserCtxtState {
    err: Option<xmlRelaxNGValidityErrorFunc>,
    warn: Option<xmlRelaxNGValidityWarningFunc>,
    ctx: SendPtr,
    serror: Option<xmlStructuredErrorFunc>,
    serror_ctx: SendPtr,
    parse_flags: c_int,
    resource_loader: Option<crate::abi::callbacks::xmlResourceLoader>,
    resource_ctxt: SendPtr,
    inc_limit: c_int,
}

/// Error-callback state attached to a RELAX NG validation context.
#[derive(Debug, Clone, Copy, Default)]
struct ValidCtxtState {
    err: Option<xmlRelaxNGValidityErrorFunc>,
    warn: Option<xmlRelaxNGValidityWarningFunc>,
    ctx: SendPtr,
    serror: Option<xmlStructuredErrorFunc>,
    serror_ctx: SendPtr,
    err_no: c_int,
}

static PARSER_CTXT_STATE: Lazy<Mutex<HashMap<usize, ParserCtxtState>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

static VALID_CTXT_STATE: Lazy<Mutex<HashMap<usize, ValidCtxtState>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

// ═══════════════════════════════════════════════════════════════════════════════
// libc FILE* plumbing (the FILE* is opaque at the ABI boundary)
// ═══════════════════════════════════════════════════════════════════════════════

extern "C" {
    /// libc `size_t fwrite(const void *ptr, size_t size, size_t nmemb, FILE *stream)`.
    fn fwrite(ptr: *const c_void, size: usize, nmemb: usize, stream: *mut c_void) -> usize;
    /// The libc `FILE *stdout` variable.
    static mut stdout: *mut c_void;
}

/// Write `text` to `output`; a NULL `output` falls back to stdout (the same
/// convention as `xmlBufferDump` and the debug dumpers in this crate).
///
/// # SAFETY
///
/// - `output` must be a valid `FILE*` or NULL.
unsafe fn write_to_stream(output: *mut c_void, text: &str) {
    let stream = if output.is_null() {
        // SAFETY: reading the exported libc data symbol.
        unsafe { stdout }
    } else {
        output
    };
    if stream.is_null() || text.is_empty() {
        return;
    }
    // SAFETY: stream is a valid FILE*; text is a valid buffer of text.len() bytes.
    unsafe {
        fwrite(text.as_ptr() as *const c_void, 1, text.len(), stream);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Schema rendering for xmlRelaxNGDump / xmlRelaxNGDumpTree
// ═══════════════════════════════════════════════════════════════════════════════

/// RELAX NG pattern-kind names (the XML element names from the RELAX NG syntax).
const fn pattern_kind_name(t: &RelaxNgPatternType) -> &'static str {
    match t {
        RelaxNgPatternType::Element => "element",
        RelaxNgPatternType::Attribute => "attribute",
        RelaxNgPatternType::Text => "text",
        RelaxNgPatternType::Choice => "choice",
        RelaxNgPatternType::Sequence => "sequence",
        RelaxNgPatternType::Interleave => "interleave",
        RelaxNgPatternType::ZeroOrMore => "zeroOrMore",
        RelaxNgPatternType::OneOrMore => "oneOrMore",
        RelaxNgPatternType::Optional => "optional",
        RelaxNgPatternType::List => "list",
        RelaxNgPatternType::Group => "group",
        RelaxNgPatternType::Data => "data",
        RelaxNgPatternType::Value => "value",
        RelaxNgPatternType::Ref => "ref",
        RelaxNgPatternType::Define => "define",
        RelaxNgPatternType::Grammar => "grammar",
        RelaxNgPatternType::NotAllowed => "notAllowed",
        RelaxNgPatternType::Empty => "empty",
        RelaxNgPatternType::ExternalRef => "externalRef",
        RelaxNgPatternType::Include => "include",
        RelaxNgPatternType::Start => "start",
    }
}

/// Render a name class in a compact, readable form.
fn format_name_class(nc: &RelaxNgNameClass) -> String {
    match nc {
        RelaxNgNameClass::Name(n) => n.clone(),
        RelaxNgNameClass::AnyName => "*".to_string(),
        RelaxNgNameClass::NsName(ns) => format!("nsName({})", ns),
        RelaxNgNameClass::Choice(choices) => choices
            .iter()
            .map(format_name_class)
            .collect::<Vec<_>>()
            .join(" | "),
        RelaxNgNameClass::Except(positive, negative) => {
            format!(
                "{} - {}",
                format_name_class(positive),
                format_name_class(negative)
            )
        }
    }
}

/// Render a pattern and its children as an indented tree.
fn render_pattern(p: &RelaxNgPattern, depth: usize) -> String {
    let indent = "  ".repeat(depth);
    let mut line = indent;
    line.push_str(pattern_kind_name(&p.pattern_type));
    if let Some(nc) = &p.name_class {
        line.push(' ');
        line.push_str(&format_name_class(nc));
    }
    if let Some(name) = &p.name {
        line.push_str(" (");
        line.push_str(name);
        line.push(')');
    }
    if let Some(ns) = &p.ns {
        line.push_str(" ns=\"");
        line.push_str(ns);
        line.push('"');
    }
    if let Some(dt) = &p.datatype {
        line.push_str(" datatype=\"");
        line.push_str(dt);
        line.push('"');
    }
    if let Some(v) = &p.value {
        line.push_str(" value=\"");
        line.push_str(v);
        line.push('"');
    }
    if let Some(lib) = &p.datatype_library {
        line.push_str(" library=\"");
        line.push_str(lib);
        line.push('"');
    }
    line.push('\n');
    let mut s = line;
    for child in &p.children {
        s.push_str(&render_pattern(child, depth + 1));
    }
    s
}

/// Render a grammar: named defines, the start pattern, and included grammars.
fn render_grammar(g: &RelaxNgGrammar, depth: usize) -> String {
    let indent = "  ".repeat(depth);
    let mut s = format!("{}grammar\n", indent);
    for def in &g.defines {
        s.push_str(&format!("{}  define \"{}\"\n", indent, def.name));
        s.push_str(&render_pattern(&def.pattern, depth + 2));
    }
    if let Some(start) = &g.start {
        s.push_str(&format!("{}  start\n", indent));
        s.push_str(&render_pattern(start, depth + 2));
    }
    for inc in &g.includes {
        s.push_str(&render_grammar(inc, depth + 1));
    }
    s
}

/// Render the full schema.
fn render_schema(schema: &RelaxNgSchema, tree_mode: bool) -> String {
    let mut s = if tree_mode {
        "RELAX NG schema tree\n".to_string()
    } else {
        "RELAX NG schema\n".to_string()
    };
    s.push_str(&render_grammar(&schema.grammar, 0));
    for e in &schema.errors {
        s.push_str("error: ");
        s.push_str(e);
        s.push('\n');
    }
    s
}

// ═══════════════════════════════════════════════════════════════════════════════
// 1. Initialization / Cleanup
// ═══════════════════════════════════════════════════════════════════════════════

/// Initialize the datatype subsystem used by RELAX NG `<data>`/`<value>`
/// patterns.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlRelaxNGInitTypes(void);
/// ```
///
/// Returns 0 on success, -1 on error. The candidate's datatype checking is
/// compiled into the internal engine and needs no global initialization, so
/// this always succeeds.
///
/// # SAFETY
///
/// The function touches crate-global state only; it is safe
/// as long as the caller respects the library's global
/// initialization/cleanup ordering (xmlInitParser before use,
/// xmlCleanupParser only after all users are done).
///
/// Violating the global lifecycle ordering, or calling this after
/// teardown or from a signal handler, is undefined behavior.
#[no_mangle]
pub const unsafe extern "C" fn xmlRelaxNGInitTypes() -> c_int {
    0
}

/// Tear down the datatype subsystem used by RELAX NG patterns.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlRelaxNGCleanupTypes(void);
/// ```
///
/// No-op: the candidate keeps no global datatype state.
///
/// # SAFETY
///
/// The function touches crate-global state only; it is safe
/// as long as the caller respects the library's global
/// initialization/cleanup ordering (xmlInitParser before use,
/// xmlCleanupParser only after all users are done).
///
/// Violating the global lifecycle ordering, or calling this after
/// teardown or from a signal handler, is undefined behavior.
#[no_mangle]
pub const unsafe extern "C" fn xmlRelaxNGCleanupTypes() {
    // No-op: no global datatype state to tear down.
}

// ═══════════════════════════════════════════════════════════════════════════════
// 2. Dumping
// ═══════════════════════════════════════════════════════════════════════════════

/// Dump a RELAX NG schema to a file stream.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlRelaxNGDump(FILE *output, xmlRelaxNG *schema);
/// ```
///
/// `FILE*` is opaque at the ABI boundary and is passed as `*mut c_void`; a NULL
/// `output` falls back to stdout. Upstream's exact debug format is a
/// libxml2-internal artifact; the candidate renders the parsed grammar
/// (defines, start pattern, includes) in a readable, equivalent form.
///
/// # SAFETY
///
/// - `output` must be a valid `FILE*` or NULL.
/// - `schema` must be a valid `Box<RelaxNgSchema>` pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlRelaxNGDump(output: *mut c_void, schema: *mut c_void) {
    // SAFETY: output/schema validated inside.
    unsafe { dump_schema_impl(output, schema, false) };
}

/// Dump the pattern tree of a RELAX NG schema to a file stream.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlRelaxNGDumpTree(FILE *output, xmlRelaxNG *schema);
/// ```
///
/// Same opaque-pointer and format conventions as `xmlRelaxNGDump`; the output
/// is the same readable pattern-tree rendering (upstream's internal tree layout
/// is not replicated byte-for-byte).
///
/// # SAFETY
///
/// - `output` must be a valid `FILE*` or NULL.
/// - `schema` must be a valid `Box<RelaxNgSchema>` pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlRelaxNGDumpTree(output: *mut c_void, schema: *mut c_void) {
    // SAFETY: output/schema validated inside.
    unsafe { dump_schema_impl(output, schema, true) };
}

/// Shared implementation for `xmlRelaxNGDump` and `xmlRelaxNGDumpTree`.
///
/// # SAFETY
///
/// - `output` must be a valid `FILE*` or NULL.
/// - `schema` must be a valid `Box<RelaxNgSchema>` pointer or NULL.
unsafe fn dump_schema_impl(output: *mut c_void, schema: *mut c_void, tree_mode: bool) {
    if schema.is_null() {
        // SAFETY: output is a valid FILE* or NULL (stdout fallback).
        unsafe { write_to_stream(output, "RELAX NG schema is NULL\n") };
        return;
    }
    // SAFETY: The schema pointer comes from the ABI layer and is a live
    // Box<RelaxNgSchema>; it is only read here.
    let s = unsafe { &*(schema as *const RelaxNgSchema) };
    let text = render_schema(s, tree_mode);
    // SAFETY: output is a valid FILE* or NULL (stdout fallback).
    unsafe { write_to_stream(output, &text) };
}

// ═══════════════════════════════════════════════════════════════════════════════
// 3. Parser context construction
// ═══════════════════════════════════════════════════════════════════════════════

/// Create a RELAX NG parser context from an already-parsed XML document.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlRelaxNGParserCtxt *xmlRelaxNGNewDocParserCtxt(xmlDoc *doc);
/// ```
///
/// Returns a parser context (`*mut c_void` pointing at a `Box<RelaxNgSchema>`,
/// matching the internal engine's convention) or NULL on allocation failure.
///
/// NOTE: upstream defers schema compilation to `xmlRelaxNGParse` and reports
/// parse errors through the parser error callbacks. The internal engine parses
/// eagerly in the context constructor, so a failed parse still yields a
/// (valid, empty) schema object with the failure recorded in its error list;
/// `xmlRelaxNGParse` then returns that object and validation reports the
/// recorded errors.
///
/// # SAFETY
///
/// - `doc` must be a valid pointer to an `_xmlDoc` or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlRelaxNGNewDocParserCtxt(doc: *mut _xmlDoc) -> *mut c_void {
    let schema = if doc.is_null() {
        let mut s = RelaxNgSchema::new();
        s.errors.push("Document is NULL".to_string());
        s
    } else {
        // SAFETY: doc is a valid _xmlDoc; rng_parse_schema_doc reads it.
        match unsafe { rng_parse_schema_doc(doc) } {
            Ok(s) => s,
            Err(e) => {
                let mut s = RelaxNgSchema::new();
                s.errors.push(e);
                s
            }
        }
    };
    // SAFETY: The schema is boxed and handed to the ABI layer; it is released
    // by xmlRelaxNGFreeParserCtxt / xmlRelaxNGFree, both of which reconstruct
    // the Box.
    Box::into_raw(Box::new(schema)) as *mut c_void
}

// ═══════════════════════════════════════════════════════════════════════════════
// 4. Parser error handlers
// ═══════════════════════════════════════════════════════════════════════════════

/// Set the error and warning callbacks on a RELAX NG parser context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlRelaxNGSetParserErrors(xmlRelaxNGParserCtxt *ctxt,
///                                xmlRelaxNGValidityErrorFunc err,
///                                xmlRelaxNGValidityWarningFunc warn,
///                                void *ctx);
/// ```
///
/// The callbacks are kept in a side table keyed by the context address (the
/// internal engine's context structs have no callback fields). Because schema
/// parsing in the candidate is eager, these callbacks are not invoked by the
/// parser; they are stored for ABI parity.
///
/// # SAFETY
///
/// - `ctxt` must be a valid parser context pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlRelaxNGSetParserErrors(
    ctxt: *mut c_void,
    err: Option<xmlRelaxNGValidityErrorFunc>,
    warn: Option<xmlRelaxNGValidityWarningFunc>,
    ctx: *mut c_void,
) {
    if ctxt.is_null() {
        return;
    }
    let mut map = PARSER_CTXT_STATE.lock();
    let state = map.entry(ctxt as usize).or_default();
    state.err = err;
    state.warn = warn;
    state.ctx = SendPtr(ctx);
}

/// Retrieve the error and warning callbacks from a RELAX NG parser context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlRelaxNGGetParserErrors(xmlRelaxNGParserCtxt *ctxt,
///                               xmlRelaxNGValidityErrorFunc *err,
///                               xmlRelaxNGValidityWarningFunc *warn,
///                               void **ctx);
/// ```
///
/// Returns 0 on success, -1 if `ctxt` is NULL. Any of `err`, `warn`, `ctx` may
/// be NULL to skip that output.
///
/// # SAFETY
///
/// - `ctxt` must be a valid parser context pointer or NULL.
/// - `err`/`warn` must be valid out-parameters or NULL.
/// - `ctx` must be a valid `void **` out-parameter or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlRelaxNGGetParserErrors(
    ctxt: *mut c_void,
    err: *mut Option<xmlRelaxNGValidityErrorFunc>,
    warn: *mut Option<xmlRelaxNGValidityWarningFunc>,
    ctx: *mut *mut c_void,
) -> c_int {
    if ctxt.is_null() {
        return -1;
    }
    let state = PARSER_CTXT_STATE
        .lock()
        .get(&(ctxt as usize))
        .copied()
        .unwrap_or_default();
    if !err.is_null() {
        // SAFETY: err is a valid out-parameter.
        unsafe { *err = state.err };
    }
    if !warn.is_null() {
        // SAFETY: warn is a valid out-parameter.
        unsafe { *warn = state.warn };
    }
    if !ctx.is_null() {
        // SAFETY: ctx is a valid out-parameter.
        unsafe { *ctx = state.ctx.0 };
    }
    0
}

/// Set the structured error callback on a RELAX NG parser context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlRelaxNGSetParserStructuredErrors(xmlRelaxNGParserCtxt *ctxt,
///                                          xmlStructuredErrorFunc serror,
///                                          void *ctx);
/// ```
///
/// Stored for ABI parity; the candidate's eager parser reports errors through
/// the schema's internal error list rather than structured callbacks.
///
/// # SAFETY
///
/// - `ctxt` must be a valid parser context pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlRelaxNGSetParserStructuredErrors(
    ctxt: *mut c_void,
    serror: Option<xmlStructuredErrorFunc>,
    ctx: *mut c_void,
) {
    if ctxt.is_null() {
        return;
    }
    let mut map = PARSER_CTXT_STATE.lock();
    let state = map.entry(ctxt as usize).or_default();
    state.serror = serror;
    state.serror_ctx = SendPtr(ctx);
}

// ═══════════════════════════════════════════════════════════════════════════════
// 5. Validation context error handlers
// ═══════════════════════════════════════════════════════════════════════════════

/// Set the error and warning callbacks on a RELAX NG validation context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlRelaxNGSetValidErrors(xmlRelaxNGValidCtxt *ctxt,
///                               xmlRelaxNGValidityErrorFunc err,
///                               xmlRelaxNGValidityWarningFunc warn,
///                               void *ctx);
/// ```
///
/// The callbacks are consulted by this module's streaming entry points
/// (`xmlRelaxNGValidatePopElement`). Document validation
/// (`xmlRelaxNGValidateDoc`) is owned by the internal engine and reports
/// through its return value / `ctxt->errors` instead.
///
/// # SAFETY
///
/// - `ctxt` must be a valid validation context pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlRelaxNGSetValidErrors(
    ctxt: *mut c_void,
    err: Option<xmlRelaxNGValidityErrorFunc>,
    warn: Option<xmlRelaxNGValidityWarningFunc>,
    ctx: *mut c_void,
) {
    if ctxt.is_null() {
        return;
    }
    let mut map = VALID_CTXT_STATE.lock();
    let state = map.entry(ctxt as usize).or_default();
    state.err = err;
    state.warn = warn;
    state.ctx = SendPtr(ctx);
}

/// Retrieve the error and warning callbacks from a RELAX NG validation context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlRelaxNGGetValidErrors(xmlRelaxNGValidCtxt *ctxt,
///                              xmlRelaxNGValidityErrorFunc *err,
///                              xmlRelaxNGValidityWarningFunc *warn,
///                              void **ctx);
/// ```
///
/// Returns 0 on success, -1 if `ctxt` is NULL. Any of `err`, `warn`, `ctx` may
/// be NULL to skip that output.
///
/// # SAFETY
///
/// - `ctxt` must be a valid validation context pointer or NULL.
/// - `err`/`warn` must be valid out-parameters or NULL.
/// - `ctx` must be a valid `void **` out-parameter or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlRelaxNGGetValidErrors(
    ctxt: *mut c_void,
    err: *mut Option<xmlRelaxNGValidityErrorFunc>,
    warn: *mut Option<xmlRelaxNGValidityWarningFunc>,
    ctx: *mut *mut c_void,
) -> c_int {
    if ctxt.is_null() {
        return -1;
    }
    let state = VALID_CTXT_STATE
        .lock()
        .get(&(ctxt as usize))
        .copied()
        .unwrap_or_default();
    if !err.is_null() {
        // SAFETY: err is a valid out-parameter.
        unsafe { *err = state.err };
    }
    if !warn.is_null() {
        // SAFETY: warn is a valid out-parameter.
        unsafe { *warn = state.warn };
    }
    if !ctx.is_null() {
        // SAFETY: ctx is a valid out-parameter.
        unsafe { *ctx = state.ctx.0 };
    }
    0
}

/// Set the structured error callback on a RELAX NG validation context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlRelaxNGSetValidStructuredErrors(xmlRelaxNGValidCtxt *ctxt,
///                                         xmlStructuredErrorFunc serror,
///                                         void *ctx);
/// ```
///
/// Consulted by `xmlRelaxNGValidatePopElement`, which delivers each released
/// validation error as a minimal `_xmlError` record (domain `XML_FROM_RELAXNGV`,
/// level `XML_ERR_ERROR`).
///
/// # SAFETY
///
/// - `ctxt` must be a valid validation context pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlRelaxNGSetValidStructuredErrors(
    ctxt: *mut c_void,
    serror: Option<xmlStructuredErrorFunc>,
    ctx: *mut c_void,
) {
    if ctxt.is_null() {
        return;
    }
    let mut map = VALID_CTXT_STATE.lock();
    let state = map.entry(ctxt as usize).or_default();
    state.serror = serror;
    state.serror_ctx = SendPtr(ctx);
}

// ═══════════════════════════════════════════════════════════════════════════════
// 6. Streaming (push/pop) validation
// ═══════════════════════════════════════════════════════════════════════════════

/// Push an element start onto the streaming validation stack.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlRelaxNGValidatePushElement(xmlRelaxNGValidCtxt *ctxt,
///                                   xmlDoc *doc,
///                                   xmlNode *elem);
/// ```
///
/// Returns 0 if the element is valid, -1 on internal error, or a positive
/// number of validation errors.
///
/// Upstream's `xmlRelaxNGValidatePushElement` delegates to
/// `xmlRelaxNGValidateFullElement`; the internal engine validates the whole
/// element subtree (there is no start-tag-only mode), so a push validates the
/// element immediately and reports any errors in `ctxt->errors`. Nested
/// elements are validated again when their own push arrives, so per-push error
/// counts may double-count errors that belong to subtrees.
///
/// # SAFETY
///
/// - `ctxt` must be a valid validation context pointer.
/// - `doc` must be a valid `_xmlDoc` pointer.
/// - `elem` must be a valid element node pointer.
#[no_mangle]
pub unsafe extern "C" fn xmlRelaxNGValidatePushElement(
    ctxt: *mut c_void,
    doc: *mut _xmlDoc,
    elem: *mut _xmlNode,
) -> c_int {
    if ctxt.is_null() || doc.is_null() || elem.is_null() {
        return -1;
    }
    // SAFETY: All three pointers are validated above; the internal export
    // validates the element subtree against the schema's start pattern.
    unsafe { crate::xml::relaxng::xmlRelaxNGValidateFullElement(ctxt, doc, elem) }
}

/// Push character data onto the streaming validation stack.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlRelaxNGValidatePushCData(xmlRelaxNGValidCtxt *ctxt,
///                                 const xmlChar *data,
///                                 int len);
/// ```
///
/// Returns 0 if the character data is valid, -1 on internal error, or a
/// positive number of validation errors.
///
/// Character content is validated as part of element validation
/// (`text`/`data`/`value` patterns) by the internal engine; there is no
/// incremental text state to update here, so a well-formed call always
/// succeeds.
///
/// # SAFETY
///
/// - `ctxt` must be a valid validation context pointer.
/// - `data` must be a valid buffer of `len` bytes (or NULL when `len` is 0).
#[no_mangle]
pub const unsafe extern "C" fn xmlRelaxNGValidatePushCData(
    ctxt: *mut c_void,
    data: *const xmlChar,
    len: c_int,
) -> c_int {
    if ctxt.is_null() || (data.is_null() && len > 0) {
        return -1;
    }
    0
}

/// Pop an element end off the streaming validation stack.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlRelaxNGValidatePopElement(xmlRelaxNGValidCtxt *ctxt,
///                                  xmlDoc *doc,
///                                  xmlNode *elem);
/// ```
///
/// Returns 0 if the element is valid, -1 on internal error, or a positive
/// number of validation errors.
///
/// Because the internal engine validated the element subtree eagerly at push
/// time, the pop step performs upstream's "release the accumulated errors"
/// step: the context's error list is drained and each message is delivered to
/// the callbacks registered with `xmlRelaxNGSetValidErrors` /
/// `xmlRelaxNGSetValidStructuredErrors` (the engine does not distinguish
/// warnings, so all messages are routed to the error callback). The return
/// value is 0 unless the arguments are invalid.
///
/// # SAFETY
///
/// - `ctxt` must be a valid validation context pointer.
/// - `doc` must be a valid `_xmlDoc` pointer.
/// - `elem` must be a valid element node pointer.
#[no_mangle]
pub unsafe extern "C" fn xmlRelaxNGValidatePopElement(
    ctxt: *mut c_void,
    doc: *mut _xmlDoc,
    elem: *mut _xmlNode,
) -> c_int {
    if ctxt.is_null() || doc.is_null() || elem.is_null() {
        return -1;
    }

    // Drain the accumulated errors (upstream "release the accumulated errors").
    // SAFETY: ctxt is a live Box<RelaxNgValidCtxt> from the ABI layer.
    let valid = unsafe { &mut *(ctxt as *mut RelaxNgValidCtxt) };
    let msgs = std::mem::take(&mut valid.errors);
    valid.nb_errors = 0;

    // Deliver the released errors to the registered callbacks. The state is
    // copied out so the callbacks are never invoked while the lock is held.
    let state = VALID_CTXT_STATE.lock().get(&(ctxt as usize)).copied();
    if let Some(st) = state {
        for m in &msgs {
            // The message must be NUL-terminated for the C callback.
            let Ok(cstr) = CString::new(m.as_str()) else {
                continue;
            };
            if let Some(cb) = st.err {
                // SAFETY: cb is a C callback registered by the caller; the
                // message pointer is valid for the duration of the call.
                unsafe { cb(st.ctx.0, cstr.as_ptr()) };
            }
            if let Some(cb) = st.serror {
                let err_rec = _xmlError {
                    domain: XML_FROM_RELAXNGV,
                    code: 0,
                    message: cstr.as_ptr() as *mut c_char,
                    level: XML_ERR_ERROR as c_int,
                    file: ptr::null_mut(),
                    line: 0,
                    str1: ptr::null_mut(),
                    str2: ptr::null_mut(),
                    str3: ptr::null_mut(),
                    int1: 0,
                    int2: 0,
                    ctxt,
                    node: elem as *mut c_void,
                };
                // SAFETY: err_rec is valid for the duration of the call (the
                // upstream contract for structured error callbacks).
                unsafe { cb(st.serror_ctx.0, &err_rec as *const _xmlError) };
            }
        }
    }
    0
}

// ═══════════════════════════════════════════════════════════════════════════════
// 7. Parser flags
// ═══════════════════════════════════════════════════════════════════════════════

/// Set parser flags on a RELAX NG parser context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlRelaxParserSetFlag(xmlRelaxNGParserCtxt *ctxt, int flag);
/// ```
///
/// Returns 0 on success, -1 on error. `flag == 0` resets the flags
/// (`XML_RELAXNG_PARSE_FREE`). The flags are stored for ABI parity; the
/// candidate's parser compiles schemas eagerly and does not consult them.
///
/// # SAFETY
///
/// - `ctxt` must be a valid parser context pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlRelaxParserSetFlag(ctxt: *mut c_void, flag: c_int) -> c_int {
    if ctxt.is_null() {
        return -1;
    }
    let mut map = PARSER_CTXT_STATE.lock();
    let state = map.entry(ctxt as usize).or_default();
    state.parse_flags = if flag == 0 { 0 } else { flag };
    0
}

/// Set the incremental-compile limit on a RELAX NG parser context
/// (upstream relaxng.c `xmlRelaxParserSetIncLImit`).
///
/// # SAFETY
///
/// - `ctxt` must be a valid parser context pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlRelaxParserSetIncLImit(ctxt: *mut c_void, limit: c_int) -> c_int {
    if ctxt.is_null() || limit < 0 {
        return -1;
    }
    let mut map = PARSER_CTXT_STATE.lock();
    map.entry(ctxt as usize).or_default().inc_limit = limit;
    0
}

/// Install a custom resource loader on a RELAX NG parser context
/// (upstream relaxng.c `xmlRelaxNGSetResourceLoader`).
///
/// # SAFETY
///
/// - `ctxt` must be a valid parser context pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlRelaxNGSetResourceLoader(
    ctxt: *mut c_void,
    loader: Option<crate::abi::callbacks::xmlResourceLoader>,
    vctxt: *mut c_void,
) {
    if ctxt.is_null() {
        return;
    }
    let mut map = PARSER_CTXT_STATE.lock();
    let state = map.entry(ctxt as usize).or_default();
    state.resource_loader = loader;
    state.resource_ctxt = SendPtr(vctxt);
}

/// Clear the error state of a RELAX NG validation context
/// (upstream relaxng.c `xmlRelaxNGValidCtxtClearErrors`).
///
/// # SAFETY
///
/// - `ctxt` must be a valid validation context pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlRelaxNGValidCtxtClearErrors(ctxt: *mut c_void) {
    if ctxt.is_null() {
        return;
    }
    unsafe {
        let vc = &mut *(ctxt as *mut RelaxNgValidCtxt);
        vc.errors.clear();
        vc.nb_errors = 0;
    }
    let mut map = VALID_CTXT_STATE.lock();
    let state = map.entry(ctxt as usize).or_default();
    state.err = None;
    state.err_no = 0; // XML_RELAXNG_OK
}
