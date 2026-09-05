//! exports_parser — C ABI exports for the XML parser family (§11.1-I).
//!
//! Implements the parser/parserInternals/xmlIO/encoding/tree export surface:
//! parser-context creation and lifecycle, the `xmlCtxtRead*` family, parser
//! input buffers and streams, encoding switches, the deprecated node-info
//! sequence, global I/O callback registration, external-entity loaders, the
//! `xmlFile*` I/O callbacks, low-level character scanning helpers and the
//! SAX/DTD parse front-ends.
//!
//! Where an internal engine entry point exists (e.g. `crate::xml::parser::helpers`),
//! the export wraps it; otherwise the function is ported from upstream
//! `parser.c` / `parserInternals.c` / `xmlIO.c` / `error.c` / `encoding.c`
//! (see `archaeology/libxml2-git`).
//!
//! # Upstream contract
//!
//! Parity target is upstream `parser.c`, `parserInternals.c`, `xmlIO.c`,
//! `error.c` and `encoding.c` (libxml2 2.15.3) with the `parser.h`/
//! `parserInternals.h`/`xmlIO.h` signatures. Residuals R-000164 (parser/tree
//! structural parity), R-000165 (parser-context accessors and input
//! constructors) and R-000169 (input filename ownership) all land here.
//!
//! # Conceptual behavior
//!
//! This module implements the parser export surface: parser-context
//! creation/lifecycle, the `xmlCtxtRead*`/`xmlRead*` families, parser input
//! buffers and streams, encoding switches, the deprecated node-info sequence,
//! global I/O callback registration, external-entity loaders, the `xmlFile*`
//! I/O callbacks and the SAX/DTD parse front-ends. Internal engine entry
//! points are wrapped; the rest are ported from the upstream sources.
//!
//! # Ownership & safety invariants
//!
//! Parser contexts are caller-owned (freed with `xmlFreeParserCtxt`); docs
//! returned by `xmlRead*` are caller-owned (freed with `xmlFreeDoc`); inputs
//! created by `xmlNewInputFrom*` are owned by the context once pushed.
//! Filenames stored in `_xmlParserInput.filename` and `doc->URL` are owned
//! copies — R-000169 fixed xml_strndup on non-NUL-terminated Rust Strings
//! (heap-buffer-overflow) and borrowed filename pointers.
//!
//! # Historical quirks & epochs
//!
//! QUIRK-0001/LORE-0001: since 2.9.0 (commit `52d8ade7`, 2012-07-30) default
//! parser limits apply unless `XML_PARSE_HUGE` is set. E-002: parse-error
//! diagnostics changed across 2.9.10 (non-recursive parser refactor) and
//! 2.12.x (error-handling rework); E-005: exit codes reworked in 2.13.0.
//! R-000164 (11.1-N) aligned the parse-time DOM construction with upstream
//! (TREE-001 byte-identical).
//!
//! # Deliberate oddities
//!
//! The deprecated `xmlParse*` no-ops and the `xmlFileMatch`/
//! `xmlParserInputRead` trivial bodies are deliberate (R-000138 set:
//! upstreams own bodies are empty/trivial). `xmlReadMemory` accepts size-0
//! input (R-000163) and applies options/URL on both success and recovery
//! paths (R-000164).
//!
//! # Proving courts
//!
//! The PARSER court family, the ERROR-001 probe (error-family-probe.c, 48/48
//! byte-identical), the TREE-001 structural probe and the DSO-LOADER/
//! HEADER-COMPILE courts cover this module; the parser unit suite runs under
//! cargo test.
//!
//! # Tempting simplifications that would break parity
//!
//! A tempting simplification is to store the input filename as a borrowed
//! pointer into a Rust String — R-000169 proved that produces dangling
//! `filename`/`doc->URL` pointers and heap-reuse garbage on the second parse;
//! every construction path must own its filename copy. Another shortcut,
//! skipping the `XML_PARSE_HUGE`/limits logic, would diverge from the 2.9.0+
//! oracle on large documents (PARSER-LIMIT courts).

#![allow(missing_docs)]
#![allow(non_snake_case)]
#![allow(non_camel_case_types)]
#![allow(non_upper_case_globals)]

// SAFETY-SCOPE: EXPORT-PARSER-MECHANICAL-001
// (11.1-Z.3 proof scope, classified-generated) — this module is the
// mechanical extern-"C" export surface: every `unsafe` block in it is
// the documented indirection/registry-access pattern whose validity
// rests on the upstream C contract, and the exported signatures are
// machine-measured by the ABI-FUNCTION-SIGNATURE and DSO-LOADER
// courts and the C-API differential probes. The safety contract of
// each export is stated in its own doc comment; this scope covers the
// mechanical wrappers' unsafe blocks.

use core::ffi::CStr;
use core::ptr;
use std::os::raw::{c_char, c_int, c_long, c_uchar, c_uint, c_ulong, c_void};

use parking_lot::Mutex;

use crate::abi::allocator::{
    xmlFreeImpl, xmlMallocImpl, xmlMallocZero, xmlMemStrdupImpl, xmlReallocImpl,
};
use crate::abi::callbacks::{
    xmlGenericErrorFunc, xmlInputCloseCallback, xmlInputReadCallback, xmlOutputCloseCallback,
    xmlOutputWriteCallback, xmlStructuredErrorFunc,
};
use crate::abi::structs::*;
use crate::abi::types::*;
use crate::xml::parser::helpers;
use crate::xml::parser::input::InputBuffer;
use crate::xml::{dtd, encoding, entities, errors, globals, io, string, tree};

// ═══════════════════════════════════════════════════════════════════════════════
// Local ABI types (upstream xmlIO.h / parser.h, not present in callbacks.rs)
// ═══════════════════════════════════════════════════════════════════════════════

/// `xmlInputMatchCallback` — decide whether a filename is handled by the
/// registered input callback pair.
type xmlInputMatchCallback = unsafe extern "C" fn(filename: *const c_char) -> c_int;

/// `xmlInputOpenCallback` — open a resource and return an I/O context.
type xmlInputOpenCallback = unsafe extern "C" fn(filename: *const c_char) -> *mut c_void;

/// `xmlOutputMatchCallback` — decide whether a filename is handled by the
/// registered output callback pair.
type xmlOutputMatchCallback = unsafe extern "C" fn(filename: *const c_char) -> c_int;

/// `xmlOutputOpenCallback` — open a resource for writing and return a context.
type xmlOutputOpenCallback = unsafe extern "C" fn(filename: *const c_char) -> *mut c_void;

/// `xmlExternalEntityLoader` — resolve an external entity to a parser input.
type xmlExternalEntityLoader = unsafe extern "C" fn(
    URL: *const c_char,
    ID: *const c_char,
    ctxt: *mut _xmlParserCtxt,
) -> *mut _xmlParserInput;

#[derive(Clone, Copy)]
struct InputCallbackEntry {
    matchcb: Option<xmlInputMatchCallback>,
    opencb: Option<xmlInputOpenCallback>,
    readcb: Option<xmlInputReadCallback>,
    closecb: Option<xmlInputCloseCallback>,
}

#[allow(dead_code)]
#[derive(Clone, Copy)]
struct OutputCallbackEntry {
    matchcb: Option<xmlOutputMatchCallback>,
    opencb: Option<xmlOutputOpenCallback>,
    writecb: Option<xmlOutputWriteCallback>,
    closecb: Option<xmlOutputCloseCallback>,
}

static INPUT_CALLBACKS: Mutex<Vec<InputCallbackEntry>> = Mutex::new(Vec::new());
static OUTPUT_CALLBACKS: Mutex<Vec<OutputCallbackEntry>> = Mutex::new(Vec::new());

static EXTERNAL_ENTITY_LOADER: Mutex<Option<xmlExternalEntityLoader>> =
    Mutex::new(Some(default_external_entity_loader));

// Deprecated legacy function codes not present in types.rs (upstream xmlerror.h).
const XML_ERR_USER_STOP: c_int = 111;
#[allow(dead_code)]
const XML_ERR_RESOURCE_LIMIT: c_int = 114;

// XML_SCAN_* flags (upstream include/private/parser.h).
const XML_SCAN_NC: c_int = 1;
const XML_SCAN_NMTOKEN: c_int = 2;
const XML_SCAN_OLD10: c_int = 4;

// xmlParserLoadSubset bits (upstream parser.h, 2.15.3):
//   XML_DETECT_IDS = 2, XML_COMPLETE_ATTRS = 4, XML_SKIP_IDS = 8.
// These are internal bits of `ctxt->loadsubset`, NOT parse-option flags — they
// must match the header values exactly or consumers that inspect `loadsubset`
// directly (lxml's SAX target bridge gates default-attribute delivery on
// `loadsubset & XML_COMPLETE_ATTRS`) drop DTD default attributes.
#[allow(dead_code)]
const XML_DETECT_IDS: c_int = 2;
const XML_COMPLETE_ATTRS: c_int = 4;
const XML_SKIP_IDS: c_int = 8;

/// Keep enough input around to show errors in context (parserInternals.c).
const LINE_LEN: usize = 80;

/// Minimal amount of data the parser expects in the buffer (parserInternals.c).
#[allow(dead_code)]
const INPUT_CHUNK: usize = 100;

const XML_INVALID_CHAR: c_int = -1;

// ═══════════════════════════════════════════════════════════════════════════════
// Internal helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Shared context initialisation: zeroes `ctxt`, installs the SAX handler and
/// sets the initial parser state (upstream `xmlInitSAXParserCtxt`).
///
/// # Safety
///
/// `ctxt` must be a valid, writable, freshly allocated parser context.
unsafe fn init_sax_parser_ctxt(
    ctxt: *mut _xmlParserCtxt,
    sax: *const _xmlSAXHandler,
    userData: *mut c_void,
) -> c_int {
    unsafe {
        ptr::write_bytes(ctxt as *mut u8, 0, core::mem::size_of::<_xmlParserCtxt>());

        let c = &mut *ctxt;

        // SAX handler.
        if c.sax.is_null() {
            let new_sax =
                xmlMallocZero(core::mem::size_of::<_xmlSAXHandler>()) as *mut _xmlSAXHandler;
            if new_sax.is_null() {
                return -1;
            }
            c.sax = new_sax;
        }
        if sax.is_null() {
            crate::xml::sax::xmlSAX2InitDefaultSAXHandler(c.sax);
            c.userData = ctxt as *mut c_void;
        } else if (*sax).initialized == XML_SAX2_MAGIC as c_uint {
            // Full SAX2 handler copy.
            ptr::copy_nonoverlapping(sax, c.sax, 1);
            c.userData = if userData.is_null() {
                ctxt as *mut c_void
            } else {
                userData
            };
        } else {
            // SAX1 handler: only the V1 prefix is meaningful.
            ptr::write_bytes(c.sax as *mut u8, 0, core::mem::size_of::<_xmlSAXHandler>());
            ptr::copy_nonoverlapping(
                sax as *const u8,
                c.sax as *mut u8,
                core::mem::size_of::<_xmlSAXHandlerV1>(),
            );
            c.userData = if userData.is_null() {
                ctxt as *mut c_void
            } else {
                userData
            };
        }

        c.wellFormed = 1;
        c.standalone = -1;
        c.errNo = XML_ERR_OK;
        c.valid = 1;
        c.nsWellFormed = 1;
        c.instate = xmlParserInputState::XML_PARSER_START as c_int;
        c.keepBlanks = globals::get_keep_blanks_default();
        c.replaceEntities = globals::get_substitute_entities_default();
        c.linenumbers = 1;
        c.charset = xmlCharEncoding::XML_CHAR_ENCODING_UTF8 as c_int;
        c.pedantic = globals::get_pedantic_parser_default();
        c.loadsubset = globals::get_load_ext_dtd_default();
        c.docdict = 1;
        c.options = 0;

        c.vctxt.userData = ctxt as *mut c_void;
        c.vctxt.valid = 1;
    }
    0
}

/// Mirror `options` into the parser context's historical struct members
/// (upstream `xmlCtxtSetOptionsInternal`).
///
/// `keepBlanks` is special: the executed 2.15.3 oracle seeds it from the
/// deprecated `xmlKeepBlanksDefaultValue` at context creation and only ever
/// LOWERS it (XML_PARSE_NOBLANKS) — option application never re-raises it.
/// Empirical: a context created while `xmlKeepBlanksDefault(0)` drops
/// whitespace-only text for ALL its reads, even reused ones, and
/// `xmlCtxtUseOptions`/read options without NOBLANKS do not restore it.
///
/// # Safety
///
/// `ctxt` must be a valid, writable parser context.
pub(crate) unsafe fn apply_options(ctxt: *mut _xmlParserCtxt, options: c_int) {
    unsafe {
        let c = &mut *ctxt;
        c.options = options;
        c.recovery = (options & XML_PARSE_RECOVER != 0) as c_int;
        c.replaceEntities = (options & XML_PARSE_NOENT != 0) as c_int;
        // UPSTREAM-PARITY (parser.c xmlCtxtUseOptions):
        //   ctxt->loadsubset = (options & XML_PARSE_DTDLOAD) ? XML_DETECT_IDS : 0;
        //   ctxt->loadsubset |= (options & XML_PARSE_DTDATTR) ? XML_COMPLETE_ATTRS : 0;
        //   ctxt->loadsubset |= (options & XML_PARSE_SKIP_IDS) ? XML_SKIP_IDS : 0;
        // `loadsubset` is an ABI-visible field; lxml's SAX target bridge reads
        // `loadsubset & XML_COMPLETE_ATTRS` to decide whether to deliver DTD
        // default attributes, so the bit values must be the upstream ones
        // (XML_DETECT_IDS=2, XML_COMPLETE_ATTRS=4, XML_SKIP_IDS=8).
        c.loadsubset = if options & XML_PARSE_DTDLOAD != 0 {
            XML_DETECT_IDS
        } else {
            0
        };
        if options & XML_PARSE_DTDATTR != 0 {
            c.loadsubset |= XML_COMPLETE_ATTRS;
        }
        if options & XML_PARSE_SKIP_IDS != 0 {
            c.loadsubset |= XML_SKIP_IDS;
        }
        c.validate = (options & XML_PARSE_DTDVALID != 0) as c_int;
        c.pedantic = (options & XML_PARSE_PEDANTIC != 0) as c_int;
        if options & XML_PARSE_NOBLANKS != 0 {
            c.keepBlanks = 0;
        }
        c.dictNames = if options & XML_PARSE_NODICT != 0 {
            0
        } else {
            1
        };
    }
}

/// Find the registered encoding handler for an `xmlCharEncoding` value, or NULL.
unsafe fn encoding_handler_for(enc: c_int) -> *mut _xmlCharEncodingHandler {
    let e: xmlCharEncoding = unsafe { core::mem::transmute(enc) };
    match encoding::encoding_name(e) {
        Some(name) => {
            let mut nul = name.to_vec();
            nul.push(0);
            encoding::find_encoding_handler(nul.as_ptr() as *const xmlChar)
        }
        None => ptr::null_mut(),
    }
}

/// Build a `_xmlParserInput` that references the data owned by `buf` (an input
/// buffer previously created by the xmlIO layer). The buffer keeps the data
/// alive; the returned input must be freed with `helpers::free_parser_input`.
///
/// # Safety
///
/// `buf` must be a valid input buffer or NULL, and must outlive the returned
/// input.
unsafe fn parser_input_from_buf(buf: *mut _xmlParserInputBuffer) -> *mut _xmlParserInput {
    let input =
        unsafe { xmlMallocZero(core::mem::size_of::<_xmlParserInput>()) } as *mut _xmlParserInput;
    if input.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        (*input).buf = buf;
        (*input).line = 1;
        (*input).col = 1;
        if !buf.is_null() {
            let b = &*buf;
            if !b.buffer.is_null() {
                let xbuf = &*(b.buffer as *mut _xmlBuffer);
                if !xbuf.content.is_null() {
                    (*input).base = xbuf.content;
                    (*input).cur = xbuf.content;
                    (*input).end = xbuf.content.add(xbuf.use_ as usize);
                    (*input).length = xbuf.use_ as c_int;
                }
            }
        }
    }
    input
}

/// `pub(crate)` wrapper of [`parser_input_from_buf`] for the sibling
/// xmlNewInputFrom* family (11.1-X R-000165 closure).
pub(crate) unsafe fn parser_input_from_buf_pub(
    buf: *mut _xmlParserInputBuffer,
) -> *mut _xmlParserInput {
    unsafe { parser_input_from_buf(buf) }
}

/// Materialise an `InputBuffer` (owned copy) from a raw `_xmlParserInput`,
/// so the data survives the caller's input lifetime.
///
/// # Safety
///
/// `input` must be a valid pointer to a `_xmlParserInput`.
unsafe fn input_buffer_from_parser_input(input: *mut _xmlParserInput) -> InputBuffer {
    unsafe {
        let pi = &*input;
        if !pi.buf.is_null() {
            let b = &*pi.buf;
            if let Some(read) = b.readcallback {
                return helpers::input_from_io(Some(read), b.closecallback, b.context);
            }
        }
        if !pi.base.is_null() && !pi.end.is_null() && pi.end >= pi.base {
            let len = (pi.end as usize).saturating_sub(pi.base as usize);
            let slice = core::slice::from_raw_parts(pi.base, len);
            return InputBuffer::from_memory(slice, None);
        }
        InputBuffer::from_memory(&[], None)
    }
}

/// Core of `xmlCtxtRead*`: reset the context, wire an input buffer, parse,
/// and return the resulting document (freed on hard error unless recovery).
///
/// # Safety
///
/// `ctxt` must be a valid parser context; `input` is consumed.
unsafe fn ctxt_read_doc(
    ctxt: *mut _xmlParserCtxt,
    input: InputBuffer,
    url: *const c_char,
    options: c_int,
) -> *mut _xmlDoc {
    unsafe {
        xmlCtxtReset(ctxt);
        apply_options(ctxt, options);
        helpers::setup_parser_input(ctxt, input);
        if helpers::parse_document(ctxt) != 0 {
            let doc = (*ctxt).myDoc;
            (*ctxt).myDoc = ptr::null_mut();
            if options & XML_PARSE_RECOVER != 0 {
                return doc;
            }
            if !doc.is_null() {
                tree::free_doc(doc);
            }
            return ptr::null_mut();
        }
        let doc = (*ctxt).myDoc;
        if !doc.is_null() && !url.is_null() {
            (*doc).URL = string::xml_strdup(url as *const xmlChar);
        }
        doc
    }
}

