//! XML parser state machine — document-level parsing with SAX dispatch (§85 Phase 3).
//!
//! This module implements the core parser state machine that orchestrates
//! tokenization and SAX event dispatch. It handles document structure:
//! prolog (XML declaration, DTD, misc), content (root element with children),
//! and epilog (misc after root). SAX events are dispatched through the
//! `SaxDispatcher` to the handlers registered in the parser context.

use crate::abi::callbacks::*;
use crate::abi::constants::*;
use crate::abi::structs::*;
use crate::abi::types::xmlElementContentOccur::*;
use crate::abi::types::xmlElementContentType::*;
use crate::abi::types::xmlElementTypeVal::*;
use crate::abi::types::xmlEntityType::*;
use crate::abi::types::*;
use crate::xml::parser::input::{InputBuffer, InputStack};
use crate::xml::parser::tokenizer::{XmlToken, XmlTokenizer};
use crate::xml::sax::dispatch::SaxDispatcher;
use core::ptr;
use std::os::raw::{c_char, c_int, c_void};

// ─────────────────────────────────────────────────────────────────────────────
// Parser constants (parser states)
// ─────────────────────────────────────────────────────────────────────────────

const XML_PARSER_START: c_int = 0;
const XML_PARSER_MISC: c_int = 1;
const XML_PARSER_PROLOG: c_int = 2;
const XML_PARSER_CONTENT: c_int = 3;
const XML_PARSER_CDATA_SECTION: c_int = 4;
const XML_PARSER_ENTITY_REF: c_int = 5;

/// Whether a SAX `error` slot holds one of the candidate's legacy default
/// handlers (upstream error.c treats these as "do not invoke directly —
/// format via `xmlFormatError`" in `xmlVRaiseError`).
fn is_legacy_error_handler(cb: errorSAXFunc) -> bool {
    let ptr = cb as usize;
    ptr == crate::xml::errors::xmlParserError as errorSAXFunc as usize
        || ptr == crate::xml::sax::default::default_sax_handler::error as errorSAXFunc as usize
}

