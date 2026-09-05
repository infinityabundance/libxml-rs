//! Parser helper functions — bridge between C ABI exports and the internal parser (§19).
//!
//! This module provides the glue layer that C ABI entry points (in `crate::abi::exports_xml2`)
//! call into. Each helper takes raw C pointers and converts them to safe Rust types,
//! delegates to the internal parser, and converts results back.
//!
//! # Ownership model
//!
//! When `setup_parser_input` is called, the `InputBuffer` is boxed and leaked (stored as a raw
//! pointer in a per-context side table, NOT in `ctxt._private`). This keeps the buffer's data
//! alive so that the `_xmlParserInput` pointers (`base`/`cur`/`end`) remain valid.
//! `parse_document` and `parse_chunk` take ownership of that boxed buffer, move it into an
//! `InputStack`, create an `XmlParser`, run it, and drop everything — consuming the buffer in
//! the process.
//!
//! # Why a side table?
//!
//! `ctxt._private` is application data (upstream `xmlCtxtSetPrivate`/`xmlCtxtGetPrivate`); the
//! candidate must never stash internal parse state there, or freeing a context whose private
//! field the application set would free the application's pointer as an `InputBuffer`
//! (11.1-X R-000165 closure discovery). The boxed input lives in `PARSER_INPUT_STASH` keyed by
//! context address and is released by `free_parser_ctxt`/`xmlCtxtReset`/`parse_document`.
//!
//! # Upstream contract
//!
//! Glue layer between the C ABI entry points (`crate::abi::exports_*`) and the
//! internal parser; mirrors the context-creation and parse-entry surface of
//! upstream parser.c and parserInternals.c (SRC-LIBXML2-2.15.0). Parity
//! target: the system libxml2 2.15.3 oracle.
//!
//! # Conceptual behavior
//!
//! Each helper takes raw C pointers, converts them to safe Rust types,
//! delegates to the internal parser, and converts results back. It also owns
//! the per-context input-buffer stash that keeps `_xmlParserInput`
//! base/cur/end pointers alive across the parse.
//!
//! # Ownership & safety invariants
//!
//! Ownership model (see above): `setup_parser_input` boxes and leaks the
//! InputBuffer into the `PARSER_INPUT_STASH` side table keyed by context
//! address; `parse_document` / `parse_chunk` take that box back, run the
//! parser, and drop everything. SAFETY: `ctxt._private` is never used for
//! internal state — it is application data (`xmlCtxtSetPrivate`), and stashing
//! the input there would free the application pointer as an InputBuffer
//! (11.1-X R-000165 closure discovery). Filenames are owned duplicates
//! (`xml_strndup`, R-000169).
//!
//! # Historical quirks & epochs
//!
//! The side-table design dates from the 11.1-X closure (R-000165/R-000169):
//! earlier code either borrowed the boxed buffer Rust String into
//! `_xmlParserInput.filename` (dangling) or `xml_strdup`ed a non-NUL-
//! terminated `as_ptr()` (heap-buffer-overflow, caught by ASan). Epoch: the
//! 2.15.3 oracle era.
//!
//! # Deliberate oddities
//!
//! The stash is a deliberate oddity: a global mutex map keyed by context
//! address with StashPtr manually Send+Sync — required because the C-visible
//! input must outlive the caller input-buffer handle but cannot live in
//! `_private`.
//!
//! # Proving courts
//!
//! Exercised by TREE-001 (doc->URL / base fingerprints), ERROR-001 (filename
//! prefixes like e.xml:1:), the DSO-LOADER court and `cargo test --lib`
//! (ASan-clean). Receipts under courts/receipts/phase-11.
//!
//! # Tempting simplifications that would break parity
//!
//! The tempting simplification is stashing the boxed input in `ctxt._private`
//! — that would free the application pointer when the context is freed
//! (11.1-X discovery) and break `xmlCtxtSetPrivate` consumers. A second one
//! is passing Rust String slices as filenames — non-NUL-terminated and
//! dangling after the context dies (R-000169). Never do either.

use core::ffi::CStr;
use core::ptr;
use std::collections::{HashMap, HashSet};
use std::mem::size_of;
use std::os::raw::{c_char, c_int, c_void};

use crate::abi::allocator::{xmlFreeImpl, xmlMallocImpl, xmlMallocZero};
use crate::abi::callbacks::{xmlInputCloseCallback, xmlInputReadCallback};
use crate::abi::structs::{_xmlParserCtxt, _xmlParserInput, _xmlParserInputBuffer, _xmlSAXHandler};
use crate::xml::parser::input::{InputBuffer, InputStack};
use crate::xml::parser::state::XmlParser;
use crate::xml::sax::xmlSAX2InitDefaultSAXHandler;

/// Per-context stash of the boxed `InputBuffer` backing the C-visible
/// `_xmlParserInput` (see the module docs for why this is not `ctxt._private`).
struct StashPtr(*mut InputBuffer);
unsafe impl Send for StashPtr {}
unsafe impl Sync for StashPtr {}

static PARSER_INPUT_STASH: once_cell::sync::Lazy<parking_lot::Mutex<HashMap<usize, StashPtr>>> =
    once_cell::sync::Lazy::new(Default::default);

/// Per-context incremental-push state (SP-14.3.1-3): whether the accumulated
/// input has already parsed as a complete document and delivered its events.
/// Once delivered, further non-terminating `xmlParseChunk` calls only buffer
/// (the remaining input can only be epilog), and the terminating call runs a
/// silent probe so nothing is delivered twice.
#[derive(Default)]
struct PushState {
    /// A non-final call parsed the accumulated input to a clean document end
    /// and the completing parse delivered all events.
    completed: bool,
    /// `ctxt->wellFormed` captured when the incremental parse started, before
    /// any probe mutates the context. Consumers may pre-zero it — PHP
    /// ext/xml's expat-compat layer sets `wellFormed = 0` right after creating
    /// the push context — and every probe/delivery/final re-parse must observe
    /// the same starting state, so `parse_chunk` restores it before each
    /// `XmlParser` construction (SP-14.3.1-4: the reference-substitution gate
    /// mirrors upstream's `if (!ctxt->wellFormed) return;`, which holds for the
    /// whole document when the consumer disabled well-formed tracking).
    start_well_formed: i32,
    /// Whether `start_well_formed` was captured yet.
    captured: bool,
    /// Byte offset of the accumulated input whose SAX events were already
    /// delivered by eager-partial parses on earlier non-final calls
    /// (SP-14.3.1-6). Later parses suppress the events at or below this
    /// offset and deliver only the new tail; 0 = nothing delivered yet.
    delivered_bytes: usize,
    /// Whether THIS parser created an internal SAX-compat registry document
    /// (XML_DOC_INTERNAL) on `ctxt->myDoc` (state.rs
    /// `ensure_entity_registry_dtd`). Only such docs are reclaimed by
    /// `free_parser_ctxt` — caller-owned documents are never dereferenced or
    /// freed there (upstream xmlFreeParserCtxt ignores myDoc entirely).
    internal_doc_created: bool,
}