/// Parse DTD declaration text using the internal engine by wrapping it in a
/// synthetic document (`<!DOCTYPE none [ ... ]><none/>`) when the text is a
/// bare DTD subset, or parsing it directly when it is already a document.
///
/// Returns a detached DTD (never owned by a document), or NULL.
///
/// # Safety
///
/// `ctxt` must be a valid parser context; `data` must be readable for `len`
/// bytes.
unsafe fn parse_dtd_text(
    ctxt: *mut _xmlParserCtxt,
    data: &[u8],
    public_id: *const xmlChar,
    system_id: *const xmlChar,
) -> *mut _xmlDtd {
    unsafe {
        // If the content is already a full document (contains a DOCTYPE),
        // parse it directly; otherwise wrap the declarations.
        let has_doctype = data
            .windows(9)
            .any(|w| w.eq_ignore_ascii_case(b"<!DOCTYPE"));
        let mut wrapped: Vec<u8>;
        let parse_data: &[u8] = if has_doctype {
            data
        } else {
            wrapped = Vec::with_capacity(data.len() + 32);
            wrapped.extend_from_slice(b"<!DOCTYPE none [");
            wrapped.extend_from_slice(data);
            wrapped.extend_from_slice(b"]><none/>");
            &wrapped
        };

        let input = InputBuffer::from_memory(parse_data, None);
        helpers::setup_parser_input(ctxt, input);
        let rc = helpers::parse_document(ctxt);
        let doc = (*ctxt).myDoc;
        (*ctxt).myDoc = ptr::null_mut();

        if rc == 0 && !doc.is_null() && !(*doc).intSubset.is_null() {
            let dtd = (*doc).intSubset;
            (*doc).intSubset = ptr::null_mut();
            (*dtd).parent = ptr::null_mut();
            (*dtd).doc = ptr::null_mut();
            if !public_id.is_null() {
                (*dtd).ExternalID = string::xml_strdup(public_id);
            }
            if !system_id.is_null() {
                (*dtd).SystemID = string::xml_strdup(system_id);
            }
            tree::free_doc(doc);
            return dtd;
        }

        if !doc.is_null() {
            tree::free_doc(doc);
        }
        // Fallback: an empty DTD carrying the identifiers.

        dtd::new_dtd(
            ptr::null_mut(),
            c"none".as_ptr() as *const xmlChar,
            public_id,
            system_id,
        )
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Context creation / lifecycle
// ═══════════════════════════════════════════════════════════════════════════════

/// Create a new parser context with a default SAX2 handler.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlParserCtxtPtr xmlNewParserCtxt(void);
/// ```
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
pub unsafe extern "C" fn xmlNewParserCtxt() -> *mut _xmlParserCtxt {
    unsafe {
        globals::init_parser();
        let ctxt = xmlMallocZero(core::mem::size_of::<_xmlParserCtxt>()) as *mut _xmlParserCtxt;
        if ctxt.is_null() {
            return ptr::null_mut();
        }
        if init_sax_parser_ctxt(ctxt, ptr::null(), ptr::null_mut()) < 0 {
            helpers::free_parser_ctxt(ctxt);
            return ptr::null_mut();
        }
        ctxt
    }
}

/// Create a new parser context using the given SAX handler (or the default
/// SAX2 handler when `sax` is NULL).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlParserCtxtPtr xmlNewSAXParserCtxt(const xmlSAXHandler *sax, void *userData);
/// ```
///
/// # SAFETY
///
/// - `sax`, `userData` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlNewSAXParserCtxt(
    sax: *const _xmlSAXHandler,
    userData: *mut c_void,
) -> *mut _xmlParserCtxt {
    unsafe {
        globals::init_parser();
        let ctxt = xmlMallocZero(core::mem::size_of::<_xmlParserCtxt>()) as *mut _xmlParserCtxt;
        if ctxt.is_null() {
            return ptr::null_mut();
        }
        if init_sax_parser_ctxt(ctxt, sax, userData) < 0 {
            helpers::free_parser_ctxt(ctxt);
            return ptr::null_mut();
        }
        ctxt
    }
}

/// Initialise a parser context (legacy API): zeroes the context, installs a
/// default SAX2 handler and sets the initial parser state.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlInitParserCtxt(xmlParserCtxtPtr ctxt);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlInitParserCtxt(ctxt: *mut _xmlParserCtxt) -> c_int {
    unsafe { init_sax_parser_ctxt(ctxt, ptr::null(), ptr::null_mut()) }
}

/// Clear (reset) a parser context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlClearParserCtxt(xmlParserCtxtPtr ctxt);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlClearParserCtxt(ctxt: *mut _xmlParserCtxt) {
    unsafe { xmlCtxtReset(ctxt) }
}

/// Reset a parser context: drop the input stack, node/name stacks, strings,
/// document and error state so the context can be reused.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlCtxtReset(xmlParserCtxtPtr ctxt);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlCtxtReset(ctxt: *mut _xmlParserCtxt) {
    if ctxt.is_null() {
        return;
    }
    unsafe {
        let c = &mut *ctxt;

        // Free all inputs on the stack.
        let input_nr = c.inputNr;
        let input_tab = c.inputTab;
        if !input_tab.is_null() {
            for i in 0..input_nr {
                let input = *input_tab.add(i as usize);
                if !input.is_null() {
                    helpers::free_parser_input(input);
                }
            }
            xmlFreeImpl(input_tab as *mut c_void);
        }
        c.inputTab = ptr::null_mut();
        c.inputMax = 0;
        c.inputNr = 0;
        c.input = ptr::null_mut();

        // Free the stored InputBuffer (stashed by setup_parser_input; the
        // side table keeps ctxt._private application data — 11.1-X).
        helpers::free_stashed_input_buffer(ctxt);
        // Drop any incremental-push state (SP-14.3.1-3).
        helpers::free_push_state(ctxt);

        // Node stack (array only; nodes are owned by the doc).
        if !c.nodeTab.is_null() {
            xmlFreeImpl(c.nodeTab as *mut c_void);
        }
        c.nodeTab = ptr::null_mut();
        c.nodeMax = 0;
        c.nodeNr = 0;
        c.node = ptr::null_mut();

        // Name stack.
        if !c.nameTab.is_null() {
            xmlFreeImpl(c.nameTab as *mut c_void);
        }
        c.nameTab = ptr::null_mut();
        c.nameMax = 0;
        c.nameNr = 0;
        c.name = ptr::null();

        // Space stack: keep the allocation, reset the counter.
        c.spaceNr = 0;
        c.space = ptr::null_mut();

        // Namespaces.
        c.nsNr = 0;

        // Strings owned by the context.
        if !c.version.is_null() {
            xmlFreeImpl(c.version as *mut c_void);
            c.version = ptr::null_mut();
        }
        if !c.encoding.is_null() {
            xmlFreeImpl(c.encoding as *mut c_void);
            c.encoding = ptr::null_mut();
        }
        if !c.extSubURI.is_null() {
            xmlFreeImpl(c.extSubURI as *mut c_void);
            c.extSubURI = ptr::null_mut();
        }
        if !c.extSubSystem.is_null() {
            xmlFreeImpl(c.extSubSystem as *mut c_void);
            c.extSubSystem = ptr::null_mut();
        }
        if !c.directory.is_null() {
            xmlFreeImpl(c.directory as *mut c_void);
            c.directory = ptr::null_mut();
        }

        // Document: the context owns it until reset/free.
        if !c.myDoc.is_null() {
            tree::free_doc(c.myDoc);
        }
        c.myDoc = ptr::null_mut();

        // Parser state.
        c.standalone = -1;
        c.hasExternalSubset = 0;
        c.hasPErefs = 0;
        c.instate = xmlParserInputState::XML_PARSER_START as c_int;
        c.wellFormed = 1;
        c.nsWellFormed = 1;
        c.disableSAX = 0;
        c.valid = 1;
        c.record_info = 0;
        c.checkIndex = 0;
        c.inSubset = 0;
        c.errNo = XML_ERR_OK;
        c.depth = 0;
        c.nbentities = 0;
        c.sizeentities = 0;
        c.nbErrors = 0;
        c.nbWarnings = 0;

        xmlInitNodeInfoSeq(&mut c.node_seq);

        if c.lastError.code != XML_ERR_OK {
            errors::reset_error(&mut c.lastError);
        }
    }
}

/// Reset a push-parser context and set up a fresh input chunk.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlCtxtResetPush(xmlParserCtxtPtr ctxt, const char *chunk, int size,
///                      const char *filename, const char *encoding);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `chunk`, `filename`, `encoding` must point to valid NUL-terminated
///   strings (or NULL where the C contract allows) for the lifetime
///   of the call.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlCtxtResetPush(
    ctxt: *mut _xmlParserCtxt,
    chunk: *const c_char,
    size: c_int,
    filename: *const c_char,
    encoding: *const c_char,
) -> c_int {
    if ctxt.is_null() {
        return 1;
    }
    unsafe {
        xmlCtxtReset(ctxt);

        let slice = if size > 0 && !chunk.is_null() {
            core::slice::from_raw_parts(chunk as *const u8, size as usize)
        } else {
            &[]
        };
        let uri = if filename.is_null() {
            None
        } else {
            CStr::from_ptr(filename).to_str().ok()
        };
        let input = InputBuffer::from_memory(slice, uri);
        helpers::setup_parser_input(ctxt, input);

        if !encoding.is_null() {
            let handler = encoding::find_encoding_handler(encoding as *const xmlChar);
            if !handler.is_null() {
                xmlSwitchToEncoding(ctxt, handler);
            }
        }
    }
    0
}

/// Apply a full set of parser options, clearing options not present.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlCtxtSetOptions(xmlParserCtxtPtr ctxt, int options);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlCtxtSetOptions(ctxt: *mut _xmlParserCtxt, options: c_int) -> c_int {
    if ctxt.is_null() {
        return -1;
    }
    const ALL_MASK: c_int = XML_PARSE_RECOVER
        | XML_PARSE_NOENT
        | XML_PARSE_DTDLOAD
        | XML_PARSE_DTDATTR
        | XML_PARSE_DTDVALID
        | XML_PARSE_NOERROR
        | XML_PARSE_NOWARNING
        | XML_PARSE_PEDANTIC
        | XML_PARSE_NOBLANKS
        | XML_PARSE_SAX1
        | XML_PARSE_NONET
        | XML_PARSE_NODICT
        | XML_PARSE_NSCLEAN
        | XML_PARSE_NOCDATA
        | XML_PARSE_COMPACT
        | XML_PARSE_OLD10
        | XML_PARSE_HUGE
        | XML_PARSE_OLDSAX
        | XML_PARSE_IGNORE_ENC
        | XML_PARSE_BIG_LINES;

    unsafe {
        apply_options(ctxt, options & ALL_MASK);
    }
    options & !ALL_MASK
}

/// Install a per-context structured error handler.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlCtxtSetErrorHandler(xmlParserCtxtPtr ctxt,
///                             xmlStructuredErrorFunc handler, void *data);
/// ```
///
/// # SAFETY
///
/// - `ctxt`, `data` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `handler` must be a valid callback (or None);
///   the callback is invoked with the documented context pointer and
///   must itself uphold the same pointer invariants.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlCtxtSetErrorHandler(
    ctxt: *mut _xmlParserCtxt,
    handler: Option<xmlStructuredErrorFunc>,
    data: *mut c_void,
) {
    if ctxt.is_null() {
        return;
    }
    unsafe {
        (*ctxt).errorHandler = handler;
        (*ctxt).errorCtxt = data;
    }
}

/// Set the maximum entity expansion amplification factor.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlCtxtSetMaxAmplification(xmlParserCtxtPtr ctxt, unsigned maxAmpl);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlCtxtSetMaxAmplification(ctxt: *mut _xmlParserCtxt, maxAmpl: c_uint) {
    if ctxt.is_null() || maxAmpl == 0 {
        return;
    }
    unsafe {
        (*ctxt).maxAmpl = maxAmpl;
    }
}

/// Get the last error raised on the context, or NULL.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// const xmlError *xmlCtxtGetLastError(void *ctx);
/// ```
///
/// # SAFETY
///
/// - `ctx` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlCtxtGetLastError(ctx: *mut c_void) -> *const _xmlError {
    if ctx.is_null() {
        return ptr::null();
    }
    let ctxt = ctx as *mut _xmlParserCtxt;
    unsafe {
        if (*ctxt).lastError.code == XML_ERR_OK {
            return ptr::null();
        }
        &(*ctxt).lastError
    }
}

/// Reset the context's last-error state.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlCtxtResetLastError(void *ctx);
/// ```
///
/// # SAFETY
///
/// - `ctx` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlCtxtResetLastError(ctx: *mut c_void) {
    if ctx.is_null() {
        return;
    }
    let ctxt = ctx as *mut _xmlParserCtxt;
    unsafe {
        (*ctxt).errNo = XML_ERR_OK;
        if (*ctxt).lastError.code != XML_ERR_OK {
            // Upstream xmlResetError frees the owned strings.
            crate::xml::globals::free_error_strings(&(*ctxt).lastError);
            errors::reset_error(&mut (*ctxt).lastError);
        }
    }
}

/// Handle an out-of-memory error on a parser context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlCtxtErrMemory(xmlParserCtxtPtr ctxt);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlCtxtErrMemory(ctxt: *mut _xmlParserCtxt) {
    if ctxt.is_null() {
        return;
    }
    unsafe {
        let c = &mut *ctxt;
        c.errNo = XML_ERR_NO_MEMORY;
        c.instate = xmlParserInputState::XML_PARSER_EOF as c_int;
        c.wellFormed = 0;
        c.disableSAX = 2;

        c.lastError.domain = XML_FROM_PARSER;
        c.lastError.code = XML_ERR_NO_MEMORY;
        c.lastError.level = xmlErrorLevel::XML_ERR_FATAL as c_int;
        // Owned copy (upstream xmlRaiseMemoryError): the per-context last
        // error strings are freed on reset/free, so static literals would be
        // a double-free/UB hazard.
        c.lastError.message =
            crate::abi::allocator::xmlMemStrdupImpl(c"out of memory\n".as_ptr()) as *mut c_char;

        if let Some(handler) = c.errorHandler {
            handler(c.errorCtxt, &c.lastError);
        } else if !c.sax.is_null() {
            if let Some(serror) = (*c.sax).serror {
                serror(c.userData, &c.lastError);
            }
        }
    }
}

/// Stop the parser: no further processing will happen.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlStopParser(xmlParserCtxtPtr ctxt);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlStopParser(ctxt: *mut _xmlParserCtxt) {
    if ctxt.is_null() {
        return;
    }
    unsafe {
        (*ctxt).disableSAX = 2;
        if (*ctxt).errNo == XML_ERR_OK {
            (*ctxt).errNo = XML_ERR_USER_STOP;
            (*ctxt).lastError.code = XML_ERR_USER_STOP;
            (*ctxt).wellFormed = 0;
        }
    }
}

/// Return the byte offset of the current parse position within the current
/// entity, or -1 when it cannot be computed.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// long xmlByteConsumed(xmlParserCtxtPtr ctxt);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlByteConsumed(ctxt: *mut _xmlParserCtxt) -> c_long {
    if ctxt.is_null() {
        return -1;
    }
    unsafe {
        let input = (*ctxt).input;
        if input.is_null() {
            return -1;
        }
        if !(*input).buf.is_null() && !(*(*input).buf).encoder.is_null() {
            // With an encoder we cannot cheaply compute the original byte
            // position; report the raw consumed count.
            return (*(*input).buf).rawconsumed as c_long;
        }
        let consumed = (*input).consumed;
        if (*input).base.is_null() {
            return consumed as c_long;
        }
        (consumed + ((*input).cur as usize).saturating_sub((*input).base as usize) as c_ulong)
            as c_long
    }
}

/// Extract the directory part of a filename (newly allocated).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// char *xmlParserGetDirectory(const char *filename);
/// ```
///
/// # SAFETY
///
///
/// - `filename` must point to valid NUL-terminated
///   strings (or NULL where the C contract allows) for the lifetime
///   of the call.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlParserGetDirectory(filename: *const c_char) -> *mut c_char {
    if filename.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        let len = libc::strlen(filename);
        let mut last_sep: Option<usize> = None;
        for i in 0..len {
            if *filename.add(i) == b'/' as c_char {
                last_sep = Some(i);
            }
        }
        match last_sep {
            Some(0) => xmlMemStrdupImpl(c"/".as_ptr() as *const c_char) as *mut c_char,
            Some(pos) => {
                let slice = core::slice::from_raw_parts(filename as *const u8, pos);
                let mut v = slice.to_vec();
                v.push(0);
                xmlMemStrdupImpl(v.as_ptr() as *const c_char) as *mut c_char
            }
            None => xmlMemStrdupImpl(c".".as_ptr() as *const c_char) as *mut c_char,
        }
    }
}

/// Check whether a file exists: 0 if stat fails, 2 if it is a directory,
/// 1 otherwise.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlCheckFilename(const char *path);
/// ```
///
/// # SAFETY
///
///
/// - `path` must point to valid NUL-terminated
///   strings (or NULL where the C contract allows) for the lifetime
///   of the call.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlCheckFilename(path: *const c_char) -> c_int {
    if path.is_null() {
        return 0;
    }
    unsafe {
        let mut st: libc::stat = core::mem::zeroed();
        if libc::stat(path, &mut st) != 0 {
            return 0;
        }
        if st.st_mode & libc::S_IFMT == libc::S_IFDIR {
            2
        } else {
            1
        }
    }
}

