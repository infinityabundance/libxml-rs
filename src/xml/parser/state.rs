//! XML parser state machine — document-level parsing with SAX dispatch (§85 Phase 3).
//!
//! This module implements the core parser state machine that orchestrates
//! tokenization and SAX event dispatch. It handles document structure:
//! prolog (XML declaration, DTD, misc), content (root element with children),
//! and epilog (misc after root). SAX events are dispatched through the
//! `SaxDispatcher` to the handlers registered in the parser context.
//!
//! # Upstream contract
//!
//! Mirrors upstream parser.c (SRC-LIBXML2-2.15.0-PARSER-C, oracle tree
//! `oracle/historical/src/libxml2-2.15.0/parser.c`). The parity target is the
//! system libxml2 2.15.3 oracle: diagnostics, tree structure, entity semantics
//! and exit codes must be byte-identical.
//!
//! # Conceptual behavior
//!
//! This module is the parser state machine that orchestrates tokenization and
//! SAX event dispatch: prolog (XML declaration, DTD, misc), content (root
//! element with children), epilog, entity reference expansion, recovery modes
//! and the push parser. It also implements the entity amplification guard
//! (`parser_entity_check`), entity content caching (`parse_entity_content`) and
//! attribute-value reference substitution (`substitute_refs`).
//!
//! # Ownership & safety invariants
//!
//! SAFETY: all raw `_xmlParserCtxt` / `_xmlEntity` / `_xmlNode` accesses are
//! audited unsafe blocks against the upstream ownership model: the context owns
//! its inputs and SAX handler; entity content parsed into `ent->children` is
//! owned by the entity declaration and freed with the DTD; the caller owns the
//! produced document (`xmlFreeDoc`). Borrowed pointers (node->parent,
//! node->doc, node->ns) are never freed here.
//!
//! # Historical quirks & epochs
//!
//! Epoch-pinned diagnostics: E-002 (single diagnostic since the 2.12.x error
//! rework, commit c6083a32), E-004 (entity content TEXT compact since 2.13.0,
//! commit 8d04f0ee), E-005 (exit 4 since 2.13.0; the entity-in-attribute fatal
//! error reported twice by the 2.13+ oracle, R-000121 hybrid). Security epochs:
//! default parser limits since 2.9.0 (QUIRK-0001, commit 52d8ade7), the
//! amplification guard since CVE-2014-3660 (SEC-0006, fixes be2a7eda and
//! 72a46a51), entity loop detection since CVE-2013-2877 (SEC-0004). 2.15.x:
//! xmlParseReference passes userData to SAX text callbacks (SEC-0011).
//!
//! # Deliberate oddities
//!
//! Deliberate oddities preserved for parity: the unconditional amplification
//! check with no XML_PARSE_HUGE bypass, disableSAX = 2 catastrophic stop with
//! 100-error suppression (xmlCtxtVErr parity), TEXT compact synthesis for
//! short text runs (R-000119), and the double report of XML_ERR_LT_IN_ATTRIBUTE
//! from the parser plus validation paths (R-000121, E-005 hybrid).
//!
//! # Proving courts
//!
//! PARSER court family; data-ABI probes ERROR-001 (48 malformed inputs x 4
//! passes, byte-identical) and TREE-001 (27-block structural fingerprint);
//! SECURITY-LIMITS amplification sweep; CLI-XMLLINT-0033/0034; and
//! `cargo test --lib` (counts generated into atlas/TEST_COUNTS.json by
//! tools/evidence/test_counts.py). Receipts under courts/receipts/phase-11.
//!
//! # Tempting simplifications that would break parity
//!
//! Removing the amplification guard would reintroduce CVE-2014-3660; parsing
//! entity content without the XML_ENT_EXPANDING guard would loop on recursive
//! declarations (CVE-2013-2877); not caching entity content in `ent->children`
//! would diverge from the debug dumps (R-000119); reporting errors at the
//! tokenizer current position instead of the exact detection point would shift
//! carets (R-000163). Never simplify the epoch hybrid — the single diagnostic
//! with exit 4 is the oracle current observable behavior.

use crate::abi::callbacks::*;
use crate::abi::structs::*;
use crate::abi::types::xmlElementContentOccur::*;
use crate::abi::types::xmlElementContentType::*;
use crate::abi::types::xmlElementType::*;
use crate::abi::types::xmlElementTypeVal::*;
use crate::abi::types::xmlEntityType::*;
use crate::abi::types::*;
use crate::xml::parser::input::{InputBuffer, InputStack};
use crate::xml::parser::tokenizer::{XmlToken, XmlTokenizer};
use crate::xml::sax::dispatch::SaxDispatcher;
use core::ptr;
use std::os::raw::{c_char, c_int, c_ulong, c_void};

// ─────────────────────────────────────────────────────────────────────────────
// Parser constants (parser states)
// ─────────────────────────────────────────────────────────────────────────────

// `ctxt->instate` is an ABI-visible field: consumers compiled against the
// real libxml2 headers (PHP ext/xml expat-compat `compat.c` gate on
// `instate == XML_PARSER_CONTENT` while resolving entity references) compare
// against the header's `xmlParserInputState` numbering. The single source of
// truth is `abi::types::xmlParserInputState` (which mirrors include/libxml/
// parser.h 2.15 verbatim); these aliases keep the write sites readable and
// make a future drift a compile-time error (SP-14.3.1-4, bug71592).
use crate::abi::types::xmlParserInputState as ParserState;
const XML_PARSER_START: c_int = ParserState::XML_PARSER_START as c_int;
const XML_PARSER_MISC: c_int = ParserState::XML_PARSER_MISC as c_int;
const XML_PARSER_PROLOG: c_int = ParserState::XML_PARSER_PROLOG as c_int;
const XML_PARSER_CONTENT: c_int = ParserState::XML_PARSER_CONTENT as c_int;
const XML_PARSER_DTD: c_int = ParserState::XML_PARSER_DTD as c_int;
const XML_PARSER_EOF: c_int = ParserState::XML_PARSER_EOF as c_int;
const XML_PARSER_EPILOG: c_int = ParserState::XML_PARSER_EPILOG as c_int;
const XML_PARSER_XML_DECL: c_int = ParserState::XML_PARSER_XML_DECL as c_int;

/// XML_WAR_NS_URI (include/libxml/xmlerror.h:99) — a namespace URI that
/// fails xmlParseURISafe ("xmlns: ... is not a valid URI").
const XML_WAR_NS_URI: c_int = 99;

/// XML_WAR_NS_URI_RELATIVE (include/libxml/xmlerror.h:100) — the namespace
/// URI warning for relative (non-absolute) xmlns URIs.
const XML_WAR_NS_URI_RELATIVE: c_int = 100;

/// XML_NS_ERR_XML_NAMESPACE (include/libxml/xmlerror.h:200) — namespace
/// declaration errors (empty xmlns, xml-prefix misuse, xmlns redefinition).
const XML_NS_ERR_XML_NAMESPACE: c_int = 200;

/// XML_NS_ERR_UNDEFINED_NAMESPACE (include/libxml/xmlerror.h:201) — a
/// namespace prefix used on an element or attribute name has no binding in
/// scope. R-000166.
const XML_NS_ERR_UNDEFINED_NAMESPACE: c_int = 201;

/// Whether a namespace URI carries a scheme (`[a-zA-Z][a-zA-Z0-9+.-]*:`
/// prefix) — upstream xmlParseURISafe's `scheme == NULL` check.
const fn has_uri_scheme(uri: &[u8]) -> bool {
    let mut i = 0usize;
    if i < uri.len() && uri[i].is_ascii_alphabetic() {
        i += 1;
        while i < uri.len()
            && (uri[i].is_ascii_alphanumeric() || matches!(uri[i], b'+' | b'-' | b'.'))
        {
            i += 1;
        }
        return i < uri.len() && uri[i] == b':';
    }
    false
}

/// Whether a SAX `error` slot holds one of the candidate's legacy default
/// handlers (upstream error.c treats these as "do not invoke directly —
/// format via `xmlFormatError`" in `xmlVRaiseError`).
#[allow(dead_code)]
fn is_legacy_error_handler(cb: errorSAXFunc) -> bool {
    let ptr = cb as usize;
    ptr == crate::xml::errors::XML_PARSER_ERROR_SAX1 as errorSAXFunc as usize
        || ptr == crate::xml::sax::default::default_sax_handler::error as errorSAXFunc as usize
}

/// Whether a SAX `warning` slot holds the candidate's legacy default handler.
#[allow(dead_code)]
fn is_legacy_warning_handler(cb: warningSAXFunc) -> bool {
    let ptr = cb as usize;
    ptr == crate::xml::errors::XML_PARSER_WARNING_SAX1 as errorSAXFunc as usize
        || ptr == crate::xml::sax::default::default_sax_handler::warning as warningSAXFunc as usize
}

// ─────────────────────────────────────────────────────────────────────────────
// XmlParser
// ─────────────────────────────────────────────────────────────────────────────

/// The internal parser state machine.
///
/// Wraps a tokenizer and a C ABI parser context pointer. Provides methods for
/// parsing documents or chunks (push parser), dispatching SAX events, and
/// managing error state.
pub(crate) struct XmlParser {
    /// The tokenizer producing lexical tokens from the input stack.
    tokenizer: XmlTokenizer,
    /// Raw pointer to the C ABI `_xmlParserCtxt`.
    ctxt: *mut _xmlParserCtxt,
    /// Parser options bitmask (from `XML_PARSE_*` constants).
    options: c_int,
    /// Whether the handler uses SAX1 callbacks only.
    #[allow(dead_code)]
    sax1: bool,
    /// Incremental push-probe mode (helpers::parse_chunk). When set, SAX
    /// dispatch and error delivery are suppressed and an end-of-input inside
    /// an open construct pauses the parse silently instead of raising
    /// "Premature end of data" — the parse only reports success when the
    /// accumulated input forms a complete document (SP-14.3.1-3).
    probe: bool,
    /// The context was ALREADY not-well-formed when this parse began
    /// (`ctxt->wellFormed == 0` before the per-parse reset). PHP ext/xml's
    /// expat-compat layer zeroes `wellFormed` right after creating the push
    /// context and never re-arms it, so upstream parses run with
    /// wellFormed == 0 for the whole document: xmlParseReference then returns
    /// at its `if (!ctxt->wellFormed) return;` guard and the ENGINE never
    /// re-parses resolved entity content — the expat-compat `get_entity` side
    /// effects (h_cdata / h_default / external-entity-ref feeds) are the ONLY
    /// delivery. Re-substituting here would double the content
    /// (SP-14.3.1-4: bug30875/gh14834 regressions).
    started_unwellformed: bool,
    /// Set when a probe parse paused at an end-of-input that could continue.
    paused: bool,
    /// Suppress SAX delivery only (diagnostics still delivered). Used by the
    /// final xmlParseChunk pass after an early delivery: the completing parse
    /// already delivered every event, so the final pass must not rebuild the
    /// tree or re-invoke handlers, but trailing-epilog errors must surface.
    sax_suppressed: bool,
    /// The parse aborted on a construct truncated by the end of the
    /// currently available input (an unterminated start tag / CDATA). An
    /// incremental probe must NOT deliver in that case: a later push call may
    /// complete the construct, and delivering the partial events now could
    /// not be retracted (SP-14.3.1-3). Distinguishes "<a" (wait) from a
    /// complete-but-invalid document like "</foo>" closing "<foo:a>"
    /// (deliver; upstream parsed it eagerly — bug25666).
    truncated_abort: bool,
    /// Eager-partial delivery mode (SP-14.3.1-6): SAX and diagnostics stay ON
    /// (unlike the silent probe) but an end-of-input inside an open construct
    /// PAUSES the parse instead of raising "Premature end of data". Upstream
    /// parses each xmlParseChunk eagerly and fires every event whose construct
    /// completed, so a non-final call on an incomplete document delivers the
    /// events it can (XML_OPTION_PARSE_HUGE multi-call flow); the terminating
    /// call continues from where delivery stopped.
    partial_delivery: bool,
    /// Byte offset below which SAX events were already delivered by an earlier
    /// eager-partial parse of the same accumulated input. Re-parses (later
    /// non-final calls and the terminating call) suppress the events in
    /// [0, `sax_suppress_until`) and fire only the new tail, so nothing is
    /// delivered twice (SP-14.3.1-6). 0 = suppress nothing.
    sax_suppress_until: usize,
    /// SAX-mode namespace scope stack (prefix, href) of the currently open
    /// elements. Upstream keeps the in-scope namespace stack on the parser
    /// context (`ctxt->nsTab`/`nsNr`, pushed by `xmlParseStartTag2` and popped
    /// at element end), which is what makes an ancestor-declared prefix
    /// resolve for pure-SAX parses (no tree, `ctxt->node == NULL`). The
    /// candidate consults it when no tree is being built (PHP expat-compat
    /// SAX2 mode) and falls back to `xmlSearchNs` on the tree otherwise.
    ns_scope: Vec<(Vec<u8>, Vec<u8>)>,
    /// DTD attribute defaults by element name (upstream `ctxt->attsDefault`, a
    /// hash of `xmlDefAttrs` per (localname, prefix), filled while the
    /// internal subset's `ATTLIST` declarations are parsed). `xmlParseStartTag2`
    /// appends the defaults that are not already present on the tag to the
    /// SAX2 attribute set and reports their count as `nb_defaulted`
    /// (SP-14.3.1-4, bug35447: `type (literal|pattern|sub) "literal"`).
    dtd_attr_defaults: Vec<(Vec<u8>, Vec<(Vec<u8>, Vec<u8>)>)>,
    /// Legacy-format "cur input" tail for the next raised parser error:
    /// upstream `xmlFormatError` prints the current (entity) input's
    /// `Entity: line N: ` info + window after the parent window
    /// (HOSTILE-FAILURE F2). Consumed by `raise_parser_error`.
    cur_error_tail: Option<(c_int, Option<(Vec<u8>, usize)>)>,
}

// ─── Construction and accessors ─────────────────────────────────────────────

// ═══════════════════════════════════════════════════════════════════════════════
// Token stream helpers
// ═══════════════════════════════════════════════════════════════════════════════

impl XmlParser {
    /// Create a new parser from an input stack and parser context.
    ///
    /// The tokenizer takes ownership of the input stack.
    ///
    /// # Safety
    ///
    /// `ctxt` must be a valid, properly initialized `_xmlParserCtxt`.
    pub unsafe fn new(input: InputStack, ctxt: *mut _xmlParserCtxt) -> Self {
        Self::new_with_mode(input, ctxt, false)
    }

    /// Create a new parser, optionally in incremental push-probe mode
    /// (`probe == true`, see the `probe` field docs).
    ///
    /// # Safety
    ///
    /// `ctxt` must be a valid, properly initialized `_xmlParserCtxt`.
    pub unsafe fn new_with_mode(input: InputStack, ctxt: *mut _xmlParserCtxt, probe: bool) -> Self {
        Self::new_with_flags(input, ctxt, probe, false)
    }

    /// Create a new parser with explicit probe / SAX-suppression flags.
    ///
    /// # Safety
    ///
    /// `ctxt` must be a valid, properly initialized `_xmlParserCtxt`.
    pub unsafe fn new_with_flags(
        input: InputStack,
        ctxt: *mut _xmlParserCtxt,
        probe: bool,
        sax_suppressed: bool,
    ) -> Self {
        let options = unsafe { (*ctxt).options };
        let initialized = unsafe { (*(*ctxt).sax).initialized };
        let sax1 = initialized != crate::abi::types::XML_SAX2_MAGIC as u32;

        // Whether the consumer disabled well-formed tracking before this
        // parse (PHP expat-compat sets ctxt->wellFormed = 0 at create): read
        // BEFORE the per-parse state reset below.
        let started_unwellformed = unsafe { (*ctxt).wellFormed == 0 };

        // Set initial parser state
        unsafe {
            (*ctxt).instate = XML_PARSER_START;
            (*ctxt).wellFormed = 1;
            (*ctxt).errNo = XML_ERR_OK;
            (*ctxt).nbErrors = 0;
            (*ctxt).nbWarnings = 0;
        }

        let mut tokenizer = XmlTokenizer::new(input);
        // UPSTREAM-PARITY (parserInternals.h): without XML_PARSE_HUGE a name
        // longer than XML_MAX_NAME_LENGTH (50 000) is rejected; with it the
        // limit is XML_MAX_TEXT_LENGTH (10 000 000) (SP-14.3.1-6).
        tokenizer.set_max_name_length(if (options & crate::abi::types::XML_PARSE_HUGE) != 0 {
            10_000_000
        } else {
            50_000
        });
        // UPSTREAM-PARITY (parser.c xmlParseXMLDecl): XML_PARSE_OLD10 makes
        // any non-"1.0" declaration version fatal (the tokenizer records the
        // version diagnostic in scan order, so it needs the flag up front).
        tokenizer.set_old10((options & crate::abi::types::XML_PARSE_OLD10) != 0);

        XmlParser {
            tokenizer,
            ctxt,
            options,
            sax1,
            probe,
            started_unwellformed,
            paused: false,
            sax_suppressed,
            partial_delivery: false,
            sax_suppress_until: 0,
            truncated_abort: false,
            ns_scope: Vec::new(),
            dtd_attr_defaults: Vec::new(),
            cur_error_tail: None,
        }
    }

    /// Create an eager-partial delivery parser (SP-14.3.1-6): SAX and
    /// diagnostics stay on, an end-of-input inside an open construct pauses
    /// silently, and events whose position falls at or below
    /// `suppress_until` (already delivered by an earlier partial parse of the
    /// same accumulated input) are skipped. The tokenizer additionally splits
    /// character runs at `suppress_until`, so the event segmentation matches
    /// the earlier partial parse exactly and no text is lost or doubled
    /// across the delivery boundary. Used for NON-final calls: upstream parses
    /// each chunk eagerly, so an incomplete document still delivers every
    /// event whose construct completed.
    ///
    /// # Safety
    ///
    /// `ctxt` must be a valid, properly initialized `_xmlParserCtxt`;
    /// `suppress_until` must be a byte offset into the accumulated input
    /// (0 = suppress nothing).
    pub unsafe fn new_with_partial_resume(
        input: InputStack,
        ctxt: *mut _xmlParserCtxt,
        suppress_until: usize,
    ) -> Self {
        let mut parser = Self::new_with_resume(input, ctxt, suppress_until);
        parser.partial_delivery = true;
        parser
    }

    /// Create a resume parser (SP-14.3.1-6): like `new` but SAX events at or
    /// below `suppress_until` (already delivered by earlier eager-partial
    /// parses) are skipped and character runs split at the boundary. An
    /// end-of-input inside an open construct still raises (terminating-call
    /// semantics — upstream xmlParseChunk's terminate path reports
    /// "Premature end of data"; bug81351).
    ///
    /// # Safety
    ///
    /// `ctxt` must be a valid, properly initialized `_xmlParserCtxt`;
    /// `suppress_until` must be a byte offset into the accumulated input
    /// (0 = suppress nothing).
    pub unsafe fn new_with_resume(
        input: InputStack,
        ctxt: *mut _xmlParserCtxt,
        suppress_until: usize,
    ) -> Self {
        let mut parser = Self::new_with_flags(input, ctxt, false, false);
        parser.sax_suppress_until = suppress_until;
        if suppress_until > 0 {
            parser.tokenizer.set_split_chars_at(Some(suppress_until));
        }
        parser
    }

    /// Return whether the parser dispatches through the SAX2 element
    /// callbacks (`startElementNs`/`endElementNs`) — upstream
    /// `xmlCtxtInitializeLate` (parser.c): SAX2 only when
    /// `XML_PARSE_SAX1` is not set, `sax->initialized == XML_SAX2_MAGIC` and
    /// SAX2 element handlers exist (or no SAX1 element handlers at all). PHP
    /// ext/xml's expat-compat layer resets `sax->initialized = 1` for
    /// `xml_parser_create()` (non-namespace) and keeps the magic for
    /// `xml_parser_create_ns()`; the two configurations must dispatch
    /// through SAX1 (raw QName + full attribute list, no namespace
    /// processing) and SAX2 respectively — bug50576/bug72714.
    ///
    /// # Safety
    ///
    /// - `self.ctxt` must be a valid, initialized `_xmlParserCtxt` whose
    ///   `sax` is a valid `_xmlSAXHandler`; only `initialized` and the
    ///   element-handler slots are read.
    fn sax2_mode(&self) -> bool {
        if (self.options & XML_PARSE_SAX1) != 0 {
            return false;
        }
        unsafe {
            let sax = &*(*self.ctxt).sax;
            if sax.initialized != crate::abi::types::XML_SAX2_MAGIC as u32 {
                return false;
            }
            sax.startElementNs.is_some()
                || sax.endElementNs.is_some()
                || (sax.startElement.is_none() && sax.endElement.is_none())
        }
    }

    /// Return whether the most recent probe parse paused at an
    /// end-of-input that could still continue (incomplete document).
    pub(crate) const fn is_paused(&self) -> bool {
        self.paused
    }

    /// Return whether the most recent parse aborted on a construct truncated
    /// by the end of the available input (see `truncated_abort`).
    pub(crate) const fn was_truncated_abort(&self) -> bool {
        self.truncated_abort
    }

    /// Get a mutable reference to the tokenizer.
    #[allow(dead_code)]
    pub const fn tokenizer(&mut self) -> &mut XmlTokenizer {
        &mut self.tokenizer
    }

    /// Get a shared reference to the parser context.
    ///
    /// # Safety
    ///
    /// The context pointer must remain valid.
    #[allow(dead_code)]
    pub unsafe fn ctxt(&self) -> &_xmlParserCtxt {
        unsafe { &*self.ctxt }
    }

    /// Get a mutable reference to the parser context.
    ///
    /// # Safety
    ///
    /// The context pointer must remain valid.
    #[allow(dead_code)]
    pub unsafe fn ctxt_mut(&mut self) -> &mut _xmlParserCtxt {
        unsafe { &mut *self.ctxt }
    }

    /// Get the raw parser context pointer.
    #[allow(dead_code)]
    pub const fn ctxt_raw(&self) -> *mut _xmlParserCtxt {
        self.ctxt
    }

    /// Return whether the parser is in recovery mode.
    const fn is_recovery(&self) -> bool {
        (self.options & XML_PARSE_RECOVER) != 0
    }

    /// Return whether pedantic mode (XML_PARSE_PEDANTIC) is active.
    ///
    /// # Safety
    ///
    /// - `self.ctxt` must be a valid, initialized `_xmlParserCtxt`
    ///   (established by `XmlParser::new`); only the `pedantic` field is
    ///   read.
    fn is_pedantic(&self) -> bool {
        unsafe { (*self.ctxt).pedantic != 0 }
    }