static PUSH_STATE: once_cell::sync::Lazy<parking_lot::Mutex<HashMap<usize, PushState>>> =
    once_cell::sync::Lazy::new(Default::default);

fn push_state(ctxt: *mut _xmlParserCtxt) -> parking_lot::MappedMutexGuard<'static, PushState> {
    parking_lot::MutexGuard::map(PUSH_STATE.lock(), |m| {
        m.entry(ctxt as usize).or_insert_with(PushState::default)
    })
}

/// Record that the parser created the internal SAX-compat registry document
/// (XML_DOC_INTERNAL) on `ctxt->myDoc`, so `free_parser_ctxt` can reclaim it
/// without dereferencing a possibly caller-freed pointer.
pub(crate) fn mark_internal_doc(ctxt: *mut _xmlParserCtxt) {
    push_state(ctxt).internal_doc_created = true;
}

/// Re-apply the `wellFormed` value the context had when the incremental parse
/// started (see `PushState::start_well_formed`). Each probe/delivery/final
/// re-parse must observe the same starting state: PHP's expat-compat layer
/// zeroes `wellFormed` at create, and the engine mirrors upstream's
/// `if (!ctxt->wellFormed) return;` reference guard for such contexts
/// (SP-14.3.1-4).
fn restore_start_well_formed(ctxt: *mut _xmlParserCtxt) {
    let guard = PUSH_STATE.lock();
    if let Some(st) = guard.get(&(ctxt as usize)) {
        if st.captured {
            unsafe {
                (*ctxt).wellFormed = st.start_well_formed;
            }
        }
    }
}

/// Drop the incremental-push state for `ctxt`, if any.
pub(crate) fn free_push_state(ctxt: *mut _xmlParserCtxt) {
    PUSH_STATE.lock().remove(&(ctxt as usize));
}

/// Stash the boxed input buffer for `ctxt` (takes ownership of `buf`).
pub(crate) fn stash_input_buffer(ctxt: *mut _xmlParserCtxt, buf: *mut InputBuffer) {
    PARSER_INPUT_STASH
        .lock()
        .insert(ctxt as usize, StashPtr(buf));
}

/// Take (remove) the stashed input buffer for `ctxt`; the caller owns it.
pub(crate) fn take_stashed_input_buffer(ctxt: *mut _xmlParserCtxt) -> *mut InputBuffer {
    PARSER_INPUT_STASH
        .lock()
        .remove(&(ctxt as usize))
        .map_or(ptr::null_mut(), |s| s.0)
}

/// Apply a whole-buffer encoding override to the still-stashed memory input
/// of `ctxt` (upstream `xmlSwitchToEncoding` against a memory parser
/// context whose `input->buf` is NULL — the PHP `overrideEncoding` flow
/// switches between `xmlCreateMemoryParserCtxt` and `xmlParseDocument`).
///
/// Transcodes the buffered bytes with the named converter, resets the
/// position, and repopulates `ctxt->input` so the parse reads the new UTF-8
/// allocation (the old Vec is dropped by the `InputBuffer` itself).
///
/// Returns 0 on success, -1 when no stashed memory input exists or the
/// override cannot be applied.
///
/// # Safety
///
/// - `ctxt` must be a valid `_xmlParserCtxt` set up via `setup_parser_input`
///   whose parse has not started; `name` must be a valid NUL-terminated
///   string.
pub(crate) unsafe fn apply_memory_encoding_override(
    ctxt: *mut _xmlParserCtxt,
    name: *const c_char,
) -> c_int {
    if ctxt.is_null() || name.is_null() {
        return -1;
    }
    let ib_ptr = PARSER_INPUT_STASH
        .lock()
        .get(&(ctxt as usize))
        .map_or(ptr::null_mut(), |s| s.0);
    if ib_ptr.is_null() {
        return -1;
    }
    let name_bytes = unsafe { CStr::from_ptr(name).to_bytes() };
    unsafe {
        let ib = &mut *ib_ptr;
        if !ib.apply_name_encoding_override(name_bytes) {
            return -1;
        }
        // The conversion replaced the data Vec; point ctxt->input at the new
        // allocation from the stream start.
        let pi = (*ctxt).input;
        if pi.is_null() {
            return -1;
        }
        ib.populate_parser_input_without_filename(&mut *pi);
    }
    0
}