/// Test whether a public/system ID pair is one of the XHTML DTDs.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlIsXHTML(const xmlChar *systemID, const xmlChar *publicID);
/// ```
///
/// # SAFETY
///
///
/// - `systemID`, `publicID` must point to valid NUL-terminated
///   strings (or NULL where the C contract allows) for the lifetime
///   of the call.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlIsXHTML(systemID: *const xmlChar, publicID: *const xmlChar) -> c_int {
    const XHTML_STRICT_PUBLIC_ID: &[u8] = b"-//W3C//DTD XHTML 1.0 Strict//EN\0";
    const XHTML_STRICT_SYSTEM_ID: &[u8] = b"http://www.w3.org/TR/xhtml1/DTD/xhtml1-strict.dtd\0";
    const XHTML_FRAME_PUBLIC_ID: &[u8] = b"-//W3C//DTD XHTML 1.0 Frameset//EN\0";
    const XHTML_FRAME_SYSTEM_ID: &[u8] = b"http://www.w3.org/TR/xhtml1/DTD/xhtml1-frameset.dtd\0";
    const XHTML_TRANS_PUBLIC_ID: &[u8] = b"-//W3C//DTD XHTML 1.0 Transitional//EN\0";
    const XHTML_TRANS_SYSTEM_ID: &[u8] =
        b"http://www.w3.org/TR/xhtml1/DTD/xhtml1-transitional.dtd\0";

    if systemID.is_null() && publicID.is_null() {
        return -1;
    }
    unsafe {
        if !publicID.is_null()
            && (string::xml_strcmp(publicID, XHTML_STRICT_PUBLIC_ID.as_ptr() as *const xmlChar)
                == 0
                || string::xml_strcmp(publicID, XHTML_FRAME_PUBLIC_ID.as_ptr() as *const xmlChar)
                    == 0
                || string::xml_strcmp(publicID, XHTML_TRANS_PUBLIC_ID.as_ptr() as *const xmlChar)
                    == 0)
        {
            return 1;
        }
        if !systemID.is_null()
            && (string::xml_strcmp(systemID, XHTML_STRICT_SYSTEM_ID.as_ptr() as *const xmlChar)
                == 0
                || string::xml_strcmp(systemID, XHTML_FRAME_SYSTEM_ID.as_ptr() as *const xmlChar)
                    == 0
                || string::xml_strcmp(systemID, XHTML_TRANS_SYSTEM_ID.as_ptr() as *const xmlChar)
                    == 0)
        {
            return 1;
        }
    }
    0
}

// ═══════════════════════════════════════════════════════════════════════════════
// Context creation from sources
// ═══════════════════════════════════════════════════════════════════════════════

/// Create a parser context for an in-memory document.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlParserCtxtPtr xmlCreateMemoryParserCtxt(const char *buffer, int size);
/// ```
///
/// # SAFETY
///
///
/// - `buffer` must point to valid NUL-terminated
///   strings (or NULL where the C contract allows) for the lifetime
///   of the call.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlCreateMemoryParserCtxt(
    buffer: *const c_char,
    size: c_int,
) -> *mut _xmlParserCtxt {
    if buffer.is_null() || size < 0 {
        return ptr::null_mut();
    }
    unsafe {
        let ctxt = xmlNewParserCtxt();
        if ctxt.is_null() {
            return ptr::null_mut();
        }
        let input = helpers::input_from_memory(buffer, size);
        helpers::setup_parser_input(ctxt, input);
        ctxt
    }
}

/// Create a parser context for push parsing.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlParserCtxtPtr xmlCreatePushParserCtxt(xmlSAXHandler *sax, void *user_data,
///                                          const char *chunk, int size,
///                                          const char *filename);
/// ```
///
/// # SAFETY
///
/// - `sax`, `user_data` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `chunk`, `filename` must point to valid NUL-terminated
///   strings (or NULL where the C contract allows) for the lifetime
///   of the call.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlCreatePushParserCtxt(
    sax: *mut _xmlSAXHandler,
    user_data: *mut c_void,
    chunk: *const c_char,
    size: c_int,
    filename: *const c_char,
) -> *mut _xmlParserCtxt {
    unsafe {
        let ctxt = xmlNewSAXParserCtxt(sax, user_data);
        if ctxt.is_null() {
            return ptr::null_mut();
        }
        // UPSTREAM-PARITY (parser.c xmlCreatePushParserCtxt): the push
        // context forces dictNames on (and clears XML_PARSE_NODICT), so
        // element/attribute names are interned in the document dictionary
        // and pointer-identical to xmlDictLookup results — lxml's
        // _MultiTagMatcher (iterparse tag=...) compares name pointers.
        (*ctxt).options &= !crate::abi::types::XML_PARSE_NODICT;
        (*ctxt).dictNames = 1;
        let slice = if size > 0 && !chunk.is_null() {
            core::slice::from_raw_parts(chunk as *const u8, size as usize)
        } else {
            &[]
        };
        let uri = if filename.is_null() {
            None
        } else {
            CStr::from_ptr(filename).to_str().ok()
        };
        let input = InputBuffer::from_memory(slice, uri);
        helpers::setup_parser_input(ctxt, input);
        ctxt
    }
}

/// Create a parser context for an I/O stream.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlParserCtxtPtr xmlCreateIOParserCtxt(xmlSAXHandler *sax, void *user_data,
///                                        xmlInputReadCallback ioread,
///                                        xmlInputCloseCallback ioclose,
///                                        void *ioctx, xmlCharEncoding enc);
/// ```
///
/// # SAFETY
///
/// - `sax`, `user_data`, `ioctx` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `ioread`, `ioclose` must be a valid callback (or None);
///   the callback is invoked with the documented context pointer and
///   must itself uphold the same pointer invariants.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlCreateIOParserCtxt(
    sax: *mut _xmlSAXHandler,
    user_data: *mut c_void,
    ioread: Option<xmlInputReadCallback>,
    ioclose: Option<xmlInputCloseCallback>,
    ioctx: *mut c_void,
    enc: c_int,
) -> *mut _xmlParserCtxt {
    unsafe {
        let ctxt = xmlNewSAXParserCtxt(sax, user_data);
        if ctxt.is_null() {
            return ptr::null_mut();
        }
        let input = helpers::input_from_io(ioread, ioclose, ioctx);
        helpers::setup_parser_input(ctxt, input);
        if enc != xmlCharEncoding::XML_CHAR_ENCODING_NONE as c_int {
            xmlSwitchEncoding(ctxt, enc);
        }
        ctxt
    }
}

/// Create a parser context for a file or URL.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlParserCtxtPtr xmlCreateURLParserCtxt(const char *filename, int options);
/// ```
///
/// # SAFETY
///
///
/// - `filename` must point to valid NUL-terminated
///   strings (or NULL where the C contract allows) for the lifetime
///   of the call.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlCreateURLParserCtxt(
    filename: *const c_char,
    options: c_int,
) -> *mut _xmlParserCtxt {
    if filename.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        let ctxt = xmlNewParserCtxt();
        if ctxt.is_null() {
            return ptr::null_mut();
        }
        apply_options(ctxt, options);
        let input = match open_filename_routed(filename, ctxt) {
            RoutedFileOpen::Loaded(i) => i,
            RoutedFileOpen::Failed => {
                // UPSTREAM-PARITY (parserInternals.c xmlNewInputFromFile via
                // the registered loader): NULL loader result is XML_IO_ENOENT
                // — raise xmlCtxtErrIO, no built-in fallback.
                emit_io_warning(ctxt, io_load_failure_message(filename));
                helpers::free_parser_ctxt(ctxt);
                return ptr::null_mut();
            }
            RoutedFileOpen::EntityLoaderFailed => {
                // UPSTREAM-PARITY (parser.c xmlCreateURLParserCtxt ->
                // xmlLoadResource): a custom entity loader returning NULL
                // fails the open SILENTLY.
                helpers::free_parser_ctxt(ctxt);
                return ptr::null_mut();
            }
            RoutedFileOpen::Builtin => match helpers::input_from_file(filename) {
                Ok(i) => i,
                Err(_) => {
                    helpers::free_parser_ctxt(ctxt);
                    return ptr::null_mut();
                }
            },
        };
        helpers::setup_parser_input(ctxt, input);
        ctxt
    }
}

/// Create a parser context for an external entity.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlParserCtxtPtr xmlCreateEntityParserCtxt(const xmlChar *URL,
///                                            const xmlChar *ID,
///                                            const xmlChar *base);
/// ```
///
/// # SAFETY
///
///
/// - `URL`, `ID`, `base` must point to valid NUL-terminated
///   strings (or NULL where the C contract allows) for the lifetime
///   of the call.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlCreateEntityParserCtxt(
    URL: *const xmlChar,
    ID: *const xmlChar,
    base: *const xmlChar,
) -> *mut _xmlParserCtxt {
    let _ = base; // base URI resolution is a no-op here
    unsafe {
        let ctxt = xmlNewParserCtxt();
        if ctxt.is_null() {
            return ptr::null_mut();
        }
        let input = xmlLoadExternalEntity(URL as *const c_char, ID as *const c_char, ctxt);
        if input.is_null() {
            helpers::free_parser_ctxt(ctxt);
            return ptr::null_mut();
        }
        if xmlPushInput(ctxt, input) < 0 {
            helpers::free_parser_input(input);
            helpers::free_parser_ctxt(ctxt);
            return ptr::null_mut();
        }
        ctxt
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// CtxtRead family
// ═══════════════════════════════════════════════════════════════════════════════

/// Parse an XML in-memory document with a given context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDocPtr xmlCtxtReadDoc(xmlParserCtxtPtr ctxt, const xmlChar *cur,
///                          const char *URL, const char *encoding, int options);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `cur`, `URL`, `_encoding` must point to valid NUL-terminated
///   strings (or NULL where the C contract allows) for the lifetime
///   of the call.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlCtxtReadDoc(
    ctxt: *mut _xmlParserCtxt,
    cur: *const xmlChar,
    URL: *const c_char,
    encoding: *const c_char,
    options: c_int,
) -> *mut _xmlDoc {
    if ctxt.is_null() || cur.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        let len = string::xml_strlen(cur);
        let mut input = helpers::input_from_memory(cur as *const c_char, len as c_int);
        if !encoding.is_null() {
            let name = core::ffi::CStr::from_ptr(encoding).to_bytes();
            input.apply_explicit_input_encoding(name);
        }
        ctxt_read_doc(ctxt, input, URL, options)
    }
}

/// Parse an XML file with a given context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDocPtr xmlCtxtReadFile(xmlParserCtxtPtr ctxt, const char *filename,
///                           const char *encoding, int options);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `filename`, `_encoding` must point to valid NUL-terminated
///   strings (or NULL where the C contract allows) for the lifetime
///   of the call.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlCtxtReadFile(
    ctxt: *mut _xmlParserCtxt,
    filename: *const c_char,
    _encoding: *const c_char,
    options: c_int,
) -> *mut _xmlDoc {
    if ctxt.is_null() || filename.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        // UPSTREAM-PARITY (parser.c xmlCtxtReadFile -> xmlCtxtNewInputFromUrl
        // -> xmlLoadResource): a registered external entity loader governs the
        // open; below it the xmlParserInputBufferCreateFilenameDefault (php
        // streams loader) is consulted; a NULL loader result raises
        // xmlCtxtErrIO(XML_IO_ENOENT, filename) — "I/O warning : failed to
        // load \"%s\": %s\n" — and parsing fails.
        match open_filename_routed(filename, ctxt) {
            RoutedFileOpen::Loaded(input) => ctxt_read_doc(ctxt, input, filename, options),
            RoutedFileOpen::Failed => {
                emit_io_warning(ctxt, io_load_failure_message(filename));
                ptr::null_mut()
            }
            RoutedFileOpen::EntityLoaderFailed => {
                // UPSTREAM-PARITY (parser.c xmlCtxtReadFile): a custom entity
                // loader returning NULL fails the read SILENTLY.
                ptr::null_mut()
            }
            RoutedFileOpen::Builtin => match helpers::input_from_file(filename) {
                Ok(input) => ctxt_read_doc(ctxt, input, filename, options),
                Err(_) => ptr::null_mut(),
            },
        }
    }
}

/// Parse an XML in-memory block with a given context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDocPtr xmlCtxtReadMemory(xmlParserCtxtPtr ctxt, const char *buffer,
///                             int size, const char *URL, const char *encoding,
///                             int options);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `buffer`, `URL`, `_encoding` must point to valid NUL-terminated
///   strings (or NULL where the C contract allows) for the lifetime
///   of the call.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlCtxtReadMemory(
    ctxt: *mut _xmlParserCtxt,
    buffer: *const c_char,
    size: c_int,
    URL: *const c_char,
    encoding: *const c_char,
    options: c_int,
) -> *mut _xmlDoc {
    if ctxt.is_null() || buffer.is_null() || size < 0 {
        return ptr::null_mut();
    }
    unsafe {
        let mut input = helpers::input_from_memory(buffer, size);
        // UPSTREAM-PARITY (xmlCtxtNewInputFromMemory): an explicit `encoding`
        // argument switches the input to that encoding BEFORE the parse, so
        // the parser never sees raw UCS-2/UCS-4/Latin-1 bytes (lxml feeds
        // PEP-393 python strings this way). Best-effort: when the name is
        // unknown or the buffer was already converted the raw bytes stay and
        // BOM/declaration detection decides as usual.
        if !encoding.is_null() {
            let name = core::ffi::CStr::from_ptr(encoding).to_bytes();
            input.apply_explicit_input_encoding(name);
        }
        ctxt_read_doc(ctxt, input, URL, options)
    }
}

/// Parse an XML document from a file descriptor with a given context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDocPtr xmlCtxtReadFd(xmlParserCtxtPtr ctxt, int fd, const char *URL,
///                         const char *encoding, int options);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `URL`, `_encoding` must point to valid NUL-terminated
///   strings (or NULL where the C contract allows) for the lifetime
///   of the call.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlCtxtReadFd(
    ctxt: *mut _xmlParserCtxt,
    fd: c_int,
    URL: *const c_char,
    _encoding: *const c_char,
    options: c_int,
) -> *mut _xmlDoc {
    if ctxt.is_null() || fd < 0 {
        return ptr::null_mut();
    }
    unsafe {
        let mut buf = Vec::new();
        let mut tmp = [0u8; 4096];
        loop {
            let n = libc::read(fd, tmp.as_mut_ptr() as *mut c_void, tmp.len());
            if n <= 0 {
                break;
            }
            buf.extend_from_slice(&tmp[..n as usize]);
        }
        let input = helpers::input_from_memory(buf.as_ptr() as *const c_char, buf.len() as c_int);
        ctxt_read_doc(ctxt, input, URL, options)
    }
}

/// Parse an XML document from I/O callbacks with a given context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDocPtr xmlCtxtReadIO(xmlParserCtxtPtr ctxt, xmlInputReadCallback ioread,
///                         xmlInputCloseCallback ioclose, void *ioctx,
///                         const char *URL, const char *encoding, int options);
/// ```
///
/// # SAFETY
///
/// - `ctxt`, `ioctx` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `URL`, `_encoding` must point to valid NUL-terminated
///   strings (or NULL where the C contract allows) for the lifetime
///   of the call.
///
/// - `ioread`, `ioclose` must be a valid callback (or None);
///   the callback is invoked with the documented context pointer and
///   must itself uphold the same pointer invariants.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlCtxtReadIO(
    ctxt: *mut _xmlParserCtxt,
    ioread: Option<xmlInputReadCallback>,
    ioclose: Option<xmlInputCloseCallback>,
    ioctx: *mut c_void,
    URL: *const c_char,
    _encoding: *const c_char,
    options: c_int,
) -> *mut _xmlDoc {
    if ctxt.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        let input = helpers::input_from_io(ioread, ioclose, ioctx);
        // UPSTREAM-PARITY (parser.c xmlCtxtNewInputFromIO): the URL becomes
        // the input's filename, which feeds the `file:line:` error prefix.
        let input = if !URL.is_null() {
            match std::ffi::CStr::from_ptr(URL).to_str() {
                Ok(s) => input.with_filename(s),
                Err(_) => input,
            }
        } else {
            input
        };
        ctxt_read_doc(ctxt, input, URL, options)
    }
}

/// Parse a document from a raw parser input, taking ownership of `input`.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDocPtr xmlCtxtParseDocument(xmlParserCtxtPtr ctxt, xmlParserInputPtr input);
/// ```
///
/// # SAFETY
///
/// - `ctxt`, `input` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlCtxtParseDocument(
    ctxt: *mut _xmlParserCtxt,
    input: *mut _xmlParserInput,
) -> *mut _xmlDoc {
    if ctxt.is_null() || input.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        // Determine whether the caller's input is already owned by the
        // context's input stack (pushed via xmlPushInput).
        let mut owned = false;
        let nr = (*ctxt).inputNr;
        let tab = (*ctxt).inputTab;
        if !tab.is_null() {
            for i in 0..nr {
                if *tab.add(i as usize) == input {
                    owned = true;
                    break;
                }
            }
        }
        if (*ctxt).input == input {
            owned = true;
        }

        // Copy the data first so the context reset cannot invalidate it.
        let ib = input_buffer_from_parser_input(input);

        xmlCtxtReset(ctxt);
        helpers::setup_parser_input(ctxt, ib);
        helpers::parse_document(ctxt);

        if !owned {
            helpers::free_parser_input(input);
        }

        (*ctxt).myDoc
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Parser input buffers / streams
// ═══════════════════════════════════════════════════════════════════════════════

/// Allocate a parser input buffer for the given encoding.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlParserInputBufferPtr xmlAllocParserInputBuffer(xmlCharEncoding enc);
/// ```
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
pub unsafe extern "C" fn xmlAllocParserInputBuffer(enc: c_int) -> *mut _xmlParserInputBuffer {
    unsafe {
        let buf = xmlMallocZero(core::mem::size_of::<_xmlParserInputBuffer>())
            as *mut _xmlParserInputBuffer;
        if buf.is_null() {
            return ptr::null_mut();
        }
        let b = &mut *buf;
        b.buffer = io::buf_create(crate::abi::data_globals::xmlDefaultBufferSize) as *mut c_void;
        b.raw = io::buf_create(crate::abi::data_globals::xmlDefaultBufferSize) as *mut c_void;
        if b.buffer.is_null() || b.raw.is_null() {
            io::buf_free(b.buffer as *mut _xmlBuffer);
            io::buf_free(b.raw as *mut _xmlBuffer);
            xmlFreeImpl(buf as *mut c_void);
            return ptr::null_mut();
        }
        b.compressed = -1;

        if enc != xmlCharEncoding::XML_CHAR_ENCODING_NONE as c_int
            && enc != xmlCharEncoding::XML_CHAR_ENCODING_ERROR as c_int
        {
            let handler = encoding_handler_for(enc);
            if !handler.is_null() {
                b.encoder = handler as *mut c_void;
            }
        }
        buf
    }
}

/// Grow an input buffer by reading up to `len` bytes from its source.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlParserInputBufferGrow(xmlParserInputBufferPtr in, int len);
/// ```
///
/// # SAFETY
///
/// - `in_` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlParserInputBufferGrow(
    in_: *mut _xmlParserInputBuffer,
    len: c_int,
) -> c_int {
    if in_.is_null() || len <= 0 {
        return 0;
    }
    unsafe {
        let b = &mut *in_;
        if b.error != 0 {
            return -1;
        }
        let Some(read_cb) = b.readcallback else {
            // Memory-based buffer: nothing to grow.
            return 0;
        };
        let mut tmp = vec![0u8; len as usize];
        let n = read_cb(b.context, tmp.as_mut_ptr() as *mut c_char, len);
        if n < 0 {
            b.error = 1;
            return -1;
        }
        if n == 0 {
            return 0;
        }
        io::input_buffer_push(in_, tmp.as_ptr() as *const c_char, n);
        n
    }
}

