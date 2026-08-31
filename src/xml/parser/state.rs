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
//! `cargo test --lib` (1135+ tests). Receipts under courts/receipts/phase-11.
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

const XML_PARSER_START: c_int = 0;
const XML_PARSER_MISC: c_int = 1;
const XML_PARSER_PROLOG: c_int = 2;
const XML_PARSER_CONTENT: c_int = 3;
#[allow(dead_code)]
const XML_PARSER_CDATA_SECTION: c_int = 4;
#[allow(dead_code)]
const XML_PARSER_ENTITY_REF: c_int = 5;

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
fn is_legacy_error_handler(cb: errorSAXFunc) -> bool {
    let ptr = cb as usize;
    ptr == crate::xml::errors::xmlParserError as errorSAXFunc as usize
        || ptr == crate::xml::sax::default::default_sax_handler::error as errorSAXFunc as usize
}

/// Whether a SAX `warning` slot holds the candidate's legacy default handler.
#[allow(dead_code)]
fn is_legacy_warning_handler(cb: warningSAXFunc) -> bool {
    let ptr = cb as usize;
    ptr == crate::xml::errors::xmlParserWarning as warningSAXFunc as usize
        || ptr == crate::xml::sax::default::default_sax_handler::warning as warningSAXFunc as usize
}
#[allow(dead_code)]
const XML_PARSER_ENTITY_VALUE: c_int = 6;
#[allow(dead_code)]
const XML_PARSER_ATTRIBUTE_VALUE: c_int = 7;
const XML_PARSER_DTD: c_int = 8;
const XML_PARSER_EOF: c_int = 9;
const XML_PARSER_EPILOG: c_int = 10;
#[allow(dead_code)]
const XML_PARSER_PI: c_int = 11;
#[allow(dead_code)]
const XML_PARSER_IGNORE: c_int = 12;
#[allow(dead_code)]
const XML_PARSER_COMMENT: c_int = 13;
const XML_PARSER_XML_DECL: c_int = 14;

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
}

// ─── Construction and accessors ─────────────────────────────────────────────

