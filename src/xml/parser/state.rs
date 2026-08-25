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
            let errno = unsafe { (*self.ctxt).errNo };
            eprintln!("parse_document: parse_prolog failed, errNo={}", errno);
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
            let errno = unsafe { (*self.ctxt).errNo };
            eprintln!("parse_document: parse_content failed, errNo={}", errno);
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
            let errno = unsafe { (*self.ctxt).errNo };
            eprintln!("parse_document: parse_epilog failed, errNo={}", errno);
            if !self.is_recovery() {
                self.sax_end_document();
                return -1;
            }
        }

        // Mark EOF
        unsafe {
            (*self.ctxt).instate = XML_PARSER_EOF;
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
    fn parse_internal_subset(&mut self, _content: &[u8]) -> Result<(), ()> {
        // In a full implementation, this would parse entity/attribute/element
        // declarations within the internal subset. For Phase 3, we acknowledge
        // that the internal subset was present but do not fully parse it.
        //
        // The tokenizer already scanned past the internal subset content
        // as part of the DOCTYPE token.

        // Mark that we're parsing the DTD
        unsafe {
            (*self.ctxt).inSubset = 1;
        }

        // TODO: Parse declarations within the internal subset
        // This involves scanning for <!ELEMENT, <!ATTLIST, <!ENTITY, <!NOTATION,
        // and <?PI and <!-- comments within the DTD.

        Ok(())
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
        // Push element name onto the context name stack
        self.push_name(&name);

        // Separate namespace declarations from regular attributes
        let mut ns_decls: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        let mut regular_attrs: Vec<(Option<Vec<u8>>, Vec<u8>, Vec<u8>)> = Vec::new();

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
                    let localname = attr_name[colons + 1..].to_vec();
                    regular_attrs.push((Some(prefix), localname, attr_value.clone()));
                } else {
                    regular_attrs.push((None, attr_name.clone(), attr_value.clone()));
                }
            }
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
                                "Opening and ending tag mismatch",
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
                        self.set_error(XML_ERR_DOCUMENT_END, "Unclosed element tag");
                        if !self.is_recovery() {
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
            // Substitute predefined entity
            self.sax_characters(replacement);

            // Dispatch reference SAX event
            if !self.is_sax_disabled() {
                let name_cstr = Self::vec_to_cstr_null(inner);
                unsafe {
                    let sax = &*(*self.ctxt).sax;
                    let ctx = (*self.ctxt).userData;
                    SaxDispatcher::reference(sax, ctx, name_cstr);
                }
            }
        } else if (self.options & XML_PARSE_NOENT) != 0 {
            // Entity substitution requested: resolve and expand the entity
            // For now, just dispatch the reference event
            if !self.is_sax_disabled() {
                let name_cstr = Self::vec_to_cstr_null(inner);
                unsafe {
                    let sax = &*(*self.ctxt).sax;
                    let ctx = (*self.ctxt).userData;
                    SaxDispatcher::reference(sax, ctx, name_cstr);
                }
            }

            // Try to resolve the entity via SAX callback
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
            } else {
                // Entity found — in a full implementation we'd expand the entity content
                // by pushing its content onto the input stack
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
                // URI is empty for non-namespaced attributes initially
                let uri_cstr = ptr::null();
                let value_cstr = Self::vec_to_cstr_null(value);
                attr_vec.push(local_cstr);
                attr_vec.push(prefix_cstr);
                attr_vec.push(uri_cstr);
                attr_vec.push(value_cstr);
                // valueEnd is the end of the value string — we pass null to indicate
                // the value is null-terminated
                attr_vec.push(ptr::null());
            }

            let localname = Self::vec_to_cstr_null(_name);
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
                localname,
                ptr::null(), // prefix
                ptr::null(), // URI
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
        unsafe {
            let sax = &*(*self.ctxt).sax;
            let ctx = (*self.ctxt).userData;
            let data_cstr = Self::vec_to_cstr_null(data);
            SaxDispatcher::cdata_block(sax, ctx, data_cstr, data.len() as c_int);
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

            // Build a C string for the error message
            let msg_cstr = std::ffi::CString::new(msg).unwrap_or_default();

            // Dispatch SAX error callback
            if !self.is_sax_disabled() {
                let sax = &*(*self.ctxt).sax;
                let ctx = (*self.ctxt).userData;
                SaxDispatcher::error(sax, ctx, msg_cstr.as_ptr());
            }

            // Also raise a structured error via the global error system
            crate::xml::errors::raise_error(
                self.ctxt as *mut c_void,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                XML_FROM_PARSER,
                code,
                xmlErrorLevel::XML_ERR_ERROR as c_int,
                ptr::null(),
                0,
                ptr::null(),
                ptr::null(),
                ptr::null(),
                0,
                0,
                msg_cstr.as_ptr(),
            );
        }
    }

    /// Set a warning on the parser context.
    fn set_warning(&mut self, msg: &str) {
        unsafe {
            (*self.ctxt).nbWarnings = (*self.ctxt).nbWarnings.wrapping_add(1);

            let msg_cstr = std::ffi::CString::new(msg).unwrap_or_default();

            if !self.is_sax_disabled() {
                let sax = &*(*self.ctxt).sax;
                let ctx = (*self.ctxt).userData;
                SaxDispatcher::warning(sax, ctx, msg_cstr.as_ptr());
            }

            crate::xml::errors::raise_error(
                self.ctxt as *mut c_void,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                XML_FROM_PARSER,
                0,
                xmlErrorLevel::XML_ERR_WARNING as c_int,
                ptr::null(),
                0,
                ptr::null(),
                ptr::null(),
                ptr::null(),
                0,
                0,
                msg_cstr.as_ptr(),
            );
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