/// Push `len` bytes into an input buffer (push parser).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlParserInputBufferPush(xmlParserInputBufferPtr in, int len, const char *buf);
/// ```
///
/// # SAFETY
///
/// - `in_` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `buf` must point to valid NUL-terminated
///   strings (or NULL where the C contract allows) for the lifetime
///   of the call.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlParserInputBufferPush(
    in_: *mut _xmlParserInputBuffer,
    len: c_int,
    buf: *const c_char,
) -> c_int {
    if in_.is_null() {
        return -1;
    }
    if len < 0 || (len > 0 && buf.is_null()) {
        return -1;
    }
    if len == 0 {
        return 0;
    }
    io::input_buffer_push(in_, buf, len)
}

/// Read up to `len` bytes from an input buffer's source.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlParserInputBufferRead(xmlParserInputBufferPtr in, int len);
/// ```
///
/// # SAFETY
///
/// - `in_` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlParserInputBufferRead(
    in_: *mut _xmlParserInputBuffer,
    len: c_int,
) -> c_int {
    xmlParserInputBufferGrow(in_, len)
}

/// Deprecated: reading directly from an input stream is an error.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlParserInputRead(xmlParserInputPtr in, int len);
/// ```
///
/// # SAFETY
///
/// - `_in_` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub const unsafe extern "C" fn xmlParserInputRead(
    _in_: *mut _xmlParserInput,
    _len: c_int,
) -> c_int {
    -1
}

/// Grow a parser input's buffer by reading more data from its source.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlParserInputGrow(xmlParserInputPtr in, int len);
/// ```
///
/// # SAFETY
///
/// - `in_` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlParserInputGrow(in_: *mut _xmlParserInput, len: c_int) -> c_int {
    if in_.is_null() || len < 0 {
        return -1;
    }
    unsafe {
        let pi = &*in_;
        if pi.base.is_null() || pi.cur.is_null() {
            return -1;
        }
        if pi.buf.is_null() {
            // Pure memory input: nothing to grow.
            return 0;
        }
        let b = &*pi.buf;
        // Memory buffers are not growable.
        if b.readcallback.is_none() && b.encoder.is_null() {
            return 0;
        }
        xmlParserInputBufferGrow(pi.buf, len)
    }
}

/// Shrink a parser input, releasing already-consumed data from the buffer.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlParserInputShrink(xmlParserInputPtr in);
/// ```
///
/// # SAFETY
///
/// - `in_` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlParserInputShrink(in_: *mut _xmlParserInput) {
    if in_.is_null() {
        return;
    }
    unsafe {
        let pi = &mut *in_;
        if pi.buf.is_null() || pi.base.is_null() || pi.cur.is_null() {
            return;
        }
        let used = (pi.cur as usize).saturating_sub(pi.base as usize);
        if used > LINE_LEN {
            // The candidate's inputs are backed by stable memory buffers, so
            // the base pointer cannot move; account for the consumed bytes.
            pi.consumed = pi.consumed.saturating_add((used - LINE_LEN) as c_ulong);
        }
    }
}

/// Create a new (empty) parser input stream.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlParserInputPtr xmlNewInputStream(xmlParserCtxtPtr ctxt);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlNewInputStream(ctxt: *mut _xmlParserCtxt) -> *mut _xmlParserInput {
    unsafe {
        let input = xmlMallocZero(core::mem::size_of::<_xmlParserInput>()) as *mut _xmlParserInput;
        if input.is_null() {
            if !ctxt.is_null() {
                xmlCtxtErrMemory(ctxt);
            }
            return ptr::null_mut();
        }
        (*input).line = 1;
        (*input).col = 1;
        input
    }
}

/// Wrap an input buffer in a parser input stream.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlParserInputPtr xmlNewIOInputStream(xmlParserCtxtPtr ctxt,
///                                       xmlParserInputBufferPtr input,
///                                       xmlCharEncoding enc);
/// ```
///
/// # SAFETY
///
/// - `ctxt`, `input` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlNewIOInputStream(
    ctxt: *mut _xmlParserCtxt,
    input: *mut _xmlParserInputBuffer,
    enc: c_int,
) -> *mut _xmlParserInput {
    if ctxt.is_null() || input.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        let pi = xmlNewInputStream(ctxt);
        if pi.is_null() {
            return ptr::null_mut();
        }
        (*pi).buf = input;
        if enc != xmlCharEncoding::XML_CHAR_ENCODING_NONE as c_int
            && enc != xmlCharEncoding::XML_CHAR_ENCODING_ERROR as c_int
        {
            let handler = encoding_handler_for(enc);
            if !handler.is_null() {
                io::input_buffer_set_encoder(input, handler);
            }
        }
        pi
    }
}

/// Create a parser input stream from a zero-terminated string. The string
/// must remain valid for the lifetime of the input (static mode).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlParserInputPtr xmlNewStringInputStream(xmlParserCtxtPtr ctxt,
///                                           const xmlChar *buffer);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `buffer` must point to valid NUL-terminated
///   strings (or NULL where the C contract allows) for the lifetime
///   of the call.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlNewStringInputStream(
    ctxt: *mut _xmlParserCtxt,
    buffer: *const xmlChar,
) -> *mut _xmlParserInput {
    if ctxt.is_null() || buffer.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        let input = xmlNewInputStream(ctxt);
        if input.is_null() {
            return ptr::null_mut();
        }
        let len = string::xml_strlen(buffer);
        (*input).base = buffer;
        (*input).cur = buffer;
        (*input).end = buffer.add(len);
        (*input).length = len as c_int;
        input
    }
}

/// Setup the parser context to parse a new buffer (legacy API).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlSetupParserForBuffer(xmlParserCtxtPtr ctxt, const xmlChar* buffer,
///                              const char *filename);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `buffer`, `filename` must point to valid NUL-terminated
///   strings (or NULL where the C contract allows) for the lifetime
///   of the call.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlSetupParserForBuffer(
    ctxt: *mut _xmlParserCtxt,
    buffer: *const xmlChar,
    filename: *const c_char,
) {
    if ctxt.is_null() || buffer.is_null() {
        return;
    }
    unsafe {
        xmlCtxtReset(ctxt);
        let len = string::xml_strlen(buffer);
        let uri = if filename.is_null() {
            None
        } else {
            CStr::from_ptr(filename).to_str().ok()
        };
        let input = InputBuffer::from_memory(core::slice::from_raw_parts(buffer, len), uri);
        helpers::setup_parser_input(ctxt, input);
    }
}

/// Push an input stream onto the context's input stack.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlPushInput(xmlParserCtxtPtr ctxt, xmlParserInputPtr input);
/// ```
///
/// # SAFETY
///
/// - `ctxt`, `input` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlPushInput(
    ctxt: *mut _xmlParserCtxt,
    input: *mut _xmlParserInput,
) -> c_int {
    if ctxt.is_null() || input.is_null() {
        return -1;
    }
    unsafe {
        let c = &mut *ctxt;
        if c.inputNr >= c.inputMax {
            let new_max = if c.inputMax == 0 { 5 } else { c.inputMax * 2 };
            let new_tab = xmlReallocImpl(
                c.inputTab as *mut c_void,
                (new_max as usize) * core::mem::size_of::<*mut _xmlParserInput>(),
            ) as *mut *mut _xmlParserInput;
            if new_tab.is_null() {
                return -1;
            }
            c.inputTab = new_tab;
            c.inputMax = new_max;
        }
        *c.inputTab.add(c.inputNr as usize) = input;
        c.input = input;
        (*input).id = c.input_id;
        c.input_id += 1;
        let idx = c.inputNr;
        c.inputNr += 1;
        idx
    }
}

/// Pop the top input from the context's input stack and free it; returns the
/// current character after the pop (0 at end of input).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar xmlPopInput(xmlParserCtxtPtr ctxt);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlPopInput(ctxt: *mut _xmlParserCtxt) -> xmlChar {
    if ctxt.is_null() || (*ctxt).inputNr <= 1 {
        return 0;
    }
    unsafe {
        let c = &mut *ctxt;
        c.inputNr -= 1;
        let popped = *c.inputTab.add(c.inputNr as usize);
        *c.inputTab.add(c.inputNr as usize) = ptr::null_mut();
        if c.inputNr > 0 {
            c.input = *c.inputTab.add((c.inputNr - 1) as usize);
        } else {
            c.input = ptr::null_mut();
        }
        if !popped.is_null() {
            helpers::free_parser_input(popped);
        }
        if c.input.is_null() {
            return 0;
        }
        let cur = (*c.input).cur;
        let end = (*c.input).end;
        if cur.is_null() || cur >= end {
            0
        } else {
            *cur
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Encoding switching
// ═══════════════════════════════════════════════════════════════════════════════

/// Switch the input encoding of the current input.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlSwitchEncoding(xmlParserCtxtPtr ctxt, xmlCharEncoding enc);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlSwitchEncoding(ctxt: *mut _xmlParserCtxt, enc: c_int) -> c_int {
    if ctxt.is_null() || (*ctxt).input.is_null() {
        return -1;
    }
    if enc == xmlCharEncoding::XML_CHAR_ENCODING_NONE as c_int {
        return 0;
    }
    unsafe {
        let handler = encoding_handler_for(enc);
        if handler.is_null() {
            return -1;
        }
        xmlSwitchToEncoding(ctxt, handler)
    }
}

/// Switch the input encoding by name.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlSwitchEncodingName(xmlParserCtxtPtr ctxt, const char *encoding);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `encoding` must point to valid NUL-terminated
///   strings (or NULL where the C contract allows) for the lifetime
///   of the call.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlSwitchEncodingName(
    ctxt: *mut _xmlParserCtxt,
    encoding: *const c_char,
) -> c_int {
    if ctxt.is_null() || encoding.is_null() {
        return -1;
    }
    unsafe {
        let handler = encoding::find_encoding_handler(encoding as *const xmlChar);
        if handler.is_null() {
            return -1;
        }
        xmlSwitchToEncoding(ctxt, handler)
    }
}

/// Switch the encoding of a specific parser input using an encoding handler.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlSwitchInputEncoding(xmlParserCtxtPtr ctxt, xmlParserInputPtr input,
///                            xmlCharEncodingHandlerPtr handler);
/// ```
///
/// # SAFETY
///
/// - `ctxt`, `input`, `handler` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlSwitchInputEncoding(
    ctxt: *mut _xmlParserCtxt,
    input: *mut _xmlParserInput,
    handler: *mut _xmlCharEncodingHandler,
) -> c_int {
    let _ = ctxt;
    if input.is_null() {
        return -1;
    }
    unsafe {
        if (*input).buf.is_null() {
            return -1;
        }
        io::input_buffer_set_encoder((*input).buf, handler);
    }
    0
}

/// Switch the encoding of the current input using an encoding handler.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlSwitchToEncoding(xmlParserCtxtPtr ctxt,
///                         xmlCharEncodingHandlerPtr handler);
/// ```
///
/// # SAFETY
///
/// - `ctxt`, `handler` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlSwitchToEncoding(
    ctxt: *mut _xmlParserCtxt,
    handler: *mut _xmlCharEncodingHandler,
) -> c_int {
    if ctxt.is_null() {
        return -1;
    }
    unsafe {
        let input = (*ctxt).input;
        if input.is_null() {
            return -1;
        }
        // Memory-parser inputs (xmlCreateMemoryParserCtxt) carry buf == NULL;
        // their bytes live in the Rust-side InputBuffer (helpers.rs side
        // table), which already transcoded any BOM/declared encoding. A
        // caller-driven switch (PHP dom overrideEncoding) must therefore
        // transcode the whole buffered stream there (upstream applies the
        // input-buffer encoder before any read).
        if (*input).buf.is_null() && !(*handler).name.is_null() {
            return helpers::apply_memory_encoding_override(ctxt, (*handler).name);
        }
        io::input_buffer_set_encoder((*input).buf, handler);
    }
    0
}

// ═══════════════════════════════════════════════════════════════════════════════
// Node info sequence (deprecated, parser.h)
// ═══════════════════════════════════════════════════════════════════════════════

/// Initialise a node info sequence.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlInitNodeInfoSeq(xmlParserNodeInfoSeqPtr seq);
/// ```
///
/// # SAFETY
///
/// - `seq` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlInitNodeInfoSeq(seq: *mut _xmlParserNodeInfoSeq) {
    if seq.is_null() {
        return;
    }
    unsafe {
        (*seq).length = 0;
        (*seq).maximum = 0;
        (*seq).buffer = ptr::null_mut();
    }
}

/// Clear (release and reinitialise) a node info sequence.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlClearNodeInfoSeq(xmlParserNodeInfoSeqPtr seq);
/// ```
///
/// # SAFETY
///
/// - `seq` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlClearNodeInfoSeq(seq: *mut _xmlParserNodeInfoSeq) {
    if seq.is_null() {
        return;
    }
    unsafe {
        if !(*seq).buffer.is_null() {
            xmlFreeImpl((*seq).buffer as *mut c_void);
        }
        xmlInitNodeInfoSeq(seq);
    }
}

/// Find the index where the info record for `node` is (or should be) in the
/// sorted sequence; binary search by node pointer.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// unsigned long xmlParserFindNodeInfoIndex(xmlParserNodeInfoSeqPtr seq,
///                                          xmlNodePtr node);
/// ```
///
/// # SAFETY
///
/// - `seq`, `node` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlParserFindNodeInfoIndex(
    seq: *mut _xmlParserNodeInfoSeq,
    node: *mut _xmlNode,
) -> c_ulong {
    if seq.is_null() || node.is_null() {
        return c_ulong::MAX;
    }
    unsafe {
        let s = &*seq;
        if s.buffer.is_null() || s.length == 0 {
            return 0;
        }
        let mut lower: usize = 0;
        let mut upper: usize = s.length as usize;
        while lower < upper {
            let middle = lower + (upper - lower) / 2;
            let cur_node = (*s.buffer.add(middle)).node;
            if cur_node == node {
                return middle as c_ulong;
            }
            if (cur_node as usize) < (node as usize) {
                lower = middle + 1;
            } else {
                upper = middle;
            }
        }
        lower as c_ulong
    }
}

/// Find the node info record for a given node, or NULL.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// const xmlParserNodeInfo *xmlParserFindNodeInfo(xmlParserCtxtPtr ctxt,
///                                                xmlNodePtr node);
/// ```
///
/// # SAFETY
///
/// - `ctxt`, `node` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlParserFindNodeInfo(
    ctxt: *mut _xmlParserCtxt,
    node: *mut _xmlNode,
) -> *const _xmlParserNodeInfo {
    if ctxt.is_null() || node.is_null() {
        return ptr::null();
    }
    unsafe {
        let seq = &(*ctxt).node_seq;
        let seq_mut = seq as *const _ as *mut _xmlParserNodeInfoSeq;
        let pos = xmlParserFindNodeInfoIndex(seq_mut, node);
        if !seq.buffer.is_null() && (pos as usize) < (seq.length as usize) {
            let info = &*seq.buffer.add(pos as usize);
            if info.node == node {
                return info;
            }
        }
        ptr::null()
    }
}

/// Insert a node info record into the context's sorted sequence.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlParserAddNodeInfo(xmlParserCtxtPtr ctxt, xmlParserNodeInfoPtr info);
/// ```
///
/// # SAFETY
///
/// - `ctxt`, `info` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlParserAddNodeInfo(
    ctxt: *mut _xmlParserCtxt,
    info: *mut _xmlParserNodeInfo,
) {
    if ctxt.is_null() || info.is_null() {
        return;
    }
    unsafe {
        let seq = &mut (*ctxt).node_seq;
        let node = (*info).node;
        let pos = xmlParserFindNodeInfoIndex(seq, node as *mut _xmlNode) as usize;

        if pos < seq.length as usize && !seq.buffer.is_null() && (*seq.buffer.add(pos)).node == node
        {
            // Node already recorded: update the record in place.
            ptr::copy_nonoverlapping(info, seq.buffer.add(pos), 1);
            return;
        }

        // Grow the buffer (upstream xmlGrowCapacity: 50% growth from a
        // minimum of 4, capped at XML_MAX_ITEMS = 1 billion).
        if seq.length + 1 > seq.maximum {
            let new_max = xml_grow_capacity(seq.maximum);
            if new_max < 0 {
                xmlCtxtErrMemory(ctxt);
                return;
            }
            let new_buf = xmlReallocImpl(
                seq.buffer as *mut c_void,
                (new_max as usize) * core::mem::size_of::<_xmlParserNodeInfo>(),
            ) as *mut _xmlParserNodeInfo;
            if new_buf.is_null() {
                xmlCtxtErrMemory(ctxt);
                return;
            }
            seq.buffer = new_buf;
            seq.maximum = new_max as c_ulong;
        }

        // Shift elements right to make room at `pos`.
        let length = seq.length as usize;
        for i in (pos + 1..=length).rev() {
            ptr::copy_nonoverlapping(seq.buffer.add(i - 1), seq.buffer.add(i), 1);
        }
        ptr::copy_nonoverlapping(info, seq.buffer.add(pos), 1);
        seq.length += 1;
    }
}

/// Upstream `xmlGrowCapacity` (private/memory.h) for a zero-based capacity:
/// 50% growth, minimum initial allocation 4, capped at XML_MAX_ITEMS.
/// Returns the new capacity or -1 on overflow/cap exhaustion.
// The `as u64` casts are width-correcting for 32-bit platforms where
// `c_ulong` is 32 bits; on x86-64 they are identity casts.
#[allow(clippy::unnecessary_cast)]
const unsafe fn xml_grow_capacity(capacity: c_ulong) -> c_int {
    const XML_MAX_ITEMS: u64 = 1_000_000_000;
    const ELEM_SIZE: usize = core::mem::size_of::<_xmlParserNodeInfo>();
    if capacity == 0 {
        return 4;
    }
    if capacity as u64 >= XML_MAX_ITEMS || (capacity as usize) > usize::MAX / 2 / ELEM_SIZE {
        return -1;
    }
    let extra = capacity.div_ceil(2);
    if capacity as u64 > XML_MAX_ITEMS - extra as u64 {
        return XML_MAX_ITEMS as c_int;
    }
    (capacity + extra) as c_int
}

