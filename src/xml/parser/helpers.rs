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
use std::collections::HashMap;
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
}

static PUSH_STATE: once_cell::sync::Lazy<parking_lot::Mutex<HashMap<usize, PushState>>> =
    once_cell::sync::Lazy::new(Default::default);

fn push_state(ctxt: *mut _xmlParserCtxt) -> parking_lot::MappedMutexGuard<'static, PushState> {
    parking_lot::MutexGuard::map(PUSH_STATE.lock(), |m| {
        m.entry(ctxt as usize).or_insert_with(PushState::default)
    })
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
/// - `keepBlanks` — `1` (preserve whitespace by default)
/// - `replaceEntities` — `0` (don't replace entities by default)
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
        c.keepBlanks = 1;
        c.replaceEntities = 0;
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
        // Free SAX handler.
        let sax = (*ctxt).sax;
        if !sax.is_null() {
            xmlFreeImpl(sax as *mut c_void);
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
        // Drop incremental-push state so a later context allocated at the
        // same address starts clean (SP-14.3.1-3).
        free_push_state(ctxt);

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
    InputBuffer::from_file(path_str).map_err(|_| ())
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
    // Both callbacks must be provided for I/O to work.
    let (read, close) = match (ioread, ioclose) {
        (Some(r), Some(c)) => (r, c),
        _ => return InputBuffer::from_memory(&[], None),
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
        // Non-final call. Once the document completed and delivered, later
        // calls can only bring epilog content; just buffer until the final
        // call (which re-validates silently).
        if !push_state(ctxt).completed {
            // Silent completeness probe. Parse the whole accumulated input
            // with delivery deferred: it must not invoke handlers (the
            // probe re-runs on every call and a partial document must not
            // leak half its events).
            let probe_buf = base.duplicate_for_reparse();
            let input_stack = InputStack::new(probe_buf);
            // SAFETY: ctxt is a valid, initialised parser context.
            restore_start_well_formed(ctxt);
            let mut probe = unsafe { XmlParser::new_with_mode(input_stack, ctxt, true) };
            let rc = probe.parse_document();
            if rc == 0 || (rc != 0 && !probe.is_paused() && !probe.was_truncated_abort()) {
                // The accumulated input parsed through to its end: either a
                // clean document end or a failure on a COMPLETE token at the
                // end of the input (e.g. an end-tag mismatch closing the
                // root). Upstream parses each chunk eagerly and fires the
                // events either way, so deliver them now exactly once
                // (PHP xml_parse defaults isFinal=false — bug25666/xml009).
                push_state(ctxt).completed = true;
                let delivery_buf = base.duplicate_for_reparse();
                let input_stack = InputStack::new(delivery_buf);
                // SAFETY: ctxt is a valid, initialised parser context.
                restore_start_well_formed(ctxt);
                let mut parser = unsafe { XmlParser::new(input_stack, ctxt) };
                parser.parse_document();
                // UPSTREAM-PARITY (parser.c xmlParseChunk): the delivery
                // parse's outcome is reported on the non-final call — the
                // recorded error code once the document is no longer
                // well-formed (a fatal error, or a stop from an
                // entity-resolving SAX handler that leaves errNo set and
                // wellFormed = 0), 0 otherwise. PHP's expat-compat XML_Parse
                // maps a non-zero return to FALSE, so xml_parse() returns
                // FALSE for bug71592 exactly like the oracle.
                let mut failed = false;
                if unsafe { (*ctxt).wellFormed } == 0 {
                    failed = true;
                }
                // More data may still arrive: stash the accumulated buffer
                // for the next xmlParseChunk call (upstream keeps the data in
                // ctxt->input even after a failed non-final call). A
                // probe/early-delivery never changes the reported status of a
                // non-final call on a still-paused document (0 = more data
                // expected).
                stash_input_buffer(ctxt, Box::into_raw(Box::new(base)));
                return if failed { unsafe { (*ctxt).errNo } } else { 0 };
            }
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
        // Plain final parse of everything accumulated so far.
        let input_stack = InputStack::new(base);
        // SAFETY: ctxt is a valid, initialised parser context.
        restore_start_well_formed(ctxt);
        let mut parser = unsafe { XmlParser::new(input_stack, ctxt) };
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