/// Whether a SAX `warning` slot holds the candidate's legacy default handler.
fn is_legacy_warning_handler(cb: warningSAXFunc) -> bool {
    let ptr = cb as usize;
    ptr == crate::xml::errors::xmlParserWarning as warningSAXFunc as usize
        || ptr == crate::xml::sax::default::default_sax_handler::warning as warningSAXFunc as usize
}
const XML_PARSER_ENTITY_VALUE: c_int = 6;
const XML_PARSER_ATTRIBUTE_VALUE: c_int = 7;
const XML_PARSER_DTD: c_int = 8;
const XML_PARSER_EOF: c_int = 9;
const XML_PARSER_EPILOG: c_int = 10;
const XML_PARSER_PI: c_int = 11;
const XML_PARSER_IGNORE: c_int = 12;
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
    pub fn tokenizer(&mut self) -> &mut XmlTokenizer {
        &mut self.tokenizer
    }

    /// Get a shared reference to the parser context.
    ///
    /// # Safety
    ///
    /// The context pointer must remain valid.
    pub unsafe fn ctxt(&self) -> &_xmlParserCtxt {
        unsafe { &*self.ctxt }
    }

    /// Get a mutable reference to the parser context.
    ///
    /// # Safety
    ///
    /// The context pointer must remain valid.
    pub unsafe fn ctxt_mut(&mut self) -> &mut _xmlParserCtxt {
        unsafe { &mut *self.ctxt }
    }

    /// Get the raw parser context pointer.
    pub fn ctxt_raw(&self) -> *mut _xmlParserCtxt {
        self.ctxt
    }

    /// Return whether the parser is in recovery mode.
    fn is_recovery(&self) -> bool {
        (self.options & XML_PARSE_RECOVER) != 0
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

        // Parse prolog (XML declaration, DTD, misc)
        if self.parse_prolog().is_err() {
            if !self.is_recovery() {
                self.sax_end_document();
                return -1;
            }
        }

        // Parse content (root element)
        unsafe {
            (*self.ctxt).instate = XML_PARSER_CONTENT;
        }
        if self.parse_content().is_err() {
            if !self.is_recovery() {
                self.sax_end_document();
                return -1;
            }
        }

        // Parse epilog (misc after root)
        unsafe {
            (*self.ctxt).instate = XML_PARSER_EPILOG;
        }
        if self.parse_epilog().is_err() {
            if !self.is_recovery() {
                self.sax_end_document();
                return -1;
            }
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
        0
    }

    /// Parse a chunk of input (push parser mode).
    ///
    /// `chunk` contains the next bytes of input. If `terminate` is true,
    /// this is the final chunk and the document should be completed.
    ///
    /// Returns 0 on success, -1 on error.
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

    /// Parse the prolog: XML declaration, DTD, and misc (comments, PIs, whitespace).
    fn parse_prolog(&mut self) -> Result<(), ()> {
        loop {
            let token = self.tokenizer.next_token();
            match token {
                XmlToken::Eof => {
                    // Empty document or end of prolog
                    return Ok(());
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
                XmlToken::ProcessingInstruction { target, data } => {
                    self.sax_pi(&target, &data);
                }
                XmlToken::Characters(data) => {
                    // Whitespace-only in prolog is allowed; non-whitespace is an error
                    if data.iter().any(|&b| !b.is_ascii_whitespace()) {
                        self.set_error(
                            XML_ERR_DOCUMENT_START,
                            "Non-whitespace characters in prolog",
                        );
                        if !self.is_recovery() {
                            return Err(());
                        }
                    }
                }
                XmlToken::StartTag { .. } => {
                    // Start of root element — prolog is complete
                    self.push_token_back(&token);
                    return Ok(());
                }
                _ => {
                    // Unexpected token in prolog
                    self.set_error(XML_ERR_DOCUMENT_START, "Unexpected token in prolog");
                    if !self.is_recovery() {
                        return Err(());
                    }
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
            crate::abi::allocator::xmlFree(name_cstr as *mut c_void);
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
                let (pub_id, sys_id) = split_two_quoted(after_kw);
                let external_type = if is_param {
                    XML_EXTERNAL_PARAMETER_ENTITY as c_int
                } else {
                    XML_EXTERNAL_GENERAL_PARSED_ENTITY as c_int
                };
                let pub_c = pub_id
                    .map(|s| Self::vec_to_cstr_null(s))
                    .unwrap_or(ptr::null());
                let sys_c = sys_id
                    .map(|s| Self::vec_to_cstr_null(s))
                    .unwrap_or(ptr::null());
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
                    crate::abi::allocator::xmlFree(pub_c as *mut c_void);
                }
                if !sys_c.is_null() {
                    crate::abi::allocator::xmlFree(sys_c as *mut c_void);
                }
            } else {
                // Internal: quoted value.
                let value = read_quoted(tail);
                let v = value
                    .map(|s| Self::vec_to_cstr_null(s))
                    .unwrap_or(ptr::null());
                crate::xml::entities::add_entity(
                    dtd,
                    name_cstr,
                    etype,
                    ptr::null(),
                    ptr::null(),
                    v,
                );
                if !v.is_null() {
                    crate::abi::allocator::xmlFree(v as *mut c_void);
                }
            }
            crate::abi::allocator::xmlFree(name_cstr as *mut c_void);
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
            let elem_decl = crate::xml::dtd::get_element_decl(dtd, elem_cstr);
            if elem_decl.is_null() {
                crate::abi::allocator::xmlFree(elem_cstr as *mut c_void);
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
                    crate::abi::allocator::xmlFree(dv as *mut c_void);
                }
                crate::abi::allocator::xmlFree(attr_cstr as *mut c_void);
            }
            crate::abi::allocator::xmlFree(elem_cstr as *mut c_void);
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
            let pub_c = pub_id
                .map(|s| Self::vec_to_cstr_null(s))
                .unwrap_or(ptr::null());
            let sys_c = sys_id
                .map(|s| Self::vec_to_cstr_null(s))
                .unwrap_or(ptr::null());
            crate::xml::dtd::add_notation_decl(dtd, name_cstr, pub_c, sys_c);
            if !pub_c.is_null() {
                crate::abi::allocator::xmlFree(pub_c as *mut c_void);
            }
            if !sys_c.is_null() {
                crate::abi::allocator::xmlFree(sys_c as *mut c_void);
            }
            crate::abi::allocator::xmlFree(name_cstr as *mut c_void);
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
            let ext_cstr = _ext_id
                .map(|s| Self::vec_to_cstr_null(s))
                .unwrap_or(ptr::null());
            let sys_cstr = _sys_id
                .map(|s| Self::vec_to_cstr_null(s))
                .unwrap_or(ptr::null());

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

    /// Parse element content.
    ///
    /// Loops until EOF or an end tag is found. Dispatches SAX events for
    /// character data, comments, PIs, CDATA sections, and nested elements.
    ///
    /// Returns `Ok(())` even if EOF is reached, as long as at least one
    /// element was parsed. This handles self-closing root elements like
    /// `<root/>` where EOF after the element is valid.
    fn parse_content(&mut self) -> Result<(), ()> {
        let mut has_content = false;
        loop {
            let token = self.tokenizer.next_token_raw();
            match token {
                XmlToken::Eof => {
                    // EOF at start of content (no root element) is an error.
                    // EOF after content (root element was parsed) is fine.
                    if !has_content {
                        self.set_error(XML_ERR_DOCUMENT_END, "Unexpected EOF in content");
                        return if self.is_recovery() { Ok(()) } else { Err(()) };
                    }
                    return Ok(());
                }
                XmlToken::StartTag {
                    name,
                    attributes,
                    empty,
                } => {
                    has_content = true;
                    self.parse_element(name, attributes, empty)?;
                }
                XmlToken::EndTag(name) => {
                    // End tag without matching start — push back so caller can handle it
                    self.push_token_back(&XmlToken::EndTag(name));
                    return Ok(());
                }
                XmlToken::Characters(data) => {
                    if !data.is_empty() {
                        if has_content {
                            // After the root element: whitespace is allowed but
                            // discarded (upstream libxml2 does not create a node
                            // for it); non-whitespace is extra content.
                            if data.iter().any(|&b| !b.is_ascii_whitespace()) {
                                self.set_error(
                                    XML_ERR_DOCUMENT_END,
                                    "Extra content at the end of the document",
                                );
                                if !self.is_recovery() {
                                    return Err(());
                                }
                            }
                        } else {
                            self.sax_characters(&data);
                        }
                    }
                }
                XmlToken::Comment(data) => {
                    self.sax_comment(&data);
                }
                XmlToken::ProcessingInstruction { target, data } => {
                    self.sax_pi(&target, &data);
                }
                XmlToken::Cdata(data) => {
                    self.sax_cdata(&data);
                }
                XmlToken::Reference(data) => {
                    self.parse_reference(&data)?;
                }
                XmlToken::XmlDecl { .. } => {
                    // XML declaration in content is an error
                    self.set_error(XML_ERR_MISPLACED_XML_PI, "XML declaration in content");
                    if !self.is_recovery() {
                        return Err(());
                    }
                }
                XmlToken::DocType(_) => {
                    // DOCTYPE in content is an error
                    self.set_error(XML_ERR_DOCTYPE_NOT_FINISHED, "DOCTYPE in content");
                    if !self.is_recovery() {
                        return Err(());
                    }
                }
            }
        }
    }

    /// Parse a single element: start tag, content, and matching end tag.
    fn parse_element(
        &mut self,
        name: Vec<u8>,
        attributes: Vec<(Vec<u8>, Vec<u8>)>,
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
        let mut regular_attrs: Vec<(Option<Vec<u8>>, Vec<u8>, Vec<u8>)> = Vec::new();

        // UPSTREAM-PARITY: attribute values are parsed with
        // xmlParseAttValueInternal, which substitutes character references
        // always, predefined entities always, and declared entities when
        // XML_PARSE_NOENT is set. The tokenizer scans the raw value, so the
        // substitution happens here.
        let mut new_attributes: Vec<(Vec<u8>, Vec<u8>)> = Vec::with_capacity(attributes.len());
        for (n, v) in attributes.into_iter() {
            let value = self.substitute_refs(&v)?;
            new_attributes.push((n, value));
        }
        let attributes = new_attributes;

        for (attr_name, attr_value) in &attributes {
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
                    regular_attrs.push((Some(prefix), localname, attr_value.clone()));
                } else {
                    regular_attrs.push((None, attr_name.clone(), attr_value.clone()));
                }
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
                let next = self.tokenizer.next_token_raw();
                match next {
                    XmlToken::EndTag(end_name) => {
                        // Check for matching end tag
                        if end_name != name {
                            self.set_error(
                                XML_ERR_TAG_NAME_MISMATCH,
                                &format!(
                                    "Opening and ending tag mismatch: {} line {} and {}",
                                    String::from_utf8_lossy(&name),
                                    open_line,
                                    String::from_utf8_lossy(&end_name)
                                ),
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
                        empty: child_empty,
                    } => {
                        self.parse_element(child_name, child_attrs, child_empty)?;
                    }
                    XmlToken::Characters(data) => {
                        if !data.is_empty() {
                            self.sax_characters(&data);
                        }
                    }
                    XmlToken::Comment(data) => {
                        self.sax_comment(&data);
                    }
                    XmlToken::ProcessingInstruction { target, data } => {
                        self.sax_pi(&target, &data);
                    }
                    XmlToken::Cdata(data) => {
                        self.sax_cdata(&data);
                    }
                    XmlToken::Reference(data) => {
                        self.parse_reference(&data)?;
                    }
                    XmlToken::Eof => {
                        // UPSTREAM-PARITY: in recovery mode an unclosed
                        // element at EOF is closed silently; without recovery
                        // it is a hard error.
                        if !self.is_recovery() {
                            self.set_error(XML_ERR_DOCUMENT_END, "Unclosed element tag");
                            self.pop_name();
                            return Err(());
                        }
                        break;
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
    fn substitute_refs(&mut self, value: &[u8]) -> Result<Vec<u8>, ()> {
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
                            u32::from_str_radix(&String::from_utf8_lossy(num), 10).ok()
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
                                        let content =
                                            unsafe { (*(ent as *mut _xmlEntity)).content };
                                        if !content.is_null() && unsafe { (*content) == b'<' } {
                                            self.set_error(
                                                crate::abi::types::XML_ERR_LT_IN_ATTRIBUTE,
                                                &format!(
                                                    "'<' in entity '{}' is not allowed in attributes \
                                                     values",
                                                    String::from_utf8_lossy(inner)
                                                ),
                                            );
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
                                                    crate::abi::allocator::xmlFree(
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
    fn parse_reference(&mut self, data: &[u8]) -> Result<(), ()> {
        // data includes the '&' and ';' delimiters, e.g. "&amp;", "&#60;", "&#x3C;"
        if data.len() < 3 {
            self.set_error(XML_ERR_ENTITYREF_NO_NAME, "Empty entity reference");
            return if self.is_recovery() { Ok(()) } else { Err(()) };
        }

        let inner = &data[1..data.len() - 1]; // Strip '&' and ';'

        if inner.is_empty() {
            self.set_error(XML_ERR_ENTITYREF_NO_NAME, "Empty entity reference");
            return if self.is_recovery() { Ok(()) } else { Err(()) };
        }

        // Character reference
        if inner.starts_with(b"#") {
            let num_part = &inner[1..];
            let codepoint = if num_part.starts_with(b"x") || num_part.starts_with(b"X") {
                // Hex character reference: &#xAB;
                u32::from_str_radix(&String::from_utf8_lossy(&num_part[1..]), 16).map_err(|_| {
                    self.set_error(
                        XML_ERR_INVALID_HEX_CHARREF,
                        "Invalid hex character reference",
                    );
                })?
            } else {
                // Decimal character reference: &#123;
                u32::from_str_radix(&String::from_utf8_lossy(num_part), 10).map_err(|_| {
                    self.set_error(
                        XML_ERR_INVALID_DEC_CHARREF,
                        "Invalid decimal character reference",
                    );
                })?
            };

            // Validate the codepoint
            if !is_valid_xml_char(codepoint) {
                self.set_error(XML_ERR_INVALID_CHAR, "Invalid XML character reference");
                return if self.is_recovery() { Ok(()) } else { Err(()) };
            }

            // Convert codepoint to UTF-8 and dispatch as character data
            if let Some(ch) = char::from_u32(codepoint) {
                let mut utf8_buf = [0u8; 4];
                let encoded = ch.encode_utf8(&mut utf8_buf);
                self.sax_characters(encoded.as_bytes());
            }

            return Ok(());
        }

        // Entity reference: &name;
        // Check for predefined XML entities
        let replacement = match inner {
            b"amp" => Some(b"&" as &[u8]),
            b"lt" => Some(b"<" as &[u8]),
            b"gt" => Some(b">" as &[u8]),
            b"quot" => Some(b"\"" as &[u8]),
            b"apos" => Some(b"'" as &[u8]),
            _ => None,
        };

        if let Some(replacement) = replacement {
            // UPSTREAM-PARITY: predefined entities are substituted into the
            // text stream; no ENTITY_REF node is created.
            self.sax_characters(replacement);
        } else if (self.options & XML_PARSE_NOENT) != 0 {
            // Entity substitution requested: resolve the entity and re-parse
            // its content (upstream xmlParseReference pushes the entity
            // content as a new input, which handles nested references and
            // markup inside entity values).
            let entity = if !self.is_sax_disabled() {
                let name_cstr = Self::vec_to_cstr_null(inner);
                unsafe {
                    let sax = &*(*self.ctxt).sax;
                    let ctx = (*self.ctxt).userData;
                    SaxDispatcher::get_entity(sax, ctx, name_cstr)
                }
            } else {
                ptr::null_mut()
            };

            if entity.is_null() {
                // Undeclared entity
                self.set_warning(&format!(
                    "Undeclared entity: {}",
                    String::from_utf8_lossy(inner)
                ));
            } else if !unsafe { (*entity).content }.is_null() {
                // Re-parse the entity content from a pushed input.
                let content = unsafe { (*entity).content };
                let len = unsafe { libc::strlen(content as *const c_char) };
                let bytes = unsafe { core::slice::from_raw_parts(content, len) };
                let buf = InputBuffer::from_memory(bytes, None);
                self.tokenizer.push_input(buf);
            } else {
                // External entity with no in-memory content: report the
                // reference and leave it unresolved.
                if !self.is_sax_disabled() {
                    let name_cstr = Self::vec_to_cstr_null(inner);
                    unsafe {
                        let sax = &*(*self.ctxt).sax;
                        let ctx = (*self.ctxt).userData;
                        SaxDispatcher::reference(sax, ctx, name_cstr);
                    }
                }
            }
        } else {
            // Not substituting: dispatch the reference event for the application to handle
            if !self.is_sax_disabled() {
                let name_cstr = Self::vec_to_cstr_null(inner);
                unsafe {
                    let sax = &*(*self.ctxt).sax;
                    let ctx = (*self.ctxt).userData;
                    SaxDispatcher::reference(sax, ctx, name_cstr);
                }
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
                XmlToken::ProcessingInstruction { target, data } => {
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
        attrs: &[(Option<Vec<u8>>, Vec<u8>, Vec<u8>)],
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
        // Resolve the prefix against this element's namespace declarations.
        let uri: Option<Vec<u8>> = match &prefix_opt {
            Some(p) if p == b"xml" => Some(b"http://www.w3.org/XML/1998/namespace".to_vec()),
            Some(p) => ns_decls
                .iter()
                .find(|(dp, _)| dp == p)
                .map(|(_, u)| u.clone()),
            None => ns_decls
                .iter()
                .find(|(dp, _)| dp.is_empty())
                .map(|(_, u)| u.clone()),
        };

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
                let uri_cstr = Self::vec_to_cstr_null(uri);
                ns_vec.push(prefix_cstr);
                ns_vec.push(uri_cstr);
            }

            // Build the attribute array for SAX2: [localname1, prefix1, uri1, valueStart1, valueEnd1, ...]
            let mut attr_vec: Vec<*const xmlChar> = Vec::with_capacity(attrs.len() * 5);
            for (prefix, localname, value) in attrs {
                let local_cstr = Self::vec_to_cstr_null(localname);
                let prefix_cstr = prefix
                    .as_ref()
                    .map(|p| Self::vec_to_cstr_null(p))
                    .unwrap_or(ptr::null());
                // Resolve the attribute prefix against the namespace declarations.
                let uri_cstr = match prefix {
                    Some(p) if p == b"xml" => {
                        Self::vec_to_cstr_null(b"http://www.w3.org/XML/1998/namespace")
                    }
                    Some(p) => ns_decls
                        .iter()
                        .find(|(dp, _)| dp == p)
                        .map(|(_, u)| Self::vec_to_cstr_null(u))
                        .unwrap_or(ptr::null()),
                    _ => ptr::null(),
                };
                let value_cstr = Self::vec_to_cstr_null(value);
                attr_vec.push(local_cstr);
                attr_vec.push(prefix_cstr);
                attr_vec.push(uri_cstr);
                attr_vec.push(value_cstr);
                // valueEnd is the end of the value string — we pass null to indicate
                // the value is null-terminated
                attr_vec.push(ptr::null());
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
                        b"xml\0".as_ptr() as *const xmlChar,
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
                b"http://www.w3.org/XML/1998/namespace\0".as_ptr() as *const xmlChar,
            );
            (*new_ns).prefix = crate::xml::string::xml_strdup(b"xml\0".as_ptr() as *const xmlChar);
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

    /// Set an error on the parser context.
    fn set_error(&mut self, code: c_int, msg: &str) {
        unsafe {
            (*self.ctxt).errNo = code;
            (*self.ctxt).wellFormed = 0;
            (*self.ctxt).nbErrors = (*self.ctxt).nbErrors.wrapping_add(1);

            // UPSTREAM-PARITY (error.c `xmlCtxtVErr` / parserInternals.c):
            // unless XML_PARSE_NOERROR is set, the error is delivered
            // through the configured channel — a custom SAX error slot
            // receives the message directly, otherwise the error is raised
            // and streamed through the generic handler (`xmlFormatError`
            // fragment sequence). The source window is built from the
            // tokenizer position, mirroring the legacy `file:line: parser
            // error : msg` + caret stderr report.
            if (*self.ctxt).options & XML_PARSE_NOERROR == 0 {
                let msg_cstr = std::ffi::CString::new(msg).unwrap_or_default();
                let window = self.build_error_window();
                let delivery = self.error_delivery();
                let (line, _col, _pos) = self.tokenizer.current_pos();
                let fname = self
                    .tokenizer
                    .input()
                    .current_ref()
                    .filename()
                    .map(|f| std::ffi::CString::new(f).unwrap_or_default());
                let file_ptr = fname.as_ref().map(|c| c.as_ptr()).unwrap_or(ptr::null());
                crate::xml::errors::raise_error_streamed(
                    self.ctxt as *mut c_void,
                    XML_FROM_PARSER,
                    code,
                    xmlErrorLevel::XML_ERR_ERROR as c_int,
                    file_ptr,
                    line as c_int,
                    msg_cstr.as_ptr(),
                    window.as_ref().map(|(w, c)| (w.as_slice(), *c)),
                    delivery,
                );
            }
        }
    }

    /// Set a warning on the parser context.
    fn set_warning(&mut self, msg: &str) {
        unsafe {
            (*self.ctxt).nbWarnings = (*self.ctxt).nbWarnings.wrapping_add(1);

            // UPSTREAM-PARITY: warnings are suppressed by XML_PARSE_NOWARNING.
            if (*self.ctxt).options & XML_PARSE_NOWARNING == 0 {
                let msg_cstr = std::ffi::CString::new(msg).unwrap_or_default();
                let window = self.build_error_window();
                let delivery = self.warning_delivery();
                let (line, _col, _pos) = self.tokenizer.current_pos();
                let fname = self
                    .tokenizer
                    .input()
                    .current_ref()
                    .filename()
                    .map(|f| std::ffi::CString::new(f).unwrap_or_default());
                let file_ptr = fname.as_ref().map(|c| c.as_ptr()).unwrap_or(ptr::null());
                crate::xml::errors::raise_error_streamed(
                    self.ctxt as *mut c_void,
                    XML_FROM_PARSER,
                    0,
                    xmlErrorLevel::XML_ERR_WARNING as c_int,
                    file_ptr,
                    line as c_int,
                    msg_cstr.as_ptr(),
                    window.as_ref().map(|(w, c)| (w.as_slice(), *c)),
                    delivery,
                );
            }
        }
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

    /// Select the generic delivery for a parser warning (see
    /// `error_delivery`; the `warning` SAX slot).
    fn warning_delivery(&self) -> crate::xml::errors::GenericDelivery {
        use crate::xml::errors::GenericDelivery;
        unsafe {
            let sax = &*(*self.ctxt).sax;
            match sax.warning {
                None => GenericDelivery::None,
                Some(cb) if is_legacy_warning_handler(cb) => GenericDelivery::Stream,
                Some(cb) => GenericDelivery::Custom(cb, (*self.ctxt).userData),
            }
        }
    }

    /// Build the source window (current input line) and 0-based caret column
    /// for the generic error report — the legacy `xmlParserPrintFileContext`
    /// equivalent.
    fn build_error_window(&mut self) -> Option<(Vec<u8>, usize)> {
        let consumed = self.tokenizer.input().current_ref().consumed().to_vec();
        let remaining = self.tokenizer.input().current_ref().remaining().to_vec();
        let line_start = consumed
            .iter()
            .rposition(|&b| b == b'\n')
            .map(|i| i + 1)
            .unwrap_or(0);
        let line_end = remaining
            .iter()
            .position(|&b| b == b'\n')
            .unwrap_or(remaining.len());
        let mut line_bytes = Vec::with_capacity(consumed.len() - line_start + line_end);
        line_bytes.extend_from_slice(&consumed[line_start..]);
        line_bytes.extend_from_slice(&remaining[..line_end]);
        let (_line, col, _pos) = self.tokenizer.current_pos();
        Some((line_bytes, col.saturating_sub(1) as usize))
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
fn is_valid_xml_char(codepoint: u32) -> bool {
    match codepoint {
        0x09 | 0x0A | 0x0D => true,
        0x20..=0xD7FF => true,
        0xE000..=0xFFFD => true,
        0x10000..=0x10FFFF => true,
        _ => false,
    }
}

/// Helper trait to trim ASCII whitespace from byte slices.
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
fn skip_ws(s: &[u8], idx: &mut usize) {
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
    let mut rest = s;
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
            crate::abi::allocator::xmlFree(tmp as *mut c_void);
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
    let mut rest = s;
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