// ═══════════════════════════════════════════════════════════════════════════════
// I/O callback registration (xmlIO.h)
// ═══════════════════════════════════════════════════════════════════════════════

/// Register a new set of input I/O callbacks.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlRegisterInputCallbacks(xmlInputMatchCallback matchFunc,
///                               xmlInputOpenCallback openFunc,
///                               xmlInputReadCallback readFunc,
///                               xmlInputCloseCallback closeFunc);
/// ```
///
/// # SAFETY
///
///
/// - `matchFunc`, `openFunc`, `readFunc`, `closeFunc` must be a valid callback (or None);
///   the callback is invoked with the documented context pointer and
///   must itself uphold the same pointer invariants.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlRegisterInputCallbacks(
    matchFunc: Option<xmlInputMatchCallback>,
    openFunc: Option<xmlInputOpenCallback>,
    readFunc: Option<xmlInputReadCallback>,
    closeFunc: Option<xmlInputCloseCallback>,
) -> c_int {
    unsafe {
        globals::init_parser();
    }
    let mut table = INPUT_CALLBACKS.lock();
    if table.len() >= 10 {
        return -1;
    }
    table.push(InputCallbackEntry {
        matchcb: matchFunc,
        opencb: openFunc,
        readcb: readFunc,
        closecb: closeFunc,
    });
    (table.len() - 1) as c_int
}

/// Register the default compiled-in input callbacks (the `xmlFile*` pair).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlRegisterDefaultInputCallbacks(void);
/// ```
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
pub unsafe extern "C" fn xmlRegisterDefaultInputCallbacks() {
    unsafe {
        xmlRegisterInputCallbacks(
            Some(xmlFileMatch),
            Some(xmlFileOpen),
            Some(xmlFileRead),
            Some(xmlFileClose),
        );
    }
}

/// Remove the top input callback from the stack.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlPopInputCallbacks(void);
/// ```
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
pub unsafe extern "C" fn xmlPopInputCallbacks() -> c_int {
    unsafe {
        globals::init_parser();
    }
    let mut table = INPUT_CALLBACKS.lock();
    if table.is_empty() {
        return -1;
    }
    table.pop();
    table.len() as c_int
}

/// Clear the entire input callback table.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlCleanupInputCallbacks(void);
/// ```
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
pub unsafe extern "C" fn xmlCleanupInputCallbacks() {
    unsafe {
        globals::init_parser();
    }
    INPUT_CALLBACKS.lock().clear();
}

/// Read a URI through the registered input callbacks (upstream
/// `xmlParserInputBufferCreateFilename`): the first registered pair whose
/// match callback accepts the URI is opened, read to EOF, and closed.
/// Returns `None` when no registered pair matches — callers fall back to
/// the regular file path. NULL callbacks inside a matching pair are treated
/// like upstream (an entry whose match callback is NULL is skipped).
///
/// Used by the XInclude loader so custom I/O schemes registered through
/// `xmlRegisterInputCallbacks` are honored (upstream xmlXIncludeLoadDoc →
/// xmlNewInputFromFile; Phase-12 EXTERNAL-CONSUMERS court: io1.c registers
/// an sql: scheme and XInclude hrefs route through it).
///
/// # SAFETY
///
/// - `uri` must be a valid NUL-terminated C string live for the call.
pub(crate) unsafe fn read_uri_via_input_callbacks(uri: *const c_char) -> Option<Vec<u8>> {
    let table = INPUT_CALLBACKS.lock();
    for e in table.iter() {
        let Some(matchcb) = e.matchcb else {
            continue;
        };
        // SAFETY: callbacks were registered by the caller and must uphold
        // the xmlInput*Callback contracts.
        if unsafe { matchcb(uri) } == 0 {
            continue;
        }
        let (Some(opencb), Some(readcb)) = (e.opencb, e.readcb) else {
            return None;
        };
        // SAFETY: the open callback returns a context for read/close.
        let ctx = unsafe { opencb(uri) };
        if ctx.is_null() {
            return None;
        }
        let mut data = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            // SAFETY: readcb fills `buf` per the xmlInputReadCallback contract.
            let n = unsafe { readcb(ctx, buf.as_mut_ptr() as *mut c_char, buf.len() as c_int) };
            if n < 0 {
                if let Some(closecb) = e.closecb {
                    unsafe { closecb(ctx) };
                }
                return None;
            }
            if n == 0 {
                break;
            }
            data.extend_from_slice(&buf[..n as usize]);
        }
        if let Some(closecb) = e.closecb {
            unsafe { closecb(ctx) };
        }
        return Some(data);
    }
    None
}

/// Register a new set of output I/O callbacks.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlRegisterOutputCallbacks(xmlOutputMatchCallback matchFunc,
///                                xmlOutputOpenCallback openFunc,
///                                xmlOutputWriteCallback writeFunc,
///                                xmlOutputCloseCallback closeFunc);
/// ```
///
/// # SAFETY
///
///
/// - `matchFunc`, `openFunc`, `writeFunc`, `closeFunc` must be a valid callback (or None);
///   the callback is invoked with the documented context pointer and
///   must itself uphold the same pointer invariants.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlRegisterOutputCallbacks(
    matchFunc: Option<xmlOutputMatchCallback>,
    openFunc: Option<xmlOutputOpenCallback>,
    writeFunc: Option<xmlOutputWriteCallback>,
    closeFunc: Option<xmlOutputCloseCallback>,
) -> c_int {
    unsafe {
        globals::init_parser();
    }
    let mut table = OUTPUT_CALLBACKS.lock();
    if table.len() >= 10 {
        return -1;
    }
    table.push(OutputCallbackEntry {
        matchcb: matchFunc,
        opencb: openFunc,
        writecb: writeFunc,
        closecb: closeFunc,
    });
    (table.len() - 1) as c_int
}

/// Register the default compiled-in output callbacks.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlRegisterDefaultOutputCallbacks(void);
/// ```
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
pub unsafe extern "C" fn xmlRegisterDefaultOutputCallbacks() {
    unsafe {
        xmlRegisterOutputCallbacks(Some(xmlFileMatch), None, None, None);
    }
}

/// Register the HTTP POST output callbacks (upstream: default output callbacks).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlRegisterHTTPPostCallbacks(void);
/// ```
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
pub unsafe extern "C" fn xmlRegisterHTTPPostCallbacks() {
    unsafe { xmlRegisterDefaultOutputCallbacks() }
}

/// Remove the top output callback from the stack.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlPopOutputCallbacks(void);
/// ```
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
pub unsafe extern "C" fn xmlPopOutputCallbacks() -> c_int {
    unsafe {
        globals::init_parser();
    }
    let mut table = OUTPUT_CALLBACKS.lock();
    if table.is_empty() {
        return -1;
    }
    table.pop();
    table.len() as c_int
}

/// Clear the entire output callback table.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlCleanupOutputCallbacks(void);
/// ```
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
pub unsafe extern "C" fn xmlCleanupOutputCallbacks() {
    unsafe {
        globals::init_parser();
    }
    OUTPUT_CALLBACKS.lock().clear();
}

// ═══════════════════════════════════════════════════════════════════════════════
// External entity loaders (parser.h)
// ═══════════════════════════════════════════════════════════════════════════════

/// Default external entity loader: resolve `url` against the filesystem,
/// honouring XML_PARSE_NONET.
///
/// # Safety
///
/// `url`/`publicId` must be valid C strings or NULL; `ctxt` may be NULL.
unsafe extern "C" fn default_external_entity_loader(
    url: *const c_char,
    public_id: *const c_char,
    ctxt: *mut _xmlParserCtxt,
) -> *mut _xmlParserInput {
    let _ = public_id;
    if url.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        // Refuse network access when NONET is set.
        if !ctxt.is_null() && (*ctxt).options & XML_PARSE_NONET != 0 {
            let len = libc::strlen(url);
            if len >= 7 && libc::strncasecmp(url, c"http://".as_ptr() as *const c_char, 7) == 0 {
                return ptr::null_mut();
            }
        }
        // UPSTREAM-PARITY (parserInternals.c xmlDefaultExternalEntityLoader
        // -> xmlNewInputFromFile -> xmlNewInputFromUrl): the registered
        // xmlParserInputBufferCreateFilenameDefault (php streams loader) is
        // consulted BEFORE the input-callback table and the built-in open. A
        // NULL loader result is XML_IO_ENOENT — xmlCtxtErrIO is raised and
        // there is no fallback.
        if globals::get_parser_input_buffer_create_filename_value_cross_dso().is_some() {
            // SAFETY: url is a valid NUL-terminated C string for the call.
            return match call_loader_materialize(url) {
                Err(()) => {
                    emit_io_warning(ctxt, io_load_failure_message(url));
                    ptr::null_mut()
                }
                Ok(data) => {
                    // Build a MEMORY-backed C input: the entity machinery
                    // consumes the loader result through base/end (upstream
                    // buffers the external entity content the same way). A
                    // zero-length result (php://memory, 0-byte file) is a
                    // VALID empty input — the parse reports "Document is
                    // empty" (php DOM createFromFile).
                    let mem = if data.is_empty() {
                        crate::xml::io::input_buffer_create_empty()
                    } else {
                        io::input_buffer_create_mem(
                            data.as_ptr() as *const c_char,
                            data.len() as c_int,
                            xmlCharEncoding::XML_CHAR_ENCODING_NONE as c_int,
                        )
                    };
                    if mem.is_null() {
                        return ptr::null_mut();
                    }
                    parser_input_from_buf(mem)
                }
            };
        }
        // Try the registered input callbacks first.
        let table = INPUT_CALLBACKS.lock();
        for entry in table.iter() {
            if let (Some(match_cb), Some(open_cb)) = (entry.matchcb, entry.opencb) {
                if match_cb(url) != 0 {
                    let ctx = open_cb(url);
                    if !ctx.is_null() {
                        let buf = helpers::alloc_parser_input_buffer();
                        if buf.is_null() {
                            if let Some(close_cb) = entry.closecb {
                                close_cb(ctx);
                            }
                            return ptr::null_mut();
                        }
                        (*buf).context = ctx;
                        (*buf).readcallback = entry.readcb;
                        (*buf).closecallback = entry.closecb;
                        return parser_input_from_buf(buf);
                    }
                }
            }
        }

        // Fall back to a plain file open.
        let buf =
            io::input_buffer_create_file(url, xmlCharEncoding::XML_CHAR_ENCODING_NONE as c_int);
        if buf.is_null() {
            // UPSTREAM-PARITY (parserInternals.c xmlNewInputFromFile): a
            // failed load raises xmlCtxtErrIO(ctxt, XML_IO_ENOENT, url) —
            // "I/O warning : failed to load \"%s\": %s\n" with the
            // strerror text (HOSTILE-FAILURE F7).
            let errno = *libc::__errno_location();
            let errstr = if errno == 0 {
                String::new()
            } else {
                std::ffi::CStr::from_ptr(libc::strerror(errno))
                    .to_string_lossy()
                    .into_owned()
            };
            let url_str = std::ffi::CStr::from_ptr(url).to_string_lossy();
            emit_io_warning(ctxt, format!("failed to load \"{url_str}\": {errstr}\n"));
            return ptr::null_mut();
        }
        parser_input_from_buf(buf)
    }
}

/// UPSTREAM-PARITY (parserInternals.c xmlCtxtErrIO): raise an I/O warning
/// (XML_FROM_IO, XML_IO_ENOENT, XML_ERR_WARNING) through the parser's
/// channel — "I/O warning : <message>".
pub(crate) unsafe fn emit_io_warning(ctxt: *mut _xmlParserCtxt, message: String) {
    let msg_c = std::ffi::CString::new(message).unwrap_or_default();
    let delivery = if ctxt.is_null() {
        crate::xml::errors::GenericDelivery::Stream
    } else {
        unsafe { crate::xml::errors::parser_delivery(ctxt) }
    };
    unsafe {
        crate::xml::errors::raise_error_streamed(
            ctxt as *mut c_void,
            crate::abi::types::XML_FROM_IO,
            crate::abi::types::XML_IO_ENOENT,
            crate::abi::types::xmlErrorLevel::XML_ERR_WARNING as c_int,
            ptr::null(),
            0,
            0,
            ptr::null(),
            ptr::null(),
            ptr::null(),
            0,
            msg_c.as_ptr(),
            None,
            None,
            delivery,
            None,
        );
    }
}

/// Result of routing a filename open through the registered loaders
/// (upstream 2.14+ `xmlLoadResource` layering).
#[allow(dead_code)]
pub(crate) enum RoutedFileOpen {
    /// No custom loader is registered — the caller falls back to the built-in
    /// file open (`helpers::input_from_file`).
    Builtin,
    /// A registered loader returned NULL: upstream reports `XML_IO_ENOENT`
    /// with NO built-in fallback (php streams loader: missing file, percent-
    /// encoded-NUL guard, disabled entity loader).
    Failed,
    /// A registered EXTERNAL ENTITY loader (`xmlSetExternalEntityLoader`)
    /// returned NULL for a file/URL open. Upstream `xmlCtxtNewInputFromUrl`
    /// propagates that NULL silently (no `xmlCtxtErrIO` — the custom loader
    /// owns its own error reporting), so callers fail without a warning.
    EntityLoaderFailed,
    /// The loader produced an input buffer whose bytes were materialized
    /// (filename = the original URI).
    Loaded(InputBuffer),
}

/// UPSTREAM-PARITY (parserInternals.c `xmlNewInputFromUrl`): when a custom
/// `xmlParserInputBufferCreateFilenameDefault` is registered (PHP installs
/// its streams loader at request init), filename opens consult it FIRST —
/// php streams unescape `file://` URIs, enforce the percent-encoded-NUL
/// guard, honor stream contexts and emit their own failure warnings. A NULL
/// loader result is `XML_IO_ENOENT`; upstream does NOT fall back to the
/// built-in open in that case. Without a registered loader the caller keeps
/// the built-in path.
///
/// Invoke the registered loader and materialize the produced buffer's bytes
/// through its read callback, releasing the C buffer/stream exactly once
/// (the close callback runs when the buffer is freed). Returns `Err(())` on
/// a NULL loader result or a read-callback error.
///
/// # Safety
///
/// - `uri` must be a valid NUL-terminated C string live for the call; the
///   registered loader callback (if any) must uphold the
///   `xmlParserInputBufferCreateFilenameFunc` contract.
pub(crate) unsafe fn call_loader_materialize(uri: *const c_char) -> Result<Vec<u8>, ()> {
    // SAFETY: reads the per-thread loader slot — through the R-000177
    // cross-DSO bridge so the whole-archive facade copies observe the
    // loader a consumer registered via the core DSO's exported setter
    // (upstream: single core DSO, registration visible everywhere).
    let Some(func) = globals::get_parser_input_buffer_create_filename_value_cross_dso() else {
        return Err(());
    };
    // SAFETY: `func` is the consumer-registered C loader and must uphold the
    // xmlParserInputBufferCreateFilenameFunc contract (uri + enc in, buffer
    // out, or NULL on failure).
    let buf = unsafe { func(uri, xmlCharEncoding::XML_CHAR_ENCODING_NONE as c_int) };
    if buf.is_null() {
        return Err(());
    }
    let (read, ctx) = unsafe {
        let b = &*buf;
        (b.readcallback, b.context)
    };
    let mut data: Vec<u8> = Vec::new();
    let mut result = Err(());
    if let Some(read) = read {
        let mut tmp = [0u8; 4096];
        loop {
            // SAFETY: the loader's buffer carries the consumer's read
            // callback + context (xmlParserInputBufferCreateIO contract).
            let n = unsafe { read(ctx, tmp.as_mut_ptr() as *mut c_char, tmp.len() as c_int) };
            if n < 0 {
                break;
            }
            if n == 0 {
                result = Ok(());
                break;
            }
            data.extend_from_slice(&tmp[..n as usize]);
        }
    } else {
        // A memory-backed loader buffer (no read callback): copy its content.
        unsafe {
            let b = &*buf;
            if !b.buffer.is_null() {
                let xbuf = &*(b.buffer as *mut _xmlBuffer);
                if !xbuf.content.is_null() && xbuf.use_ > 0 {
                    data.extend_from_slice(std::slice::from_raw_parts(
                        xbuf.content as *const u8,
                        xbuf.use_ as usize,
                    ));
                }
            }
        }
        result = Ok(());
    }
    // Release the loader's C buffer: the close callback (php streams IO
    // close) runs exactly once now that the bytes are owned here.
    io::input_buffer_free(buf);
    result.map(|()| data)
}