    /// Return whether SAX dispatch is currently disabled.
    ///
    /// # Safety
    ///
    /// - `self.ctxt` must be a valid, initialized `_xmlParserCtxt`; only the
    ///   `disableSAX` field is read.
    fn is_sax_disabled(&self) -> bool {
        unsafe { (*self.ctxt).disableSAX != 0 }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Main parsing entry points
    // ─────────────────────────────────────────────────────────────────────────

    /// Parse a complete XML document.
    ///
    /// Returns 0 on success, -1 on error.
    ///
    /// # Safety
    ///
    /// - `self.ctxt` must be a valid, initialized `_xmlParserCtxt` (see
    ///   `XmlParser::new`); the function writes parser state fields
    ///   (`instate`, `standalone`, `wellFormed`, `disableSAX`, `errNo`) and
    ///   reads `myDoc`, which must be NULL or a valid `_xmlDoc` whose
    ///   `standalone` field is written.
    pub fn parse_document(&mut self) -> c_int {
        // Fire startDocument
        self.sax_start_document();

        // Update parser state
        unsafe {
            (*self.ctxt).instate = XML_PARSER_MISC;
        }

        // UPSTREAM-PARITY (parserInternals.c xmlParserGrow): an I/O source
        // whose read callback reported an error raises an XML_IO_UNKNOWN
        // warning at the first grow, then parsing continues onto the empty
        // content and reports "Document is empty" — both diagnostics appear
        // (HOSTILE-CALLBACKS C4).
        if self.tokenizer.input().current_ref().has_source_error() {
            // XML_IO_UNKNOWN = 1500 (include/libxml/xmlerror.h 2.15).
            const XML_IO_UNKNOWN: c_int = 1500;
            self.raise_error_now(
                XML_FROM_IO,
                XML_IO_UNKNOWN,
                xmlErrorLevel::XML_ERR_WARNING as c_int,
                "Unknown IO error\n".to_string(),
                None,
                None,
                None,
                0,
            );
            // fall through: the empty input then reports "Document is
            // empty" exactly like the oracle.
        }

        // UPSTREAM-PARITY (xmlParseDocument): an empty input is reported as
        // "Document is empty" before anything else. In an incremental probe
        // (or an eager-partial delivery) the empty input is simply "more data
        // expected" — it pauses without firing endDocument (SP-14.3.1-6).
        if self.tokenizer.is_input_empty() {
            if self.probe || self.partial_delivery {
                self.paused = true;
                return -2;
            }
            self.raise_error_now(
                XML_FROM_PARSER,
                XML_ERR_DOCUMENT_EMPTY,
                xmlErrorLevel::XML_ERR_FATAL as c_int,
                "Document is empty\n".to_string(),
                None,
                None,
                None,
                0,
            );
            self.sax_end_document();
            return -1;
        }

        // Parse prolog (XML declaration, DTD, misc). Returns whether a root
        // element start tag was seen (pushed back for parse_content); on
        // failure the relevant error ("Document is empty", "Start tag
        // expected", doc-level invalid element name) was already raised.
        let root_seen = match self.parse_prolog() {
            Ok(v) => v,
            Err(()) => {
                if (self.probe || self.partial_delivery) && self.paused {
                    // End of the currently available input inside the prolog:
                    // the document may continue on a later push call. The
                    // document is not finished, so no endDocument fires
                    // (SP-14.3.1-6).
                    return -2;
                }
                if !self.is_recovery() {
                    self.sax_end_document();
                    return -1;
                }
                false
            }
        };

        if !root_seen {
            self.sax_end_document();
            return -1;
        }

        // Parse content (root element)
        unsafe {
            (*self.ctxt).instate = XML_PARSER_CONTENT;
        }
        if self.parse_content().is_err() && !self.is_recovery() {
            if (self.probe || self.partial_delivery) && self.paused {
                // Same pause: the root element is not finished yet (no
                // endDocument — SP-14.3.1-6).
                return -2;
            }
            self.sax_end_document();
            return -1;
        }

        // UPSTREAM-PARITY: a catastrophic stop ends the parse here.
        if unsafe { (*self.ctxt).disableSAX } == 2 {
            self.sax_end_document();
            return -1;
        }

        // Parse epilog (misc after root). When the root parse already failed
        // (errNo set) it ends EARLY — upstream xmlParseDocument leaves the
        // trailing bytes unparsed and finishes the document, so no further
        // diagnostics may fire from the leftovers (R-000166 plus the
        // mismatch/start-tag continuation family: load_error2_gte2_12 must
        // not gain a spurious epilog warning for the content after the
        // stray </book> closed the root). Only an input fully consumed by
        // the root parse can have a meaningful epilog left.
        let epilog_failed = if self.tokenizer.input().current_ref().remaining().is_empty() {
            unsafe {
                (*self.ctxt).instate = XML_PARSER_EPILOG;
            }
            self.parse_epilog().is_err()
        } else {
            false
        };
        if epilog_failed && !self.is_recovery() {
            self.sax_end_document();
            return -1;
        }

        // Mark EOF
        unsafe {
            (*self.ctxt).instate = XML_PARSER_EOF;
        }

        // UPSTREAM-PARITY: the parsed document inherits the standalone flag
        // from the XML declaration (tri-state: -1 unset, 0 "no", 1 "yes").
        unsafe {
            let doc = (*self.ctxt).myDoc;
            if !doc.is_null() {
                (*doc).standalone = (*self.ctxt).standalone;
            }
        }

        self.sax_end_document();

        // UPSTREAM-PARITY (xmlParseDocument): a not-well-formed document is
        // a parse failure; the caller frees the partial tree.
        if unsafe { (*self.ctxt).wellFormed } == 0 {
            return -1;
        }
        0
    }

    /// Parse a chunk of input (push parser mode).
    ///
    /// `chunk` contains the next bytes of input. If `terminate` is true,
    /// this is the final chunk and the document should be completed.
    ///
    /// Returns 0 on success, -1 on error.
    #[allow(dead_code)]
    pub fn parse_chunk(&mut self, chunk: &[u8], terminate: bool) -> c_int {
        // In push mode, we append data to the current input buffer
        // and continue parsing from where we left off.
        //
        // For now, we treat this as a simple feed: the tokenizer's input
        // is the concatenation of all chunks. A full implementation would
        // manage incremental parsing with proper state preservation.

        if chunk.is_empty() && !terminate {
            return 0;
        }

        // Push the chunk data as a new input on the stack
        let buf = InputBuffer::from_memory(chunk, None);
        self.tokenizer.push_input(buf);

        // If terminating, finish parsing the document
        if terminate {
            return self.parse_document();
        }

        0
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Prolog parsing
    // ─────────────────────────────────────────────────────────────────────────

    /// Parse the prolog: XML declaration, DTD, and misc (comments, PIs,
    /// whitespace).
    ///
    /// Returns `Ok(true)` when the prolog ended at a root-element start tag
    /// (pushed back for `parse_content`), `Ok(false)` when it ended at EOF
    /// or non-'<' content (the caller raises "Start tag expected" —
    /// upstream `xmlParseDocument`), and `Err(())` on a fatal prolog error.
    ///
    /// # Safety
    ///
    /// - `self.ctxt` must be a valid, initialized `_xmlParserCtxt`; the
    ///   `wellFormed` field is read to decide error reporting.
    fn parse_prolog(&mut self) -> Result<bool, ()> {
        loop {
            let (token, start) = self.tokenizer.next_token_with_start();
            // Document-level CDATA (and its tokenizer error) is reported as
            // "StartTag: invalid element name" — drop the recorded
            // CDATA-termination error first.
            if let XmlToken::Cdata { start_pos, .. } = &token {
                self.tokenizer.take_errors();
                self.raise_error_at(
                    XML_FROM_PARSER,
                    XML_ERR_NAME_REQUIRED,
                    xmlErrorLevel::XML_ERR_FATAL as c_int,
                    "StartTag: invalid element name\n".to_string(),
                    None,
                    None,
                    None,
                    0,
                    *start_pos + 1,
                );
                return Err(());
            }
            self.raise_pending_errors();
            match token {
                XmlToken::Eof => {
                    // Empty document or end of prolog without a root:
                    // upstream "Start tag expected, '<' not found" (only
                    // while wellFormed). In an incremental probe (or an
                    // eager-partial delivery) the root element simply has not
                    // arrived yet — pause silently (SP-14.3.1-6).
                    if self.probe || self.partial_delivery {
                        self.paused = true;
                        return Err(());
                    }
                    if unsafe { (*self.ctxt).wellFormed } != 0 {
                        self.raise_error_now(
                            XML_FROM_PARSER,
                            XML_ERR_DOCUMENT_EMPTY,
                            xmlErrorLevel::XML_ERR_FATAL as c_int,
                            "Start tag expected, '<' not found\n".to_string(),
                            None,
                            None,
                            None,
                            0,
                        );
                    }
                    return Err(());
                }
                XmlToken::XmlDecl {
                    version,
                    encoding,
                    standalone,
                } => {
                    self.parse_xml_decl(version, encoding, standalone)?;
                    unsafe {
                        (*self.ctxt).instate = XML_PARSER_PROLOG;
                    }
                }
                XmlToken::DocType {
                    content,
                    unterminated,
                } => {
                    // A DOCTYPE body cut off by the end of the available
                    // input (no closing `>` yet) may complete on a later push
                    // call: an incremental probe or eager-partial delivery
                    // must not parse the partial body (KEY-2). On a real
                    // terminating parse the partial body is parsed as-is
                    // (its errors surface through parse_dtd), matching the
                    // pre-KEY-2 behavior.
                    if unterminated && (self.probe || self.partial_delivery) {
                        self.truncated_abort = true;
                        return Err(());
                    }
                    self.parse_dtd(&content)?;
                    unsafe {
                        (*self.ctxt).instate = XML_PARSER_PROLOG;
                    }
                }
                XmlToken::Comment { data, unterminated } => {
                    // UPSTREAM-PARITY (SP-14.3.1-8, gh20439_1): a comment cut
                    // off by the end of the available input (no `-->`) is a
                    // construct that may continue on a later push call — an
                    // incremental probe or eager-partial delivery must NOT
                    // fire the partial comment or deliver its
                    // "Comment not terminated" error; it pauses (truncated).
                    if unterminated && (self.probe || self.partial_delivery) {
                        self.truncated_abort = true;
                        return Err(());
                    }
                    self.sax_comment(&data);
                }
                XmlToken::ProcessingInstruction {
                    target,
                    data,
                    unterminated,
                    ..
                } => {
                    // A PI cut off by the end of the available input (no
                    // `?>`) may complete on a later push call: an incremental
                    // probe or eager-partial delivery must NOT fire the
                    // partial PI or deliver its "never end" error; it pauses
                    // (KEY-3, mirroring unterminated comments/CDATA).
                    if unterminated && (self.probe || self.partial_delivery) {
                        self.truncated_abort = true;
                        return Err(());
                    }
                    self.sax_pi(&target, &data);
                }
                XmlToken::Characters(data) => {
                    // Upstream misc stops at the first non-blank character;
                    // whitespace-only runs are skipped.
                    if data.iter().any(|&b| !b.is_ascii_whitespace()) {
                        if unsafe { (*self.ctxt).wellFormed } != 0 {
                            self.raise_error_at(
                                XML_FROM_PARSER,
                                XML_ERR_DOCUMENT_EMPTY,
                                xmlErrorLevel::XML_ERR_FATAL as c_int,
                                "Start tag expected, '<' not found\n".to_string(),
                                None,
                                None,
                                None,
                                0,
                                start,
                            );
                        }
                        return Err(());
                    }
                }
                XmlToken::StartTag { .. } => {
                    // Start of root element — prolog is complete.
                    self.push_token_back(&token);
                    return Ok(true);
                }
                XmlToken::EndTag { start_pos, .. } => {
                    // UPSTREAM-PARITY: an end tag at document level fails the
                    // element-name parse → "StartTag: invalid element name"
                    // at the position right after '<'.
                    self.raise_error_at(
                        XML_FROM_PARSER,
                        XML_ERR_NAME_REQUIRED,
                        xmlErrorLevel::XML_ERR_FATAL as c_int,
                        "StartTag: invalid element name\n".to_string(),
                        None,
                        None,
                        None,
                        0,
                        start_pos + 1,
                    );
                    return Err(());
                }
                XmlToken::Reference(_) => {
                    // A reference at document level: upstream treats it as
                    // char data via xmlParseContent → "Start tag expected".
                    if unsafe { (*self.ctxt).wellFormed } != 0 {
                        self.raise_error_at(
                            XML_FROM_PARSER,
                            XML_ERR_DOCUMENT_EMPTY,
                            xmlErrorLevel::XML_ERR_FATAL as c_int,
                            "Start tag expected, '<' not found\n".to_string(),
                            None,
                            None,
                            None,
                            0,
                            start,
                        );
                    }
                    return Err(());
                }
                XmlToken::Cdata { .. } => {
                    // Handled above (document-level invalid element name).
                    unreachable!()
                }
            }
        }
    }

    /// Handle an XML declaration (`<?xml ...?>`).
    ///
    /// # Safety
    ///
    /// - `self.ctxt` must be a valid, initialized `_xmlParserCtxt`; the
    ///   function writes `instate`, `version`, `encoding` and `standalone`.
    ///   The `version`/`encoding` pointers are NUL-terminated buffers
    ///   created by `vec_to_cstr` and are intentionally leaked, not freed.
    fn parse_xml_decl(
        &mut self,
        version: Vec<u8>,
        encoding: Option<Vec<u8>>,
        standalone: Option<Vec<u8>>,
    ) -> Result<(), ()> {
        unsafe {
            (*self.ctxt).instate = XML_PARSER_XML_DECL;
        }

        // Store version
        if !version.is_empty() {
            let version_cstr = Self::vec_to_cstr(&version);
            unsafe {
                if !(*self.ctxt).version.is_null() {
                    // Free old version — in a real impl we'd use xmlFree
                }
                (*self.ctxt).version = version_cstr as *mut xmlChar;
            }
        }

        // Store encoding
        if let Some(enc) = encoding {
            let enc_cstr = Self::vec_to_cstr(&enc);
            unsafe {
                if !(*self.ctxt).encoding.is_null() {
                    // Free old encoding
                }
                (*self.ctxt).encoding = enc_cstr as *mut xmlChar;
                // UPSTREAM-PARITY (parser.c xmlParseXMLDecl / SAX2.c
                // xmlSAX2EndDocument): the declaration is only RECORDED on
                // the document at the end of the parse via xmlGetActualEncoding
                // — which resolves to the input encoder's name when one was
                // installed (a caller override). Under XML_PARSE_IGNORE_ENC the
                // declaration is ignored, so stamping it here would shadow the
                // caller-supplied encoding (PHP Dom\XMLDocument
                // overrideEncoding sets IGNORE_ENC and later fills
                // doc->encoding from its own argument when it is still NULL).
                let ignore_enc =
                    (*self.ctxt).options & crate::abi::types::XML_PARSE_IGNORE_ENC != 0;
                if !ignore_enc && !(*self.ctxt).myDoc.is_null() {
                    let my_doc = (*self.ctxt).myDoc;
                    if (*my_doc).encoding.is_null() {
                        (*my_doc).encoding = crate::xml::string::xml_strdup((*self.ctxt).encoding);
                    }
                }
            }
        }

        // UPSTREAM-PARITY (parser.c xmlParseXMLDecl / xmlParseSDDecl): an
        // XML declaration without a standalone attribute yields -2; a
        // declared standalone="yes"/"no" yields 1/0.
        unsafe { (*self.ctxt).standalone = -2 };
        // Store standalone
        if let Some(sa) = standalone {
            if sa.eq_ignore_ascii_case(b"yes") {
                unsafe { (*self.ctxt).standalone = 1 };
            } else if sa.eq_ignore_ascii_case(b"no") {
                unsafe { (*self.ctxt).standalone = 0 };
            } else {
                self.set_warning("Invalid standalone value in XML declaration");
            }
        }

        Ok(())
    }

    /// Parse the DTD from a `<!DOCTYPE ...>` declaration.
    ///
    /// # Safety
    ///
    /// - `self.ctxt` must be a valid, initialized `_xmlParserCtxt` whose
    ///   `sax` pointer is a valid `_xmlSAXHandler` and whose `userData`
    ///   matches the SAX handler's expectations; `content` is a byte slice
    ///   owned by the caller and live for the call.
    fn parse_dtd(&mut self, content: &[u8]) -> Result<(), ()> {
        unsafe {
            (*self.ctxt).instate = XML_PARSER_DTD;
            (*self.ctxt).inSubset = 1;
        }

        // Extract the root element name (first word after DOCTYPE)
        let trimmed = content.trim_ascii_start();
        let root_name: Vec<u8> = trimmed
            .iter()
            .take_while(|&&b| {
                b.is_ascii_alphanumeric() || b == b'_' || b == b':' || b == b'-' || b == b'.'
            })
            .copied()
            .collect();

        // Check for external ID
        let after_root = &trimmed[root_name.len()..].trim_ascii_start();
        let (ext_id, sys_id) = Self::extract_external_id(after_root);

        // UPSTREAM-PARITY (parser.c xmlParseDocTypeDecl): a DOCTYPE whose
        // ExternalID is a SYSTEM or PUBLIC id marks ctxt->hasExternalSubset
        // — whether or not the external subset is ever loaded (a
        // non-validating processor is not obligated to load it). The flag
        // gates the fatal branch of xmlHandleUndeclaredEntity ([WFC: Entity
        // Declared]).
        if ext_id.is_some() || sys_id.is_some() {
            unsafe {
                (*self.ctxt).hasExternalSubset = 1;
            }
        }

        // Check for internal subset (content between [...])
        let has_internal = content.contains(&b'[');

        // Fire internalSubset SAX event
        if !self.is_sax_disabled() && !self.below_delivery_boundary() {
            let name_cstr = if !root_name.is_empty() {
                Self::vec_to_cstr_null(root_name.as_slice())
            } else {
                ptr::null()
            };
            let ext_cstr = ext_id
                .as_ref()
                .map(|s| Self::vec_to_cstr_null(s.as_slice()))
                .unwrap_or(ptr::null());
            let sys_cstr = sys_id
                .as_ref()
                .map(|s| Self::vec_to_cstr_null(s.as_slice()))
                .unwrap_or(ptr::null());

            unsafe {
                let sax = &*(*self.ctxt).sax;
                let ctx = (*self.ctxt).userData;
                SaxDispatcher::internal_subset(sax, ctx, name_cstr, ext_cstr, sys_cstr);
            }
        }

        // If there's an internal subset, parse it
        if has_internal {
            self.parse_internal_subset(content)?;
        }

        // If there's an external ID (PUBLIC or SYSTEM) and the parser is in a
        // state that requires the external subset, parse it (upstream
        // xmlSAX2ExternalSubset: `ctxt->validate || (loadsubset & ~XML_SKIP_IDS)`
        // — DTDVALID, DTDLOAD and DTDATTR all trigger the load). A SYSTEM-only
        // DOCTYPE (no PUBLIC id) still references an external DTD.
        let want_ext = unsafe {
            let c = &*self.ctxt;
            c.validate != 0 || (c.loadsubset & !crate::abi::constants::XML_SKIP_IDS) != 0
        };
        if (ext_id.is_some() || sys_id.is_some()) && want_ext {
            self.parse_external_subset(&root_name, ext_id.as_deref(), sys_id.as_deref())?;
        }

        unsafe {
            (*self.ctxt).inSubset = 0;
        }

        Ok(())
    }

    /// Parse the internal DTD subset (content between `[` and `]`).
    ///
    /// Scans for `<!ELEMENT`, `<!ENTITY`, `<!ATTLIST`, `<!NOTATION`
    /// declarations plus comments and PIs, and populates the DTD's
    /// declaration hash tables so that validation and serialization work.
    ///
    /// # Safety
    ///
    /// - `self.ctxt` must be a valid, initialized `_xmlParserCtxt` whose
    ///   `myDoc` is NULL or a valid `_xmlDoc`; when non-NULL its `intSubset`
    ///   must be NULL or a valid `_xmlDtd` (the DTD is passed to the
    ///   `parse_*_decl` helpers, which require it valid and non-NULL).
    fn parse_internal_subset(&mut self, content: &[u8]) -> Result<(), ()> {
        // Mark that we're parsing the DTD
        unsafe {
            (*self.ctxt).inSubset = 1;
        }

        // Extract the text between the outermost '[' and ']'.
        let Some(open) = content.iter().position(|&b| b == b'[') else {
            return Ok(());
        };
        let Some(close) = content.iter().rposition(|&b| b == b']') else {
            return Ok(());
        };
        if close <= open {
            return Ok(());
        }
        let subset = &content[open + 1..close];

        // The declaration registry document to register into. In a parse that
        // builds a real tree (DOM/reader) this is the document's own
        // `intSubset`. In a push/expat-compat SAX parse (`xml_parser`/ext/xml)
        // there is no real document, so unlike upstream we must not drop the
        // internal subset: internal general entities must still be registered
        // for later NOENT substitution. Heuristically pre-create the registry
        // only when the subset actually declares a general entity, mirroring
        // upstream `xmlParseEntityDecl` (parser.c: lazily keep a
        // SAX_COMPAT_MODE document + "fake" intSubset and register the entity
        // via xmlSAX2EntityDecl).
        let mut dtd = Self::current_int_subset_opt(unsafe { (*self.ctxt).myDoc });
        self.process_dtd_fragment(&mut dtd, subset, 0, false);

        unsafe {
            (*self.ctxt).inSubset = 0;
        }

        Ok(())
    }

    /// Process one DTD declaration fragment (internal-subset body, external
    /// subset file, or a parameter-entity replacement text) into `dtd`.
    /// Handles the markup declarations (ELEMENT/ENTITY/ATTLIST/NOTATION),
    /// comments, PIs and parameter-entity references. PE references BETWEEN
    /// declarations expand in place (external PEs load their file); PE
    /// references inside a declaration's argument list (e.g.
    /// `<!ATTLIST th %attrs; >`) are expanded before the declaration is
    /// parsed, skipping quoted literals (an entity VALUE keeps its %refs
    /// raw — they expand at use time). `ext` selects external-subset
    /// semantics (entity declarations always register; no SAX-compat
    /// registry gating). `depth` bounds PE nesting (upstream's recursion
    /// guard for parameter entities).
    fn process_dtd_fragment(
        &mut self,
        dtd: &mut Option<*mut _xmlDtd>,
        data: &[u8],
        depth: usize,
        ext: bool,
    ) {
        let mut i = 0usize;
        while i < data.len() {
            while i < data.len() && data[i].is_ascii_whitespace() {
                i += 1;
            }
            if i >= data.len() {
                break;
            }
            if data[i] != b'<' {
                // Stray text outside a markup declaration: a well-formed
                // parameter-entity reference (%Name;) is the only construct
                // with parser-visible meaning here.
                // UPSTREAM-PARITY (parser.c xmlParsePERefInternal): parsing
                // a %Name; reference sets ctxt->hasPErefs BEFORE the entity
                // is resolved (line 7618) — it also fires for undeclared
                // parameter entities. The flag gates
                // xmlHandleUndeclaredEntity's fatal branch: documents whose
                // declarations may live in external parameter entities (or an
                // external subset) relax [WFC: Entity Declared] to a
                // non-fatal report, because a non-validating processor is not
                // obligated to load them.
                if data[i] == b'%' {
                    let rest = &data[i + 1..];
                    let name_len = rest.iter().take_while(|&&b| dtd_name_char(b)).count();
                    if name_len > 0 && rest.get(name_len) == Some(&b';') {
                        let pe_name = &rest[..name_len];
                        unsafe {
                            (*self.ctxt).hasPErefs = 1;
                        }
                        // UPSTREAM-PARITY (parser.c xmlParsePEReference): the
                        // reference is expanded in place when the parameter
                        // entity resolves (an external PE's file is fetched;
                        // an internal PE's replacement text is re-scanned for
                        // nested references). DOMDocument_validate_external_dtd
                        // (dom.xml `%incent;` -> dom.ent) needs the external
                        // PE's declarations to land in the internal subset.
                        if depth < 10 && !dtd.is_none() {
                            self.expand_pe_reference(dtd, pe_name, depth);
                        }
                        i += 1 + name_len + 1;
                        continue;
                    }
                }
                i += 1;
                continue;
            }
            if data[i..].starts_with(b"<!--") {
                match find_subseq(&data[i..], b"-->") {
                    Some(p) => i += p + 3,
                    None => break,
                }
                continue;
            }
            if data[i..].starts_with(b"<?") {
                match find_subseq(&data[i..], b"?>") {
                    Some(p) => i += p + 2,
                    None => break,
                }
                continue;
            }
            if data[i..].starts_with(b"<!DOCTYPE") {
                // A nested <!DOCTYPE> in an external subset / PE is an error
                // upstream; skip past the declaration end defensively.
                let rest = &data[i + 2..];
                match find_decl_end(rest) {
                    Some(gt) => i += 2 + gt + 1,
                    None => break,
                }
                continue;
            }
            if data[i..].starts_with(b"<!")
                && !data[i..].starts_with(b"<!ELEMENT")
                && !data[i..].starts_with(b"<!ATTLIST")
                && !data[i..].starts_with(b"<!ENTITY")
                && !data[i..].starts_with(b"<!NOTATION")
            {
                // Unknown markup declaration; skip past the declaration end.
                let rest = &data[i + 2..];
                match find_decl_end(rest) {
                    Some(gt) => {
                        i += 2 + gt + 1;
                        continue;
                    }
                    None => break,
                }
            }
            let rest = &data[i + 2..];
            let Some(gt) = find_decl_end(rest) else {
                break;
            };
            let decl = &rest[..gt];
            let kw_end = decl
                .iter()
                .position(|&b| b.is_ascii_whitespace())
                .unwrap_or(decl.len());
            let kw = &decl[..kw_end];
            let raw_args = &decl[kw_end..];
            // Inline parameter-entity expansion (outside quoted literals).
            let expanded = if raw_args.contains(&b'%') {
                self.expand_args_pes(raw_args, depth)
            } else {
                None
            };
            let args: &[u8] = match &expanded {
                Some(v) => v.as_slice(),
                None => raw_args,
            };
            if kw.eq_ignore_ascii_case(b"ELEMENT") {
                if let Some(d) = *dtd {
                    Self::parse_element_decl(d, args);
                }
            } else if kw.eq_ignore_ascii_case(b"ENTITY") {
                // UPSTREAM-PARITY (parser.c xmlParseEntityDecl): WHICH
                // declarations are registered into the parser's entity
                // table depends on whether a real document is being built.
                // Tree consumers receive every declaration through the
                // document DTD (registered here into the real intSubset).
                // Expat-compat SAX consumers (no real doc — myDoc is NULL
                // or the internal SAX_COMPAT_MODE registry) receive them
                // through the SAX entityDecl events instead; upstream
                // registers into the SAX-compat myDoc only general entities
                // with a replacement VALUE (always) and external general
                // PARSED entities when substitution was requested
                // (ctxt->replaceEntities != 0). Parameter and NDATA-unparsed
                // declarations never enter the registry — which is why an
                // external parsed entity declared WITHOUT NOENT resolves as
                // undeclared on the next reference, exactly like upstream
                // (SP-14.3.1-4, bug71592).
                let mut register = true;
                if !ext {
                    let compat_registry = unsafe {
                        let doc = (*self.ctxt).myDoc;
                        if doc.is_null() {
                            true
                        } else {
                            // SAX_COMPAT_MODE docs are marked XML_DOC_INTERNAL
                            // (see ensure_entity_registry_dtd).
                            (*doc).properties
                                & (crate::abi::types::xmlDocProperties::XML_DOC_INTERNAL as c_int)
                                != 0
                        }
                    };
                    if compat_registry {
                        register = self.compat_entity_must_register(args);
                    }
                }
                if register {
                    if dtd.is_none() {
                        *dtd = self.ensure_entity_registry_dtd();
                    }
                    if let Some(d) = *dtd {
                        self.parse_entity_decl(d, args);
                    }
                }
                // UPSTREAM-PARITY (parser.c xmlParseEntityDecl): when a
                // non-parameter NDATA (unparsed) external entity is
                // declared the SAX unparsedEntityDecl event fires (php
                // expat/notation handler).
                self.fire_sax_unparsed_entity_decl(args);
            } else if kw.eq_ignore_ascii_case(b"ATTLIST") {
                if let Some(d) = *dtd {
                    Self::parse_attlist_decl(d, args);
                }
                // UPSTREAM-PARITY (parser.c xmlParseAttributeListDecl):
                // defaults land in ctxt->attsDefault regardless of a
                // document tree (SP-14.3.1-4, bug35447).
                self.collect_attlist_defaults(args);
            } else if kw.eq_ignore_ascii_case(b"NOTATION") {
                if let Some(d) = *dtd {
                    Self::parse_notation_decl(d, args);
                }
                // UPSTREAM-PARITY (parser.c xmlParseNotationDecl): the
                // SAX notationDecl event fires for a DTD notation.
                self.fire_sax_notation_decl(args);
            }
            i += 2 + gt + 1;
        }
    }

    /// Expand a decl-level `%name;` reference: fetch the parameter entity
    /// (internal replacement text or external file) and process its content
    /// as a declaration fragment into `dtd`.
    fn expand_pe_reference(&mut self, dtd: &mut Option<*mut _xmlDtd>, name: &[u8], depth: usize) {
        let doc = unsafe { (*self.ctxt).myDoc };
        if doc.is_null() {
            return;
        }
        let name_cstr = Self::vec_to_cstr_null(name);
        let ent = unsafe { crate::xml::entities::get_parameter_entity(doc, name_cstr) };
        if ent.is_null() {
            return;
        }
        unsafe {
            use crate::abi::types::xmlEntityType::*;
            match (*ent).etype {
                t if t == XML_INTERNAL_PARAMETER_ENTITY as c_int => {
                    let c = (*ent).content;
                    if !c.is_null() {
                        let len = crate::abi::exports_xml2::xmlStrlen(c);
                        let text = core::slice::from_raw_parts(c, len as usize);
                        self.process_dtd_fragment(dtd, text, depth + 1, false);
                    }
                }
                t if t == XML_EXTERNAL_PARAMETER_ENTITY as c_int => {
                    let sys = (*ent).SystemID;
                    if !sys.is_null() {
                        let sys_str = crate::xml::string::xmlstr_to_string(sys as *const xmlChar);
                        if let Some(p) = self.resolve_dtd_path(sys_str.as_bytes()) {
                            if let Ok(content) = std::fs::read(&p) {
                                self.process_dtd_fragment(dtd, &content, depth + 1, true);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// Expand parameter-entity references in a declaration's argument list,
    /// skipping quoted literals (an entity value stores its %refs raw).
    /// Returns None when nothing was expanded.
    fn expand_args_pes(&self, args: &[u8], depth: usize) -> Option<Vec<u8>> {
        if depth >= 10 {
            return None;
        }
        let doc = unsafe { (*self.ctxt).myDoc };
        if doc.is_null() {
            return None;
        }
        let mut out: Vec<u8> = Vec::with_capacity(args.len());
        let mut changed = false;
        let mut i = 0usize;
        while i < args.len() {
            match args[i] {
                b'\'' | b'"' => {
                    let q = args[i];
                    out.push(q);
                    i += 1;
                    while i < args.len() && args[i] != q {
                        out.push(args[i]);
                        i += 1;
                    }
                    if i < args.len() {
                        out.push(args[i]);
                        i += 1;
                    }
                }
                b'%' => {
                    let rest = &args[i + 1..];
                    let name_len = rest.iter().take_while(|&&b| dtd_name_char(b)).count();
                    if name_len > 0 && rest.get(name_len) == Some(&b';') {
                        let pe_name = &rest[..name_len];
                        let name_cstr = Self::vec_to_cstr_null(pe_name);
                        let ent =
                            unsafe { crate::xml::entities::get_parameter_entity(doc, name_cstr) };
                        let expanded: Option<Vec<u8>> = if !ent.is_null() {
                            let e = unsafe { &*ent };
                            use crate::abi::types::xmlEntityType::*;
                            match e.etype {
                                t if t == XML_INTERNAL_PARAMETER_ENTITY as c_int => {
                                    if e.content.is_null() {
                                        None
                                    } else {
                                        let len = unsafe {
                                            crate::abi::exports_xml2::xmlStrlen(e.content)
                                        };
                                        let text: &[u8] = unsafe {
                                            core::slice::from_raw_parts(e.content, len as usize)
                                        };
                                        // Nested refs expand recursively; the
                                        // whole replacement text is spliced in
                                        // place of the reference.
                                        Some(
                                            self.expand_args_pes(text, depth + 1)
                                                .unwrap_or_else(|| text.to_vec()),
                                        )
                                    }
                                }
                                _ => None,
                            }
                        } else {
                            None
                        };
                        match expanded {
                            Some(v) => {
                                out.extend_from_slice(&v);
                                changed = true;
                            }
                            None => {
                                out.extend_from_slice(&args[i..i + 1 + name_len + 1]);
                            }
                        }
                        i += 1 + name_len + 1;
                        continue;
                    }
                    out.push(args[i]);
                    i += 1;
                }
                _ => {
                    out.push(args[i]);
                    i += 1;
                }
            }
        }
        if changed {
            Some(out)
        } else {
            None
        }
    }

    /// Return the document's current `intSubset`, or None when absent.
    fn current_int_subset_opt(doc: *mut _xmlDoc) -> Option<*mut _xmlDtd> {
        if doc.is_null() {
            return None;
        }
        unsafe {
            let s = (*doc).intSubset;
            if s.is_null() {
                None
            } else {
                Some(s)
            }
        }
    }

    /// Split a decl argument string into its (leading) name and optional
    /// SYSTEM/PUBLIC ids: `(name, pub, sys)` with `name` always present.
    fn split_dtd_name_ids(
        args: &[u8],
        _base_candidates: bool,
    ) -> (Vec<u8>, Option<Vec<u8>>, Option<Vec<u8>>) {
        let a = args.trim_ascii_start();
        let name_end = a
            .iter()
            .position(|&b| b.is_ascii_whitespace())
            .unwrap_or(a.len());
        let name = a[..name_end].to_vec();
        let tail = a[name_end..].trim_ascii_start();
        let span = tail;
        let (public_id, sys) = if span.starts_with(b"PUBLIC") {
            match split_two_quoted(&span[6..]) {
                (Some(p), Some(s)) => (Some(p.to_vec()), Some(s.to_vec())),
                (Some(p), None) => (Some(p.to_vec()), None),
                _ => (None, None),
            }
        } else if span.starts_with(b"SYSTEM") {
            match read_quoted(&span[6..]) {
                Some(s) => (None, Some(s.to_vec())),
                None => (None, None),
            }
        } else {
            (None, None)
        };
        (name, public_id, sys)
    }

    /// Fire the SAX `notationDecl` callback for a `<!NOTATION ...>` decl whose
    /// argument text is `args` (everything after the leading `NOTATION` keyword).
    /// Mirrors upstream `xmlParseNotationDecl` (SAX `notationDecl`).
    fn fire_sax_notation_decl(&self, args: &[u8]) {
        if self.sax_blocked() || self.below_delivery_boundary() {
            return;
        }
        unsafe {
            let sax = &*(*self.ctxt).sax;
            if sax.notationDecl.is_none() {
                return;
            }
            let ctx = (*self.ctxt).userData;
            let (name, public_id, sys) = Self::split_dtd_name_ids(args, false);
            if name.is_empty() {
                return;
            }
            let name_c = Self::vec_to_cstr_null(&name);
            let pub_c = public_id
                .as_deref()
                .map(Self::vec_to_cstr_null)
                .unwrap_or(ptr::null());
            let sys_c = sys
                .as_deref()
                .map(Self::vec_to_cstr_null)
                .unwrap_or(ptr::null());
            SaxDispatcher::notation_decl(sax, ctx, name_c, pub_c, sys_c);
            crate::abi::allocator::xmlFreeImpl(name_c as *mut c_void);
            if !pub_c.is_null() {
                crate::abi::allocator::xmlFreeImpl(pub_c as *mut c_void);
            }
            if !sys_c.is_null() {
                crate::abi::allocator::xmlFreeImpl(sys_c as *mut c_void);
            }
        }
    }

    /// Fire the SAX `unparsedEntityDecl` callback when an NDATA (unparsed)
    /// external general entity declaration is parsed.
    /// Mirrors upstream `xmlParseEntityDecl` NDATA branch.
    fn fire_sax_unparsed_entity_decl(&self, args: &[u8]) {
        if self.sax_blocked() || self.below_delivery_boundary() {
            return;
        }
        unsafe {
            let sax = &*(*self.ctxt).sax;
            if sax.unparsedEntityDecl.is_none() {
                return;
            }
            let a = args.trim_ascii_start();
            // Parameter entities can never be unparsed (NDATA).
            if a.starts_with(b"%") {
                return;
            }
            let (name, _, _) = Self::split_dtd_name_ids(args, true);
            if name.is_empty() {
                return;
            }
            let (has_ndata, notation) = find_ndata_notation(a);
            if !has_ndata {
                return;
            }
            let (_, public_id, sys2) = Self::split_dtd_name_ids(args, true);
            // Only unparsed EXTERNAL entities fire this event; require a
            // SYSTEM or PUBLIC id. (`notation` may be empty when the notation
            // name is elided.)
            if sys2.is_none() && public_id.is_none() {
                return;
            }
            let ctx = (*self.ctxt).userData;
            let name_c = Self::vec_to_cstr_null(&name);
            let pub_c = public_id
                .as_deref()
                .map(Self::vec_to_cstr_null)
                .unwrap_or(ptr::null());
            let sys_c = sys2
                .as_deref()
                .map(Self::vec_to_cstr_null)
                .unwrap_or(ptr::null());
            let notation_c = notation.map(Self::vec_to_cstr_null).unwrap_or(ptr::null());
            SaxDispatcher::unparsed_entity_decl(sax, ctx, name_c, pub_c, sys_c, notation_c);
            crate::abi::allocator::xmlFreeImpl(name_c as *mut c_void);
            if !pub_c.is_null() {
                crate::abi::allocator::xmlFreeImpl(pub_c as *mut c_void);
            }
            if !sys_c.is_null() {
                crate::abi::allocator::xmlFreeImpl(sys_c as *mut c_void);
            }
            if !notation_c.is_null() {
                crate::abi::allocator::xmlFreeImpl(notation_c as *mut c_void);
            }
        }
    }

    /// Ensure a DTD to register internal general entities into.
    ///
    /// When a real document tree exists this is its `intSubset`. Otherwise
    /// (expat-compat SAX parse such as PHP `ext/xml`) we lazily keep an
    /// internal SAX-compatibility-mode document on `ctxt->myDoc` exactly like
    /// upstream `xmlParseEntityDecl` (parser.c): marked `XML_DOC_INTERNAL`, with
    /// a `"fake"` intSubset, so later `xmlGetDocEntity`/PHP's compat `getEntity`
    /// still resolves NOENT-substituted entities that the SAX consumer never
    /// renders as a tree.
    fn ensure_entity_registry_dtd(&mut self) -> Option<*mut _xmlDtd> {
        unsafe {
            let ctxt = self.ctxt;
            let doc = (*ctxt).myDoc;
            let doc = if doc.is_null() {
                let d = crate::xml::tree::new_doc(
                    c"SAX compatibility mode document".as_ptr() as *const xmlChar
                );
                if d.is_null() {
                    return None;
                }
                // An internal, parser-owned document (never handed to the
                // SAX consumer; upstream `xmlNewDoc(SAX_COMPAT_MODE)` + `
                // properties = XML_DOC_INTERNAL`).
                (*d).properties |= crate::abi::types::xmlDocProperties::XML_DOC_INTERNAL as c_int;
                (*ctxt).myDoc = d;
                d
            } else {
                doc
            };
            if (*doc).intSubset.is_null() {
                let sd = crate::xml::dtd::create_int_subset(
                    doc,
                    c"fake".as_ptr() as *const xmlChar,
                    ptr::null(),
                    ptr::null(),
                );
                if sd.is_null() {
                    return None;
                }
            }
            Some((*doc).intSubset)
        }
    }

    /// Parse a `<!ELEMENT name contentmodel>` declaration.
    ///
    /// # Safety
    ///
    /// - `dtd` must be a valid, non-NULL `_xmlDtd` whose hash tables are
    ///   initialized (`dtd::add_element_decl` may insert into them); `args`
    ///   is a byte slice owned by the caller, live for the call; the
    ///   temporary `name_cstr` is freed before returning.
    fn parse_element_decl(dtd: *mut _xmlDtd, args: &[u8]) {
        let args = trim_ascii(args);
        if args.is_empty() {
            return;
        }
        let name_end = args
            .iter()
            .position(|&b| b.is_ascii_whitespace())
            .unwrap_or(args.len());
        let name = &args[..name_end];
        let model = trim_ascii(&args[name_end..]);
        if name.is_empty() || model.is_empty() {
            return;
        }
        let name_cstr = Self::vec_to_cstr_null(name);
        unsafe {
            let elem = if model.eq_ignore_ascii_case(b"EMPTY") {
                crate::xml::dtd::add_element_decl(
                    dtd,
                    name_cstr,
                    XML_ELEMENT_TYPE_EMPTY as c_int,
                    ptr::null_mut(),
                )
            } else if model.eq_ignore_ascii_case(b"ANY") {
                crate::xml::dtd::add_element_decl(
                    dtd,
                    name_cstr,
                    XML_ELEMENT_TYPE_ANY as c_int,
                    ptr::null_mut(),
                )
            } else if model.starts_with(b"(") {
                let (content, is_mixed) = Self::parse_content_model(model);
                let etype = if is_mixed {
                    XML_ELEMENT_TYPE_MIXED as c_int
                } else {
                    XML_ELEMENT_TYPE_ELEMENT as c_int
                };
                crate::xml::dtd::add_element_decl(dtd, name_cstr, etype, content)
            } else {
                ptr::null_mut()
            };
            let _ = elem;
            crate::abi::allocator::xmlFreeImpl(name_cstr as *mut c_void);
        }
    }

    /// Parse a content model: `( cp (','|'|') cp ... ) ('?'|'*'|'+')?`.
    ///
    /// Returns the content-model tree and whether it is a mixed model
    /// (contains `#PCDATA`).
    ///
    /// # Safety
    ///
    /// - `s` is a byte slice owned by the caller, live for the call; the
    ///   returned tree is dereferenced to inspect its `type_` field and must
    ///   be valid (it comes from `parse_cp`, which returns only NULL or a
    ///   valid tree).
    fn parse_content_model(s: &[u8]) -> (*mut _xmlElementContent, bool) {
        let mut idx = 0usize;
        let tree = Self::parse_cp(s, &mut idx);
        if tree.is_null() {
            return (ptr::null_mut(), false);
        }
        let mixed = unsafe {
            let t = &*tree;
            t.type_ == XML_ELEMENT_CONTENT_PCDATA as c_int
        };
        (tree, mixed)
    }

    /// Parse a single content particle (recursive).
    ///
    /// # Safety
    ///
    /// - `s` is a byte slice owned by the caller; `idx` is a valid writable
    ///   index into `s`; the returned `_xmlElementContent` tree is built from
    ///   `dtd::create_content_model` allocations and is acyclic (each
    ///   `c1`/`c2` child is a fresh node); group construction writes
    ///   `c1`/`c2`/`parent`/`ocur` on freshly allocated nodes.
    fn parse_cp(s: &[u8], idx: &mut usize) -> *mut _xmlElementContent {
        skip_ws(s, idx);
        if *idx >= s.len() || s[*idx] != b'(' {
            // A name particle.
            let name = read_name(s, idx);
            if name.is_empty() {
                return ptr::null_mut();
            }
            let node = unsafe {
                crate::xml::dtd::create_content_model(
                    Self::vec_to_cstr_null(name),
                    XML_ELEMENT_CONTENT_ELEMENT as c_int,
                )
            };
            apply_occurrence(node, s, idx);
            return node;
        }
        // Group: '(' cp (sep cp)* ')' occ?
        *idx += 1; // consume '('
        skip_ws(s, idx);

        // Mixed content: (#PCDATA) or (#PCDATA | name | ...)
        let first_is_pcdata = s[*idx..].starts_with(b"#PCDATA");
        if first_is_pcdata {
            let pcdata = unsafe {
                crate::xml::dtd::create_content_model(
                    ptr::null(),
                    XML_ELEMENT_CONTENT_PCDATA as c_int,
                )
            };
            *idx += 7; // consume "#PCDATA"
            skip_ws(s, idx);
            let mut items: Vec<*mut _xmlElementContent> = vec![pcdata];
            let mut group_type = XML_ELEMENT_CONTENT_OR as c_int;
            let mut seen_sep = false;
            loop {
                skip_ws(s, idx);
                if *idx >= s.len() {
                    break;
                }
                let c = s[*idx];
                if c == b')' {
                    *idx += 1;
                    break;
                }
                if c == b'|' || c == b',' {
                    if !seen_sep {
                        seen_sep = true;
                        group_type = if c == b'|' {
                            XML_ELEMENT_CONTENT_OR as c_int
                        } else {
                            XML_ELEMENT_CONTENT_SEQ as c_int
                        };
                    }
                    *idx += 1;
                    skip_ws(s, idx);
                    let child = Self::parse_cp(s, idx);
                    if child.is_null() {
                        break;
                    }
                    items.push(child);
                    continue;
                }
                break;
            }
            // Build a left-leaning chain so the dump flattens to
            // `(#PCDATA | a | b)`.
            let mut node = items[0];
            for item in &items[1..] {
                let group =
                    unsafe { crate::xml::dtd::create_content_model(ptr::null(), group_type) };
                unsafe {
                    (*group).c1 = node;
                    (*node).parent = group;
                    (*group).c2 = *item;
                    (*(*item)).parent = group;
                    (*group).ocur = XML_ELEMENT_CONTENT_ONCE as c_int;
                }
                node = group;
            }
            apply_occurrence(node, s, idx);
            return node;
        }

        // Element-content group: (a, b, c) or (a | b | c), possibly nested.
        let mut items: Vec<(*mut _xmlElementContent, u8)> = Vec::new();
        loop {
            skip_ws(s, idx);
            if *idx >= s.len() {
                break;
            }
            let c = s[*idx];
            if c == b')' {
                *idx += 1;
                break;
            }
            if c == b',' || c == b'|' {
                *idx += 1;
                continue;
            }
            let child = Self::parse_cp(s, idx);
            if child.is_null() {
                break;
            }
            items.push((child, c));
        }
        if items.is_empty() {
            return ptr::null_mut();
        }
        let sep = if items.len() > 1 { items[1].1 } else { b',' };
        let group_type = if sep == b'|' {
            XML_ELEMENT_CONTENT_OR as c_int
        } else {
            XML_ELEMENT_CONTENT_SEQ as c_int
        };
        // Build a left-leaning chain: ((a,b),c) with OR/SEQ connectors.
        let mut node = items[0].0;
        for item in &items[1..] {
            let group = unsafe { crate::xml::dtd::create_content_model(ptr::null(), group_type) };
            unsafe {
                (*group).c1 = node;
                (*node).parent = group;
                (*group).c2 = item.0;
                (*(item.0)).parent = group;
                (*group).ocur = XML_ELEMENT_CONTENT_ONCE as c_int;
            }
            node = group;
        }
        apply_occurrence(node, s, idx);
        node
    }

    /// Whether a general `<!ENTITY ...>` declaration must be registered into
    /// the expat-compat entity registry — upstream `xmlParseEntityDecl`'s SAX-
    /// mode rule (parser.c): an internal entity with a quoted VALUE registers
    /// unconditionally; an external SYSTEM/PUBLIC general PARSED entity
    /// registers only when entity substitution was requested
    /// (`ctxt->replaceEntities != 0`); parameter entities and NDATA-unparsed
    /// entities never enter the registry (their SAX `entityDecl`/
    /// `unparsedEntityDecl` events carry them to the consumer).
    ///
    /// # Safety
    ///
    /// - `self.ctxt` must be a valid, initialized `_xmlParserCtxt`; `args` is a
    ///   caller-owned byte slice (the declaration body after `<!ENTITY`), live
    ///   for the call.
    fn compat_entity_must_register(&self, args: &[u8]) -> bool {
        let args = trim_ascii(args);
        if args.is_empty() {
            return false;
        }
        let mut rest = args;
        let mut is_param = false;
        if rest.starts_with(b"%") {
            is_param = true;
            rest = trim_ascii(&rest[1..]);
        }
        if is_param {
            // Parameter entities resolve through the SAX `getParameterEntity`
            // callback, never through the general-entity `getEntity`;
            // upstream keeps them out of the SAX-compat document.
            return false;
        }
        let name_end = rest
            .iter()
            .position(|&b| b.is_ascii_whitespace())
            .unwrap_or(rest.len());
        let tail = trim_ascii(&rest[name_end..]);
        if tail.is_empty() {
            return false;
        }
        if tail.starts_with(b"SYSTEM") || tail.starts_with(b"PUBLIC") {
            // External general entity. Upstream registers it as
            // XML_EXTERNAL_GENERAL_PARSED_ENTITY only when substitution was
            // requested; an NDATA suffix makes it UNPARSED, which never
            // enters the registry (only the SAX unparsedEntityDecl event
            // fires).
            if find_ndata_notation(tail).0 {
                return false;
            }
            return unsafe { (*self.ctxt).replaceEntities } != 0;
        }
        // Internal general entity with a quoted value.
        true
    }

    /// Parse a `<!ENTITY ...>` declaration (general or parameter).
    ///
    /// # Safety
    ///
    /// - `dtd` must be a valid, non-NULL `_xmlDtd` (passed to
    ///   `entities::add_entity`); `args` is a caller-owned byte slice, live
    ///   for the call; temporary C strings are freed before returning.
    fn parse_entity_decl(&mut self, dtd: *mut _xmlDtd, args: &[u8]) {
        let args = trim_ascii(args);
        if args.is_empty() {
            return;
        }
        let mut rest = args;
        let mut is_param = false;
        if rest.starts_with(b"%") {
            is_param = true;
            rest = trim_ascii(&rest[1..]);
        }
        let name_end = rest
            .iter()
            .position(|&b| b.is_ascii_whitespace())
            .unwrap_or(rest.len());
        let name = &rest[..name_end];
        let tail = trim_ascii(&rest[name_end..]);
        if name.is_empty() {
            return;
        }
        let etype = if is_param {
            XML_INTERNAL_PARAMETER_ENTITY as c_int
        } else {
            XML_INTERNAL_GENERAL_ENTITY as c_int
        };
        unsafe {
            let name_cstr = Self::vec_to_cstr_null(name);
            // External: SYSTEM "uri" / PUBLIC "pub" "uri", optionally with an
            // NDATA <notation> suffix (making it an UNPARSED entity).
            if tail.starts_with(b"SYSTEM") || tail.starts_with(b"PUBLIC") {
                let after_kw = trim_ascii(&tail[6..]);
                // UPSTREAM-PARITY (xmlParseEntityDecl): for a SYSTEM-only
                // declaration the first literal is the SystemID; for PUBLIC
                // the first is the public ID and the second the SystemID.
                let (pub_id, sys_id) = if tail.starts_with(b"PUBLIC") {
                    split_two_quoted(after_kw)
                } else {
                    (None, read_quoted(after_kw))
                };
                // ndata (only meaningful for general, non-parameter entities):
                // an unparsed entity typed by a notation declaration. The
                // mere presence of NDATA classifies it as unparsed even if no
                // notation name follows (DOMEntity_fields tolerates the empty
                // form and reports an empty notation).
                let (has_ndata, notation) = if !is_param {
                    find_ndata_notation(tail)
                } else {
                    (false, None)
                };
                // UPSTREAM-PARITY (xmlParseEntityDecl / xmlSAX2EntityDecl): a
                // general non-NDATA external entity is PARSED; an NDATA entity
                // is UNPARSED and its notation name is carried on the entity's
                // content so DOMEntity::$notationName resolves it (the SYSTEM
                // id / public id go to SystemID / ExternalID).
                let base_type = if is_param {
                    XML_EXTERNAL_PARAMETER_ENTITY as c_int
                } else {
                    XML_EXTERNAL_GENERAL_PARSED_ENTITY as c_int
                };
                let external_type = if has_ndata && !is_param {
                    crate::abi::types::xmlEntityType::XML_EXTERNAL_GENERAL_UNPARSED_ENTITY as c_int
                } else {
                    base_type
                };
                // UPSTREAM-PARITY (parser.c xmlParseExternalID): the quoted
                // literals are stored even when EMPTY (`PUBLIC ""` keeps an
                // allocated empty ExternalID, not NULL).
                let pub_c = pub_id
                    .map(Self::vec_to_cstr_keep_empty)
                    .unwrap_or(ptr::null());
                let sys_c = sys_id
                    .map(Self::vec_to_cstr_keep_empty)
                    .unwrap_or(ptr::null());
                // Only an NDATA (unparsed) entity carries a notation name on its
                // content (present but nameless NDATA keeps an empty string); a
                // parsed external entity has no content of its own.
                let notation_c = if has_ndata && !is_param {
                    Some(
                        notation
                            .as_deref()
                            .map(Self::vec_to_cstr_null)
                            .unwrap_or_else(|| Self::vec_to_cstr_null(b"")),
                    )
                } else {
                    None
                };
                let ent = crate::xml::entities::add_entity(
                    dtd,
                    name_cstr,
                    external_type,
                    pub_c,
                    sys_c,
                    notation_c.unwrap_or(ptr::null()),
                );
                // UPSTREAM-PARITY (SAX2.c xmlSAX2EntityDecl): a declared
                // external entity's `URI` is the SystemID resolved against the
                // parser base — the newest input with a filename, else
                // ctxt->directory (which php sets to the CWD for memory
                // loads). xmlNodeGetBase on an ENTITY_DECL node returns
                // ent->URI verbatim (DTDNamedNodeMap: "mypicture.gif"
                // resolves to "<dir>/mypicture.gif").
                if !ent.is_null() && !sys_c.is_null() && unsafe { (*ent).URI.is_null() } {
                    unsafe {
                        let mut base: *const c_char = ptr::null();
                        let c = &*(*self).ctxt;
                        if !c.inputTab.is_null() {
                            let mut i = c.inputNr - 1;
                            while i >= 0 {
                                let inp = *(c.inputTab.add(i as usize));
                                if !inp.is_null() && !(*inp).filename.is_null() {
                                    base = (*inp).filename;
                                    break;
                                }
                                if i == 0 {
                                    break;
                                }
                                i -= 1;
                            }
                        }
                        if base.is_null() {
                            base = c.directory;
                        }
                        if !base.is_null() {
                            let mut uri: *mut xmlChar = ptr::null_mut();
                            let res = crate::abi::exports_uri::xmlBuildURISafe(
                                sys_c as *const c_char,
                                base,
                                &mut uri,
                            );
                            if res == 0 && !uri.is_null() {
                                (*ent).URI = uri;
                            } else if !uri.is_null() {
                                crate::abi::allocator::xmlFreeImpl(uri as *mut c_void);
                            }
                        }
                    }
                }
                if !pub_c.is_null() {
                    crate::abi::allocator::xmlFreeImpl(pub_c as *mut c_void);
                }
                if !sys_c.is_null() {
                    crate::abi::allocator::xmlFreeImpl(sys_c as *mut c_void);
                }
                if let Some(n) = notation_c {
                    if !n.is_null() {
                        crate::abi::allocator::xmlFreeImpl(n as *mut c_void);
                    }
                }
            } else {
                // Internal: quoted value.
                let value = read_quoted(tail);
                // UPSTREAM-PARITY (parser.c xmlParseEntityDecl): the raw
                // value text is kept as the entity's `orig`, so dumps print
                // it verbatim (bug67081: a parameter-entity reference such
                // as `%coreattrs;` inside the value stays raw instead of
                // being %-escaped by the no-orig fallback path).
                let v = value.map(Self::vec_to_cstr_null).unwrap_or(ptr::null());
                crate::xml::entities::add_entity_with_orig(
                    dtd,
                    name_cstr,
                    etype,
                    ptr::null(),
                    ptr::null(),
                    v,
                    v,
                );
                if !v.is_null() {
                    crate::abi::allocator::xmlFreeImpl(v as *mut c_void);
                }
            }
            crate::abi::allocator::xmlFreeImpl(name_cstr as *mut c_void);
        }
    }

    /// Collect the DTD attribute DEFAULTS of one `<!ATTLIST ...>` body into
    /// the parser-scoped defaults table (upstream `ctxt->attsDefault`, filled
    /// by xmlParseAttributeListDecl): every `attr type default` whose default
    /// declaration carries a value (`#FIXED "v"` or a literal `"v"`).
    /// Mirrors the scan of `parse_attlist_decl` without the DTD writes, and
    /// mirrors upstream's skip of `xmlns`/`xmlns:*` defaults.
    fn collect_attlist_defaults(&mut self, args: &[u8]) {
        let args = trim_ascii(args);
        if args.is_empty() {
            return;
        }
        let name_end = args
            .iter()
            .position(|&b| b.is_ascii_whitespace())
            .unwrap_or(args.len());
        let elem_name = args[..name_end].to_vec();
        if elem_name.is_empty() {
            return;
        }
        let mut rest = trim_ascii(&args[name_end..]);
        let mut defaults: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        while !rest.is_empty() {
            let aend = rest
                .iter()
                .position(|&b| b.is_ascii_whitespace())
                .unwrap_or(rest.len());
            let attr_name = &rest[..aend];
            rest = trim_ascii(&rest[aend..]);
            if attr_name.is_empty() {
                break;
            }
            if attr_name == b"xmlns" || attr_name.starts_with(b"xmlns:") {
                // Skip the type + default anyway to keep the scan aligned.
                let consumed = skip_dtd_attr_type(rest);
                rest = trim_ascii(&rest[consumed..]);
                let (_, _, consumed2) = parse_attr_default(rest);
                rest = trim_ascii(&rest[consumed2..]);
                continue;
            }
            let consumed = skip_dtd_attr_type(rest);
            rest = trim_ascii(&rest[consumed..]);
            let (_, default_val, consumed2) = parse_attr_default(rest);
            rest = trim_ascii(&rest[consumed2..]);
            if let Some(v) = default_val {
                defaults.push((attr_name.to_vec(), v.to_vec()));
            }
        }
        if defaults.is_empty() {
            return;
        }
        if let Some(e) = self
            .dtd_attr_defaults
            .iter_mut()
            .find(|(n, _)| n == &elem_name)
        {
            e.1.extend(defaults);
        } else {
            self.dtd_attr_defaults.push((elem_name, defaults));
        }
    }

    /// Parse a `<!ATTLIST elem attr type default ...>` declaration.
    ///
    /// # Safety
    ///
    /// - `dtd` must be a valid, non-NULL `_xmlDtd`; `args` is a caller-owned
    ///   byte slice live for the call; `elem_decl` (from
    ///   `dtd::get_element_decl_created`) must be NULL or a valid `_xmlElement`
    ///   declaration; the enumeration tree from `parse_attr_type` (if any) is
    ///   handed to `dtd::add_attribute_decl`.
    fn parse_attlist_decl(dtd: *mut _xmlDtd, args: &[u8]) {
        let args = trim_ascii(args);
        if args.is_empty() {
            return;
        }
        let name_end = args
            .iter()
            .position(|&b| b.is_ascii_whitespace())
            .unwrap_or(args.len());
        let elem_name = &args[..name_end];
        let mut rest = trim_ascii(&args[name_end..]);
        let elem_cstr = Self::vec_to_cstr_null(elem_name);
        unsafe {
            // UPSTREAM-PARITY (parser.c xmlParseAttlistDecl): an ATTLIST for
            // an undeclared element creates an UNDEFINED element declaration
            // (valid.c xmlGetDtdElementDesc).
            let elem_decl = crate::xml::dtd::get_element_decl_created(dtd, elem_cstr);
            if elem_decl.is_null() {
                crate::abi::allocator::xmlFreeImpl(elem_cstr as *mut c_void);
                return;
            }
            while !rest.is_empty() {
                let aend = rest
                    .iter()
                    .position(|&b| b.is_ascii_whitespace())
                    .unwrap_or(rest.len());
                let attr_name = &rest[..aend];
                rest = trim_ascii(&rest[aend..]);
                if attr_name.is_empty() {
                    break;
                }
                // Attribute type.
                let (atype, tree, consumed) = parse_attr_type(rest);
                rest = trim_ascii(&rest[consumed..]);
                // Default declaration.
                let (def, default_val, consumed2) = parse_attr_default(rest);
                rest = trim_ascii(&rest[consumed2..]);
                // UPSTREAM-PARITY (SAX2.c xmlSAX2AttributeDecl): the
                // declaration is keyed by the attribute's LOCAL name plus its
                // PREFIX (valid.c xmlAddAttributeDecl: xmlHashAdd3(name, ns,
                // elem) with ns = prefix). The raw QName is split here so
                // `<!ATTLIST root p:A ...>` is looked up as ("A", "p",
                // "root") — xmlGetDtdQAttrDesc / xmlHasNsProp rely on that.
                let (local_cstr, prefix_cstr) = match attr_name.iter().position(|&b| b == b':') {
                    Some(c) if c > 0 && c + 1 < attr_name.len() => (
                        Self::vec_to_cstr_null(&attr_name[c + 1..]),
                        Self::vec_to_cstr_null(&attr_name[..c]),
                    ),
                    _ => (Self::vec_to_cstr_null(attr_name), ptr::null()),
                };
                let dv = default_val
                    .as_ref()
                    .map(|s| Self::vec_to_cstr_null(s))
                    .unwrap_or(ptr::null());
                crate::xml::dtd::add_attribute_decl(
                    dtd,
                    elem_decl,
                    local_cstr,
                    prefix_cstr,
                    atype,
                    def,
                    dv,
                    tree,
                );
                if !dv.is_null() {
                    crate::abi::allocator::xmlFreeImpl(dv as *mut c_void);
                }
                if !prefix_cstr.is_null() {
                    crate::abi::allocator::xmlFreeImpl(prefix_cstr as *mut c_void);
                }
                crate::abi::allocator::xmlFreeImpl(local_cstr as *mut c_void);
            }
            crate::abi::allocator::xmlFreeImpl(elem_cstr as *mut c_void);
        }
    }

    /// Parse a `<!NOTATION ...>` declaration.
    ///
    /// # Safety
    ///
    /// - `dtd` must be a valid, non-NULL `_xmlDtd`; `args` is a caller-owned
    ///   byte slice live for the call; temporary C strings are freed before
    ///   returning.
    fn parse_notation_decl(dtd: *mut _xmlDtd, args: &[u8]) {
        let args = trim_ascii(args);
        if args.is_empty() {
            return;
        }
        let name_end = args
            .iter()
            .position(|&b| b.is_ascii_whitespace())
            .unwrap_or(args.len());
        let name = &args[..name_end];
        let tail = trim_ascii(&args[name_end..]);
        let name_cstr = Self::vec_to_cstr_null(name);
        unsafe {
            // UPSTREAM-PARITY (parser.c xmlParseNotationDecl): `PUBLIC "pub"
            // "sys"` puts the public id in ExternalID and the URI in
            // SystemID; `SYSTEM "sys"` leaves ExternalID absent and stores
            // the URI in SystemID only (php DOMNotation::$publicId reads
            // ExternalID and maps NULL to "").
            let (pub_id, sys_id) = if tail.len() >= 6 && tail[..6].eq_ignore_ascii_case(b"PUBLIC") {
                split_two_quoted(trim_ascii(&tail[6..]))
            } else if tail.len() >= 6 && tail[..6].eq_ignore_ascii_case(b"SYSTEM") {
                (None, read_quoted(trim_ascii(&tail[6..])))
            } else {
                split_two_quoted(tail)
            };
            let pub_c = pub_id.map(Self::vec_to_cstr_null).unwrap_or(ptr::null());
            let sys_c = sys_id.map(Self::vec_to_cstr_null).unwrap_or(ptr::null());
            crate::xml::dtd::add_notation_decl(dtd, name_cstr, pub_c, sys_c);
            if !pub_c.is_null() {
                crate::abi::allocator::xmlFreeImpl(pub_c as *mut c_void);
            }
            if !sys_c.is_null() {
                crate::abi::allocator::xmlFreeImpl(sys_c as *mut c_void);
            }
            crate::abi::allocator::xmlFreeImpl(name_cstr as *mut c_void);
        }
    }

    /// Parse the external DTD subset.
    ///
    /// When XML_PARSE_DTDLOAD is set upstream resolves and parses the
    /// referenced external DTD, merging its declarations into the document's
    /// external subset (`doc->extSubset`). The candidate mirrors this for the
    /// common file case: resolve `sys_id` relative to the document URL's
    /// directory, read the file, and parse its declarations into the DTD node.
    ///
    /// # Safety
    ///
    /// - `self.ctxt` must be a valid, initialized `_xmlParserCtxt` with a
    ///   valid `sax` handler and matching `userData`; the SAX event receives
    ///   NUL-terminated strings that are intentionally leaked, remaining
    ///   valid until the process exits.
    fn parse_external_subset(
        &mut self,
        name: &[u8],
        ext_id: Option<&[u8]>,
        sys_id: Option<&[u8]>,
    ) -> Result<(), ()> {
        // Create the external subset DTD node (upstream xmlParserLoadSubset
        // -> xmlNewDtd), which becomes doc->extSubset. nokogiri reads it back
        // as Document#external_subset.
        let name_cstr = Self::vec_to_cstr_null(name);
        let ext_cstr = ext_id.map(Self::vec_to_cstr_null).unwrap_or(ptr::null());
        let sys_cstr = sys_id.map(Self::vec_to_cstr_null).unwrap_or(ptr::null());
        let dtd = unsafe {
            crate::abi::exports_xml2::xmlNewDtd(
                (*self.ctxt).myDoc,
                name_cstr as *const xmlChar,
                ext_cstr as *const xmlChar,
                sys_cstr as *const xmlChar,
            )
        };
        if !dtd.is_null() {
            unsafe {
                (*self.ctxt).inSubset = 1;
            }
            if let Some(sys) = sys_id {
                self.load_external_dtd_file(dtd, sys);
            }
            unsafe {
                (*self.ctxt).inSubset = 0;
            }
        }

        // Fire externalSubset SAX event
        if !self.is_sax_disabled() {
            unsafe {
                let sax = &*(*self.ctxt).sax;
                let ctx = (*self.ctxt).userData;
                SaxDispatcher::external_subset(sax, ctx, name_cstr, ext_cstr, sys_cstr);
            }
        }

        if !name_cstr.is_null() {
            unsafe { crate::abi::allocator::xmlFreeImpl(name_cstr as *mut c_void) };
        }
        if !ext_cstr.is_null() {
            unsafe { crate::abi::allocator::xmlFreeImpl(ext_cstr as *mut c_void) };
        }
        if !sys_cstr.is_null() {
            unsafe { crate::abi::allocator::xmlFreeImpl(sys_cstr as *mut c_void) };
        }

        Ok(())
    }

    /// Resolve a DTD system ID relative to the document URL's directory,
    /// read the file, and parse its declarations into `dtd`.
    fn load_external_dtd_file(&mut self, dtd: *mut _xmlDtd, sys_id: &[u8]) {
        let path = self.resolve_dtd_path(sys_id);
        let content = match path {
            Some(p) => std::fs::read(&p).ok(),
            None => None,
        };
        let Some(content) = content else { return };
        if content.is_empty() {
            return;
        }
        // Parse declarations from the external file (same grammar as the
        // internal subset, but the file root has no <!DOCTYPE> wrapper). The
        // shared fragment processor handles markup declarations, comments,
        // PIs, inline parameter-entity references and decl-level PE refs
        // (nested external PEs included).
        let mut dtd_opt = Some(dtd);
        self.process_dtd_fragment(&mut dtd_opt, &content, 0, true);
    }

    /// Resolve a DTD system id to a filesystem path, honoring a relative
    /// reference against the document URL's directory.
    fn resolve_dtd_path(&self, sys_id: &[u8]) -> Option<std::path::PathBuf> {
        let sys = core::str::from_utf8(sys_id).ok()?;
        // UPSTREAM-PARITY: a file:/// system id (php rawurlencode'd absolute
        // DTD paths, e.g. "file:///%2Fsrcb%2F...%2Fdtdexample.dtd") resolves
        // to the percent-decoded local path; non-file URIs have no local
        // resolution.
        if let Some(rest) = sys.strip_prefix("file://") {
            // "file:///abs/path": the path is the whole remainder;
            // "file://host/abs/path": drop the authority component.
            let path_enc = if rest.starts_with('/') {
                rest
            } else {
                match rest.splitn(2, '/').nth(1) {
                    Some(p) => p,
                    None => rest,
                }
            };
            let decoded = percent_decode_uri(path_enc);
            return Some(std::path::PathBuf::from(decoded));
        }
        if sys.contains("://") {
            // Non-file URI (http etc.): no local resolution.
            return None;
        }
        let doc_url = unsafe { (*self.ctxt).myDoc };
        let mut dir: Option<std::path::PathBuf> = None;
        if !doc_url.is_null() {
            let url = unsafe { (*doc_url).URL };
            if !url.is_null() {
                let url_str = unsafe { std::ffi::CStr::from_ptr(url as *const c_char) }
                    .to_str()
                    .ok();
                dir = url_str.and_then(|u| {
                    let p = std::path::Path::new(u);
                    p.parent().map(|d| d.to_path_buf())
                });
            }
        }
        // UPSTREAM-PARITY: memory loads resolve relative ids against the
        // parser's base directory (`ctxt->directory`, set by php for
        // DOMDocument::loadXML from the CWD) — the document URL is only
        // assigned after the parse, so it cannot serve as a base here.
        if dir.is_none() {
            let d = unsafe { (*self.ctxt).directory };
            if !d.is_null() {
                let ds = unsafe { std::ffi::CStr::from_ptr(d as *const c_char) }
                    .to_str()
                    .ok()
                    .map(|s| s.to_string());
                dir = ds.map(std::path::PathBuf::from);
            }
        }
        let sys_path = std::path::Path::new(sys);
        if sys_path.is_absolute() {
            Some(sys_path.to_path_buf())
        } else if let Some(d) = dir {
            Some(d.join(sys_path))
        } else {
            Some(sys_path.to_path_buf())
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Content parsing
    // ─────────────────────────────────────────────────────────────────────────

    /// Parse the root element (upstream `xmlParseDocument` element branch):
    /// the root start tag was pushed back by the prolog. After the root,
    /// trailing misc (comments/PIs/whitespace) is consumed and any
    /// remaining input raises "Extra content at the end of the document"
    /// (upstream `xmlParserCheckEOF` with XML_ERR_DOCUMENT_END).
    ///
    /// # Safety
    ///
    /// - `self.ctxt` must be a valid, initialized `_xmlParserCtxt`; the
    ///   `disableSAX` and `errNo` fields are read.
    fn parse_content(&mut self) -> Result<(), ()> {
        let token = self.tokenizer.next_token_raw();
        self.raise_pending_errors();
        match token {
            XmlToken::StartTag {
                name,
                attributes,
                attr_end,
                attr_start,
                end_pos,
                empty,
                unterminated,
            } => {
                if unterminated {
                    // Errors (incl. "Couldn't find end of Start Tag") were
                    // already raised; upstream's element parse failed. The
                    // construct may continue on a later push call, so an
                    // incremental probe (or eager-partial delivery) must not
                    // deliver.
                    if self.probe || self.partial_delivery {
                        self.truncated_abort = true;
                    }
                    return Err(());
                }
                self.parse_element(name, attributes, attr_end, attr_start, end_pos, empty)?;

                // UPSTREAM-PARITY: a catastrophic stop (disableSAX == 2)
                // ends the parse without the trailing checks.
                if unsafe { (*self.ctxt).disableSAX } == 2 {
                    return Ok(());
                }

                // UPSTREAM-PARITY: trailing misc, then xmlParserCheckEOF.
                self.parse_misc_after_root()?;
                if !self.tokenizer.input().current_ref().remaining().is_empty() {
                    // UPSTREAM-PARITY (R-000166): the fatal is raised only
                    // when no prior error was recorded; otherwise the
                    // remaining input is ignored (upstream sets EOF and
                    // finishes).
                    if unsafe { (*self.ctxt).errNo } == crate::abi::types::XML_ERR_OK {
                        self.raise_error_now(
                            XML_FROM_PARSER,
                            XML_ERR_DOCUMENT_END,
                            xmlErrorLevel::XML_ERR_FATAL as c_int,
                            "Extra content at the end of the document\n".to_string(),
                            None,
                            None,
                            None,
                            0,
                        );
                        return Err(());
                    }
                }
                Ok(())
            }
            _ => Err(()),
        }
    }

    /// Consume trailing misc after the root element (upstream
    /// `xmlParseMisc`): blanks, PIs, comments. Anything else (including a
    /// second root or stray text) raises "Extra content at the end of the
    /// document" at the token start (upstream `xmlParserCheckEOF`).
    ///
    /// # Safety
    ///
    /// - `self.ctxt` must be a valid, initialized `_xmlParserCtxt`; the
    ///   `errNo` field is read to gate the "Extra content" error.
    fn parse_misc_after_root(&mut self) -> Result<(), ()> {
        loop {
            let (token, start) = self.tokenizer.next_token_with_start();
            self.raise_pending_errors();
            match token {
                XmlToken::Eof => return Ok(()),
                XmlToken::Comment { data, unterminated } => {
                    if unterminated && (self.probe || self.partial_delivery) {
                        // See parse_prolog's Comment arm (SP-14.3.1-8).
                        self.truncated_abort = true;
                        return Err(());
                    }
                    self.sax_comment(&data);
                }
                XmlToken::ProcessingInstruction {
                    target,
                    data,
                    unterminated,
                    ..
                } => {
                    // See parse_prolog's PI arm (KEY-3): pause in probes.
                    if unterminated && (self.probe || self.partial_delivery) {
                        self.truncated_abort = true;
                        return Err(());
                    }
                    self.sax_pi(&target, &data);
                }
                XmlToken::Characters(data) => {
                    if data.iter().any(|&b| !b.is_ascii_whitespace()) {
                        // UPSTREAM-PARITY (parser.c XML_PARSER_EPILOG): the
                        // fatal "Extra content" is raised only when no prior
                        // error was recorded (errNo == XML_ERR_OK) — a prior
                        // namespace/other error suppresses it, the stray
                        // content is ignored, and the document stays
                        // well-formed (R-000166).
                        if unsafe { (*self.ctxt).errNo } == crate::abi::types::XML_ERR_OK {
                            self.raise_error_at(
                                XML_FROM_PARSER,
                                XML_ERR_DOCUMENT_END,
                                xmlErrorLevel::XML_ERR_FATAL as c_int,
                                "Extra content at the end of the document\n".to_string(),
                                None,
                                None,
                                None,
                                0,
                                start,
                            );
                            return Err(());
                        }
                        return Ok(());
                    }
                }
                _ => {
                    // UPSTREAM-PARITY: same errNo gate as above.
                    if unsafe { (*self.ctxt).errNo } == crate::abi::types::XML_ERR_OK {
                        self.raise_error_at(
                            XML_FROM_PARSER,
                            XML_ERR_DOCUMENT_END,
                            xmlErrorLevel::XML_ERR_FATAL as c_int,
                            "Extra content at the end of the document\n".to_string(),
                            None,
                            None,
                            None,
                            0,
                            start,
                        );
                        return Err(());
                    }
                    return Ok(());
                }
            }
        }
    }

    /// Parse a single element: start tag, content, and matching end tag.
    ///
    /// # Safety
    ///
    /// - `self.ctxt` must be a valid, initialized `_xmlParserCtxt` with valid
    ///   `sax`/`userData`; `myDoc` must be NULL or a valid `_xmlDoc`; `node`
    ///   may be NULL or a valid `_xmlNode` (passed to `tree::search_ns`);
    ///   the byte-slice inputs are owned by the caller and live for the
    ///   call.
    fn parse_element(
        &mut self,
        name: Vec<u8>,
        attributes: Vec<(Vec<u8>, Vec<u8>)>,
        attr_end: Vec<usize>,
        attr_start: Vec<usize>,
        end_pos: usize,
        empty: bool,
    ) -> Result<(), ()> {
        // Line where this element's start tag appeared (used for upstream
        // "Opening and ending tag mismatch: X line N and Y" diagnostics).
        let open_line = {
            let (l, _, _) = self.tokenizer.current_pos();
            l
        };
        // Push element name onto the context name stack
        self.push_name(&name);

        // UPSTREAM-PARITY: attribute values are parsed with
        // xmlParseAttValueInternal, which substitutes character references
        // always, predefined entities always, and declared entities when
        // XML_PARSE_NOENT is set. The tokenizer scans the raw value, so the
        // substitution happens here.
        let mut new_attributes: Vec<(Vec<u8>, Vec<u8>, bool)> =
            Vec::with_capacity(attributes.len());
        for (idx, (n, v)) in attributes.into_iter().enumerate() {
            // UPSTREAM-PARITY (parser.c xmlParseStartTag2): an attribute value
            // containing a reference is duplicated and passed to the SAX
            // layer with a non-NULL valueEnd, which forces the non-compact
            // xmlNodeParseAttValue path (R-000120).
            let had_ref = v.contains(&b'&');
            let value_start = attr_start.get(idx).copied().unwrap_or(0);
            let value = self.substitute_refs(&v, value_start)?;
            new_attributes.push((n, value, had_ref));
        }
        let attributes = new_attributes;

        // SAX1 vs SAX2 element dispatch follows upstream xmlCtxtInitializeLate
        // (parser.c). SAX1 consumers (PHP xml_parser_create, non-namespace
        // expat-compat) receive the raw QName and the full attribute list with
        // xmlns declarations as ordinary attributes and NO namespace
        // processing — upstream xmlParseStartTag. bug50576/bug72714.
        let sax2 = self.sax2_mode();

        // Separate namespace declarations from regular attributes.
        let mut ns_decls: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        let mut regular_attrs: Vec<(Option<Vec<u8>>, Vec<u8>, Vec<u8>, bool)> = Vec::new();
        // Parallel to `regular_attrs`: the byte offset just past each
        // attribute value's closing quote (upstream's position for
        // namespace diagnostics).
        let mut attr_pos: Vec<usize> = Vec::new();

        if sax2 {
            for (idx, (attr_name, attr_value, _had_ref)) in attributes.iter().enumerate() {
                // UPSTREAM-PARITY (parser.c xmlParseStartTag2): the default
                // namespace declaration warns when the URI is not absolute
                // (xmlns: URI %s is not absolute); the prefixed form warns only
                // in pedantic mode. The diagnostic is attributed to the position
                // just past the value's closing quote.
                if attr_name == b"xmlns" || attr_name.starts_with(b"xmlns:") {
                    if attr_name == b"xmlns" {
                        // UPSTREAM-PARITY (parser.c xmlParseStartTag2): a
                        // non-empty default namespace URI is first validated
                        // with xmlParseURISafe (a parse failure warns
                        // XML_WAR_NS_URI); only a URI that parses but lacks a
                        // scheme warns "not absolute" (xmlNsWarn, default
                        // namespace only — not pedantic-gated).
                        if !attr_value.is_empty()
                            && !crate::abi::exports_uri::uri_reference_valid(attr_value, false)
                        {
                            let pos = attr_end.get(idx).copied().unwrap_or(0);
                            self.raise_error_at(
                                XML_FROM_NAMESPACE,
                                XML_WAR_NS_URI,
                                xmlErrorLevel::XML_ERR_WARNING as c_int,
                                format!(
                                    "xmlns: '{}' is not a valid URI\n",
                                    String::from_utf8_lossy(attr_value)
                                ),
                                Some(attr_value.clone()),
                                None,
                                None,
                                0,
                                pos,
                            );
                        } else if !attr_value.is_empty() && !has_uri_scheme(attr_value) {
                            let pos = attr_end.get(idx).copied().unwrap_or(0);
                            self.raise_error_at(
                                XML_FROM_NAMESPACE,
                                XML_WAR_NS_URI_RELATIVE,
                                xmlErrorLevel::XML_ERR_WARNING as c_int,
                                format!(
                                    "xmlns: URI {} is not absolute\n",
                                    String::from_utf8_lossy(attr_value)
                                ),
                                Some(attr_value.clone()),
                                None,
                                None,
                                0,
                                pos,
                            );
                        }
                    } else {
                        // Prefixed declarations: xmlParseURISafe runs for any
                        // non-empty value; the relative-URI warning is
                        // pedantic-only (upstream xmlNsWarn under pedantic).
                        let prefix = &attr_name[b"xmlns:".len()..];
                        if !attr_value.is_empty()
                            && !crate::abi::exports_uri::uri_reference_valid(attr_value, false)
                        {
                            let pos = attr_end.get(idx).copied().unwrap_or(0);
                            self.raise_error_at(
                                XML_FROM_NAMESPACE,
                                XML_WAR_NS_URI,
                                xmlErrorLevel::XML_ERR_WARNING as c_int,
                                format!(
                                    "xmlns:{}: '{}' is not a valid URI\n",
                                    String::from_utf8_lossy(prefix),
                                    String::from_utf8_lossy(attr_value)
                                ),
                                Some(attr_value.clone()),
                                None,
                                None,
                                0,
                                pos,
                            );
                        } else if self.is_pedantic()
                            && !attr_value.is_empty()
                            && !has_uri_scheme(attr_value)
                        {
                            let pos = attr_end.get(idx).copied().unwrap_or(0);
                            self.raise_error_at(
                                XML_FROM_NAMESPACE,
                                XML_WAR_NS_URI_RELATIVE,
                                xmlErrorLevel::XML_ERR_WARNING as c_int,
                                format!(
                                    "xmlns:{}: URI {} is not absolute\n",
                                    String::from_utf8_lossy(prefix),
                                    String::from_utf8_lossy(attr_value)
                                ),
                                Some(attr_value.clone()),
                                None,
                                None,
                                0,
                                pos,
                            );
                        }
                    }
                }
                // UPSTREAM-PARITY (parser.c xmlParseStartTag2): namespace
                // declaration errors are reported (non-fatal) and the invalid
                // declaration is skipped. R-000166.
                if attr_name == b"xmlns" || attr_name.starts_with(b"xmlns:") {
                    let pos = attr_end.get(idx).copied().unwrap_or(0);
                    let xml_ns_uri = b"http://www.w3.org/XML/1998/namespace";
                    let xmlns_uri = b"http://www.w3.org/2000/xmlns/";
                    let (decl_prefix, decl_value): (Vec<u8>, Vec<u8>) = if attr_name == b"xmlns" {
                        (Vec::new(), attr_value.clone())
                    } else {
                        (attr_name[b"xmlns:".len()..].to_vec(), attr_value.clone())
                    };
                    let mut skip_decl = false;
                    if decl_prefix == b"xml" {
                        if decl_value != xml_ns_uri {
                            self.raise_error_at(
                                XML_FROM_NAMESPACE,
                                XML_NS_ERR_XML_NAMESPACE as c_int,
                                xmlErrorLevel::XML_ERR_ERROR as c_int,
                                "xml namespace prefix mapped to wrong URI\n".to_string(),
                                None,
                                None,
                                None,
                                0,
                                pos,
                            );
                        }
                        skip_decl = true;
                    } else if decl_value == xml_ns_uri {
                        if attr_name == b"xmlns" {
                            // Default xmlns with the xml namespace URI.
                            self.raise_error_at(
                                XML_FROM_NAMESPACE,
                                XML_NS_ERR_XML_NAMESPACE as c_int,
                                xmlErrorLevel::XML_ERR_ERROR as c_int,
                                "xml namespace URI cannot be the default namespace\n".to_string(),
                                None,
                                None,
                                None,
                                0,
                                pos,
                            );
                        } else {
                            self.raise_error_at(
                                XML_FROM_NAMESPACE,
                                XML_NS_ERR_XML_NAMESPACE as c_int,
                                xmlErrorLevel::XML_ERR_ERROR as c_int,
                                "xml namespace URI mapped to wrong prefix\n".to_string(),
                                None,
                                None,
                                None,
                                0,
                                pos,
                            );
                        }
                        skip_decl = true;
                    } else if decl_prefix == b"xmlns" {
                        self.raise_error_at(
                            XML_FROM_NAMESPACE,
                            XML_NS_ERR_XML_NAMESPACE as c_int,
                            xmlErrorLevel::XML_ERR_ERROR as c_int,
                            "redefinition of the xmlns prefix is forbidden\n".to_string(),
                            None,
                            None,
                            None,
                            0,
                            pos,
                        );
                        skip_decl = true;
                    } else if decl_value == xmlns_uri {
                        self.raise_error_at(
                            XML_FROM_NAMESPACE,
                            XML_NS_ERR_XML_NAMESPACE as c_int,
                            xmlErrorLevel::XML_ERR_ERROR as c_int,
                            "reuse of the xmlns namespace name is forbidden\n".to_string(),
                            None,
                            None,
                            None,
                            0,
                            pos,
                        );
                        skip_decl = true;
                    } else if decl_value.is_empty() && attr_name != b"xmlns" {
                        // UPSTREAM-PARITY: only PREFIXED declarations with an
                        // empty URI are errors (xmlns:p=""); the default
                        // xmlns="" legitimately undeclares the default
                        // namespace (R-000166).
                        self.raise_error_at(
                            XML_FROM_NAMESPACE,
                            XML_NS_ERR_XML_NAMESPACE as c_int,
                            xmlErrorLevel::XML_ERR_ERROR as c_int,
                            format!(
                                "xmlns:{}: Empty XML namespace is not allowed\n",
                                String::from_utf8_lossy(&decl_prefix)
                            ),
                            None,
                            None,
                            None,
                            0,
                            pos,
                        );
                        skip_decl = true;
                    }
                    if skip_decl {
                        continue;
                    }
                }
                if attr_name == b"xmlns" {
                    // Default namespace declaration: xmlns="uri"
                    ns_decls.push((Vec::new(), attr_value.clone()));
                } else if let Some(prefix) = attr_name.strip_prefix(b"xmlns:") {
                    // Prefixed namespace declaration: xmlns:prefix="uri"
                    ns_decls.push((prefix.to_vec(), attr_value.clone()));
                } else {
                    // Regular attribute
                    // Split qualified name into prefix and localname
                    if let Some(colons) = attr_name.iter().position(|&b| b == b':') {
                        let prefix = attr_name[..colons].to_vec();
                        if prefix == b"xml" {
                            // UPSTREAM-PARITY: using the xml: prefix materializes
                            // the xml namespace on the document (visible in
                            // --debug dumps as a document-level namespace).
                            self.ensure_doc_xml_ns();
                        }
                        let localname = attr_name[colons + 1..].to_vec();
                        regular_attrs.push((
                            Some(prefix),
                            localname,
                            attr_value.clone(),
                            *_had_ref,
                        ));
                        attr_pos.push(attr_end.get(idx).copied().unwrap_or(0));
                    } else {
                        regular_attrs.push((
                            None,
                            attr_name.clone(),
                            attr_value.clone(),
                            *_had_ref,
                        ));
                        attr_pos.push(attr_end.get(idx).copied().unwrap_or(0));
                    }
                }
            }

            // UPSTREAM-PARITY (parser.c xmlParseStartTag2): namespace prefixes
            // used on attributes and on the element itself must be declared by
            // this tag's declarations or by an ancestor's; otherwise
            // XML_NS_ERR_UNDEFINED_NAMESPACE is raised. The xml prefix is
            // always bound. Rejected declarations (empty xmlns:p="", wrong
            // xml/xmlns bindings) are skipped above, so they correctly count as
            // undeclared. R-000166.
            let parent_node = unsafe { (*self.ctxt).node };
            let my_doc = unsafe { (*self.ctxt).myDoc };
            // Ancestor prefixes from the parser-scoped namespace stack (pure-SAX
            // parses have no tree; upstream keeps this on ctxt->nsTab).
            // Snapshot BEFORE the current element's own declarations are pushed
            // (they are checked separately via `ns_decls`).
            let ancestor_prefixes: Vec<Vec<u8>> = self
                .ns_scope
                .iter()
                .rev()
                .map(|(dp, _)| dp.clone())
                .collect();
            let prefix_declared = |prefix: &[u8]| -> bool {
                if prefix == b"xml" {
                    return true;
                }
                if ns_decls.iter().any(|(dp, _)| dp == prefix) {
                    return true;
                }
                if parent_node.is_null() {
                    // Pure-SAX parse (no tree): the ancestor declarations live on
                    // the parser-scoped namespace stack (upstream ctxt->nsTab).
                    return ancestor_prefixes.iter().any(|dp| dp == prefix);
                }
                if !parent_node.is_null() {
                    // SAFETY: the document and parent are valid; the prefix is
                    // NUL-terminated for the search.
                    let mut prefix_c = prefix.to_vec();
                    prefix_c.push(0);
                    let ns = unsafe {
                        crate::xml::tree::search_ns(
                            my_doc,
                            parent_node,
                            prefix_c.as_ptr() as *const xmlChar,
                        )
                    };
                    return !ns.is_null();
                }
                false
            };
            let element_local = match name.iter().position(|&b| b == b':') {
                Some(colons) => &name[colons + 1..],
                None => name.as_slice(),
            };
            for (i, (prefix, localname, _, _)) in regular_attrs.iter().enumerate() {
                if let Some(p) = prefix {
                    if !prefix_declared(p) {
                        let pos = attr_pos.get(i).copied().unwrap_or(end_pos);
                        self.raise_error_at(
                            XML_FROM_NAMESPACE,
                            XML_NS_ERR_UNDEFINED_NAMESPACE as c_int,
                            xmlErrorLevel::XML_ERR_ERROR as c_int,
                            format!(
                                "Namespace prefix {} for {} on {} is not defined\n",
                                String::from_utf8_lossy(p),
                                String::from_utf8_lossy(localname),
                                String::from_utf8_lossy(element_local)
                            ),
                            Some(p.clone()),
                            None,
                            None,
                            0,
                            pos,
                        );
                    }
                }
            }
            if let Some(colons) = name.iter().position(|&b| b == b':') {
                let prefix = &name[..colons];
                if !prefix_declared(prefix) {
                    self.raise_error_at(
                        XML_FROM_NAMESPACE,
                        XML_NS_ERR_UNDEFINED_NAMESPACE as c_int,
                        xmlErrorLevel::XML_ERR_ERROR as c_int,
                        format!(
                            "Namespace prefix {} on {} is not defined\n",
                            String::from_utf8_lossy(prefix),
                            String::from_utf8_lossy(element_local)
                        ),
                        Some(prefix.to_vec()),
                        None,
                        None,
                        0,
                        end_pos,
                    );
                }
            }

            // UPSTREAM-PARITY: elements using the xml: prefix materialize the
            // xml namespace on the document as well.
            if name.starts_with(b"xml:") {
                self.ensure_doc_xml_ns();
            }
        } // end of the SAX2-only namespace classification + validation region

        // Fire startElement SAX event. The default SAX2 handler manages
        // nodeTab/nodeNr internally. For SAX2 parses the element's own
        // namespace declarations are registered on the parser-scoped
        // namespace stack afterwards, so that pure-SAX consumers (no tree,
        // `ctxt->node == NULL`) resolve ancestor-declared prefixes — PHP's
        // expat-compat `xml_parser_create_ns` (bug25666/xml009/xml010).
        let ns_scope_mark = self.ns_scope.len();
        if sax2 {
            self.sax_start_element(&name, &attributes, &regular_attrs, &ns_decls, end_pos);
            if unsafe { (*self.ctxt).node }.is_null() {
                self.ns_scope.extend(ns_decls.iter().cloned());
            }
        } else {
            // SAX1 dispatch: raw QName + every attribute (xmlns included).
            self.sax1_start_element(&name, &attributes, end_pos);
        }

        // UPSTREAM-PARITY (xmlreader.c): the XML reader is streaming, so it
        // distinguishes a self-closed `<a/>` start (NO END_ELEMENT event)
        // from an explicitly closed `<a></a>` (END_ELEMENT fires). The
        // whole-tree reader rebuilds events from the parsed tree, which
        // cannot tell the two forms apart — during a reader parse
        // (ctxt->parseMode == XML_PARSE_READER, set by reader/mod.rs) the
        // just-created element node is recorded so the reader's event
        // builder can suppress the END event for self-closed elements.
        if empty
            && unsafe { (*self.ctxt).parseMode }
                == crate::abi::types::xmlParserMode::XML_PARSE_READER as c_int
        {
            let node = unsafe { (*self.ctxt).node };
            if !node.is_null() {
                crate::xml::parser::helpers::mark_self_closed(unsafe { (*self.ctxt).myDoc }, node);
            }
        }

        // If not an empty element, parse content until matching end tag
        if !empty {
            loop {
                // UPSTREAM-PARITY: catastrophic errors (RESOURCE_LIMIT /
                // ENTITY_LOOP) set disableSAX = 2, which really stops the
                // parser.
                if unsafe { (*self.ctxt).disableSAX } == 2 {
                    break;
                }
                let next = self.tokenizer.next_token_raw();
                self.raise_pending_errors();
                match next {
                    XmlToken::EndTag {
                        name: end_name,
                        unterminated,
                        ..
                    } => {
                        // An end tag cut off by the end of the currently
                        // available input (chunk boundary): in an incremental
                        // probe this pauses — the tag may complete on a
                        // later push call — and never delivers (SP-14.3.1-3).
                        if unterminated && self.probe {
                            self.truncated_abort = true;
                            return Err(());
                        }
                        // Check for matching end tag. When the end tag names
                        // a DIFFERENT element, upstream xmlParseElementEnd /
                        // xmlParseEndTag2 (2.15) does not stop the parse: it
                        // reports XML_ERR_TAG_NAME_MISMATCH (76, FATAL) and
                        // closes the CURRENT open element as if its own end
                        // tag had appeared (the stray name only feeds the
                        // message), then keeps scanning — subsequent
                        // structural errors are reported too
                        // (DOMDocument_loadXML_error1_gte2_12 reports a
                        // second mismatch later in the document). Closing the
                        // current element is recovery-independent: the error
                        // clears wellFormed, which decides whether the doc is
                        // kept, not whether scanning continues.
                        if end_name != name {
                            // UPSTREAM-PARITY (xmlParseEndTag2):
                            // XML_ERR_TAG_NAME_MISMATCH (76), FATAL,
                            // str1 = open name, str2 = close name,
                            // int1 = open line. Upstream consumes the end
                            // tag's `>` (NEXT1) BEFORE this raise, so
                            // ctxt->input sits on the end tag's own line
                            // when the generic error handler fires — PHP
                            // prints ctxt->input->line, so the mirror must
                            // be refreshed first (a broken start tag can
                            // leave the tokenizer scanning silently across
                            // many lines with no SAX event in between,
                            // DOMDocument_loadXML_error2_gte2_12's line 7).
                            self.sync_input_position();
                            self.raise_error_now(
                                XML_FROM_PARSER,
                                XML_ERR_TAG_NAME_MISMATCH,
                                xmlErrorLevel::XML_ERR_FATAL as c_int,
                                format!(
                                    "Opening and ending tag mismatch: {} line {} and {}\n",
                                    String::from_utf8_lossy(&name),
                                    open_line,
                                    String::from_utf8_lossy(&end_name)
                                ),
                                Some(name.clone()),
                                Some(end_name.clone()),
                                None,
                                open_line as c_int,
                            );
                            // The stray end tag closes the current element:
                            // fall through to the normal end-element path
                            // (SAX end event + pop) and let the parent's
                            // content loop continue after this end tag.
                            break;
                        }
                        break;
                    }
                    XmlToken::StartTag {
                        name: child_name,
                        attributes: child_attrs,
                        attr_end: child_attr_end,
                        attr_start: child_attr_start,
                        end_pos: child_end_pos,
                        empty: child_empty,
                        unterminated,
                    } => {
                        if unterminated {
                            // Errors (incl. "Couldn't find end of Start Tag")
                            // were already raised; the child element FAILED
                            // to start (upstream xmlParseElementStart
                            // returned -1 — an unquoted/duplicate/invalid
                            // attribute construct, an over-long name, or a
                            // tag truncated by EOF). The construct may
                            // continue on a later push call: probes and
                            // eager-partial deliveries must not deliver.
                            if self.probe || self.partial_delivery {
                                self.truncated_abort = true;
                                return Err(());
                            }
                            // UPSTREAM-PARITY (2.15 xmlParseContentInternal):
                            // a failed child start tag never opens an
                            // element — the parse simply continues scanning
                            // in the CURRENT element's content, so later
                            // structural errors (e.g. the stray end tag that
                            // closes this element) are still reported
                            // (DOMDocument_load_error2_gte2_12's fourth
                            // warning). No name was pushed for the child, so
                            // nothing is popped.
                            continue;
                        }
                        self.parse_element(
                            child_name,
                            child_attrs,
                            child_attr_end,
                            child_attr_start,
                            child_end_pos,
                            child_empty,
                        )?;
                    }
                    XmlToken::Characters(data) => {
                        if !data.is_empty() {
                            // UPSTREAM-PARITY (parser.c xmlCharacters): with
                            // XML_PARSE_NOBLANKS (keepBlanks == 0) a
                            // whitespace-only run is dropped before the SAX
                            // characters event fires.
                            let keep_blanks = unsafe { (*self.ctxt).keepBlanks } != 0;
                            if keep_blanks
                                || !data
                                    .iter()
                                    .all(|&b| b == b' ' || b == b'\t' || b == b'\n' || b == b'\r')
                            {
                                self.sax_characters(&data);
                            }
                        }
                    }
                    XmlToken::Comment { data, unterminated } => {
                        if unterminated && (self.probe || self.partial_delivery) {
                            // See parse_prolog's Comment arm (SP-14.3.1-8).
                            self.truncated_abort = true;
                            self.pop_name();
                            return Err(());
                        }
                        self.sax_comment(&data);
                    }
                    XmlToken::ProcessingInstruction {
                        target,
                        data,
                        unterminated,
                        ..
                    } => {
                        // See parse_prolog's PI arm (KEY-3): an unterminated
                        // PI (no `?>`) pauses in probes; the element itself
                        // stays open (only the PI is incomplete).
                        if unterminated && (self.probe || self.partial_delivery) {
                            self.truncated_abort = true;
                            return Err(());
                        }
                        self.sax_pi(&target, &data);
                    }
                    XmlToken::Cdata {
                        data, unterminated, ..
                    } => {
                        if unterminated {
                            // "Premature end of data in CDATA section" was
                            // already recorded; the CDATA content is dropped.
                            // The section may continue on a later push call:
                            // probes and eager-partial deliveries must not
                            // deliver.
                            if self.probe || self.partial_delivery {
                                self.truncated_abort = true;
                            }
                            self.pop_name();
                            return Err(());
                        }
                        self.sax_cdata(&data);
                    }
                    XmlToken::Reference(data) => {
                        self.parse_reference(&data)?;
                    }
                    XmlToken::Eof => {
                        // UPSTREAM-PARITY (xmlParseElement /
                        // xmlParseContentInternal): EOF inside an open
                        // element raises "Premature end of data in tag %s
                        // line %d" (77) BEFORE recovery closes the element —
                        // recovery only decides whether parsing continues,
                        // not whether the diagnostic fires (ext/simplexml +
                        // ext/dom xml_parsing_LIBXML_RECOVER expect the same
                        // warning block as the non-recover case). The raise
                        // is skipped only when a prior fatal already reported
                        // the real cause (wellFormed == 0). In an incremental
                        // probe (or eager-partial delivery) the end of the
                        // currently available input inside an open element
                        // pauses the parse (more data may complete it later;
                        // SP-14.3.1-6).
                        if self.probe || self.partial_delivery {
                            self.paused = true;
                            return Err(());
                        }
                        if unsafe { (*self.ctxt).wellFormed } != 0 {
                            self.raise_error_now(
                                XML_FROM_PARSER,
                                XML_ERR_TAG_NOT_FINISHED,
                                xmlErrorLevel::XML_ERR_FATAL as c_int,
                                format!(
                                    "Premature end of data in tag {} line {}\n",
                                    String::from_utf8_lossy(&name),
                                    open_line
                                ),
                                Some(name.clone()),
                                None,
                                None,
                                open_line as c_int,
                            );
                        }
                        if self.is_recovery() {
                            break;
                        }
                        self.pop_name();
                        return Err(());
                    }
                    XmlToken::DocType {
                        unterminated: dt_unterminated,
                        ..
                    } => {
                        // KEY-2 (content-`<!`-markup rule): a `<!DOCTYPE` in
                        // element content is an invalid element start —
                        // upstream xmlParseStartTag fails the name at the
                        // '!' with XML_ERR_NAME_REQUIRED (68) and the doc
                        // becomes not well-formed (a DOCTYPE is only legal in
                        // the prolog). A body still truncated by the end of
                        // the available input pauses in probes (it may
                        // complete on a later push call — and would still be
                        // illegal here).
                        if dt_unterminated && (self.probe || self.partial_delivery) {
                            self.truncated_abort = true;
                            self.pop_name();
                            return Err(());
                        }
                        self.raise_error_now(
                            XML_FROM_PARSER,
                            XML_ERR_NAME_REQUIRED,
                            xmlErrorLevel::XML_ERR_FATAL as c_int,
                            "StartTag: invalid element name\n".to_string(),
                            None,
                            None,
                            None,
                            0,
                        );
                        self.pop_name();
                        return Err(());
                    }
                    _ => {
                        // Ignore unexpected tokens in recovery mode
                        if !self.is_recovery() {
                            self.set_error(
                                XML_ERR_INTERNAL_ERROR,
                                "Unexpected token in element content",
                            );
                            self.pop_name();
                            return Err(());
                        }
                    }
                }
            }
        }

        // Fire endElement SAX event
        // The default SAX handler pops nodeTab/nodeNr internally.
        self.sax_end_element(&name);

        // Pop the element's own namespace declarations from the
        // parser-scoped stack (registered after the start event above).
        self.ns_scope.truncate(ns_scope_mark);

        // Pop element name from context stack
        self.pop_name();
        Ok(())
    }

    /// Substitute entity and character references in an attribute value.
    ///
    /// # UPSTREAM-PARITY
    ///
    /// Mirrors libxml2's `xmlParseAttValueInternal`: character references are
    /// always substituted, the five predefined entities are always
    /// substituted, and other entities are substituted when `XML_PARSE_NOENT`
    /// is set. Unresolvable references are left as-is (they would be reported
    /// through the SAX reference callback by the full implementation).
    ///
    /// Returns `Err(())` when a referenced entity's content is not allowed in
    /// an attribute value (a `<` in entity content, upstream
    /// `XML_ERR_LT_IN_ATTRIBUTE`).
    ///
    /// `value_start_pos` is the document byte offset just after the
    /// attribute's opening quote — the caret for the `<`-in-entity error
    /// points at the `&` of the offending reference (R-000121).
    ///
    /// # Safety
    ///
    /// - `self.ctxt` must be a valid, initialized `_xmlParserCtxt`; `myDoc`
    ///   must be NULL or a valid `_xmlDoc` whose entity table is initialized
    ///   (`entities::get_entity`); `value` is a caller-owned byte slice;
    ///   entity `content` pointers (from `get_entity` and
    ///   `get_entity_content`) are read as NUL-terminated strings while the
    ///   entity is live.
    fn substitute_refs(&mut self, value: &[u8], value_start_pos: usize) -> Result<Vec<u8>, ()> {
        let mut out = Vec::with_capacity(value.len());
        let mut i = 0usize;
        while i < value.len() {
            if value[i] == b'&' {
                if let Some(semi) = value[i..].iter().position(|&b| b == b';') {
                    let inner = &value[i + 1..i + semi];
                    let mut replaced: Option<Vec<u8>> = None;
                    if inner.starts_with(b"#") {
                        // Character reference: &#N; or &#xH;
                        let num = &inner[1..];
                        let codepoint = if num.starts_with(b"x") || num.starts_with(b"X") {
                            u32::from_str_radix(&String::from_utf8_lossy(&num[1..]), 16).ok()
                        } else {
                            String::from_utf8_lossy(num).parse::<u32>().ok()
                        };
                        if let Some(cp) = codepoint {
                            if is_valid_xml_char(cp) {
                                if let Some(ch) = char::from_u32(cp) {
                                    let mut buf = [0u8; 4];
                                    replaced = Some(ch.encode_utf8(&mut buf).as_bytes().to_vec());
                                }
                            }
                        }
                    } else {
                        // Named entity: predefined ones always, others when NOENT.
                        match inner {
                            b"amp" => replaced = Some(b"&".to_vec()),
                            b"lt" => replaced = Some(b"<".to_vec()),
                            b"gt" => replaced = Some(b">".to_vec()),
                            b"quot" => replaced = Some(b"\"".to_vec()),
                            b"apos" => replaced = Some(b"'".to_vec()),
                            _ => {
                                // UPSTREAM-PARITY: xmlParseAttValueInternal
                                // resolves the entity to check its content for
                                // `<` regardless of XML_PARSE_NOENT.
                                let doc = unsafe { (*self.ctxt).myDoc };
                                if !doc.is_null() {
                                    let name_cstr = Self::vec_to_cstr_null(inner);
                                    let ent =
                                        unsafe { crate::xml::entities::get_entity(doc, name_cstr) };
                                    unsafe { libc::free(name_cstr as *mut libc::c_void) };
                                    if !ent.is_null() {
                                        let content = unsafe { (*ent).content };
                                        // UPSTREAM-PARITY (xmlCheckEntityInAttValue):
                                        // any '<' anywhere in the entity content
                                        // is illegal in an attribute value.
                                        let content_has_lt = if content.is_null() {
                                            false
                                        } else {
                                            unsafe {
                                                core::slice::from_raw_parts(
                                                    content,
                                                    libc::strlen(content as *const c_char),
                                                )
                                                .contains(&b'<')
                                            }
                                        };
                                        if content_has_lt {
                                            // UPSTREAM-PARITY (R-000121, E-005):
                                            // the error fires once for
                                            // xmlParseAttValueInternal's
                                            // xmlCheckEntityInAttValue scan and
                                            // again from the entity-expansion
                                            // re-scan (xmlExpandEntityInAttValue
                                            // with the reference entity), so the
                                            // the 2.13.0+ oracle reports it twice
                                            // with the caret right past the ';'
                                            // of the offending reference (the
                                            // input position when the error
                                            // fires). --noent takes the
                                            // xmlExpandEntityInAttValue path
                                            // only, reporting it once.
                                            let ref_pos = value_start_pos + i + semi + 1;
                                            let msg = format!(
                                                "'<' in entity '{}' is not allowed in attributes \
                                                 values",
                                                String::from_utf8_lossy(inner)
                                            );
                                            self.raise_error_at(
                                                XML_FROM_PARSER,
                                                crate::abi::types::XML_ERR_LT_IN_ATTRIBUTE,
                                                xmlErrorLevel::XML_ERR_FATAL as c_int,
                                                msg.clone(),
                                                None,
                                                None,
                                                None,
                                                0,
                                                ref_pos,
                                            );
                                            if (self.options & XML_PARSE_NOENT) == 0 {
                                                self.raise_error_at(
                                                    XML_FROM_PARSER,
                                                    crate::abi::types::XML_ERR_LT_IN_ATTRIBUTE,
                                                    xmlErrorLevel::XML_ERR_FATAL as c_int,
                                                    msg,
                                                    None,
                                                    None,
                                                    None,
                                                    0,
                                                    ref_pos,
                                                );
                                            }
                                            return Err(());
                                        }
                                        if (self.options & XML_PARSE_NOENT) != 0 {
                                            let content = unsafe {
                                                crate::xml::entities::get_entity_content(ent)
                                            };
                                            if !content.is_null() {
                                                let len = unsafe {
                                                    crate::xml::tree::xml_strlen(content)
                                                };
                                                replaced = Some(unsafe {
                                                    core::slice::from_raw_parts(
                                                        content,
                                                        len as usize,
                                                    )
                                                    .to_vec()
                                                });
                                                unsafe {
                                                    crate::abi::allocator::xmlFreeImpl(
                                                        content as *mut core::ffi::c_void,
                                                    )
                                                };
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if let Some(r) = replaced {
                        out.extend_from_slice(&r);
                        i += semi + 1;
                        continue;
                    }
                }
            }
            out.push(value[i]);
            i += 1;
        }
        Ok(out)
    }

    /// Parse a reference (entity or character).
    ///
    /// The tokenizer has already recorded structural errors (no name,
    /// missing ';', invalid charref digits/values) with upstream positions;
    /// this function performs the substitution semantics and raises the
    /// undeclared-entity error (upstream `xmlParseReference`).
    ///
    /// # Safety
    ///
    /// - `self.ctxt` must be a valid, initialized `_xmlParserCtxt` with
    ///   valid `sax`/`userData`; `entity` (from the SAX getEntity handler)
    ///   must be NULL or a valid `_xmlEntity` whose `content`, `SystemID`,
    ///   `ExternalID` and `expandedSize` fields are consistent; a loaded
    ///   external-entity input is a valid `_xmlParserInput` freed via
    ///   `xmlFreeInputStream`.
    fn parse_reference(&mut self, data: &[u8]) -> Result<(), ()> {
        if data.len() < 2 {
            // Bare "&" with no name — the tokenizer raised
            // "xmlParseEntityRef: no name".
            return Err(());
        }

        let inner = &data[1..]; // includes ';' when the reference is well-formed

        // Character reference: &#N; / &#xH;
        if inner.starts_with(b"#") {
            let body = &inner[1..];
            let (num, radix) =
                if let Some(h) = body.strip_prefix(b"x").or_else(|| body.strip_prefix(b"X")) {
                    (h, 16)
                } else {
                    (body, 10)
                };
            // Strip a trailing ';' (present only for well-formed refs).
            let digits = if num.ends_with(b";") {
                &num[..num.len() - 1]
            } else {
                num
            };
            let parsed = u32::from_str_radix(&String::from_utf8_lossy(digits), radix).ok();
            match parsed {
                Some(cp) if is_valid_xml_char(cp) => {
                    // UPSTREAM-PARITY: a valid charref dispatches as char
                    // data; the tokenizer already validated the value.
                    if let Some(ch) = char::from_u32(cp) {
                        let mut utf8_buf = [0u8; 4];
                        let encoded = ch.encode_utf8(&mut utf8_buf);
                        self.sax_characters(encoded.as_bytes());
                    }
                    Ok(())
                }
                _ => {
                    // Structural/value errors were already recorded by the
                    // tokenizer; upstream returns without substituting.
                    Err(())
                }
            }
        } else {
            // Entity reference: &name;
            if !inner.ends_with(b";") {
                // "EntityRef: expecting ';'" was already recorded.
                return Err(());
            }
            let name = &inner[..inner.len() - 1];

            // Predefined XML entities are substituted unconditionally.
            let replacement = match name {
                b"amp" => Some(b"&" as &[u8]),
                b"lt" => Some(b"<" as &[u8]),
                b"gt" => Some(b">" as &[u8]),
                b"quot" => Some(b"\"" as &[u8]),
                b"apos" => Some(b"'" as &[u8]),
                _ => None,
            };
            if let Some(replacement) = replacement {
                self.sax_characters(replacement);
                return Ok(());
            }

            // Resolve the declared entity through the SAX getEntity handler.
            let entity = if self.probe {
                // Incremental probe: resolve entities side-effect-free. PHP's
                // expat-compat getEntity has user-visible side effects (it
                // feeds the raw "&name;" text to the default handler), which
                // must not fire during a probe — the completing parse
                // dispatches it.
                let name_cstr = Self::vec_to_cstr_null(name);
                unsafe {
                    let pre = crate::abi::exports_misc::xmlGetPredefinedEntity(name_cstr);
                    if pre.is_null() {
                        crate::xml::tree::get_doc_entity((*self.ctxt).myDoc, name_cstr)
                    } else {
                        pre
                    }
                }
            } else if !self.is_sax_disabled() {
                let name_cstr = Self::vec_to_cstr_null(name);
                unsafe {
                    let sax = &*(*self.ctxt).sax;
                    let ctx = (*self.ctxt).userData;
                    SaxDispatcher::get_entity(sax, ctx, name_cstr)
                }
            } else {
                ptr::null_mut()
            };

            if entity.is_null() {
                // UPSTREAM-PARITY (parser.c xmlHandleUndeclaredEntity +
                // the ent==NULL continuation of xmlParseReference): the
                // severity of an undeclared entity reference depends on the
                // document's DTD state.
                //
                // [WFC: Entity Declared] makes the reference FATAL only for a
                // standalone document or one with neither an external subset
                // nor parameter-entity references: a non-validating processor
                // is not obligated to load declarations from external subsets
                // or external parameter entities, so it must not abort on
                // references that those unloaded declarations might define.
                // With XML_PARSE_DTDVALID the [VC: Entity Declared] validity
                // error is raised instead; otherwise the reference is a
                // non-fatal XML_WAR_UNDECLARED_ENTITY error/warning and
                // parsing continues — PHP's expat-compat parser observes the
                // document's remaining content after an undeclared reference
                // (xml004/xml_closures_001).
                let fatal = unsafe {
                    let c = &*self.ctxt;
                    c.standalone == 1 || (c.hasExternalSubset == 0 && c.hasPErefs == 0)
                };
                let msg = format!("Entity '{}' not defined\n", String::from_utf8_lossy(name));
                if fatal {
                    self.raise_error_now(
                        XML_FROM_PARSER,
                        XML_ERR_UNDECLARED_ENTITY,
                        xmlErrorLevel::XML_ERR_FATAL as c_int,
                        msg,
                        Some(name.to_vec()),
                        None,
                        None,
                        0,
                    );
                    if !self.is_recovery() {
                        return Err(());
                    }
                } else {
                    let (code, level, domain) = unsafe {
                        let c = &*self.ctxt;
                        if c.validate != 0 {
                            // [VC: Entity Declared] — validity error, level
                            // XML_ERR_ERROR, domain XML_FROM_DTD (upstream
                            // xmlValidityError). Non-fatal; parsing continues.
                            (
                                XML_ERR_UNDECLARED_ENTITY,
                                xmlErrorLevel::XML_ERR_ERROR as c_int,
                                XML_FROM_DTD,
                            )
                        } else if (c.loadsubset & !crate::abi::constants::XML_SKIP_IDS != 0)
                            || (c.replaceEntities != 0 && c.options & XML_PARSE_NO_XXE == 0)
                        {
                            // xmlErrMsgStr: non-fatal error (XML_ERR_ERROR)
                            // when the external subset is loaded or entity
                            // substitution was requested without NO_XXE.
                            (
                                XML_WAR_UNDECLARED_ENTITY,
                                xmlErrorLevel::XML_ERR_ERROR as c_int,
                                XML_FROM_PARSER,
                            )
                        } else {
                            // xmlWarningMsg: plain warning.
                            (
                                XML_WAR_UNDECLARED_ENTITY,
                                xmlErrorLevel::XML_ERR_WARNING as c_int,
                                XML_FROM_PARSER,
                            )
                        }
                    };
                    self.raise_error_now(
                        domain,
                        code,
                        level,
                        msg,
                        Some(name.to_vec()),
                        None,
                        None,
                        0,
                    );
                }
                unsafe {
                    (*self.ctxt).valid = 0;
                }
                // UPSTREAM-PARITY (xmlParseReference): an undeclared
                // reference dispatches the SAX reference event only when
                // entities are not substituted (replaceEntities == 0). In
                // recovery mode this builds an entity-ref node without a
                // backing declaration; the expat-compat default handler has
                // already seen the raw "&name;" text through getEntity.
                if !self.is_sax_disabled()
                    && !self.probe
                    && unsafe { (*self.ctxt).replaceEntities == 0 }
                {
                    let name_cstr = Self::vec_to_cstr_null(name);
                    unsafe {
                        let sax = &*(*self.ctxt).sax;
                        let ctx = (*self.ctxt).userData;
                        SaxDispatcher::reference(sax, ctx, name_cstr);
                    }
                }
                return Ok(());
            }

            // UPSTREAM-PARITY (parser.c xmlParseReference): an entity-resolving
            // SAX handler may stop the parser while the reference is looked up
            // (PHP expat-compat get_entity fires the external-entity-ref
            // callback for an external general parsed entity; a FALSE return
            // runs xmlStopParser → disableSAX = 2, wellFormed = 0, errNo =
            // XML_ERROR_EXTERNAL_ENTITY_HANDLING). The reference then expands
            // to nothing and no further event fires (SP-14.3.1-4, bug71592).
            //
            // UPSTREAM-PARITY (expat-compat mode): a context that entered the
            // parse already not-well-formed (PHP zeroes ctxt->wellFormed at
            // create) never re-parses resolved entity content upstream — the
            // expat-compat get_entity side effects are the only delivery, so
            // substituting here too would duplicate the content
            // (bug30875/gh14834).
            if self.started_unwellformed || unsafe { (*self.ctxt).wellFormed } == 0 {
                return Ok(());
            }

            if (self.options & XML_PARSE_NOENT) != 0 {
                // UPSTREAM-PARITY (parser.c xmlParseReference): the first-
                // parse phase runs only for internal general entities or,
                // for EXTERNAL general parsed entities, when the load is
                // allowed — `(ent->etype == XML_INTERNAL_GENERAL_ENTITY) ||
                // (!(ctxt->options & XML_PARSE_NO_XXE) &&
                //  (ctxt->replaceEntities || ctxt->validate))`. Under
                // LIBXML_NO_XXE an external entity is neither loaded nor
                // substituted and its reference expands to NOTHING — no
                // entity-loader call, no "failed to load" I/O warning
                // (ext/simplexml + ext/dom xml_parsing_LIBXML_NO_XXE).
                let is_external =
                    unsafe { (*entity).etype } == XML_EXTERNAL_GENERAL_PARSED_ENTITY as c_int;
                let may_load_external = (self.options & XML_PARSE_NO_XXE) == 0;
                if !is_external || may_load_external {
                    self.parse_entity_content(entity, None)?;
                    // UPSTREAM-PARITY (xmlParseReference): "We also check for
                    // amplification if entities aren't substituted. They might
                    // be expanded later." — unconditional, no XML_PARSE_HUGE
                    // bypass.
                    // consumed = the document position after the reference.
                    let (_, _, dpos) = self.tokenizer.current_pos();
                    if self.parser_entity_check(
                        unsafe { (*entity).expandedSize },
                        ptr::null_mut(),
                        dpos as c_ulong,
                    ) {
                        return Err(());
                    }
                    if !unsafe { (*entity).content }.is_null() {
                        // Re-parse the entity content from a pushed input.
                        let content = unsafe { (*entity).content };
                        let len = unsafe { libc::strlen(content as *const c_char) };
                        let bytes = unsafe { core::slice::from_raw_parts(content, len) };
                        let buf = InputBuffer::from_memory(bytes, None);
                        self.tokenizer.push_input(buf);
                    } else {
                        // External entity with no in-memory content: load it
                        // through the registered loader and re-parse.
                        let loaded = unsafe {
                            let sys = (*entity).SystemID as *const c_char;
                            let ext = (*entity).ExternalID as *const c_char;
                            crate::abi::exports_parser::xmlLoadExternalEntity(sys, ext, self.ctxt)
                        };
                        if loaded.is_null() {
                            // UPSTREAM-PARITY (xmlCtxtParseEntity): an external
                            // entity that cannot be loaded (e.g. XML_PARSE_NONET
                            // refusing an http URL) fails silently — the reference
                            // simply expands to nothing.
                            return Ok(());
                        }
                        unsafe {
                            let base = (*loaded).base;
                            let end = (*loaded).end;
                            let len = end.offset_from(base).max(0) as usize;
                            let bytes = if base.is_null() || len == 0 {
                                &[][..]
                            } else {
                                core::slice::from_raw_parts(base, len)
                            };
                            let buf = InputBuffer::from_memory(bytes, None);
                            self.tokenizer.push_input(buf);
                            // free_parser_input (xmlFreeInputStream) frees the
                            // owned buffer; no separate buffer free here.
                            crate::abi::exports_xml2::xmlFreeInputStream(loaded);
                        }
                    }
                }
            } else {
                // UPSTREAM-PARITY (xmlParseReference): the entity content is
                // parsed into ent->children on the first reference
                // (xmlCtxtParseEntity), before the reference event fires.
                self.parse_entity_content(entity, None)?;
                // UPSTREAM-PARITY (xmlParseReference): unconditional
                // amplification check (also when not substituting).
                let (_, _, dpos2) = self.tokenizer.current_pos();
                if self.parser_entity_check(
                    unsafe { (*entity).expandedSize },
                    ptr::null_mut(),
                    dpos2 as c_ulong,
                ) {
                    return Err(());
                }
                // Not substituting: dispatch the reference event.
                if !self.is_sax_disabled() && !self.probe {
                    let name_cstr = Self::vec_to_cstr_null(name);
                    unsafe {
                        let sax = &*(*self.ctxt).sax;
                        let ctx = (*self.ctxt).userData;
                        SaxDispatcher::reference(sax, ctx, name_cstr);
                    }
                }
            }

            Ok(())
        }
    }

    /// UPSTREAM-PARITY (parser.c xmlCtxtParseEntity, first-parse branch):
    /// parse the entity's content into a node list stored in `ent->children`
    /// (`ent->last` updated, each node's parent set to `ent`, document
    /// pointer propagated). Text runs and character references become text
    /// nodes (coalesced); nested general-entity references are resolved
    /// recursively and become entity-ref nodes carrying the referenced entity
    /// (content shared, children = entity), matching xmlNewReference. The
    /// parse is guarded against loops with the XML_ENT_EXPANDING flag and
    /// cached with XML_ENT_PARSED.
    ///
    /// # Safety
    ///
    /// - `self.ctxt` must be a valid, initialized `_xmlParserCtxt`; `ent`
    ///   must be non-NULL and a valid `_xmlEntity` whose `flags`, `content`
    ///   and `children` fields are consistent; the function mutates `ent`
    ///   and builds its children node list.
    fn parse_entity_content(
        &mut self,
        ent: *mut _xmlEntity,
        ref_win: Option<(Vec<u8>, usize)>,
    ) -> Result<(), ()> {
        const XML_ENT_PARSED: c_int = 1 << 0;
        const XML_ENT_EXPANDING: c_int = 1 << 3;
        unsafe {
            if ent.is_null() || ((*ent).flags & XML_ENT_PARSED) != 0 {
                return Ok(());
            }
            if ((*ent).flags & XML_ENT_EXPANDING) != 0 {
                // UPSTREAM-PARITY (parser.c xmlCtxtParseEntity): a reference
                // to an entity that is already being expanded is a loop. The
                // message text is xmlErrString(XML_ERR_ENTITY_LOOP); the
                // legacy "cur input" tail carries the referencing entity
                // content's info + window (HOSTILE-FAILURE F2).
                self.cur_error_tail = Some((1, ref_win));
                // UPSTREAM-PARITY (xmlCtxtVErr frozen-parent position): the
                // oracle attributes the loop error to the END of the
                // top-level reference — the parent input's `col` sits on the
                // trailing ';' (the raw NEXT consume does not bump col). The
                // tokenizer has already consumed the reference, so its
                // position is one past it.
                let (_, _, dpos) = self.tokenizer.current_pos();
                let ref_end = dpos.saturating_sub(1);
                self.raise_error_at(
                    XML_FROM_PARSER,
                    XML_ERR_ENTITY_LOOP,
                    xmlErrorLevel::XML_ERR_FATAL as c_int,
                    "Detected an entity reference loop\n".to_string(),
                    None,
                    None,
                    None,
                    0,
                    ref_end,
                );
                return Err(());
            }
            if (*ent).content.is_null() {
                (*ent).flags |= XML_ENT_PARSED;
                return Ok(());
            }
            let doc = (*ent).doc;
            let content = (*ent).content;
            let len = libc::strlen(content as *const c_char);
            let bytes = core::slice::from_raw_parts(content, len);
            // Recursive-sum accumulator (upstream xmlCheckEntityInAttValue):
            // starts at the raw content length and adds each nested general
            // entity's expanded size plus the fixed cost.
            let mut expansion_sum: c_ulong = len as c_ulong;

            (*ent).flags |= XML_ENT_EXPANDING;

            let mut head: *mut _xmlNode = ptr::null_mut();
            let mut last: *mut _xmlNode = ptr::null_mut();
            let mut pending: Vec<u8> = Vec::new();

            macro_rules! flush_text {
                () => {
                    if !pending.is_empty() {
                        // UPSTREAM-PARITY (SAX2.c xmlSAX2TextNode): short text
                        // runs are stored inline in the node struct when
                        // XML_PARSE_COMPACT is set — the oracle's --debug shows
                        // `TEXT compact` for the entity's parsed content.
                        let len = pending.len();
                        let node = if (self.options & XML_PARSE_COMPACT) != 0 && len < 16 {
                            let n = crate::abi::allocator::xmlMallocZero(core::mem::size_of::<
                                _xmlNode,
                            >()) as *mut _xmlNode;
                            if !n.is_null() {
                                (*n).type_ = XML_TEXT_NODE as c_int;
                                (*n).name = crate::xml::string::xml_strdup(
                                    b"text\0".as_ptr() as *const xmlChar
                                );
                                let inline =
                                    std::ptr::addr_of_mut!((*n).properties) as *mut xmlChar;
                                ptr::copy_nonoverlapping(pending.as_ptr(), inline, len);
                                *inline.add(len) = 0;
                                (*n).content = inline;
                                crate::abi::data_globals::register_node_hook(n);
                            }
                            n
                        } else {
                            let mut nul = pending.clone();
                            nul.push(0);
                            crate::xml::tree::new_text(nul.as_ptr() as *const xmlChar)
                        };
                        if !node.is_null() {
                            (*node).doc = doc;
                            // UPSTREAM-PARITY: the entity input stream starts
                            // at line 1, so parsed content nodes carry line 1.
                            (*node).line = 1;
                            if last.is_null() {
                                head = node;
                            } else {
                                (*last).next = node;
                                (*node).prev = last;
                            }
                            last = node;
                        }
                        pending.clear();
                    }
                };
            }

            let mut i = 0usize;
            while i < bytes.len() {
                if bytes[i] == b'&' {
                    if let Some(semi_rel) = bytes[i + 1..].iter().position(|&b| b == b';') {
                        let inner = &bytes[i + 1..i + 1 + semi_rel];
                        if inner.starts_with(b"#") {
                            // Numeric character reference: decoded into text.
                            let mut digits = &inner[1..];
                            let radix = if digits
                                .strip_prefix(b"x")
                                .or_else(|| digits.strip_prefix(b"X"))
                                .is_some()
                            {
                                digits = &digits[1..];
                                16
                            } else {
                                10
                            };
                            if let Ok(cp) =
                                u32::from_str_radix(&String::from_utf8_lossy(digits), radix)
                            {
                                if let Some(ch) = char::from_u32(cp) {
                                    let mut buf4 = [0u8; 4];
                                    pending.extend_from_slice(ch.encode_utf8(&mut buf4).as_bytes());
                                }
                            }
                        } else {
                            // Named reference.
                            let replacement = match inner {
                                b"amp" => Some(b"&" as &[u8]),
                                b"lt" => Some(b"<" as &[u8]),
                                b"gt" => Some(b">" as &[u8]),
                                b"quot" => Some(b"\"" as &[u8]),
                                b"apos" => Some(b"'" as &[u8]),
                                _ => None,
                            };
                            if let Some(rep) = replacement {
                                pending.extend_from_slice(rep);
                            } else {
                                // General entity: resolve, recurse, and emit an
                                // entity-ref node carrying the entity.
                                let name_cstr = Self::vec_to_cstr_null(inner);
                                let ent2 = if self.probe {
                                    // Side-effect-free probe resolution (the
                                    // compat getEntity feeds the default
                                    // handler — must not fire in probes).
                                    let pre =
                                        crate::abi::exports_misc::xmlGetPredefinedEntity(name_cstr);
                                    if pre.is_null() {
                                        crate::xml::tree::get_doc_entity(
                                            (*self.ctxt).myDoc,
                                            name_cstr,
                                        )
                                    } else {
                                        pre
                                    }
                                } else if !self.is_sax_disabled() {
                                    let sax = &*(*self.ctxt).sax;
                                    let ctx = (*self.ctxt).userData;
                                    SaxDispatcher::get_entity(sax, ctx, name_cstr)
                                } else {
                                    ptr::null_mut()
                                };
                                if !ent2.is_null() {
                                    // Window for a possible loop raise: the
                                    // referencing entity's content at the
                                    // reference position (upstream's current
                                    // input at the raise; HOSTILE-FAILURE F2).
                                    let ref_win = if (*ent).content.is_null() {
                                        None
                                    } else {
                                        let clen = libc::strlen((*ent).content as *const c_char);
                                        let cdata =
                                            core::slice::from_raw_parts((*ent).content, clen);
                                        let at = (i + semi_rel + 2).min(clen);
                                        crate::xml::parser::tokenizer::window_at_data(cdata, at)
                                    };
                                    self.parse_entity_content(ent2, ref_win)?;
                                    // UPSTREAM-PARITY (parser.c xmlParseReference):
                                    // every general-entity reference is subject to
                                    // the amplification check; the accumulation
                                    // target is the scanning entity's slot and
                                    // consumed is the position after the reference.
                                    let after = i + semi_rel + 2;
                                    if self.parser_entity_check(
                                        (*ent2).expandedSize,
                                        &mut (*ent).expandedSize,
                                        after as c_ulong,
                                    ) {
                                        return Err(());
                                    }
                                    // Recursive-sum contribution (upstream
                                    // xmlCheckEntityInAttValue accumulation).
                                    expansion_sum = expansion_sum
                                        .saturating_add((*ent2).expandedSize)
                                        .saturating_add(20);
                                    flush_text!();
                                    let node =
                                        crate::abi::allocator::xmlMallocZero(core::mem::size_of::<
                                            _xmlNode,
                                        >(
                                        )) as *mut _xmlNode;
                                    if !node.is_null() {
                                        (*node).type_ = XML_ENTITY_REF_NODE as c_int;
                                        (*node).name = crate::xml::string::xml_strdup(name_cstr);
                                        (*node).doc = doc;
                                        (*node).content = (*ent2).content;
                                        (*node).children = ent2 as *mut _xmlNode;
                                        (*node).last = ent2 as *mut _xmlNode;
                                        if last.is_null() {
                                            head = node;
                                        } else {
                                            (*last).next = node;
                                            (*node).prev = last;
                                        }
                                        last = node;
                                    }
                                }
                                crate::abi::allocator::xmlFreeImpl(name_cstr as *mut c_void);
                            }
                        }
                        i += semi_rel + 2;
                        continue;
                    }
                }
                let start = i;
                while i < bytes.len() && bytes[i] != b'&' {
                    i += 1;
                }
                if i > start {
                    pending.extend_from_slice(&bytes[start..i]);
                }
            }
            flush_text!();

            (*ent).flags &= !XML_ENT_EXPANDING;
            (*ent).flags |= XML_ENT_PARSED;

            // UPSTREAM-PARITY (parser.c): the entity's expanded size is the
            // recursive sum of its content plus every nested general-entity
            // reference's expanded size plus the fixed cost — the value
            // xmlParserEntityCheck compares against the amplification bound.
            (*ent).expandedSize = expansion_sum;

            (*ent).children = head;
            (*ent).last = last;
            let mut cur = head;
            while !cur.is_null() {
                (*cur).parent = ent as *mut _xmlNode;
                if (*cur).doc != doc {
                    // UPSTREAM-PARITY: xmlSetTreeDoc on the parsed list.
                    crate::abi::exports_tree::xmlSetTreeDoc(cur, doc);
                }
                cur = (*cur).next;
            }
        }
        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Epilog parsing
    // ─────────────────────────────────────────────────────────────────────────

    /// Parse the epilog: misc (comments, PIs, whitespace) after the root element.
    fn parse_epilog(&mut self) -> Result<(), ()> {
        loop {
            let token = self.tokenizer.next_token();
            match token {
                XmlToken::Eof => {
                    return Ok(());
                }
                XmlToken::Comment { data, unterminated } => {
                    if unterminated && (self.probe || self.partial_delivery) {
                        // See parse_prolog's Comment arm (SP-14.3.1-8).
                        self.truncated_abort = true;
                        return Err(());
                    }
                    self.sax_comment(&data);
                }
                XmlToken::ProcessingInstruction {
                    target,
                    data,
                    unterminated,
                    ..
                } => {
                    // See parse_prolog's PI arm (KEY-3): pause in probes.
                    if unterminated && (self.probe || self.partial_delivery) {
                        self.truncated_abort = true;
                        return Err(());
                    }
                    self.sax_pi(&target, &data);
                }
                XmlToken::Characters(data) => {
                    // Only whitespace is allowed in the epilog
                    if data.iter().any(|&b| !b.is_ascii_whitespace()) {
                        self.set_error(
                            XML_ERR_DOCUMENT_END,
                            "Non-whitespace characters after root element",
                        );
                        if !self.is_recovery() {
                            return Err(());
                        }
                    }
                }
                _ => {
                    // Unexpected token in epilog
                    self.set_error(
                        XML_ERR_EXTRA_CONTENT,
                        "Unexpected content after root element",
                    );
                    if !self.is_recovery() {
                        return Err(());
                    }
                }
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // SAX dispatch helpers
    // ─────────────────────────────────────────────────────────────────────────

    /// Return whether SAX delivery is disabled entirely (stopped / silent
    /// probe / sax-suppressed diagnostics pass).
    fn sax_blocked(&self) -> bool {
        self.is_sax_disabled() || self.probe || self.sax_suppressed
    }

    /// Whether an event at the tokenizer's current byte position must be
    /// skipped because it lies at or below the already-delivered prefix of the
    /// accumulated input (eager-partial delivery resume, SP-14.3.1-6). Events
    /// fire after their token was consumed, so the current position is the
    /// token's end: a token ending at or before the boundary was delivered by
    /// the earlier partial parse; only tokens ending past it are new.
    fn below_delivery_boundary(&self) -> bool {
        self.sax_suppress_until > 0
            && self.tokenizer.input().current_pos().2 <= self.sax_suppress_until
    }

    /// Fire `startDocument` SAX event.
    ///
    /// # Safety
    ///
    /// - `self.ctxt` must be a valid, initialized `_xmlParserCtxt` whose
    ///   `sax` is a valid `_xmlSAXHandler` and whose `userData` matches the
    ///   handler.
    fn sax_start_document(&mut self) {
        // startDocument fires once per parse session: any parse with a
        // delivery boundary is a continuation of an earlier eager-partial
        // parse that already fired it (SP-14.3.1-6).
        if self.sax_blocked() || self.sax_suppress_until > 0 {
            return;
        }
        unsafe {
            let sax = &*(*self.ctxt).sax;
            let ctx = (*self.ctxt).userData;
            SaxDispatcher::start_document(sax, ctx);
        }
    }

    /// Fire `endDocument` SAX event.
    ///
    /// # Safety
    ///
    /// - `self.ctxt` must be a valid, initialized `_xmlParserCtxt` whose
    ///   `sax` is a valid `_xmlSAXHandler` and whose `userData` matches the
    ///   handler.
    fn sax_end_document(&mut self) {
        if self.sax_blocked() {
            return;
        }
        unsafe {
            let sax = &*(*self.ctxt).sax;
            let ctx = (*self.ctxt).userData;
            SaxDispatcher::end_document(sax, ctx);
        }
    }

    /// Fire `startElement` SAX event with namespace processing.
    ///
    /// `attrs` is a list of `(prefix, localname, value)` tuples.
    /// `ns_decls` is a list of `(prefix, uri)` tuples.
    ///
    /// # Safety
    ///
    /// - `self.ctxt` must be a valid, initialized `_xmlParserCtxt` with
    ///   valid `sax`/`userData`; `myDoc` must be NULL or a valid `_xmlDoc`;
    ///   `node` may be NULL or a valid `_xmlNode` (for `tree::search_ns`);
    ///   `attrs` and `ns_decls` are caller-owned slices live for the call;
    ///   the NUL-terminated C strings passed to the SAX callback are
    ///   intentionally leaked and stay valid for the duration of the
    ///   callback.
    fn sax_start_element(
        &mut self,
        _name: &[u8],
        _raw_attributes: &[(Vec<u8>, Vec<u8>, bool)],
        attrs: &[(Option<Vec<u8>>, Vec<u8>, Vec<u8>, bool)],
        ns_decls: &[(Vec<u8>, Vec<u8>)],
        end_pos: usize,
    ) {
        if self.sax_blocked() || self.below_delivery_boundary() {
            return;
        }
        self.sync_input_position();
        // UPSTREAM-PARITY: `cur` sits at the tag's closing `>` when the SAX
        // start-element callback fires (depth-error window capture).
        self.sync_cur_at(end_pos);

        // UPSTREAM-PARITY (SAX2.c xmlSAX2StartElementNs): the first validity
        // check runs before the element is processed — a validating parse
        // with no DTD at all (no external subset and no populated internal
        // subset) raises XML_DTD_NO_DTD (522) "Validation failed: no DTD
        // found !" through the DTD domain ("validity error"), clears
        // ctxt->valid, and disables further validation (ctxt->validate = 0).
        // The error is attributed to `end_pos` — the tag's closing `>` /
        // `/>` — exactly where upstream's cur sits when the SAX callback
        // fires (xmlParseStartTag2 calls startElementNs before consuming
        // the tag end). This is what XML_PARSE_DTDVALID consumers observe
        // (parse2.c / parse4.c / reader2.c check ctxt->valid; Phase-12
        // EXTERNAL-CONSUMERS court).
        unsafe {
            let ctxt = self.ctxt;
            if (*ctxt).validate != 0 {
                let my_doc = (*ctxt).myDoc;
                let no_dtd = my_doc.is_null()
                    || ((*my_doc).extSubset.is_null()
                        && ((*my_doc).intSubset.is_null()
                            || ((*(*my_doc).intSubset).notations.is_null()
                                && (*(*my_doc).intSubset).elements.is_null()
                                && (*(*my_doc).intSubset).attributes.is_null()
                                && (*(*my_doc).intSubset).entities.is_null())));
                if no_dtd {
                    self.raise_error_at(
                        XML_FROM_DTD,
                        XML_DTD_NO_DTD,
                        xmlErrorLevel::XML_ERR_ERROR as c_int,
                        "Validation failed: no DTD found !".to_string(),
                        None,
                        None,
                        None,
                        0,
                        end_pos,
                    );
                    (*ctxt).valid = 0;
                    (*ctxt).validate = 0;
                }
            }
        }

        // Split the element QName into prefix and local name. The tokenizer
        // yields the raw qualified name (e.g. "xsl:stylesheet"); SAX2
        // requires the local name plus a separate prefix.
        let (prefix_opt, localname) = if let Some(colons) = _name.iter().position(|&b| b == b':') {
            (Some(_name[..colons].to_vec()), _name[colons + 1..].to_vec())
        } else {
            (None, _name.to_vec())
        };
        // Resolve the prefix against this element's namespace declarations
        // and, when not declared here, the ancestor scope (upstream
        // xmlParserNsLookupUri walks the parser's in-scope namespace stack).
        let my_doc = unsafe { (*self.ctxt).myDoc };
        let parent_node = unsafe { (*self.ctxt).node };
        let resolve_uri = |prefix: Option<&[u8]>| -> Option<Vec<u8>> {
            // Own declarations win (the nearest in-scope binding).
            if let Some(p) = prefix {
                if p == b"xml" {
                    return Some(b"http://www.w3.org/XML/1998/namespace".to_vec());
                }
                if let Some((_, u)) = ns_decls.iter().find(|(dp, _)| dp == p) {
                    return Some(u.clone());
                }
            } else if let Some((_, u)) = ns_decls.iter().find(|(dp, _)| dp.is_empty()) {
                return Some(u.clone());
            }
            // Ancestor scope. For a pure-SAX parse there is no tree
            // (ctxt->node == NULL), so walk the parser-scoped namespace
            // stack — upstream ctxt->nsTab, pushed by xmlParseStartTag2 when
            // the xmlns declarations are processed and popped at element end
            // (SP-14.3.1-3, bug25666/xml009/xml010).
            if parent_node.is_null() {
                for (sp, su) in self.ns_scope.iter().rev() {
                    match prefix {
                        Some(p) => {
                            if sp == p {
                                if su.is_empty() {
                                    // xmlns:p="" is a rejected declaration
                                    // and never reaches the scope; an empty
                                    // default href means the element is in
                                    // NO namespace (URI NULL).
                                    return None;
                                }
                                return Some(su.clone());
                            }
                        }
                        None => {
                            if sp.is_empty() {
                                if su.is_empty() {
                                    return None;
                                }
                                return Some(su.clone());
                            }
                        }
                    }
                }
            }
            unsafe {
                if !parent_node.is_null() {
                    // UPSTREAM-PARITY (parser.c xmlParserNsLookupUri): a
                    // NULL prefix resolves the DEFAULT namespace
                    // (xmlSearchNs(doc, node, NULL) matches the
                    // prefix-less declaration); searching with an empty
                    // string would never match because the default
                    // declaration's prefix is NULL, not "".
                    let ns = if let Some(p) = prefix {
                        let mut pb = p.to_vec();
                        pb.push(0);
                        crate::xml::tree::search_ns(
                            my_doc,
                            parent_node,
                            pb.as_ptr() as *const xmlChar,
                        )
                    } else {
                        crate::xml::tree::search_ns(my_doc, parent_node, ptr::null())
                    };
                    if !ns.is_null() && !(*ns).href.is_null() {
                        let mut l = 0usize;
                        while *(*ns).href.add(l) != 0 {
                            l += 1;
                        }
                        return Some(core::slice::from_raw_parts((*ns).href, l).to_vec());
                    }
                }
            }
            None
        };
        let uri: Option<Vec<u8>> = resolve_uri(prefix_opt.as_deref());

        unsafe {
            let sax = &*(*self.ctxt).sax;
            let ctx = (*self.ctxt).userData;

            // Build the namespace array for SAX2: [prefix1, uri1, prefix2, uri2, ...]
            let mut ns_vec: Vec<*const xmlChar> = Vec::with_capacity(ns_decls.len() * 2);
            for (prefix, uri) in ns_decls {
                let prefix_cstr = if prefix.is_empty() {
                    ptr::null()
                } else {
                    Self::vec_to_cstr_null(prefix)
                };
                // UPSTREAM-PARITY: xmlns="" declares an empty (not NULL)
                // namespace URI — xmlNewNs stores href="".
                let uri_cstr = if uri.is_empty() {
                    let empty = vec![0u8];
                    let p = empty.as_ptr();
                    core::mem::forget(empty);
                    p as *const xmlChar
                } else {
                    Self::vec_to_cstr_null(uri)
                };
                ns_vec.push(prefix_cstr);
                ns_vec.push(uri_cstr);
            }

            // Build the attribute array for SAX2: [localname1, prefix1, uri1, valueStart1, valueEnd1, ...]
            let mut attr_vec: Vec<*const xmlChar> = Vec::with_capacity(attrs.len() * 5);
            for (prefix, localname, value, _had_ref) in attrs {
                let local_cstr = Self::vec_to_cstr_null(localname);
                let prefix_cstr = prefix
                    .as_ref()
                    .map(|p| Self::vec_to_cstr_null(p))
                    .unwrap_or(ptr::null());
                // Resolve the attribute prefix against the namespace
                // declarations and the ancestor scope (upstream
                // xmlParserNsLookupUri).
                let uri_cstr = match prefix {
                    Some(p) => resolve_uri(Some(p))
                        .map(|u| Self::vec_to_cstr_null(&u))
                        .unwrap_or(ptr::null()),
                    _ => ptr::null(),
                };
                // UPSTREAM-PARITY: the SAX2 atts array value pointer is
                // never NULL — an EMPTY value (`bar=""`) points at the
                // empty string with valueEnd == valueStart (xmlParseStartTag2
                // hands SAX2 the raw in-place value). A NULL start would make
                // the tree builder drop the attribute entirely.
                let value_cstr = if value.is_empty() {
                    c"".as_ptr() as *const xmlChar
                } else {
                    Self::vec_to_cstr_null(value)
                };
                attr_vec.push(local_cstr);
                attr_vec.push(prefix_cstr);
                attr_vec.push(uri_cstr);
                attr_vec.push(value_cstr);
                // UPSTREAM-PARITY (parser.c xmlParseStartTag2, SAX2 atts
                // array): valueEnd is ALWAYS valueStart + length. The byte
                // AT valueEnd differs by case — for a raw in-place value it
                // is the closing quote; when the value was duplicated
                // (reference present / normalization) it is the NUL
                // terminator (*valueEnd == 0), which SAX2.c
                // xmlSAX2AttributeNs uses to choose the compact
                // xmlSAX2TextNode path vs the non-compact
                // xmlNodeParseAttValue path (R-000120). External SAX2
                // consumers (PHP ext/xml compat) compute the value length
                // as valueEnd - valueStart unconditionally, so NULL is
                // never valid here.
                attr_vec.push({ value_cstr.add(value.len()) } as *const xmlChar);
            }

            // UPSTREAM-PARITY (parser.c xmlParseStartTag2 "Default
            // attributes"): DTD defaults from the internal subset
            // (ctxt->attsDefault) that are not already present on the tag are
            // appended to the SAX2 attribute set and counted in nb_defaulted
            // (SP-14.3.1-4, bug35447). Keyed by the element's local name
            // first, then the raw QName as written in the ATTLIST.
            let mut nb_defaulted: c_int = 0;
            let default_entries: Vec<(Vec<u8>, Vec<u8>)> = self
                .dtd_attr_defaults
                .iter()
                .find(|(n, _)| n == &localname || n == _name)
                .map(|(_, v)| v.clone())
                .unwrap_or_default();
            for (dname, dval) in &default_entries {
                if dname == b"xmlns" || dname.starts_with(b"xmlns:") {
                    continue;
                }
                let (dprefix, dlocal) = match dname.iter().position(|&b| b == b':') {
                    Some(c) => (Some(dname[..c].to_vec()), dname[c + 1..].to_vec()),
                    None => (None, dname.clone()),
                };
                // Skip defaults already present on the tag (the attribute's
                // own binding wins).
                if attrs.iter().any(|(p, l, _, _)| {
                    l == &dlocal
                        && match (p, &dprefix) {
                            (Some(p), Some(dp)) => p == dp,
                            (None, None) => true,
                            _ => false,
                        }
                }) {
                    continue;
                }
                let local_cstr = Self::vec_to_cstr_null(&dlocal);
                let prefix_cstr = dprefix
                    .as_ref()
                    .map(|p| Self::vec_to_cstr_null(p))
                    .unwrap_or(ptr::null());
                let uri_cstr = match dprefix.as_deref() {
                    Some(dp) => resolve_uri(Some(dp))
                        .map(|u| Self::vec_to_cstr_null(&u))
                        .unwrap_or(ptr::null()),
                    None => ptr::null(),
                };
                // Same empty-value rule as the tag's own attributes (a
                // NULL start would drop the defaulted attribute).
                let value_cstr = if dval.is_empty() {
                    c"".as_ptr() as *const xmlChar
                } else {
                    Self::vec_to_cstr_null(dval)
                };
                attr_vec.push(local_cstr);
                attr_vec.push(prefix_cstr);
                attr_vec.push(uri_cstr);
                attr_vec.push(value_cstr);
                attr_vec.push({ value_cstr.add(dval.len()) } as *const xmlChar);
                nb_defaulted += 1;
            }

            let localname_cstr = self.sax_name_cstr(&localname);
            let prefix_cstr = prefix_opt
                .as_ref()
                .map(|p| Self::vec_to_cstr_null(p))
                .unwrap_or(ptr::null());
            let uri_cstr = uri
                .as_ref()
                .map(|u| Self::vec_to_cstr_null(u))
                .unwrap_or(ptr::null());
            let nb_namespaces = ns_decls.len() as c_int;
            let namespaces_ptr = if ns_vec.is_empty() {
                ptr::null_mut()
            } else {
                ns_vec.as_mut_ptr()
            };
            let nb_attributes = (attrs.len() as c_int) + nb_defaulted;
            let attributes_ptr = if attr_vec.is_empty() {
                ptr::null_mut()
            } else {
                attr_vec.as_mut_ptr()
            };

            // Leak the vectors so the pointers remain valid during the callback
            // In a production implementation, we'd manage this more carefully
            core::mem::forget(ns_vec);
            core::mem::forget(attr_vec);

            SaxDispatcher::start_element(
                sax,
                ctx,
                localname_cstr,
                prefix_cstr,
                uri_cstr,
                nb_namespaces,
                namespaces_ptr,
                nb_attributes,
                nb_defaulted,
                attributes_ptr,
            );
        }
    }

    /// Fire a SAX1 `startElement` event (upstream `xmlParseStartTag` SAX1
    /// dispatch): the RAW QName and a NULL-terminated SAX1 attribute array
    /// `[name1, value1, name2, value2, ..., NULL]` built from every
    /// attribute — xmlns declarations included, no namespace processing.
    ///
    /// `attributes` is the substituted `(raw_name, value, had_ref)` list.
    ///
    /// # Safety
    ///
    /// - `self.ctxt` must be a valid, initialized `_xmlParserCtxt` with
    ///   valid `sax`/`userData`; the caller-owned slices live for the call;
    ///   the NUL-terminated C strings passed to the SAX callback are
    ///   intentionally leaked and stay valid for the duration of the
    ///   callback.
    fn sax1_start_element(
        &mut self,
        name: &[u8],
        attributes: &[(Vec<u8>, Vec<u8>, bool)],
        end_pos: usize,
    ) {
        if self.sax_blocked() || self.below_delivery_boundary() {
            return;
        }
        self.sync_input_position();
        // UPSTREAM-PARITY: `cur` sits at the tag's closing `>` when the SAX
        // start-element callback fires.
        self.sync_cur_at(end_pos);
        unsafe {
            let sax = &*(*self.ctxt).sax;
            let ctx = (*self.ctxt).userData;
            let name_cstr = Self::vec_to_cstr_null(name);
            let mut att_vec: Vec<*const xmlChar> = Vec::with_capacity(attributes.len() * 2 + 1);
            for (n, v, _had_ref) in attributes {
                att_vec.push(Self::vec_to_cstr_null(n));
                att_vec.push(Self::vec_to_cstr_null(v));
            }
            att_vec.push(ptr::null());
            let atts_ptr = att_vec.as_ptr() as *mut *const xmlChar;
            core::mem::forget(att_vec);
            // Dispatch through the context's own SAX struct: `userData` is
            // the consumer's opaque context (PHP ext/xml compat passes its
            // XML_Parser object) and must NOT be reinterpreted as a parser
            // context here.
            if let Some(cb) = sax.startElement {
                cb(ctx, name_cstr, atts_ptr);
            }
        }
    }

    /// Fire `endElement` SAX event.
    ///
    /// # Safety
    ///
    /// - `self.ctxt` must be a valid, initialized `_xmlParserCtxt` with
    ///   valid `sax`/`userData`; `name` is a caller-owned slice live for the
    ///   call.
    fn sax_end_element(&mut self, name: &[u8]) {
        if self.sax_blocked() || self.below_delivery_boundary() {
            return;
        }
        // UPSTREAM-PARITY (parser.c xmlParseEndTag1/xmlParseEndTag2): the
        // endElement/endElementNs callback fires with the input already past
        // the end tag's closing '>' — xml_get_current_line_number/column/
        // byte_index report that position (bug26614: `</DATA> at line 9, col
        // %d (byte 96)`, one byte past the '>' of `</data>`). The end tag was
        // fully consumed by the tokenizer, so syncing here publishes the
        // position right after the tag (SP-14.3.1-5).
        self.sync_input_position();
        // SAX1 consumers (upstream xmlParseEndTag1) receive the raw QName
        // through the SAX1 endElement callback with no namespace processing.
        if !self.sax2_mode() {
            if !self.is_sax_disabled() {
                unsafe {
                    let sax = &*(*self.ctxt).sax;
                    let ctx = (*self.ctxt).userData;
                    let name_cstr = Self::vec_to_cstr_null(name);
                    if let Some(cb) = sax.endElement {
                        cb(ctx, name_cstr);
                    }
                }
            }
            return;
        }
        // UPSTREAM-PARITY (SAX2.c xmlSAX2EndElementNs): a validating parse
        // checks the just-closed element against the DTD before the node is
        // popped. The full per-element check (declaration lookup across both
        // subsets + etype-specific content checks) runs with the upstream
        // diagnostics attributed to the end-tag position; ctxt->valid is
        // cleared on every violation (XML_PARSE_DTDVALID consumers observe
        // the element errors, e.g. lxml `dtd_validation=True`).
        self.validate_end_element();
        unsafe {
            let sax = &*(*self.ctxt).sax;
            let ctx = (*self.ctxt).userData;
            // UPSTREAM-PARITY (parser.c xmlParseElementEnd): the
            // endElementNs callback receives the LOCAL name (ctxt->name),
            // the opening tag's prefix and its URI (tag->prefix / tag->URI)
            // — never the raw QName — so namespaced consumers (lxml's
            // _MultiTagMatcher) match end events like start events.
            let (prefix_opt, localname) = if let Some(colons) = name.iter().position(|&b| b == b':')
            {
                (Some(name[..colons].to_vec()), name[colons + 1..].to_vec())
            } else {
                (None, name.to_vec())
            };
            let name_cstr = self.sax_name_cstr(&localname);
            let prefix_cstr = prefix_opt
                .as_ref()
                .map(|p| Self::vec_to_cstr_null(p))
                .unwrap_or(ptr::null());
            let uri_cstr = {
                let node = (*self.ctxt).node;
                if node.is_null() {
                    // Pure-SAX parse (no tree): resolve through the
                    // parser-scoped namespace stack (upstream ctxt->nsTab).
                    let prefix = prefix_opt.as_deref();
                    let uri = self
                        .ns_scope
                        .iter()
                        .rev()
                        .find(|(p, _)| match prefix {
                            Some(pf) => p == pf,
                            None => p.is_empty(),
                        })
                        .map(|(_, u)| u.clone());
                    match uri {
                        Some(u) => {
                            if u.is_empty() {
                                // xmlns="" undeclares the default namespace:
                                // the SAX2 URI is NULL, not "".
                                ptr::null()
                            } else {
                                Self::vec_to_cstr_null(&u)
                            }
                        }
                        None => ptr::null(),
                    }
                } else {
                    let ns = (*node).ns;
                    if ns.is_null() {
                        ptr::null()
                    } else {
                        (*ns).href
                    }
                }
            };
            SaxDispatcher::end_element(sax, ctx, name_cstr, prefix_cstr, uri_cstr);
        }
    }

    /// UPSTREAM-PARITY (SAX2.c xmlSAX2EndElementNs): per-element DTD
    /// validation of the just-closed element, run before the node is popped
    /// (children are complete). Mirrors the tree path of upstream
    /// `xmlValidateOneElement` (valid.c): declaration lookup across the
    /// INTERNAL and EXTERNAL subsets (xmlValidGetElemDecl), then the
    /// etype-specific content checks (UNDEFINED / EMPTY / ANY / MIXED /
    /// ELEMENT) with the upstream diagnostics. Errors are attributed to the
    /// end-tag position (input already past the closing '>'), so libxml
    /// consumers (php) report the end-tag line, exactly like upstream's
    /// parser-located validity errors. Attribute declarations are NOT
    /// rechecked here (upstream validates attributes at start tags).
    fn validate_end_element(&mut self) {
        unsafe {
            let ctxt = self.ctxt;
            if (*ctxt).validate == 0 || (*ctxt).wellFormed == 0 || (*ctxt).node.is_null() {
                return;
            }
            let my_doc = (*ctxt).myDoc;
            if my_doc.is_null() {
                return;
            }
            let int_dtd = (*my_doc).intSubset;
            let ext_dtd = (*my_doc).extSubset;
            if int_dtd.is_null() && ext_dtd.is_null() {
                return;
            }
            let node = (*ctxt).node;
            if (*node).type_ != crate::abi::types::xmlElementType::XML_ELEMENT_NODE as c_int
                || (*node).name.is_null()
            {
                return;
            }
            let node_name = (*node).name;
            let prefix = if !(*node).ns.is_null() {
                (*(*node).ns).prefix
            } else {
                ptr::null()
            };

            // Declaration lookup (xmlValidGetElemDecl): prefixed lookup on
            // both subsets first, then the plain-name fallback, internal
            // subset winning over the external subset.
            let mut decl =
                crate::xml::validation::get_dtd_qelement_desc(int_dtd, node_name, prefix);
            if decl.is_null() && !ext_dtd.is_null() {
                decl = crate::xml::validation::get_dtd_qelement_desc(ext_dtd, node_name, prefix);
            }
            if decl.is_null() {
                decl =
                    crate::xml::validation::get_dtd_qelement_desc(int_dtd, node_name, ptr::null());
            }
            if decl.is_null() && !ext_dtd.is_null() {
                decl =
                    crate::xml::validation::get_dtd_qelement_desc(ext_dtd, node_name, ptr::null());
            }
            if decl.is_null() {
                (*ctxt).valid = 0;
                self.raise_end_element_valid_error(
                    crate::abi::types::XML_DTD_UNKNOWN_ELEM,
                    format!(
                        "No declaration for element {}\n",
                        crate::xml::string::xmlstr_to_string(node_name)
                    ),
                );
                return;
            }

            use crate::abi::types::xmlElementContentType::*;
            use crate::abi::types::xmlElementTypeVal::*;
            let mut ret = 1;
            match (*decl).etype {
                t if t == XML_ELEMENT_TYPE_UNDEFINED as c_int => {
                    (*ctxt).valid = 0;
                    self.raise_end_element_valid_error(
                        crate::abi::types::XML_DTD_UNKNOWN_ELEM,
                        format!(
                            "No declaration for element {}\n",
                            crate::xml::string::xmlstr_to_string(node_name)
                        ),
                    );
                    return;
                }
                t if t == XML_ELEMENT_TYPE_EMPTY as c_int => {
                    if !(*node).children.is_null() {
                        ret = 0;
                        self.raise_end_element_valid_error(
                            528, // XML_DTD_NOT_EMPTY
                            format!(
                                "Element {} was declared EMPTY this one has content\n",
                                crate::xml::string::xmlstr_to_string(node_name)
                            ),
                        );
                    }
                }
                t if t == XML_ELEMENT_TYPE_ANY as c_int => {}
                t if t == XML_ELEMENT_TYPE_MIXED as c_int => {
                    let content = (*decl).content;
                    if !content.is_null() && (*content).type_ == XML_ELEMENT_CONTENT_PCDATA as c_int
                    {
                        // Declared #PCDATA: any element child is an error.
                        let mut child = (*node).children;
                        while !child.is_null() {
                            if (*child).type_
                                == crate::abi::types::xmlElementType::XML_ELEMENT_NODE as c_int
                            {
                                ret = 0;
                                self.raise_end_element_valid_error(
                                    529, // XML_DTD_NOT_PCDATA
                                    format!(
                                        "Element {} was declared #PCDATA but contains non text nodes\n",
                                        crate::xml::string::xmlstr_to_string(node_name)
                                    ),
                                );
                                break;
                            }
                            child = (*child).next;
                        }
                    } else {
                        // Mixed list: every element child must be listed.
                        let mut child = (*node).children;
                        while !child.is_null() {
                            if (*child).type_
                                == crate::abi::types::xmlElementType::XML_ELEMENT_NODE as c_int
                            {
                                let mut fullname = (*child).name;
                                let mut owned: *mut xmlChar = ptr::null_mut();
                                if !(*child).ns.is_null() && !(*(*child).ns).prefix.is_null() {
                                    fullname = crate::xml::string::build_qname(
                                        (*child).name,
                                        (*(*child).ns).prefix,
                                        ptr::null_mut(),
                                        0,
                                    );
                                    owned = fullname as *mut xmlChar;
                                }
                                let ok = crate::xml::validation::validate_check_mixed(
                                    ptr::null_mut(),
                                    content,
                                    fullname,
                                );
                                if ok != 1 {
                                    ret = 0;
                                    self.raise_end_element_valid_error(
                                        515, // XML_DTD_INVALID_CHILD
                                        format!(
                                            "Element {} is not declared in {} list of possible children\n",
                                            crate::xml::string::xmlstr_to_string(fullname),
                                            crate::xml::string::xmlstr_to_string(node_name)
                                        ),
                                    );
                                }
                                if !owned.is_null() {
                                    crate::abi::allocator::xmlFreeImpl(owned as *mut c_void);
                                }
                            }
                            child = (*child).next;
                        }
                    }
                }
                t if t == XML_ELEMENT_TYPE_ELEMENT as c_int => {
                    // Element-only content: collect child element qnames and
                    // check against the content model.
                    let mut names: Vec<*const xmlChar> = Vec::new();
                    let mut owned: Vec<*mut xmlChar> = Vec::new();
                    let mut child = (*node).children;
                    while !child.is_null() {
                        if (*child).type_
                            == crate::abi::types::xmlElementType::XML_ELEMENT_NODE as c_int
                        {
                            let mut fullname = (*child).name;
                            if !(*child).ns.is_null() && !(*(*child).ns).prefix.is_null() {
                                let fnp = crate::xml::string::build_qname(
                                    (*child).name,
                                    (*(*child).ns).prefix,
                                    ptr::null_mut(),
                                    0,
                                );
                                if !fnp.is_null() {
                                    fullname = fnp;
                                    owned.push(fnp);
                                }
                            }
                            names.push(fullname);
                        }
                        child = (*child).next;
                    }
                    let result = crate::xml::dtd::valid_content_model((*decl).content, &names);
                    for n in owned {
                        crate::abi::allocator::xmlFreeImpl(n as *mut c_void);
                    }
                    if result != crate::xml::dtd::ContentModelResult::Valid {
                        ret = 0;
                        // expecting/got formatting (valid.c
                        // xmlValidateElementContent warn branch).
                        let elem_str = crate::xml::string::xmlstr_to_string(node_name);
                        let expr = snprintf_element_content((*decl).content, true);
                        let got = snprintf_children_list((*node).children);
                        self.raise_end_element_valid_error(
                            504, // XML_DTD_CONTENT_MODEL
                            format!(
                                "Element {} content does not follow the DTD, expecting {}, got {}\n",
                                elem_str, expr, got
                            ),
                        );
                    }
                }
                _ => {}
            }
            (*ctxt).valid &= ret;
        }
    }

    /// Raise one parse-time DTD validity error at the end-tag position.
    fn raise_end_element_valid_error(&mut self, code: c_int, msg: String) {
        unsafe {
            (*self.ctxt).valid = 0;
        }
        self.sync_input_position();
        let byte_pos = self.tokenizer.current_pos().2;
        self.raise_error_at(
            crate::abi::types::XML_FROM_VALID,
            code,
            crate::abi::types::xmlErrorLevel::XML_ERR_ERROR as c_int,
            msg,
            None,
            None,
            None,
            0,
            byte_pos,
        );
    }

    /// Fire `characters` SAX event.
    ///
    /// # Safety
    ///
    /// - `self.ctxt` must be a valid, initialized `_xmlParserCtxt` with
    ///   valid `sax`/`userData`; `data` is a caller-owned slice live for the
    ///   call.
    fn sax_characters(&mut self, data: &[u8]) {
        if self.sax_blocked() || data.is_empty() || self.below_delivery_boundary() {
            return;
        }
        self.sync_input_position();
        unsafe {
            let sax = &*(*self.ctxt).sax;
            let ctx = (*self.ctxt).userData;
            let data_cstr = Self::vec_to_cstr_null(data);
            SaxDispatcher::characters(sax, ctx, data_cstr, data.len() as c_int);
        }
    }

    /// Fire `comment` SAX event.
    ///
    /// # Safety
    ///
    /// - `self.ctxt` must be a valid, initialized `_xmlParserCtxt` with
    ///   valid `sax`/`userData`; `data` is a caller-owned slice live for the
    ///   call.
    fn sax_comment(&mut self, data: &[u8]) {
        if self.sax_blocked() || self.below_delivery_boundary() {
            return;
        }
        self.sync_input_position();
        unsafe {
            let sax = &*(*self.ctxt).sax;
            let ctx = (*self.ctxt).userData;
            let data_cstr = Self::vec_to_cstr_null(data);
            SaxDispatcher::comment(sax, ctx, data_cstr);
        }
    }

    /// Fire `processingInstruction` SAX event.
    ///
    /// # Safety
    ///
    /// - `self.ctxt` must be a valid, initialized `_xmlParserCtxt` with
    ///   valid `sax`/`userData`; `target` and `data` are caller-owned slices
    ///   live for the call.
    fn sax_pi(&mut self, target: &[u8], data: &[u8]) {
        if self.sax_blocked() || self.below_delivery_boundary() {
            return;
        }
        self.sync_input_position();
        unsafe {
            let sax = &*(*self.ctxt).sax;
            let ctx = (*self.ctxt).userData;
            let target_cstr = Self::vec_to_cstr_null(target);
            let data_cstr = Self::vec_to_cstr_null(data);
            SaxDispatcher::processing_instruction(sax, ctx, target_cstr, data_cstr);
        }
    }

    /// Fire `cdataBlock` SAX event.
    ///
    /// # Safety
    ///
    /// - `self.ctxt` must be a valid, initialized `_xmlParserCtxt` with
    ///   valid `sax`/`userData`; `data` is a caller-owned slice live for the
    ///   call.
    fn sax_cdata(&mut self, data: &[u8]) {
        if self.sax_blocked() || data.is_empty() || self.below_delivery_boundary() {
            return;
        }
        self.sync_input_position();
        unsafe {
            let sax = &*(*self.ctxt).sax;
            let ctx = (*self.ctxt).userData;
            let data_cstr = Self::vec_to_cstr_null(data);
            let len = data.len() as c_int;
            // UPSTREAM-PARITY (parser.c xmlParseCDSect): the CDATA content
            // goes to the cdataBlock callback unless it is NULL or
            // XML_PARSE_NOCDATA is set, in which case it falls back to the
            // characters callback. Consumers (lxml's default parser nulls
            // cdataBlock for strip_cdata) rely on the fallback — the
            // pre-fix dispatch dropped the content entirely.
            let use_chars = sax.cdataBlock.is_none()
                || ((*self.ctxt).options & crate::abi::types::XML_PARSE_NOCDATA) != 0;
            if use_chars {
                SaxDispatcher::characters(sax, ctx, data_cstr, len);
            } else {
                SaxDispatcher::cdata_block(sax, ctx, data_cstr, len);
            }
        }
    }

    /// Mirror the tokenizer's current (line, col) into `ctxt->input` so the
    /// default SAX tree builder can stamp node line numbers (upstream parity:
    /// nodes carry the line of their construct).
    ///
    /// # Safety
    ///
    /// - `self.ctxt` must be a valid, initialized `_xmlParserCtxt`; `input`
    ///   must be NULL or a valid `_xmlParserInput` whose `line` and `col`
    ///   fields are written.
    fn sync_input_position(&mut self) {
        unsafe {
            let ctxt = &mut *self.ctxt;
            if !ctxt.input.is_null() {
                // UPSTREAM-PARITY (SP-14.3.1-8, gh20439/bug27908): repoint
                // the C input at the buffer the tokenizer is CURRENTLY
                // reading — base/cur/end/line/col — so consumers that
                // dereference input->base/cur see live source text. PHP
                // expat-compat's default-handler raw-markup emit seeks back
                // from input->cur to the tag's opening '<' and passes the
                // span to the callback. The push context's C input was wired
                // to the empty constructor buffer and the accumulated buffer
                // is rebuilt on every xmlParseChunk, so the pointers must be
                // refreshed at every event (a stale base made the seek
                // dereference a dangling 0x1 pointer).
                self.tokenizer
                    .input()
                    .current_ref()
                    .populate_parser_input_without_filename(&mut *ctxt.input);
                // The candidate never shrinks the C input's buffer, so the
                // compat byte-index formula `consumed + (cur - base)` must
                // stay `cur - base`: consumed is reset to 0 (the populate
                // helper records the buffer's internal pos there, which would
                // double-count — bug26614 regressed from 96 to 192).
                (*ctxt.input).consumed = 0;
            }
        }
    }

    /// Point the C input's `cur` at an exact byte offset (upstream's `cur`
    /// sits at the tag's closing `>` when the SAX start-element callback
    /// fires — the depth-error window must be captured there,
    /// HOSTILE-FAILURE F1).
    fn sync_cur_at(&mut self, byte_pos: usize) {
        unsafe {
            let ctxt = &mut *self.ctxt;
            if !ctxt.input.is_null() && !(*ctxt.input).base.is_null() {
                (*ctxt.input).cur = (*ctxt.input).base.add(byte_pos);
            }
        }
    }

    /// Materialize the xml namespace on the document (upstream keeps it on
    /// `doc->oldNs`; created once per document).
    ///
    /// # Safety
    ///
    /// - `self.ctxt` must be a valid, initialized `_xmlParserCtxt`; `myDoc`
    ///   must be NULL or a valid `_xmlDoc` whose `oldNs` chain is a valid
    ///   linked list of `_xmlNs` nodes with NULL or null-terminated `prefix`
    ///   and `href`.
    fn ensure_doc_xml_ns(&mut self) {
        unsafe {
            let doc = (*self.ctxt).myDoc;
            if doc.is_null() {
                return;
            }
            let mut ns = (*doc).oldNs;
            while !ns.is_null() {
                if !(*ns).prefix.is_null()
                    && crate::abi::exports_xml2::xmlStrEqual(
                        (*ns).prefix,
                        c"xml".as_ptr() as *const xmlChar,
                    ) != 0
                {
                    return;
                }
                ns = (*ns).next;
            }
            let new_ns = crate::abi::allocator::xmlMallocZero(size_of::<_xmlNs>()) as *mut _xmlNs;
            if new_ns.is_null() {
                return;
            }
            (*new_ns).type_ = crate::abi::types::XML_LOCAL_NAMESPACE as c_int;
            (*new_ns).href = crate::xml::string::xml_strdup(
                c"http://www.w3.org/XML/1998/namespace".as_ptr() as *const xmlChar,
            );
            (*new_ns).prefix = crate::xml::string::xml_strdup(c"xml".as_ptr() as *const xmlChar);
            (*new_ns).next = (*doc).oldNs;
            (*doc).oldNs = new_ns;
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Name stack management
    // ─────────────────────────────────────────────────────────────────────────

    /// Push an element name onto the context's name stack.
    ///
    /// # Safety
    ///
    /// - `self.ctxt` must be a valid, initialized `_xmlParserCtxt`; `name`
    ///   is a caller-owned slice; the NUL-terminated copy is leaked
    ///   intentionally and remains valid until the context is freed.
    fn push_name(&mut self, name: &[u8]) {
        let name_cstr = Self::vec_to_cstr_null(name);
        unsafe {
            // Store in the context's name field
            (*self.ctxt).name = name_cstr;
            (*self.ctxt).nameNr += 1;
        }
    }

    /// Pop an element name from the context's name stack.
    ///
    /// # Safety
    ///
    /// - `self.ctxt` must be a valid, initialized `_xmlParserCtxt`; the
    ///   `name` pointer is cleared when the stack empties and must not be
    ///   used afterwards.
    fn pop_name(&mut self) {
        unsafe {
            if (*self.ctxt).nameNr > 0 {
                (*self.ctxt).nameNr -= 1;
            }
            if (*self.ctxt).nameNr == 0 {
                (*self.ctxt).name = ptr::null();
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Token stack / push-back
    // ─────────────────────────────────────────────────────────────────────────

    /// Push a token back onto the token stream (single-token lookahead).
    ///
    /// Push a token back onto the tokenizer's input.
    fn push_token_back(&mut self, token: &XmlToken) {
        self.tokenizer.push_back_token(token.clone());
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Error handling
    // ─────────────────────────────────────────────────────────────────────────

    /// Set an error on the parser context (legacy wrapper: ERROR level at
    /// the current position).
    fn set_error(&mut self, code: c_int, msg: &str) {
        self.raise_error_now(
            XML_FROM_PARSER,
            code,
            xmlErrorLevel::XML_ERR_ERROR as c_int,
            format!("{}\n", msg),
            None,
            None,
            None,
            0,
        );
    }

    /// Set a warning on the parser context.
    fn set_warning(&mut self, msg: &str) {
        self.raise_error_now(
            XML_FROM_PARSER,
            0,
            xmlErrorLevel::XML_ERR_WARNING as c_int,
            format!("{}\n", msg),
            None,
            None,
            None,
            0,
        );
    }

    /// Raise a parser error with upstream's full routing (11.1-M):
    /// context bookkeeping (errNo / wellFormed / nbErrors / nbWarnings),
    /// then — unless XML_PARSE_NOERROR — delivery through the structured
    /// handler or the selected generic channel.
    ///
    /// `line`/`col` are 1-based (col is a byte column, upstream
    /// `input->col`); `window` is the source line + 0-based caret column;
    /// `enc_bytes` feeds the `XML_ERR_INVALID_ENCODING` "Bytes:" fragment.
    ///
    /// # Safety
    ///
    /// - `self.ctxt` must be a valid, initialized `_xmlParserCtxt`; the
    ///   error path dereferences `self.ctxt` directly and passes it to
    ///   `raise_error_streamed` as an opaque pointer; the tokenizer's
    ///   current input must be valid for `filename()`.
    #[allow(clippy::too_many_arguments)]
    fn raise_parser_error(
        &mut self,
        domain: c_int,
        code: c_int,
        level: c_int,
        msg: String,
        str1: Option<Vec<u8>>,
        str2: Option<Vec<u8>>,
        str3: Option<Vec<u8>>,
        int1: c_int,
        line: c_int,
        col: c_int,
        window: Option<(Vec<u8>, usize)>,
        enc_bytes: Option<[u8; 4]>,
    ) {
        // Incremental probe mode (helpers::parse_chunk) keeps the context
        // bookkeeping — errNo / nbErrors / wellFormed drive the probe's
        // completeness verdict and must stay faithful — but defers every
        // DELIVERY to handlers (SP-14.3.1-3): probes re-parse the accumulated
        // buffer on each push call, so invoking the structured/generic error
        // channel here would duplicate diagnostics once the completing parse
        // runs. A probe fatal must likewise not set disableSAX = 1 (that
        // would suppress the events preceding the fatal in the completing
        // parse); catastrophic stops (disableSAX = 2) stay effective because
        // they gate the parse flow itself.
        let defer_delivery = self.probe;
        unsafe {
            // UPSTREAM-PARITY (parserInternals.c xmlCtxtVErr): catastrophic
            // errors — XML_ERR_RESOURCE_LIMIT and XML_ERR_ENTITY_LOOP (plus
            // the xmlIsCatastrophicError family) — set disableSAX = 2, which
            // really stops the parser. Other errors are suppressed once 100
            // have been reported and the document is already not well-formed;
            // a fatal error (non-recovery) disables further SAX dispatch.
            if code == XML_ERR_RESOURCE_LIMIT || code == XML_ERR_ENTITY_LOOP {
                (*self.ctxt).disableSAX = 2;
            } else {
                // Report at least one fatal error.
                if (*self.ctxt).nbErrors >= 100
                    && (level < xmlErrorLevel::XML_ERR_FATAL as c_int
                        || (*self.ctxt).wellFormed == 0)
                {
                    return;
                }
                if level == xmlErrorLevel::XML_ERR_FATAL as c_int
                    && !self.is_recovery()
                    && !defer_delivery
                {
                    (*self.ctxt).disableSAX = 1;
                }
            }

            // UPSTREAM-PARITY (parserInternals.c xmlCtxtVErr): warnings only
            // bump nbWarnings; other levels update errNo, nbErrors, and —
            // for fatal errors only — clear wellFormed.
            if level == xmlErrorLevel::XML_ERR_WARNING as c_int {
                (*self.ctxt).nbWarnings = (*self.ctxt).nbWarnings.wrapping_add(1);
            } else {
                (*self.ctxt).errNo = code;
                if level == xmlErrorLevel::XML_ERR_FATAL as c_int {
                    (*self.ctxt).wellFormed = 0;
                }
                (*self.ctxt).nbErrors = (*self.ctxt).nbErrors.wrapping_add(1);
            }
        }

        if !defer_delivery && unsafe { (*self.ctxt).options } & XML_PARSE_NOERROR == 0 {
            let msg_cstr = std::ffi::CString::new(msg).unwrap_or_default();
            let s1 = str1.and_then(|s| std::ffi::CString::new(s).ok());
            let s2 = str2.and_then(|s| std::ffi::CString::new(s).ok());
            let s3 = str3.and_then(|s| std::ffi::CString::new(s).ok());
            // UPSTREAM-PARITY (parserInternals.c xmlCtxtVErr): the error
            // location is the current input's filename/line/col, but when
            // the current input has no filename and the input stack is
            // nested (inputNr > 1) upstream falls back to the PARENT
            // input's location — entity-content errors are attributed to
            // the referencing document (HOSTILE-CALLBACKS C1/C2).
            let (fname_opt, fline, fcol) = self.tokenizer.input().error_context();
            let fname = fname_opt.map(|f| std::ffi::CString::new(f).unwrap_or_default());
            let file_ptr = fname.as_ref().map(|c| c.as_ptr()).unwrap_or(ptr::null());
            let parent_context = self.tokenizer.input().depth() > 1
                && self.tokenizer.input().current_ref().filename().is_none();
            let line = if parent_context { fline as c_int } else { line };
            let col = if parent_context { fcol as c_int } else { col };
            let s1_ptr = s1.as_ref().map(|c| c.as_ptr()).unwrap_or(ptr::null());
            let s2_ptr = s2.as_ref().map(|c| c.as_ptr()).unwrap_or(ptr::null());
            let s3_ptr = s3.as_ref().map(|c| c.as_ptr()).unwrap_or(ptr::null());
            let window_ref = window.as_ref().map(|(w, caret)| (w.as_slice(), *caret));
            // UPSTREAM-PARITY (error.c xmlFormatError): the "cur input" tail
            // (current input's info + window after the parent window) — set
            // by the entity-loop raise (HOSTILE-FAILURE F2).
            let tail = self.cur_error_tail.take();
            let tail_ref = tail
                .as_ref()
                .map(|(l, w)| (*l, w.as_ref().map(|(wb, c)| (wb.as_slice(), *c))));
            let delivery = self.error_delivery();
            unsafe {
                crate::xml::errors::raise_error_streamed(
                    self.ctxt as *mut c_void,
                    domain,
                    code,
                    level,
                    file_ptr,
                    line,
                    col,
                    s1_ptr,
                    s2_ptr,
                    s3_ptr,
                    int1,
                    msg_cstr.as_ptr(),
                    window_ref,
                    enc_bytes,
                    delivery,
                    tail_ref,
                );
            }
        }
    }

    /// Raise all errors recorded by the tokenizer during the last scan (in
    /// order — upstream raises them at their detection points).
    fn raise_pending_errors(&mut self) {
        let errors = self.tokenizer.take_errors();
        for e in errors {
            self.raise_parser_error(
                e.domain,
                e.code,
                e.level,
                e.msg,
                e.str1,
                e.str2,
                e.str3,
                e.int1,
                e.line,
                e.col,
                e.window,
                e.enc_bytes,
            );
        }
    }

    /// UPSTREAM-PARITY (parser.c xmlParserEntityCheck): the entity-expansion
    /// amplification guard. Every general-entity reference adds the referenced
    /// entity's expanded size plus XML_ENT_FIXED_COST to an accumulation slot
    /// and, once that exceeds XML_PARSER_ALLOWED_EXPANSION while the ratio to
    /// the consumed input exceeds the amplification factor (default
    /// XML_MAX_AMPLIFICATION_DEFAULT, or `xmlCtxtSetMaxAmplification`'s
    /// value), raises a fatal XML_ERR_RESOURCE_LIMIT — with no XML_PARSE_HUGE
    /// bypass (the guard is unconditional, matching 2.15). Lineage: SEC-0006
    /// (CVE-2014-3660, fix commit be2a7eda + regression fix 72a46a51), verified
    /// by the SECURITY-LIMITS court amplification sweep.
    ///
    /// `slot` is the accumulation target: for references nested inside an
    /// entity content scan it is the scanning entity's `expandedSize`
    /// (upstream: the current input's entity); for top-level document
    /// references it is NULL and `ctxt->sizeentcopy` is used. `consumed` is
    /// the current stream position after the reference (upstream
    /// `input->consumed + (cur - base)`).
    ///
    /// Returns `true` when the error was raised (caller must abort).
    ///
    /// # Safety
    ///
    /// - `self.ctxt` must be a valid, initialized `_xmlParserCtxt`; `slot`
    ///   must be NULL or a valid writable `c_ulong` accumulation slot
    ///   (typically the scanning entity's `expandedSize`).
    fn parser_entity_check(
        &mut self,
        extra: c_ulong,
        slot: *mut c_ulong,
        consumed: c_ulong,
    ) -> bool {
        const XML_PARSER_ALLOWED_EXPANSION: c_ulong = 1_000_000;
        const XML_ENT_FIXED_COST: c_ulong = 20;
        const XML_MAX_AMPLIFICATION_DEFAULT: c_ulong = 5;

        unsafe {
            let consumed = consumed.saturating_add((*self.ctxt).sizeentities);
            let target = if slot.is_null() {
                (*self.ctxt).sizeentcopy = (*self.ctxt)
                    .sizeentcopy
                    .saturating_add(extra)
                    .saturating_add(XML_ENT_FIXED_COST);
                (*self.ctxt).sizeentcopy
            } else {
                *slot = (*slot)
                    .saturating_add(extra)
                    .saturating_add(XML_ENT_FIXED_COST);
                *slot
            };
            let max_ampl = if (*self.ctxt).maxAmpl == 0 {
                XML_MAX_AMPLIFICATION_DEFAULT
            } else {
                (*self.ctxt).maxAmpl as c_ulong
            };
            if target > XML_PARSER_ALLOWED_EXPANSION
                && (target == c_ulong::MAX || target / max_ampl > consumed)
            {
                self.raise_error_now(
                    XML_FROM_PARSER,
                    XML_ERR_RESOURCE_LIMIT,
                    xmlErrorLevel::XML_ERR_FATAL as c_int,
                    "Maximum entity amplification factor exceeded, see ".to_string()
                        + "xmlCtxtSetMaxAmplification.\n",
                    None,
                    None,
                    None,
                    0,
                );
                return true;
            }
        }
        false
    }

    /// Raise an error at the tokenizer's current position.
    #[allow(clippy::too_many_arguments)]
    fn raise_error_now(
        &mut self,
        domain: c_int,
        code: c_int,
        level: c_int,
        msg: String,
        str1: Option<Vec<u8>>,
        str2: Option<Vec<u8>>,
        str3: Option<Vec<u8>>,
        int1: c_int,
    ) {
        let (line, col, window) = self.tokenizer.capture_error_pos();
        self.raise_parser_error(
            domain, code, level, msg, str1, str2, str3, int1, line, col, window, None,
        );
    }

    /// Raise an error attributed to a specific byte position (e.g. the
    /// start of a token).
    #[allow(clippy::too_many_arguments)]
    fn raise_error_at(
        &mut self,
        domain: c_int,
        code: c_int,
        level: c_int,
        msg: String,
        str1: Option<Vec<u8>>,
        str2: Option<Vec<u8>>,
        str3: Option<Vec<u8>>,
        int1: c_int,
        byte_pos: usize,
    ) {
        self.tokenizer.record_error_at(
            domain, code, level, msg, str1, str2, str3, int1, byte_pos, None,
        );
        self.raise_pending_errors();
    }

    /// Select the generic delivery for a parser error: a custom SAX `error`
    /// slot is called directly (upstream `channel(data, msg)` path), while
    /// the default/legacy handlers route through the fragment stream.
    ///
    /// # Safety
    ///
    /// - `self.ctxt` must be a valid, initialized `_xmlParserCtxt` whose
    ///   `sax` is a valid `_xmlSAXHandler`; the `error` slot is read to
    ///   choose the delivery, and `userData` is captured for the custom
    ///   callback.
    fn error_delivery(&self) -> crate::xml::errors::GenericDelivery {
        unsafe { crate::xml::errors::parser_delivery(self.ctxt) }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Utility helpers
    // ─────────────────────────────────────────────────────────────────────────

    /// SAX callback name pointer: with `dictNames` the parser hands the
    /// callback the dict-interned string (upstream `xmlParseQNameHashed` /
    /// `xmlParseEndTag2` return the document-dictionary pointer), so
    /// consumers that compare name POINTERS — lxml's `_MultiTagMatcher`
    /// (iterparse/iterwalk `tag=...`) interns its tags with `xmlDictLookup`
    /// on the same dictionary and requires pointer identity — match. Falls
    /// back to a fresh NUL-terminated copy when the dictionary is
    /// unavailable.
    ///
    /// # Safety
    ///
    /// - `name` is a caller-owned byte slice live for the call; the returned
    ///   pointer is valid for the duration of the SAX callback (dict-owned or
    ///   leaked copy).
    unsafe fn sax_name_cstr(&self, name: &[u8]) -> *const xmlChar {
        unsafe {
            let ctxt = self.ctxt;
            if (*ctxt).dictNames != 0 && !(*ctxt).dict.is_null() && !name.is_empty() {
                let interned = crate::abi::exports_xml2::xmlDictLookup(
                    (*ctxt).dict,
                    name.as_ptr() as *const xmlChar,
                    name.len() as c_int,
                );
                if !interned.is_null() {
                    return interned;
                }
            }
            Self::vec_to_cstr_null(name)
        }
    }

    /// Convert a byte slice to a null-terminated C string pointer.
    /// The returned pointer is valid until the backing memory is freed.
    fn vec_to_cstr_null(data: &[u8]) -> *const xmlChar {
        if data.is_empty() {
            return ptr::null();
        }
        // Allocate a null-terminated buffer
        let mut buf = data.to_vec();
        buf.push(0);
        let ptr = buf.as_ptr();
        // Leak the buffer so the pointer remains valid
        core::mem::forget(buf);
        ptr as *const xmlChar
    }

    /// Convert a byte slice to a null-terminated C string pointer, preserving
    /// an EMPTY slice as a non-NULL single NUL byte.
    ///
    /// UPSTREAM-PARITY (parser.c xmlParsePubidLiteral/xmlParseSystemLiteral):
    /// a quoted `PUBLIC ""` / `SYSTEM ""` literal yields an allocated empty
    /// string, never NULL — DOMEntity::$publicId/$systemId distinguish the
    /// empty literal from an absent id.
    fn vec_to_cstr_keep_empty(data: &[u8]) -> *const xmlChar {
        // Allocate a null-terminated buffer (empty => single NUL byte)
        let mut buf = data.to_vec();
        buf.push(0);
        let ptr = buf.as_ptr();
        // Leak the buffer so the pointer remains valid
        core::mem::forget(buf);
        ptr as *const xmlChar
    }

    /// Convert a byte slice to a mutable null-terminated C string pointer.
    fn vec_to_cstr(data: &[u8]) -> *mut xmlChar {
        if data.is_empty() {
            return ptr::null_mut();
        }
        let mut buf = data.to_vec();
        buf.push(0);
        let ptr = buf.as_mut_ptr();
        core::mem::forget(buf);
        ptr as *mut xmlChar
    }

    /// Extract external ID and system ID from content after the root element name.
    fn extract_external_id(content: &[u8]) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
        let content = content.trim_ascii_start();

        // Check for PUBLIC
        if content.len() > 6 && content[..6].eq_ignore_ascii_case(b"PUBLIC") {
            let after = content[6..].trim_ascii_start();
            // Skip the public ID literal
            if after.is_empty() || (after[0] != b'"' && after[0] != b'\'') {
                return (None, None);
            }
            let quote = after[0];
            let end = match after[1..].iter().position(|&b| b == quote) {
                Some(p) => p + 1,
                None => return (None, None),
            };
            let pub_id = Some(after[1..end].to_vec());
            let rest = after[end + 1..].trim_ascii_start();

            // Skip the system ID literal
            if rest.is_empty() || (rest[0] != b'"' && rest[0] != b'\'') {
                return (pub_id, None);
            }
            let sys_quote = rest[0];
            let sys_end = match rest[1..].iter().position(|&b| b == sys_quote) {
                Some(p) => p + 1,
                None => return (pub_id, None),
            };
            let sys_id = Some(rest[1..sys_end].to_vec());
            return (pub_id, sys_id);
        }

        // Check for SYSTEM
        if content.len() > 6 && content[..6].eq_ignore_ascii_case(b"SYSTEM") {
            let after = content[6..].trim_ascii_start();
            if after.is_empty() || (after[0] != b'"' && after[0] != b'\'') {
                return (None, None);
            }
            let quote = after[0];
            let sys_end = match after[1..].iter().position(|&b| b == quote) {
                Some(p) => p + 1,
                None => return (None, None),
            };
            let sys_id = Some(after[1..sys_end].to_vec());
            return (None, sys_id);
        }

        (None, None)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Free functions
// ─────────────────────────────────────────────────────────────────────────────

/// Check whether a Unicode codepoint is a valid XML character.
const fn is_valid_xml_char(codepoint: u32) -> bool {
    matches!(
        codepoint,
        0x09 | 0x0A | 0x0D | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x10000..=0x10FFFF
    )
}

/// Helper trait to trim ASCII whitespace from byte slices.
#[allow(dead_code)]
trait TrimAscii {
    fn trim_ascii_start(&self) -> &[u8];
}

impl TrimAscii for [u8] {
    fn trim_ascii_start(&self) -> &[u8] {
        let start = self
            .iter()
            .position(|&b| !b.is_ascii_whitespace())
            .unwrap_or(self.len());
        &self[start..]
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// DTD internal-subset scanning helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Trim leading and trailing ASCII whitespace.
fn trim_ascii(s: &[u8]) -> &[u8] {
    let start = s
        .iter()
        .position(|&b| !b.is_ascii_whitespace())
        .unwrap_or(s.len());
    let end = s
        .iter()
        .rposition(|&b| !b.is_ascii_whitespace())
        .map(|i| i + 1)
        .unwrap_or(start);
    &s[start..end]
}

/// Find a sub-sequence; returns the byte offset or None.
fn find_subseq(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// XML NameStartChar test over raw DTD-subset bytes (upstream
/// `xmlIsNameStartChar`). Non-ASCII bytes are accepted loosely because the
/// subset scan works on the declared encoding's bytes; ASCII follows the XML
/// grammar exactly.
const fn dtd_name_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b == b':' || b >= 0x80
}

/// XML NameChar test over raw DTD-subset bytes (upstream `xmlIsNameChar`).
const fn dtd_name_char(b: u8) -> bool {
    dtd_name_start(b) || b.is_ascii_digit() || b == b'-' || b == b'.'
}

/// Find the closing `>` of a markup declaration, honoring quoted strings.
fn find_decl_end(s: &[u8]) -> Option<usize> {
    let mut in_quote: u8 = 0;
    for (i, &b) in s.iter().enumerate() {
        if in_quote != 0 {
            if b == in_quote {
                in_quote = 0;
            }
            continue;
        }
        if b == b'\'' || b == b'"' {
            in_quote = b;
        } else if b == b'>' {
            return Some(i);
        }
    }
    None
}

/// Skip ASCII whitespace.
const fn skip_ws(s: &[u8], idx: &mut usize) {
    while *idx < s.len() && s[*idx].is_ascii_whitespace() {
        *idx += 1;
    }
}

/// Read an XML Name; returns the slice and advances `idx`.
fn read_name<'a>(s: &'a [u8], idx: &mut usize) -> &'a [u8] {
    let start = *idx;
    while *idx < s.len() {
        let b = s[*idx];
        if b.is_ascii_alphanumeric()
            || b == b'_'
            || b == b':'
            || b == b'-'
            || b == b'.'
            || b >= 0x80
        {
            *idx += 1;
        } else {
            break;
        }
    }
    &s[start..*idx]
}

/// Apply an occurrence suffix (`?`, `*`, `+`) to a content-model node.
///
/// # Safety
///
/// - `node` must be NULL or a valid, non-const `_xmlElementContent` whose
///   `ocur` field is written; `s` is a caller-owned byte slice; `idx` is a
///   valid writable index into `s`.
fn apply_occurrence(node: *mut crate::abi::structs::_xmlElementContent, s: &[u8], idx: &mut usize) {
    if node.is_null() {
        return;
    }
    skip_ws(s, idx);
    if *idx >= s.len() {
        return;
    }
    let ocur = match s[*idx] {
        b'?' => crate::abi::types::xmlElementContentOccur::XML_ELEMENT_CONTENT_OPT as c_int,
        b'*' => crate::abi::types::xmlElementContentOccur::XML_ELEMENT_CONTENT_MULT as c_int,
        b'+' => crate::abi::types::xmlElementContentOccur::XML_ELEMENT_CONTENT_PLUS as c_int,
        _ => return,
    };
    *idx += 1;
    unsafe {
        (*node).ocur = ocur;
    }
}

/// Read a quoted string (double or single). Returns the unquoted content or None.
fn read_quoted(s: &[u8]) -> Option<&[u8]> {
    let s = trim_ascii(s);
    if s.len() < 2 {
        return None;
    }
    let q = s[0];
    if q != b'\'' && q != b'"' {
        return None;
    }
    let end = s[1..].iter().position(|&b| b == q)?;
    Some(&s[1..1 + end])
}

/// Find the notation introduced by `NDATA` in an external entity declaration.
///
/// Returns `(has_ndata, notation)`. `has_ndata` is true whenever a standalone
/// `NDATA` keyword is present (even with no following name — DOMEntity_fields
/// tolerates the empty form as an empty notation); the notation name, when
/// present, is returned in the second element. Tokens inside quoted literals
/// (public/system ids) are skipped so an occurrence of the substring `NDATA`
/// inside a URI is not misread.
fn find_ndata_notation(s: &[u8]) -> (bool, Option<&[u8]>) {
    let mut i = 0usize;
    let n = s.len();
    while i < n {
        while i < n && (s[i] as char).is_ascii_whitespace() {
            i += 1;
        }
        if i >= n {
            break;
        }
        if s[i] == b'\'' || s[i] == b'"' {
            // Skip the whole quoted literal (ids may contain spaces/NDATA).
            let q = s[i];
            i += 1;
            while i < n && s[i] != q {
                i += 1;
            }
            i = (i + 1).min(n);
            continue;
        }
        let start = i;
        while i < n && !(s[i] as char).is_ascii_whitespace() && s[i] != b'\'' && s[i] != b'"' {
            i += 1;
        }
        if i == start {
            i += 1;
            continue;
        }
        let tok = &s[start..i];
        if tok.eq_ignore_ascii_case(b"NDATA") {
            // The next token (if any) is the notation name.
            let mut j = i;
            while j < n && (s[j] as char).is_ascii_whitespace() {
                j += 1;
            }
            if j >= n || s[j] == b'\'' || s[j] == b'"' {
                return (true, None);
            }
            let ns = j;
            while j < n && !(s[j] as char).is_ascii_whitespace() && s[j] != b'\'' && s[j] != b'"' {
                j += 1;
            }
            return (true, Some(&s[ns..j]));
        }
    }
    (false, None)
}

/// Split `PUBLIC "pub" "sys"` or `SYSTEM "sys"` into the two quoted parts.
fn split_two_quoted(s: &[u8]) -> (Option<&[u8]>, Option<&[u8]>) {
    let s = trim_ascii(s);
    let mut rest = s;
    let mut first: Option<&[u8]> = None;
    let mut second: Option<&[u8]> = None;
    while !rest.is_empty() {
        let rest_t = trim_ascii(rest);
        if rest_t.is_empty() {
            break;
        }
        // Skip an optional keyword (PUBLIC/SYSTEM).
        if rest_t[0].is_ascii_alphabetic() {
            let kw_len = rest_t
                .iter()
                .position(|&b| b.is_ascii_whitespace())
                .unwrap_or(rest_t.len());
            rest = trim_ascii(&rest_t[kw_len..]);
            continue;
        }
        if rest_t[0] == b'\'' || rest_t[0] == b'"' {
            let q = rest_t[0];
            let end = rest_t[1..].iter().position(|&b| b == q);
            match end {
                Some(e) => {
                    if first.is_none() {
                        first = Some(&rest_t[1..1 + e]);
                    } else {
                        second = Some(&rest_t[1..1 + e]);
                    }
                    rest = trim_ascii(&rest_t[1 + e + 1..]);
                }
                None => break,
            }
            continue;
        }
        break;
    }
    (first, second)
}

/// Parse an attribute type: `CDATA | ID | IDREF | IDREFS | ENTITY | ENTITIES |
/// NMTOKEN | NMTOKENS | NOTATION (...) | (a | b | c)`.
///
/// Returns `(type, enumeration_tree, consumed_bytes)`.
fn parse_attr_type(s: &[u8]) -> (c_int, *mut crate::abi::structs::_xmlEnumeration, usize) {
    use crate::abi::types::xmlAttributeType::*;
    let s = trim_ascii(s);
    let rest = s;
    let kw_len = rest
        .iter()
        .position(|&b| b.is_ascii_whitespace())
        .unwrap_or(rest.len());
    let kw = &rest[..kw_len];
    let simple = match kw {
        k if k.eq_ignore_ascii_case(b"CDATA") => Some(XML_ATTRIBUTE_CDATA as c_int),
        k if k.eq_ignore_ascii_case(b"ID") => Some(XML_ATTRIBUTE_ID as c_int),
        k if k.eq_ignore_ascii_case(b"IDREF") => Some(XML_ATTRIBUTE_IDREF as c_int),
        k if k.eq_ignore_ascii_case(b"IDREFS") => Some(XML_ATTRIBUTE_IDREFS as c_int),
        k if k.eq_ignore_ascii_case(b"ENTITY") => Some(XML_ATTRIBUTE_ENTITY as c_int),
        k if k.eq_ignore_ascii_case(b"ENTITIES") => Some(XML_ATTRIBUTE_ENTITIES as c_int),
        k if k.eq_ignore_ascii_case(b"NMTOKEN") => Some(XML_ATTRIBUTE_NMTOKEN as c_int),
        k if k.eq_ignore_ascii_case(b"NMTOKENS") => Some(XML_ATTRIBUTE_NMTOKENS as c_int),
        _ => None,
    };
    if let Some(t) = simple {
        return (t, ptr::null_mut(), kw_len);
    }
    if kw.eq_ignore_ascii_case(b"NOTATION") {
        let after = trim_ascii(&rest[kw_len..]);
        if after.starts_with(b"(") {
            let (tree, consumed) = parse_enumeration(after);
            return (XML_ATTRIBUTE_NOTATION as c_int, tree, kw_len + consumed);
        }
        return (XML_ATTRIBUTE_NOTATION as c_int, ptr::null_mut(), kw_len);
    }
    // Enumeration: (a | b | c)
    if rest.starts_with(b"(") {
        let (tree, consumed) = parse_enumeration(rest);
        return (XML_ATTRIBUTE_ENUMERATION as c_int, tree, consumed);
    }
    (XML_ATTRIBUTE_CDATA as c_int, ptr::null_mut(), kw_len)
}

/// Consume an attribute TYPE from the start of an ATTLIST body WITHOUT
/// building the enumeration chain (parse_attr_type minus the allocation);
/// returns the number of bytes consumed. Mirrors parse_attr_type's scan so
/// the defaults collector stays aligned with parse_attlist_decl.
fn skip_dtd_attr_type(s: &[u8]) -> usize {
    let s = trim_ascii(s);
    if s.starts_with(b"(") {
        // Enumeration: (a | b | c)
        let mut depth = 0usize;
        for (i, &b) in s.iter().enumerate() {
            if b == b'(' {
                depth += 1;
            } else if b == b')' {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
        }
        return s.len();
    }
    let kw_len = s
        .iter()
        .position(|&b| b.is_ascii_whitespace())
        .unwrap_or(s.len());
    if s[..kw_len].eq_ignore_ascii_case(b"NOTATION") {
        let tail = &s[kw_len..];
        let after = trim_ascii(tail);
        if after.starts_with(b"(") {
            let lead = kw_len + (tail.len() - after.len());
            let mut depth = 0usize;
            for (i, &b) in after.iter().enumerate() {
                if b == b'(' {
                    depth += 1;
                } else if b == b')' {
                    depth -= 1;
                    if depth == 0 {
                        return lead + i + 1;
                    }
                }
            }
            return s.len();
        }
    }
    kw_len
}

/// Parse `( a | b | c )` into an enumeration chain.
///
/// # Safety
///
/// - `s` is a caller-owned byte slice, live for the call; the returned
///   `_xmlEnumeration` chain consists of zero-initialized nodes whose `name`
///   fields are `xml_strdup` allocations and whose `next` links are valid
///   (the last is NULL); the caller owns the chain.
fn parse_enumeration(s: &[u8]) -> (*mut crate::abi::structs::_xmlEnumeration, usize) {
    let s = trim_ascii(s);
    if !s.starts_with(b"(") {
        return (ptr::null_mut(), 0);
    }
    let mut idx = 1usize;
    let mut head: *mut crate::abi::structs::_xmlEnumeration = ptr::null_mut();
    let mut tail: *mut crate::abi::structs::_xmlEnumeration = ptr::null_mut();
    loop {
        skip_ws(s, &mut idx);
        if idx >= s.len() {
            break;
        }
        if s[idx] == b')' {
            idx += 1;
            break;
        }
        if s[idx] == b'|' {
            idx += 1;
            continue;
        }
        let name = read_name(s, &mut idx);
        if name.is_empty() {
            break;
        }
        let node = unsafe {
            crate::abi::allocator::xmlMallocZero(size_of::<crate::abi::structs::_xmlEnumeration>())
                as *mut crate::abi::structs::_xmlEnumeration
        };
        if node.is_null() {
            break;
        }
        unsafe {
            let tmp = vec_to_cstr_null_helper(name);
            (*node).name = crate::xml::string::xml_strdup(tmp as *const u8);
            crate::abi::allocator::xmlFreeImpl(tmp as *mut c_void);
        }
        if head.is_null() {
            head = node;
        } else {
            unsafe {
                (*tail).next = node;
            }
        }
        tail = node;
    }
    (head, idx)
}

/// Parse an attribute default: `#REQUIRED | #IMPLIED | #FIXED "v" | "v"`.
///
/// Returns `(def, default_value, consumed_bytes)` where `consumed_bytes` is
/// measured from the START of the (trimmed) input and covers the whole
/// default construct (keyword + required whitespace + quoted literal), so a
/// caller can continue parsing further attributes of the same `<!ATTLIST>`.
fn parse_attr_default(s: &[u8]) -> (c_int, Option<&[u8]>, usize) {
    use crate::abi::types::xmlAttributeDefault::*;
    let s = trim_ascii(s);
    let rest = s;
    let kw_len = rest
        .iter()
        .position(|&b| b.is_ascii_whitespace())
        .unwrap_or(rest.len());
    let kw = &rest[..kw_len];
    if kw.eq_ignore_ascii_case(b"#REQUIRED") {
        return (XML_ATTRIBUTE_REQUIRED as c_int, None, kw_len);
    }
    if kw.eq_ignore_ascii_case(b"#IMPLIED") {
        return (XML_ATTRIBUTE_IMPLIED as c_int, None, kw_len);
    }
    if kw.eq_ignore_ascii_case(b"#FIXED") {
        let tail = &rest[kw_len..];
        let ws_skip = tail.len() - tail.trim_ascii_start().len();
        let after = &tail[ws_skip..];
        match read_quoted(after) {
            Some(v) => (
                XML_ATTRIBUTE_FIXED as c_int,
                Some(v),
                kw_len + ws_skip + v.len() + 2,
            ),
            None => (XML_ATTRIBUTE_FIXED as c_int, None, kw_len),
        }
    } else {
        // Bare quoted default value (no #keyword): upstream xmlParseDefaultDecl
        // returns XML_ATTRIBUTE_NONE with the value — NOT #IMPLIED (which is
        // only produced by the explicit keyword). The DTD dumper prints
        // ` CDATA "default title"`, never ` #IMPLIED "default title"`.
        match read_quoted(rest) {
            Some(v) => (XML_ATTRIBUTE_NONE as c_int, Some(v), v.len() + 2),
            None => (XML_ATTRIBUTE_NONE as c_int, None, kw_len),
        }
    }
}

/// Build a NUL-terminated C string from a byte slice (helper for DTD parsing).
fn vec_to_cstr_null_helper(s: &[u8]) -> *mut c_char {
    let mut v = s.to_vec();
    v.push(0);
    let boxed = v.into_boxed_slice();
    Box::into_raw(boxed) as *mut c_char
}

/// Percent-decode a URI path component (`%2F` -> `/`, `%20` -> space, ...);
/// malformed escapes are kept verbatim.
fn percent_decode_uri(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0usize;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let h = hex_val(b[i + 1]);
            let l = hex_val(b[i + 2]);
            if let (Some(h), Some(l)) = (h, l) {
                out.push((h << 4) | l);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Snprintf a content-model expression (upstream valid.c
/// `xmlSnprintfElementContent`): `(title , author)`, `(#PCDATA | a | b)*`, etc.
///
/// # SAFETY
///
/// - `content` must be NULL or a valid `_xmlElementContent` tree whose
///   `type_`/`ocur`/`name`/`prefix`/`c1`/`c2` fields are valid for a walk.
unsafe fn snprintf_element_content(
    content: *mut crate::abi::structs::_xmlElementContent,
    englob: bool,
) -> String {
    use crate::abi::types::xmlElementContentOccur::*;
    use crate::abi::types::xmlElementContentType::*;
    let mut out = String::new();
    unsafe fn walk(
        out: &mut String,
        content: *mut crate::abi::structs::_xmlElementContent,
        englob: bool,
    ) {
        if content.is_null() {
            return;
        }
        let c = unsafe { &*content };
        if englob {
            out.push('(');
        }
        match c.type_ {
            t if t == XML_ELEMENT_CONTENT_PCDATA as c_int => {
                out.push_str("#PCDATA");
            }
            t if t == XML_ELEMENT_CONTENT_ELEMENT as c_int => {
                if !c.prefix.is_null() {
                    out.push_str(&crate::xml::string::xmlstr_to_string(c.prefix));
                    out.push(':');
                }
                if !c.name.is_null() {
                    out.push_str(&crate::xml::string::xmlstr_to_string(c.name));
                }
            }
            t if t == XML_ELEMENT_CONTENT_SEQ as c_int || t == XML_ELEMENT_CONTENT_OR as c_int => {
                let c1_is_group = !c.c1.is_null()
                    && ((*(c.c1)).type_ == XML_ELEMENT_CONTENT_SEQ as c_int
                        || (*(c.c1)).type_ == XML_ELEMENT_CONTENT_OR as c_int);
                walk(out, c.c1, c1_is_group);
                out.push_str(if c.type_ == XML_ELEMENT_CONTENT_SEQ as c_int {
                    " , "
                } else {
                    " | "
                });
                let c2_is_group = !c.c2.is_null()
                    && (((*(c.c2)).type_ == XML_ELEMENT_CONTENT_OR as c_int
                        || (*(c.c2)).type_ == XML_ELEMENT_CONTENT_SEQ as c_int)
                        || ((*(c.c2)).ocur != XML_ELEMENT_CONTENT_ONCE as c_int
                            && (*(c.c2)).type_ != XML_ELEMENT_CONTENT_ELEMENT as c_int));
                walk(out, c.c2, c2_is_group);
            }
            _ => {}
        }
        if englob {
            out.push(')');
        }
        match c.ocur {
            t if t == XML_ELEMENT_CONTENT_OPT as c_int => out.push('?'),
            t if t == XML_ELEMENT_CONTENT_MULT as c_int => out.push('*'),
            t if t == XML_ELEMENT_CONTENT_PLUS as c_int => out.push('+'),
            _ => {}
        }
    }
    walk(&mut out, content, englob);
    out
}

/// Snprintf the "got" children list of an element (upstream valid.c
/// `xmlSnprintfElements`): blank text is skipped, elements print (qualified)
/// names, non-blank text/CDATA/entity references print `CDATA`, and every
/// printed token is followed by a space when a sibling follows.
///
/// # SAFETY
///
/// - `first` must be NULL or the first child of a valid node list whose
///   `next` links are valid; `name`/`ns`/`content` of the visited nodes must
///   be valid per node type.
unsafe fn snprintf_children_list(first: *mut crate::abi::structs::_xmlNode) -> String {
    let mut out = String::new();
    out.push('(');
    let mut cur = first;
    while !cur.is_null() {
        let c = unsafe { &*cur };
        match c.type_ {
            t if t == crate::abi::types::xmlElementType::XML_ELEMENT_NODE as c_int => {
                if !c.ns.is_null() && !(*(c.ns)).prefix.is_null() {
                    out.push_str(&crate::xml::string::xmlstr_to_string((*(c.ns)).prefix));
                    out.push(':');
                }
                if !c.name.is_null() {
                    out.push_str(&crate::xml::string::xmlstr_to_string(c.name));
                }
                if !c.next.is_null() {
                    out.push(' ');
                }
            }
            t if t == crate::abi::types::xmlElementType::XML_TEXT_NODE as c_int => {
                let blank = if c.content.is_null() {
                    true
                } else {
                    let s = crate::xml::string::xmlstr_to_string(c.content);
                    s.trim().is_empty()
                };
                if !blank {
                    out.push_str("CDATA");
                    if !c.next.is_null() {
                        out.push(' ');
                    }
                }
            }
            t if t == crate::abi::types::xmlElementType::XML_CDATA_SECTION_NODE as c_int
                || t == crate::abi::types::xmlElementType::XML_ENTITY_REF_NODE as c_int =>
            {
                out.push_str("CDATA");
                if !c.next.is_null() {
                    out.push(' ');
                }
            }
            _ => {}
        }
        cur = c.next;
    }
    out.push(')');
    out
}
