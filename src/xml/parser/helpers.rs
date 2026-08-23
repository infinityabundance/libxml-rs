//! Parser helper functions — bridge between C ABI exports and the internal parser (§19).
//!
//! This module provides the glue layer that C ABI entry points (in `crate::abi::exports_xml2`)
//! call into. Each helper takes raw C pointers and converts them to safe Rust types,
//! delegates to the internal parser, and converts results back.
//!
//! # Ownership model
//!
//! When `setup_parser_input` is called, the `InputBuffer` is boxed and leaked (stored as a raw
//! pointer in `ctxt._private`). This keeps the buffer's data alive so that the
//! `_xmlParserInput` pointers (`base`/`cur`/`end`) remain valid. `parse_document` and
//! `parse_chunk` take ownership of that boxed buffer, move it into an `InputStack`, create
//! an `XmlParser`, run it, and drop everything — consuming the buffer in the process.

use core::ffi::CStr;
use core::ptr;
use std::mem::size_of;
use std::os::raw::{c_char, c_int, c_void};

use crate::abi::allocator::{xmlFree, xmlMalloc, xmlMallocZero};
use crate::abi::callbacks::{xmlInputCloseCallback, xmlInputReadCallback};
use crate::abi::structs::{_xmlParserCtxt, _xmlParserInput, _xmlParserInputBuffer, _xmlSAXHandler};
use crate::xml::parser::input::{InputBuffer, InputStack};
use crate::xml::parser::state::XmlParser;
use crate::xml::sax::xmlSAX2InitDefaultSAXHandler;

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
        unsafe { xmlFree(ctxt as *mut c_void) };
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
            xmlFree(sax as *mut c_void);
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
            xmlFree(input_tab as *mut c_void);
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
            xmlFree(node_tab as *mut c_void);
        }

        // Free the name stack.
        let name_tab = (*ctxt).nameTab;
        if !name_tab.is_null() {
            xmlFree(name_tab as *mut c_void);
        }

        // Free the stored InputBuffer (leaked in setup_parser_input).
        let private_data = (*ctxt)._private;
        if !private_data.is_null() {
            // SAFETY: The pointer was obtained via Box::into_raw in setup_parser_input.
            // Reconstruct the Box and let it drop.
            let _ = Box::from_raw(private_data as *mut InputBuffer);
        }

        // Free the context itself.
        xmlFree(ctxt as *mut c_void);
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
    match unsafe { InputBuffer::from_callback(read, close, ioctx) } {
        Ok(buf) => buf,
        Err(_) => {
            // Return an empty buffer on failure.
            InputBuffer::from_memory(&[], None)
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

        // Store the leaked pointer so we can free it later.
        c._private = input_buf_ptr as *mut c_void;

        // SAFETY: input_buf_ptr points to a live InputBuffer whose data Vec
        // will not move while the _xmlParserInput references it.
        let parser_input = alloc_parser_input(&*input_buf_ptr, None);
        c.input = parser_input;

        // Allocate inputTab with initial capacity of 4 pointers.
        let tab_size = 4 * size_of::<*mut _xmlParserInput>();
        // SAFETY: xmlMalloc returns uninitialised memory or NULL.
        let tab = xmlMalloc(tab_size) as *mut *mut _xmlParserInput;
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
/// Takes ownership of the `InputBuffer` stored in `ctxt._private` (from a
/// previous `setup_parser_input` call), creates an `InputStack` and `XmlParser`,
/// and runs `parse_document`.
///
/// Returns `0` on success, `-1` on error.
///
/// # Safety
///
/// - `ctxt` must be a valid pointer to a `_xmlParserCtxt` that was set up via
///   `setup_parser_input` (or equivalent).
/// - After this call, the `InputBuffer` is consumed and `ctxt._private` is set
///   to NULL. The context's `input` and `inputTab` may contain dangling pointers
///   and should not be used for further parsing.
pub(crate) unsafe fn parse_document(ctxt: *mut _xmlParserCtxt) -> c_int {
    // Take ownership of the stored InputBuffer.
    // SAFETY: The pointer was stored by setup_parser_input via Box::into_raw.
    let input_buf = {
        let c = &mut *ctxt;
        let ptr = c._private as *mut InputBuffer;
        c._private = ptr::null_mut();
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

    // Take ownership of the stored InputBuffer (if any).
    let stored_buf: Option<Box<InputBuffer>> = unsafe {
        let c = &mut *ctxt;
        if c._private.is_null() {
            None
        } else {
            let ptr = c._private as *mut InputBuffer;
            c._private = ptr::null_mut();
            // SAFETY: ptr is a valid Box<InputBuffer> from Box::into_raw.
            Some(unsafe { Box::from_raw(ptr) })
        }
    };

    // Build the input stack. If there is a stored buffer, use it as the base;
    // otherwise create an empty one.
    let mut input_stack = if let Some(buf) = stored_buf {
        InputStack::new(*buf)
    } else {
        InputStack::new(InputBuffer::from_memory(&[], None))
    };

    // Push the chunk onto the stack.
    if !chunk_slice.is_empty() {
        let chunk_buf = InputBuffer::from_memory(chunk_slice, None);
        input_stack.push(chunk_buf);
    }

    // SAFETY: ctxt is a valid, initialised parser context.
    let mut parser = unsafe { XmlParser::new(input_stack, ctxt) };

    if terminate != 0 {
        parser.parse_document()
    } else {
        0
    }
    // parser is dropped here.
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
        // (base, cur, end, line, col, length, consumed, filename).
        input.populate_parser_input(pi);

        // Override the filename if one was explicitly provided.
        if let Some(fname) = filename {
            pi.filename = fname.as_ptr() as *const c_char;
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
/// This frees only the struct itself — any data it points to is owned
/// elsewhere and must be freed separately.
///
/// # Safety
///
/// `input` must be a valid pointer returned by `alloc_parser_input` or
/// `xmlMalloc`, or NULL (in which case this is a no-op).
pub(crate) unsafe fn free_parser_input(input: *mut _xmlParserInput) {
    if input.is_null() {
        return;
    }
    // SAFETY: The pointer was allocated via xmlMalloc (or xmlMallocZero).
    unsafe { xmlFree(input as *mut c_void) };
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
    unsafe { xmlFree(buf as *mut c_void) };
}