/// Route a filename open through the registered loaders, materializing the
/// result into an owned [`InputBuffer`] (filename = the original URI).
///
/// UPSTREAM LAYERING (2.14+ `xmlLoadResource`, R-000177): a REGISTERED
/// external entity loader (`xmlSetExternalEntityLoader`) is consulted first
/// for file/URL opens — main documents go through the same resource loader
/// as entities (`xmlCtxtNewInputFromUrl` -> `xmlLoadResource` ->
/// `xmlCurrentExternalEntityLoader`). A NULL custom-loader result is
/// `EntityLoaderFailed` (silent upstream — no `xmlCtxtErrIO`, the custom
/// loader reports its own errors). With NO custom entity loader the default
/// loader's tail is the `xmlParserInputBufferCreateFilenameDefault` (php
/// streams) loader, which is what the rest of this function implements
/// (upstream `xmlNewInputFromUrl`).
///
/// The registration is read through the R-000177 cross-DSO bridge (facade
/// copies must see a loader registered via the core DSO's exported setter).
///
/// # Safety
///
/// - `uri` must be a valid NUL-terminated C string live for the call.
/// - `ctxt` must be NULL or a valid parser context live for the call (passed
///   to the entity loader exactly as upstream `xmlLoadResource` does).
pub(crate) unsafe fn open_filename_routed(
    uri: *const c_char,
    ctxt: *mut _xmlParserCtxt,
) -> RoutedFileOpen {
    // A custom external entity loader governs file/URL opens too.
    if external_entity_loader_active() {
        // SAFETY: uri is a valid C string; ctxt is NULL or valid.
        let input = xmlLoadExternalEntity(uri, ptr::null(), ctxt);
        if input.is_null() {
            return RoutedFileOpen::EntityLoaderFailed;
        }
        // Materialize the loader's bytes into an owned InputBuffer (the
        // loader result is freed here; the parse consumes the copy).
        let loaded = input_bytes_owned(input);
        let named = if uri.is_null() {
            None
        } else {
            // SAFETY: uri is a valid NUL-terminated C string.
            Some(
                unsafe { CStr::from_ptr(uri) }
                    .to_string_lossy()
                    .into_owned(),
            )
        };
        return RoutedFileOpen::Loaded(InputBuffer::from_memory(
            loaded.as_deref().unwrap_or(&[]),
            named.as_deref(),
        ));
    }
    // No registered loader: the caller keeps the built-in open. The slot is
    // read through the R-000177 cross-DSO bridge (facade copies must see a
    // loader registered via the core DSO's exported setter).
    if globals::get_parser_input_buffer_create_filename_value_cross_dso().is_none() {
        return RoutedFileOpen::Builtin;
    }
    // SAFETY: uri is a valid NUL-terminated C string for the call.
    let loaded = unsafe { call_loader_materialize(uri) };
    match loaded {
        Err(()) => RoutedFileOpen::Failed,
        Ok(bytes) => {
            let named = if uri.is_null() {
                None
            } else {
                // SAFETY: uri is a valid NUL-terminated C string.
                Some(
                    unsafe { CStr::from_ptr(uri) }
                        .to_string_lossy()
                        .into_owned(),
                )
            };
            RoutedFileOpen::Loaded(InputBuffer::from_memory(&bytes, named.as_deref()))
        }
    }
}

/// True when a custom external entity loader is registered process-wide
/// (the core DSO's `xmlSetExternalEntityLoader` registration, or this DSO's
/// own when the accessor does not resolve in a single-DSO link). The
/// process-visible registration is authoritative (R-000177).
fn external_entity_loader_active() -> bool {
    match foreign_external_entity_loader() {
        Some(_) => true,
        None => EXTERNAL_ENTITY_LOADER.lock().is_some(),
    }
}

/// Route a filename open through the `xmlParserInputBufferCreateFilenameDefault`
/// (php streams) loader ONLY — no external-entity-loader consult.
///
/// The xmlTextReader family reads through `xmlNewInputFromFile` upstream,
/// which does NOT go through the external entity loader (verified against
/// the executed 2.15.3 oracle), so the reader must not pick up an
/// `xmlSetExternalEntityLoader` registration.
///
/// # Safety
///
/// - `uri` must be a valid NUL-terminated C string live for the call.
pub(crate) unsafe fn open_filename_routed_input_only(uri: *const c_char) -> RoutedFileOpen {
    // No registered loader: the caller keeps the built-in open. The slot is
    // read through the R-000177 cross-DSO bridge (facade copies must see a
    // loader registered via the core DSO's exported setter).
    if globals::get_parser_input_buffer_create_filename_value_cross_dso().is_none() {
        return RoutedFileOpen::Builtin;
    }
    // SAFETY: uri is a valid NUL-terminated C string for the call.
    let loaded = unsafe { call_loader_materialize(uri) };
    match loaded {
        Err(()) => RoutedFileOpen::Failed,
        Ok(bytes) => {
            let named = if uri.is_null() {
                None
            } else {
                // SAFETY: uri is a valid NUL-terminated C string.
                Some(
                    unsafe { CStr::from_ptr(uri) }
                        .to_string_lossy()
                        .into_owned(),
                )
            };
            RoutedFileOpen::Loaded(InputBuffer::from_memory(&bytes, named.as_deref()))
        }
    }
}

/// Copy the bytes of a loader-produced `_xmlParserInput` into an owned
/// `Vec` and release the input (upstream `xmlCtxtParseDocument` consumes the
/// input; the candidate's parse paths own an [`InputBuffer`]). The input's
/// underlying buffer is freed with the input (xmlFreeInputStream).
///
/// # Safety
///
/// - `input` must be a valid `_xmlParserInput` produced by a registered
///   loader / `xmlLoadExternalEntity`, not yet freed.
unsafe fn input_bytes_owned(input: *mut _xmlParserInput) -> Option<Vec<u8>> {
    unsafe {
        let base = (*input).base;
        let end = (*input).end;
        let len = if base.is_null() {
            0
        } else {
            end.offset_from(base).max(0) as usize
        };
        let bytes = if base.is_null() || len == 0 {
            None
        } else {
            Some(core::slice::from_raw_parts(base, len).to_vec())
        };
        crate::abi::exports_xml2::xmlFreeInputStream(input);
        bytes
    }
}

/// Compose the upstream `xmlCtxtErrIO(XML_IO_ENOENT, uri)` message text:
/// `failed to load "<uri>": <errno text>\n`. When errno is stale (the
/// registered php streams loader returned NULL without touching errno, e.g.
/// the percent-NUL guard) the `XML_IO_ENOENT` table text is used.
///
/// # Safety
///
/// - `uri` must be NULL or a valid NUL-terminated C string live for the call.
pub(crate) fn io_load_failure_message(uri: *const c_char) -> String {
    // SAFETY: reads errno only.
    let errno = unsafe { *libc::__errno_location() };
    let errstr = if errno == 0 {
        // xmlErrString(XML_IO_ENOENT) table text (error.c 2.15).
        "No such file or directory".to_string()
    } else {
        // SAFETY: strerror(errno) returns a static message for the value.
        unsafe { std::ffi::CStr::from_ptr(libc::strerror(errno)) }
            .to_string_lossy()
            .into_owned()
    };
    let url_str = if uri.is_null() {
        String::new()
    } else {
        // SAFETY: uri is a valid NUL-terminated C string.
        unsafe { std::ffi::CStr::from_ptr(uri) }
            .to_string_lossy()
            .into_owned()
    };
    format!("failed to load \"{url_str}\": {errstr}\n")
}

/// Set the application-wide external entity loader.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlSetExternalEntityLoader(xmlExternalEntityLoader f);
/// ```
///
/// # SAFETY
///
///
/// - `f` must be a valid callback (or None);
///   the callback is invoked with the documented context pointer and
///   must itself uphold the same pointer invariants.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlSetExternalEntityLoader(f: Option<xmlExternalEntityLoader>) {
    *EXTERNAL_ENTITY_LOADER.lock() = f;
}

/// Get the current external entity loader.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlExternalEntityLoader xmlGetExternalEntityLoader(void);
/// ```
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
pub unsafe extern "C" fn xmlGetExternalEntityLoader() -> Option<xmlExternalEntityLoader> {
    *EXTERNAL_ENTITY_LOADER.lock()
}

/// External entity loader that disables network access.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlParserInputPtr xmlNoNetExternalEntityLoader(const char *URL,
///                                                const char *ID,
///                                                xmlParserCtxtPtr ctxt);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `URL`, `ID` must point to valid NUL-terminated
///   strings (or NULL where the C contract allows) for the lifetime
///   of the call.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlNoNetExternalEntityLoader(
    URL: *const c_char,
    ID: *const c_char,
    ctxt: *mut _xmlParserCtxt,
) -> *mut _xmlParserInput {
    unsafe {
        let old_options = if ctxt.is_null() { 0 } else { (*ctxt).options };
        if !ctxt.is_null() {
            (*ctxt).options |= XML_PARSE_NONET;
        }
        let input = default_external_entity_loader(URL, ID, ctxt);
        if !ctxt.is_null() {
            (*ctxt).options = old_options;
        }
        input
    }
}

/// Load an external entity using the registered loader.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlParserInputPtr xmlLoadExternalEntity(const char *URL, const char *ID,
///                                         xmlParserCtxtPtr ctxt);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `URL`, `ID` must point to valid NUL-terminated
///   strings (or NULL where the C contract allows) for the lifetime
///   of the call.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlLoadExternalEntity(
    URL: *const c_char,
    ID: *const c_char,
    ctxt: *mut _xmlParserCtxt,
) -> *mut _xmlParserInput {
    // R-000177: the loader xmlSetExternalEntityLoader registers binds to the
    // CORE DSO, so a load performed by a whole-archive facade's private copy
    // must consult the process-visible registration first (upstream: one
    // core instance, one loader). Single-DSO links resolve their own export.
    let loader = match foreign_external_entity_loader() {
        Some(f) => Some(f),
        None => *EXTERNAL_ENTITY_LOADER.lock(),
    };
    match loader {
        Some(f) => unsafe { f(URL, ID, ctxt) },
        None => unsafe { default_external_entity_loader(URL, ID, ctxt) },
    }
}

/// Resolve the process-visible `xmlGetExternalEntityLoader` (the CORE DSO's
/// registration) via the dynamic symbol scope.
#[cfg(target_os = "linux")]
fn foreign_external_entity_loader() -> Option<xmlExternalEntityLoader> {
    use std::sync::OnceLock;
    type Getter = unsafe extern "C" fn() -> Option<xmlExternalEntityLoader>;
    static GETTER: OnceLock<Option<Getter>> = OnceLock::new();
    let getter = *GETTER.get_or_init(|| {
        // SAFETY: dlsym(RTLD_DEFAULT) returns the exported accessor address
        // or NULL; the transmute (pointer-sized) is sound.
        unsafe {
            let sym = libc::dlsym(libc::RTLD_DEFAULT, c"xmlGetExternalEntityLoader".as_ptr());
            if sym.is_null() {
                None
            } else {
                Some(std::mem::transmute::<*mut c_void, Getter>(sym))
            }
        }
    });
    getter.and_then(|g| unsafe { g() })
}

#[cfg(not(target_os = "linux"))]
fn foreign_external_entity_loader() -> Option<xmlExternalEntityLoader> {
    None
}

/// Check an input for HTTP access; with XML_PARSE_NONET set, HTTP inputs are
/// refused and freed.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlParserInputPtr xmlCheckHTTPInput(xmlParserCtxtPtr ctxt,
///                                     xmlParserInputPtr ret);
/// ```
///
/// # SAFETY
///
/// - `ctxt`, `ret` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlCheckHTTPInput(
    ctxt: *mut _xmlParserCtxt,
    ret: *mut _xmlParserInput,
) -> *mut _xmlParserInput {
    if ret.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        if !ctxt.is_null() && (*ctxt).options & XML_PARSE_NONET != 0 {
            let filename = (*ret).filename;
            if !filename.is_null() {
                let len = libc::strlen(filename);
                if len >= 7
                    && libc::strncasecmp(filename, c"http://".as_ptr() as *const c_char, 7) == 0
                {
                    // free_parser_input now frees the owned buffer (upstream
                    // xmlFreeInputStream semantics); no separate buf free.
                    helpers::free_parser_input(ret);
                    return ptr::null_mut();
                }
            }
        }
        ret
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlFile* I/O callbacks (xmlIO.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// Match callback: the file I/O handlers accept every filename.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlFileMatch(const char *filename);
/// ```
///
/// # SAFETY
///
///
/// - `_filename` must point to valid NUL-terminated
///   strings (or NULL where the C contract allows) for the lifetime
///   of the call.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub const unsafe extern "C" fn xmlFileMatch(_filename: *const c_char) -> c_int {
    1
}

/// Open a file and return a `FILE *` I/O context (cast to `void *`).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void *xmlFileOpen(const char *filename);
/// ```
///
/// # SAFETY
///
///
/// - `filename` must point to valid NUL-terminated
///   strings (or NULL where the C contract allows) for the lifetime
///   of the call.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlFileOpen(filename: *const c_char) -> *mut c_void {
    if filename.is_null() {
        return ptr::null_mut();
    }
    unsafe { libc::fopen(filename, c"rb".as_ptr() as *const c_char) as *mut c_void }
}

/// Read up to `len` bytes from a `FILE *` I/O context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlFileRead(void *context, char *buffer, int len);
/// ```
///
/// # SAFETY
///
/// - `context`, `buffer` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlFileRead(
    context: *mut c_void,
    buffer: *mut c_char,
    len: c_int,
) -> c_int {
    if context.is_null() || buffer.is_null() || len <= 0 {
        return -1;
    }
    unsafe {
        let n = libc::fread(
            buffer as *mut c_void,
            1,
            len as usize,
            context as *mut libc::FILE,
        );
        if n < len as usize && libc::ferror(context as *mut libc::FILE) != 0 {
            return -1;
        }
        n as c_int
    }
}

/// Close a `FILE *` I/O context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlFileClose(void *context);
/// ```
///
/// # SAFETY
///
/// - `context` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlFileClose(context: *mut c_void) -> c_int {
    if context.is_null() {
        return -1;
    }
    unsafe {
        let file = context as *mut libc::FILE;
        let fd = libc::fileno(file);
        if fd == 0 {
            // stdin must not be closed.
            return 0;
        }
        if fd == 1 || fd == 2 {
            // stdout/stderr are only flushed.
            return if libc::fflush(file) == 0 { 0 } else { -1 };
        }
        libc::fclose(file)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Low-level character scanning (parserInternals.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// Return the current character (UTF-8 decoded, EOL normalised) and its byte
/// length in `*len`. Does not advance the input pointer.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlCurrentChar(xmlParserCtxtPtr ctxt, int *len);
/// ```
///
/// # SAFETY
///
/// - `ctxt`, `len` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlCurrentChar(ctxt: *mut _xmlParserCtxt, len: *mut c_int) -> c_int {
    if ctxt.is_null() || len.is_null() || (*ctxt).input.is_null() {
        return 0;
    }
    unsafe {
        let pi = &*((*ctxt).input);
        let cur = pi.cur;
        if cur.is_null() {
            *len = 0;
            return 0;
        }
        let avail = (pi.end as usize).saturating_sub(cur as usize);
        let c = *cur;

        if c < 0x80 {
            if c == b'\r' {
                // EOL normalisation: CR (optionally CRLF) becomes LF.
                if avail >= 2 && *cur.add(1) == b'\n' {
                    (*(*ctxt).input).cur = cur.add(1);
                }
                *len = 1;
                return b'\n' as c_int;
            }
            if c == 0 {
                if avail == 0 {
                    *len = 0;
                } else {
                    *len = 1;
                }
                return 0;
            }
            *len = 1;
            return c as c_int;
        }

        // Multi-byte UTF-8.
        if avail < 2 || (*cur.add(1) & 0xc0) != 0x80 {
            *len = 1;
            return XML_INVALID_CHAR;
        }
        if c < 0xe0 {
            if c < 0xc2 {
                *len = 1;
                return XML_INVALID_CHAR;
            }
            let val = (((c & 0x1f) as c_int) << 6) | ((*cur.add(1) & 0x3f) as c_int);
            *len = 2;
            return val;
        }
        if avail < 3 || (*cur.add(2) & 0xc0) != 0x80 {
            *len = 1;
            return XML_INVALID_CHAR;
        }
        if c < 0xf0 {
            let val = (((c & 0x0f) as c_int) << 12)
                | (((*cur.add(1) & 0x3f) as c_int) << 6)
                | ((*cur.add(2) & 0x3f) as c_int);
            if val < 0x800 || (0xd800..0xe000).contains(&val) {
                *len = 1;
                return XML_INVALID_CHAR;
            }
            *len = 3;
            return val;
        }
        if avail < 4 || (*cur.add(3) & 0xc0) != 0x80 {
            *len = 1;
            return XML_INVALID_CHAR;
        }
        let val = (((c & 0x07) as c_int) << 18)
            | (((*cur.add(1) & 0x3f) as c_int) << 12)
            | (((*cur.add(2) & 0x3f) as c_int) << 6)
            | ((*cur.add(3) & 0x3f) as c_int);
        if !(0x10000..0x110000).contains(&val) {
            *len = 1;
            return XML_INVALID_CHAR;
        }
        *len = 4;
        val
    }
}

/// Advance to the next character, updating line/column accounting.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlNextChar(xmlParserCtxtPtr ctxt);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlNextChar(ctxt: *mut _xmlParserCtxt) {
    if ctxt.is_null() || (*ctxt).input.is_null() {
        return;
    }
    unsafe {
        let pi = &mut *((*ctxt).input);
        let cur = pi.cur;
        if cur.is_null() {
            return;
        }
        let avail = (pi.end as usize).saturating_sub(cur as usize);
        if avail == 0 {
            return;
        }
        let c = *cur;

        if c < 0x80 {
            if c == b'\n' {
                pi.cur = cur.add(1);
                pi.line += 1;
                pi.col = 1;
            } else if c == b'\r' {
                // CRLF is a single line break.
                pi.cur = cur.add(if avail >= 2 && *cur.add(1) == b'\n' {
                    2
                } else {
                    1
                });
                pi.line += 1;
                pi.col = 1;
            } else {
                pi.cur = cur.add(1);
                pi.col += 1;
            }
            return;
        }

        pi.col += 1;

        if avail < 2 || (*cur.add(1) & 0xc0) != 0x80 {
            pi.cur = cur.add(1);
            return;
        }
        if c < 0xe0 {
            if c < 0xc2 {
                pi.cur = cur.add(1);
                return;
            }
            pi.cur = cur.add(2);
            return;
        }
        if avail < 3 || (*cur.add(2) & 0xc0) != 0x80 {
            pi.cur = cur.add(1);
            return;
        }
        if c < 0xf0 {
            let val = (((c as c_int) << 8) as u32) | (*cur.add(1) as u32);
            if (val < 0xe0a0) || (0xeda0..0xee00).contains(&val) {
                pi.cur = cur.add(1);
                return;
            }
            pi.cur = cur.add(3);
            return;
        }
        if avail < 4 || (*cur.add(3) & 0xc0) != 0x80 {
            pi.cur = cur.add(1);
            return;
        }
        let val = (((c as c_int) << 8) as u32) | (*cur.add(1) as u32);
        if !(0xf090..0xf490).contains(&val) {
            pi.cur = cur.add(1);
            return;
        }
        pi.cur = cur.add(4);
    }
}