impl XmlParser {
    /// Create a new parser from an input stack and parser context.
    ///
    /// The tokenizer takes ownership of the input stack.
    ///
    /// # Safety
    ///
    /// `ctxt` must be a valid, properly initialized `_xmlParserCtxt`.
    pub unsafe fn new(input: InputStack, ctxt: *mut _xmlParserCtxt) -> Self {
        let options = unsafe { (*ctxt).options };
        let initialized = unsafe { (*(*ctxt).sax).initialized };
        let sax1 = initialized != crate::abi::types::XML_SAX2_MAGIC as u32;

        // Set initial parser state
        unsafe {
            (*ctxt).instate = XML_PARSER_START;
            (*ctxt).wellFormed = 1;
            (*ctxt).errNo = XML_ERR_OK;
            (*ctxt).nbErrors = 0;
            (*ctxt).nbWarnings = 0;
        }

        XmlParser {
            tokenizer: XmlTokenizer::new(input),
            ctxt,
            options,
            sax1,
        }
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
    fn is_pedantic(&self) -> bool {
        unsafe { (*self.ctxt).pedantic != 0 }
    }

    /// Return whether SAX dispatch is currently disabled.
    fn is_sax_disabled(&self) -> bool {
        unsafe { (*self.ctxt).disableSAX != 0 }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Main parsing entry points
    // ─────────────────────────────────────────────────────────────────────────

    /// Parse a complete XML document.
    ///
    /// Returns 0 on success, -1 on error.
    pub fn parse_document(&mut self) -> c_int {
        // Fire startDocument
        self.sax_start_document();

        // Update parser state
        unsafe {
            (*self.ctxt).instate = XML_PARSER_MISC;
        }

        // UPSTREAM-PARITY (xmlParseDocument): an empty input is reported as
        // "Document is empty" before anything else.
        if self.tokenizer.is_input_empty() {
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
            self.sax_end_document();
            return -1;
        }

        // UPSTREAM-PARITY: a catastrophic stop ends the parse here.
        if unsafe { (*self.ctxt).disableSAX } == 2 {
            self.sax_end_document();
            return -1;
        }

        // Parse epilog (misc after root)
        unsafe {
            (*self.ctxt).instate = XML_PARSER_EPILOG;
        }
        if self.parse_epilog().is_err() && !self.is_recovery() {
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
                    // while wellFormed).
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
                XmlToken::DocType(content) => {
                    self.parse_dtd(&content)?;
                    unsafe {
                        (*self.ctxt).instate = XML_PARSER_PROLOG;
                    }
                }
                XmlToken::Comment(data) => {
                    self.sax_comment(&data);
                }
                XmlToken::ProcessingInstruction { target, data, .. } => {
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

        // Check for internal subset (content between [...])
        let has_internal = content.contains(&b'[');

        // Fire internalSubset SAX event
        if !self.is_sax_disabled() {
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

        // If there's an external ID and DTDLOAD is set, parse external subset
        if ext_id.is_some() && (self.options & XML_PARSE_DTDLOAD) != 0 {
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
    fn parse_internal_subset(&mut self, content: &[u8]) -> Result<(), ()> {
        // Mark that we're parsing the DTD
        unsafe {
            (*self.ctxt).inSubset = 1;
        }

        let dtd = unsafe {
            let doc = (*self.ctxt).myDoc;
            if doc.is_null() || (*doc).intSubset.is_null() {
                return Ok(());
            }
            (*doc).intSubset
        };

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

        let mut i = 0usize;
        while i < subset.len() {
            while i < subset.len() && subset[i].is_ascii_whitespace() {
                i += 1;
            }
            if i >= subset.len() {
                break;
            }
            if subset[i] != b'<' {
                // Parameter-entity reference (%name;) or stray text: skip.
                i += 1;
                continue;
            }
            if subset[i..].starts_with(b"<!--") {
                match find_subseq(&subset[i..], b"-->") {
                    Some(p) => i += p + 3,
                    None => break,
                }
                continue;
            }
            if subset[i..].starts_with(b"<?") {
                match find_subseq(&subset[i..], b"?>") {
                    Some(p) => i += p + 2,
                    None => break,
                }
                continue;
            }
            if subset[i..].starts_with(b"<!") {
                let rest = &subset[i + 2..];
                let Some(gt) = find_decl_end(rest) else {
                    break;
                };
                let decl = &rest[..gt];
                let kw_end = decl
                    .iter()
                    .position(|&b| b.is_ascii_whitespace())
                    .unwrap_or(decl.len());
                let kw = &decl[..kw_end];
                let args = &decl[kw_end..];
                if kw.eq_ignore_ascii_case(b"ELEMENT") {
                    Self::parse_element_decl(dtd, args);
                } else if kw.eq_ignore_ascii_case(b"ENTITY") {
                    Self::parse_entity_decl(dtd, args);
                } else if kw.eq_ignore_ascii_case(b"ATTLIST") {
                    Self::parse_attlist_decl(dtd, args);
                } else if kw.eq_ignore_ascii_case(b"NOTATION") {
                    Self::parse_notation_decl(dtd, args);
                }
                i += 2 + gt + 1;
                continue;
            }
            i += 1;
        }

        unsafe {
            (*self.ctxt).inSubset = 0;
        }

        Ok(())
    }

    /// Parse a `<!ELEMENT name contentmodel>` declaration.
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

    /// Parse a `<!ENTITY ...>` declaration (general or parameter).
    fn parse_entity_decl(dtd: *mut _xmlDtd, args: &[u8]) {
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
            // External: SYSTEM "uri" / PUBLIC "pub" "uri".
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
                let external_type = if is_param {
                    XML_EXTERNAL_PARAMETER_ENTITY as c_int
                } else {
                    XML_EXTERNAL_GENERAL_PARSED_ENTITY as c_int
                };
                let pub_c = pub_id.map(Self::vec_to_cstr_null).unwrap_or(ptr::null());
                let sys_c = sys_id.map(Self::vec_to_cstr_null).unwrap_or(ptr::null());
                crate::xml::entities::add_entity(
                    dtd,
                    name_cstr,
                    external_type,
                    pub_c,
                    sys_c,
                    ptr::null(),
                );
                crate::xml::entities::add_entity(
                    dtd,
                    name_cstr,
                    external_type,
                    pub_c,
                    sys_c,
                    ptr::null(),
                );
                if !pub_c.is_null() {
                    crate::abi::allocator::xmlFreeImpl(pub_c as *mut c_void);
                }
                if !sys_c.is_null() {
                    crate::abi::allocator::xmlFreeImpl(sys_c as *mut c_void);
                }
            } else {
                // Internal: quoted value.
                let value = read_quoted(tail);
                let v = value.map(Self::vec_to_cstr_null).unwrap_or(ptr::null());
                crate::xml::entities::add_entity(
                    dtd,
                    name_cstr,
                    etype,
                    ptr::null(),
                    ptr::null(),
                    v,
                );
                if !v.is_null() {
                    crate::abi::allocator::xmlFreeImpl(v as *mut c_void);
                }
            }
            crate::abi::allocator::xmlFreeImpl(name_cstr as *mut c_void);
        }
    }

    /// Parse a `<!ATTLIST elem attr type default ...>` declaration.
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
                let attr_cstr = Self::vec_to_cstr_null(attr_name);
                let dv = default_val
                    .as_ref()
                    .map(|s| Self::vec_to_cstr_null(s))
                    .unwrap_or(ptr::null());
                crate::xml::dtd::add_attribute_decl(
                    dtd, elem_decl, attr_cstr, atype, def, dv, tree,
                );
                if !dv.is_null() {
                    crate::abi::allocator::xmlFreeImpl(dv as *mut c_void);
                }
                crate::abi::allocator::xmlFreeImpl(attr_cstr as *mut c_void);
            }
            crate::abi::allocator::xmlFreeImpl(elem_cstr as *mut c_void);
        }
    }

    /// Parse a `<!NOTATION ...>` declaration.
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
            let (pub_id, sys_id) = split_two_quoted(tail);
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
    fn parse_external_subset(
        &mut self,
        _name: &[u8],
        _ext_id: Option<&[u8]>,
        _sys_id: Option<&[u8]>,
    ) -> Result<(), ()> {
        // TODO: Load and parse the external DTD subset
        // This requires URI resolution and I/O

        // Fire externalSubset SAX event
        if !self.is_sax_disabled() {
            let name_cstr = Self::vec_to_cstr_null(_name);
            let ext_cstr = _ext_id.map(Self::vec_to_cstr_null).unwrap_or(ptr::null());
            let sys_cstr = _sys_id.map(Self::vec_to_cstr_null).unwrap_or(ptr::null());

            unsafe {
                let sax = &*(*self.ctxt).sax;
                let ctx = (*self.ctxt).userData;
                SaxDispatcher::external_subset(sax, ctx, name_cstr, ext_cstr, sys_cstr);
            }
        }

        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Content parsing
    // ─────────────────────────────────────────────────────────────────────────

    /// Parse the root element (upstream `xmlParseDocument` element branch):
    /// the root start tag was pushed back by the prolog. After the root,
    /// trailing misc (comments/PIs/whitespace) is consumed and any
    /// remaining input raises "Extra content at the end of the document"
    /// (upstream `xmlParserCheckEOF` with XML_ERR_DOCUMENT_END).
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
                    // already raised; upstream's element parse failed.
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
    fn parse_misc_after_root(&mut self) -> Result<(), ()> {
        loop {
            let (token, start) = self.tokenizer.next_token_with_start();
            self.raise_pending_errors();
            match token {
                XmlToken::Eof => return Ok(()),
                XmlToken::Comment(data) => self.sax_comment(&data),
                XmlToken::ProcessingInstruction { target, data, .. } => {
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

        // Separate namespace declarations from regular attributes
        let mut ns_decls: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        let mut regular_attrs: Vec<(Option<Vec<u8>>, Vec<u8>, Vec<u8>, bool)> = Vec::new();
        // Parallel to `regular_attrs`: the byte offset just past each
        // attribute value's closing quote (upstream's position for
        // namespace diagnostics).
        let mut attr_pos: Vec<usize> = Vec::new();

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

        for (idx, (attr_name, attr_value, _had_ref)) in attributes.iter().enumerate() {
            // UPSTREAM-PARITY (parser.c xmlParseStartTag2): the default
            // namespace declaration warns when the URI is not absolute
            // (xmlns: URI %s is not absolute); the prefixed form warns only
            // in pedantic mode. The diagnostic is attributed to the position
            // just past the value's closing quote.
            if attr_name == b"xmlns" || attr_name.starts_with(b"xmlns:") {
                if attr_name == b"xmlns" {
                    if !attr_value.is_empty() && !has_uri_scheme(attr_value) {
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
                } else if self.is_pedantic()
                    && !attr_value.is_empty()
                    && !has_uri_scheme(attr_value)
                {
                    let prefix = &attr_name[b"xmlns:".len()..];
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
                    regular_attrs.push((Some(prefix), localname, attr_value.clone(), *_had_ref));
                    attr_pos.push(attr_end.get(idx).copied().unwrap_or(0));
                } else {
                    regular_attrs.push((None, attr_name.clone(), attr_value.clone(), *_had_ref));
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
        let prefix_declared = |prefix: &[u8]| -> bool {
            if prefix == b"xml" {
                return true;
            }
            if ns_decls.iter().any(|(dp, _)| dp == prefix) {
                return true;
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

        // Fire startElement SAX event
        // The default SAX handler manages nodeTab/nodeNr internally.
        self.sax_start_element(&name, &regular_attrs, &ns_decls);

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
                    XmlToken::EndTag { name: end_name, .. } => {
                        // Check for matching end tag
                        if end_name != name {
                            // UPSTREAM-PARITY (xmlParseElementEnd):
                            // XML_ERR_TAG_NAME_MISMATCH (76), FATAL,
                            // str1 = open name, str2 = close name,
                            // int1 = open line.
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
                            if !self.is_recovery() {
                                self.pop_name();
                                return Err(());
                            }
                            // In recovery mode, treat this as a stray end tag
                            self.sax_end_element(&end_name);
                            continue;
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
                            // Errors were already raised; the child element
                            // failed to parse (upstream xmlParseElementStart
                            // returned -1).
                            self.pop_name();
                            return Err(());
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
                    XmlToken::Comment(data) => {
                        self.sax_comment(&data);
                    }
                    XmlToken::ProcessingInstruction { target, data, .. } => {
                        self.sax_pi(&target, &data);
                    }
                    XmlToken::Cdata {
                        data, unterminated, ..
                    } => {
                        if unterminated {
                            // "Premature end of data in CDATA section" was
                            // already recorded; the CDATA content is dropped.
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
                        // line %d" (77) — but only while wellFormed (a prior
                        // fatal error already reported the real cause). In
                        // recovery mode the element is closed silently.
                        if self.is_recovery() {
                            break;
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
            let entity = if !self.is_sax_disabled() {
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
                // UPSTREAM-PARITY (xmlParseReference): an undeclared entity
                // in a document without external subset/PE refs is fatal:
                // "Entity '%s' not defined\n" (26), str1 = name, at the
                // current position (after the ';'). In recovery mode the
                // reference event still fires, building an entity-ref node
                // without a backing declaration.
                self.raise_error_now(
                    XML_FROM_PARSER,
                    XML_ERR_UNDECLARED_ENTITY,
                    xmlErrorLevel::XML_ERR_FATAL as c_int,
                    format!("Entity '{}' not defined\n", String::from_utf8_lossy(name)),
                    Some(name.to_vec()),
                    None,
                    None,
                    0,
                );
                if !self.is_recovery() {
                    return Err(());
                }
                if !self.is_sax_disabled() {
                    let name_cstr = Self::vec_to_cstr_null(name);
                    unsafe {
                        let sax = &*(*self.ctxt).sax;
                        let ctx = (*self.ctxt).userData;
                        SaxDispatcher::reference(sax, ctx, name_cstr);
                    }
                }
                return Ok(());
            }

            if (self.options & XML_PARSE_NOENT) != 0 {
                // UPSTREAM-PARITY (xmlParseReference): the entity content is
                // parsed into ent->children on first reference regardless of
                // the substitution mode.
                self.parse_entity_content(entity)?;
                // UPSTREAM-PARITY (xmlParseReference): "We also check for
                // amplification if entities aren't substituted. They might be
                // expanded later." — unconditional, no XML_PARSE_HUGE bypass.
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
            } else {
                // UPSTREAM-PARITY (xmlParseReference): the entity content is
                // parsed into ent->children on the first reference
                // (xmlCtxtParseEntity), before the reference event fires.
                self.parse_entity_content(entity)?;
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
                if !self.is_sax_disabled() {
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
    fn parse_entity_content(&mut self, ent: *mut _xmlEntity) -> Result<(), ()> {
        const XML_ENT_PARSED: c_int = 1 << 0;
        const XML_ENT_EXPANDING: c_int = 1 << 3;
        unsafe {
            if ent.is_null() || ((*ent).flags & XML_ENT_PARSED) != 0 {
                return Ok(());
            }
            if ((*ent).flags & XML_ENT_EXPANDING) != 0 {
                // UPSTREAM-PARITY (parser.c xmlCtxtParseEntity): a reference
                // to an entity that is already being expanded is a loop.
                self.raise_error_now(
                    XML_FROM_PARSER,
                    XML_ERR_ENTITY_LOOP,
                    xmlErrorLevel::XML_ERR_FATAL as c_int,
                    "Entity loop detected\n".to_string(),
                    None,
                    None,
                    None,
                    0,
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
                                let ent2 = if !self.is_sax_disabled() {
                                    let sax = &*(*self.ctxt).sax;
                                    let ctx = (*self.ctxt).userData;
                                    SaxDispatcher::get_entity(sax, ctx, name_cstr)
                                } else {
                                    ptr::null_mut()
                                };
                                if !ent2.is_null() {
                                    self.parse_entity_content(ent2)?;
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
                XmlToken::Comment(data) => {
                    self.sax_comment(&data);
                }
                XmlToken::ProcessingInstruction { target, data, .. } => {
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

    /// Fire `startDocument` SAX event.
    fn sax_start_document(&mut self) {
        if self.is_sax_disabled() {
            return;
        }
        unsafe {
            let sax = &*(*self.ctxt).sax;
            let ctx = (*self.ctxt).userData;
            SaxDispatcher::start_document(sax, ctx);
        }
    }

    /// Fire `endDocument` SAX event.
    fn sax_end_document(&mut self) {
        if self.is_sax_disabled() {
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
    fn sax_start_element(
        &mut self,
        _name: &[u8],
        attrs: &[(Option<Vec<u8>>, Vec<u8>, Vec<u8>, bool)],
        ns_decls: &[(Vec<u8>, Vec<u8>)],
    ) {
        if self.is_sax_disabled() {
            return;
        }
        self.sync_input_position();

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
            // Ancestor scope.
            unsafe {
                if !parent_node.is_null() {
                    let mut p = prefix.map(|p| p.to_vec()).unwrap_or_default();
                    p.push(0);
                    let ns = crate::xml::tree::search_ns(
                        my_doc,
                        parent_node,
                        p.as_ptr() as *const xmlChar,
                    );
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
            for (prefix, localname, value, had_ref) in attrs {
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
                let value_cstr = Self::vec_to_cstr_null(value);
                attr_vec.push(local_cstr);
                attr_vec.push(prefix_cstr);
                attr_vec.push(uri_cstr);
                attr_vec.push(value_cstr);
                // UPSTREAM-PARITY (parser.c xmlParseStartTag2 / SAX2.c
                // xmlSAX2AttributeNs): when the raw value contained an
                // entity/character reference the value was duplicated and is
                // null-terminated, so valueEnd points at the NUL byte
                // (*valueEnd == 0); otherwise valueEnd is NULL. The handler
                // uses this to decide the compact xmlSAX2TextNode path vs the
                // non-compact xmlNodeParseAttValue path (R-000120).
                if *had_ref {
                    attr_vec.push({ value_cstr.add(value.len()) } as *const xmlChar);
                } else {
                    attr_vec.push(ptr::null());
                }
            }

            let localname_cstr = Self::vec_to_cstr_null(&localname);
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
            let nb_attributes = attrs.len() as c_int;
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
                0, // nb_defaulted
                attributes_ptr,
            );
        }
    }

    /// Fire `endElement` SAX event.
    fn sax_end_element(&mut self, name: &[u8]) {
        if self.is_sax_disabled() {
            return;
        }
        unsafe {
            let sax = &*(*self.ctxt).sax;
            let ctx = (*self.ctxt).userData;
            let name_cstr = Self::vec_to_cstr_null(name);
            SaxDispatcher::end_element(
                sax,
                ctx,
                name_cstr,
                ptr::null(), // prefix
                ptr::null(), // URI
            );
        }
    }

    /// Fire `characters` SAX event.
    fn sax_characters(&mut self, data: &[u8]) {
        if self.is_sax_disabled() || data.is_empty() {
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
    fn sax_comment(&mut self, data: &[u8]) {
        if self.is_sax_disabled() {
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
    fn sax_pi(&mut self, target: &[u8], data: &[u8]) {
        if self.is_sax_disabled() {
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
    fn sax_cdata(&mut self, data: &[u8]) {
        if self.is_sax_disabled() || data.is_empty() {
            return;
        }
        self.sync_input_position();
        unsafe {
            let sax = &*(*self.ctxt).sax;
            let ctx = (*self.ctxt).userData;
            let data_cstr = Self::vec_to_cstr_null(data);
            SaxDispatcher::cdata_block(sax, ctx, data_cstr, data.len() as c_int);
        }
    }

    /// Mirror the tokenizer's current (line, col) into `ctxt->input` so the
    /// default SAX tree builder can stamp node line numbers (upstream parity:
    /// nodes carry the line of their construct).
    fn sync_input_position(&mut self) {
        let (line, col, _pos) = self.tokenizer.current_pos();
        unsafe {
            let ctxt = &mut *self.ctxt;
            if !ctxt.input.is_null() {
                (*ctxt.input).line = line as c_int;
                (*ctxt.input).col = col as c_int;
            }
        }
    }

    /// Materialize the xml namespace on the document (upstream keeps it on
    /// `doc->oldNs`; created once per document).
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
    fn push_name(&mut self, name: &[u8]) {
        let name_cstr = Self::vec_to_cstr_null(name);
        unsafe {
            // Store in the context's name field
            (*self.ctxt).name = name_cstr;
            (*self.ctxt).nameNr += 1;
        }
    }

    /// Pop an element name from the context's name stack.
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
                if level == xmlErrorLevel::XML_ERR_FATAL as c_int && !self.is_recovery() {
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

        if unsafe { (*self.ctxt).options } & XML_PARSE_NOERROR == 0 {
            let msg_cstr = std::ffi::CString::new(msg).unwrap_or_default();
            let s1 = str1.and_then(|s| std::ffi::CString::new(s).ok());
            let s2 = str2.and_then(|s| std::ffi::CString::new(s).ok());
            let s3 = str3.and_then(|s| std::ffi::CString::new(s).ok());
            let fname = self
                .tokenizer
                .input()
                .current_ref()
                .filename()
                .map(|f| std::ffi::CString::new(f).unwrap_or_default());
            let file_ptr = fname.as_ref().map(|c| c.as_ptr()).unwrap_or(ptr::null());
            let s1_ptr = s1.as_ref().map(|c| c.as_ptr()).unwrap_or(ptr::null());
            let s2_ptr = s2.as_ref().map(|c| c.as_ptr()).unwrap_or(ptr::null());
            let s3_ptr = s3.as_ref().map(|c| c.as_ptr()).unwrap_or(ptr::null());
            let window_ref = window.as_ref().map(|(w, caret)| (w.as_slice(), *caret));
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
    fn error_delivery(&self) -> crate::xml::errors::GenericDelivery {
        use crate::xml::errors::GenericDelivery;
        unsafe {
            let sax = &*(*self.ctxt).sax;
            match sax.error {
                None => GenericDelivery::None,
                Some(cb) if is_legacy_error_handler(cb) => GenericDelivery::Stream,
                Some(cb) => GenericDelivery::Custom(cb, (*self.ctxt).userData),
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Utility helpers
    // ─────────────────────────────────────────────────────────────────────────

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

/// Parse `( a | b | c )` into an enumeration chain.
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
/// Returns `(def, default_value, consumed_bytes)`.
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
        let after = trim_ascii(&rest[kw_len..]);
        match read_quoted(after) {
            Some(v) => (
                XML_ATTRIBUTE_FIXED as c_int,
                Some(v),
                kw_len + (after.len() - trim_ascii(after).len()) + after.len(),
            ),
            None => (XML_ATTRIBUTE_FIXED as c_int, None, kw_len),
        }
    } else {
        match read_quoted(rest) {
            Some(v) => (XML_ATTRIBUTE_IMPLIED as c_int, Some(v), rest.len()),
            None => (XML_ATTRIBUTE_IMPLIED as c_int, None, kw_len),
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