/// Drop the stashed input buffer for `ctxt`, if any.
pub(crate) fn free_stashed_input_buffer(ctxt: *mut _xmlParserCtxt) {
    if let Some(buf) = PARSER_INPUT_STASH.lock().remove(&(ctxt as usize)) {
        // SAFETY: the pointer was stashed via Box::into_raw.
        unsafe { drop(Box::from_raw(buf.0)) };
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Context creation / destruction
// ═══════════════════════════════════════════════════════════════════════════════

/// Create a new parser context with a default SAX2 handler.
///
/// Returns a pointer to a zero-initialised `_xmlParserCtxt` with:
///
/// - `sax` — a newly allocated `_xmlSAXHandler` initialised via
///   `xmlSAX2InitDefaultSAXHandler`
/// - `userData` — set to `ctxt` itself (so SAX callbacks can recover the context)
/// - `wellFormed` — `1` (the document starts well-formed)
/// - `instate` — `0` (`XML_PARSER_START`)
/// - `keepBlanks` — the deprecated `xmlKeepBlanksDefaultValue` (fresh contexts
///   snapshot it exactly like upstream `xmlInitParserCtxt`, so
///   `xmlKeepBlanksDefault(0)` suppresses whitespace-only text nodes)
/// - `replaceEntities` — the deprecated `xmlSubstituteEntitiesDefaultValue`
/// - `linenumbers` — `1` (track line numbers)
///
/// # Safety
///
/// The caller must eventually free the returned context with `free_parser_ctxt`.
/// The returned pointer may be null if allocation fails.
#[allow(non_snake_case)]
pub(crate) unsafe fn create_parser_ctxt() -> *mut _xmlParserCtxt {
    // SAFETY: xmlMallocZero returns zero-initialised memory or NULL.
    let ctxt = unsafe { xmlMallocZero(size_of::<_xmlParserCtxt>()) } as *mut _xmlParserCtxt;
    if ctxt.is_null() {
        return ptr::null_mut();
    }

    // Allocate and initialise the default SAX2 handler.
    // SAFETY: xmlMallocZero returns a valid pointer or NULL.
    let sax = unsafe { xmlMallocZero(size_of::<_xmlSAXHandler>()) } as *mut _xmlSAXHandler;
    if sax.is_null() {
        // Free the context since the SAX allocation failed.
        // SAFETY: ctxt was just allocated above and is non-null.
        unsafe { xmlFreeImpl(ctxt as *mut c_void) };
        return ptr::null_mut();
    }

    // SAFETY: sax is non-null and points to zero-initialised memory.
    unsafe { xmlSAX2InitDefaultSAXHandler(sax) };

    unsafe {
        let c = &mut *ctxt;
        c.sax = sax;
        c.userData = ctxt as *mut c_void;
        c.wellFormed = 1;
        c.instate = 0; // XML_PARSER_START
                       // UPSTREAM-PARITY (parserInternals.c xmlInitParserCtxt): a fresh
                       // context snapshots the deprecated per-thread defaults. keepBlanks
                       // is load-bearing: the executed 2.15.3 oracle never re-raises it once
                       // seeded (only XML_PARSE_NOBLANKS lowers it), so
                       // `xmlKeepBlanksDefault(0)` governs whitespace-only text nodes in
                       // fresh-context reads even when the read options omit NOBLANKS
                       // (R-000177 three-DSO reads resolve the process-visible cell).
        c.keepBlanks = crate::xml::globals::get_keep_blanks_default();
        c.replaceEntities = crate::xml::globals::get_substitute_entities_default();
        c.linenumbers = 1;
        // UPSTREAM-PARITY (parser.c xmlInitParserCtxt): a fresh context is
        // valid and namespace-well-formed until a failure says otherwise;
        // endDocument mirrors these into the document properties.
        c.valid = 1;
        c.nsWellFormed = 1;
        // UPSTREAM-PARITY: the standalone flag is tri-state: -1 unknown/unset,
        // 0 "no", 1 "yes" (xmlNewParserCtxt initialises it to -1).
        c.standalone = -1;
        c.errNo = crate::abi::types::XML_ERR_OK;
        c.options = 0;
    }

    ctxt
}

/// Free a parser context and all associated resources.
///
/// This frees:
///
/// - The SAX handler (if `sax` is non-null)
/// - All input buffers in `inputTab`
/// - The `inputTab` array itself
/// - The node stack (`nodeTab`)
/// - The name stack (`nameTab`)
/// - The stored `InputBuffer` (from `_private`)
/// - The context struct itself
///
/// # Safety
///
/// `ctxt` must be a valid pointer returned by `create_parser_ctxt` (or NULL,
/// in which case this function is a no-op).
#[allow(non_snake_case)]
pub(crate) unsafe fn free_parser_ctxt(ctxt: *mut _xmlParserCtxt) {
    if ctxt.is_null() {
        return;
    }

    unsafe {
        // Free SAX handler (upstream parserInternals.c xmlFreeParserCtxt): a
        // handler that IS one of the exported STATIC defaults — the XML
        // xmlDefaultSAXHandler or the htmlDefaultSAXHandler global selected by
        // html contexts created with a NULL sax — is not heap-owned and must
        // never be freed here (only heap handler structs are reclaimed).
        let sax = (*ctxt).sax;
        if !sax.is_null() {
            let statics = [
                core::ptr::addr_of!(crate::abi::data_globals::xmlDefaultSAXHandler)
                    as *const c_void,
                core::ptr::addr_of!(crate::abi::data_globals::htmlDefaultSAXHandler)
                    as *const c_void,
            ];
            let is_static = statics.iter().any(|&p| p == sax as *const c_void);
            if !is_static {
                xmlFreeImpl(sax as *mut c_void);
            }
        }

        // Free all inputs in the input stack.
        let input_nr = (*ctxt).inputNr;
        let input_tab = (*ctxt).inputTab;
        if !input_tab.is_null() {
            for i in 0..input_nr {
                let input = *input_tab.add(i as usize);
                if !input.is_null() {
                    free_parser_input(input);
                }
            }
            xmlFreeImpl(input_tab as *mut c_void);
        }

        // Free the current input (if not already in inputTab).
        let cur_input = (*ctxt).input;
        if !cur_input.is_null() {
            // If inputTab was set up, the current input is already in the tab
            // and was freed above. We only free it here if inputTab was NULL.
            if input_tab.is_null() {
                free_parser_input(cur_input);
            }
        }

        // Free the node stack (the array itself; nodes are owned by the doc).
        let node_tab = (*ctxt).nodeTab;
        if !node_tab.is_null() {
            xmlFreeImpl(node_tab as *mut c_void);
        }

        // Free the name stack.
        let name_tab = (*ctxt).nameTab;
        if !name_tab.is_null() {
            xmlFreeImpl(name_tab as *mut c_void);
        }

        // Free the stored InputBuffer (stashed in the side table by
        // setup_parser_input). `ctxt._private` is application data and is
        // NEVER touched here (11.1-X).
        free_stashed_input_buffer(ctxt);

        // UPSTREAM-PARITY (parser SAX-compat entity registry): a document the
        // parser created internally (XML_DOC_INTERNAL — see
        // state.rs::ensure_entity_registry_dtd) is never delivered to the
        // caller, so it must be reclaimed here. Ownership is tracked by a flag
        // (set at creation) instead of dereferencing `ctxt->myDoc`, which the
        // caller may already have freed — upstream xmlFreeParserCtxt never
        // touches myDoc, and caller-owned documents are never reclaimed here.
        let reclaim_internal_doc = push_state(ctxt).internal_doc_created;
        // Drop incremental-push state so a later context allocated at the
        // same address starts clean (SP-14.3.1-3).
        free_push_state(ctxt);
        if reclaim_internal_doc {
            let my_doc = (*ctxt).myDoc;
            if !my_doc.is_null() {
                (*ctxt).myDoc = ptr::null_mut();
                crate::xml::tree::free_doc(my_doc);
            }
        }

        // Free the parser dictionary (upstream xmlFreeParserCtxt).
        if !(*ctxt).dict.is_null() {
            crate::abi::exports_xml2::xmlDictFree((*ctxt).dict);
        }

        // Free the per-context last error's owned strings (the error paths
        // now fill ctxt->lastError with strdup'd copies).
        crate::xml::globals::free_error_strings(&(*ctxt).lastError);

        // UPSTREAM-PARITY (parser SAX-compat entity registry): a document the
        // parser created internally (SAX_COMPAT_MODE docs kept so expat-style
        // SAX consumers can resolve NOENT general entities — see
        // state.rs::ensure_entity_registry_dtd) is never delivered to the
        // caller, so it must be reclaimed here. Normal caller-owned documents
        // are NOT `XML_DOC_INTERNAL` and are detached by the front-ends before
        // the context is freed, so they are untouched.
        let my_doc = (*ctxt).myDoc;
        if !my_doc.is_null()
            && ((*my_doc).properties
                & (crate::abi::types::xmlDocProperties::XML_DOC_INTERNAL as c_int))
                != 0
        {
            (*ctxt).myDoc = ptr::null_mut();
            crate::xml::tree::free_doc(my_doc);
        }

        // Free the context itself.
        xmlFreeImpl(ctxt as *mut c_void);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Input buffer creation helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Create an `InputBuffer` from a raw memory buffer.
///
/// The bytes are copied into an owned buffer, so the caller may free `buffer`
/// after calling this function.
///
/// If `size` is negative or `buffer` is null, an empty `InputBuffer` is returned.
///
/// # Safety
///
/// - `buffer` must be valid for reads of at least `size` bytes, or NULL.
/// - If `size` is negative, `buffer` is not dereferenced.
pub(crate) unsafe fn input_from_memory(buffer: *const c_char, size: c_int) -> InputBuffer {
    let slice = if size > 0 && !buffer.is_null() {
        // SAFETY: Caller guarantees `buffer` points to at least `size` readable bytes.
        unsafe { std::slice::from_raw_parts(buffer as *const u8, size as usize) }
    } else {
        &[]
    };
    InputBuffer::from_memory(slice, None)
}

/// Create an `InputBuffer` from memory with a source URI recorded as the
/// input's filename (upstream `xmlCtxtReadMemory` sets `input->filename` from
/// the URL, which feeds the `file:line:` error prefix). Non-UTF-8 URIs are
/// dropped (documented divergence — the candidate's input layer stores
/// filenames as UTF-8).
///
/// # Safety
///
/// - `buffer` must be valid for reads of at least `size` bytes, or NULL.
/// - `uri` must be a valid NUL-terminated C string, or NULL.
pub(crate) unsafe fn input_from_memory_named(
    buffer: *const c_char,
    size: c_int,
    uri: *const c_char,
) -> InputBuffer {
    let slice = if size > 0 && !buffer.is_null() {
        // SAFETY: Caller guarantees `buffer` points to at least `size` readable bytes.
        unsafe { std::slice::from_raw_parts(buffer as *const u8, size as usize) }
    } else {
        &[]
    };
    let uri_str = if !uri.is_null() {
        // SAFETY: Caller guarantees `uri` is a valid C string.
        unsafe { std::ffi::CStr::from_ptr(uri) }
            .to_str()
            .ok()
            .map(|s| s.to_string())
    } else {
        None
    };
    InputBuffer::from_memory(slice, uri_str.as_deref())
}

/// Create an `InputBuffer` from a file path.
///
/// Returns `Ok(InputBuffer)` on success, or `Err(())` if the file cannot be
/// opened, read, or the path is not valid UTF-8.
///
/// # Safety
///
/// `filename` must be a valid null-terminated C string, or NULL (in which case
/// `Err(())` is returned).
pub(crate) unsafe fn input_from_file(filename: *const c_char) -> Result<InputBuffer, ()> {
    if filename.is_null() {
        return Err(());
    }

    // SAFETY: Caller guarantees `filename` is a valid null-terminated C string.
    let path = unsafe { CStr::from_ptr(filename) };
    let path_str = path.to_str().map_err(|_| ())?;
    // UPSTREAM-PARITY (xmlIO.c xmlFileOpen / xmlParserInputBufferCreateFilename):
    // a file:// URI opens through the plain-file path; strip the scheme and
    // any authority component so the OS open sees a local path.
    let path_str = if let Some(rest) = path_str.strip_prefix("file://") {
        match rest.find('/') {
            Some(idx) => &rest[idx..],
            None => "/",
        }
    } else if let Some(rest) = path_str.strip_prefix("file:") {
        rest
    } else {
        path_str
    };
    InputBuffer::from_file(path_str).map_err(|_| ())
}

/// No-op input close callback: upstream `xmlReaderForIO` accepts a NULL
/// close callback (xmlIO.c xmlNewIOInputStream), so `input_from_io` cannot
/// require one.
const unsafe extern "C" fn noop_input_close(_ctx: *mut c_void) -> c_int {
    0
}

/// Create an `InputBuffer` from I/O callbacks.
///
/// The callbacks are used to read all available data from the source.
/// If reading fails, an empty `InputBuffer` is returned (the error is silently
/// swallowed, matching libxml2's behaviour in some code paths).
///
/// # Safety
///
/// - `ioread` must be a valid function pointer or `None`.
/// - `ioclose` must be a valid function pointer or `None`.
/// - If callbacks are provided, `ioctx` must be a valid context pointer for them.
pub(crate) unsafe fn input_from_io(
    ioread: Option<xmlInputReadCallback>,
    ioclose: Option<xmlInputCloseCallback>,
    ioctx: *mut c_void,
) -> InputBuffer {
    let (read, close) = match ioread {
        Some(r) => (
            r,
            ioclose.unwrap_or(noop_input_close as xmlInputCloseCallback),
        ),
        None => return InputBuffer::from_memory(&[], None),
    };

    // SAFETY: The callbacks are used immediately to read all data. The caller
    // guarantees the function pointers and context are valid for the duration
    // of this call.
    match InputBuffer::from_callback(read, close, ioctx) {
        Ok(buf) => buf,
        Err(_) => {
            // UPSTREAM-PARITY (xmlIO.c/xmlParserInputBufferGrow): a read
            // callback that reports an error makes the parser raise an I/O
            // error on the first grow — NOT an empty-document parse
            // (HOSTILE-CALLBACKS C4).
            InputBuffer::failed_source()
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Context ↔ input wiring
// ═══════════════════════════════════════════════════════════════════════════════

/// Set up the parser context with an input buffer.
///
/// This function:
///
/// 1. Boxes the `InputBuffer` and stores the raw pointer in `ctxt._private`
///    (keeping the data alive for the lifetime of the context).
/// 2. Allocates a `_xmlParserInput` pointing into the buffer's data.
/// 3. Sets `ctxt.input` to the new input.
/// 4. Allocates `inputTab` (initial capacity 4) and pushes the input onto it.
///
/// # Safety
///
/// - `ctxt` must be a valid, writable pointer to a `_xmlParserCtxt`.
/// - After this call, the context owns the `InputBuffer` (via `_private`).
/// - The caller must not use the `InputBuffer` directly afterwards.
#[allow(non_snake_case)]
pub(crate) unsafe fn setup_parser_input(ctxt: *mut _xmlParserCtxt, input: InputBuffer) {
    // Box the InputBuffer and leak it so _xmlParserInput pointers stay valid.
    // SAFETY: Box::into_raw gives us a raw pointer that we later reconstruct
    // in free_parser_ctxt or consume in parse_document/parse_chunk.
    let input_buf_ptr = Box::into_raw(Box::new(input));

    unsafe {
        let c = &mut *ctxt;

        // Store the leaked pointer in the side table so we can free it later
        // (ctxt._private stays application data — 11.1-X).
        stash_input_buffer(ctxt, input_buf_ptr);

        // SAFETY: input_buf_ptr points to a live InputBuffer whose data Vec
        // will not move while the _xmlParserInput references it.
        let parser_input = alloc_parser_input(&*input_buf_ptr, None);
        c.input = parser_input;

        // Allocate inputTab with initial capacity of 4 pointers.
        let tab_size = 4 * size_of::<*mut _xmlParserInput>();
        // SAFETY: xmlMalloc returns uninitialised memory or NULL.
        let tab = xmlMallocImpl(tab_size) as *mut *mut _xmlParserInput;
        if tab.is_null() {
            // Allocation failure — leave inputTab null, inputNr 0.
            c.inputTab = ptr::null_mut();
            c.inputMax = 0;
            c.inputNr = 0;
            return;
        }
        // Zero-initialise the table.
        ptr::write_bytes(tab, 0, 4);

        *tab = parser_input;
        c.inputTab = tab;
        c.inputMax = 4;
        c.inputNr = 1;
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Parsing entry points
// ═══════════════════════════════════════════════════════════════════════════════

/// Parse a complete document using the internal parser.
///
/// Takes ownership of the `InputBuffer` stashed for `ctxt` (from a
/// previous `setup_parser_input` call), creates an `InputStack` and
/// `XmlParser`, and runs `parse_document`.
///
/// Returns `0` on success, `-1` on error.
///
/// # Safety
///
/// - `ctxt` must be a valid pointer to a `_xmlParserCtxt` that was set up via
///   `setup_parser_input` (or equivalent).
/// - After this call, the `InputBuffer` is consumed and the stash entry is
///   removed. The context's `input` and `inputTab` may contain dangling
///   pointers and should not be used for further parsing.
pub(crate) unsafe fn parse_document(ctxt: *mut _xmlParserCtxt) -> c_int {
    // Take ownership of the stashed InputBuffer.
    // SAFETY: The pointer was stashed by setup_parser_input via Box::into_raw.
    let input_buf = {
        let ptr = take_stashed_input_buffer(ctxt);
        if ptr.is_null() {
            // No input buffer — nothing to parse.
            return -1;
        }
        // SAFETY: ptr is a valid Box<InputBuffer> from Box::into_raw.
        unsafe { Box::from_raw(ptr) }
    };

    // Move the InputBuffer into an InputStack.
    let input_stack = InputStack::new(*input_buf);
    // input_buf is consumed here.

    // SAFETY: ctxt is a valid, initialised parser context.
    let mut parser = unsafe { XmlParser::new(input_stack, ctxt) };
    parser.parse_document()
    // parser is dropped here, which drops the tokenizer and its InputStack,
    // which drops the InputBuffer.
}

/// Parse a chunk of input (push parser mode).
///
/// Each call processes the given chunk as part of the input stream. If
/// `terminate` is non-zero, the document is finalised and fully parsed.
///
/// Returns `0` on success (or if more data is expected), `-1` on error.
///
/// # Upstream contract (SP-14.3.1-3)
///
/// Upstream `xmlParseChunk` parses each chunk eagerly and fires events as
/// data becomes available; a non-terminating call on a COMPLETE document
/// therefore delivers everything, which consumers such as PHP ext/xml's
/// expat-compat layer rely on (`xml_parse()` defaults `isFinal = false` —
/// bug25666/xml009/xml010). The candidate accumulates the input and parses
/// it as a whole. To keep both models: on every non-terminating call the
/// accumulated input is probed with a silent parse (no handler delivery, no
/// diagnostics — SP-14.3.1-3); when the probe reaches a clean document end
/// the buffer is parsed again with full delivery exactly once, and later
/// calls (epilog only) merely buffer until the terminating call, which runs
/// a silent probe plus, if the document is no longer clean (trailing junk),
/// a diagnostics-only pass so late errors still surface.
///
/// # Safety
///
/// - `ctxt` must be a valid pointer to a `_xmlParserCtxt`.
/// - `chunk` must be a valid pointer to at least `size` readable bytes, or NULL
///   (in which case `size` should be 0).
pub(crate) unsafe fn parse_chunk(
    ctxt: *mut _xmlParserCtxt,
    chunk: *const c_char,
    size: c_int,
    terminate: c_int,
) -> c_int {
    // Build a byte slice from the chunk.
    let chunk_slice = if size > 0 && !chunk.is_null() {
        // SAFETY: Caller guarantees the chunk pointer is valid for `size` bytes.
        unsafe { std::slice::from_raw_parts(chunk as *const u8, size as usize) }
    } else {
        &[]
    };

    // UPSTREAM-PARITY (parser.c xmlParseChunk): a context whose parse was
    // stopped by xmlStopParser (disableSAX == 2) refuses further chunks and
    // reports the recorded error (SP-14.3.1-4, bug71592: PHP's expat-compat
    // external-entity-ref handler returns FALSE → xmlStopParser + errNo =
    // XML_ERROR_EXTERNAL_ENTITY_HANDLING; every later xmlParseChunk must
    // return that error instead of parsing the remainder). disableSAX == 1 is
    // the candidate's internal "a fatal was reported" marker and does not
    // stop chunk delivery.
    if unsafe { (*ctxt).disableSAX } == 2 {
        return unsafe { (*ctxt).errNo };
    }

    // UPSTREAM-PARITY (parser.c xmlParseTryOrFinish `case XML_PARSER_EOF`):
    // a context that finished a complete document stays at XML_PARSER_EOF,
    // so every later xmlParseChunk parses nothing and reports the previous
    // outcome (0 when well-formed). gh12254 calls xml_parse_into_struct twice
    // on the same parser; the second call must not fire the element events
    // again (SP-14.3.1-7). Incomplete parses never set instate = EOF, so the
    // multi-call incremental flows are unaffected.
    if unsafe { (*ctxt).instate } == crate::abi::types::xmlParserInputState::XML_PARSER_EOF as c_int
    {
        if unsafe { (*ctxt).wellFormed } == 0 {
            return unsafe { (*ctxt).errNo };
        }
        return 0;
    }

    // Take ownership of the stashed InputBuffer (the base accumulated so
    // far — the constructor's initial chunk is stashed by
    // setup_parser_input), or start empty.
    let mut base: InputBuffer = {
        let ptr = take_stashed_input_buffer(ctxt);
        if ptr.is_null() {
            InputBuffer::from_memory(&[], None)
        } else {
            // SAFETY: ptr is a valid Box<InputBuffer> from Box::into_raw.
            unsafe { *Box::from_raw(ptr) }
        }
    };

    // Append the chunk to the accumulated input (upstream xmlParseChunk
    // grows ctxt->input's base with each chunk; the candidate parses the
    // whole accumulated stream).
    base.push_bytes(chunk_slice);

    // Capture the consumer-set wellFormed BEFORE the first parse mutates it
    // (PHP expat-compat zeroes it at create; see PushState docs). Restored
    // before every engine construction below.
    {
        let mut st = push_state(ctxt);
        if !st.captured {
            st.captured = true;
            st.start_well_formed = unsafe { (*ctxt).wellFormed };
        }
    }

    if terminate == 0 {
        // Non-final call. Upstream parses each chunk eagerly: events fire as
        // soon as their construct completed, even when the document is not
        // finished (SP-14.3.1-6 — the XML_OPTION_PARSE_HUGE multi-call flow
        // delivers CONTAINER/A/A/SECOND on the first call and only the
        // container's end on the final call). The candidate re-parses the
        // whole accumulated input, so each call probes silently first and
        // then runs a delivery parse that suppresses the events at or below
        // `delivered_bytes` (already fired by an earlier partial parse).
        let completed = push_state(ctxt).completed;
        if !completed {
            // Silent completeness probe: tells us whether the accumulated
            // input forms a complete document (clean end or a definitive
            // failure on a complete token), paused at a clean construct
            // boundary (more data may arrive), or is truncated mid-construct.
            let probe_buf = base.duplicate_for_reparse();
            let input_stack = InputStack::new(probe_buf);
            // SAFETY: ctxt is a valid, initialised parser context.
            restore_start_well_formed(ctxt);
            let mut probe = unsafe { XmlParser::new_with_mode(input_stack, ctxt, true) };
            let rc = probe.parse_document();
            let paused = probe.is_paused();
            let truncated = probe.was_truncated_abort();
            if rc == 0 || (rc != 0 && !paused && !truncated) {
                // The accumulated input parsed through to its end: either a
                // clean document end or a failure on a COMPLETE token at the
                // end of the input (e.g. an end-tag mismatch closing the
                // root). Deliver everything not yet delivered, exactly once
                // (PHP xml_parse defaults isFinal=false — bug25666/xml009).
                let delivered = push_state(ctxt).delivered_bytes;
                push_state(ctxt).completed = true;
                let delivery_buf = base.duplicate_for_reparse();
                let input_stack = InputStack::new(delivery_buf);
                // SAFETY: ctxt is a valid, initialised parser context.
                restore_start_well_formed(ctxt);
                let mut parser =
                    unsafe { XmlParser::new_with_resume(input_stack, ctxt, delivered) };
                parser.parse_document();
                // UPSTREAM-PARITY (parser.c xmlParseChunk): the delivery
                // parse's outcome is reported on the non-final call — the
                // recorded error code once the document is no longer
                // well-formed (a fatal error, or a stop from an
                // entity-resolving SAX handler that leaves errNo set and
                // wellFormed = 0), 0 otherwise. PHP's expat-compat XML_Parse
                // maps a non-zero return to FALSE, so xml_parse() returns
                // FALSE for bug71592 exactly like the oracle.
                let failed = unsafe { (*ctxt).wellFormed } == 0;
                let err = unsafe { (*ctxt).errNo };
                // More data may still arrive: stash the accumulated buffer
                // for the next xmlParseChunk call (upstream keeps the data in
                // ctxt->input even after a failed non-final call).
                stash_input_buffer(ctxt, Box::into_raw(Box::new(base)));
                return if failed { err } else { 0 };
            } else if paused && !truncated {
                // Incomplete document whose constructs up to the end of the
                // available input are all complete: deliver them eagerly,
                // exactly like upstream's per-chunk parsing, and record how
                // far the delivery got so later calls only fire the new tail.
                let delivered = push_state(ctxt).delivered_bytes;
                let delivery_buf = base.duplicate_for_reparse();
                let input_stack = InputStack::new(delivery_buf);
                // SAFETY: ctxt is a valid, initialised parser context.
                restore_start_well_formed(ctxt);
                let mut parser =
                    unsafe { XmlParser::new_with_partial_resume(input_stack, ctxt, delivered) };
                parser.parse_document(); // pauses at the end of the input
                push_state(ctxt).delivered_bytes = base.len();
                stash_input_buffer(ctxt, Box::into_raw(Box::new(base)));
                return 0;
            }
            // Truncated mid-construct: nothing beyond `delivered_bytes` is
            // deliverable yet — buffer until the construct completes.
        }
        // Incomplete document: buffer until the terminating call.
        stash_input_buffer(ctxt, Box::into_raw(Box::new(base)));
        0
    } else if push_state(ctxt).completed {
        // Final call after an early delivery. Never deliver twice: probe
        // silently; if the accumulated input no longer parses cleanly
        // (trailing junk in the epilog), re-run with diagnostics enabled but
        // SAX delivery suppressed so the errors surface exactly once.
        let probe_buf = base.duplicate_for_reparse();
        let input_stack = InputStack::new(probe_buf);
        // SAFETY: ctxt is a valid, initialised parser context.
        restore_start_well_formed(ctxt);
        let mut probe = unsafe { XmlParser::new_with_mode(input_stack, ctxt, true) };
        let rc = probe.parse_document();
        if unsafe { (*ctxt).wellFormed } == 0 || rc != 0 {
            // The tail broke the document: surface the real diagnostics
            // (the completing parse already delivered the clean prefix).
            let diag_buf = base.duplicate_for_reparse();
            let input_stack = InputStack::new(diag_buf);
            // SAFETY: ctxt is a valid, initialised parser context.
            restore_start_well_formed(ctxt);
            let mut parser = unsafe { XmlParser::new_with_flags(input_stack, ctxt, false, true) };
            parser.parse_document();
            free_push_state(ctxt);
            // UPSTREAM-PARITY (parser.c xmlParseChunk): the terminating call
            // reports the recorded error once the document is not
            // well-formed; a clean tail reports 0.
            if unsafe { (*ctxt).wellFormed } == 0 {
                return unsafe { (*ctxt).errNo };
            }
            return 0;
        }
        free_push_state(ctxt);
        0
    } else {
        // Plain final parse of everything accumulated so far. When earlier
        // non-final calls already delivered a prefix eagerly, continue from
        // the delivery boundary instead of re-firing it.
        let delivered = push_state(ctxt).delivered_bytes;
        let input_stack = InputStack::new(base);
        // SAFETY: ctxt is a valid, initialised parser context.
        restore_start_well_formed(ctxt);
        let mut parser = unsafe { XmlParser::new_with_resume(input_stack, ctxt, delivered) };
        let rc = parser.parse_document();
        free_push_state(ctxt);
        // UPSTREAM-PARITY (parser.c xmlParseChunk): report the recorded
        // error code when the document ended not well-formed.
        if unsafe { (*ctxt).wellFormed } == 0 {
            return unsafe { (*ctxt).errNo };
        }
        rc
        // parser is dropped here.
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// C ABI struct allocation / deallocation
// ═══════════════════════════════════════════════════════════════════════════════

/// Allocate and initialise a C ABI `_xmlParserInput` from an `InputBuffer`.
///
/// The returned `_xmlParserInput` contains raw pointers (`base`, `cur`, `end`)
/// that point into the `InputBuffer`'s internal data storage. The caller must
/// ensure the `InputBuffer` outlives the returned struct.
///
/// If `filename` is `Some`, it is used as the input's filename; otherwise the
/// filename from the `InputBuffer` is used.
///
/// # Safety
///
/// - `input` must remain alive and unmoved for the lifetime of the returned
///   `_xmlParserInput`.
/// - The returned pointer must be freed with `free_parser_input` or `xmlFree`.
pub(crate) unsafe fn alloc_parser_input(
    input: &InputBuffer,
    filename: Option<&str>,
) -> *mut _xmlParserInput {
    // SAFETY: xmlMallocZero returns zero-initialised memory or NULL.
    let ptr = unsafe { xmlMallocZero(size_of::<_xmlParserInput>()) } as *mut _xmlParserInput;
    if ptr.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        let pi = &mut *ptr;

        // Use the InputBuffer's populate method to fill in the core fields
        // (base, cur, end, line, col, length, consumed). The Rust-side
        // filename is NOT borrowed here: `populate_parser_input` would point
        // at a Rust String that the parser later moves/drops (a dangling C
        // pointer — the observed heap-reuse garbage). Instead the filename is
        // duplicated into memory owned by the _xmlParserInput itself
        // (upstream keeps the filename on the input struct and frees it with
        // xmlFreeInputStream).
        input.populate_parser_input_without_filename(pi);

        // Own a C copy of the filename: the explicit override wins, else the
        // buffer's own filename. NOTE: `xml_strndup` (not `xml_strdup`) is
        // used because the source is a Rust `String` whose `as_ptr()` is NOT
        // NUL-terminated — `xml_strdup` would scan past the end of the
        // allocation (heap-buffer-overflow) and copy garbage, which the
        // TREE-001 probe observed as `URL=t.xml<V>`. The explicit length
        // makes the copy exact and NUL-terminates it.
        let owned = filename
            .map(|s| s.to_string())
            .or_else(|| input.filename().map(|s| s.to_string()));
        if let Some(fname) = owned {
            pi.filename = crate::xml::string::xml_strndup(
                fname.as_ptr() as *const crate::abi::types::xmlChar,
                fname.len(),
            ) as *const c_char;
        }

        // Set remaining fields that populate_parser_input does not touch.
        pi.buf = ptr::null_mut();
        pi.directory = ptr::null();
        pi.free = None;
        pi.encoding = ptr::null();
        pi.version = ptr::null();
        pi.flags = 0;
        pi.id = 0;
        pi.parentConsumed = 0;
        pi.entity = ptr::null_mut();
    }

    ptr
}

/// Allocate and initialise a C ABI `_xmlParserInputBuffer`.
///
/// The returned buffer is zero-initialised with all fields set to NULL/0.
///
/// # Safety
///
/// The returned pointer must be freed with `free_parser_input_buffer` or
/// `xmlFree` when no longer needed.
pub(crate) unsafe fn alloc_parser_input_buffer() -> *mut _xmlParserInputBuffer {
    // SAFETY: xmlMallocZero returns zero-initialised memory or NULL.
    unsafe { xmlMallocZero(size_of::<_xmlParserInputBuffer>()) as *mut _xmlParserInputBuffer }
}

/// Owned content of `_xmlParserInputBuffer`s built by
/// `xmlParserInputBufferCreateMem`. Upstream (xmlIO.c) copies the memory
/// into the buffer's internal `buffer` xmlBuf and leaves `readcallback`
/// NULL — the parser pulls bytes from the xmlBuf. The candidate's
/// `_xmlParserInputBuffer` is a field shim (no live xmlBuf object), so the
/// bytes live here, keyed by the buffer address, and are consumed by the
/// text-reader setup (`reader_from_input`) when no read callback is set.
static PARSER_INPUT_BUF_CONTENT_STASH: once_cell::sync::Lazy<
    parking_lot::Mutex<HashMap<usize, Vec<u8>>>,
> = once_cell::sync::Lazy::new(Default::default);

/// Self-closed (`<a/>`) element nodes of the CURRENT reader parse, keyed by
/// document. The whole-tree XML reader rebuilds traversal events from the
/// parsed tree, but the tree cannot tell `<a/>` from `<a></a>` — upstream's
/// streaming reader knows from the SAX stream (an explicitly closed element
/// fires END_ELEMENT, a self-closed one does not). While
/// `ctxt->parseMode == XML_PARSE_READER`, the parser records every
/// self-closed element node here under its document; the reader's event
/// builder consumes the entries as it walks (reader/mod.rs build_events) and
/// drops a document's whole entry set when its parse fails, so stale markers
/// cannot linger across parses (keyed by doc — parses run on many threads).
static SELF_CLOSED_NODES: once_cell::sync::Lazy<
    parking_lot::Mutex<HashMap<usize, HashSet<usize>>>,
> = once_cell::sync::Lazy::new(Default::default);

/// Record `node` (in `doc`) as parsed from a self-closed `<a/>` start tag.
pub(crate) fn mark_self_closed(
    doc: *mut crate::abi::structs::_xmlDoc,
    node: *mut crate::abi::structs::_xmlNode,
) {
    if doc.is_null() || node.is_null() {
        return;
    }
    SELF_CLOSED_NODES
        .lock()
        .entry(doc as usize)
        .or_default()
        .insert(node as usize);
}

/// Whether `node` (in `doc`) was parsed from a self-closed `<a/>` start tag
/// (consumes the marker — the reader's event walk visits each element once).
pub(crate) fn take_self_closed(
    doc: *mut crate::abi::structs::_xmlDoc,
    node: *mut crate::abi::structs::_xmlNode,
) -> bool {
    if doc.is_null() || node.is_null() {
        return false;
    }
    SELF_CLOSED_NODES
        .lock()
        .get_mut(&(doc as usize))
        .is_some_and(|set| set.remove(&(node as usize)))
}

/// Drop every self-closed marker recorded for `doc` (failed reader parse).
pub(crate) fn drop_self_closed(doc: *mut crate::abi::structs::_xmlDoc) {
    if doc.is_null() {
        return;
    }
    SELF_CLOSED_NODES.lock().remove(&(doc as usize));
}

/// Stash the owned byte content of a memory parser input buffer.
pub(crate) fn stash_input_buf_content(buf: *mut _xmlParserInputBuffer, data: Vec<u8>) {
    PARSER_INPUT_BUF_CONTENT_STASH
        .lock()
        .insert(buf as usize, data);
}

/// Take (remove) the stashed byte content of a memory parser input buffer.
pub(crate) fn take_input_buf_content(buf: *mut _xmlParserInputBuffer) -> Option<Vec<u8>> {
    PARSER_INPUT_BUF_CONTENT_STASH
        .lock()
        .remove(&(buf as usize))
}

/// Allocate a memory parser input buffer holding a COPY of `buffer[..size]`
/// (upstream xmlIO.c xmlParserInputBufferCreateMem: content in the internal
/// buffer, `readcallback` NULL).
///
/// # Safety
///
/// - `buffer` must be valid for reads of at least `size` bytes.
pub(crate) unsafe fn alloc_parser_input_buffer_with_mem(
    buffer: *const c_char,
    size: c_int,
) -> *mut _xmlParserInputBuffer {
    // SAFETY: caller guarantees the slice is readable.
    let bytes = unsafe { core::slice::from_raw_parts(buffer as *const u8, size as usize) }.to_vec();
    let buf = unsafe { alloc_parser_input_buffer() };
    if !buf.is_null() {
        stash_input_buf_content(buf, bytes);
    }
    buf
}

/// Free a C ABI `_xmlParserInput`.
///
/// Mirrors upstream `xmlFreeInputStream` (parserInternals.c): the
/// deallocation callback, when set, takes full ownership of the input
/// (it frees the struct itself); otherwise the owned filename, directory
/// and buffer are freed before the struct.
///
/// # Safety
///
/// `input` must be a valid pointer returned by `alloc_parser_input`,
/// `xmlNewInputStream`, `xmlNewInputFrom*` or `xmlMalloc`, or NULL (in
/// which case this is a no-op).
pub(crate) unsafe fn free_parser_input(input: *mut _xmlParserInput) {
    if input.is_null() {
        return;
    }
    // SAFETY: The filename is an owned xmlMalloc'd copy made by
    // alloc_parser_input (upstream frees input->filename with the input).
    // The deallocation callback (upstream input->free) is invoked first and
    // owns the whole input when set.
    if let Some(free_cb) = unsafe { (*input).free } {
        // SAFETY: The callback contract matches upstream xmlFreeInputStream:
        // it frees the input (and its buffer) itself.
        unsafe { free_cb(input as *mut c_char) };
        return;
    }
    unsafe {
        if !(*input).filename.is_null() {
            crate::abi::allocator::xmlFreeImpl((*input).filename as *mut c_void);
        }
        if !(*input).directory.is_null() {
            crate::abi::allocator::xmlFreeImpl((*input).directory as *mut c_void);
        }
        if !(*input).buf.is_null() {
            // The input owns its buffer (xmlNewInputFrom* family and the
            // xmlLoadExternalEntity parser_input_from_buf path).
            crate::xml::io::input_buffer_free((*input).buf);
        }
        // SAFETY: The pointer was allocated via xmlMalloc (or xmlMallocZero).
        xmlFreeImpl(input as *mut c_void);
    }
}

/// Free a C ABI `_xmlParserInputBuffer`.
///
/// This frees only the struct itself — any resources referenced by its
/// callbacks or buffers must be managed separately.
///
/// # Safety
///
/// `buf` must be a valid pointer returned by `alloc_parser_input_buffer` or
/// `xmlMalloc`, or NULL (in which case this is a no-op).
pub(crate) unsafe fn free_parser_input_buffer(buf: *mut _xmlParserInputBuffer) {
    if buf.is_null() {
        return;
    }
    // Drop any stashed memory content (xmlParserInputBufferCreateMem).
    take_input_buf_content(buf);
    // SAFETY: The pointer was allocated via xmlMalloc (or xmlMallocZero).
    unsafe { xmlFreeImpl(buf as *mut c_void) };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Push-parser chunk accumulation: xmlParseChunk-style feeds accumulate
    /// into the stashed input and the terminating call parses the whole
    /// stream (parse4.c — Phase-12 EXTERNAL-CONSUMERS court).
    ///
    /// # Safety
    ///
    /// - `ctxt` is created and freed exactly once; `myDoc` is freed exactly
    ///   once.
    #[test]
    fn test_push_chunk_accumulates() {
        unsafe {
            let ctxt = create_parser_ctxt();
            assert!(!ctxt.is_null());
            // initial chunk (xmlCreatePushParserCtxt feeds the first bytes
            // through setup_parser_input)
            let c0 = b"<doc";
            let input = InputBuffer::from_memory(c0, None);
            setup_parser_input(ctxt, input);
            assert_eq!(parse_chunk(ctxt, c"/>\n".as_ptr(), 3, 0), 0);
            // terminating call with no new data parses the accumulated input
            assert_eq!(parse_chunk(ctxt, ptr::null(), 0, 1), 0);
            let doc = (*ctxt).myDoc;
            assert!(!doc.is_null());
            let root = (*doc).children;
            assert!(!root.is_null());
            assert_eq!(crate::xml::string::xmlstr_to_bytes((*root).name), b"doc");
            crate::xml::tree::free_doc(doc);
            free_parser_ctxt(ctxt);
        }
    }
}