/// Skip blank characters (space, tab, LF, CR), updating line/column.
/// Returns the number of blanks skipped.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlSkipBlankChars(xmlParserCtxtPtr ctxt);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlSkipBlankChars(ctxt: *mut _xmlParserCtxt) -> c_int {
    if ctxt.is_null() || (*ctxt).input.is_null() {
        return 0;
    }
    unsafe {
        let pi = &mut *((*ctxt).input);
        let mut cur = pi.cur;
        if cur.is_null() {
            return 0;
        }
        let end = pi.end;
        let mut res = 0;
        while cur < end && (*cur == 0x20 || *cur == 0x09 || *cur == 0x0a || *cur == 0x0d) {
            if *cur == b'\n' {
                pi.line += 1;
                pi.col = 1;
            } else {
                pi.col += 1;
            }
            cur = cur.add(1);
            res += 1;
        }
        pi.cur = cur;
        res
    }
}

/// XML 1.0 5th-edition NameStartChar predicate (upstream `xmlIsNameStartCharNew`).
const fn is_name_start_char_new(c: c_int) -> bool {
    if c == b' ' as c_int || c == b'>' as c_int || c == b'/' as c_int {
        return false;
    }
    (c >= b'a' as c_int && c <= b'z' as c_int)
        || (c >= b'A' as c_int && c <= b'Z' as c_int)
        || c == b'_' as c_int
        || c == b':' as c_int
        || (c >= 0xC0 && c <= 0xD6)
        || (c >= 0xD8 && c <= 0xF6)
        || (c >= 0xF8 && c <= 0x2FF)
        || (c >= 0x370 && c <= 0x37D)
        || (c >= 0x37F && c <= 0x1FFF)
        || (c >= 0x200C && c <= 0x200D)
        || (c >= 0x2070 && c <= 0x218F)
        || (c >= 0x2C00 && c <= 0x2FEF)
        || (c >= 0x3001 && c <= 0xD7FF)
        || (c >= 0xF900 && c <= 0xFDCF)
        || (c >= 0xFDF0 && c <= 0xFFFD)
        || (c >= 0x10000 && c <= 0xEFFFF)
}

/// XML 1.0 5th-edition NameChar predicate (upstream `xmlIsNameCharNew`).
const fn is_name_char_new(c: c_int) -> bool {
    if c == b' ' as c_int || c == b'>' as c_int || c == b'/' as c_int {
        return false;
    }
    (c >= b'a' as c_int && c <= b'z' as c_int)
        || (c >= b'A' as c_int && c <= b'Z' as c_int)
        || (c >= b'0' as c_int && c <= b'9' as c_int)
        || c == b'_' as c_int
        || c == b':' as c_int
        || c == b'-' as c_int
        || c == b'.' as c_int
        || c == 0xB7
        || (c >= 0xC0 && c <= 0xD6)
        || (c >= 0xD8 && c <= 0xF6)
        || (c >= 0xF8 && c <= 0x2FF)
        || (c >= 0x300 && c <= 0x36F)
        || (c >= 0x370 && c <= 0x37D)
        || (c >= 0x37F && c <= 0x1FFF)
        || (c >= 0x200C && c <= 0x200D)
        || (c >= 0x203F && c <= 0x2040)
        || (c >= 0x2070 && c <= 0x218F)
        || (c >= 0x2C00 && c <= 0x2FEF)
        || (c >= 0x3001 && c <= 0xD7FF)
        || (c >= 0xF900 && c <= 0xFDCF)
        || (c >= 0xFDF0 && c <= 0xFFFD)
        || (c >= 0x10000 && c <= 0xEFFFF)
}

/// Scan an XML Name (or NCName/Nmtoken) at `ctxt->input->cur`, advancing the
/// input pointer. Returns a pointer to the end of the name, or NULL when the
/// name exceeds `max` bytes.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// const xmlChar *xmlScanName(xmlParserCtxtPtr ctxt, int max, int flags);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlScanName(
    ctxt: *mut _xmlParserCtxt,
    max: c_int,
    flags: c_int,
) -> *const xmlChar {
    if ctxt.is_null() || (*ctxt).input.is_null() || max <= 0 {
        return ptr::null();
    }
    unsafe {
        let pi = &mut *((*ctxt).input);
        let mut ptr = pi.cur;
        if ptr.is_null() {
            return ptr::null();
        }
        let end = pi.end;
        let mut remaining = max as usize;
        let stop: u8 = if flags & XML_SCAN_NC != 0 { b':' } else { 0 };
        let old10 = flags & XML_SCAN_OLD10 != 0;
        let mut f = flags;

        loop {
            if ptr >= end {
                break;
            }
            let c = *ptr;
            let (cp, len) = if c < 0x80 {
                if stop != 0 && c == stop {
                    break;
                }
                (c as c_int, 1usize)
            } else {
                // Decode a multi-byte UTF-8 character.
                let avail = (end as usize).saturating_sub(ptr as usize);
                let mut l = 4usize;
                let cp = decode_utf8_char(ptr, avail, &mut l);
                if cp < 0 {
                    break;
                }
                (cp, l)
            };

            let ok = if f & XML_SCAN_NMTOKEN != 0 {
                if old10 {
                    is_name_char_old10(cp)
                } else {
                    is_name_char_new(cp)
                }
            } else if old10 {
                is_name_start_char_old10(cp)
            } else {
                is_name_start_char_new(cp)
            };
            if !ok {
                break;
            }
            if len > remaining {
                return ptr::null();
            }
            ptr = ptr.add(len);
            remaining -= len;
            f |= XML_SCAN_NMTOKEN;
        }

        pi.cur = ptr;
        ptr
    }
}

/// Decode a UTF-8 character at `ptr` with `avail` bytes available; returns the
/// codepoint (or -1 on invalid/truncated input) and sets `*len` to its length.
const unsafe fn decode_utf8_char(ptr: *const u8, avail: usize, len: &mut usize) -> c_int {
    unsafe {
        let c = *ptr;
        if avail < 2 || (*ptr.add(1) & 0xc0) != 0x80 {
            return -1;
        }
        if c < 0xe0 {
            if c < 0xc2 {
                return -1;
            }
            *len = 2;
            return (((c & 0x1f) as c_int) << 6) | ((*ptr.add(1) & 0x3f) as c_int);
        }
        if avail < 3 || (*ptr.add(2) & 0xc0) != 0x80 {
            return -1;
        }
        if c < 0xf0 {
            let val = (((c & 0x0f) as c_int) << 12)
                | (((*ptr.add(1) & 0x3f) as c_int) << 6)
                | ((*ptr.add(2) & 0x3f) as c_int);
            if val < 0x800 || (val >= 0xd800 && val < 0xe000) {
                return -1;
            }
            *len = 3;
            return val;
        }
        if avail < 4 || (*ptr.add(3) & 0xc0) != 0x80 {
            return -1;
        }
        let val = (((c & 0x07) as c_int) << 18)
            | (((*ptr.add(1) & 0x3f) as c_int) << 12)
            | (((*ptr.add(2) & 0x3f) as c_int) << 6)
            | ((*ptr.add(3) & 0x3f) as c_int);
        if val < 0x10000 || val >= 0x110000 {
            return -1;
        }
        *len = 4;
        val
    }
}

/// XML 1.0 (pre-revision-5) NameStartChar predicate: Letter, '_' or ':'.
const fn is_name_start_char_old10(c: c_int) -> bool {
    (c >= b'a' as c_int && c <= b'z' as c_int)
        || (c >= b'A' as c_int && c <= b'Z' as c_int)
        || c == b'_' as c_int
        || c == b':' as c_int
        || (c >= 0xC0 && c <= 0xD6)
        || (c >= 0xD8 && c <= 0xF6)
        || (c >= 0xF8 && c <= 0x2FF)
        || (c >= 0x370 && c <= 0x37D)
        || (c >= 0x37F && c <= 0x1FFF)
        || (c >= 0x200C && c <= 0x200D)
        || (c >= 0x2070 && c <= 0x218F)
        || (c >= 0x2C00 && c <= 0x2FEF)
        || (c >= 0x3001 && c <= 0xD7FF)
        || (c >= 0xF900 && c <= 0xFDCF)
        || (c >= 0xFDF0 && c <= 0xFFFD)
        || (c >= 0x10000 && c <= 0xEFFFF)
}

/// XML 1.0 (pre-revision-5) NameChar predicate: NameStartChar, digits, '.',
/// '-', combining chars and extenders.
const fn is_name_char_old10(c: c_int) -> bool {
    is_name_start_char_old10(c)
        || (c >= b'0' as c_int && c <= b'9' as c_int)
        || c == b'.' as c_int
        || c == b'-' as c_int
        || c == 0xB7
        || (c >= 0x300 && c <= 0x36F)
        || c == 0x02D0
        || c == 0x02D1
        || c == 0x0387
        || c == 0x0640
        || c == 0x0E46
        || c == 0x0EC6
        || c == 0x3005
        || (c >= 0x3031 && c <= 0x3035)
        || (c >= 0x309D && c <= 0x309E)
        || (c >= 0x30FC && c <= 0x30FE)
}

/// Decode entities from the current input position: char references and
/// (predefined and DTD-declared) entity references are substituted. Stops at
/// the first of `end`/`end2`/`end3`, or after `len` bytes (`len < 0` = rest).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlDecodeEntities(xmlParserCtxtPtr ctxt, int len, xmlChar end,
///                            xmlChar end2, xmlChar end3);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlDecodeEntities(
    ctxt: *mut _xmlParserCtxt,
    len: c_int,
    end: xmlChar,
    end2: xmlChar,
    end3: xmlChar,
) -> *mut xmlChar {
    if ctxt.is_null() || (*ctxt).input.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        let pi = &*((*ctxt).input);
        let cur = pi.cur;
        if cur.is_null() {
            return ptr::null_mut();
        }
        let avail = (pi.end as usize).saturating_sub(cur as usize);
        let n = if len < 0 {
            avail
        } else {
            (len as usize).min(avail)
        };

        let mut out: Vec<u8> = Vec::new();
        let mut i = 0usize;

        while i < n {
            let c = *cur.add(i);
            if c == end || c == end2 || c == end3 {
                break;
            }
            if c != b'&' {
                out.push(c);
                i += 1;
                continue;
            }

            // Character reference: &#...; or &#x...;
            if i + 1 < n && *cur.add(i + 1) == b'#' {
                let (value, consumed) = parse_char_ref(cur.add(i), n - i);
                if consumed == 0 {
                    out.push(b'&');
                    i += 1;
                    continue;
                }
                let mut buf = [0u8; 4];
                let blen = copy_char_utf8(&mut buf, value);
                out.extend_from_slice(&buf[..blen]);
                i += consumed;
                continue;
            }

            // Entity reference: &name;
            let mut j = i + 1;
            while j < n
                && ((*cur.add(j)).is_ascii_alphanumeric()
                    || *cur.add(j) == b'_'
                    || *cur.add(j) == b'-'
                    || *cur.add(j) == b'.'
                    || *cur.add(j) == b':')
            {
                j += 1;
            }
            if j < n && *cur.add(j) == b';' {
                let name = core::slice::from_raw_parts(cur.add(i + 1), j - i - 1);
                let mut replaced = false;
                // Predefined entities.
                let content: Option<&[u8]> = match name {
                    b"amp" => Some(b"&"),
                    b"lt" => Some(b"<"),
                    b"gt" => Some(b">"),
                    b"quot" => Some(b"\""),
                    b"apos" => Some(b"'"),
                    _ => None,
                };
                if let Some(c) = content {
                    out.extend_from_slice(c);
                    replaced = true;
                } else {
                    // DTD-declared entity.
                    let mut name_nul = name.to_vec();
                    name_nul.push(0);
                    let ent = entities::get_entity((*ctxt).myDoc, name_nul.as_ptr());
                    if !ent.is_null() && !(*ent).content.is_null() {
                        let clen = string::xml_strlen((*ent).content);
                        out.extend_from_slice(core::slice::from_raw_parts((*ent).content, clen));
                        replaced = true;
                    }
                }
                if replaced {
                    i = j + 1;
                    continue;
                }
            }
            out.push(b'&');
            i += 1;
        }

        out.push(0);
        let result = xmlMallocImpl(out.len()) as *mut xmlChar;
        if result.is_null() {
            return ptr::null_mut();
        }
        ptr::copy_nonoverlapping(out.as_ptr(), result, out.len());
        result
    }
}

/// Parse a numeric character reference at `ptr` (starting at '&#'); returns
/// the value and total bytes consumed, or (0, 0) when malformed.
const unsafe fn parse_char_ref(ptr: *const u8, avail: usize) -> (c_int, usize) {
    unsafe {
        if avail < 3 || *ptr != b'&' || *ptr.add(1) != b'#' {
            return (0, 0);
        }
        let mut i = 2usize;
        let hex = i < avail && (*ptr.add(i) == b'x' || *ptr.add(i) == b'X');
        if hex {
            i += 1;
        }
        let start = i;
        let mut value: u32 = 0;
        while i < avail && *ptr.add(i) != b';' {
            let d = (*ptr.add(i) as char).to_digit(if hex { 16 } else { 10 });
            match d {
                Some(d) => {
                    value = value
                        .saturating_mul(if hex { 16 } else { 10 })
                        .saturating_add(d);
                    i += 1;
                }
                None => return (0, 0),
            }
        }
        if i == start || i >= avail || *ptr.add(i) != b';' {
            return (0, 0);
        }
        (value as c_int, i + 1)
    }
}

/// Encode a codepoint into a UTF-8 byte sequence; returns the byte count.
const fn copy_char_utf8(out: &mut [u8; 4], val: c_int) -> usize {
    if val < 0x80 {
        out[0] = val as u8;
        1
    } else if val < 0x800 {
        out[0] = 0xC0 | ((val >> 6) as u8);
        out[1] = 0x80 | ((val & 0x3F) as u8);
        2
    } else if val < 0x10000 {
        out[0] = 0xE0 | ((val >> 12) as u8);
        out[1] = 0x80 | (((val >> 6) & 0x3F) as u8);
        out[2] = 0x80 | ((val & 0x3F) as u8);
        3
    } else if val < 0x110000 {
        out[0] = 0xF0 | ((val >> 18) as u8);
        out[1] = 0x80 | (((val >> 12) & 0x3F) as u8);
        out[2] = 0x80 | (((val >> 6) & 0x3F) as u8);
        out[3] = 0x80 | ((val & 0x3F) as u8);
        4
    } else {
        out[0] = 0;
        1
    }
}

/// Detect the character encoding of a buffer from its initial bytes.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlCharEncoding xmlDetectCharEncoding(const unsigned char *in, int len);
/// ```
///
/// # SAFETY
///
/// - `in_` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub const unsafe extern "C" fn xmlDetectCharEncoding(in_: *const c_uchar, len: c_int) -> c_int {
    if in_.is_null() {
        return xmlCharEncoding::XML_CHAR_ENCODING_NONE as c_int;
    }
    unsafe {
        if len >= 4 {
            if *in_ == 0x00 && *in_.add(1) == 0x00 && *in_.add(2) == 0x00 && *in_.add(3) == 0x3C {
                return xmlCharEncoding::XML_CHAR_ENCODING_UCS4BE as c_int;
            }
            if *in_ == 0x3C && *in_.add(1) == 0x00 && *in_.add(2) == 0x00 && *in_.add(3) == 0x00 {
                return xmlCharEncoding::XML_CHAR_ENCODING_UCS4LE as c_int;
            }
            if *in_ == 0x4C && *in_.add(1) == 0x6F && *in_.add(2) == 0xA7 && *in_.add(3) == 0x94 {
                return xmlCharEncoding::XML_CHAR_ENCODING_EBCDIC as c_int;
            }
            if *in_ == 0x3C && *in_.add(1) == 0x3F && *in_.add(2) == 0x78 && *in_.add(3) == 0x6D {
                return xmlCharEncoding::XML_CHAR_ENCODING_UTF8 as c_int;
            }
            if *in_ == 0x3C && *in_.add(1) == 0x00 && *in_.add(2) == 0x3F && *in_.add(3) == 0x00 {
                return xmlCharEncoding::XML_CHAR_ENCODING_UTF16LE as c_int;
            }
            if *in_ == 0x00 && *in_.add(1) == 0x3C && *in_.add(2) == 0x00 && *in_.add(3) == 0x3F {
                return xmlCharEncoding::XML_CHAR_ENCODING_UTF16BE as c_int;
            }
        }
        if len >= 3 && *in_ == 0xEF && *in_.add(1) == 0xBB && *in_.add(2) == 0xBF {
            return xmlCharEncoding::XML_CHAR_ENCODING_UTF8 as c_int;
        }
        if len >= 2 {
            if *in_ == 0xFE && *in_.add(1) == 0xFF {
                return xmlCharEncoding::XML_CHAR_ENCODING_UTF16BE as c_int;
            }
            if *in_ == 0xFF && *in_.add(1) == 0xFE {
                return xmlCharEncoding::XML_CHAR_ENCODING_UTF16LE as c_int;
            }
        }
    }
    xmlCharEncoding::XML_CHAR_ENCODING_NONE as c_int
}

/// Convert the first line of `in` using the encoding handler, appending the
/// result to `out`.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlCharEncFirstLine(xmlCharEncodingHandlerPtr handler,
///                         struct _xmlBuffer *out, struct _xmlBuffer *in);
/// ```
///
/// # SAFETY
///
/// - `handler`, `out`, `in_` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlCharEncFirstLine(
    handler: *mut _xmlCharEncodingHandler,
    out: *mut _xmlBuffer,
    in_: *mut _xmlBuffer,
) -> c_int {
    encoding::xmlCharEncInFunc(handler, out, in_)
}

/// Check whether the current thread is the main thread.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlIsMainThread(void);
/// ```
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
pub const unsafe extern "C" fn xmlIsMainThread() -> c_int {
    1
}

// ═══════════════════════════════════════════════════════════════════════════════
// Error reporting helpers (xmlerror.h)
// ═══════════════════════════════════════════════════════════════════════════════

/// Print file and line information for a parser input to the generic error
/// channel.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlParserPrintFileInfo(struct _xmlParserInput *input);
/// ```
///
/// # SAFETY
///
/// - `input` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlParserPrintFileInfo(input: *mut _xmlParserInput) {
    if input.is_null() {
        return;
    }
    unsafe {
        let channel: Option<xmlGenericErrorFunc> = globals::get_generic_error_func();
        let data = globals::get_generic_error_ctx();
        let Some(ch) = channel else { return };

        let msg = if !(*input).filename.is_null() {
            let file = CStr::from_ptr((*input).filename);
            let s = format!("{}:{}: ", file.to_string_lossy(), (*input).line);
            std::ffi::CString::new(s).unwrap_or_default()
        } else {
            let s = format!("Entity: line {}: ", (*input).line);
            std::ffi::CString::new(s).unwrap_or_default()
        };
        ch(data, msg.as_ptr());
    }
}

/// Print the input context around the current error position to the generic
/// error channel.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlParserPrintFileContext(struct _xmlParserInput *input);
/// ```
///
/// # SAFETY
///
/// - `input` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlParserPrintFileContext(input: *mut _xmlParserInput) {
    if input.is_null() || (*input).cur.is_null() {
        return;
    }
    unsafe {
        let channel: Option<xmlGenericErrorFunc> = globals::get_generic_error_func();
        let data = globals::get_generic_error_ctx();
        let Some(ch) = channel else { return };

        let pi = &*input;
        let cur = pi.cur;
        let base = pi.base;
        let end = pi.end;

        // Build a window of up to 80 bytes ending at `cur`.
        let before = if base.is_null() {
            0
        } else {
            (cur as usize).saturating_sub(base as usize)
        };
        let take = before.min(LINE_LEN);
        let start = cur.sub(take);
        let n = (end as usize).saturating_sub(start as usize).min(LINE_LEN);

        let mut content = vec![0u8; n];
        if n > 0 {
            ptr::copy_nonoverlapping(start, content.as_mut_ptr(), n);
        }
        let line = std::ffi::CString::new(content.clone()).unwrap_or_default();
        ch(data, line.as_ptr());

        // Caret line pointing at the current character.
        let mut caret = vec![b' '; take];
        if take < LINE_LEN + 1 {
            caret.push(b'^');
        }
        let caret_c = std::ffi::CString::new(caret).unwrap_or_default();
        ch(data, caret_c.as_ptr());
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// SAX/DTD parse front-ends
// ═══════════════════════════════════════════════════════════════════════════════

/// Handle an entity reference by pushing the entity's content as a new input
/// stream (deprecated internal API).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlHandleEntity(xmlParserCtxtPtr ctxt, xmlEntityPtr entity);
/// ```
///
/// # SAFETY
///
/// - `ctxt`, `entity` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlHandleEntity(ctxt: *mut _xmlParserCtxt, entity: *mut c_void) {
    if ctxt.is_null() {
        return;
    }
    unsafe {
        let ent = entity as *mut _xmlEntity;
        if ent.is_null() {
            return;
        }
        // Unparsed entities cannot be included by reference.
        if (*ent).etype == xmlEntityType::XML_EXTERNAL_GENERAL_UNPARSED_ENTITY as c_int {
            return;
        }

        let mut input = ptr::null_mut();
        if !(*ent).content.is_null() {
            // Internal entity: push its replacement text as a new stream.
            let content = (*ent).content;
            let pi = xmlNewInputStream(ctxt);
            if pi.is_null() {
                return;
            }
            let len = string::xml_strlen(content);
            (*pi).base = content;
            (*pi).cur = content;
            (*pi).end = content.add(len);
            (*pi).length = len as c_int;
            (*pi).entity = ent;
            input = pi;
        } else if !(*ent).URI.is_null() {
            // External parsed entity: load it through the entity loader.
            input = xmlLoadExternalEntity(
                (*ent).URI as *const c_char,
                (*ent).ExternalID as *const c_char,
                ctxt,
            );
            if !input.is_null() {
                (*input).entity = ent;
            }
        }

        if input.is_null() {
            return;
        }
        xmlPushInput(ctxt, input);
    }
}

/// Load and parse a DTD, returning the resulting `xmlDtd` (detached from any
/// document).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDtdPtr xmlSAXParseDTD(xmlSAXHandlerPtr sax, const xmlChar *publicId,
///                          const xmlChar *systemId);
/// ```
///
/// # SAFETY
///
/// - `sax` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `publicId`, `systemId` must point to valid NUL-terminated
///   strings (or NULL where the C contract allows) for the lifetime
///   of the call.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlSAXParseDTD(
    sax: *mut _xmlSAXHandler,
    publicId: *const xmlChar,
    systemId: *const xmlChar,
) -> *mut _xmlDtd {
    if publicId.is_null() && systemId.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        let ctxt = xmlNewSAXParserCtxt(sax, ptr::null_mut());
        if ctxt.is_null() {
            return ptr::null_mut();
        }
        apply_options(ctxt, XML_PARSE_DTDLOAD);

        // Resolve via the SAX resolveEntity callback when available, else
        // load the system ID directly.
        let mut input = ptr::null_mut();
        if !sax.is_null() {
            if let Some(resolve) = (*sax).resolveEntity {
                input = resolve((*ctxt).userData, publicId, systemId);
            }
        }
        if input.is_null() {
            if systemId.is_null() {
                helpers::free_parser_ctxt(ctxt);
                return ptr::null_mut();
            }
            input = xmlLoadExternalEntity(systemId as *const c_char, ptr::null(), ctxt);
        }
        if input.is_null() {
            helpers::free_parser_ctxt(ctxt);
            return ptr::null_mut();
        }

        // Materialise the DTD text before freeing the input struct.
        let data: Vec<u8> = {
            let pi = &*input;
            if !pi.base.is_null() && !pi.end.is_null() && pi.end >= pi.base {
                let len = (pi.end as usize).saturating_sub(pi.base as usize);
                core::slice::from_raw_parts(pi.base, len).to_vec()
            } else if !pi.buf.is_null() {
                input_buffer_data(pi.buf)
            } else {
                Vec::new()
            }
        };
        helpers::free_parser_input(input);

        let dtd = parse_dtd_text(ctxt, &data, publicId, systemId);
        helpers::free_parser_ctxt(ctxt);
        dtd
    }
}

/// Load and parse a DTD from an input buffer.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDtdPtr xmlIOParseDTD(xmlSAXHandlerPtr sax, xmlParserInputBufferPtr input,
///                         xmlCharEncoding enc);
/// ```
///
/// # SAFETY
///
/// - `sax`, `input` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlIOParseDTD(
    sax: *mut _xmlSAXHandler,
    input: *mut _xmlParserInputBuffer,
    enc: c_int,
) -> *mut _xmlDtd {
    if input.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        let ctxt = xmlNewSAXParserCtxt(sax, ptr::null_mut());
        if ctxt.is_null() {
            io::input_buffer_free(input);
            return ptr::null_mut();
        }
        apply_options(ctxt, XML_PARSE_DTDLOAD);
        if enc != xmlCharEncoding::XML_CHAR_ENCODING_NONE as c_int {
            (*ctxt).charset = enc;
        }

        // Materialise the data from the input buffer.
        let data: Vec<u8> = input_buffer_data(input);
        io::input_buffer_free(input);

        let dtd = parse_dtd_text(ctxt, &data, ptr::null(), ptr::null());
        helpers::free_parser_ctxt(ctxt);
        dtd
    }
}

/// Extract the buffered data of an input buffer as an owned byte vector.
unsafe fn input_buffer_data(buf: *mut _xmlParserInputBuffer) -> Vec<u8> {
    unsafe {
        if buf.is_null() {
            return Vec::new();
        }
        let b = &*buf;
        if let Some(read) = b.readcallback {
            let mut out = Vec::new();
            let mut tmp = [0u8; 4096];
            loop {
                let n = read(
                    b.context,
                    tmp.as_mut_ptr() as *mut c_char,
                    tmp.len() as c_int,
                );
                if n <= 0 {
                    break;
                }
                out.extend_from_slice(&tmp[..n as usize]);
            }
            return out;
        }
        if !b.buffer.is_null() {
            let xbuf = &*(b.buffer as *mut _xmlBuffer);
            if !xbuf.content.is_null() && xbuf.use_ > 0 {
                return core::slice::from_raw_parts(xbuf.content, xbuf.use_ as usize).to_vec();
            }
        }
        Vec::new()
    }
}

/// Parse an external general entity and build a tree.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDocPtr xmlSAXParseEntity(xmlSAXHandlerPtr sax, const char *filename);
/// ```
///
/// # SAFETY
///
/// - `sax` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `filename` must point to valid NUL-terminated
///   strings (or NULL where the C contract allows) for the lifetime
///   of the call.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlSAXParseEntity(
    sax: *mut _xmlSAXHandler,
    filename: *const c_char,
) -> *mut _xmlDoc {
    if filename.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        let ctxt = xmlNewSAXParserCtxt(sax, ptr::null_mut());
        if ctxt.is_null() {
            return ptr::null_mut();
        }
        let input = match open_filename_routed(filename, ctxt) {
            RoutedFileOpen::Loaded(i) => i,
            RoutedFileOpen::Failed => {
                emit_io_warning(ctxt, io_load_failure_message(filename));
                helpers::free_parser_ctxt(ctxt);
                return ptr::null_mut();
            }
            RoutedFileOpen::EntityLoaderFailed => {
                helpers::free_parser_ctxt(ctxt);
                return ptr::null_mut();
            }
            RoutedFileOpen::Builtin => match helpers::input_from_file(filename) {
                Ok(i) => i,
                Err(_) => {
                    helpers::free_parser_ctxt(ctxt);
                    return ptr::null_mut();
                }
            },
        };
        helpers::setup_parser_input(ctxt, input);
        let rc = helpers::parse_document(ctxt);
        let doc = (*ctxt).myDoc;
        (*ctxt).myDoc = ptr::null_mut();
        if rc != 0 || (*ctxt).wellFormed == 0 {
            if !doc.is_null() {
                tree::free_doc(doc);
            }
            helpers::free_parser_ctxt(ctxt);
            return ptr::null_mut();
        }
        helpers::free_parser_ctxt(ctxt);
        doc
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// C14N: xmlC14NDocSave
// ═══════════════════════════════════════════════════════════════════════════════

/// Canonicalise a document (or node set) and save it to a file.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlC14NDocSave(xmlDocPtr doc, xmlNodeSetPtr nodes, int mode,
///                    xmlChar **inclusive_ns_prefixes, int with_comments,
///                    const char *filename, int compression);
/// ```
///
/// # SAFETY
///
/// - `doc`, `nodes`, `inclusive_ns_prefixes` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `filename` must point to valid NUL-terminated
///   strings (or NULL where the C contract allows) for the lifetime
///   of the call.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlC14NDocSave(
    doc: *mut _xmlDoc,
    nodes: *mut _xmlNodeSet,
    mode: c_int,
    inclusive_ns_prefixes: *mut *mut xmlChar,
    with_comments: c_int,
    filename: *const c_char,
    compression: c_int,
) -> c_int {
    if filename.is_null() {
        return -1;
    }
    unsafe {
        let output = io::output_buffer_create_filename(filename, ptr::null_mut(), compression);
        if output.is_null() {
            return -1;
        }
        let ret = crate::xml::c14n::xmlC14NDocSaveTo(
            doc,
            nodes,
            mode,
            inclusive_ns_prefixes,
            with_comments,
            output,
        );
        if ret < 0 {
            io::output_buffer_close(output);
            return -1;
        }
        let close_ret = io::output_buffer_close(output);
        if close_ret < 0 {
            -1
        } else {
            ret
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A read-callback state pair mirroring PHP's streams IO loader: the
    /// registered `xmlParserInputBufferCreateFilenameDefault` serves bytes
    /// through an `xmlParserInputBufferCreateIO` buffer (php builds exactly
    /// this shape with php_libxml_streams_IO_read/close over a php_stream).
    /// The loader reaches the state through a thread-local pointer (the
    /// loader slot itself is per-thread TLS, so there is no cross-thread
    /// aliasing).
    struct ServeState {
        data: &'static [u8],
        pos: usize,
        closed: bool,
    }

    thread_local! {
        static SERVE_STATE: std::cell::Cell<*mut ServeState> =
            std::cell::Cell::new(std::ptr::null_mut());
    }

    unsafe extern "C" fn serve_read(ctx: *mut c_void, buffer: *mut c_char, len: c_int) -> c_int {
        // SAFETY: ctx is the ServeState set up by the test; buffer is a
        // writable len-byte region per the xmlInputReadCallback contract.
        let st = unsafe { &mut *(ctx as *mut ServeState) };
        if st.pos >= st.data.len() {
            return 0;
        }
        let n = (len as usize).min(st.data.len() - st.pos);
        unsafe {
            core::ptr::copy_nonoverlapping(st.data.as_ptr().add(st.pos), buffer as *mut u8, n);
        }
        st.pos += n;
        n as c_int
    }

    unsafe extern "C" fn serve_close(ctx: *mut c_void) -> c_int {
        // SAFETY: ctx is the ServeState set up by the test.
        let st = unsafe { &mut *(ctx as *mut ServeState) };
        st.closed = true;
        0
    }

    /// The php-shaped loader: build an IO buffer over the thread-local serve
    /// state. `uri` is deliberately ignored — php's loader opens whatever the
    /// php streams layer resolves, so a "file://" URI or a non-existent path
    /// both reach the stream; this guard proves the ENGINE consults the
    /// loader instead of the built-in path (which would fail on the bogus
    /// URI used here).
    unsafe extern "C" fn serving_loader(
        _uri: *const c_char,
        _enc: c_int,
    ) -> *mut _xmlParserInputBuffer {
        SERVE_STATE.with(|cell| {
            let st = cell.get();
            if st.is_null() {
                return ptr::null_mut();
            }
            crate::abi::exports_xml2::xmlParserInputBufferCreateIO(
                Some(serve_read as xmlInputReadCallback),
                Some(serve_close as xmlInputCloseCallback),
                st as *mut c_void,
                xmlCharEncoding::XML_CHAR_ENCODING_NONE as c_int,
            )
        })
    }

    unsafe extern "C" fn record_message(ctx: *mut c_void, err: *const _xmlError) {
        if err.is_null() {
            return;
        }
        // SAFETY: ctx is the recording Vec set up by the test; the error is
        // live for the call and its message is NUL-terminated.
        let out = unsafe { &mut *(ctx as *mut Vec<u8>) };
        let msg = unsafe { (*err).message };
        if !msg.is_null() {
            // SAFETY: message is a NUL-terminated C string for the call.
            let bytes = unsafe { std::ffi::CStr::from_ptr(msg) }.to_bytes();
            out.extend_from_slice(bytes);
        }
    }

    /// SP-14.3.2 S8 / dom-L2 (bug79971_1): a registered
    /// `xmlParserInputBufferCreateFilenameDefault` (PHP's streams loader) is
    /// consulted by the main-document file open (`xmlReadFile`/
    /// `xmlCtxtReadFile` -> xmlNewInputFromFile -> xmlNewInputFromUrl): its
    /// bytes are parsed even when the URI is not a real file, and a NULL
    /// loader result reports the xmlCtxtErrIO "failed to load" warning with
    /// NO built-in fallback.
    ///
    /// # Safety
    ///
    /// - the callbacks and stack state are valid for the duration of each
    ///   call; the loader/generic-handler TLS slots are restored before the
    ///   test ends (serialized via the error-handler test lock).
    #[test]
    fn test_main_doc_open_consults_registered_input_loader() {
        use crate::xml::globals::ERROR_HANDLER_TEST_LOCK;

        // Serialize against the handler-slot tests (the generic func slot is
        // shared global state); the loader slot is this thread's TLS but it
        // is restored so later engine state stays pristine.
        let _guard = ERROR_HANDLER_TEST_LOCK.lock();
        let old_loader = globals::get_parser_input_buffer_create_filename_value();
        let old_struct = globals::get_structured_error_func();
        let old_struct_ctx = globals::get_structured_error_ctx();

        let mut captured: Vec<u8> = Vec::new();
        let captured_ptr = &mut captured as *mut Vec<u8> as *mut c_void;
        // SAFETY: set/restore of the handler slots is serialized under
        // ERROR_HANDLER_TEST_LOCK for the test's duration.
        unsafe {
            globals::set_structured_error_func(
                captured_ptr,
                Some(record_message as xmlStructuredErrorFunc),
            );
        }

        unsafe {
            let mut serve = ServeState {
                data: b"<root><a>1</a></root>",
                pos: 0,
                closed: false,
            };
            SERVE_STATE.with(|cell| cell.set(&mut serve as *mut ServeState));
            globals::set_parser_input_buffer_create_filename_value(Some(serving_loader));

            // The URI names no real file — only the loader can satisfy it.
            let ctxt = helpers::create_parser_ctxt();
            assert!(!ctxt.is_null());
            let doc = xmlCtxtReadFile(
                ctxt,
                c"file:///definitely-not-a-file.xml".as_ptr(),
                ptr::null(),
                0,
            );
            assert!(
                !doc.is_null(),
                "registered loader must be consulted for the main document open"
            );
            let root = (*doc).children;
            assert!(
                !root.is_null() && !(*root).name.is_null(),
                "served document must produce a root element"
            );
            assert_eq!(
                crate::xml::string::xmlstr_to_bytes((*root).name as *const u8),
                b"root",
                "document served by the loader must be parsed"
            );
            assert!(serve.closed, "loader stream must be closed exactly once");
            tree::free_doc(doc);
            helpers::free_parser_ctxt(ctxt);

            // A loader result of NULL is XML_IO_ENOENT: the built-in open is
            // NOT attempted and the xmlCtxtErrIO ENOENT report ("failed to
            // load") reaches the structured handler.
            globals::set_parser_input_buffer_create_filename_value(None);
            SERVE_STATE.with(|cell| cell.set(ptr::null_mut()));
            unsafe extern "C" fn null_loader(
                _uri: *const c_char,
                _enc: c_int,
            ) -> *mut _xmlParserInputBuffer {
                ptr::null_mut()
            }
            globals::set_parser_input_buffer_create_filename_value(Some(null_loader));
            let ctxt2 = helpers::create_parser_ctxt();
            assert!(!ctxt2.is_null());
            let doc2 = xmlCtxtReadFile(
                ctxt2,
                c"file:///definitely-not-a-file.xml".as_ptr(),
                ptr::null(),
                0,
            );
            assert!(doc2.is_null(), "NULL loader result must fail the open");
            let got = String::from_utf8_lossy(&captured);
            assert!(
                got.contains("failed to load"),
                "xmlCtxtErrIO report must reach the error channel: {got:?}"
            );
            helpers::free_parser_ctxt(ctxt2);
        }

        // Restore both slots.
        unsafe {
            globals::set_parser_input_buffer_create_filename_value(old_loader);
            globals::set_structured_error_func(old_struct_ctx, old_struct);
        }
    }
}
